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

use crate::midi::{MidiData, TimedEvent, TrackId};
use crate::midi_input::{LiveNoteKey, OutputNoteKey};
use crate::synth::TrackSynth;
use std::collections::{HashMap, HashSet};

/// Number of beats per bar (time signature numerator).
const BEATS_PER_BAR: u64 = 4;

/// Tolerance (in seconds) for dispatching events at the current playhead.
/// At 48 kHz this is ~4.8 samples.
const EVENT_DISPATCH_TOLERANCE_SECS: f64 = 0.0001;

/// Key identifying a single note voice: (stable track ID, channel, pitch).
type NoteKey = (TrackId, u8, u8);

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
    /// Control changes currently active (synth_index, channel, controller) → value.
    active_controllers: HashMap<(usize, u8, u8), u8>,
    /// Mixer settings per synth index: `(gain, pan_l, pan_r)`.
    synth_mixers: HashMap<usize, (f32, f32, f32)>,
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
            active_controllers: HashMap::new(),
            synth_mixers: HashMap::new(),
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
        self.active_controllers.clear();
        self.update_synth_mixers(data);
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
        self.update_synth_mixers(data);
    }

    fn update_synth_mixers(&mut self, data: &MidiData) {
        self.synth_mixers.clear();
        for track in &data.tracks {
            let gain = if track.mixer.volume_db <= -60.0 {
                0.0
            } else {
                10.0f32.powf(track.mixer.volume_db / 20.0)
            };
            let pan = track.mixer.pan.clamp(-1.0, 1.0);
            let pan_l = (1.0 - pan).min(1.0);
            let pan_r = (1.0 + pan).min(1.0);
            self.synth_mixers
                .insert(track.synth_index, (gain, pan_l, pan_r));
        }
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
    pub fn seek(
        &mut self,
        time: f64,
        synths: &mut [TrackSynth],
        live_notes: &HashMap<LiveNoteKey, (usize, u8)>,
    ) {
        self.playhead_time = time;

        // Compute the set of notes that *should* be active at `time`.
        let mut target_active: HashMap<NoteKey, (usize, u8)> = HashMap::new();
        let mut target_ccs: HashMap<(usize, u8, u8), u8> = HashMap::new();

        self.current_event_idx = 0;
        while self.current_event_idx < self.events.len()
            && self.events[self.current_event_idx].time_seconds < time
        {
            let ev = &self.events[self.current_event_idx];
            match &ev.event_type {
                crate::midi::MidiEventType::NoteOn { pitch, velocity } => {
                    target_active.insert(
                        (ev.track_id, ev.channel, *pitch),
                        (ev.synth_index, *velocity),
                    );
                }
                crate::midi::MidiEventType::NoteOff { pitch } => {
                    target_active.remove(&(ev.track_id, ev.channel, *pitch));
                }
                crate::midi::MidiEventType::ControlChange { controller, value } => {
                    target_ccs.insert((ev.synth_index, ev.channel, *controller), *value);
                }
            }
            self.current_event_idx += 1;
        }

        for (&(synth_index, channel, controller), _) in &self.active_controllers {
            if !target_ccs.contains_key(&(synth_index, channel, controller)) {
                send_to_synth(
                    synths,
                    synth_index,
                    channel,
                    &crate::midi::MidiEventType::ControlChange {
                        controller,
                        value: 0,
                    },
                );
            }
        }
        for (&(synth_index, channel, controller), &value) in &target_ccs {
            if self
                .active_controllers
                .get(&(synth_index, channel, controller))
                != Some(&value)
            {
                send_to_synth(
                    synths,
                    synth_index,
                    channel,
                    &crate::midi::MidiEventType::ControlChange { controller, value },
                );
            }
        }
        self.active_controllers = target_ccs;

        let old_outputs = output_note_set(&self.active_notes);
        let target_outputs = output_note_set(&target_active);

        // Only release an output voice if neither the target sequence state nor
        // the live MIDI keyboard still owns it.
        for &(synth_index, channel, pitch) in old_outputs.difference(&target_outputs) {
            if !live_owns_output(live_notes, (synth_index, channel, pitch)) {
                send_to_synth(
                    synths,
                    synth_index,
                    channel,
                    &crate::midi::MidiEventType::NoteOff { pitch },
                );
            }
        }

        // Likewise, do not re-trigger a voice already held by the live input.
        for &(synth_index, channel, pitch) in target_outputs.difference(&old_outputs) {
            if !live_owns_output(live_notes, (synth_index, channel, pitch)) {
                let velocity = target_active
                    .iter()
                    .find_map(|((_, ch, p), (synth, velocity))| {
                        (*synth == synth_index && *ch == channel && *p == pitch)
                            .then_some(*velocity)
                    })
                    .unwrap_or(100);
                send_to_synth(
                    synths,
                    synth_index,
                    channel,
                    &crate::midi::MidiEventType::NoteOn { pitch, velocity },
                );
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
        live_notes: &HashMap<LiveNoteKey, (usize, u8)>,
        left: &mut [f32],
        right: &mut [f32],
        sample_rate: f64,
    ) {
        let frames_total = left.len().min(right.len());
        let mut frames_rendered = 0;

        while frames_rendered < frames_total {
            // Check for loop: if playhead is at or past the loop end, rewind.
            if self.loop_end_time > 0.0 && self.playhead_time >= self.loop_end_time {
                self.silence_sequence_notes(synths, live_notes);
                self.playhead_time = 0.0;
                self.current_event_idx = 0;
            }

            // Dispatch any events whose time has been reached (before rendering).
            while self.current_event_idx < self.events.len() {
                let ev = &self.events[self.current_event_idx];
                if ev.time_seconds <= self.playhead_time + EVENT_DISPATCH_TOLERANCE_SECS {
                    match &ev.event_type {
                        crate::midi::MidiEventType::NoteOn { velocity, pitch } => {
                            let key: NoteKey = (ev.track_id, ev.channel, *pitch);
                            let output_key = (ev.synth_index, ev.channel, *pitch);
                            let already_sounding = self.is_note_active(output_key)
                                || live_owns_output(live_notes, output_key);
                            self.active_notes.insert(key, (ev.synth_index, *velocity));
                            if !already_sounding {
                                send_to_synth(synths, ev.synth_index, ev.channel, &ev.event_type);
                            }
                        }
                        crate::midi::MidiEventType::NoteOff { pitch } => {
                            let key: NoteKey = (ev.track_id, ev.channel, *pitch);
                            self.active_notes.remove(&key);
                            let output_key = (ev.synth_index, ev.channel, *pitch);
                            if !self.is_note_active(output_key)
                                && !live_owns_output(live_notes, output_key)
                            {
                                send_to_synth(synths, ev.synth_index, ev.channel, &ev.event_type);
                            }
                        }
                        crate::midi::MidiEventType::ControlChange { controller, value } => {
                            self.active_controllers
                                .insert((ev.synth_index, ev.channel, *controller), *value);
                            send_to_synth(synths, ev.synth_index, ev.channel, &ev.event_type);
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

                for (synth_idx, synth) in synths.iter_mut().enumerate() {
                    track_left.fill(0.0);
                    track_right.fill(0.0);
                    synth.render(track_left, track_right);

                    let (gain, pan_l, pan_r) = self
                        .synth_mixers
                        .get(&synth_idx)
                        .copied()
                        .unwrap_or((1.0, 1.0, 1.0));
                    let gain_l = gain * pan_l;
                    let gain_r = gain * pan_r;

                    for i in 0..chunk_frames {
                        left[frames_rendered + i] += track_left[i] * gain_l;
                        right[frames_rendered + i] += track_right[i] * gain_r;
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

    /// Return whether the sequencer currently owns this output voice.
    pub fn is_note_active(&self, output_key: OutputNoteKey) -> bool {
        self.active_notes
            .iter()
            .any(|((_, channel, pitch), (synth_index, _))| {
                (*synth_index, *channel, *pitch) == output_key
            })
    }

    /// Return the distinct pitches currently sounding on one MIDI track.
    pub fn active_pitches_for_track(&self, track_id: TrackId) -> Vec<u8> {
        let mut pitches: Vec<u8> = self
            .active_notes
            .keys()
            .filter_map(|(track, _, pitch)| (*track == track_id).then_some(*pitch))
            .collect();
        pitches.sort_unstable();
        pitches.dedup();
        pitches
    }

    /// Release sequence-owned voices while preserving notes held by live MIDI.
    pub fn silence_sequence_notes(
        &mut self,
        synths: &mut [TrackSynth],
        live_notes: &HashMap<LiveNoteKey, (usize, u8)>,
    ) {
        for (synth_index, channel, pitch) in output_note_set(&self.active_notes) {
            if !live_owns_output(live_notes, (synth_index, channel, pitch)) {
                send_to_synth(
                    synths,
                    synth_index,
                    channel,
                    &crate::midi::MidiEventType::NoteOff { pitch },
                );
            }
        }
        self.active_notes.clear();

        for (&(synth_index, channel, controller), &value) in &self.active_controllers {
            if controller == 64 && value > 0 {
                send_to_synth(
                    synths,
                    synth_index,
                    channel,
                    &crate::midi::MidiEventType::ControlChange {
                        controller: 64,
                        value: 0,
                    },
                );
            }
        }
        self.active_controllers
            .retain(|&(_, _, ctrl), _| ctrl != 64);
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

fn output_note_set(notes: &HashMap<NoteKey, (usize, u8)>) -> HashSet<OutputNoteKey> {
    notes
        .iter()
        .map(|((_, channel, pitch), (synth_index, _))| (*synth_index, *channel, *pitch))
        .collect()
}

fn live_owns_output(
    live_notes: &HashMap<LiveNoteKey, (usize, u8)>,
    output_key: OutputNoteKey,
) -> bool {
    live_notes
        .iter()
        .any(|((_, channel, pitch), (synth_index, _))| {
            (*synth_index, *channel, *pitch) == output_key
        })
}

fn send_to_synth(
    synths: &mut [TrackSynth],
    synth_index: usize,
    channel: u8,
    event: &crate::midi::MidiEventType,
) {
    if synths.is_empty() {
        return;
    }
    if let Some(synth) = synths.get_mut(synth_index % synths.len()) {
        synth.send_midi_event(channel, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_pitches_are_distinct_and_filtered_by_track() {
        let mut sequencer = CustomSequencer::new();
        sequencer.active_notes.insert((TrackId(10), 0, 60), (0, 90));
        sequencer.active_notes.insert((TrackId(10), 1, 60), (0, 80));
        sequencer
            .active_notes
            .insert((TrackId(10), 0, 64), (0, 100));
        sequencer
            .active_notes
            .insert((TrackId(20), 0, 67), (0, 100));

        assert_eq!(
            sequencer.active_pitches_for_track(TrackId(10)),
            vec![60, 64]
        );
        assert_eq!(sequencer.active_pitches_for_track(TrackId(20)), vec![67]);
        assert!(sequencer.active_pitches_for_track(TrackId(30)).is_empty());
    }

    #[test]
    fn synth_mixers_calculated_correctly_on_load() {
        let mut sequencer = CustomSequencer::new();
        let mut data = MidiData::new_empty(&["Piano".into(), "Bass".into()]);
        data.tracks[0].synth_index = 0;
        data.tracks[0].mixer.volume_db = -6.0;
        data.tracks[0].mixer.pan = -0.5;

        data.tracks[1].synth_index = 1;
        data.tracks[1].mixer.volume_db = 0.0;
        data.tracks[1].mixer.pan = 0.5;

        sequencer.load(&data);

        let m0 = sequencer.synth_mixers.get(&0).unwrap();
        // -6dB is approx 0.501187
        assert!((m0.0 - 0.5012).abs() < 0.01);
        // pan = -0.5 -> pan_l = 1.0, pan_r = 0.5
        assert_eq!(m0.1, 1.0);
        assert_eq!(m0.2, 0.5);

        let m1 = sequencer.synth_mixers.get(&1).unwrap();
        assert!((m1.0 - 1.0).abs() < 0.001);
        // pan = 0.5 -> pan_l = 0.5, pan_r = 1.0
        assert_eq!(m1.1, 0.5);
        assert_eq!(m1.2, 1.0);
    }

    #[test]
    fn seek_updates_active_pedal_and_silence_releases() {
        let mut sequencer = CustomSequencer::new();
        let mut data = MidiData::new_empty(&["Piano".into()]);
        data.tracks[0].synth_index = 0;
        // Pedal down at tick 240 (0.25s), pedal up at tick 960 (1.0s)
        data.tracks[0].set_pedal_interval(240, 960, 0);
        sequencer.load(&data);

        let live_notes = HashMap::new();
        let mut synths: Vec<TrackSynth> = Vec::new();

        // Seek to 0.1s (before pedal): pedal should NOT be active
        sequencer.seek(0.1, &mut synths, &live_notes);
        assert_eq!(sequencer.active_controllers.get(&(0, 0, 64)), None);

        // Seek to 0.5s (inside pedal): pedal SHOULD be active with value 127
        sequencer.seek(0.5, &mut synths, &live_notes);
        assert_eq!(sequencer.active_controllers.get(&(0, 0, 64)), Some(&127));

        // Seek past 1.0s (after pedal): pedal SHOULD be inactive (value 0)
        sequencer.seek(1.2, &mut synths, &live_notes);
        assert_eq!(sequencer.active_controllers.get(&(0, 0, 64)), Some(&0));

        // Seek back inside pedal
        sequencer.seek(0.5, &mut synths, &live_notes);
        assert_eq!(sequencer.active_controllers.get(&(0, 0, 64)), Some(&127));

        // Silence sequence notes: should clear active CC 64
        sequencer.silence_sequence_notes(&mut synths, &live_notes);
        assert_eq!(sequencer.active_controllers.get(&(0, 0, 64)), None);
    }
}
