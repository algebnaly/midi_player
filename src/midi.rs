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
use std::fs;

#[derive(Debug, Clone)]
pub struct Note {
    pub pitch: u8,
    pub velocity: u8,
    pub start_tick: u64,
    pub end_tick: u64,
    pub channel: u8,
}

#[derive(Debug, Clone)]
pub struct TrackData {
    pub name: String,
    pub notes: Vec<Note>,
    pub synth_index: usize,
}

#[derive(Debug, Clone)]
pub struct MidiData {
    pub tracks: Vec<TrackData>,
    pub ticks_per_beat: u16,
    pub tempo_map: Vec<(u64, u32)>,
}

#[derive(Debug, Clone)]
pub enum MidiEventType {
    NoteOn { pitch: u8, velocity: u8 },
    NoteOff { pitch: u8 },
}

#[derive(Debug, Clone)]
pub struct TimedEvent {
    pub time_seconds: f64,
    pub channel: u8,
    pub track_index: usize,
    pub synth_index: usize,
    pub event_type: MidiEventType,
}

impl MidiData {
    pub fn new_empty(track_names: &[String]) -> Self {
        let mut tracks = Vec::new();
        if track_names.is_empty() {
            tracks.push(TrackData {
                name: "Track 0".to_string(),
                notes: Vec::new(),
                synth_index: 0,
            });
        } else {
            for (synth_index, name) in track_names.iter().enumerate() {
                tracks.push(TrackData {
                    name: name.clone(),
                    notes: Vec::new(),
                    synth_index,
                });
            }
        }

        MidiData {
            tracks,
            ticks_per_beat: 480,
            tempo_map: vec![(0, 500_000)],
        }
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

        for (track_idx, track) in self.tracks.iter().enumerate() {
            for note in &track.notes {
                let start_sec = tick_to_seconds(note.start_tick);
                let end_sec = tick_to_seconds(note.end_tick);

                events.push(TimedEvent {
                    time_seconds: start_sec,
                    channel: note.channel,
                    track_index: track_idx,
                    synth_index: track.synth_index,
                    event_type: MidiEventType::NoteOn {
                        pitch: note.pitch,
                        velocity: note.velocity,
                    },
                });

                events.push(TimedEvent {
                    time_seconds: end_sec,
                    channel: note.channel,
                    track_index: track_idx,
                    synth_index: track.synth_index,
                    event_type: MidiEventType::NoteOff { pitch: note.pitch },
                });
            }
        }

        events.sort_by(|a, b| {
            a.time_seconds
                .partial_cmp(&b.time_seconds)
                .unwrap_or(std::cmp::Ordering::Equal)
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
            let mut name = None;

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
            if !notes.is_empty() {
                tracks.push(TrackData {
                    name: name.unwrap_or_else(|| format!("Track {}", tracks.len())),
                    notes,
                    synth_index: 0,
                });
            }
        }

        if tracks.is_empty() {
            tracks.push(TrackData {
                name: "Track 0".to_string(),
                notes: Vec::new(),
                synth_index: 0,
            });
        }

        tempo_map.sort_by_key(|t| t.0);

        Ok(MidiData {
            tracks,
            ticks_per_beat,
            tempo_map,
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

    pub fn to_smf(&self) -> Smf<'static> {
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
            let mut events = Vec::new();

            // Create absolute note on/off events
            #[derive(Debug, Clone)]
            enum Ev {
                On(u8, u8, u8),
                Off(u8, u8, u8),
            } // ch, pitch, vel

            let mut abs_events: Vec<(u64, Ev)> = Vec::new();
            for n in &t.notes {
                abs_events.push((n.start_tick, Ev::On(n.channel, n.pitch, n.velocity)));
                abs_events.push((n.end_tick, Ev::Off(n.channel, n.pitch, 0)));
            }

            // Sort by tick, then NoteOff before NoteOn
            abs_events.sort_by(|a, b| {
                if a.0 != b.0 {
                    a.0.cmp(&b.0)
                } else {
                    match (&a.1, &b.1) {
                        (Ev::Off(_, _, _), Ev::On(_, _, _)) => std::cmp::Ordering::Less,
                        (Ev::On(_, _, _), Ev::Off(_, _, _)) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    }
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
