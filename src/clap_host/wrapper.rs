//! CLAP plugin wrapper – manages the full lifecycle of a single plugin instance.
//!
//! [`ClapPluginWrapper`] encapsulates:
//! * Loading a `.clap` bundle from disk.
//! * Scanning the bundle for an *instrument* plugin descriptor.
//! * Activating the plugin with an appropriate audio configuration.
//! * Queuing MIDI note events and rendering audio blocks.

use crate::clap_audio::buffers::HostAudioBuffers;
use crate::clap_audio::config::FullAudioConfig;
use clack_host::prelude::*;
use std::error::Error;

use super::host::{MidiPlayerHost, MidiPlayerHostMainThread, MidiPlayerHostShared, host_info};

/// Wraps a fully activated CLAP plugin, ready to process audio.
///
/// # Usage
///
/// ```ignore
/// let mut wrapper = ClapPluginWrapper::new("./plugin.clap", 44100)?;
/// wrapper.send_note_on(0, 60, 100);
/// wrapper.render_block(&mut left_buf, &mut right_buf);
/// ```
pub struct ClapPluginWrapper {
    /// The started audio processor handle.
    audio_processor: StartedPluginAudioProcessor<MidiPlayerHost>,
    /// Host-side audio buffers matching the plugin's port layout.
    buffers: HostAudioBuffers,
    /// Monotonically increasing sample counter used for the `steady_time`
    /// field in CLAP process calls.
    steady_counter: u64,
    /// Buffer for pending MIDI note events to be delivered to the plugin on the
    /// next [`render_block`](ClapPluginWrapper::render_block) call.
    pending_events: EventBuffer,
}

impl ClapPluginWrapper {
    /// Load a `.clap` bundle, find the first instrument plugin, activate it,
    /// and return a ready-to-use wrapper.
    ///
    /// # Arguments
    ///
    /// * `plugin_path` – filesystem path to the `.clap` shared library.
    /// * `sample_rate` – the sample rate of the host audio stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the bundle cannot be loaded, contains no suitable
    /// plugin, or if activation / start-processing fails.
    pub fn new(plugin_path: &str, sample_rate: u32) -> Result<Self, Box<dyn Error>> {
        // Load the bundle and leak it to obtain a 'static lifetime required by
        // clack-host.  This is intentional – a plugin stays loaded for the
        // entire lifetime of the application.
        let bundle = unsafe { PluginEntry::load(plugin_path)? };
        let bundle = Box::leak(Box::new(bundle));

        let factory = bundle
            .get_factory::<clack_host::factory::plugin::PluginFactory>()
            .ok_or("No plugin factory found in entry")?;

        // Scan for an instrument plugin; fall back to the first descriptor.
        let plugin_id = Self::find_instrument_plugin_id(&factory)?;
        println!("Selected plugin {}", plugin_id.to_string_lossy());

        let mut instance = PluginInstance::<MidiPlayerHost>::new(
            |_| MidiPlayerHostShared::new(),
            |shared| MidiPlayerHostMainThread::new(shared),
            bundle,
            &plugin_id,
            &host_info(),
        )?;

        let config = Self::build_audio_config(sample_rate);

        let audio_processor = instance
            .activate(|_, _| (), config.as_clack_plugin_config())?
            .start_processing()?;

        Ok(Self {
            audio_processor,
            buffers: HostAudioBuffers::from_config(config),
            steady_counter: 0,
            pending_events: EventBuffer::new(),
        })
    }

    /// Queue a MIDI Note-On event to be delivered on the next render call.
    pub fn send_note_on(&mut self, channel: u8, key: u8, velocity: u8) {
        use clack_host::events::event_types::NoteOnEvent;
        use clack_host::events::{Match, Pckn};
        let pckn = Pckn::new(Match::All, channel as u16, key as u16, Match::All);
        let event = NoteOnEvent::new(0, pckn, velocity as f64 / 127.0);
        self.pending_events.push(&event);
    }

    /// Queue a MIDI Note-Off event to be delivered on the next render call.
    pub fn send_note_off(&mut self, channel: u8, key: u8) {
        use clack_host::events::event_types::NoteOffEvent;
        use clack_host::events::{Match, Pckn};
        let pckn = Pckn::new(Match::All, channel as u16, key as u16, Match::All);
        let event = NoteOffEvent::new(0, pckn, 0.0);
        self.pending_events.push(&event);
    }

    /// Process one block of audio, adding the plugin's output into the provided
    /// left/right buffers (additive mixing).
    ///
    /// Any pending note events queued via [`send_note_on`](Self::send_note_on)
    /// / [`send_note_off`](Self::send_note_off) are delivered to the plugin at
    /// the start of this block and then cleared.
    pub fn render_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frame_count = left.len();
        self.buffers.ensure_buffer_size_matches(frame_count * 2); // 2 channels

        let (ins, mut outs) = self.buffers.prepare_plugin_buffers(frame_count * 2);

        let input_events = InputEvents::from_buffer(&self.pending_events);
        let mut output_events = OutputEvents::void();

        let _ = self.audio_processor.process(
            &ins,
            &mut outs,
            &input_events,
            &mut output_events,
            Some(self.steady_counter),
            None,
        );

        self.pending_events.clear();
        self.steady_counter += frame_count as u64;

        // Copy plugin output into the caller's left/right buffers (additive).
        let mut muxed = vec![0.0f32; frame_count * 2];
        self.buffers.write_to_cpal_buffer(&mut muxed);

        for i in 0..frame_count {
            left[i] += muxed[i * 2];
            right[i] += muxed[i * 2 + 1];
        }
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Scan the plugin factory for an instrument/synthesizer/sampler and return
    /// its plugin ID as an owned `CString`.  Falls back to the first plugin.
    fn find_instrument_plugin_id(
        factory: &clack_host::factory::plugin::PluginFactory<'_>,
    ) -> Result<std::ffi::CString, Box<dyn Error>> {
        let count = factory.plugin_count();
        for i in 0..count {
            if let Some(desc) = factory.plugin_descriptor(i) {
                for feature in desc.features() {
                    if let Ok(f) = feature.to_str() {
                        let lower = f.to_lowercase();
                        if lower.contains("instrument")
                            || lower.contains("synthesizer")
                            || lower.contains("sampler")
                        {
                            let id = desc.id().ok_or("Plugin has no ID")?;
                            return Ok(id.to_owned());
                        }
                    }
                }
            }
        }
        // Fallback: use the first descriptor.
        let desc = factory
            .plugin_descriptor(0)
            .ok_or("No plugin descriptors found in bundle")?;
        let id = desc.id().ok_or("Plugin has no ID")?;
        Ok(id.to_owned())
    }

    /// Build a [`FullAudioConfig`] suitable for a stereo-output instrument
    /// plugin with no audio inputs.
    fn build_audio_config(sample_rate: u32) -> FullAudioConfig {
        use crate::clap_audio::config::{
            AudioPortLayout, PluginAudioPortInfo, PluginAudioPortsConfig,
        };

        let output_config = PluginAudioPortsConfig {
            main_port_index: 0,
            ports: vec![PluginAudioPortInfo {
                _id: None,
                port_layout: AudioPortLayout::Stereo,
                name: "Main Out".into(),
            }],
        };
        let input_config = PluginAudioPortsConfig {
            main_port_index: 0,
            ports: vec![],
        };

        FullAudioConfig {
            plugin_input_port_config: input_config,
            plugin_output_port_config: output_config,
            output_channel_count: 2,
            min_buffer_size: 1,
            max_likely_buffer_size: 1024,
            sample_rate,
            sample_format: cpal::SampleFormat::F32,
        }
    }
}
