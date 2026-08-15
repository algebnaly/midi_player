use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oxisynth::{MidiEvent, SoundFont, Synth};
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    println!("Loading Salamander Grand Piano (534MB SF2, might take a few seconds)...");

    let mut synth = Synth::default();
    let mut sf2 = File::open(
        "Pianos/SalamanderGrandPiano-SF2-V3+20200602/SalamanderGrandPiano-V3+20200602.sf2",
    )
    .unwrap();
    synth.add_font(SoundFont::load(&mut sf2).unwrap(), true);

    let synth_arc = Arc::new(Mutex::new(synth));

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no output device available");
    let config = device.default_output_config().unwrap();
    let channels = config.channels() as usize;

    let s_clone = synth_arc.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels;
                    let mut buf_l = vec![0.0f32; frames];
                    let mut buf_r = vec![0.0f32; frames];

                    if let Ok(mut s) = s_clone.try_lock() {
                        s.write((&mut buf_l[..], &mut buf_r[..]));
                    }

                    for (i, frame) in data.chunks_mut(channels).enumerate() {
                        frame[0] = buf_l[i];
                        if channels > 1 {
                            frame[1] = buf_r[i];
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
    println!("Playing a beautiful piano progression... 🎹");

    let mut s = synth_arc.lock().unwrap();

    macro_rules! play_chord {
        ($notes:expr, $vel:expr, $dur:expr) => {
            for &note in $notes.iter() {
                s.send_event(MidiEvent::NoteOn {
                    channel: 0,
                    key: note,
                    vel: $vel,
                })
                .unwrap();
            }
            drop(s);
            sleep(Duration::from_millis($dur));
            s = synth_arc.lock().unwrap();
            for &note in $notes.iter() {
                s.send_event(MidiEvent::NoteOff {
                    channel: 0,
                    key: note,
                })
                .unwrap();
            }
        };
    }

    macro_rules! arpeggiate {
        ($notes:expr, $vel:expr, $delay:expr) => {
            for &note in $notes.iter() {
                s.send_event(MidiEvent::NoteOn {
                    channel: 0,
                    key: note,
                    vel: $vel,
                })
                .unwrap();
                drop(s);
                sleep(Duration::from_millis($delay));
                s = synth_arc.lock().unwrap();
            }
            drop(s);
            sleep(Duration::from_millis(400));
            s = synth_arc.lock().unwrap();
            for &note in $notes.iter() {
                s.send_event(MidiEvent::NoteOff {
                    channel: 0,
                    key: note,
                })
                .unwrap();
            }
        };
    }

    // A beautiful, emotional progression
    // C Major 9
    arpeggiate!([48, 55, 60, 62, 64], 80, 150);
    sleep(Duration::from_millis(100));

    // G Major / B
    arpeggiate!([47, 55, 59, 62, 67], 75, 150);
    sleep(Duration::from_millis(100));

    // A minor 9
    arpeggiate!([45, 52, 57, 60, 64, 71], 85, 120);
    sleep(Duration::from_millis(200));

    // F Major 7 (sustained chord)
    play_chord!([41, 48, 53, 57, 60, 64], 70, 2000);

    println!("Demo finished!");
    drop(s);
    sleep(Duration::from_millis(3000)); // wait for natural piano tail/reverb
}
