//! Audio device discovery and CPAL stream construction.
//!
//! This module isolates all platform-specific audio I/O setup from the rest of
//! the application.  [`AudioEngine`] owns the active CPAL output stream and
//! exposes the negotiated sample rate so that the rest of the engine can
//! configure itself accordingly.

use crate::midi::MidiEventType;
use crate::midi_input::{LiveMidiEvent, LiveNoteKey};
use crate::sequencer::CustomSequencer;
use crate::synth::TrackSynth;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Receiver;
use oxisynth::Synth;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Holds the active CPAL output stream and provides the negotiated sample rate.
///
/// The stream's audio callback mixes output from the sequencer's synth tracks
/// and a dedicated preview synth, then writes the result to the hardware
/// buffer.
pub struct AudioEngine {
    /// The live CPAL output stream.  Dropping this stops audio output.
    pub _stream: cpal::Stream,
    /// The sample rate negotiated with the audio device (Hz).
    pub sample_rate: f64,
}

impl AudioEngine {
    /// Discover an output device, build an audio stream, and start playback.
    ///
    /// The stream's callback reads from `sequencer` / `synths` for sequenced
    /// playback and from `preview_synth` for live note previews.  Output is
    /// gated by `paused`.
    ///
    /// Device selection prefers PipeWire / PulseAudio sinks when available,
    /// falling back to the OS default.
    pub fn new(
        sequencer: Arc<Mutex<CustomSequencer>>,
        synths: Arc<Mutex<Vec<TrackSynth>>>,
        preview_synth: Arc<Mutex<Synth>>,
        live_midi_rx: Receiver<LiveMidiEvent>,
        live_notes: Arc<Mutex<HashMap<LiveNoteKey, u8>>>,
        paused: Arc<AtomicBool>,
        global_gain: Arc<AtomicU32>,
        silence_flushed: Arc<AtomicBool>,
    ) -> anyhow::Result<Self> {
        let host = cpal::default_host();

        // --- Device discovery ---
        let (device, config) = Self::select_output_device(&host)?;
        let sample_rate = config.sample_rate() as f64;
        let channels = config.channels() as usize;

        // Pre-allocate audio work buffers to avoid heap allocations in the
        // real-time callback.  Sized to a generous initial capacity; grown
        // on demand if the CPAL buffer is larger (should only happen once).
        let initial_cap = 4096usize;
        let mut buf_out_l = vec![0.0f32; initial_cap];
        let mut buf_out_r = vec![0.0f32; initial_cap];
        let mut buf_tmp_l = vec![0.0f32; initial_cap];
        let mut buf_tmp_r = vec![0.0f32; initial_cap];

        // --- Build the output stream ---
        let paused_clone = paused;
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels;

                    // Grow pre-allocated buffers if needed (rare).
                    if frames > buf_out_l.len() {
                        buf_out_l.resize(frames, 0.0);
                        buf_out_r.resize(frames, 0.0);
                        buf_tmp_l.resize(frames, 0.0);
                        buf_tmp_r.resize(frames, 0.0);
                    }

                    let out_l = &mut buf_out_l[..frames];
                    let out_r = &mut buf_out_r[..frames];
                    let tmp_l = &mut buf_tmp_l[..frames];
                    let tmp_r = &mut buf_tmp_r[..frames];

                    out_l.fill(0.0);
                    out_r.fill(0.0);

                    let is_paused = paused_clone.load(Ordering::SeqCst);

                    // Keep all live MIDI mutation on the audio thread. The
                    // sequencer lock is also needed while paused so ownership
                    // checks can prevent one source from cutting another off.
                    if let Ok(mut seq) = sequencer.try_lock()
                        && let Ok(mut s_vec) = synths.try_lock()
                        && let Ok(mut held_live_notes) = live_notes.try_lock()
                    {
                        drain_live_midi(&live_midi_rx, &mut held_live_notes, &seq, &mut s_vec);

                        if !is_paused {
                            // Playing: sequencer dispatch + rendering.
                            tmp_l[..frames].fill(0.0);
                            tmp_r[..frames].fill(0.0);
                            seq.render_block(
                                &mut s_vec,
                                &held_live_notes,
                                tmp_l,
                                tmp_r,
                                sample_rate,
                            );
                            for i in 0..frames {
                                out_l[i] += tmp_l[i];
                                out_r[i] += tmp_r[i];
                            }
                        } else {
                            // Paused: render synth tails and live input.
                            for synth in s_vec.iter_mut() {
                                tmp_l[..frames].fill(0.0);
                                tmp_r[..frames].fill(0.0);
                                synth.render(tmp_l, tmp_r);
                                for i in 0..frames {
                                    out_l[i] += tmp_l[i];
                                    out_r[i] += tmp_r[i];
                                }
                            }
                        }
                    }

                    // Standalone preview synth (always active)
                    if let Ok(mut p_synth) = preview_synth.try_lock() {
                        tmp_l[..frames].fill(0.0);
                        tmp_r[..frames].fill(0.0);
                        p_synth.write((&mut tmp_l[..frames], &mut tmp_r[..frames]));
                        for i in 0..frames {
                            out_l[i] += tmp_l[i];
                            out_r[i] += tmp_r[i];
                        }
                    }

                    // Apply the lock-free master gain after every source has
                    // been mixed, then interleave into the CPAL buffer.
                    let gain = f32::from_bits(global_gain.load(Ordering::Relaxed));
                    for (i, frame) in data.chunks_mut(channels).enumerate() {
                        frame[0] = out_l[i] * gain;
                        if channels > 1 {
                            frame[1] = out_r[i] * gain;
                        }
                    }

                    // Signal that a silence block was output (for shutdown).
                    if is_paused {
                        silence_flushed.store(true, Ordering::SeqCst);
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Unsupported sample format")),
        };

        stream.play()?;

        Ok(Self {
            _stream: stream,
            sample_rate,
        })
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Select the best output device, preferring PipeWire / PulseAudio.
    fn select_output_device(
        host: &cpal::Host,
    ) -> anyhow::Result<(cpal::Device, cpal::SupportedStreamConfig)> {
        // First pass: look for a pipewire or pulse device.
        if let Ok(devices) = host.output_devices() {
            for device in devices {
                #[allow(deprecated)]
                let name_result = device.name();
                if let Ok(name) = name_result {
                    if name.contains("pipewire") || name.contains("pulse") {
                        if let Ok(config) = device.default_output_config() {
                            return Ok((device, config));
                        }
                    }
                }
            }
        }

        // Fallback: OS default device.
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No output audio device available"))?;
        let config = device.default_output_config()?;
        Ok((device, config))
    }
}

fn drain_live_midi(
    receiver: &Receiver<LiveMidiEvent>,
    live_notes: &mut HashMap<LiveNoteKey, u8>,
    sequencer: &CustomSequencer,
    synths: &mut [TrackSynth],
) {
    for event in receiver.try_iter() {
        let key = event.key();
        match event {
            LiveMidiEvent::NoteOn {
                channel,
                pitch,
                velocity,
                ..
            } => {
                let already_sounding =
                    live_notes.contains_key(&key) || sequencer.is_note_active(key);
                live_notes.insert(key, velocity);
                if !already_sounding {
                    send_live_event(
                        synths,
                        key.0,
                        channel,
                        &MidiEventType::NoteOn { pitch, velocity },
                    );
                }
            }
            LiveMidiEvent::NoteOff { channel, pitch, .. } => {
                let was_live = live_notes.remove(&key).is_some();
                if was_live && !sequencer.is_note_active(key) {
                    send_live_event(synths, key.0, channel, &MidiEventType::NoteOff { pitch });
                }
            }
        }
    }
}

fn send_live_event(
    synths: &mut [TrackSynth],
    synth_index: usize,
    channel: u8,
    event: &MidiEventType,
) {
    if synths.is_empty() {
        return;
    }
    let synth_count = synths.len();
    if let Some(synth) = synths.get_mut(synth_index % synth_count) {
        synth.send_midi_event(channel, event);
    }
}
