//! MIDI event sequencer.
//!
//! [`CustomSequencer`] drives playback by walking a sorted list of
//! [`TimedEvent`]s and dispatching them to the appropriate
//! [`TrackSynth`](crate::synth::TrackSynth) at the correct sample-accurate
//! moment.
//!
//! The sequencer operates in a **block-based** fashion: the audio callback
//! hands it a pair of output buffers and a sample rate, and it advances the
//! playhead while rendering each sub-block between consecutive MIDI events.
//!
//! Looping is always enabled: the loop region spans from time 0 to the end of
//! the last bar that contains notes (rounded up to a full bar boundary).

use crate::midi::{MidiData, TimedEvent};
use crate::synth::TrackSynth;
use std::collections::HashMap;

/// Number of beats per bar (time signature numerator).
const BEATS_PER_BAR: u64 = 4;

/// Tolerance (in seconds) for dispatching events at the current playhead.
/// At 48 kHz this is ~4.8 samples.
const EVENT_DISPATCH_TOLERANCE_SECS: f64 = 0.0001;

/// Key identifying a single note voice: (track_index, channel, pitch).
type NoteKey = (usize, u8, u8);

/// Sample-accurate MIDI event sequencer with automatic looping.
///
/// Maintains a sorted event list and a playhead position.  On each
/// [`render_block`](Self::render_block) call it:
///
/// 1. Computes the number of silent frames until the next event (or loop point).
/// 2. Asks each [`TrackSynth`] to render that sub-block.
/// 3. Dispatches the event to the target synth.
/// 4. When the playhead reaches `loop_end_time`, silences all synths and
///    rewinds to time 0.
pub struct CustomSequencer {
    /// The full list of MIDI events, sorted by time.
    pub events: Vec<TimedEvent>,
    /// Current playhead position in seconds.
    pub playhead_time: f64,
    /// Index of the next event to be dispatched.
    pub current_event_idx: usize,
    /// End of the loop region in seconds (computed on [`load`](Self::load)).
    /// When the playhead reaches this point, playback loops back to 0.
    pub loop_end_time: f64,
    /// Pre-allocated per-track work buffers (avoids heap allocation in the
    /// real-time callback).  Grown on demand.
    track_buf_l: Vec<f32>,
    track_buf_r: Vec<f32>,
    /// Notes currently sounding, tracked so that `seek()` can diff against the
    /// new target state instead of blindly re-triggering everything.
    /// Key: (track_index, channel, pitch) → (synth_index, velocity).
    active_notes: HashMap<NoteKey, (usize, u8)>,
}

