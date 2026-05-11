//! Audio device discovery and CPAL stream construction.
//!
//! This module isolates all platform-specific audio I/O setup from the rest of
//! the application.  [`AudioEngine`] owns the active CPAL output stream and
//! exposes the negotiated sample rate so that the rest of the engine can
//! configure itself accordingly.

use crate::sequencer::CustomSequencer;
use crate::synth::TrackSynth;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oxisynth::Synth;
use std::sync::atomic::{AtomicBool, Ordering};
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
        paused: Arc<AtomicBool>,
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

                    if !is_paused {
                        // Playing: full sequencer (dispatches events + renders)
                        if let Ok(mut seq) = sequencer.try_lock() {
                            if let Ok(mut s_vec) = synths.try_lock() {
                                tmp_l[..frames].fill(0.0);
                                tmp_r[..frames].fill(0.0);
                                seq.render_block(&mut s_vec, tmp_l, tmp_r, sample_rate);
                                for i in 0..frames {
                                    out_l[i] += tmp_l[i];
                                    out_r[i] += tmp_r[i];
                                }
                            }
                        }
                    } else {
                        // Paused: render track synths directly for preview
                        if let Ok(mut s_vec) = synths.try_lock() {
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

                    // Interleave into the CPAL buffer.
                    for (i, frame) in data.chunks_mut(channels).enumerate() {
                        frame[0] = out_l[i];
                        if channels > 1 {
                            frame[1] = out_r[i];
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
