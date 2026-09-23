//! Musical composition generator imitating the whimsical chamber acoustic style of out2.mp3.
//! Title: "Whispering Leaves & Pizzicato Dreams" (微风漫步与断奏之境)
//! Key: G Major, Tempo: 112 BPM, Length: 40 bars (~86 seconds).
//!
//! Featuring:
//! 1. Pizzicato Strings (Bank 0, Preset 45) - Bouncy, syncopated, playful staccato
//! 2. Acoustic Guitar (Bank 0, Preset 25) - Warm fingerpicking arpeggios & chord rhythm
//! 3. Grand Piano (Bank 0, Preset 0) - Expressive singing melody & lyrical counterpoint
//! 4. Cello (Bank 0, Preset 42) - Warm sustained tenor counter-melody & singing lead
//! 5. Double Bass (Bank 0, Preset 43) - Grounding acoustic root-fifth pulses & walking steps
//!
//! Run with: cargo run --example generate_whimsical_chamber

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
    ticks_per_beat: u16,
    tempo_map: Vec<(u64, u32)>,
    next_track_id: u64,
    tracks: Vec<TrackData>,
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
    SoundFont {
        path: String,
        #[serde(default)]
        bank: u32,
        #[serde(default)]
        preset: u8,
    },
}

#[derive(Debug, Serialize, Deserialize)]
enum TrackMode {
    Melodic,
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

// ── Composition State ────────────────────────────────────────────────

struct Composition {
    pizzicato_notes: Vec<Note>,
    guitar_notes: Vec<Note>,
    piano_notes: Vec<Note>,
    piano_pedals: Vec<ControlEvent>,
    cello_notes: Vec<Note>,
    bass_notes: Vec<Note>,
}

impl Composition {
    fn new() -> Self {
        Self {
            pizzicato_notes: Vec::new(),
            guitar_notes: Vec::new(),
            piano_notes: Vec::new(),
            piano_pedals: Vec::new(),
            cello_notes: Vec::new(),
            bass_notes: Vec::new(),
        }
    }

    fn add_pizz(&mut self, pitch: u8, vel: u8, tick: u64, len: u64) {
        self.pizzicato_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: tick,
            end_tick: tick + len,
            channel: 0,
        });
    }

    fn add_guitar(&mut self, pitch: u8, vel: u8, tick: u64, len: u64) {
        self.guitar_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: tick,
            end_tick: tick + len,
            channel: 1,
        });
    }

    fn add_piano(&mut self, pitch: u8, vel: u8, tick: u64, len: u64) {
        self.piano_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: tick,
            end_tick: tick + len,
            channel: 2,
        });
    }

    fn add_piano_pedal(&mut self, start_tick: u64, end_tick: u64) {
        self.piano_pedals.push(ControlEvent {
            tick: start_tick,
            channel: 2,
            controller: 64,
            value: 127,
        });
        self.piano_pedals.push(ControlEvent {
            tick: end_tick,
            channel: 2,
            controller: 64,
            value: 0,
        });
    }

    fn add_cello(&mut self, pitch: u8, vel: u8, tick: u64, len: u64) {
        self.cello_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: tick,
            end_tick: tick + len,
            channel: 3,
        });
    }

    fn add_bass(&mut self, pitch: u8, vel: u8, tick: u64, len: u64) {
        self.bass_notes.push(Note {
            pitch,
            velocity: vel,
            start_tick: tick,
            end_tick: tick + len,
            channel: 4,
        });
    }
}

// ── Music Theory Pitch Constants ─────────────────────────────────────
// G Major: G, A, B, C, D, E, F#
const G1: u8 = 31;
const A1: u8 = 33;
const B1: u8 = 35;
const C2: u8 = 36;
const CS2: u8 = 37;
const D2: u8 = 38;
const DS2: u8 = 39;
const E2: u8 = 40;
const FS2: u8 = 42;
const G2: u8 = 43;
const A2: u8 = 45;
const B2: u8 = 47;
const C3: u8 = 48;
const D3: u8 = 50;
const DS3: u8 = 51;
const E3: u8 = 52;
const FS3: u8 = 54;
const G3: u8 = 55;
const A3: u8 = 57;
const B3: u8 = 59;
const C4: u8 = 60;
const D4: u8 = 62;
const DS4: u8 = 63;
const E4: u8 = 64;
const FS4: u8 = 66;
const G4: u8 = 67;
const A4: u8 = 69;
const B4: u8 = 71;
const C5: u8 = 72;
const D5: u8 = 74;
const DS5: u8 = 75;
const E5: u8 = 76;
const FS5: u8 = 78;
const G5: u8 = 79;
const A5: u8 = 81;
const B5: u8 = 83;
const C6: u8 = 84;
const D6: u8 = 86;
const E6: u8 = 88;
const FS6: u8 = 90;
const G6: u8 = 91;
const A6: u8 = 93;
const B6: u8 = 95;
const C7: u8 = 96;
const D7: u8 = 98;

// ── Generator Logic ──────────────────────────────────────────────────

