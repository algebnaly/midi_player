//! High-level playback controller.
//!
//! [`Player`] is the main façade that the UI interacts with.  It owns the
//! audio engine, sequencer, synth instances, and exposes simple verbs like
//! [`play`](Player::play), [`pause`](Player::pause), and
//! [`seek`](Player::seek).
//!
//! Internally it delegates to:
//! * [`AudioEngine`](crate::audio_engine::AudioEngine) – CPAL stream management.
//! * [`CustomSequencer`](crate::sequencer::CustomSequencer) – MIDI event scheduling.
//! * [`TrackSynth`](crate::synth::TrackSynth) – per-track audio rendering.

use crate::audio_engine::AudioEngine;
use crate::clap_host::ClapPluginWrapper;
use crate::midi::MidiData;
use crate::sequencer::CustomSequencer;
use crate::synth::TrackSynth;
use oxisynth::{SoundFont, Synth};
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Top-level playback controller.
///
/// Coordinates the sequencer, synth pool, and audio engine.  All public
/// methods are safe to call from the GTK main thread.
pub struct Player {
    /// The MIDI sequencer (shared with the audio callback).
    sequencer: Arc<Mutex<CustomSequencer>>,
    /// Collection of synthesizer tracks (shared with the audio callback).
    synths: Arc<Mutex<Vec<TrackSynth>>>,
    /// The underlying CPAL audio engine.
    _engine: AudioEngine,
    /// Whether playback is currently paused.
    paused: Arc<AtomicBool>,
    /// Set by the audio callback after it outputs a block while paused.
    /// Used by [`shutdown`](Self::shutdown) to confirm silence was flushed.
    silence_flushed: Arc<AtomicBool>,
    /// A snapshot of the currently loaded MIDI data (for hot-swap).
    current_midi: Arc<Mutex<Option<MidiData>>>,
    /// The sample rate negotiated with the audio device (Hz).
    #[allow(dead_code)]
    sample_rate: f64,
}

impl Player {
    /// Initialise the player, loading a SoundFont from `sf2_path`.
    ///
    /// A CLAP plugin is optionally loaded from `clap_path` if the file
    /// exists.  If neither a SoundFont nor a CLAP plugin can be loaded the
    /// player will still start, but no audio will be produced.
    pub fn new(sf2_path: &str, clap_path: &str) -> anyhow::Result<Self> {
        let font = Self::load_soundfont(sf2_path)?;

        let mut main_synth = Synth::default();
        // Sample rate will be set after audio engine negotiation, but we need
        // a reasonable default for the SoundFont synth.  The actual rate is
        // overwritten below.
        main_synth.set_sample_rate(44100.0);
        main_synth.add_font(font, true);

        let mut synths_vec = vec![TrackSynth::SoundFont(main_synth)];

        // Try loading a CLAP plugin.
        if let Ok(clap) = ClapPluginWrapper::new(clap_path, 44100) {
            println!("Loaded CLAP plugin successfully from {}", clap_path);
            synths_vec.push(TrackSynth::ClapPlugin(clap));
        } else {
            println!("No CLAP plugin loaded from {}", clap_path);
        }

        let synths = Arc::new(Mutex::new(synths_vec));
        let sequencer = Arc::new(Mutex::new(CustomSequencer::new()));

        // Preview synth (separate instance for live note auditioning).
        let preview_synth = {
            let font2 = Self::load_soundfont(sf2_path)?;
            let mut s = Synth::default();
            s.set_sample_rate(44100.0);
            s.add_font(font2, true);
            Arc::new(Mutex::new(s))
        };

        let paused = Arc::new(AtomicBool::new(false));
        let silence_flushed = Arc::new(AtomicBool::new(false));

        let engine = AudioEngine::new(
            sequencer.clone(),
            synths.clone(),
            preview_synth,
            paused.clone(),
            silence_flushed.clone(),
        )?;

        let sample_rate = engine.sample_rate;

        // Now that we know the real sample rate, update the SoundFont synths.
        if let Ok(mut s_vec) = synths.lock() {
            for synth in s_vec.iter_mut() {
                if let TrackSynth::SoundFont(s) = synth {
                    s.set_sample_rate(sample_rate as f32);
                }
            }
        }

        Ok(Self {
            sequencer,
            synths,
            _engine: engine,
            paused,
            silence_flushed,
            current_midi: Arc::new(Mutex::new(None)),
            sample_rate,
        })
    }

    // ------------------------------------------------------------------
    // Playback controls
    // ------------------------------------------------------------------

    /// Start playing the given MIDI data from the beginning.
    pub fn play(&self, data: MidiData) -> anyhow::Result<()> {
        *self.current_midi.lock().unwrap() = Some(data.clone());
        let mut seq = self.sequencer.lock().unwrap();
        seq.load(&data);
        self.paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Pause playback and silence all notes.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        if let Ok(mut s_vec) = self.synths.lock() {
            for synth in s_vec.iter_mut() {
                synth.all_notes_off();
            }
        }
    }

