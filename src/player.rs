use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct Player {
    sequencer: Arc<Mutex<MidiFileSequencer>>,
    _stream: cpal::Stream,
}

impl Player {
    pub fn new(sf2_path: &str) -> anyhow::Result<Self> {
        // Load SoundFont
        let mut sf2_file = File::open(sf2_path)?;
        let sound_font = Arc::new(SoundFont::new(&mut sf2_file)?);

        // Audio setup
        let host = cpal::default_host();

        let mut selected_device = None;
        let mut selected_config = None;

        // Try to prefer pipewire/pulse if available on Linux
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

        // Fallback to default device
        if selected_device.is_none() {
            if let Some(device) = host.default_output_device() {
                if let Ok(config) = device.default_output_config() {
                    selected_device = Some(device);
                    selected_config = Some(config);
                }
            }
        }

        // Fallback to the first working device
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

        let seq_clone = sequencer.clone();
        let channels = config.channels() as usize;

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut seq = seq_clone.lock().unwrap();
                    let mut left = vec![0.0f32; data.len() / channels];
                    let mut right = vec![0.0f32; data.len() / channels];
                    seq.render(&mut left, &mut right);

                    for (i, frame) in data.chunks_mut(channels).enumerate() {
                        frame[0] = left[i];
                        if channels > 1 {
                            frame[1] = right[i];
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
        })
    }

    pub fn play(&self, midi_path: &str) -> anyhow::Result<()> {
        let mut midi_file = File::open(midi_path)?;
        let midi_data = Arc::new(MidiFile::new(&mut midi_file)?);

        let mut seq = self.sequencer.lock().unwrap();
        seq.play(&midi_data, false);
        Ok(())
    }

    pub fn play_buffer(&self, buffer: &[u8]) -> anyhow::Result<()> {
        let mut cursor = std::io::Cursor::new(buffer);
        let midi_data = Arc::new(MidiFile::new(&mut cursor)?);

        let mut seq = self.sequencer.lock().unwrap();
        seq.play(&midi_data, false);
        Ok(())
    }

    pub fn stop(&self) {
        let mut seq = self.sequencer.lock().unwrap();
        seq.stop();
    }

    pub fn get_time(&self) -> f64 {
        let seq = self.sequencer.lock().unwrap();
        seq.get_position()
    }

    pub fn is_playing(&self) -> bool {
        let seq = self.sequencer.lock().unwrap();
        seq.end_of_sequence() == false
    }
}
