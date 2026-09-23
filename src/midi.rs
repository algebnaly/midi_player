//! MIDI data model, file I/O, and event compilation.
//!
//! This module defines the core data structures for representing MIDI music:
//!
//! * [`Note`] – a single note with pitch, velocity, timing, and channel.
//! * [`TrackData`] – a named collection of notes (one per track).
//! * [`MidiData`] – the top-level container holding all tracks, tempo map,
//!   and ticks-per-beat resolution.
//!
//! [`MidiData`] can be loaded from a Standard MIDI File (`.mid`) via
//! [`MidiData::load`], created empty via [`MidiData::new_empty`], or exported
//! back to SMF via [`MidiData::export_to_file`].
//!
//! For playback the [`compile_events`](MidiData::compile_events) method
//! converts the note list into a sorted sequence of [`TimedEvent`]s
//! (NoteOn / NoteOff with absolute timestamps in seconds).

use anyhow::Result;
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::drum_map::DrumMap;

/// Stable identity of a MIDI track. Unlike its vector index, this value does
/// not change when tracks are reordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TrackId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub velocity: u8,
    pub start_tick: u64,
    pub end_tick: u64,
    pub channel: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackMixerSettings {
    pub mute: bool,
    pub solo: bool,
    pub volume_db: f32,
    pub pan: f32,
}

