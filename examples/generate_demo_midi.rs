//! Generate a demo MIDI file with drums, bass, and piano tracks.
//!
//! Usage: cargo run --example generate_demo_midi

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use std::fs;

fn main() {
    let tpb: u16 = 480; // ticks per beat
    let tempo: u32 = 500_000; // 120 BPM

    let header = Header {
        format: Format::Parallel,
        timing: Timing::Metrical(u15::new(tpb)),
    };

    // ── Track 0: Tempo map ────────────────────────────────────────
    let tempo_track = vec![
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(tempo))),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ];

    // ── Track 1: Piano (channel 0) ────────────────────────────────
    let piano_track = build_piano_track(tpb);

    // ── Track 2: Bass (channel 1) ─────────────────────────────────
    let bass_track = build_bass_track(tpb);

    // ── Track 3: Drums (channel 9) ────────────────────────────────
    let drum_track = build_drum_track(tpb);

    let smf = Smf {
        header,
        tracks: vec![tempo_track, piano_track, bass_track, drum_track],
    };

    let path = "demo_with_drums.mid";
    let mut buf = Vec::new();
    smf.write(&mut buf).expect("Failed to write MIDI");
    fs::write(path, &buf).expect("Failed to save file");
    println!("✅ Generated: {path}");
    println!("   Tracks: Piano (ch0), Bass (ch1), Drums (ch9)");
    println!("   Tempo: 120 BPM, Length: 8 bars");
}

// ── Helper: note on/off events ────────────────────────────────────

fn note_on(delta: u32, ch: u8, pitch: u8, vel: u8) -> TrackEvent<'static> {
    TrackEvent {
        delta: u28::new(delta),
        kind: TrackEventKind::Midi {
            channel: u4::new(ch),
            message: MidiMessage::NoteOn {
                key: u7::new(pitch),
                vel: u7::new(vel),
            },
        },
    }
}

fn note_off(delta: u32, ch: u8, pitch: u8) -> TrackEvent<'static> {
    TrackEvent {
        delta: u28::new(delta),
        kind: TrackEventKind::Midi {
            channel: u4::new(ch),
            message: MidiMessage::NoteOff {
                key: u7::new(pitch),
                vel: u7::new(0),
            },
        },
    }
}

fn track_name(name: &[u8]) -> TrackEvent<'_> {
    TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::TrackName(name)),
    }
}

fn end_of_track() -> TrackEvent<'static> {
    TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    }
}

// ── Piano track: simple chord progression ─────────────────────────
// Am - F - C - G  (2 bars each, repeated)
fn build_piano_track(tpb: u16) -> Vec<TrackEvent<'static>> {
    let mut events: Vec<TrackEvent<'static>> = vec![];
    // Safety: track_name borrows, so we push note events separately
    events.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::TrackName(b"Piano")),
    });

    let ch = 0u8;
    let half = tpb as u32 * 2; // half note duration
    let bar = tpb as u32 * 4;

    // Chord definitions: (root, pitches)
    let chords: [(u8, u8, u8); 4] = [
        (57, 60, 64), // Am: A3, C4, E4
        (53, 57, 60), // F:  F3, A3, C4
        (48, 52, 55), // C:  C3, E3, G3
        (43, 47, 50), // G:  G2, B2, D3
    ];

    // Play 8 bars = 2 repetitions of 4-chord progression
    for _ in 0..2 {
        for &(p1, p2, p3) in &chords {
            // Two half-note chords per bar pair (= 2 bars)
            for _ in 0..2 {
                // Chord on
                events.push(note_on(0, ch, p1, 80));
                events.push(note_on(0, ch, p2, 75));
                events.push(note_on(0, ch, p3, 75));
                // Chord off after half note
                events.push(note_off(half, ch, p1));
                events.push(note_off(0, ch, p2));
                events.push(note_off(0, ch, p3));
            }
        }
    }

    events.push(end_of_track());
    events
}

