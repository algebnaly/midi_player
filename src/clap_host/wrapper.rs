//! CLAP plugin wrapper – manages the full lifecycle of a single plugin instance.
//!
//! [`ClapPluginWrapper`] encapsulates:
//! * Loading a `.clap` bundle from disk.
//! * Scanning the bundle for an *instrument* plugin descriptor.
//! * Activating the plugin with an appropriate audio configuration.
//! * Queuing MIDI note events and rendering audio blocks.
//!
//! [`ClapPluginGuiHandle`] holds the main-thread `PluginInstance` for GUI
//! operations.  It is `!Send` and must remain on the GTK main thread.

use crate::clap_audio::buffers::HostAudioBuffers;
use crate::clap_audio::config::FullAudioConfig;
use clack_extensions::gui::{GuiApiType, GuiConfiguration, PluginGui, Window};
use clack_host::prelude::*;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::host::{MidiPlayerHost, MidiPlayerHostMainThread, MidiPlayerHostShared, host_info};

/// Wraps a fully activated CLAP plugin, ready to process audio.
///
/// This struct lives behind `Arc<Mutex<>>` and is shared with the audio thread.
/// It only holds the audio-processing handle; for GUI use [`ClapPluginGuiHandle`].
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
    /// Current tempo in BPM, passed to the plugin via transport events.
    tempo: f64,
    /// Whether the host transport is currently playing.
    playing: bool,
    /// Tracks which notes are currently on: `active_notes[ch][key]`.
    /// Used by `send_all_notes_off` to send NoteOff only for active notes.
    active_notes: [[bool; 128]; 16],
    /// Pre-allocated interleave buffer, avoids heap allocation in the
    /// real-time audio callback.  Grown on demand (should only happen once).
    mux_buffer: Vec<f32>,
}

/// Main-thread handle that retains the `PluginInstance` for GUI operations.
///
/// This type is `!Send` — it must live on the GTK main thread alongside the
/// `GtkWindow` it references for transient window management.
pub struct ClapPluginGuiHandle {
    /// The plugin instance.  Kept alive for its `plugin_handle()` method.
    instance: PluginInstance<MidiPlayerHost>,
    /// Cached PluginGui extension, if the plugin supports it.
    gui_extension: Option<PluginGui>,
    /// Whether the GUI is currently open.
    gui_open: bool,
    /// X11 container window ID (for embedded mode).  `None` if using floating.
    container_window: Option<u32>,
    /// X11 connection kept alive while the container window exists.
    x11_conn: Option<x11rb::rust_connection::RustConnection>,
    /// Shared callback flag — set by the plugin (via host), polled here.
    callback_flag: Arc<AtomicBool>,
}

impl ClapPluginWrapper {
    /// Load a `.clap` bundle, find the first instrument plugin, activate it,
    /// and return both the audio wrapper and a main-thread GUI handle.
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
    pub fn new(
        plugin_path: &str,
        sample_rate: u32,
    ) -> Result<(Self, ClapPluginGuiHandle), Box<dyn Error>> {
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

        let callback_flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&callback_flag);

        let mut instance = PluginInstance::<MidiPlayerHost>::new(
            |_| MidiPlayerHostShared::with_shared_flag(flag_clone),
            |shared| MidiPlayerHostMainThread::new(shared),
            bundle,
            &plugin_id,
            &host_info(),
        )?;

        // Probe for GUI extension before activation.
        let gui_extension = instance.plugin_shared_handle().get_extension::<PluginGui>();

        let config = Self::build_audio_config(sample_rate);

        let audio_processor = instance
            .activate(|_, _| (), config.as_clack_plugin_config())?
            .start_processing()?;

        let wrapper = Self {
            audio_processor,
            buffers: HostAudioBuffers::from_config(config),
            steady_counter: 0,
            pending_events: EventBuffer::new(),
            tempo: 120.0,
            playing: true,
            active_notes: [[false; 128]; 16],
            mux_buffer: vec![0.0f32; 4096],
        };

        let gui_handle = ClapPluginGuiHandle {
            instance,
            gui_extension,
            gui_open: false,
            container_window: None,
            x11_conn: None,
            callback_flag,
        };