impl Default for TrackMixerSettings {
    fn default() -> Self {
        Self {
            mute: false,
            solo: false,
            volume_db: 0.0,
            pan: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TrackInputSettings {
    pub armed: bool,
    pub channel_filter: Option<u8>,
    pub transpose: i8,
}

/// Describes which synthesizer backend a track should use.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SynthSource {
    SoundFont {
        path: String,
        #[serde(default)]
        bank: u32,
        #[serde(default)]
        preset: u8,
    },
    ClapPlugin { path: String },
    Sfz { path: String },
}

impl Default for SynthSource {
    fn default() -> Self {
        SynthSource::SoundFont {
            path: String::new(),
            bank: 0,
            preset: 0,
        }
    }
}

/// Distinguishes melodic tracks from drum/percussion tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TrackMode {
    /// Standard melodic instrument (piano, guitar, bass, etc.).
    Melodic,
    /// Percussion/drum track with a custom drum map.
    Drum(DrumMap),
}

impl Default for TrackMode {
    fn default() -> Self {
        TrackMode::Melodic
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlEvent {
    pub tick: u64,
    pub channel: u8,
    pub controller: u8,
    pub value: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackData {
    pub id: TrackId,
    pub name: String,
    pub notes: Vec<Note>,
    #[serde(default)]
    pub control_events: Vec<ControlEvent>,
    #[serde(skip, default)]
    pub synth_index: usize,
    pub synth_source: SynthSource,
    pub mixer: TrackMixerSettings,
    pub input: TrackInputSettings,
    pub mode: TrackMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiData {
    pub tracks: Vec<TrackData>,
    pub ticks_per_beat: u16,
    pub tempo_map: Vec<(u64, u32)>,
    next_track_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiEventType {
    NoteOn { pitch: u8, velocity: u8 },
    NoteOff { pitch: u8 },
    ControlChange { controller: u8, value: u8 },
}

#[derive(Debug, Clone)]
pub struct TimedEvent {
    pub time_seconds: f64,
    pub channel: u8,
    pub track_id: TrackId,
    pub synth_index: usize,
    pub event_type: MidiEventType,
}

impl MidiData {
    pub fn new_empty(track_names: &[String]) -> Self {
        let mut tracks = Vec::new();
        if track_names.is_empty() {
            tracks.push(TrackData {
                id: TrackId(1),
                name: "Track 0".to_string(),
                notes: Vec::new(),
                control_events: Vec::new(),
                synth_index: 0,
                synth_source: SynthSource::default(),
                mixer: TrackMixerSettings::default(),
                input: TrackInputSettings::default(),
                mode: TrackMode::default(),
            });
        } else {
            for (synth_index, name) in track_names.iter().enumerate() {
                tracks.push(TrackData {
                    id: TrackId(synth_index as u64 + 1),
                    name: name.clone(),
                    notes: Vec::new(),
                    control_events: Vec::new(),
                    synth_index,
                    synth_source: SynthSource::default(),
                    mixer: TrackMixerSettings::default(),
                    input: TrackInputSettings::default(),
                    mode: TrackMode::default(),
                });
            }
        }

        let next_track_id = tracks.len() as u64 + 1;
        MidiData {
            tracks,
            ticks_per_beat: 480,
            tempo_map: vec![(0, 500_000)],
            next_track_id,
        }
    }

    pub fn track_index(&self, track_id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|track| track.id == track_id)
    }

    pub fn add_track(&mut self, name: String, synth_index: usize) -> TrackId {
        let id = TrackId(self.next_track_id);
        self.next_track_id += 1;
        self.tracks.push(TrackData {
            id,
            name,
            notes: Vec::new(),
            control_events: Vec::new(),
            synth_index,
            synth_source: SynthSource::default(),
            mixer: TrackMixerSettings::default(),
            input: TrackInputSettings::default(),
            mode: TrackMode::default(),
        });
        id
    }

    pub fn duplicate_track(&mut self, track_id: TrackId) -> Option<TrackId> {
        let source = self
            .tracks
            .iter()
            .find(|track| track.id == track_id)?
            .clone();
        let id = TrackId(self.next_track_id);
        self.next_track_id += 1;
        let source_index = self.track_index(track_id)?;
        self.tracks.insert(
            source_index + 1,
            TrackData {
                id,
                name: format!("{} Copy", source.name),
                notes: source.notes.clone(),
                control_events: source.control_events.clone(),
                synth_index: source.synth_index,
                synth_source: source.synth_source.clone(),
                mixer: source.mixer,
                input: TrackInputSettings {
                    armed: false,
                    ..source.input
                },
                mode: source.mode.clone(),
            },
        );
        Some(id)
    }

    pub fn remove_track(&mut self, track_id: TrackId) -> bool {
        if self.tracks.len() <= 1 {
            return false;
        }
        let Some(index) = self.track_index(track_id) else {
            return false;
        };
        self.tracks.remove(index);
        true
    }

    pub fn move_track(&mut self, track_id: TrackId, new_index: usize) -> bool {
        let Some(old_index) = self.track_index(track_id) else {
            return false;
        };
        let last_index = self.tracks.len().saturating_sub(1);
        let new_index = new_index.min(last_index);
        if old_index == new_index {
            return false;
        }
        let track = self.tracks.remove(old_index);
        self.tracks.insert(new_index, track);
        true
    }

    pub fn compile_events(&self) -> Vec<TimedEvent> {
        let mut events = Vec::new();

        // Pre-calculate tempo changes for efficient lookup
        let mut tempo_changes = self.tempo_map.clone();
        tempo_changes.sort_by_key(|&(t, _)| t);

        let tick_to_seconds = |target_tick: u64| -> f64 {
            let mut time_sec = 0.0;
            let mut current_tick = 0;
            let mut current_tempo = 500_000; // default 120 BPM

            for &(tempo_tick, tempo_val) in &tempo_changes {
                if tempo_tick > target_tick {
                    break;
                }
                let dt = tempo_tick - current_tick;
                let bps = current_tempo as f64 / 1_000_000.0;
                let sec_per_tick = bps / self.ticks_per_beat as f64;
                time_sec += dt as f64 * sec_per_tick;

                current_tick = tempo_tick;
                current_tempo = tempo_val;
            }

            let dt = target_tick - current_tick;
            let bps = current_tempo as f64 / 1_000_000.0;
            let sec_per_tick = bps / self.ticks_per_beat as f64;
            time_sec += dt as f64 * sec_per_tick;

            time_sec
        };

        let has_solo = self.tracks.iter().any(|track| track.mixer.solo);
        for track in &self.tracks {
            if !track.is_audible(has_solo) {
                continue;
            }
            for note in &track.notes {
                let start_sec = tick_to_seconds(note.start_tick);
                let end_sec = tick_to_seconds(note.end_tick);

                events.push(TimedEvent {
                    time_seconds: start_sec,
                    channel: note.channel,
                    track_id: track.id,
                    synth_index: track.synth_index,
                    event_type: MidiEventType::NoteOn {
                        pitch: note.pitch,
                        velocity: note.velocity,
                    },
                });

                events.push(TimedEvent {
                    time_seconds: end_sec,
                    channel: note.channel,
                    track_id: track.id,
                    synth_index: track.synth_index,
                    event_type: MidiEventType::NoteOff { pitch: note.pitch },
                });
            }
            for ctrl in &track.control_events {
                let time_sec = tick_to_seconds(ctrl.tick);
                events.push(TimedEvent {
                    time_seconds: time_sec,
                    channel: ctrl.channel,
                    track_id: track.id,
                    synth_index: track.synth_index,
                    event_type: MidiEventType::ControlChange {
                        controller: ctrl.controller,
                        value: ctrl.value,
                    },
                });
            }
        }

        events.sort_by(|a, b| {
            a.time_seconds
                .partial_cmp(&b.time_seconds)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    same_time_event_order(&a.event_type).cmp(&same_time_event_order(&b.event_type))
                })
        });
        events
    }

    pub fn load(path: &str) -> Result<Self> {
        let data = fs::read(path)?;
        let smf = Smf::parse(&data)?;

        let ticks_per_beat = match smf.header.timing {
            Timing::Metrical(ticks) => ticks.as_int(),
            Timing::Timecode(_, _) => 480,
        };

        let mut tracks = Vec::new();
        let mut tempo_map = vec![(0, 500_000)];

        for track in &smf.tracks {
            let mut current_tick = 0;
            let mut active_notes: std::collections::HashMap<(u8, u8), (u64, u8)> =
                std::collections::HashMap::new();
            let mut notes = Vec::new();
            let mut control_events = Vec::new();
            let mut name = None;

            let mut initial_program: Option<u8> = None;

            for event in track {
                current_tick += event.delta.as_int() as u64;

                match event.kind {
                    TrackEventKind::Midi { channel, message } => {
                        let ch = channel.as_int();
                        match message {
                            MidiMessage::NoteOn { key, vel } => {
                                let p = key.as_int();
                                let v = vel.as_int();
                                if v > 0 {
                                    active_notes.insert((ch, p), (current_tick, v));
                                } else {
                                    if let Some((start, orig_vel)) = active_notes.remove(&(ch, p)) {
                                        notes.push(Note {
                                            pitch: p,
                                            velocity: orig_vel,
                                            start_tick: start,
                                            end_tick: current_tick,
                                            channel: ch,
                                        });
                                    }
                                }
                            }
                            MidiMessage::NoteOff { key, vel: _ } => {
                                let p = key.as_int();
                                if let Some((start, orig_vel)) = active_notes.remove(&(ch, p)) {
                                    notes.push(Note {
                                        pitch: p,
                                        velocity: orig_vel,
                                        start_tick: start,
                                        end_tick: current_tick,
                                        channel: ch,
                                    });
                                }
                            }
                            MidiMessage::ProgramChange { program } => {
                                initial_program.get_or_insert(program.as_int());
                            }
                            MidiMessage::Controller { controller, value } => {
                                control_events.push(ControlEvent {
                                    tick: current_tick,
                                    channel: ch,
                                    controller: controller.as_int(),
                                    value: value.as_int(),
                                });
                            }
                            _ => {}
                        }
                    }
                    TrackEventKind::Meta(meta) => match meta {
                        MetaMessage::Tempo(tempo) => {
                            if let Some((_, existing)) =
                                tempo_map.iter_mut().find(|(tick, _)| *tick == current_tick)
                            {
                                *existing = tempo.as_int();
                            } else {
                                tempo_map.push((current_tick, tempo.as_int()));
                            }
                        }
                        MetaMessage::TrackName(n) => {
                            name = Some(String::from_utf8_lossy(n).into_owned());
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }

            notes.sort_by_key(|n| n.start_tick);
            control_events.sort_by_key(|c| c.tick);
            if !notes.is_empty() || !control_events.is_empty() {
                let mode = if notes.iter().any(|n| n.channel == 9)
                    || control_events.iter().any(|c| c.channel == 9)
                {
                    TrackMode::Drum(DrumMap::gm_default())
                } else {
                    TrackMode::default()
                };
                let preset = initial_program.unwrap_or(0);
                tracks.push(TrackData {
                    id: TrackId(tracks.len() as u64 + 1),
                    name: name.unwrap_or_else(|| format!("Track {}", tracks.len())),
                    notes,
                    control_events,
                    synth_index: 0,
                    synth_source: SynthSource::SoundFont {
                        path: String::new(),
                        bank: 0,
                        preset,
                    },
                    mixer: TrackMixerSettings::default(),
                    input: TrackInputSettings::default(),
                    mode,
                });
            }
        }

        if tracks.is_empty() {
            tracks.push(TrackData {
                id: TrackId(1),
                name: "Track 0".to_string(),
                notes: vec![],
                control_events: vec![],
                synth_index: 0,
                synth_source: SynthSource::default(),
                mixer: TrackMixerSettings::default(),
                input: TrackInputSettings::default(),
                mode: TrackMode::default(),
            });
        }

        tempo_map.sort_by_key(|t| t.0);

        let next_track_id = tracks.len() as u64 + 1;
        Ok(MidiData {
            tracks,
            ticks_per_beat,
            tempo_map,
            next_track_id,
        })
    }

    pub fn get_bpm(&self) -> f64 {
        if let Some(tempo) = self.tempo_map.first() {
            60_000_000.0 / tempo.1 as f64
        } else {
            120.0
        }
    }

    pub fn set_bpm(&mut self, bpm: f64) {
        let tempo = (60_000_000.0 / bpm.max(1.0)) as u32;
        self.tempo_map = vec![(0, tempo)];
    }

    pub fn to_smf(&self) -> Smf<'_> {
        let header = Header {
            format: Format::Parallel,
            timing: Timing::Metrical(midly::num::u15::new(self.ticks_per_beat)),
        };

        let mut smf_tracks = Vec::new();

        // Track 0: Tempo map
        let mut track0 = Vec::new();
        let mut last_tick = 0;
        for (tick, tempo) in &self.tempo_map {
            let delta = *tick - last_tick;
            track0.push(TrackEvent {
                delta: midly::num::u28::new(delta as u32),
                kind: TrackEventKind::Meta(MetaMessage::Tempo(midly::num::u24::new(*tempo))),
            });
            last_tick = *tick;
        }
        track0.push(TrackEvent {
            delta: midly::num::u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf_tracks.push(track0);

        // Other tracks
        for t in &self.tracks {
            let mut events = vec![TrackEvent {
                delta: midly::num::u28::new(0),
                kind: TrackEventKind::Meta(MetaMessage::TrackName(t.name.as_bytes())),
            }];

            // Create absolute note on/off and control events
            #[derive(Debug, Clone)]
            enum Ev {
                Cc(u8, u8, u8), // ch, controller, val
                On(u8, u8, u8),
                Off(u8, u8, u8),
            }

            let mut abs_events: Vec<(u64, Ev)> = Vec::new();
            for n in &t.notes {
                abs_events.push((n.start_tick, Ev::On(n.channel, n.pitch, n.velocity)));
                abs_events.push((n.end_tick, Ev::Off(n.channel, n.pitch, 0)));
            }
            for c in &t.control_events {
                abs_events.push((c.tick, Ev::Cc(c.channel, c.controller, c.value)));
            }

            // Sort by tick, with tie-breaking:
            // CC(>=64) before NoteOff before NoteOn before CC(<64)
            abs_events.sort_by(|a, b| {
                if a.0 != b.0 {
                    a.0.cmp(&b.0)
                } else {
                    fn ev_rank(ev: &Ev) -> u8 {
                        match ev {
                            Ev::Cc(_, _, val) if *val < 64 => 0,
                            Ev::Off(_, _, _) => 1,
                            Ev::Cc(_, _, _) => 2,
                            Ev::On(_, _, _) => 3,
                        }
                    }
                    ev_rank(&a.1).cmp(&ev_rank(&b.1))
                }
            });

            let mut last_ev_tick = 0;
            for (tick, ev) in abs_events {
                let delta = tick - last_ev_tick;
                let kind = match ev {
                    Ev::On(ch, p, v) => TrackEventKind::Midi {
                        channel: midly::num::u4::new(ch),
                        message: MidiMessage::NoteOn {
                            key: midly::num::u7::new(p),
                            vel: midly::num::u7::new(v),
                        },
                    },
                    Ev::Off(ch, p, v) => TrackEventKind::Midi {
                        channel: midly::num::u4::new(ch),
                        message: MidiMessage::NoteOff {
                            key: midly::num::u7::new(p),
                            vel: midly::num::u7::new(v),
                        },
                    },
                    Ev::Cc(ch, ctrl, val) => TrackEventKind::Midi {
                        channel: midly::num::u4::new(ch),
                        message: MidiMessage::Controller {
                            controller: midly::num::u7::new(ctrl),
                            value: midly::num::u7::new(val),
                        },
                    },
                };

                // midly limits delta to u28. Loop to insert dummy events if delta is too large
                let mut remaining_delta = delta;
                while remaining_delta > 0x0FFFFFFF {
                    events.push(TrackEvent {
                        delta: midly::num::u28::new(0x0FFFFFFF),
                        // SysEx as dummy padding if needed, or just a controller
                        kind: TrackEventKind::Midi {
                            channel: midly::num::u4::new(0),
                            message: MidiMessage::Controller {
                                controller: midly::num::u7::new(0),
                                value: midly::num::u7::new(0),
                            },
                        },
                    });
                    remaining_delta -= 0x0FFFFFFF;
                }

                events.push(TrackEvent {
                    delta: midly::num::u28::new(remaining_delta as u32),
                    kind,
                });
                last_ev_tick = tick;
            }

            events.push(TrackEvent {
                delta: midly::num::u28::new(0),
                kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
            });

            smf_tracks.push(events);
        }

        Smf {
            header,
            tracks: smf_tracks,
        }
    }

    pub fn to_buffer(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.to_smf().write(&mut buf).unwrap();
        buf
    }

    pub fn export_to_file(&self, path: &str) -> Result<()> {
        fs::write(path, self.to_buffer())?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PedalInterval {
    pub start_tick: u64,
    pub end_tick: u64,
    pub channel: u8,
}

impl TrackData {
    pub fn is_audible(&self, has_solo: bool) -> bool {
        !self.mixer.mute && (!has_solo || self.mixer.solo)
    }

    /// Returns the active sustain pedal (CC 64) intervals for this track.
    pub fn pedal_intervals(&self) -> Vec<PedalInterval> {
        let mut ccs: Vec<&ControlEvent> = self
            .control_events
            .iter()
            .filter(|e| e.controller == 64)
            .collect();
        ccs.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.value.cmp(&b.value)));

        let mut intervals = Vec::new();
        let mut current_on: Option<(u64, u8)> = None;

        for cc in ccs {
            if cc.value >= 64 {
                if current_on.is_none() {
                    current_on = Some((cc.tick, cc.channel));
                }
            } else if let Some((start_tick, channel)) = current_on {
                intervals.push(PedalInterval {
                    start_tick,
                    end_tick: cc.tick.max(start_tick + 1),
                    channel,
                });
                current_on = None;
            }
        }

        if let Some((start_tick, channel)) = current_on {
            let last_tick = self
                .notes
                .iter()
                .map(|n| n.end_tick)
                .max()
                .unwrap_or(start_tick);
            intervals.push(PedalInterval {
                start_tick,
                end_tick: last_tick.max(start_tick + 480 * 4),
                channel,
            });
        }

        intervals
    }

    /// Add or replace a sustain pedal interval [start_tick, end_tick].
    pub fn set_pedal_interval(&mut self, start_tick: u64, end_tick: u64, channel: u8) {
        if end_tick <= start_tick {
            return;
        }
        self.control_events
            .retain(|e| !(e.controller == 64 && e.tick >= start_tick && e.tick <= end_tick));
        self.control_events.push(ControlEvent {
            tick: start_tick,
            channel,
            controller: 64,
            value: 127,
        });
        self.control_events.push(ControlEvent {
            tick: end_tick,
            channel,
            controller: 64,
            value: 0,
        });
        self.control_events
            .sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.value.cmp(&b.value)));
    }

    /// Delete a sustain pedal interval that covers `tick`.
    pub fn delete_pedal_interval_at(&mut self, tick: u64) -> bool {
        let intervals = self.pedal_intervals();
        if let Some(target) = intervals
            .into_iter()
            .find(|i| tick >= i.start_tick && tick <= i.end_tick)
        {
            self.control_events.retain(|e| {
                !(e.controller == 64 && e.tick >= target.start_tick && e.tick <= target.end_tick)
            });
            return true;
        }
        false
    }

    /// Move a sustain pedal interval from `[orig_start, orig_end]` to `[new_start, new_end]`.
    pub fn move_pedal_interval(
        &mut self,
        orig_start: u64,
        orig_end: u64,
        new_start: u64,
        new_end: u64,
        channel: u8,
    ) {
        self.control_events
            .retain(|e| !(e.controller == 64 && e.tick >= orig_start && e.tick <= orig_end));
        self.set_pedal_interval(new_start, new_end, channel);
    }
}

/// Tie-breaker for [`MidiData::compile_events`]: at the same timestamp, CC (off)
/// before NoteOff before CC (on) before NoteOn.
fn same_time_event_order(event: &MidiEventType) -> u8 {
    match event {
        MidiEventType::ControlChange { value, .. } if *value < 64 => 0,
        MidiEventType::NoteOff { .. } => 1,
        MidiEventType::ControlChange { .. } => 2,
        MidiEventType::NoteOn { .. } => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pitch: u8, start: u64, end: u64) -> Note {
        Note {
            pitch,
            velocity: 100,
            start_tick: start,
            end_tick: end,
            channel: 0,
        }
    }

    fn midi_with(notes: Vec<Note>) -> MidiData {
        MidiData {
            tracks: vec![TrackData {
                id: TrackId(1),
                name: "t0".into(),
                notes,
                control_events: Vec::new(),
                synth_index: 0,
                synth_source: SynthSource::default(),
                mixer: TrackMixerSettings::default(),
                input: TrackInputSettings::default(),
                mode: TrackMode::default(),
            }],
            ticks_per_beat: 480,
            tempo_map: vec![(0, 500_000)],
            next_track_id: 2,
        }
    }

    fn boundary_event_kinds(midi: &MidiData) -> Vec<&'static str> {
        midi.compile_events()
            .iter()
            .filter(|e| (e.time_seconds - 0.5).abs() < 1e-9)
            .map(|e| match &e.event_type {
                MidiEventType::NoteOn { .. } => "on",
                MidiEventType::NoteOff { .. } => "off",
                MidiEventType::ControlChange { .. } => "cc",
            })
            .collect()
    }

    #[test]
    fn compile_events_note_off_before_note_on_at_same_time() {
        // Later note stored first in Vec — the case that triggered the bug.
        let midi = midi_with(vec![note(60, 480, 960), note(60, 0, 480)]);
        let boundary = boundary_event_kinds(&midi);
        assert_eq!(boundary, &["off", "on"]);
    }

    #[test]
    fn compile_events_chronological_note_order_still_works() {
        let midi = midi_with(vec![note(60, 0, 480), note(60, 480, 960)]);
        let boundary = boundary_event_kinds(&midi);
        assert_eq!(boundary, &["off", "on"]);
    }

    #[test]
    fn track_ids_remain_stable_across_reorder_and_delete() {
        let mut midi = MidiData::new_empty(&["A".into(), "B".into()]);
        let a = midi.tracks[0].id;
        let b = midi.tracks[1].id;
        let c = midi.add_track("C".into(), 0);

        assert!(midi.move_track(c, 0));
        assert_eq!(
            midi.tracks.iter().map(|track| track.id).collect::<Vec<_>>(),
            vec![c, a, b]
        );
        assert!(midi.remove_track(a));
        assert_eq!(
            midi.tracks.iter().map(|track| track.id).collect::<Vec<_>>(),
            vec![c, b]
        );
    }

    #[test]
    fn duplicated_track_gets_a_new_id_and_copies_notes() {
        let mut midi = midi_with(vec![note(60, 0, 480)]);
        let original = midi.tracks[0].id;
        let duplicate = midi.duplicate_track(original).unwrap();

        assert_ne!(duplicate, original);
        assert_eq!(midi.tracks[1].id, duplicate);
        assert_eq!(midi.tracks[1].notes.len(), 1);
        assert_eq!(midi.tracks[1].name, "t0 Copy");
    }

    #[test]
    fn compile_events_carry_stable_track_id() {
        let midi = midi_with(vec![note(60, 0, 480)]);
        assert!(
            midi.compile_events()
                .iter()
                .all(|event| event.track_id == TrackId(1))
        );
    }

    #[test]
    fn mute_and_solo_filter_compiled_tracks() {
        let mut midi = MidiData::new_empty(&["A".into(), "B".into(), "C".into()]);
        for (index, track) in midi.tracks.iter_mut().enumerate() {
            track.notes.push(note(60 + index as u8, 0, 480));
        }

        midi.tracks[0].mixer.mute = true;
        let pitches = midi
            .compile_events()
            .into_iter()
            .filter_map(|event| match event.event_type {
                MidiEventType::NoteOn { pitch, .. } => Some(pitch),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(pitches, vec![61, 62]);

        midi.tracks[2].mixer.solo = true;
        let pitches = midi
            .compile_events()
            .into_iter()
            .filter_map(|event| match event.event_type {
                MidiEventType::NoteOn { pitch, .. } => Some(pitch),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(pitches, vec![62]);
    }

    #[test]
    fn exported_format_one_tracks_keep_their_names() {
        let midi = MidiData::new_empty(&["Piano".into(), "Strings".into()]);
        let bytes = midi.to_buffer();
        let parsed = Smf::parse(&bytes).unwrap();

        let names = parsed
            .tracks
            .iter()
            .skip(1)
            .filter_map(|track| {
                track.iter().find_map(|event| match event.kind {
                    TrackEventKind::Meta(MetaMessage::TrackName(name)) => {
                        Some(String::from_utf8_lossy(name).into_owned())
                    }
                    _ => None,
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Piano", "Strings"]);
    }

    #[test]
    fn pedal_interval_crud_and_query() {
        let mut track = TrackData {
            id: TrackId(1),
            name: "Piano".into(),
            notes: vec![note(60, 0, 480)],
            control_events: Vec::new(),
            synth_index: 0,
            synth_source: SynthSource::default(),
            mixer: TrackMixerSettings::default(),
            input: TrackInputSettings::default(),
            mode: TrackMode::default(),
        };

        // Initially no intervals
        assert!(track.pedal_intervals().is_empty());

        // Add interval [480, 960]
        track.set_pedal_interval(480, 960, 0);
        let intervals = track.pedal_intervals();
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].start_tick, 480);
        assert_eq!(intervals[0].end_tick, 960);
        assert_eq!(track.control_events.len(), 2);
        assert_eq!(track.control_events[0].controller, 64);
        assert_eq!(track.control_events[0].value, 127);
        assert_eq!(track.control_events[1].controller, 64);
        assert_eq!(track.control_events[1].value, 0);

        // Move interval from [480, 960] to [960, 1440]
        track.move_pedal_interval(480, 960, 960, 1440, 0);
        let intervals = track.pedal_intervals();
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].start_tick, 960);
        assert_eq!(intervals[0].end_tick, 1440);

        // Delete interval
        assert!(track.delete_pedal_interval_at(1000));
        assert!(track.pedal_intervals().is_empty());
        assert!(track.control_events.is_empty());
    }

    #[test]
    fn pedal_smf_round_trip() {
        let mut midi = MidiData::new_empty(&["Piano".into()]);
        midi.tracks[0].notes.push(note(60, 0, 480));
        midi.tracks[0].notes.push(note(64, 480, 960));
        midi.tracks[0].set_pedal_interval(240, 960, 0);

        // Write to temp file and load back
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_pedal_roundtrip.mid");
        let path_str = path.to_str().unwrap();
        midi.export_to_file(path_str).unwrap();

        let loaded = MidiData::load(path_str).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.tracks[0].notes.len(), 2);
        let intervals = loaded.tracks[0].pedal_intervals();
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].start_tick, 240);
        assert_eq!(intervals[0].end_tick, 960);
    }

    #[test]
    fn compile_events_interleaves_pedal_and_notes_with_correct_order() {
        let mut midi = MidiData::new_empty(&["Piano".into()]);
        // Note 1: 0..480. Note 2: 480..960.
        midi.tracks[0].notes.push(note(60, 0, 480));
        midi.tracks[0].notes.push(note(64, 480, 960));
        // Pedal 1: 0..480. Pedal 2: 480..960.
        // At tick 480:
        // Expected order: CC off (pedal 1) -> NoteOff (note 1) -> CC on (pedal 2) -> NoteOn (note 2)
        midi.tracks[0].control_events.push(ControlEvent {
            tick: 0,
            channel: 0,
            controller: 64,
            value: 127,
        });
        midi.tracks[0].control_events.push(ControlEvent {
            tick: 480,
            channel: 0,
            controller: 64,
            value: 0,
        });
        midi.tracks[0].control_events.push(ControlEvent {
            tick: 480,
            channel: 0,
            controller: 64,
            value: 127,
        });
        midi.tracks[0].control_events.push(ControlEvent {
            tick: 960,
            channel: 0,
            controller: 64,
            value: 0,
        });

        let events = midi.compile_events();
        // Filter events at tick 480 (time = 0.5s at 120bpm, 480 tpb)
        let at_480: Vec<&MidiEventType> = events
            .iter()
            .filter(|e| (e.time_seconds - 0.5).abs() < 1e-6)
            .map(|e| &e.event_type)
            .collect();

        assert_eq!(at_480.len(), 4);
        assert!(matches!(
            at_480[0],
            MidiEventType::ControlChange {
                controller: 64,
                value: 0
            }
        ));
        assert!(matches!(at_480[1], MidiEventType::NoteOff { pitch: 60 }));
        assert!(matches!(
            at_480[2],
            MidiEventType::ControlChange {
                controller: 64,
                value: 127
            }
        ));
        assert!(matches!(at_480[3], MidiEventType::NoteOn { pitch: 64, .. }));
    }
}
