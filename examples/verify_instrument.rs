use oxisynth::{MidiEvent, SoundFont, Synth};
use std::fs::File;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run --example verify_instrument <path_to_sf2> [preset_number]");
        return;
    }
    let path = &args[1];
    let preset: u8 = if args.len() > 2 {
        args[2].parse().unwrap_or(0)
    } else {
        0
    };

    println!("Testing SoundFont loading for: {}", path);
    println!("Target Preset: Bank 0, Program {}", preset);
    let mut file = File::open(path).unwrap_or_else(|e| panic!("Failed to open file '{}': {}", path, e));
    let font = SoundFont::load(&mut file).unwrap_or_else(|e| panic!("Failed to parse SoundFont '{}': {:?}", path, e));
    println!("✓ SoundFont parsed successfully!");

    let mut synth = Synth::default();
    synth.set_sample_rate(44100.0);
    let id = synth.add_font(font, true);
    println!("✓ SoundFont added to synth with ID: {:?}", id);

    // Select target preset via ProgramChange event
    synth.send_event(MidiEvent::ProgramChange { channel: 0, program_id: preset }).unwrap();

    // Pitch: for Cello/Bass test lower pitch (48 = C3 or 36 = C2), for Violin test 60/72
    let pitch = if preset == 43 { 36 } else if preset == 42 { 48 } else { 64 };
    println!("  Auditioning Note: pitch {}", pitch);

    // Simulate NoteOn on channel 0
    synth.send_event(MidiEvent::NoteOn { channel: 0, key: pitch, vel: 100 }).unwrap();

    let mut left = vec![0.0f32; 4096];
    let mut right = vec![0.0f32; 4096];
    synth.write((&mut left[..], &mut right[..]));

    let max_amp = left.iter().chain(right.iter()).fold(0.0f32, |a, &b| a.max(b.abs()));
    println!("  Peak audio amplitude generated: {:.4}", max_amp);
    if max_amp > 0.001 {
        println!("✓ Audio generated successfully! Instrument is playable.");
    } else {
        println!("⚠ Warning: Low/no audio output generated.");
    }
}