// ── Bass track: root note patterns ────────────────────────────────
fn build_bass_track(tpb: u16) -> Vec<TrackEvent<'static>> {
    let mut events: Vec<TrackEvent<'static>> = vec![];
    events.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::TrackName(b"Bass")),
    });

    let ch = 1u8;
    let quarter = tpb as u32;
    let eighth = tpb as u32 / 2;

    // Bass root notes matching chord progression
    // Am(A2=45), F(F2=41), C(C2=36), G(G2=31)
    let roots: [u8; 4] = [45, 41, 36, 43];

    for _ in 0..2 {
        for &root in &roots {
            // 2 bars of bass pattern: root-5th-root-5th  root-5th-octave-5th
            let fifth = root + 7;
            let octave = root + 12;

            // Bar 1: quarter notes
            events.push(note_on(0, ch, root, 100));
            events.push(note_off(quarter, ch, root));
            events.push(note_on(0, ch, fifth, 90));
            events.push(note_off(quarter, ch, fifth));
            events.push(note_on(0, ch, root, 95));
            events.push(note_off(quarter, ch, root));
            events.push(note_on(0, ch, fifth, 85));
            events.push(note_off(quarter, ch, fifth));

            // Bar 2: eighth note groove
            events.push(note_on(0, ch, root, 100));
            events.push(note_off(eighth, ch, root));
            events.push(note_on(0, ch, root, 70));
            events.push(note_off(eighth, ch, root));
            events.push(note_on(0, ch, fifth, 90));
            events.push(note_off(eighth, ch, fifth));
            events.push(note_on(0, ch, octave, 85));
            events.push(note_off(eighth, ch, octave));
            events.push(note_on(0, ch, root, 95));
            events.push(note_off(eighth, ch, root));
            events.push(note_on(0, ch, fifth, 80));
            events.push(note_off(eighth, ch, fifth));
            events.push(note_on(0, ch, root, 90));
            events.push(note_off(eighth, ch, root));
            events.push(note_on(0, ch, fifth, 75));
            events.push(note_off(eighth, ch, fifth));
        }
    }

    events.push(end_of_track());
    events
}