impl CustomSequencer {
    /// Create an empty sequencer with the playhead at time zero.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            playhead_time: 0.0,
            current_event_idx: 0,
            loop_end_time: 0.0,
            track_buf_l: vec![0.0f32; 4096],
            track_buf_r: vec![0.0f32; 4096],
            active_notes: HashMap::new(),
        }
    }

    /// Load (or replace) the event list from a [`MidiData`] instance.
    ///
    /// Resets the playhead and event index to the beginning, and computes the
    /// loop end time by rounding up the last note's end to the next bar.
    pub fn load(&mut self, data: &MidiData) {
        self.events = data.compile_events();
        self.current_event_idx = 0;
        self.playhead_time = 0.0;
        self.loop_end_time = Self::compute_loop_end(data);
        self.active_notes.clear();
    }

    /// Replace the event list without clearing `active_notes`.
    ///
    /// Used by hot-swap: the caller will immediately follow with
    /// [`seek()`](Self::seek) which diffs the old active set against the
    /// target set, avoiding audible re-triggers for notes that are still
    /// sustaining at the seek position.
    pub fn load_for_hot_swap(&mut self, data: &MidiData) {
        self.events = data.compile_events();
        self.current_event_idx = 0;
        self.playhead_time = 0.0;
        self.loop_end_time = Self::compute_loop_end(data);
        // Deliberately do NOT clear active_notes — seek() will diff.
    }

    /// Reset the playhead to the beginning without changing the event list.
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.playhead_time = 0.0;
        self.current_event_idx = 0;
        self.active_notes.clear();
    }

    /// Seek to an arbitrary point in time.
    ///
    /// Computes which notes should be active at the target `time` and applies
    /// a **diff** against the currently-tracked active notes.  Notes that were
    /// already sounding and should continue are left untouched (no re-trigger),
    /// notes that should stop receive NoteOff, and notes that should start
    /// receive NoteOn.  This avoids the audible re-attack that would result
    /// from a blanket `all_notes_off` + re-trigger cycle.
    pub fn seek(&mut self, time: f64, synths: &mut [TrackSynth]) {
        self.playhead_time = time;

        // Compute the set of notes that *should* be active at `time`.
        let mut target_active: HashMap<NoteKey, (usize, u8)> = HashMap::new();

        self.current_event_idx = 0;
        while self.current_event_idx < self.events.len()
            && self.events[self.current_event_idx].time_seconds < time
        {
            let ev = &self.events[self.current_event_idx];
            match &ev.event_type {
                crate::midi::MidiEventType::NoteOn { pitch, velocity } => {
                    target_active.insert(
                        (ev.track_index, ev.channel, *pitch),
                        (ev.synth_index, *velocity),
                    );
                }
                crate::midi::MidiEventType::NoteOff { pitch } => {
                    target_active.remove(&(ev.track_index, ev.channel, *pitch));
                }
            }
            self.current_event_idx += 1;
        }

        // Send NoteOff for notes that are currently active but should NOT be.
        for (key, (synth_index, _vel)) in &self.active_notes {
            if !target_active.contains_key(key) {
                let (_track_idx, channel, pitch) = key;
                let synth_idx = synth_index % synths.len();
                if let Some(synth) = synths.get_mut(synth_idx) {
                    synth.send_midi_event(
                        *channel,
                        &crate::midi::MidiEventType::NoteOff { pitch: *pitch },
                    );
                }
            }
        }

        // Send NoteOn for notes that should be active but are NOT currently.
        for (key, (synth_index, velocity)) in &target_active {
            if !self.active_notes.contains_key(key) {
                let (_track_idx, channel, pitch) = key;
                let synth_idx = synth_index % synths.len();
                if let Some(synth) = synths.get_mut(synth_idx) {
                    synth.send_midi_event(
                        *channel,
                        &crate::midi::MidiEventType::NoteOn {
                            pitch: *pitch,
                            velocity: *velocity,
                        },
                    );
                }
            }
        }

        // Update the tracked state to match.
        self.active_notes = target_active;
    }

    /// Render one block of audio into `left` / `right`, dispatching MIDI
    /// events to the appropriate synths at sample-accurate positions.
    ///
    /// Automatically loops back to the beginning when the playhead reaches
    /// [`loop_end_time`](Self::loop_end_time).
    pub fn render_block(
        &mut self,
        synths: &mut [TrackSynth],
        left: &mut [f32],
        right: &mut [f32],
        sample_rate: f64,
    ) {
        let frames_total = left.len().min(right.len());
        let mut frames_rendered = 0;

        while frames_rendered < frames_total {
            // Check for loop: if playhead is at or past the loop end, rewind.
            if self.loop_end_time > 0.0 && self.playhead_time >= self.loop_end_time {
                for synth in synths.iter_mut() {
                    synth.all_notes_off();
                }
                self.playhead_time = 0.0;
                self.current_event_idx = 0;
                self.active_notes.clear();
            }

            // Dispatch any events whose time has been reached (before rendering).
            while self.current_event_idx < self.events.len() {
                let ev = &self.events[self.current_event_idx];
                if ev.time_seconds <= self.playhead_time + EVENT_DISPATCH_TOLERANCE_SECS {
                    let synth_idx = ev.synth_index % synths.len();
                    if let Some(synth) = synths.get_mut(synth_idx) {
                        synth.send_midi_event(ev.channel, &ev.event_type);
                    }
                    // Update active_notes tracking.
                    let key: NoteKey = (
                        ev.track_index,
                        ev.channel,
                        match &ev.event_type {
                            crate::midi::MidiEventType::NoteOn { pitch, .. } => *pitch,
                            crate::midi::MidiEventType::NoteOff { pitch } => *pitch,
                        },
                    );
                    match &ev.event_type {
                        crate::midi::MidiEventType::NoteOn { velocity, .. } => {
                            self.active_notes.insert(key, (ev.synth_index, *velocity));
                        }
                        crate::midi::MidiEventType::NoteOff { .. } => {
                            self.active_notes.remove(&key);
                        }
                    }
                    self.current_event_idx += 1;
                } else {
                    break;
                }
            }

            // How many frames until the next event?
            let frames_to_next_event = if self.current_event_idx < self.events.len() {
                let event_time = self.events[self.current_event_idx].time_seconds;
                let dt = event_time - self.playhead_time;
                if dt <= 0.0 {
                    0
                } else {
                    (dt * sample_rate).round() as usize
                }
            } else {
                frames_total - frames_rendered
            };

            // Also limit chunk to the loop end so we don't overshoot.
            let frames_to_loop_end = if self.loop_end_time > 0.0 {
                let dt = self.loop_end_time - self.playhead_time;
                if dt <= 0.0 {
                    0
                } else {
                    (dt * sample_rate).ceil() as usize
                }
            } else {
                usize::MAX
            };

            let chunk_frames = frames_to_next_event
                .min(frames_total - frames_rendered)
                .min(frames_to_loop_end);

            if chunk_frames > 0 {
                let end_idx = frames_rendered + chunk_frames;

                // Clear this chunk's region in the master buffer.
                left[frames_rendered..end_idx].fill(0.0);
                right[frames_rendered..end_idx].fill(0.0);

                // Per-track render + additive mix.
                if self.track_buf_l.len() < chunk_frames {
                    self.track_buf_l.resize(chunk_frames, 0.0);
                    self.track_buf_r.resize(chunk_frames, 0.0);
                }
                let track_left = &mut self.track_buf_l[..chunk_frames];
                let track_right = &mut self.track_buf_r[..chunk_frames];

                for synth in synths.iter_mut() {
                    track_left.fill(0.0);
                    track_right.fill(0.0);
                    synth.render(track_left, track_right);

                    for i in 0..chunk_frames {
                        left[frames_rendered + i] += track_left[i];
                        right[frames_rendered + i] += track_right[i];
                    }
                }

                frames_rendered += chunk_frames;
                self.playhead_time += chunk_frames as f64 / sample_rate;
            }
        }
    }

    /// Returns `true` when all events have been dispatched and we're past the
    /// loop end (only meaningful if looping is disabled, which it currently
    /// is not).
    #[allow(dead_code)]
    pub fn is_end_of_sequence(&self) -> bool {
        self.current_event_idx >= self.events.len()
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Compute the loop end time: find the latest note end tick across all
    /// tracks, round it up to the next full bar boundary, and convert to
    /// seconds.
    ///
    /// A "bar" is `BEATS_PER_BAR * ticks_per_beat` ticks.
    fn compute_loop_end(data: &MidiData) -> f64 {
        let last_tick = data
            .tracks
            .iter()
            .flat_map(|t| t.notes.iter().map(|n| n.end_tick))
            .max()
            .unwrap_or(0);

        if last_tick == 0 {
            return 0.0;
        }

        // One bar = BEATS_PER_BAR beats = BEATS_PER_BAR * ticks_per_beat ticks
        let bar_ticks = BEATS_PER_BAR * data.ticks_per_beat as u64;
        // Round up to next bar boundary
        let loop_end_tick = ((last_tick + bar_ticks - 1) / bar_ticks) * bar_ticks;

        // Convert ticks to seconds
        let tps = data.ticks_per_beat as f64 * (data.get_bpm() / 60.0);
        if tps > 0.0 {
            loop_end_tick as f64 / tps
        } else {
            0.0
        }
    }
}
