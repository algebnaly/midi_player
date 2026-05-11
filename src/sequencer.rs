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

/// Number of beats per bar (time signature numerator).
const BEATS_PER_BAR: u64 = 4;

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
}

impl CustomSequencer {
    /// Create an empty sequencer with the playhead at time zero.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            playhead_time: 0.0,
            current_event_idx: 0,
            loop_end_time: 0.0,
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
    }

    /// Reset the playhead to the beginning without changing the event list.
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.playhead_time = 0.0;
        self.current_event_idx = 0;
    }

    /// Seek to an arbitrary point in time.
    ///
    /// Silences all synths, advances `current_event_idx` past any events
    /// before `time`, then re-sends NoteOn for notes that should still be
    /// sounding at the new position (so that `hot_swap` doesn't cut off
    /// in-progress notes).
    pub fn seek(&mut self, time: f64, synths: &mut [TrackSynth]) {
        self.playhead_time = time;
        for synth in synths.iter_mut() {
            synth.all_notes_off();
        }

        // Scan events before `time` to find notes still active at the seek point.
        // Key: (track_index, channel, pitch) → velocity
        use std::collections::HashMap;
        let mut active: HashMap<(usize, u8, u8), u8> = HashMap::new();

        self.current_event_idx = 0;
        while self.current_event_idx < self.events.len()
            && self.events[self.current_event_idx].time_seconds < time
        {
            let ev = &self.events[self.current_event_idx];
            match &ev.event_type {
                crate::midi::MidiEventType::NoteOn { pitch, velocity } => {
                    active.insert((ev.track_index, ev.channel, *pitch), *velocity);
                }
                crate::midi::MidiEventType::NoteOff { pitch } => {
                    active.remove(&(ev.track_index, ev.channel, *pitch));
                }
            }
            self.current_event_idx += 1;
        }

        // Re-trigger notes that should still be sounding.
        for ((track_idx, channel, pitch), velocity) in &active {
            let synth_idx = track_idx % synths.len();
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
                let mut track_left = vec![0.0f32; chunk_frames];
                let mut track_right = vec![0.0f32; chunk_frames];

                for synth in synths.iter_mut() {
                    track_left.fill(0.0);
                    track_right.fill(0.0);
                    synth.render(&mut track_left, &mut track_right);

                    for i in 0..chunk_frames {
                        left[frames_rendered + i] += track_left[i];
                        right[frames_rendered + i] += track_right[i];
                    }
                }

                frames_rendered += chunk_frames;
                self.playhead_time += chunk_frames as f64 / sample_rate;
            }

            // Dispatch any events whose time has been reached.
            while self.current_event_idx < self.events.len() {
                let ev = &self.events[self.current_event_idx];
                if ev.time_seconds <= self.playhead_time + 0.0001 {
                    let synth_idx = ev.track_index % synths.len();
                    if let Some(synth) = synths.get_mut(synth_idx) {
                        synth.send_midi_event(ev.channel, &ev.event_type);
                    }
                    self.current_event_idx += 1;
                } else {
                    break;
                }
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
    /// A "bar" is `BEATS_PER_BAR * SNAP_SUBDIVISIONS` minimal grid units,
    /// i.e. `BEATS_PER_BAR * ticks_per_beat` ticks.
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
