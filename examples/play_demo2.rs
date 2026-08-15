use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oxisynth::{MidiEvent, SoundFont, Synth};
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    println!("Loading SoundFonts (this might take a few seconds for the 195MB Guitar)...");

    // 1. Synth for Drums (GeneralUser GS)
    let mut synth_drums = Synth::default();
    let mut sf2_drums = File::open("GeneralUser-GS/GeneralUser-GS.sf2").unwrap();
    synth_drums.add_font(SoundFont::load(&mut sf2_drums).unwrap(), true);

    // 2. Synth for Bass (Flame Studios Ibanez)
    let mut synth_bass = Synth::default();
    let mut sf2_bass = File::open(
        "FlameStudios/FS_Ibanez_Electric_Bass_Guitar/FS Ibanez Electric Bass Guitar.sf2",
    )
    .unwrap();
    synth_bass.add_font(SoundFont::load(&mut sf2_bass).unwrap(), true);

    // 3. Synth for Guitar (Flame Studios Fender Telecaster)
    let mut synth_guitar = Synth::default();
    let mut sf2_guitar = File::open("FlameStudios/FS_Fender_Telecaster_Electric_Guitar_Both_Pickups_and_Amp/FS Fender Telecaster Electric Guitar Both Pickups and Amp.sf2").unwrap();
    synth_guitar.add_font(SoundFont::load(&mut sf2_guitar).unwrap(), true);

    // Setup Instruments (Flame Studios SF2s typically map to Bank 0, Program 0)
    synth_bass
        .send_event(MidiEvent::ProgramChange {
            channel: 0,
            program_id: 0,
        })
        .unwrap();
    synth_guitar
        .send_event(MidiEvent::ProgramChange {
            channel: 1,
            program_id: 0,
        })
        .unwrap();

    let drums_arc = Arc::new(Mutex::new(synth_drums));
    let bass_arc = Arc::new(Mutex::new(synth_bass));
    let guitar_arc = Arc::new(Mutex::new(synth_guitar));

    // 4. Initialize Audio Output via CPAL
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no output device available");
    let config = device.default_output_config().unwrap();
    let channels = config.channels() as usize;

    let d_clone = drums_arc.clone();
    let b_clone = bass_arc.clone();
    let g_clone = guitar_arc.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels;

                    let mut buf_l = vec![0.0f32; frames];
                    let mut buf_r = vec![0.0f32; frames];
                    let mut tmp_l = vec![0.0f32; frames];
                    let mut tmp_r = vec![0.0f32; frames];

                    // Render Drums
                    if let Ok(mut s) = d_clone.try_lock() {
                        tmp_l.fill(0.0);
                        tmp_r.fill(0.0);
                        s.write((&mut tmp_l[..], &mut tmp_r[..]));
                        for i in 0..frames {
                            buf_l[i] += tmp_l[i];
                            buf_r[i] += tmp_r[i];
                        }
                    }
                    // Render Bass
                    if let Ok(mut s) = b_clone.try_lock() {
                        tmp_l.fill(0.0);
                        tmp_r.fill(0.0);
                        s.write((&mut tmp_l[..], &mut tmp_r[..]));
                        for i in 0..frames {
                            buf_l[i] += tmp_l[i];
                            buf_r[i] += tmp_r[i];
                        }
                    }
                    // Render Guitar
                    if let Ok(mut s) = g_clone.try_lock() {
                        tmp_l.fill(0.0);
                        tmp_r.fill(0.0);
                        s.write((&mut tmp_l[..], &mut tmp_r[..]));
                        for i in 0..frames {
                            buf_l[i] += tmp_l[i];
                            buf_r[i] += tmp_r[i];
                        }
                    }

                    // Interleave
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
    println!("Playing High-Quality Demo... 🎧 (Fender Telecaster + Ibanez Bass + GM Drums)");

    let mut d = drums_arc.lock().unwrap();
    let mut b = bass_arc.lock().unwrap();
    let mut g = guitar_arc.lock().unwrap();

    macro_rules! drum {
        ($key:expr, $vel:expr) => {
            d.send_event(MidiEvent::NoteOn {
                channel: 9,
                key: $key,
                vel: $vel,
            })
            .unwrap();
        };
    }
    macro_rules! bass {
        ($key:expr, $vel:expr) => {
            b.send_event(MidiEvent::NoteOn {
                channel: 0,
                key: $key,
                vel: $vel,
            })
            .unwrap();
        };
    }
    macro_rules! guitar {
        ($key:expr, $vel:expr) => {
            g.send_event(MidiEvent::NoteOn {
                channel: 1,
                key: $key,
                vel: $vel,
            })
            .unwrap();
        };
    }

    // Rock rhythm at 120 BPM (500ms per beat)
    let eighth = Duration::from_millis(250);

    for bar in 0..4 {
        // Beat 1 (Kick + Bass root + Guitar Chord)
        drum!(36, 120); // Kick
        drum!(42, 90); // Hihat
        bass!(36, 110); // C2
        guitar!(60, 100);
        guitar!(64, 90);
        guitar!(67, 90); // C major chord
        drop(d);
        drop(b);
        drop(g);
        sleep(eighth);
        d = drums_arc.lock().unwrap();
        b = bass_arc.lock().unwrap();
        g = guitar_arc.lock().unwrap();

        // Beat 1.5 (Hihat + Bass octave)
        drum!(42, 90);
        bass!(48, 90); // C3
        drop(d);
        drop(b);
        drop(g);
        sleep(eighth);
        d = drums_arc.lock().unwrap();
        b = bass_arc.lock().unwrap();
        g = guitar_arc.lock().unwrap();
        b.send_event(MidiEvent::NoteOff {
            channel: 0,
            key: 48,
        })
        .unwrap();

        // Beat 2 (Snare + Hihat)
        drum!(38, 110); // Snare
        drum!(42, 90);
        bass!(36, 100); // C2
        g.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 60,
        })
        .unwrap();
        g.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 64,
        })
        .unwrap();
        g.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 67,
        })
        .unwrap();
        // Guitar chug (muted-ish feel by short duration)
        guitar!(60, 100);
        guitar!(67, 100); // C power chord
        drop(d);
        drop(b);
        drop(g);
        sleep(eighth);
        d = drums_arc.lock().unwrap();
        b = bass_arc.lock().unwrap();
        g = guitar_arc.lock().unwrap();
        g.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 60,
        })
        .unwrap();
        g.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 67,
        })
        .unwrap();

        // Beat 2.5
        drum!(42, 90);
        if bar % 2 == 1 {
            drum!(36, 100); // syncopated kick
            bass!(41, 100); // F2
        }
        drop(d);
        drop(b);
        drop(g);
        sleep(eighth);
        d = drums_arc.lock().unwrap();
        b = bass_arc.lock().unwrap();
        g = guitar_arc.lock().unwrap();

        // Beat 3 (Kick + Hihat + Guitar)
        drum!(36, 110);
        drum!(42, 90);
        bass!(43, 110); // G2
        guitar!(67, 100);
        guitar!(71, 90);
        guitar!(74, 90); // G major chord
        drop(d);
        drop(b);
        drop(g);
        sleep(eighth);
        d = drums_arc.lock().unwrap();
        b = bass_arc.lock().unwrap();
        g = guitar_arc.lock().unwrap();

        // Beat 3.5
        drum!(42, 90);
        bass!(43, 80);
        drop(d);
        drop(b);
        drop(g);
        sleep(eighth);
        d = drums_arc.lock().unwrap();
        b = bass_arc.lock().unwrap();
        g = guitar_arc.lock().unwrap();

        // Beat 4 (Snare + Hihat)
        drum!(38, 110);
        drum!(42, 90);
        bass!(36, 100);
        g.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 67,
        })
        .unwrap();
        g.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 71,
        })
        .unwrap();
        g.send_event(MidiEvent::NoteOff {
            channel: 1,
            key: 74,
        })
        .unwrap();
        drop(d);
        drop(b);
        drop(g);
        sleep(eighth);
        d = drums_arc.lock().unwrap();
        b = bass_arc.lock().unwrap();
        g = guitar_arc.lock().unwrap();

        // Beat 4.5
        drum!(46, 90); // Open Hihat
        if bar == 3 {
            drum!(49, 120); // Crash on turnaround
        }
        drop(d);
        drop(b);
        drop(g);
        sleep(eighth);
        d = drums_arc.lock().unwrap();
        b = bass_arc.lock().unwrap();
        g = guitar_arc.lock().unwrap();
        b.send_event(MidiEvent::NoteOff {
            channel: 0,
            key: 36,
        })
        .unwrap();
        b.send_event(MidiEvent::NoteOff {
            channel: 0,
            key: 43,
        })
        .unwrap();
    }

    println!("Demo finished!");
    drop(d);
    drop(b);
    drop(g);
    sleep(Duration::from_millis(1500)); // wait for crash cymbal & guitar tails
}
