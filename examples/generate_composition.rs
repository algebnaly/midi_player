//! Comprehensive composition generator for midi_player.
//! Generates "Dawn of Resonance" (晨曦共鸣) in D Major (96 BPM, 48 bars = 120 seconds).
//! Features Grand Piano, Clean Electric Guitar, Electric Bass, and Drum Kit.
//!
//! Run with: cargo run --example generate_composition

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use serde::{Deserialize, Serialize};
use std::fs;

const TPB: u16 = 480; // ticks per beat
const BAR: u64 = (TPB as u64) * 4; // 1920 ticks per bar
const BEAT: u64 = TPB as u64; // 480 ticks
const HALF_BEAT: u64 = BEAT / 2; // 240 ticks (8th note)
const QUARTER_BEAT: u64 = BEAT / 4; // 120 ticks (16th note)

// ── Models for .midiproj serialization ───────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct ProjectFile {
    schema_version: u32,
    midi: MidiData,
}

#[derive(Debug, Serialize, Deserialize)]
struct MidiData {
    tracks: Vec<TrackData>,
    ticks_per_beat: u16,
    tempo_map: Vec<(u64, u32)>,
    next_track_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrackId(u64);

#[derive(Debug, Serialize, Deserialize)]
struct TrackData {
    id: TrackId,
    name: String,
    notes: Vec<Note>,
    #[serde(default)]
    control_events: Vec<ControlEvent>,
    synth_source: SynthSource,
    mixer: TrackMixerSettings,
    input: TrackInputSettings,
    mode: TrackMode,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrackMixerSettings {
    mute: bool,
    solo: bool,
    volume_db: f32,
    pan: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrackInputSettings {
    armed: bool,
    channel_filter: Option<u8>,
    transpose: i8,
}

#[derive(Debug, Serialize, Deserialize)]
enum SynthSource {
    SoundFont { path: String },
}

#[derive(Debug, Serialize, Deserialize)]
enum TrackMode {
    Melodic,
    Drum(DrumMap),
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
enum DrumCategory {
    Kick,
    Snare,
    HiHat,
    Tom,
    Cymbal,
    Percussion,
}

#[derive(Debug, Serialize, Deserialize)]
struct DrumMapEntry {
    pitch: u8,
    name: String,
    short_name: String,
    category: DrumCategory,
}

#[derive(Debug, Serialize, Deserialize)]
struct DrumMap {
    entries: Vec<DrumMapEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Note {
    pitch: u8,
    velocity: u8,
    start_tick: u64,
    end_tick: u64,
    channel: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ControlEvent {
    tick: u64,
    channel: u8,
    controller: u8,
    value: u8,
}

fn gm_drum_map() -> DrumMap {
    DrumMap {
        entries: vec![
            DrumMapEntry { pitch: 36, name: "Bass Drum 1".into(), short_name: "Kick".into(), category: DrumCategory::Kick },
            DrumMapEntry { pitch: 37, name: "Side Stick".into(), short_name: "Stick".into(), category: DrumCategory::Snare },
            DrumMapEntry { pitch: 38, name: "Acoustic Snare".into(), short_name: "Snare".into(), category: DrumCategory::Snare },
            DrumMapEntry { pitch: 42, name: "Closed Hi-Hat".into(), short_name: "ClHat".into(), category: DrumCategory::HiHat },
            DrumMapEntry { pitch: 46, name: "Open Hi-Hat".into(), short_name: "OpHat".into(), category: DrumCategory::HiHat },
            DrumMapEntry { pitch: 49, name: "Crash Cymbal 1".into(), short_name: "Crash".into(), category: DrumCategory::Cymbal },
            DrumMapEntry { pitch: 51, name: "Ride Cymbal 1".into(), short_name: "Ride".into(), category: DrumCategory::Cymbal },
            DrumMapEntry { pitch: 43, name: "High Floor Tom".into(), short_name: "FTom".into(), category: DrumCategory::Tom },
            DrumMapEntry { pitch: 47, name: "Low-Mid Tom".into(), short_name: "MTom".into(), category: DrumCategory::Tom },
            DrumMapEntry { pitch: 50, name: "High Tom".into(), short_name: "HTom".into(), category: DrumCategory::Tom },
        ],
    }
}

// ── Composition Notes Builder ────────────────────────────────────────

struct Composition {
    piano_notes: Vec<Note>,
    piano_pedals: Vec<ControlEvent>,
    guitar_notes: Vec<Note>,
    bass_notes: Vec<Note>,
    drum_notes: Vec<Note>,
}

impl Composition {
    fn new() -> Self {
        Self {
            piano_notes: Vec::new(),
            piano_pedals: Vec::new(),
            guitar_notes: Vec::new(),
            bass_notes: Vec::new(),
            drum_notes: Vec::new(),
        }
    }

    fn add_piano_note(&mut self, pitch: u8, vel: u8, start: u64, dur: u64) {
        self.piano_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: start,
            end_tick: start + dur,
            channel: 0,
        });
    }

    fn add_pedal(&mut self, start: u64, end: u64) {
        self.piano_pedals.push(ControlEvent {
            tick: start,
            channel: 0,
            controller: 64,
            value: 127,
        });
        self.piano_pedals.push(ControlEvent {
            tick: end,
            channel: 0,
            controller: 64,
            value: 0,
        });
    }

    fn add_guitar_note(&mut self, pitch: u8, vel: u8, start: u64, dur: u64) {
        self.guitar_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: start,
            end_tick: start + dur,
            channel: 1,
        });
    }

    fn add_bass_note(&mut self, pitch: u8, vel: u8, start: u64, dur: u64) {
        self.bass_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: start,
            end_tick: start + dur,
            channel: 2,
        });
    }

