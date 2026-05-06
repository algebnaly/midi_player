use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct Player {
    sequencer: Arc<Mutex<MidiFileSequencer>>,
    _stream: cpal::Stream,
    paused: Arc<AtomicBool>,
    current_midi: Arc<Mutex<Option<Arc<MidiFile>>>>,
    sound_font: Arc<SoundFont>,
    sample_rate: i32,
    preview_synth: Arc<Mutex<Synthesizer>>,
}

impl Player {
    pub fn new(sf2_path: &str) -> anyhow::Result<Self> {
        let mut sf2_file = File::open(sf2_path)?;
        let sound_font = Arc::new(SoundFont::new(&mut sf2_file)?);

        let host = cpal::default_host();
        let mut selected_device = None;
        let mut selected_config = None;

        if let Ok(devices) = host.output_devices() {
            for device in devices {
                if let Ok(name) = device.name() {
                    if name.contains("pipewire") || name.contains("pulse") {
                        if let Ok(config) = device.default_output_config() {
                            selected_device = Some(device);
                            selected_config = Some(config);
                            break;
                        }
                    }
                }
            }
        }

        if selected_device.is_none() {
            if let Some(device) = host.default_output_device() {
                if let Ok(config) = device.default_output_config() {
                    selected_device = Some(device);
                    selected_config = Some(config);
                }
            }
        }

        if selected_device.is_none() {
            if let Ok(devices) = host.output_devices() {
                for device in devices {
                    if let Ok(config) = device.default_output_config() {
                        selected_device = Some(device);
                        selected_config = Some(config);
                        break;
                    }
                }
            }
        }

        let device =
            selected_device.ok_or_else(|| anyhow::anyhow!("No output device available"))?;
        let config = selected_config.unwrap();

        let sample_rate = config.sample_rate() as i32;

        let settings = SynthesizerSettings::new(sample_rate);
        let synthesizer = Synthesizer::new(&sound_font, &settings)?;
        let sequencer = Arc::new(Mutex::new(MidiFileSequencer::new(synthesizer)));

        let preview_synth_inner = Synthesizer::new(&sound_font, &settings)?;
        let preview_synth = Arc::new(Mutex::new(preview_synth_inner));
        let preview_clone = preview_synth.clone();

        let seq_clone = sequencer.clone();
        let channels = config.channels() as usize;
        let paused = Arc::new(AtomicBool::new(false));
        let paused_clone = paused.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if paused_clone.load(Ordering::SeqCst) {
                        for frame in data.iter_mut() {
                            *frame = 0.0;
                        }
                        return;
                    }
                    let mut out_left = vec![0.0f32; data.len() / channels];
                    let mut out_right = vec![0.0f32; data.len() / channels];

                    // Try locking, if blocked by hot-swap, just output silence briefly to avoid OS underrun pop
                    if let Ok(mut seq) = seq_clone.try_lock() {
                        let mut left = vec![0.0f32; data.len() / channels];
                        let mut right = vec![0.0f32; data.len() / channels];
                        seq.render(&mut left, &mut right);

                        for i in 0..left.len() {
                            out_left[i] += left[i];
                            out_right[i] += right[i];
                        }
                    }

                    if let Ok(mut p_synth) = preview_clone.try_lock() {
                        let mut left = vec![0.0f32; data.len() / channels];
                        let mut right = vec![0.0f32; data.len() / channels];
                        p_synth.render(&mut left, &mut right);

                        for i in 0..left.len() {
                            out_left[i] += left[i];
                            out_right[i] += right[i];
                        }
                    }

                    for (i, frame) in data.chunks_mut(channels).enumerate() {
                        frame[0] = out_left[i];
                        if channels > 1 {
                            frame[1] = out_right[i];
                        }
                    }
                },
                |err| eprintln!("an error occurred on stream: {}", err),
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Unsupported sample format")),
        };

        stream.play()?;

        Ok(Self {
            sequencer,
            _stream: stream,
            paused,
            current_midi: Arc::new(Mutex::new(None)),
            sound_font,
            sample_rate,
            preview_synth,
        })
    }

    pub fn play(&self, midi_path: &str) -> anyhow::Result<()> {
        let mut midi_file = File::open(midi_path)?;
        let midi_data = Arc::new(MidiFile::new(&mut midi_file)?);

        *self.current_midi.lock().unwrap() = Some(midi_data.clone());
        let mut seq = self.sequencer.lock().unwrap();
        seq.play(&midi_data, false);
        self.paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn play_buffer(&self, buffer: &[u8]) -> anyhow::Result<()> {
        let mut cursor = std::io::Cursor::new(buffer);
        let midi_data = Arc::new(MidiFile::new(&mut cursor)?);

        *self.current_midi.lock().unwrap() = Some(midi_data.clone());
        let mut seq = self.sequencer.lock().unwrap();
        seq.play(&midi_data, false);
        self.paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn preview_note_on(&self, pitch: u8, velocity: u8) {
        if let Ok(mut p_synth) = self.preview_synth.lock() {
            p_synth.note_on(0, pitch as i32, velocity as i32);
        }
    }

    pub fn preview_note_off(&self, pitch: u8) {
        if let Ok(mut p_synth) = self.preview_synth.lock() {
            p_synth.note_off(0, pitch as i32);
        }
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        let mut seq = self.sequencer.lock().unwrap();
        seq.stop();
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn seek(&self, time: f64) {
        let midi = self.current_midi.lock().unwrap().clone();
        if let Some(m) = midi {
            let settings = SynthesizerSettings::new(self.sample_rate);
            let synthesizer = Synthesizer::new(&self.sound_font, &settings).unwrap();
            let mut new_seq = MidiFileSequencer::new(synthesizer);
            new_seq.play(&m, false);

            let block_size = 1024;
            let mut left = vec![0.0f32; block_size];
            let mut right = vec![0.0f32; block_size];
            while new_seq.get_position() < time {
                new_seq.render(&mut left, &mut right);
            }

            *self.sequencer.lock().unwrap() = new_seq;
        }
    }

    pub fn hot_swap(&self, buffer: &[u8], time: f64) -> anyhow::Result<()> {
        let mut cursor = std::io::Cursor::new(buffer);
        let midi_data = Arc::new(MidiFile::new(&mut cursor)?);

        let settings = SynthesizerSettings::new(self.sample_rate);
        let synthesizer = Synthesizer::new(&self.sound_font, &settings).unwrap();
        let mut new_seq = MidiFileSequencer::new(synthesizer);
        new_seq.play(&midi_data, false);

        let block_size = 1024;
        let mut left = vec![0.0f32; block_size];
        let mut right = vec![0.0f32; block_size];
        while new_seq.get_position() < time {
            new_seq.render(&mut left, &mut right);
        }

        *self.current_midi.lock().unwrap() = Some(midi_data);
        *self.sequencer.lock().unwrap() = new_seq;
        Ok(())
    }

    pub fn get_time(&self) -> f64 {
        let seq = self.sequencer.lock().unwrap();
        seq.get_position()
    }

    pub fn is_playing(&self) -> bool {
        let seq = self.sequencer.lock().unwrap();
        !seq.end_of_sequence() && !self.is_paused()
    }
}
