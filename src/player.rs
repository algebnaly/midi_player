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
use crate::midi::{MidiData, TrackId};
use crate::midi_input::{LiveMidiEvent, LiveNoteKey};
use crate::sequencer::CustomSequencer;
use crate::synth::TrackSynth;
use crossbeam_channel::{Sender, unbounded};
use oxisynth::{SoundFont, Synth};
use std::collections::HashMap;
use std::fs::File;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
    /// Master output gain, stored as raw `f32` bits for lock-free audio access.
    global_gain: Arc<AtomicU32>,
    /// Set by the audio callback after it outputs a block while paused.
    /// Used by [`shutdown`](Self::shutdown) to confirm silence was flushed.
    silence_flushed: Arc<AtomicBool>,
    /// A snapshot of the currently loaded MIDI data (for hot-swap).
    current_midi: Arc<Mutex<Option<MidiData>>>,
    /// Producer endpoint used by physical MIDI input callbacks.
    live_midi_tx: Sender<LiveMidiEvent>,
    /// Live notes currently owned by the physical input, updated by audio.
    live_notes: Arc<Mutex<HashMap<LiveNoteKey, (usize, u8)>>>,
    /// Main-thread GUI handles for CLAP plugins.  Indexed by track.
    /// `None` for SoundFont tracks.
    clap_gui_handles: Vec<Option<ClapPluginGuiHandle>>,
    /// The sample rate negotiated with the audio device (Hz).
    #[allow(dead_code)]
    sample_rate: f64,
    /// Synth index for drum tracks (1 if a dedicated drum SF2 was loaded, else 0).
    drum_synth_index: usize,
    /// Synth index for the loaded SFZ file (if any, else 0).
    pub sfz_synth_index: usize,
    /// Mapping of synth sources to their indices in the `synths` vector.
    pub loaded_synths: std::collections::HashMap<crate::midi::SynthSource, usize>,
}