    fn add_drum_hit(&mut self, pitch: u8, vel: u8, start: u64) {
        self.drum_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: start,
            end_tick: start + 60,
            channel: 9,
        });
    }
}

fn build_composition() -> Composition {
    let mut comp = Composition::new();

    // =========================================================================
    // SECTION 1: INTRO (Bars 1 - 8) [0.0s - 20.0s]
    // Elegant arpeggiated piano with sustain pedal; guitar adds celestial harmonics.
    // Chords:
    // Bar 1-2: Dmaj9
    // Bar 3-4: Bm9
    // Bar 5-6: Gmaj7
    // Bar 7-8: Asus4 -> A7
    // =========================================================================

    for bar_idx in 0..8 {
        let b_start = (bar_idx as u64) * BAR;
        comp.add_pedal(b_start, b_start + BAR - 40);

        match bar_idx {
            0 | 1 => {
                // Dmaj9: D2 - A2 | F#4 - A4 - C#5 - E5 - A4 - F#4
                comp.add_piano_note(38, 76, b_start, BAR - 20); // D2
                comp.add_piano_note(45, 68, b_start + HALF_BEAT, BAR - HALF_BEAT - 20); // A2
                let pattern = [66, 69, 73, 76, 69, 66, 73, 76]; // F#4, A4, C#5, E5...
                for (step, &p) in pattern.iter().enumerate() {
                    let t = b_start + (step as u64) * HALF_BEAT;
                    let v = if step == 0 || step == 4 { 74 } else { 62 };
                    comp.add_piano_note(p, v, t, HALF_BEAT + 60);
                }
            }
            2 | 3 => {
                // Bm9: B1 - F#2 | D4 - F#4 - B4 - C#5 - F#4 - D4
                comp.add_piano_note(35, 75, b_start, BAR - 20); // B1
                comp.add_piano_note(42, 66, b_start + HALF_BEAT, BAR - HALF_BEAT - 20); // F#2
                let pattern = [62, 66, 71, 73, 66, 62, 71, 73]; // D4, F#4, B4, C#5...
                for (step, &p) in pattern.iter().enumerate() {
                    let t = b_start + (step as u64) * HALF_BEAT;
                    let v = if step == 0 || step == 4 { 72 } else { 60 };
                    comp.add_piano_note(p, v, t, HALF_BEAT + 60);
                }
            }
            4 | 5 => {
                // Gmaj7: G1 - D2 | B3 - D4 - G4 - F#5 - D4 - B3
                comp.add_piano_note(31, 78, b_start, BAR - 20); // G1
                comp.add_piano_note(38, 68, b_start + HALF_BEAT, BAR - HALF_BEAT - 20); // D2
                let pattern = [59, 62, 67, 78, 67, 62, 67, 78]; // B3, D4, G4, F#5...
                for (step, &p) in pattern.iter().enumerate() {
                    let t = b_start + (step as u64) * HALF_BEAT;
                    let v = if step == 0 || step == 4 { 74 } else { 64 };
                    comp.add_piano_note(p, v, t, HALF_BEAT + 60);
                }
            }
            6 => {
                // Asus4: A1 - E2 | D4 - E4 - A4 - E5
                comp.add_piano_note(33, 76, b_start, BAR - 20); // A1
                comp.add_piano_note(40, 68, b_start + HALF_BEAT, BAR - HALF_BEAT - 20); // E2
                let pattern = [62, 64, 69, 76, 69, 64, 69, 76];
                for (step, &p) in pattern.iter().enumerate() {
                    let t = b_start + (step as u64) * HALF_BEAT;
                    comp.add_piano_note(p, 65, t, HALF_BEAT + 60);
                }
            }
            7 => {
                // A7: A1 - E2 | C#4 - E4 - G4 - C#5
                comp.add_piano_note(33, 78, b_start, BAR - 20);
                comp.add_piano_note(40, 70, b_start + HALF_BEAT, BAR - HALF_BEAT - 20);
                let pattern = [61, 64, 67, 73, 67, 64, 67, 73];
                for (step, &p) in pattern.iter().enumerate() {
                    let t = b_start + (step as u64) * HALF_BEAT;
                    comp.add_piano_note(p, 68, t, HALF_BEAT + 60);
                }
            }
            _ => {}
        }

        // Guitar ambient chime in Bars 5 - 8
        if bar_idx >= 4 {
            let g_offset = b_start + BEAT;
            match bar_idx {
                4 => {
                    comp.add_guitar_note(78, 65, g_offset, BEAT * 2); // F#5
                    comp.add_guitar_note(74, 60, g_offset + BEAT / 2, BEAT * 2); // D5
                }
                5 => {
                    comp.add_guitar_note(78, 68, g_offset, BEAT * 2);
                    comp.add_guitar_note(81, 62, g_offset + BEAT / 2, BEAT * 2); // A5
                }
                6 => {
                    comp.add_guitar_note(76, 65, g_offset, BEAT * 2); // E5
                    comp.add_guitar_note(74, 60, g_offset + BEAT / 2, BEAT * 2); // D5
                }
                7 => {
                    comp.add_guitar_note(73, 70, g_offset, BEAT * 3); // C#5
                }
                _ => {}
            }
        }
    }

    // =========================================================================
    // SECTION 2: VERSE 1 (Bars 9 - 16) [20.0s - 40.0s]
    // Full rhythm section enters gently: side-stick, bass groove, piano melody.
    // Chords: D - F#m7 - G - A | Bm - F#m7 - G - A
    // =========================================================================

    let verse_chords = [
        (38, 45, 62, 66, 69), // D: D2, A2, D4, F#4, A4
        (42, 49, 61, 66, 69), // F#m7: F#2, C#3, C#4, F#4, A4
        (43, 50, 62, 67, 71), // G: G2, D3, D4, G4, B4
        (45, 52, 64, 69, 73), // A: A2, E3, E4, A4, C#5
        (35, 42, 62, 66, 71), // Bm: B1, F#2, D4, F#4, B4
        (42, 49, 61, 66, 69), // F#m7
        (43, 50, 62, 67, 71), // G
        (45, 52, 64, 69, 73), // A
    ];

    let verse_bass_roots = [38, 42, 43, 45, 35, 42, 43, 45]; // D, F#, G, A, B, F#, G, A

    // Piano Lead Melody in Verse
    let verse_melody: &[(usize, f64, u8, f64, u8)] = &[
        // (bar_offset_0_7, beat_start, pitch, beat_duration, vel)
        (0, 0.0, 69, 1.0, 85),  // A4
        (0, 1.0, 66, 1.0, 80),  // F#4
        (0, 2.0, 69, 2.0, 86),  // A4
        (1, 0.0, 74, 2.5, 92),  // D5
        (1, 2.5, 73, 1.5, 84),  // C#5
        (2, 0.0, 71, 2.0, 88),  // B4
        (2, 2.0, 69, 2.0, 84),  // A4
        (3, 0.0, 66, 3.5, 86),  // F#4
        (4, 0.0, 67, 1.0, 82),  // G4
        (4, 1.0, 69, 1.0, 85),  // A4
        (4, 2.0, 71, 2.0, 89),  // B4
        (5, 0.0, 74, 2.0, 92),  // D5
        (5, 2.0, 73, 1.0, 85),  // C#5
        (5, 3.0, 71, 1.0, 82),  // B4
        (6, 0.0, 69, 3.0, 86),  // A4
        (6, 3.0, 71, 1.0, 84),  // B4
        (7, 0.0, 69, 4.0, 88),  // A4 (sustained)
    ];

    for bar_i in 0..8 {
        let b = 8 + bar_i;
        let b_start = (b as u64) * BAR;
        comp.add_pedal(b_start, b_start + BAR - 40);

        let (r1, r2, c1, c2, c3) = verse_chords[bar_i];

        // Piano Left Hand Accompaniment
        comp.add_piano_note(r1, 72, b_start, BEAT * 2);
        comp.add_piano_note(r2, 65, b_start + BEAT, BEAT);
        comp.add_piano_note(c1, 60, b_start + BEAT * 2, BEAT * 2);
        comp.add_piano_note(c2, 62, b_start + BEAT * 2, BEAT * 2);
        comp.add_piano_note(c3, 62, b_start + BEAT * 2, BEAT * 2);

        // Electric Bass (Warm fingerstyle, root-5th-octave groove)
        let broot = verse_bass_roots[bar_i];
        let bfifth = broot + 7;
        let boct = broot + 12;
        comp.add_bass_note(broot, 90, b_start, BEAT + HALF_BEAT);
        comp.add_bass_note(bfifth, 80, b_start + BEAT * 2, BEAT);
        comp.add_bass_note(broot, 85, b_start + BEAT * 3, HALF_BEAT);
        comp.add_bass_note(boct, 78, b_start + BEAT * 3 + HALF_BEAT, HALF_BEAT);

        // Guitar fingerpicking counter-chords
        for beat_idx in 0..4 {
            let t = b_start + (beat_idx as u64) * BEAT;
            comp.add_guitar_note(c2, 60, t + HALF_BEAT, HALF_BEAT);
            comp.add_guitar_note(c3 + 12, 64, t + HALF_BEAT + QUARTER_BEAT, QUARTER_BEAT);
        }

        // Drums (Intimate verse beat: Kick on 1 and 3&, SideStick on 2 and 4, 8th-note Hi-Hat)
        comp.add_drum_hit(36, 95, b_start); // Kick on 1
        comp.add_drum_hit(36, 85, b_start + BEAT * 2 + HALF_BEAT); // Kick on 3&
        comp.add_drum_hit(37, 88, b_start + BEAT); // Side stick on 2
        comp.add_drum_hit(37, 92, b_start + BEAT * 3); // Side stick on 4

        for step in 0..8 {
            let t = b_start + (step as u64) * HALF_BEAT;
            let v = if step % 2 == 0 { 75 } else { 58 };
            comp.add_drum_hit(42, v, t); // Closed HH
        }

        // Fill at Bar 16 (bar_i == 7)
        if bar_i == 7 {
            comp.add_drum_hit(38, 75, b_start + BEAT * 3);
            comp.add_drum_hit(38, 85, b_start + BEAT * 3 + QUARTER_BEAT);
            comp.add_drum_hit(38, 95, b_start + BEAT * 3 + HALF_BEAT);
            comp.add_drum_hit(38, 105, b_start + BEAT * 3 + HALF_BEAT + QUARTER_BEAT);
        }
    }

    // Insert Verse Melody
    for &(bar_rel, b_offset, pitch, b_dur, vel) in verse_melody {
        let t = ((8 + bar_rel) as u64) * BAR + (b_offset * BEAT as f64) as u64;
        let d = (b_dur * BEAT as f64) as u64;
        comp.add_piano_note(pitch, vel, t, d);
    }

    // =========================================================================
    // SECTION 3: PRE-CHORUS (Bars 17 - 24) [40.0s - 60.0s]
    // Building momentum! Driving 8th bass, 16th guitar comping, 4-on-floor kick,
    // crescendo leading to explosive drum fill at Bar 24.
    // Chords: Em7 -> F#m7 -> Gmaj7 -> A | Em7 -> F#m7 -> Gmaj7 -> A7sus4 -> A7
    // =========================================================================

    let pre_chords = [
        (40, 47, 64, 67, 71), // Em7 (E)
        (42, 49, 66, 69, 73), // F#m7 (F#)
        (43, 50, 67, 71, 74), // Gmaj7 (G)
        (45, 52, 69, 73, 76), // A (A)
        (40, 47, 64, 67, 71), // Em7
        (42, 49, 66, 69, 73), // F#m7
        (43, 50, 67, 71, 74), // Gmaj7
        (45, 52, 69, 74, 76), // A7sus4 -> A7
    ];

    let pre_bass_roots = [40, 42, 43, 45, 40, 42, 43, 45]; // E, F#, G, A...

    for bar_i in 0..8 {
        let b = 16 + bar_i;
        let b_start = (b as u64) * BAR;
        comp.add_pedal(b_start, b_start + BAR - 40);

        let dyn_boost = (bar_i as u8) * 3; // Crescendo!
        let (r1, _r2, c1, c2, c3) = pre_chords[bar_i];
        let broot = pre_bass_roots[bar_i];

        // Piano pulsing chords with rising intensity
        for beat_idx in 0..4 {
            let t = b_start + (beat_idx as u64) * BEAT;
            comp.add_piano_note(c1, 72 + dyn_boost, t, HALF_BEAT + 60);
            comp.add_piano_note(c2, 74 + dyn_boost, t, HALF_BEAT + 60);
            comp.add_piano_note(c3, 76 + dyn_boost, t, HALF_BEAT + 60);
            comp.add_piano_note(c1, 65 + dyn_boost, t + HALF_BEAT, HALF_BEAT + 40);
            comp.add_piano_note(c2, 67 + dyn_boost, t + HALF_BEAT, HALF_BEAT + 40);
        }
        comp.add_piano_note(r1 - 12, 80 + dyn_boost, b_start, BAR);

        // Bass 8th-note pulse
        for step in 0..8 {
            let t = b_start + (step as u64) * HALF_BEAT;
            let v = if step % 2 == 0 { 85 + dyn_boost } else { 72 + dyn_boost };
            comp.add_bass_note(broot, v, t, HALF_BEAT - 20);
        }

        // Guitar 16th-note rhythmic chops
        for beat_idx in 0..4 {
            let t = b_start + (beat_idx as u64) * BEAT;
            comp.add_guitar_note(c2, 65 + dyn_boost, t + QUARTER_BEAT, QUARTER_BEAT);
            comp.add_guitar_note(c3, 68 + dyn_boost, t + QUARTER_BEAT * 3, QUARTER_BEAT);
        }

        // Drums: 4-on-the-floor kick, building open hats and snare backbeat
        if bar_i < 7 {
            for beat_idx in 0..4 {
                let t = b_start + (beat_idx as u64) * BEAT;
                comp.add_drum_hit(36, 92 + dyn_boost, t); // Kick on every beat
                comp.add_drum_hit(42, 70 + dyn_boost, t); // Closed HH
                comp.add_drum_hit(46, 68 + dyn_boost, t + HALF_BEAT); // Open HH upbeat
            }
            comp.add_drum_hit(38, 96 + dyn_boost, b_start + BEAT); // Snare on 2
            comp.add_drum_hit(38, 100 + dyn_boost, b_start + BEAT * 3); // Snare on 4
        } else {
            // Bar 24: Grand Pre-Chorus Drum Fill!
            comp.add_drum_hit(36, 110, b_start);
            comp.add_drum_hit(38, 105, b_start);
            comp.add_drum_hit(36, 110, b_start + BEAT);
            comp.add_drum_hit(38, 108, b_start + BEAT);
            // Tom roll cascade on beats 3 & 4
            let t3 = b_start + BEAT * 2;
            comp.add_drum_hit(50, 105, t3); // High tom
            comp.add_drum_hit(50, 108, t3 + QUARTER_BEAT);
            comp.add_drum_hit(47, 110, t3 + HALF_BEAT); // Mid tom
            comp.add_drum_hit(47, 112, t3 + HALF_BEAT + QUARTER_BEAT);
            let t4 = b_start + BEAT * 3;
            comp.add_drum_hit(43, 115, t4); // Floor tom
            comp.add_drum_hit(43, 118, t4 + QUARTER_BEAT);
            comp.add_drum_hit(38, 122, t4 + HALF_BEAT); // Snare flam
            comp.add_drum_hit(38, 127, t4 + HALF_BEAT + QUARTER_BEAT);
        }
    }

    // =========================================================================
    // SECTION 4: CHORUS (Bars 25 - 40) [60.0s - 100.0s] (16 Bars)
    // The grand climax! Royal Road progression (G - A - F#m - Bm | Em - A - D - D7) x 2.
    // Crashing cymbals, driving slap bass, soaring piano melody with rich chords,
    // chiming rhythm guitar.
    // =========================================================================

    let chorus_chords = [
        // Part 1 (Bars 25 - 32)
        (43, 50, 67, 71, 74), // Gmaj7
        (45, 52, 69, 73, 76), // A7
        (42, 49, 66, 69, 73), // F#m7
        (35, 42, 62, 66, 71), // Bm7
        (40, 47, 64, 67, 71), // Em7
        (45, 52, 69, 73, 76), // A7
        (38, 45, 62, 66, 69), // Dmaj7
        (38, 45, 60, 66, 69), // D7
        // Part 2 (Bars 33 - 40) - Climax Variation!
        (43, 50, 67, 71, 74), // Gmaj7
        (45, 52, 69, 73, 76), // A7
        (46, 53, 64, 70, 73), // F#7/A# (Emotional secondary dominant!)
        (35, 42, 62, 66, 71), // Bm7
        (40, 47, 64, 67, 71), // Em7
        (45, 52, 69, 73, 76), // A7
        (38, 45, 62, 66, 69), // D
        (38, 45, 62, 66, 74), // D (Grand hold)
    ];

    let chorus_bass_roots = [
        31, 33, 30, 35, 28, 33, 26, 26, // G1, A1, F#1, B1, E1, A1, D1, D1
        31, 33, 34, 35, 28, 33, 26, 26, // G1, A1, A#1, B1, E1, A1, D1, D1
    ];

    // High Soaring Melody in Chorus
    let chorus_melody: &[(usize, f64, u8, f64, u8)] = &[
        // Bar 25 (G): D5, D5, E5, F#5
        (0, 0.0, 74, 0.75, 105),
        (0, 0.75, 74, 0.25, 95),
        (0, 1.0, 76, 1.0, 108),
        (0, 2.0, 78, 2.0, 112),
        // Bar 26 (A): E5, D5, E5, F#5
        (1, 0.0, 76, 1.5, 106),
        (1, 1.5, 74, 0.5, 95),
        (1, 2.0, 76, 1.0, 104),
        (1, 3.0, 78, 1.0, 108),
        // Bar 27 (F#m): C#5, C#5, D5, E5
        (2, 0.0, 73, 1.0, 102),
        (2, 1.0, 73, 1.0, 95),
        (2, 2.0, 74, 1.0, 106),
        (2, 3.0, 76, 1.0, 110),
        // Bar 28 (Bm): D5, C#5, B4
        (3, 0.0, 74, 2.0, 108),
        (3, 2.0, 73, 1.0, 100),
        (3, 3.0, 71, 1.0, 96),
        // Bar 29 (Em): B4, D5, E5, G5
        (4, 0.0, 71, 1.0, 98),
        (4, 1.0, 74, 1.0, 104),
        (4, 2.0, 76, 1.0, 108),
        (4, 3.0, 79, 1.0, 114), // G5!
        // Bar 30 (A): F#5, E5, D5, E5
        (5, 0.0, 78, 1.5, 112),
        (5, 1.5, 76, 0.5, 100),
        (5, 2.0, 74, 1.0, 104),
        (5, 3.0, 76, 1.0, 108),
        // Bar 31-32 (D -> D7): D5 (grand hold with flourish)
        (6, 0.0, 74, 3.5, 115),
        (6, 3.5, 76, 0.5, 98),
        (7, 0.0, 78, 1.0, 110),
        (7, 1.0, 79, 1.0, 112),
        (7, 2.0, 81, 2.0, 118), // A5!

        // Part 2 (Bars 33 - 40): Climax iteration
        // Bar 33 (G): B5 peak!
        (8, 0.0, 83, 1.5, 122), // B5!!
        (8, 1.5, 81, 0.5, 110),
        (8, 2.0, 78, 1.0, 112),
        (8, 3.0, 76, 1.0, 108),
        // Bar 34 (A): A5, G5, F#5, E5
        (9, 0.0, 81, 1.5, 118),
        (9, 1.5, 79, 0.5, 106),
        (9, 2.0, 78, 1.0, 110),
        (9, 3.0, 76, 1.0, 106),
        // Bar 35 (F#7/A#): Dramatic tension! F#5, G5, F#5, E5
        (10, 0.0, 78, 2.0, 116),
        (10, 2.0, 79, 1.0, 110),
        (10, 3.0, 78, 1.0, 112),
        // Bar 36 (Bm): D5, F#5, B5
        (11, 0.0, 74, 1.0, 110),
        (11, 1.0, 78, 1.0, 114),
        (11, 2.0, 83, 2.0, 120), // B5!
        // Bar 37 (Em): G5, F#5, E5, D5
        (12, 0.0, 79, 1.5, 114),
        (12, 1.5, 78, 0.5, 105),
        (12, 2.0, 76, 1.0, 108),
        (12, 3.0, 74, 1.0, 104),
        // Bar 38 (A): E5, F#5, E5
        (13, 0.0, 76, 2.0, 112),
        (13, 2.0, 78, 1.0, 114),
        (13, 3.0, 76, 1.0, 108),
        // Bar 39-40 (D): Grand resolution!
        (14, 0.0, 74, 4.0, 118), // D5
        (15, 0.0, 74, 4.0, 112),
    ];

    for bar_i in 0..16 {
        let b = 24 + bar_i;
        let b_start = (b as u64) * BAR;
        comp.add_pedal(b_start, b_start + BAR - 40);

        let (r1, r2, c1, c2, c3) = chorus_chords[bar_i];
        let broot = chorus_bass_roots[bar_i];

        // Piano Left Hand Octaves
        comp.add_piano_note(r1, 100, b_start, BEAT * 2);
        comp.add_piano_note(r1 + 12, 95, b_start, BEAT * 2);
        comp.add_piano_note(r2, 92, b_start + BEAT * 2, BEAT * 2);
        comp.add_piano_note(r2 + 12, 88, b_start + BEAT * 2, BEAT * 2);

        // Piano Mid-Range Harmonic Chords
        comp.add_piano_note(c1, 85, b_start, BEAT);
        comp.add_piano_note(c2, 85, b_start, BEAT);
        comp.add_piano_note(c3, 85, b_start, BEAT);
        comp.add_piano_note(c1, 80, b_start + BEAT * 2, BEAT);
        comp.add_piano_note(c2, 80, b_start + BEAT * 2, BEAT);
        comp.add_piano_note(c3, 80, b_start + BEAT * 2, BEAT);

        // Electric Bass (Punchy Rock Groove: 1, 2&, 3, 4&)
        comp.add_bass_note(broot, 108, b_start, BEAT); // Root on 1
        comp.add_bass_note(broot + 12, 96, b_start + BEAT + HALF_BEAT, HALF_BEAT); // Octave on 2&
        comp.add_bass_note(broot + 7, 102, b_start + BEAT * 2, BEAT); // 5th on 3
        comp.add_bass_note(broot + 12, 94, b_start + BEAT * 3 + HALF_BEAT, HALF_BEAT); // Octave on 4&

        // Guitar Strumming (Full driving presence, off-beat accents)
        for beat_idx in 0..4 {
            let t = b_start + (beat_idx as u64) * BEAT;
            comp.add_guitar_note(c1, 85, t, HALF_BEAT);
            comp.add_guitar_note(c2, 88, t, HALF_BEAT);
            comp.add_guitar_note(c3, 88, t, HALF_BEAT);
            // Upbeat strum
            comp.add_guitar_note(c2, 92, t + HALF_BEAT, HALF_BEAT);
            comp.add_guitar_note(c3, 94, t + HALF_BEAT, HALF_BEAT);
        }

        // Drums (Full Driving Rock Groove)
        // Crash cymbal on downbeat of Bar 25 and Bar 33
        if bar_i == 0 || bar_i == 8 {
            comp.add_drum_hit(49, 122, b_start);
        }

        // Kick Drum: 1, 2&, 3
        comp.add_drum_hit(36, 118, b_start);
        comp.add_drum_hit(36, 108, b_start + BEAT + HALF_BEAT);
        comp.add_drum_hit(36, 114, b_start + BEAT * 2);

        // Snare: 2 and 4 (Solid backbeat!)
        comp.add_drum_hit(38, 116, b_start + BEAT);
        comp.add_drum_hit(38, 118, b_start + BEAT * 3);

        // Ride Cymbal / Open HH driving 8th notes
        for step in 0..8 {
            let t = b_start + (step as u64) * HALF_BEAT;
            let v = if step % 2 == 0 { 92 } else { 76 };
            comp.add_drum_hit(51, v, t); // Ride cymbal
        }

        // Fills at Bar 32 and Bar 40
        if bar_i == 7 || bar_i == 15 {
            comp.add_drum_hit(38, 110, b_start + BEAT * 2);
            comp.add_drum_hit(50, 112, b_start + BEAT * 2 + HALF_BEAT);
            comp.add_drum_hit(47, 115, b_start + BEAT * 3);
            comp.add_drum_hit(43, 120, b_start + BEAT * 3 + HALF_BEAT);
        }
    }

    // Insert Chorus Melody (doubled in octaves for grand piano presence!)
    for &(bar_rel, b_offset, pitch, b_dur, vel) in chorus_melody {
        let t = ((24 + bar_rel) as u64) * BAR + (b_offset * BEAT as f64) as u64;
        let d = (b_dur * BEAT as f64) as u64;
        comp.add_piano_note(pitch, vel, t, d);
        comp.add_piano_note(pitch - 12, (vel as f32 * 0.85) as u8, t, d); // Lower octave support
    }

    // =========================================================================
    // SECTION 5: OUTRO (Bars 41 - 48) [100.0s - 120.0s] (8 Bars)
    // Decrescendo into tranquil peace. Intro arpeggio returns, drums fade,
    // final lingering Dmaj9 chord.
    // Chords: Gmaj7 -> A -> Bm7 -> D | Gmaj7 -> A -> Dmaj9
    // =========================================================================

    for bar_i in 0..8 {
        let b = 40 + bar_i;
        let b_start = (b as u64) * BAR;
        comp.add_pedal(b_start, b_start + BAR - 40);

        let decay = (bar_i as u8) * 4;
        let p_vel = 70u8.saturating_sub(decay).max(45);

        match bar_i {
            0 => {
                // Crash accent + Gmaj7
                comp.add_drum_hit(49, 105, b_start);
                comp.add_piano_note(31, p_vel + 5, b_start, BAR);
                let pattern = [59, 62, 67, 74, 67, 62, 67, 74];
                for (s, &p) in pattern.iter().enumerate() {
                    comp.add_piano_note(p, p_vel, b_start + (s as u64) * HALF_BEAT, HALF_BEAT + 60);
                }
                comp.add_bass_note(31, 80, b_start, BEAT * 2);
            }
            1 => {
                // A
                comp.add_piano_note(33, p_vel + 5, b_start, BAR);
                let pattern = [61, 64, 69, 76, 69, 64, 69, 76];
                for (s, &p) in pattern.iter().enumerate() {
                    comp.add_piano_note(p, p_vel, b_start + (s as u64) * HALF_BEAT, HALF_BEAT + 60);
                }
                comp.add_bass_note(33, 76, b_start, BEAT * 2);
            }
            2 => {
                // Bm7
                comp.add_piano_note(35, p_vel + 5, b_start, BAR);
                let pattern = [62, 66, 71, 74, 71, 66, 71, 74];
                for (s, &p) in pattern.iter().enumerate() {
                    comp.add_piano_note(p, p_vel, b_start + (s as u64) * HALF_BEAT, HALF_BEAT + 60);
                }
                comp.add_bass_note(35, 72, b_start, BEAT * 2);
            }
            3 => {
                // F#m7
                comp.add_piano_note(42, p_vel + 5, b_start, BAR);
                let pattern = [61, 66, 69, 73, 69, 66, 69, 73];
                for (s, &p) in pattern.iter().enumerate() {
                    comp.add_piano_note(p, p_vel, b_start + (s as u64) * HALF_BEAT, HALF_BEAT + 60);
                }
                comp.add_bass_note(42, 68, b_start, BEAT * 2);
            }
            4 => {
                // Gmaj7 (soft)
                comp.add_piano_note(31, p_vel, b_start, BAR);
                let pattern = [59, 62, 67, 71, 67, 62, 67, 71];
                for (s, &p) in pattern.iter().enumerate() {
                    comp.add_piano_note(p, p_vel - 5, b_start + (s as u64) * HALF_BEAT, HALF_BEAT + 60);
                }
                comp.add_guitar_note(74, 55, b_start + BEAT, BEAT * 2); // D5 chime
            }
            5 => {
                // A (fading)
                comp.add_piano_note(33, p_vel, b_start, BAR);
                let pattern = [61, 64, 69, 73, 69, 64, 69, 73];
                for (s, &p) in pattern.iter().enumerate() {
                    comp.add_piano_note(p, p_vel - 8, b_start + (s as u64) * HALF_BEAT, HALF_BEAT + 60);
                }
                comp.add_guitar_note(73, 50, b_start + BEAT, BEAT * 2); // C#5 chime
            }
            6 => {
                // Slower arpeggio D
                comp.add_piano_note(38, 52, b_start, BAR);
                comp.add_piano_note(45, 48, b_start + BEAT, BAR - BEAT);
                comp.add_piano_note(62, 50, b_start + BEAT * 2, BAR - BEAT * 2);
                comp.add_piano_note(66, 48, b_start + BEAT * 3, BAR - BEAT * 3);
            }
            7 => {
                // Final Bar 48: Grand peaceful Dmaj9 chord that resonates and sustains!
                comp.add_pedal(b_start, b_start + BAR * 2); // Extra long pedal hold!
                comp.add_piano_note(26, 68, b_start, BAR * 2); // D1
                comp.add_piano_note(38, 62, b_start + 40, BAR * 2); // D2
                comp.add_piano_note(45, 58, b_start + 80, BAR * 2); // A2
                comp.add_piano_note(54, 56, b_start + 120, BAR * 2); // F#3
                comp.add_piano_note(57, 54, b_start + 160, BAR * 2); // A3
                comp.add_piano_note(61, 52, b_start + 200, BAR * 2); // C#4
                comp.add_piano_note(64, 50, b_start + 240, BAR * 2); // E4
                comp.add_piano_note(66, 48, b_start + 280, BAR * 2); // F#4

                // Delicate final guitar chime
                comp.add_guitar_note(74, 45, b_start + BEAT, BAR);
            }
            _ => {}
        }

        // Soft ride cymbal in early outro
        if bar_i < 4 {
            for beat_idx in 0..4 {
                let t = b_start + (beat_idx as u64) * BEAT;
                let v = 65u8.saturating_sub((bar_i as u8) * 12);
                if v > 20 {
                    comp.add_drum_hit(51, v, t);
                }
            }
        }
    }

    comp
}

