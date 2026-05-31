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
use crate::clap_host::{ClapPluginGuiHandle, ClapPluginWrapper};
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
    /// Main-thread GUI handles for CLAP plugins.  Indexed by track.
    /// `None` for SoundFont tracks.
    clap_gui_handles: Vec<Option<ClapPluginGuiHandle>>,
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
        let mut gui_handles: Vec<Option<ClapPluginGuiHandle>> = vec![None]; // SoundFont track

        // Try loading a CLAP plugin.
        if let Ok((clap_wrapper, gui_handle)) = ClapPluginWrapper::new(clap_path, 44100) {
            println!("Loaded CLAP plugin successfully from {}", clap_path);
            synths_vec.push(TrackSynth::ClapPlugin(clap_wrapper));
            gui_handles.push(Some(gui_handle));
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
            clap_gui_handles: gui_handles,
            sample_rate,
        })
    }

    // ------------------------------------------------------------------
    // Playback controls
    // ------------------------------------------------------------------

    /// Start playing the given MIDI data from the beginning.
    pub fn play(&self, data: MidiData) -> anyhow::Result<()> {
        let bpm = data.get_bpm();
        *self.current_midi.lock().unwrap() = Some(data.clone());
        let mut seq = self.sequencer.lock().unwrap();
        seq.load(&data);
        // Propagate BPM to CLAP plugins so their transport matches.
        if let Ok(mut s_vec) = self.synths.lock() {
            for synth in s_vec.iter_mut() {
                synth.set_tempo(bpm);
            }
        }
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
    ///
    /// Uses [`load_for_hot_swap`](CustomSequencer::load_for_hot_swap) so that
    /// the subsequent [`seek`](CustomSequencer::seek) can diff active notes
    /// instead of re-triggering everything.
    pub fn hot_swap(&self, data: MidiData, time: f64) -> anyhow::Result<()> {
        let bpm = data.get_bpm();
        let mut seq = self.sequencer.lock().unwrap();
        let mut s_vec = self.synths.lock().unwrap();
        seq.load_for_hot_swap(&data);
        seq.seek(time, &mut s_vec);
        // Propagate BPM to CLAP plugins.
        for synth in s_vec.iter_mut() {
            synth.set_tempo(bpm);
        }
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

    /// Send a preview Note-On to the synth at `synth_index`.
    pub fn preview_note_on(&self, synth_index: usize, pitch: u8, velocity: u8) {
        if let Ok(mut synths) = self.synths.lock() {
            let idx = synth_index % synths.len();
            if let Some(synth) = synths.get_mut(idx) {
                synth.send_midi_event(0, &crate::midi::MidiEventType::NoteOn { pitch, velocity });
            }
        }
    }

    /// Send a preview Note-Off to the synth at `synth_index`.
    pub fn preview_note_off(&self, synth_index: usize, pitch: u8) {
        if let Ok(mut synths) = self.synths.lock() {
            let idx = synth_index % synths.len();
            if let Some(synth) = synths.get_mut(idx) {
                synth.send_midi_event(0, &crate::midi::MidiEventType::NoteOff { pitch });
            }
        }
    }

    // ------------------------------------------------------------------
    // Plugin GUI
    // ------------------------------------------------------------------

    /// Whether the given track has a CLAP plugin that supports GUI.
    pub fn track_supports_gui(&self, track_index: usize) -> bool {
        self.clap_gui_handles
            .get(track_index)
            .is_some_and(|h| h.as_ref().is_some_and(|g| g.supports_gui()))
    }

    /// Open the CLAP plugin GUI for the given track.
    ///
    /// `parent_xid` is the X11 window ID of the GTK window (for transient
    /// linkage).  Pass `None` on Wayland or if unavailable.
    pub fn open_plugin_gui(&mut self, track_index: usize, parent_xid: Option<u64>) {
        if let Some(Some(gui)) = self.clap_gui_handles.get_mut(track_index) {
            if let Err(e) = gui.open_gui(parent_xid) {
                eprintln!("Failed to open plugin GUI: {}", e);
            }
        }
    }

    /// Close the CLAP plugin GUI for the given track.
    pub fn close_plugin_gui(&mut self, track_index: usize) {
        if let Some(Some(gui)) = self.clap_gui_handles.get_mut(track_index) {
            gui.close_gui();
        }
    }

    /// Whether the CLAP plugin GUI for the given track is currently open.
    pub fn is_plugin_gui_open(&self, track_index: usize) -> bool {
        self.clap_gui_handles
            .get(track_index)
            .is_some_and(|h| h.as_ref().is_some_and(|g| g.is_gui_open()))
    }

    /// The number of GUI handle slots (one per track).
    pub fn gui_handle_count(&self) -> usize {
        self.clap_gui_handles.len()
    }

    /// Poll all CLAP plugin instances for pending main-thread callbacks.
    ///
    /// Must be called periodically from the GTK main loop (e.g. every 16 ms)
    /// to keep plugin state synchronised.
    pub fn poll_plugin_callbacks(&mut self) {
        for handle in &mut self.clap_gui_handles {
            if let Some(gui) = handle {
                gui.poll_callbacks();
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