    /// Resume playback after a pause.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// Returns `true` if the player is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Stop playback, reset the sequencer, and silence all notes.
    #[allow(dead_code)]
    pub fn stop(&self) {
        let mut seq = self.sequencer.lock().unwrap();
        seq.reset();
        self.paused.store(true, Ordering::SeqCst);
        if let Ok(mut s_vec) = self.synths.lock() {
            for synth in s_vec.iter_mut() {
                synth.all_notes_off();
            }
        }
    }

    /// Gracefully shut down audio before the player is dropped.
    ///
    /// Silences all synths, pauses the sequencer, then waits for the audio
    /// callback to confirm it has output at least one block of silence —
    /// preventing the pop/click that occurs when CPAL drops a stream with
    /// non-zero samples in flight.
    pub fn shutdown(&self) {
        // Reset the ack flag before changing state.
        self.silence_flushed.store(false, Ordering::SeqCst);
        // Silence everything.
        if let Ok(mut s_vec) = self.synths.lock() {
            for synth in s_vec.iter_mut() {
                synth.all_notes_off();
            }
        }
        // Pause the sequencer so the callback only outputs silence.
        self.paused.store(true, Ordering::SeqCst);
        // Wait for the audio callback to confirm a silence block was output.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while !self.silence_flushed.load(Ordering::SeqCst) {
            if std::time::Instant::now() >= deadline {
                break; // Timeout safeguard — don't hang forever.
            }
            std::hint::spin_loop();
        }
    }

    /// Seek to an arbitrary time (in seconds).
    pub fn seek(&self, time: f64) {
        let mut seq = self.sequencer.lock().unwrap();
        let mut s_vec = self.synths.lock().unwrap();
        seq.seek(time, &mut s_vec);
    }

    /// Replace the current MIDI data and seek to `time` without interrupting
    /// playback.  Used for live editing (hot-swap on BPM change or note edit).
    pub fn hot_swap(&self, data: MidiData, time: f64) -> anyhow::Result<()> {
        let mut seq = self.sequencer.lock().unwrap();
        let mut s_vec = self.synths.lock().unwrap();
        seq.load(&data);
        seq.seek(time, &mut s_vec);
        *self.current_midi.lock().unwrap() = Some(data);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Returns the current playhead position in seconds.
    pub fn get_time(&self) -> f64 {
        let seq = self.sequencer.lock().unwrap();
        seq.playhead_time
    }

    /// Returns `true` if the player is actively producing audio.
    ///
    /// With looping enabled (the default), this only checks whether playback
    /// is paused — the sequencer will automatically loop back so "end of
    /// sequence" doesn't mean "stopped".
    pub fn is_playing(&self) -> bool {
        let seq = self.sequencer.lock().unwrap();
        let has_content = !seq.events.is_empty() || seq.loop_end_time > 0.0;
        has_content && !self.is_paused()
    }

    /// Returns a list of human-readable names for each synth track.
    pub fn get_synth_names(&self) -> Vec<String> {
        let synths = self.synths.lock().unwrap();
        synths
            .iter()
            .enumerate()
            .map(|(i, s)| format!("Track {} ({})", i, s.backend_label()))
            .collect()
    }

    // ------------------------------------------------------------------
    // Note preview (live auditioning from piano roll)
    // ------------------------------------------------------------------

    /// Send a preview Note-On to the synth at `track_index`.
    pub fn preview_note_on(&self, track_index: usize, pitch: u8, velocity: u8) {
        if let Ok(mut synths) = self.synths.lock() {
            let idx = track_index % synths.len();
            if let Some(synth) = synths.get_mut(idx) {
                synth.send_midi_event(0, &crate::midi::MidiEventType::NoteOn { pitch, velocity });
            }
        }
    }

    /// Send a preview Note-Off to the synth at `track_index`.
    pub fn preview_note_off(&self, track_index: usize, pitch: u8) {
        if let Ok(mut synths) = self.synths.lock() {
            let idx = track_index % synths.len();
            if let Some(synth) = synths.get_mut(idx) {
                synth.send_midi_event(0, &crate::midi::MidiEventType::NoteOff { pitch });
            }
        }
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Try loading a SoundFont from a list of candidate paths.
    fn load_soundfont(preferred_path: &str) -> anyhow::Result<SoundFont> {
        let candidates = [
            preferred_path.to_string(),
            "default1.sf2".to_string(),
            "default2.sf2".to_string(),
            "GeneralUser GS.sf2".to_string(),
        ];

        let mut last_error = None;
        for path in &candidates {
            if let Ok(mut file) = File::open(path) {
                match SoundFont::load(&mut file) {
                    Ok(sf) => return Ok(sf),
                    Err(e) => {
                        last_error = Some(anyhow::anyhow!("SoundFont '{}' failed: {:?}", path, e));
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            anyhow::anyhow!("No valid SoundFont found. Tried: {:?}", candidates)
        }))
    }
}