// ── Convert to Standard MIDI File (SMF) ───────────────────────────────

fn notes_to_midi_track(
    name: &str,
    channel: u8,
    notes: &[Note],
    pedals: &[ControlEvent],
) -> Vec<TrackEvent<'static>> {
    #[derive(Clone)]
    enum RawEv {
        NoteOn { pitch: u8, vel: u8 },
        NoteOff { pitch: u8 },
        CC { controller: u8, value: u8 },
    }

    let mut raw_events: Vec<(u64, RawEv)> = Vec::new();

    for n in notes {
        raw_events.push((n.start_tick, RawEv::NoteOn { pitch: n.pitch, vel: n.velocity }));
        raw_events.push((n.end_tick, RawEv::NoteOff { pitch: n.pitch }));
    }

    for p in pedals {
        raw_events.push((p.tick, RawEv::CC { controller: p.controller, value: p.value }));
    }

    // Sort by tick. NoteOff before NoteOn if same tick
    raw_events.sort_by(|a, b| {
        if a.0 != b.0 {
            a.0.cmp(&b.0)
        } else {
            match (&a.1, &b.1) {
                (RawEv::NoteOff { .. }, RawEv::NoteOn { .. }) => std::cmp::Ordering::Less,
                (RawEv::NoteOn { .. }, RawEv::NoteOff { .. }) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            }
        }
    });

    let mut track = Vec::new();
    let name_bytes = name.as_bytes().to_vec().into_boxed_slice();
    track.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::TrackName(Box::leak(name_bytes))),
    });

    let mut prev_tick = 0u64;
    for (tick, ev) in raw_events {
        let delta = (tick.saturating_sub(prev_tick)) as u32;
        prev_tick = tick;

        let kind = match ev {
            RawEv::NoteOn { pitch, vel } => TrackEventKind::Midi {
                channel: u4::new(channel),
                message: MidiMessage::NoteOn {
                    key: u7::new(pitch),
                    vel: u7::new(vel),
                },
            },
            RawEv::NoteOff { pitch } => TrackEventKind::Midi {
                channel: u4::new(channel),
                message: MidiMessage::NoteOff {
                    key: u7::new(pitch),
                    vel: u7::new(0),
                },
            },
            RawEv::CC { controller, value } => TrackEventKind::Midi {
                channel: u4::new(channel),
                message: MidiMessage::Controller {
                    controller: u7::new(controller),
                    value: u7::new(value),
                },
            },
        };

        track.push(TrackEvent {
            delta: u28::new(delta),
            kind,
        });
    }

    track.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    track
}