        Ok((wrapper, gui_handle))
    }

    /// Queue a MIDI Note-On event to be delivered on the next render call.
    pub fn send_note_on(&mut self, channel: u8, key: u8, velocity: u8) {
        // Send CLAP dialect NoteOn
        use clack_host::events::event_types::NoteOnEvent;
        use clack_host::events::{Match, Pckn};
        let pckn = Pckn::new(Match::All, channel as u16, key as u16, Match::All);
        let event = NoteOnEvent::new(0, pckn, velocity as f64 / 127.0);
        self.pending_events.push(&event);

        // Also send raw MIDI NoteOn (some plugins only respond to MIDI dialect)
        use clack_host::events::event_types::MidiEvent;
        let midi = MidiEvent::new(0, 0, [0x90 | (channel & 0x0F), key, velocity]);
        self.pending_events.push(&midi);

        self.active_notes[channel as usize & 0x0F][key as usize & 0x7F] = true;
    }

    /// Queue a MIDI Note-Off event to be delivered on the next render call.
    pub fn send_note_off(&mut self, channel: u8, key: u8) {
        // Send CLAP dialect NoteOff
        use clack_host::events::event_types::NoteOffEvent;
        use clack_host::events::{Match, Pckn};
        let pckn = Pckn::new(Match::All, channel as u16, key as u16, Match::All);
        let event = NoteOffEvent::new(0, pckn, 0.0);
        self.pending_events.push(&event);

        // Also send raw MIDI NoteOff
        use clack_host::events::event_types::MidiEvent;
        let midi = MidiEvent::new(0, 0, [0x80 | (channel & 0x0F), key, 0]);
        self.pending_events.push(&midi);

        self.active_notes[channel as usize & 0x0F][key as usize & 0x7F] = false;
    }

    /// Send NoteOff only for notes that are currently active.
    ///
    /// Typically only a handful of notes are sounding at any time, so this
    /// produces far fewer events than blasting 128×16 NoteOffs.
    pub fn send_all_notes_off(&mut self) {
        for ch in 0..16u8 {
            for key in 0..128u8 {
                if self.active_notes[ch as usize][key as usize] {
                    self.send_note_off(ch, key);
                }
            }
        }
    }

    /// Update the transport tempo (BPM) reported to the plugin.
    pub fn set_tempo(&mut self, bpm: f64) {
        self.tempo = bpm;
    }

    /// Update the transport playing state reported to the plugin.
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
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

        // Build a transport event reflecting current host state.
        // Some plugins (e.g. Actuate) require valid transport info to
        // produce any audio at all.
        use clack_host::events::event_types::{TransportEvent, TransportFlags};
        use clack_host::utils::{BeatTime, SecondsTime};

        let mut flags = TransportFlags::HAS_TEMPO | TransportFlags::HAS_TIME_SIGNATURE;
        if self.playing {
            flags |= TransportFlags::IS_PLAYING;
        }

        let transport = TransportEvent {
            header: Default::default(),
            flags,
            song_pos_beats: BeatTime::from_float(0.0),
            song_pos_seconds: SecondsTime::from_float(0.0),
            tempo: self.tempo,
            tempo_inc: 0.0,
            loop_start_beats: BeatTime::from_float(0.0),
            loop_end_beats: BeatTime::from_float(0.0),
            loop_start_seconds: SecondsTime::from_float(0.0),
            loop_end_seconds: SecondsTime::from_float(0.0),
            bar_start: BeatTime::from_float(0.0),
            bar_number: 0,
            time_signature_numerator: 4,
            time_signature_denominator: 4,
        };

        let _result = self.audio_processor.process(
            &ins,
            &mut outs,
            &input_events,
            &mut output_events,
            Some(self.steady_counter),
            Some(&transport),
        );

        self.pending_events.clear();
        self.steady_counter += frame_count as u64;

        // Copy plugin output into the caller's left/right buffers (additive).
        // Grow the pre-allocated mux buffer if needed (rare, typically once).
        let mux_len = frame_count * 2;
        if self.mux_buffer.len() < mux_len {
            self.mux_buffer.resize(mux_len, 0.0);
        }
        let muxed = &mut self.mux_buffer[..mux_len];
        self.buffers.write_to_cpal_buffer(muxed);

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

// =========================================================================
// GUI handle (main-thread only)
// =========================================================================

impl ClapPluginGuiHandle {
    /// Whether this plugin supports the CLAP GUI extension.
    pub fn supports_gui(&self) -> bool {
        self.gui_extension.is_some()
    }

    /// Whether the plugin GUI is currently shown.
    pub fn is_gui_open(&self) -> bool {
        self.gui_open
    }

    /// Check if the plugin has requested a main-thread callback and, if so,
    /// call `on_main_thread()`.  This must be called periodically from the
    /// GTK main loop (e.g. via a timer) to keep the plugin's internal state
    /// synchronised between the GUI and audio threads.
    pub fn poll_callbacks(&mut self) {
        if self.callback_flag.swap(false, Ordering::SeqCst) {
            self.instance.call_on_main_thread_callback();
        }
    }