// ── Drum track: rock beat ─────────────────────────────────────────
fn build_drum_track(tpb: u16) -> Vec<TrackEvent<'static>> {
    let mut events: Vec<TrackEvent<'static>> = vec![];
    events.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::TrackName(b"Drums")),
    });

    let ch = 9u8; // MIDI channel 10 (0-indexed = 9)
    let quarter = tpb as u32;
    let eighth = quarter / 2;

    // GM drum pitches
    let kick: u8 = 36; // Bass Drum 1
    let snare: u8 = 38; // Acoustic Snare
    let hh_cl: u8 = 42; // Closed Hi-Hat
    let hh_op: u8 = 46; // Open Hi-Hat
    let crash: u8 = 49; // Crash Cymbal 1
    let ride: u8 = 51; // Ride Cymbal 1

    // Drum hit: very short note (1/32 beat)
    let hit_dur = tpb as u32 / 8;

    // Bar 1 intro: crash + kick
    events.push(note_on(0, ch, crash, 110));
    events.push(note_on(0, ch, kick, 120));
    events.push(note_off(hit_dur, ch, crash));
    events.push(note_off(0, ch, kick));

    // Fill remaining of beat 1
    events.push(note_on(quarter - hit_dur, ch, hh_cl, 80));
    events.push(note_off(hit_dur, ch, hh_cl));

    // Beat 2: snare + hh
    events.push(note_on(quarter - hit_dur, ch, snare, 100));
    events.push(note_on(0, ch, hh_cl, 85));
    events.push(note_off(hit_dur, ch, snare));
    events.push(note_off(0, ch, hh_cl));

    // Beat 2 &
    events.push(note_on(eighth - hit_dur, ch, hh_cl, 70));
    events.push(note_off(hit_dur, ch, hh_cl));

    // Beat 3: kick + hh
    events.push(note_on(eighth - hit_dur, ch, kick, 110));
    events.push(note_on(0, ch, hh_cl, 85));
    events.push(note_off(hit_dur, ch, kick));
    events.push(note_off(0, ch, hh_cl));

    // Beat 3 &
    events.push(note_on(eighth - hit_dur, ch, hh_cl, 70));
    events.push(note_off(hit_dur, ch, hh_cl));

    // Beat 4: snare + open hh
    events.push(note_on(eighth - hit_dur, ch, snare, 105));
    events.push(note_on(0, ch, hh_op, 90));
    events.push(note_off(hit_dur, ch, snare));
    events.push(note_off(0, ch, hh_op));

    // Beat 4 &
    events.push(note_on(eighth - hit_dur, ch, hh_cl, 65));
    events.push(note_off(hit_dur, ch, hh_cl));

    // Record the length of bar 1 pattern for reuse
    let bar1_event_count = events.len();

    // Bars 2-7: standard rock beat (repeated)
    for bar in 1..7 {
        // Each bar: Kick-HH | Snare-HH | Kick-HH | Snare-HH  (with & hi-hats)
        let use_ride = bar >= 4; // switch to ride in second half

        let hat = if use_ride { ride } else { hh_cl };

        // Beat 1: kick + hat
        let wait = eighth - hit_dur; // remaining time from previous bar
        events.push(note_on(wait, ch, kick, 115));
        events.push(note_on(0, ch, hat, 85));
        events.push(note_off(hit_dur, ch, kick));
        events.push(note_off(0, ch, hat));

        // Beat 1 &
        events.push(note_on(eighth - hit_dur, ch, hat, 65));
        events.push(note_off(hit_dur, ch, hat));

        // Beat 2: snare + hat
        events.push(note_on(eighth - hit_dur, ch, snare, 100));
        events.push(note_on(0, ch, hat, 80));
        events.push(note_off(hit_dur, ch, snare));
        events.push(note_off(0, ch, hat));

        // Beat 2 &
        events.push(note_on(eighth - hit_dur, ch, hat, 65));
        events.push(note_off(hit_dur, ch, hat));

        // Beat 3: kick + hat
        events.push(note_on(eighth - hit_dur, ch, kick, 110));
        events.push(note_on(0, ch, hat, 85));
        events.push(note_off(hit_dur, ch, kick));
        events.push(note_off(0, ch, hat));

        // Beat 3 &: kick ghost + hat
        events.push(note_on(eighth - hit_dur, ch, hat, 65));
        if bar % 2 == 1 {
            events.push(note_on(0, ch, kick, 70)); // ghost kick
            events.push(note_off(hit_dur, ch, hat));
            events.push(note_off(0, ch, kick));
        } else {
            events.push(note_off(hit_dur, ch, hat));
        }

        // Beat 4: snare + hat (open hat on last beat of every 2nd bar)
        let hat4 = if bar % 2 == 1 { hh_op } else { hat };
        events.push(note_on(eighth - hit_dur, ch, snare, 105));
        events.push(note_on(0, ch, hat4, 85));
        events.push(note_off(hit_dur, ch, snare));
        events.push(note_off(0, ch, hat4));

        // Beat 4 &
        events.push(note_on(eighth - hit_dur, ch, hh_cl, 60));
        events.push(note_off(hit_dur, ch, hh_cl));
    }

    // Bar 8: fill!
    let wait = eighth - hit_dur;
    // Beat 1: crash + kick
    events.push(note_on(wait, ch, crash, 120));
    events.push(note_on(0, ch, kick, 120));
    events.push(note_off(hit_dur, ch, crash));
    events.push(note_off(0, ch, kick));
    // snare roll on beats 2-4
    for i in 0..6 {
        let vel = 80 + i * 7;
        events.push(note_on(eighth - hit_dur, ch, snare, vel.min(127)));
        events.push(note_off(hit_dur, ch, snare));
    }
    // Final crash
    events.push(note_on(eighth - hit_dur, ch, crash, 127));
    events.push(note_on(0, ch, kick, 127));
    events.push(note_off(hit_dur, ch, crash));
    events.push(note_off(0, ch, kick));

    events.push(end_of_track());
    events
}
