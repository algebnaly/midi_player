use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oxisynth::{MidiEvent, SoundFont, Synth};
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    // 1. Load SoundFont
    let mut sf2 = File::open("GeneralUser-GS/GeneralUser-GS.sf2").unwrap();
    let font = SoundFont::load(&mut sf2).unwrap();

    // 2. Initialize Synth
    let mut synth = Synth::default();
    synth.add_font(font, true);

    // Set up instruments
    // Ch 0: Bass (Program 33 - Electric Bass (finger))
    synth
        .send_event(MidiEvent::ProgramChange {
            channel: 0,
            program_id: 33,
        })
        .unwrap();
    // Ch 1: Guitar (Program 27 - Electric Guitar (clean))
    synth
        .send_event(MidiEvent::ProgramChange {
            channel: 1,
            program_id: 27,
        })
        .unwrap();
    // Ch 9: Drums (Automatically mapped in General MIDI)

    let synth_arc = Arc::new(Mutex::new(synth));

    // 3. Initialize Audio Output via CPAL
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no output device available");
    let config = device.default_output_config().unwrap();
    let channels = config.channels() as usize;

    let synth_clone = synth_arc.clone();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels;
                    let mut left = vec![0.0f32; frames];
                    let mut right = vec![0.0f32; frames];

                    if let Ok(mut s) = synth_clone.try_lock() {
                        s.write((&mut left[..], &mut right[..]));
                    }

                    for (i, frame) in data.chunks_mut(channels).enumerate() {
                        frame[0] = left[i];
                        if channels > 1 {
                            frame[1] = right[i];
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .unwrap(),
        _ => panic!("Unsupported sample format"),
    };

    stream.play().unwrap();

    println!("Playing demo... 🎧");
    let mut s = synth_arc.lock().unwrap();

    // Helper macro to play a note
    macro_rules! play_note {
        ($ch:expr, $key:expr, $dur:expr) => {
            s.send_event(MidiEvent::NoteOn {
                channel: $ch,
                key: $key,
                vel: 100,
            })
            .unwrap();
            drop(s);
            sleep(Duration::from_millis($dur));
            s = synth_arc.lock().unwrap();
            s.send_event(MidiEvent::NoteOff {
                channel: $ch,
                key: $key,
            })
            .unwrap();
        };
    }

    // 4. Play a simple sequence
    for _ in 0..2 {
        // Kick & Bass
        s.send_event(MidiEvent::NoteOn {
            channel: 9,
            key: 36,
            vel: 100,
        })
        .unwrap(); // Kick
        s.send_event(MidiEvent::NoteOn {
            channel: 0,
            key: 36,
            vel: 100,
        })
        .unwrap(); // Bass C2
        drop(s);
        sleep(Duration::from_millis(250));
        s = synth_arc.lock().unwrap();
        s.send_event(MidiEvent::NoteOff {
            channel: 0,
            key: 36,
        })
        .unwrap();

        // Hi-Hat
        play_note!(9, 42, 250);

        // Snare & Guitar Chord
        s.send_event(MidiEvent::NoteOn {
            channel: 9,
            key: 38,
            vel: 100,
        })
        .unwrap(); // Snare
        s.send_event(MidiEvent::NoteOn {
            channel: 1,
            key: 60,
            vel: 90,
        })
        .unwrap(); // Guitar C4
        s.send_event(MidiEvent::NoteOn {
            channel: 1,
            key: 64,
            vel: 90,
        })
        .unwrap(); // Guitar E4
        s.send_event(MidiEvent::NoteOn {
            channel: 1,
            key: 67,
            vel: 90,
        })
        .unwrap(); // Guitar G4
        drop(s);
        sleep(Duration::from_millis(250));
        s = synth_arc.lock().unwrap();
        s.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 60,
        })
        .unwrap();
        s.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 64,
        })
        .unwrap();
        s.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 67,
        })
        .unwrap();

        // Hi-Hat
        play_note!(9, 42, 250);
    }

    println!("Demo finished!");
    sleep(Duration::from_millis(500)); // wait for tails
}