fn main() {
    println!("🎼 Composing 'Dawn of Resonance' (晨曦共鸣) in D Major...");
    let comp = build_composition();

    let total_notes = comp.piano_notes.len()
        + comp.guitar_notes.len()
        + comp.bass_notes.len()
        + comp.drum_notes.len();

    println!("   Total notes generated: {}", total_notes);
    println!("   - Piano notes: {}, Pedal events: {}", comp.piano_notes.len(), comp.piano_pedals.len());
    println!("   - Guitar notes: {}", comp.guitar_notes.len());
    println!("   - Bass notes: {}", comp.bass_notes.len());
    println!("   - Drum notes: {}", comp.drum_notes.len());
    println!("   Duration: 48 bars = 192 beats = 120.0 seconds (2:00) at 96 BPM");

    // 1. Export standard MIDI (.mid)
    let tempo: u32 = 625_000; // 96 BPM = 60_000_000 / 96 = 625,000 us/beat
    let header = Header {
        format: Format::Parallel,
        timing: Timing::Metrical(u15::new(TPB)),
    };

    let tempo_track = vec![
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(b"Tempo")),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(tempo))),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ];

    let t_piano = notes_to_midi_track("Salamander Grand Piano", 0, &comp.piano_notes, &comp.piano_pedals);
    let t_guitar = notes_to_midi_track("Telecaster Guitar", 1, &comp.guitar_notes, &[]);
    let t_bass = notes_to_midi_track("Electric Bass", 2, &comp.bass_notes, &[]);
    let t_drums = notes_to_midi_track("Drum Kit", 9, &comp.drum_notes, &[]);

    let smf = Smf {
        header,
        tracks: vec![tempo_track, t_piano, t_guitar, t_bass, t_drums],
    };

    let mid_path = "dawn_of_resonance.mid";
    let mut buf = Vec::new();
    smf.write(&mut buf).expect("Failed to encode SMF");
    fs::write(mid_path, &buf).expect("Failed to write .mid file");
    println!("✅ Standard MIDI exported: {mid_path}");

    // 2. Export native project file (.midiproj) with dedicated SoundFonts per track
    let sf_piano = "/home/algebnaly/.local/share/midi_player/sf2/SalamanderGrandPiano-V3+20200602.sf2".to_string();
    let sf_guitar = "/home/algebnaly/.local/share/midi_player/sf2/FS Fender Telecaster Electric Guitar Both Pickups and Amp.sf2".to_string();
    let sf_bass = "/home/algebnaly/.local/share/midi_player/sf2/FS Ibanez Electric Bass Guitar.sf2".to_string();
    let sf_drums = "/home/algebnaly/.local/share/midi_player/sf2/GeneralUser-GS.sf2".to_string();

    let project = ProjectFile {
        schema_version: 1,
        midi: MidiData {
            ticks_per_beat: TPB,
            tempo_map: vec![(0, tempo)],
            next_track_id: 4,
            tracks: vec![
                TrackData {
                    id: TrackId(0),
                    name: "Grand Piano".to_string(),
                    notes: comp.piano_notes,
                    control_events: comp.piano_pedals,
                    synth_source: SynthSource::SoundFont { path: sf_piano },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: 0.0, pan: -0.1 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Melodic,
                },
                TrackData {
                    id: TrackId(1),
                    name: "Telecaster Guitar".to_string(),
                    notes: comp.guitar_notes,
                    control_events: Vec::new(),
                    synth_source: SynthSource::SoundFont { path: sf_guitar },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: -1.5, pan: -0.35 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Melodic,
                },
                TrackData {
                    id: TrackId(2),
                    name: "Electric Bass".to_string(),
                    notes: comp.bass_notes,
                    control_events: Vec::new(),
                    synth_source: SynthSource::SoundFont { path: sf_bass },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: 1.5, pan: 0.0 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Melodic,
                },
                TrackData {
                    id: TrackId(3),
                    name: "Drum Kit".to_string(),
                    notes: comp.drum_notes,
                    control_events: Vec::new(),
                    synth_source: SynthSource::SoundFont { path: sf_drums },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: 0.5, pan: 0.0 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Drum(gm_drum_map()),
                },
            ],
        },
    };

    let proj_path = "dawn_of_resonance.midiproj";
    let proj_toml = toml::to_string_pretty(&project).expect("Failed to serialize .midiproj");
    fs::write(proj_path, &proj_toml).expect("Failed to write .midiproj file");
    let loaded: ProjectFile = toml::from_str(&proj_toml).expect("Failed to reload .midiproj via toml deserialization");
    println!("✅ Native Project exported & verified ({loaded_tracks} tracks): {proj_path}", loaded_tracks = loaded.midi.tracks.len());
    println!("\n🎉 Ready to play! Run:");
    println!("   cargo r -r -- dawn_of_resonance.midiproj");
    println!("   or");
    println!("   cargo r -r -- dawn_of_resonance.mid");
}