    /// Open the plugin's GUI.
    ///
    /// Negotiates the windowing API with the plugin in priority order:
    ///
    /// 1. **X11 floating** – simplest for host, works on X11 and XWayland.
    /// 2. **Wayland floating** – native Wayland (no embedding per CLAP spec).
    /// 3. **X11 embedded** – host creates an X11 top-level container window.
    ///
    /// * `parent_xid` – optional X11 window ID for `set_transient` (X11 only).
    pub fn open_gui(&mut self, parent_xid: Option<u64>) -> Result<(), Box<dyn Error>> {
        let gui = self
            .gui_extension
            .ok_or("Plugin does not support GUI extension")?;

        let mut plugin_handle = self.instance.plugin_handle();

        // Negotiate the API.  Try multiple configurations in priority order.
        let api_configs = [
            // Prefer X11 floating (works on X11 and XWayland under Wayland).
            GuiConfiguration {
                api_type: GuiApiType::X11,
                is_floating: true,
            },
            // Native Wayland floating (no embedding per CLAP spec).
            GuiConfiguration {
                api_type: GuiApiType::WAYLAND,
                is_floating: true,
            },
            // Fallback: X11 embedded — we create a top-level X11 window ourselves.
            GuiConfiguration {
                api_type: GuiApiType::X11,
                is_floating: false,
            },
        ];

        let config = api_configs
            .iter()
            .find(|c| gui.is_api_supported(&mut plugin_handle, **c))
            .ok_or("Plugin does not support any available GUI API")?;

        let is_floating = config.is_floating;
        let is_x11 = config.api_type == GuiApiType::X11;
        gui.create(&mut plugin_handle, *config)?;

        if is_floating {
            // Floating mode: set transient so window stays above DAW.
            // set_transient only works on X11 (Wayland has no concept of
            // cross-process window stacking).
            if is_x11 {
                if let Some(xid) = parent_xid {
                    let window = Window::from_x11_handle(xid as std::ffi::c_ulong);
                    // SAFETY: the GTK window outlives the plugin GUI.
                    let _ = unsafe { gui.set_transient(&mut plugin_handle, window) };
                }
            }
            gui.suggest_title(&mut plugin_handle, c"CLAP Plugin");
        } else {
            // Embedded mode (X11 only): query the plugin's preferred size,
            // create a top-level X11 window, and pass it as the parent.
            let size = gui
                .get_size(&mut plugin_handle)
                .unwrap_or(clack_extensions::gui::GuiSize {
                    width: 800,
                    height: 600,
                });

            // Drop plugin_handle to release the borrow on self.instance,
            // so we can call create_x11_container.
            drop(plugin_handle);

            let (conn, container_xid) = create_x11_container(size.width, size.height)?;
            self.container_window = Some(container_xid);
            self.x11_conn = Some(conn);

            // Re-borrow for set_parent.
            let mut plugin_handle = self.instance.plugin_handle();
            let parent_window = Window::from_x11_handle(container_xid as std::ffi::c_ulong);
            // SAFETY: the container window is owned by us and lives until close_gui.
            unsafe { gui.set_parent(&mut plugin_handle, parent_window)? };
        }

        // show() may fail for embedded plugins (e.g. nih-plug returns false)
        // even though the GUI is already visible in our X11 container.
        // Treat this as non-fatal for non-floating mode.
        if let Err(e) = gui.show(&mut self.instance.plugin_handle()) {
            if is_floating {
                return Err(Box::new(e));
            }
            eprintln!(
                "[CLAP GUI] show() returned error in embedded mode (non-fatal): {:?}",
                e
            );
        }

        self.gui_open = true;
        Ok(())
    }

    /// Close the plugin's GUI and free its resources.
    pub fn close_gui(&mut self) {
        if !self.gui_open {
            return;
        }
        if let Some(gui) = self.gui_extension {
            let mut plugin_handle = self.instance.plugin_handle();
            let _ = gui.hide(&mut plugin_handle);
            gui.destroy(&mut plugin_handle);
        }
        // Destroy the X11 container window if we created one.
        if let (Some(conn), Some(win)) = (&self.x11_conn, self.container_window.take()) {
            use x11rb::connection::Connection as _;
            use x11rb::protocol::xproto::ConnectionExt as _;
            let _ = conn.destroy_window(win);
            let _ = conn.flush();
        }
        self.x11_conn = None;
        self.gui_open = false;
    }
}

/// Create a bare X11 top-level window to serve as the parent container
/// for an embedded plugin GUI.  Returns `(connection, window_id)`.
fn create_x11_container(
    width: u32,
    height: u32,
) -> Result<(x11rb::rust_connection::RustConnection, u32), Box<dyn Error>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::wrapper::ConnectionExt as _;

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];

    let win_id = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        win_id,
        screen.root,
        100, // x
        100, // y
        width as u16,
        height as u16,
        0, // border
        WindowClass::INPUT_OUTPUT,
        0, // visual (copy from parent)
        &CreateWindowAux::new()
            .event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::EXPOSURE)
            .background_pixel(screen.black_pixel),
    )?;

    // Set the window title.
    conn.change_property8(
        PropMode::REPLACE,
        win_id,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"CLAP Plugin",
    )?;

    conn.map_window(win_id)?;
    conn.flush()?;

    Ok((conn, win_id))
}

impl Drop for ClapPluginGuiHandle {
    fn drop(&mut self) {
        self.close_gui();
    }
}