fn build_composition() -> Composition {
    let mut comp = Composition::new();

    // ── Guitar fingerpicking patterns helper
    let guitar_fingerpick = |comp: &mut Composition, bar_idx: u64, root: u8, fifth: u8, mid: u8, high: u8, vel_base: u8| {
        let b = bar_idx * BAR;
        // Beat 1: Root thumb + High note
        comp.add_guitar(root, vel_base + 8, b, HALF_BEAT + 60);
        comp.add_guitar(high, vel_base, b + 15, HALF_BEAT);
        // Beat 1.5: mid string
        comp.add_guitar(mid, vel_base - 6, b + HALF_BEAT, HALF_BEAT);
        // Beat 2: high string + root octave/fifth
        comp.add_guitar(fifth, vel_base - 2, b + BEAT, HALF_BEAT);
        comp.add_guitar(high, vel_base + 2, b + BEAT + QUARTER_BEAT, QUARTER_BEAT + 40);
        // Beat 2.5: mid string
        comp.add_guitar(mid, vel_base - 5, b + BEAT + HALF_BEAT, HALF_BEAT);

        // Beat 3: fifth thumb
        comp.add_guitar(fifth, vel_base + 6, b + 2 * BEAT, HALF_BEAT + 60);
        comp.add_guitar(high, vel_base, b + 2 * BEAT + 15, HALF_BEAT);
        // Beat 3.5: mid string
        comp.add_guitar(mid, vel_base - 4, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        // Beat 4: root / high interplay
        comp.add_guitar(high, vel_base + 4, b + 3 * BEAT, QUARTER_BEAT + 40);
        comp.add_guitar(mid, vel_base - 6, b + 3 * BEAT + QUARTER_BEAT, QUARTER_BEAT + 40);
        comp.add_guitar(fifth, vel_base - 4, b + 3 * BEAT + HALF_BEAT, HALF_BEAT);
    };

    // ── Pizzicato bouncy accompaniment pattern helper
    let pizz_groove = |comp: &mut Composition, bar_idx: u64, p1: u8, p2: u8, p3: u8, p4: u8, vel: u8| {
        let b = bar_idx * BAR;
        let staccato = 120; // Crisp staccato duration
        // Bouncy syncopation: Beat 1, 2, 2.5, 3.5, 4
        comp.add_pizz(p1, vel + 5, b, staccato);
        comp.add_pizz(p2, vel - 4, b + BEAT, staccato);
        comp.add_pizz(p3, vel + 2, b + BEAT + HALF_BEAT, staccato);
        comp.add_pizz(p4, vel + 6, b + 2 * BEAT + HALF_BEAT, staccato);
        comp.add_pizz(p2, vel - 2, b + 3 * BEAT, staccato);
    };

    // ── Bass pulse pattern helper
    let bass_pulse = |comp: &mut Composition, bar_idx: u64, root: u8, fifth: u8, walk: Option<u8>, vel: u8| {
        let b = bar_idx * BAR;
        let note_len = BEAT + HALF_BEAT;
        // Beat 1: Root
        comp.add_bass(root, vel + 6, b, note_len);
        // Beat 3: Fifth
        comp.add_bass(fifth, vel, b + 2 * BEAT, note_len);
        // Optional walk note on Beat 4.5
        if let Some(w) = walk {
            comp.add_bass(w, vel - 4, b + 3 * BEAT + HALF_BEAT, HALF_BEAT);
        }
    };

    // =========================================================================
    // PART 1: INTRO (Bars 0 - 3) [Bars 1-4 in 1-based indexing]
    // =========================================================================
    // Bar 0: Solo Pizzicato sets up the whimsical bouncing motif
    {
        let b = 0 * BAR;
        comp.add_pizz(G4, 82, b, 130);
        comp.add_pizz(B4, 76, b + HALF_BEAT, 130);
        comp.add_pizz(D5, 88, b + BEAT, 130);
        comp.add_pizz(B4, 75, b + BEAT + HALF_BEAT, 130);
        comp.add_pizz(G5, 92, b + 2 * BEAT, 140);
        comp.add_pizz(FS5, 80, b + 2 * BEAT + HALF_BEAT, 130);
        comp.add_pizz(E5, 82, b + 3 * BEAT, 130);
        comp.add_pizz(D5, 78, b + 3 * BEAT + HALF_BEAT, 130);
    }

    // Bar 1: Guitar enters with warm fingerpicking (D/F#), Pizzicato answers
    {
        guitar_fingerpick(&mut comp, 1, FS2, D3, A3, FS4, 72);
        let b = 1 * BAR;
        comp.add_pizz(FS4, 80, b, 130);
        comp.add_pizz(A4, 75, b + BEAT, 130);
        comp.add_pizz(D5, 86, b + BEAT + HALF_BEAT, 130);
        comp.add_pizz(C5, 80, b + 2 * BEAT + HALF_BEAT, 130);
        comp.add_pizz(B4, 76, b + 3 * BEAT, 130);
    }

    // Bar 2: Double Bass & Cello enter softly (Em7)
    {
        guitar_fingerpick(&mut comp, 2, E2, B2, G3, E4, 74);
        pizz_groove(&mut comp, 2, E4, G4, B4, E5, 78);
        bass_pulse(&mut comp, 2, E2, B2, Some(D2), 82);
        // Cello warm long pedal tone
        comp.add_cello(E3, 76, 2 * BAR, BAR - 80);
    }

    // Bar 3: Cadd9 cadence leading into Main Theme; Piano right hand enters with cascade
    {
        guitar_fingerpick(&mut comp, 3, C2, G2, E3, D4, 74);
        pizz_groove(&mut comp, 3, C4, E4, G4, D5, 80);
        bass_pulse(&mut comp, 3, C2, G2, Some(FS2), 84);
        comp.add_cello(G3, 80, 3 * BAR, BAR - 60);

        // Piano sparkling introductory descending flourish into Theme A
        let b = 3 * BAR;
        comp.add_piano(D6, 84, b + 2 * BEAT, QUARTER_BEAT);
        comp.add_piano(B5, 80, b + 2 * BEAT + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(A5, 78, b + 2 * BEAT + HALF_BEAT, QUARTER_BEAT);
        comp.add_piano(G5, 86, b + 2 * BEAT + HALF_BEAT + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(E5, 80, b + 3 * BEAT, QUARTER_BEAT);
        comp.add_piano(FS5, 84, b + 3 * BEAT + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(A5, 88, b + 3 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano_pedal(b + 2 * BEAT, b + BAR);
    }

    // =========================================================================
    // PART 2: SECTION A (Bars 4 - 11) [Theme 1: Sunshine & Playful Promenade]
    // =========================================================================
    // Chord progression:
    // Bar 4: G
    // Bar 5: D/F#
    // Bar 6: Em7
    // Bar 7: Cadd9
    // Bar 8: G
    // Bar 9: Bm7
    // Bar 10: C
    // Bar 11: D7sus4 -> D7

    let chords_a = [
        (G2, D3, B3, G4, G2, D2, Some(FS2), "G"),
        (FS2, D3, A3, FS4, FS2, D2, Some(E2), "D/F#"),
        (E2, B2, G3, E4, E2, B2, Some(D2), "Em7"),
        (C2, G2, E3, D4, C2, G2, Some(B1), "Cadd9"),
        (G2, D3, B3, G4, G2, D2, None, "G"),
        (B2, FS3, D4, B4, B2, FS2, Some(A2), "Bm7"),
        (C2, G2, E3, C4, C2, G2, Some(CS2), "C"),
        (D2, A2, FS3, C4, D2, A2, Some(FS2), "D7"),
    ];

    let cello_notes_a = [
        (D3, 76),
        (A3, 78),
        (G3, 80),
        (E3, 82),
        (D3, 78),
        (FS3, 80),
        (G3, 82),
        (A3, 85),
    ];

    for (i, &(gr, gf, gm, gh, br, bf, bw, _name)) in chords_a.iter().enumerate() {
        let bar_idx = 4 + i as u64;
        let b = bar_idx * BAR;

        // 1. Acoustic Guitar
        guitar_fingerpick(&mut comp, bar_idx, gr, gf, gm, gh, 75);

        // 2. Double Bass
        bass_pulse(&mut comp, bar_idx, br, bf, bw, 86);

        // 3. Cello sustained lyricism
        let (cp, cv) = cello_notes_a[i];
        comp.add_cello(cp, cv, b, BAR - 40);

        // 4. Pizzicato bouncy dialogue
        pizz_groove(&mut comp, bar_idx, gr + 24, gm, gh, gh + 5, 78);
    }

    // Piano Main Theme Melody & Left Hand accompaniment across Bars 4 - 11
    {
        // Bar 4 (G): Singing, sunny motif
        let b = 4 * BAR;
        comp.add_piano(B4, 85, b, HALF_BEAT);
        comp.add_piano(D5, 82, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(G5, 94, b + BEAT, BEAT);
        comp.add_piano(FS5, 84, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(E5, 86, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(D5, 88, b + 3 * BEAT, BEAT);
        // LH harmony chord
        comp.add_piano(G3, 70, b, BEAT * 2);
        comp.add_piano(D4, 68, b, BEAT * 2);
        comp.add_piano(B3, 66, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);

        // Bar 5 (D/F#): Playful turn
        let b = 5 * BAR;
        comp.add_piano(A4, 82, b, HALF_BEAT);
        comp.add_piano(D5, 84, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(FS5, 90, b + BEAT, BEAT);
        comp.add_piano(E5, 82, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(D5, 84, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(B4, 86, b + 3 * BEAT, BEAT);
        comp.add_piano(FS3, 70, b, BEAT * 2);
        comp.add_piano(D4, 68, b, BEAT * 2);
        comp.add_piano(A3, 66, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);

        // Bar 6 (Em7): Bouncing arpeggiation
        let b = 6 * BAR;
        comp.add_piano(G4, 80, b, QUARTER_BEAT);
        comp.add_piano(B4, 82, b + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(E5, 88, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(D5, 84, b + BEAT, HALF_BEAT);
        comp.add_piano(B4, 80, b + BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(A4, 82, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(G4, 84, b + 2 * BEAT + HALF_BEAT, BEAT + HALF_BEAT);
        comp.add_piano(E3, 70, b, BEAT * 2);
        comp.add_piano(B3, 68, b, BEAT * 2);
        comp.add_piano(G3, 66, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);

        // Bar 7 (Cadd9): Ascending warm leap & resolution
        let b = 7 * BAR;
        comp.add_piano(A4, 80, b, HALF_BEAT);
        comp.add_piano(B4, 84, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(D5, 88, b + BEAT, BEAT);
        comp.add_piano(C5, 84, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(B4, 82, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(A4, 86, b + 3 * BEAT, HALF_BEAT);
        comp.add_piano(G4, 82, b + 3 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(C3, 72, b, BEAT * 2);
        comp.add_piano(G3, 70, b, BEAT * 2);
        comp.add_piano(E3, 68, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);

        // Bar 8 (G): Soaring higher octave repeat
        let b = 8 * BAR;
        comp.add_piano(G5, 92, b, HALF_BEAT);
        comp.add_piano(B5, 90, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(A5, 86, b + BEAT, HALF_BEAT);
        comp.add_piano(G5, 92, b + BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(E5, 88, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(D5, 90, b + 2 * BEAT + HALF_BEAT, BEAT + HALF_BEAT);
        comp.add_piano(G3, 72, b, BEAT * 2);
        comp.add_piano(D4, 70, b, BEAT * 2);
        comp.add_piano(B3, 68, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);

        // Bar 9 (Bm7): Sweet harmonic nuance
        let b = 9 * BAR;
        comp.add_piano(FS5, 88, b, HALF_BEAT);
        comp.add_piano(A5, 86, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(G5, 84, b + BEAT, HALF_BEAT);
        comp.add_piano(FS5, 86, b + BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(D5, 88, b + 2 * BEAT, BEAT * 2);
        comp.add_piano(B2, 70, b, BEAT * 2);
        comp.add_piano(FS3, 68, b, BEAT * 2);
        comp.add_piano(D3, 66, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);

        // Bar 10 (C): Joyful lilt
        let b = 10 * BAR;
        comp.add_piano(E5, 86, b, HALF_BEAT);
        comp.add_piano(G5, 88, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(B5, 92, b + BEAT, HALF_BEAT);
        comp.add_piano(A5, 88, b + BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(G5, 86, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(E5, 84, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(D5, 86, b + 3 * BEAT, BEAT);
        comp.add_piano(C3, 72, b, BEAT * 2);
        comp.add_piano(G3, 70, b, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);

        // Bar 11 (D7sus4 -> D7): Cadential flourish
        let b = 11 * BAR;
        comp.add_piano(D5, 86, b, QUARTER_BEAT);
        comp.add_piano(E5, 84, b + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(FS5, 88, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(A5, 92, b + BEAT, BEAT);
        comp.add_piano(C6, 90, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(B5, 86, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(A5, 88, b + 3 * BEAT, BEAT);
        comp.add_piano(D3, 74, b, BEAT * 2);
        comp.add_piano(A3, 72, b, BEAT * 2);
        comp.add_piano(FS3, 70, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);
    }

    // =========================================================================
    // PART 3: SECTION A' (Bars 12 - 19) [Polyphonic Dialogue & Counterpoint]
    // =========================================================================
    // Guitar takes active arpeggiated figures; Cello rises to lyrical tenor lead;
    // Pizzicato plays call-and-response echoes behind piano phrases.
    let chords_a_prime = [
        (G2, D3, B3, G4, G2, D2, Some(FS2)), // 12
        (FS2, D3, A3, FS4, FS2, D2, Some(E2)), // 13
        (E2, B2, G3, E4, E2, B2, Some(D2)), // 14
        (C2, G2, E3, D4, C2, G2, Some(B1)), // 15
        (A2, E3, C4, A4, A2, E2, Some(B1)), // 16 (Am7)
        (B2, FS3, D4, B4, B2, FS2, Some(C2)), // 17 (Bm7)
        (C2, G2, E3, C4, C2, G2, Some(CS2)), // 18 (Cmaj7)
        (D2, A2, FS3, D4, D2, A2, Some(FS2)), // 19 (D7)
    ];

    for (i, &(gr, gf, gm, gh, br, bf, bw)) in chords_a_prime.iter().enumerate() {
        let bar_idx = 12 + i as u64;
        let b = bar_idx * BAR;

        // Acoustic Guitar fingerpicking with higher energy
        guitar_fingerpick(&mut comp, bar_idx, gr, gf, gm, gh, 78);

        // Double Bass
        bass_pulse(&mut comp, bar_idx, br, bf, bw, 88);

        // Pizzicato fast echoes (answering phrases on beats 2 & 4)
        comp.add_pizz(gh, 84, b + HALF_BEAT, 120);
        comp.add_pizz(gm, 80, b + BEAT, 120);
        comp.add_pizz(gh + 4, 88, b + 2 * BEAT + HALF_BEAT, 120);
        comp.add_pizz(gh + 2, 82, b + 3 * BEAT, 120);
    }

    // Cello Counter-melody in Section A' (Bars 12 - 19)
    {
        // Bars 12-15: Expressive counter-line singing in tenor clef
        let b = 12 * BAR;
        comp.add_cello(B3, 82, b, BEAT * 2);
        comp.add_cello(D4, 84, b + 2 * BEAT, BEAT * 2);

        let b = 13 * BAR;
        comp.add_cello(D4, 80, b, BEAT * 2);
        comp.add_cello(C4, 82, b + 2 * BEAT, BEAT * 2);

        let b = 14 * BAR;
        comp.add_cello(B3, 84, b, BEAT);
        comp.add_cello(G3, 80, b + BEAT, BEAT);
        comp.add_cello(E4, 88, b + 2 * BEAT, BEAT * 2);

        let b = 15 * BAR;
        comp.add_cello(D4, 84, b, BEAT * 2);
        comp.add_cello(C4, 82, b + 2 * BEAT, BEAT * 2);

        // Bars 16-19: Stepwise emotional ascent
        let b = 16 * BAR;
        comp.add_cello(A3, 84, b, BEAT * 2);
        comp.add_cello(C4, 86, b + 2 * BEAT, BEAT * 2);

        let b = 17 * BAR;
        comp.add_cello(B3, 86, b, BEAT * 2);
        comp.add_cello(D4, 88, b + 2 * BEAT, BEAT * 2);

        let b = 18 * BAR;
        comp.add_cello(E4, 90, b, BEAT * 2);
        comp.add_cello(G4, 92, b + 2 * BEAT, BEAT * 2);

        let b = 19 * BAR;
        comp.add_cello(FS4, 94, b, BEAT * 3);
        comp.add_cello(A4, 90, b + 3 * BEAT, BEAT);
    }

    // Piano playful variations in Section A' (Bars 12 - 19)
    {
        let b = 12 * BAR;
        comp.add_piano(G5, 90, b, HALF_BEAT);
        comp.add_piano(D5, 84, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(B4, 86, b + BEAT, HALF_BEAT);
        comp.add_piano(D5, 88, b + BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(G5, 94, b + 2 * BEAT, BEAT);
        comp.add_piano(A5, 88, b + 3 * BEAT, BEAT);
        comp.add_piano_pedal(b, b + BAR - 40);

        let b = 13 * BAR;
        comp.add_piano(FS5, 88, b, HALF_BEAT);
        comp.add_piano(D5, 84, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(A4, 82, b + BEAT, HALF_BEAT);
        comp.add_piano(D5, 86, b + BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(FS5, 92, b + 2 * BEAT, BEAT);
        comp.add_piano(G5, 86, b + 3 * BEAT, BEAT);
        comp.add_piano_pedal(b, b + BAR - 40);

        let b = 14 * BAR;
        comp.add_piano(E5, 88, b, HALF_BEAT);
        comp.add_piano(G5, 90, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(B5, 96, b + BEAT, BEAT);
        comp.add_piano(A5, 88, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(G5, 86, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(E5, 84, b + 3 * BEAT, BEAT);
        comp.add_piano_pedal(b, b + BAR - 40);

        let b = 15 * BAR;
        comp.add_piano(D5, 86, b, HALF_BEAT);
        comp.add_piano(E5, 88, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(G5, 92, b + BEAT, BEAT);
        comp.add_piano(E5, 84, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(D5, 86, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(C5, 82, b + 3 * BEAT, BEAT);
        comp.add_piano_pedal(b, b + BAR - 40);

        let b = 16 * BAR;
        comp.add_piano(C5, 84, b, HALF_BEAT);
        comp.add_piano(E5, 88, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(A5, 92, b + BEAT, BEAT);
        comp.add_piano(G5, 88, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(FS5, 84, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(E5, 82, b + 3 * BEAT, BEAT);
        comp.add_piano_pedal(b, b + BAR - 40);

        let b = 17 * BAR;
        comp.add_piano(D5, 86, b, HALF_BEAT);
        comp.add_piano(FS5, 88, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(B5, 94, b + BEAT, BEAT);
        comp.add_piano(A5, 88, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(G5, 86, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(FS5, 84, b + 3 * BEAT, BEAT);
        comp.add_piano_pedal(b, b + BAR - 40);

        let b = 18 * BAR;
        comp.add_piano(G5, 90, b, HALF_BEAT);
        comp.add_piano(B5, 92, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(D6, 98, b + BEAT, BEAT);
        comp.add_piano(C6, 90, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(B5, 88, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(A5, 86, b + 3 * BEAT, BEAT);
        comp.add_piano_pedal(b, b + BAR - 40);

        let b = 19 * BAR;
        comp.add_piano(FS5, 88, b, QUARTER_BEAT);
        comp.add_piano(G5, 90, b + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(A5, 94, b + HALF_BEAT, HALF_BEAT);
        comp.add_piano(C6, 96, b + BEAT, BEAT);
        comp.add_piano(B5, 90, b + 2 * BEAT, HALF_BEAT);
        comp.add_piano(A5, 88, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_piano(G5, 86, b + 3 * BEAT, BEAT);
        comp.add_piano_pedal(b, b + BAR - 40);
    }

    // =========================================================================
    // PART 4: SECTION B (Bars 20 - 27) [Tender Reflection & Emotional Shift]
    // =========================================================================
    // Minor shift (Em -> Bm/D -> Cmaj7 -> G/B -> Am7 -> B7/D# -> Em -> C/D)
    // Cello soars with the primary vocal theme; Pizzicato plays soft clockwork ticking;
    // Piano plays high, bell-like twinkling arpeggios.
    let chords_b = [
        (E2, B2, G3, E4, E2, B2, Some(D2)), // 20: Em
        (D2, A2, FS3, D4, D2, A2, Some(C2)), // 21: Bm/D
        (C2, G2, E3, C4, C2, G2, Some(B1)), // 22: Cmaj7
        (B1, FS2, D3, B3, B1, FS2, Some(A1)), // 23: G/B
        (A1, E2, C3, A3, A1, E2, Some(B1)), // 24: Am7
        (DS2, B2, FS3, DS4, DS2, B2, Some(E2)), // 25: B7/D# (Chromatic lift!)
        (E2, B2, G3, E4, E2, B2, Some(D2)), // 26: Em -> Em/D
        (C2, G2, E3, D4, C2, D2, Some(FS2)), // 27: Cmaj7 -> D7sus4 (Crescendo!)
    ];

    for (i, &(gr, gf, gm, gh, br, bf, bw)) in chords_b.iter().enumerate() {
        let bar_idx = 20 + i as u64;
        let b = bar_idx * BAR;

        // Gentle guitar arpeggio
        guitar_fingerpick(&mut comp, bar_idx, gr, gf, gm, gh, 68);

        // Bass
        bass_pulse(&mut comp, bar_idx, br, bf, bw, 80);

        // Pizzicato: clockwork steady 8th-note pulses like rain drops on leaves
        for step in 0..8 {
            let t = b + step * HALF_BEAT;
            let p = if step % 2 == 0 { gm } else { gh };
            comp.add_pizz(p, 68 + (step as u8 % 3) * 3, t, 110);
        }
    }

    // Cello Primary Melody in Section B (Bars 20 - 27) - Expressive and Deep
    {
        // Bar 20: Em
        let b = 20 * BAR;
        comp.add_cello(E3, 85, b, HALF_BEAT);
        comp.add_cello(G3, 84, b + HALF_BEAT, HALF_BEAT);
        comp.add_cello(B3, 90, b + BEAT, BEAT);
        comp.add_cello(D4, 92, b + 2 * BEAT, HALF_BEAT);
        comp.add_cello(C4, 88, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_cello(B3, 86, b + 3 * BEAT, BEAT);

        // Bar 21: Bm/D
        let b = 21 * BAR;
        comp.add_cello(FS3, 84, b, HALF_BEAT);
        comp.add_cello(A3, 82, b + HALF_BEAT, HALF_BEAT);
        comp.add_cello(D4, 88, b + BEAT, BEAT);
        comp.add_cello(C4, 86, b + 2 * BEAT, HALF_BEAT);
        comp.add_cello(B3, 84, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_cello(A3, 86, b + 3 * BEAT, BEAT);

        // Bar 22: Cmaj7
        let b = 22 * BAR;
        comp.add_cello(G3, 86, b, HALF_BEAT);
        comp.add_cello(B3, 88, b + HALF_BEAT, HALF_BEAT);
        comp.add_cello(D4, 92, b + BEAT, BEAT);
        comp.add_cello(E4, 96, b + 2 * BEAT, HALF_BEAT);
        comp.add_cello(D4, 90, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_cello(C4, 88, b + 3 * BEAT, BEAT);

        // Bar 23: G/B
        let b = 23 * BAR;
        comp.add_cello(B3, 88, b, HALF_BEAT);
        comp.add_cello(D4, 90, b + HALF_BEAT, HALF_BEAT);
        comp.add_cello(G4, 96, b + BEAT, BEAT);
        comp.add_cello(FS4, 90, b + 2 * BEAT, HALF_BEAT);
        comp.add_cello(E4, 88, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_cello(D4, 90, b + 3 * BEAT, BEAT);

        // Bar 24: Am7
        let b = 24 * BAR;
        comp.add_cello(C4, 88, b, HALF_BEAT);
        comp.add_cello(E4, 92, b + HALF_BEAT, HALF_BEAT);
        comp.add_cello(A4, 98, b + BEAT, BEAT);
        comp.add_cello(G4, 92, b + 2 * BEAT, HALF_BEAT);
        comp.add_cello(E4, 88, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_cello(C4, 86, b + 3 * BEAT, BEAT);

        // Bar 25: B7/D# (Chromatic tension!)
        let b = 25 * BAR;
        comp.add_cello(B3, 88, b, HALF_BEAT);
        comp.add_cello(DS4, 94, b + HALF_BEAT, HALF_BEAT);
        comp.add_cello(FS4, 98, b + BEAT, BEAT);
        comp.add_cello(A4, 96, b + 2 * BEAT, HALF_BEAT);
        comp.add_cello(G4, 92, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_cello(FS4, 94, b + 3 * BEAT, BEAT);

        // Bar 26: Em -> Em/D (Resolution)
        let b = 26 * BAR;
        comp.add_cello(G4, 96, b, HALF_BEAT);
        comp.add_cello(E4, 92, b + HALF_BEAT, HALF_BEAT);
        comp.add_cello(B3, 88, b + BEAT, BEAT);
        comp.add_cello(D4, 90, b + 2 * BEAT, HALF_BEAT);
        comp.add_cello(C4, 88, b + 2 * BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_cello(B3, 86, b + 3 * BEAT, BEAT);

        // Bar 27: C -> D7 (Soaring upward swell into Climax!)
        let b = 27 * BAR;
        comp.add_cello(C4, 90, b, HALF_BEAT);
        comp.add_cello(D4, 92, b + HALF_BEAT, HALF_BEAT);
        comp.add_cello(E4, 96, b + BEAT, HALF_BEAT);
        comp.add_cello(FS4, 98, b + BEAT + HALF_BEAT, HALF_BEAT);
        comp.add_cello(G4, 100, b + 2 * BEAT, BEAT);
        comp.add_cello(A4, 104, b + 3 * BEAT, BEAT);
    }

    // Piano Sparkling Celesta/Bell figures in Section B (Bars 20 - 27)
    {
        for bar_idx in 20..28 {
            let b = bar_idx * BAR;
            // Twinkling high notes in Octave 6
            let p_high = match bar_idx {
                20 => (E6, G6),
                21 => (D6, FS6),
                22 => (C6, E6),
                23 => (B5, D6),
                24 => (A5, C6),
                25 => (FS5, B5),
                26 => (G5, E6),
                _ => (A5, D6),
            };
            comp.add_piano(p_high.0, 78, b + BEAT, HALF_BEAT);
            comp.add_piano(p_high.1, 82, b + BEAT + HALF_BEAT, HALF_BEAT);
            comp.add_piano(p_high.0, 76, b + 3 * BEAT, HALF_BEAT);
            comp.add_piano(p_high.1, 80, b + 3 * BEAT + HALF_BEAT, HALF_BEAT);
            comp.add_piano_pedal(b, b + BAR - 40);
        }
    }

    // =========================================================================
    // PART 5: SECTION A'' (Bars 28 - 35) [Joyful Chamber Tutti Climax]
    // =========================================================================
    // Full polyphonic bloom!
    // Piano plays melodic octaves with powerful left-hand accompaniment.
    // Pizzicato strings play dancing syncopated runs.
    // Cello belts triumphant singing tenor counterpoint.
    // Double bass walks actively with punchy acoustic clarity.
    // Guitar strums richly with wide stereo fullness.

    let chords_climax = [
        (G2, D3, B3, G4, G2, D2, Some(FS2)), // 28
        (FS2, D3, A3, FS4, FS2, D2, Some(E2)), // 29
        (E2, B2, G3, E4, E2, B2, Some(D2)), // 30
        (C2, G2, E3, D4, C2, G2, Some(B1)), // 31
        (B1, FS2, D3, G3, B1, FS2, Some(A1)), // 32 (G/B)
        (C2, G2, E3, C4, C2, G2, Some(D2)), // 33 (C)
        (A1, E2, C3, A3, A1, E2, Some(FS2)), // 34 (Am7 -> D7)
        (G2, D3, B3, G4, G2, D2, None), // 35 (G cadence)
    ];

    for (i, &(gr, gf, gm, gh, br, bf, bw)) in chords_climax.iter().enumerate() {
        let bar_idx = 28 + i as u64;
        let b = bar_idx * BAR;

        // Rich strummed guitar texture
        guitar_fingerpick(&mut comp, bar_idx, gr, gf, gm, gh, 85);

        // Powerful walking bass
        bass_pulse(&mut comp, bar_idx, br, bf, bw, 95);

        // Virtuosic, joyful Pizzicato staccato runs
        let staccato = 120;
        comp.add_pizz(gr + 24, 88, b, staccato);
        comp.add_pizz(gm + 12, 85, b + QUARTER_BEAT, staccato);
        comp.add_pizz(gh + 12, 92, b + HALF_BEAT, staccato);
        comp.add_pizz(gm + 12, 84, b + HALF_BEAT + QUARTER_BEAT, staccato);
        comp.add_pizz(gh + 12, 95, b + BEAT, staccato);
        comp.add_pizz(gr + 36, 98, b + BEAT + HALF_BEAT, staccato);
        comp.add_pizz(gh + 12, 90, b + 2 * BEAT + HALF_BEAT, staccato);
        comp.add_pizz(gm + 12, 86, b + 3 * BEAT, staccato);
    }

    // Piano Climax in Octaves (Bars 28 - 35)
    let piano_melody_climax = [
        // Bar 28 (G):
        vec![(B4, B5, 96, 0, HALF_BEAT), (D5, D6, 94, HALF_BEAT, HALF_BEAT), (G5, G6, 105, BEAT, BEAT), (FS5, FS6, 96, 2 * BEAT, HALF_BEAT), (E5, E6, 98, 2 * BEAT + HALF_BEAT, HALF_BEAT), (D5, D6, 100, 3 * BEAT, BEAT)],
        // Bar 29 (D/F#):
        vec![(A4, A5, 94, 0, HALF_BEAT), (D5, D6, 96, HALF_BEAT, HALF_BEAT), (FS5, FS6, 102, BEAT, BEAT), (E5, E6, 95, 2 * BEAT, HALF_BEAT), (D5, D6, 96, 2 * BEAT + HALF_BEAT, HALF_BEAT), (B4, B5, 98, 3 * BEAT, BEAT)],
        // Bar 30 (Em7):
        vec![(G4, G5, 92, 0, QUARTER_BEAT), (B4, B5, 94, QUARTER_BEAT, QUARTER_BEAT), (E5, E6, 100, HALF_BEAT, HALF_BEAT), (D5, D6, 96, BEAT, HALF_BEAT), (B4, B5, 94, BEAT + HALF_BEAT, HALF_BEAT), (A4, A5, 95, 2 * BEAT, HALF_BEAT), (G4, G5, 96, 2 * BEAT + HALF_BEAT, BEAT + HALF_BEAT)],
        // Bar 31 (Cadd9):
        vec![(A4, A5, 94, 0, HALF_BEAT), (B4, B5, 96, HALF_BEAT, HALF_BEAT), (D5, D6, 102, BEAT, BEAT), (C5, C6, 96, 2 * BEAT, HALF_BEAT), (B4, B5, 94, 2 * BEAT + HALF_BEAT, HALF_BEAT), (A4, A5, 96, 3 * BEAT, HALF_BEAT), (G4, G5, 94, 3 * BEAT + HALF_BEAT, HALF_BEAT)],
        // Bar 32 (G/B):
        vec![(B4, B5, 96, 0, HALF_BEAT), (D5, D6, 98, HALF_BEAT, HALF_BEAT), (G5, G6, 106, BEAT, BEAT), (B5, B6, 108, 2 * BEAT, HALF_BEAT), (A5, A6, 102, 2 * BEAT + HALF_BEAT, HALF_BEAT), (G5, G6, 104, 3 * BEAT, BEAT)],
        // Bar 33 (C):
        vec![(E5, E6, 100, 0, HALF_BEAT), (G5, G6, 104, HALF_BEAT, HALF_BEAT), (C6, C7, 108, BEAT, BEAT), (B5, B6, 102, 2 * BEAT, HALF_BEAT), (A5, A6, 100, 2 * BEAT + HALF_BEAT, HALF_BEAT), (G5, G6, 98, 3 * BEAT, BEAT)],
        // Bar 34 (Am7 -> D7):
        vec![(A4, A5, 98, 0, HALF_BEAT), (C5, C6, 100, HALF_BEAT, HALF_BEAT), (E5, E6, 104, BEAT, HALF_BEAT), (FS5, FS6, 106, BEAT + HALF_BEAT, HALF_BEAT), (D6, D7, 110, 2 * BEAT, BEAT), (C6, C7, 102, 3 * BEAT, BEAT)],
        // Bar 35 (G): Grand Resolution!
        vec![(B5, B6, 106, 0, BEAT), (A5, A6, 100, BEAT, BEAT), (G5, G6, 112, 2 * BEAT, 2 * BEAT)],
    ];

    for (i, notes) in piano_melody_climax.iter().enumerate() {
        let bar_idx = 28 + i as u64;
        let b = bar_idx * BAR;
        for &(p_low, p_high, vel, offset, dur) in notes {
            comp.add_piano(p_low, vel - 6, b + offset, dur);
            comp.add_piano(p_high, vel, b + offset, dur);
        }
        // LH full supportive chords
        let lh_root = match i {
            0 => G2,
            1 => FS2,
            2 => E2,
            3 => C2,
            4 => B1,
            5 => C2,
            6 => D2,
            _ => G2,
        };
        comp.add_piano(lh_root, 82, b, BEAT * 2);
        comp.add_piano(lh_root + 7, 78, b, BEAT * 2);
        comp.add_piano(lh_root + 12, 76, b, BEAT * 2);
        comp.add_piano(lh_root, 80, b + 2 * BEAT, BEAT * 2);
        comp.add_piano(lh_root + 7, 76, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);
    }

    // Cello Soaring Counterpoint in Climax (Bars 28 - 35)
    {
        let cello_climax_notes = [
            (D4, 92),
            (FS4, 94),
            (G4, 98),
            (E4, 95),
            (D4, 96),
            (E4, 98),
            (FS4, 102),
            (G4, 105),
        ];
        for (i, &(cp, cv)) in cello_climax_notes.iter().enumerate() {
            let bar_idx = 28 + i as u64;
            let b = bar_idx * BAR;
            comp.add_cello(cp, cv, b, BAR - 40);
        }
    }

    // =========================================================================
    // PART 6: OUTRO / CODA (Bars 36 - 39) [Gentle Twilight & Dissolving Echoes]
    // =========================================================================
    // Bar 36: G - Calming down
    {
        let b = 36 * BAR;
        guitar_fingerpick(&mut comp, 36, G2, D3, B3, G4, 70);
        bass_pulse(&mut comp, 36, G2, D2, None, 78);
        comp.add_cello(G3, 76, b, BAR - 40);
        comp.add_piano(B4, 80, b, BEAT);
        comp.add_piano(G4, 78, b + BEAT, BEAT);
        comp.add_piano(D4, 74, b + 2 * BEAT, BEAT * 2);
        comp.add_piano_pedal(b, b + BAR - 40);
    }

    // Bar 37: C/G - Gentle nostalgic sway
    {
        let b = 37 * BAR;
        guitar_fingerpick(&mut comp, 37, G2, E3, C4, G4, 66);
        bass_pulse(&mut comp, 37, G2, C2, None, 74);
        comp.add_cello(E3, 72, b, BAR - 40);
        // Pizzicato echoes the opening motif softly
        comp.add_pizz(G4, 75, b, 130);
        comp.add_pizz(B4, 70, b + HALF_BEAT, 130);
        comp.add_pizz(D5, 78, b + BEAT, 130);
        comp.add_pizz(B4, 70, b + BEAT + HALF_BEAT, 130);
    }

    // Bar 38: G - Fading into stillness
    {
        let b = 38 * BAR;
        comp.add_guitar(G2, 70, b, 2 * BEAT);
        comp.add_guitar(D3, 66, b + HALF_BEAT, 2 * BEAT);
        comp.add_guitar(B3, 64, b + BEAT, 2 * BEAT);
        comp.add_guitar(G4, 68, b + BEAT + HALF_BEAT, 2 * BEAT);
        comp.add_bass(G1, 75, b, 3 * BEAT);
        comp.add_cello(D3, 70, b, BAR - 40);

        // Piano gentle descending waterfall
        comp.add_piano(D6, 76, b, QUARTER_BEAT);
        comp.add_piano(B5, 74, b + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(G5, 72, b + HALF_BEAT, QUARTER_BEAT);
        comp.add_piano(E5, 70, b + HALF_BEAT + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(D5, 72, b + BEAT, QUARTER_BEAT);
        comp.add_piano(B4, 68, b + BEAT + QUARTER_BEAT, QUARTER_BEAT);
        comp.add_piano(G4, 74, b + BEAT + HALF_BEAT, BEAT + HALF_BEAT);
        comp.add_piano_pedal(b, b + BAR);
    }

    // Bar 39: Final Chord & Pizzicato Twinkle
    {
        let b = 39 * BAR;
        // Deep acoustic bass root
        comp.add_bass(G1, 80, b, BAR);
        // Cello warm root & fifth
        comp.add_cello(G2, 74, b, BAR);
        comp.add_cello(D3, 72, b, BAR);
        // Acoustic guitar gentle rolled chord
        comp.add_guitar(G2, 72, b, BAR);
        comp.add_guitar(D3, 68, b + 15, BAR);
        comp.add_guitar(G3, 66, b + 30, BAR);
        comp.add_guitar(B3, 65, b + 45, BAR);
        comp.add_guitar(D4, 68, b + 60, BAR);
        // Piano resonant Gadd9 chord
        comp.add_piano(G2, 75, b, BAR);
        comp.add_piano(D3, 72, b, BAR);
        comp.add_piano(G3, 70, b, BAR);
        comp.add_piano(B3, 70, b, BAR);
        comp.add_piano(D4, 72, b, BAR);
        comp.add_piano(A4, 74, b, BAR);
        comp.add_piano(B4, 76, b, BAR);
        comp.add_piano_pedal(b, b + BAR);

        // Final magical pizzicato high bell on the offbeat
        comp.add_pizz(G6, 88, b + 2 * BEAT + HALF_BEAT, 240);
    }

    comp
}

// ── SMF Track Exporter ───────────────────────────────────────────────

enum RawEv {
    NoteOn { pitch: u8, vel: u8 },
    NoteOff { pitch: u8 },
    CC { controller: u8, value: u8 },
}

fn notes_to_midi_track(
    name: &str,
    channel: u8,
    program: u8,
    notes: &[Note],
    pedals: &[ControlEvent],
) -> Vec<TrackEvent<'static>> {
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

    // Send initial ProgramChange event for General MIDI compatibility
    track.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Midi {
            channel: u4::new(channel),
            message: MidiMessage::ProgramChange {
                program: u7::new(program),
            },
        },
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
    println!("🎼 Composing 'Whispering Leaves & Pizzicato Dreams' (微风漫步与断奏之境)...");
    println!("   Style: Whimsical Chamber Acoustic OST (imitating out2.mp3)");
    println!("   Key: G Major | Tempo: 112 BPM | Length: 40 bars (~86 seconds)");

    let comp = build_composition();

    let total_notes = comp.pizzicato_notes.len()
        + comp.guitar_notes.len()
        + comp.piano_notes.len()
        + comp.cello_notes.len()
        + comp.bass_notes.len();

    println!("   Total notes generated: {}", total_notes);
    println!("   - Pizzicato Strings: {} notes (Preset 45)", comp.pizzicato_notes.len());
    println!("   - Acoustic Guitar: {} notes (Preset 25)", comp.guitar_notes.len());
    println!("   - Grand Piano: {} notes, {} pedal events (Preset 0)", comp.piano_notes.len(), comp.piano_pedals.len());
    println!("   - Cello: {} notes (Preset 42)", comp.cello_notes.len());
    println!("   - Double Bass: {} notes (Preset 43)", comp.bass_notes.len());

    // 1. Export Standard MIDI file (.mid)
    let tempo: u32 = 535_714; // 112 BPM = 60_000_000 / 112 ≈ 535,714 us/beat
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

    let t_pizz = notes_to_midi_track("Pizzicato Strings", 0, 45, &comp.pizzicato_notes, &[]);
    let t_guitar = notes_to_midi_track("Acoustic Guitar", 1, 25, &comp.guitar_notes, &[]);
    let t_piano = notes_to_midi_track("Grand Piano", 2, 0, &comp.piano_notes, &comp.piano_pedals);
    let t_cello = notes_to_midi_track("Cello", 3, 42, &comp.cello_notes, &[]);
    let t_bass = notes_to_midi_track("Double Bass", 4, 43, &comp.bass_notes, &[]);

    let smf = Smf {
        header,
        tracks: vec![tempo_track, t_pizz, t_guitar, t_piano, t_cello, t_bass],
    };

    let mid_path = "whispering_leaves.mid";
    let mut buf = Vec::new();
    smf.write(&mut buf).expect("Failed to encode SMF");
    fs::write(mid_path, &buf).expect("Failed to write .mid file");
    println!("✅ Standard MIDI exported: {mid_path}");

    // 2. Export native project file (.midiproj) with presets configured for GeneralUser-GS.sf2
    let sf_path = "/home/algebnaly/.local/share/midi_player/sf2/GeneralUser-GS.sf2".to_string();

    let project = ProjectFile {
        schema_version: 1,
        midi: MidiData {
            ticks_per_beat: TPB,
            tempo_map: vec![(0, tempo)],
            next_track_id: 6,
            tracks: vec![
                TrackData {
                    id: TrackId(1),
                    name: "Pizzicato Strings".to_string(),
                    notes: comp.pizzicato_notes,
                    control_events: Vec::new(),
                    synth_source: SynthSource::SoundFont {
                        path: sf_path.clone(),
                        bank: 0,
                        preset: 45,
                    },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: 0.5, pan: 0.25 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Melodic,
                },
                TrackData {
                    id: TrackId(2),
                    name: "Acoustic Guitar".to_string(),
                    notes: comp.guitar_notes,
                    control_events: Vec::new(),
                    synth_source: SynthSource::SoundFont {
                        path: sf_path.clone(),
                        bank: 0,
                        preset: 25,
                    },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: -0.5, pan: -0.30 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Melodic,
                },
                TrackData {
                    id: TrackId(3),
                    name: "Grand Piano".to_string(),
                    notes: comp.piano_notes,
                    control_events: comp.piano_pedals,
                    synth_source: SynthSource::SoundFont {
                        path: sf_path.clone(),
                        bank: 0,
                        preset: 0,
                    },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: 0.0, pan: -0.05 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Melodic,
                },
                TrackData {
                    id: TrackId(4),
                    name: "Cello".to_string(),
                    notes: comp.cello_notes,
                    control_events: Vec::new(),
                    synth_source: SynthSource::SoundFont {
                        path: sf_path.clone(),
                        bank: 0,
                        preset: 42,
                    },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: -0.5, pan: -0.15 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Melodic,
                },
                TrackData {
                    id: TrackId(5),
                    name: "Double Bass".to_string(),
                    notes: comp.bass_notes,
                    control_events: Vec::new(),
                    synth_source: SynthSource::SoundFont {
                        path: sf_path,
                        bank: 0,
                        preset: 43,
                    },
                    mixer: TrackMixerSettings { mute: false, solo: false, volume_db: 1.0, pan: 0.0 },
                    input: TrackInputSettings { armed: false, channel_filter: None, transpose: 0 },
                    mode: TrackMode::Melodic,
                },
            ],
        },
    };

    let proj_path = "whispering_leaves.midiproj";
    let proj_toml = toml::to_string_pretty(&project).expect("Failed to serialize .midiproj");
    fs::write(proj_path, &proj_toml).expect("Failed to write .midiproj file");
    let loaded: ProjectFile = toml::from_str(&proj_toml).expect("Failed to reload .midiproj");
    println!("✅ Native Project exported & verified ({} tracks): {proj_path}", loaded.midi.tracks.len());

    println!("\n🎉 Playable with midi_player! Run command:");
    println!("   cargo r -r -- whispering_leaves.midiproj");
    println!("   or");
    println!("   cargo r -r -- whispering_leaves.mid");
}