impl Player {
    /// Initialise the player, loading a SoundFont from `sf2_path`.
    ///
    /// If `drum_sf2_path` is non-empty and points to a valid SoundFont it is
    /// loaded as a dedicated drum synth (synth index 1).  Otherwise drum tracks
    /// fall back to the main SoundFont.
    ///
    /// A CLAP plugin is optionally loaded from `clap_path` if the file
    /// exists.  If neither a SoundFont nor a CLAP plugin can be loaded the
    /// player will still start, but no audio will be produced.
    pub fn new(sf2_path: &str, drum_sf2_path: &str, clap_path: &str, sfz_path: &str, global_gain: f32) -> anyhow::Result<Self> {
        let font = Self::load_soundfont(sf2_path)?;

        let mut main_synth = Synth::default();
        // Use unity gain inside each backend; the post-mix master gain is the
        // single source of truth for application volume.
        main_synth.set_gain(1.0);
        // Sample rate will be set after audio engine negotiation, but we need
        // a reasonable default for the SoundFont synth.  The actual rate is
        // overwritten below.
        main_synth.set_sample_rate(44100.0);
        main_synth.add_font(font, true);

        let mut synths_vec = vec![TrackSynth::SoundFont(main_synth)];
        let mut gui_handles: Vec<Option<ClapPluginGuiHandle>> = vec![None]; // SoundFont track

        // Track which synth index drum tracks should use.
        let drum_synth_index = if !drum_sf2_path.is_empty() {
            match Self::load_soundfont(drum_sf2_path) {
                Ok(drum_font) => {
                    let mut drum_synth = Synth::default();
                    drum_synth.set_gain(1.0);
                    drum_synth.set_sample_rate(44100.0);
                    drum_synth.add_font(drum_font, true);
                    println!("Loaded drum SoundFont: {}", drum_sf2_path);
                    synths_vec.push(TrackSynth::SoundFont(drum_synth));
                    gui_handles.push(None);
                    synths_vec.len() - 1
                }
                Err(e) => {
                    eprintln!("Failed to load drum SoundFont {}: {}. Drums will use main SF.", drum_sf2_path, e);
                    0
                }
            }
        } else {
            0
        };

        // Try loading a CLAP plugin.
        if let Ok((clap_wrapper, gui_handle)) = ClapPluginWrapper::new(clap_path, 44100) {
            println!("Loaded CLAP plugin successfully from {}", clap_path);
            synths_vec.push(TrackSynth::ClapPlugin(clap_wrapper));
            gui_handles.push(Some(gui_handle));
        } else {
            println!("No CLAP plugin loaded from {}", clap_path);
        }

        // Try loading an SFZ file.
        let mut sfz_synth_index = 0;
        if !sfz_path.is_empty() {
            match sfizz_rs::Sfizz::new() {
                Ok(mut sfizz) => {
                    sfizz.set_sample_rate(44100.0);
                    sfizz.set_samples_per_block(512); // Will be updated by audio engine later
                    if sfizz.load_file(sfz_path) {
                        println!("Loaded SFZ successfully from {}", sfz_path);
                        synths_vec.push(TrackSynth::Sfz(sfizz));
                        gui_handles.push(None);
                        sfz_synth_index = synths_vec.len() - 1;
                    } else {
                        eprintln!("Failed to load SFZ file {}", sfz_path);
                    }
                }
                Err(e) => eprintln!("Failed to initialize sfizz-rs: {}", e),
            }
        }

        let synths = Arc::new(Mutex::new(synths_vec));
        let sequencer = Arc::new(Mutex::new(CustomSequencer::new()));

        // Preview synth (separate instance for live note auditioning).
        let preview_synth = {
            let font2 = Self::load_soundfont(sf2_path)?;
            let mut s = Synth::default();
            s.set_gain(1.0);
            s.set_sample_rate(44100.0);
            s.add_font(font2, true);
            Arc::new(Mutex::new(s))
        };

        // No sequence is playing yet. Starting paused makes the first Play
        // follow the same data-sync-and-resume path as later playback, thereby
        // preserving a playhead position chosen before the first run.
        let paused = Arc::new(AtomicBool::new(true));
        let global_gain = Arc::new(AtomicU32::new(global_gain.clamp(0.0, 2.0).to_bits()));
        let silence_flushed = Arc::new(AtomicBool::new(false));
        let (live_midi_tx, live_midi_rx) = unbounded();
        let live_notes = Arc::new(Mutex::new(HashMap::new()));

        let engine = AudioEngine::new(
            sequencer.clone(),
            synths.clone(),
            preview_synth,
            live_midi_rx,
            live_notes.clone(),
            paused.clone(),
            global_gain.clone(),
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

        let mut loaded_synths = std::collections::HashMap::new();
        // Assume default SoundFont is at index 0.
        loaded_synths.insert(crate::midi::SynthSource::SoundFont { path: sf2_path.to_string() }, 0);
        if drum_synth_index != 0 {
            loaded_synths.insert(crate::midi::SynthSource::SoundFont { path: drum_sf2_path.to_string() }, drum_synth_index);
        }
        if sfz_synth_index != 0 {
            loaded_synths.insert(crate::midi::SynthSource::Sfz { path: sfz_path.to_string() }, sfz_synth_index);
        }
        // Assuming clap plugin is at index 2 if loaded, but we can't easily know its index without checking gui_handles. We can add it dynamically later.

        Ok(Self {
            sequencer,
            synths,
            _engine: engine,
            paused,
            global_gain,
            silence_flushed,
            current_midi: Arc::new(Mutex::new(None)),
            live_midi_tx,
            live_notes,
            clap_gui_handles: gui_handles,
            sample_rate: 44100.0,
            drum_synth_index,
            sfz_synth_index,
            loaded_synths,
        })
    }

    /// Returns the synth index that drum tracks should use.
    pub fn add_or_get_synth(&mut self, source: &crate::midi::SynthSource) -> anyhow::Result<usize> {
        if let Some(&index) = self.loaded_synths.get(source) {
            return Ok(index);
        }

        let (new_synth, gui_handle) = match source {
            crate::midi::SynthSource::SoundFont { path } => {
                let font = Self::load_soundfont(path)?;
                let mut synth = oxisynth::Synth::default();
                synth.set_gain(1.0);
                synth.set_sample_rate(self.sample_rate as f32);
                synth.add_font(font, true);
                (crate::synth::TrackSynth::SoundFont(synth), None)
            }
            crate::midi::SynthSource::Sfz { path } => {
                let mut sfizz = sfizz_rs::Sfizz::new().map_err(|e| anyhow::anyhow!("{}", e))?;
                sfizz.set_sample_rate(self.sample_rate as f32);
                sfizz.set_samples_per_block(512);
                if sfizz.load_file(path) {
                    (crate::synth::TrackSynth::Sfz(sfizz), None)
                } else {
                    return Err(anyhow::anyhow!("Failed to load SFZ: {}", path));
                }
            }
            crate::midi::SynthSource::ClapPlugin { path } => {
                let (wrapper, handle) = crate::clap_host::ClapPluginWrapper::new(path, self.sample_rate as u32)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                (crate::synth::TrackSynth::ClapPlugin(wrapper), Some(handle))
            }
        };

        let mut synths = self.synths.lock().unwrap();
        let index = synths.len();
        synths.push(new_synth);
        self.clap_gui_handles.push(gui_handle);
        self.loaded_synths.insert(source.clone(), index);

        Ok(index)
    }

    pub fn drum_synth_index(&self) -> usize {
        self.drum_synth_index
    }

    pub fn sfz_synth_index(&self) -> usize {
        self.sfz_synth_index
    }

    // ------------------------------------------------------------------
    // Playback controls
    // ------------------------------------------------------------------

    /// Start playing the given MIDI data from the beginning.
    pub fn play(&self, data: MidiData) -> anyhow::Result<()> {
        let bpm = data.get_bpm();
        *self.current_midi.lock().unwrap() = Some(data.clone());
        let mut seq = self.sequencer.lock().unwrap();
        let mut s_vec = self.synths.lock().unwrap();
        let live_notes = self.live_notes.lock().unwrap();
        seq.silence_sequence_notes(&mut s_vec, &live_notes);
        seq.load(&data);
        // Propagate BPM to CLAP plugins so their transport matches.
        for synth in s_vec.iter_mut() {
            synth.set_tempo(bpm);
        }
        self.paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Pause playback and silence all notes.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        if let (Ok(mut seq), Ok(mut s_vec), Ok(live_notes)) = (
            self.sequencer.lock(),
            self.synths.lock(),
            self.live_notes.lock(),
        ) {
            seq.silence_sequence_notes(&mut s_vec, &live_notes);
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

    /// Set the post-mix master gain used by every audio source.
    pub fn set_global_gain(&self, gain: f32) {
        self.global_gain
            .store(gain.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }

    /// Stop playback, reset the sequencer, and silence all notes.
    #[allow(dead_code)]
    pub fn stop(&self) {
        let mut seq = self.sequencer.lock().unwrap();
        let mut s_vec = self.synths.lock().unwrap();
        let live_notes = self.live_notes.lock().unwrap();
        seq.silence_sequence_notes(&mut s_vec, &live_notes);
        seq.reset();
        self.paused.store(true, Ordering::SeqCst);
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
        let live_notes = self.live_notes.lock().unwrap();
        seq.seek(time, &mut s_vec, &live_notes);
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
        let live_notes = self.live_notes.lock().unwrap();
        seq.load_for_hot_swap(&data);
        seq.seek(time, &mut s_vec, &live_notes);
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

    /// Snapshot the playhead and sounding pitches for the visible MIDI track.
    pub fn playback_snapshot(&self, track_id: TrackId) -> (f64, Vec<u8>) {
        let seq = self.sequencer.lock().unwrap();
        (seq.playhead_time, seq.active_pitches_for_track(track_id))
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

    /// Clone the producer endpoint used by a physical MIDI input connection.
    pub fn live_midi_sender(&self) -> Sender<LiveMidiEvent> {
        self.live_midi_tx.clone()
    }

    // ------------------------------------------------------------------
    // Note preview (live auditioning from piano roll)
    // ------------------------------------------------------------------

    /// Send a preview Note-On to the synth at `synth_index`.
    pub fn preview_note_on(&self, synth_index: usize, channel: u8, pitch: u8, velocity: u8) {
        if let Ok(mut synths) = self.synths.lock() {
            let idx = synth_index % synths.len();
            if let Some(synth) = synths.get_mut(idx) {
                synth.send_midi_event(channel, &crate::midi::MidiEventType::NoteOn { pitch, velocity });
            }
        }
    }

    /// Send a preview Note-Off to the synth at `synth_index`.
    pub fn preview_note_off(&self, synth_index: usize, channel: u8, pitch: u8) {
        if let Ok(mut synths) = self.synths.lock() {
            let idx = synth_index % synths.len();
            if let Some(synth) = synths.get_mut(idx) {
                synth.send_midi_event(channel, &crate::midi::MidiEventType::NoteOff { pitch });
            }
        }
    }

    // ------------------------------------------------------------------
    // Plugin GUI
    // ------------------------------------------------------------------

    /// Whether the given track has a CLAP plugin that supports GUI.
    #[allow(dead_code)]
    pub fn track_supports_gui(&self, track_index: usize) -> bool {
        self.clap_gui_handles
            .get(track_index)
            .is_some_and(|h| h.as_ref().is_some_and(|g| g.supports_gui()))
    }

    /// Open the CLAP plugin GUI for the given track.
    pub fn open_plugin_gui(&mut self, track_index: usize) {
        if let Some(Some(gui)) = self.clap_gui_handles.get_mut(track_index) {
            if let Err(e) = gui.open_gui() {
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
