//! Rust MIDI Player & Editor
//!
//! A GTK4-based MIDI piano-roll editor with multi-track playback support for
//! both SoundFont (`.sf2`) and CLAP plugin (`.clap`) synthesizers.
//!
//! # Architecture
//!
//! ```text
//! main.rs  →  window/ (GTK UI)
//!                 ├── roll_stack.rs   (piano / drum Views)
//!                 └── player.rs       (playback façade)
//!                         ├── audio_engine.rs  (CPAL stream)
//!                         ├── sequencer.rs     (MIDI event scheduling)
//!                         └── synth/           (TrackSynth abstraction)
//!                                ├── SoundFont (oxisynth)
//!                                └── ClapPlugin
//!                                       └── clap_host/ + clap_audio/
//! ```

mod app_cache;
mod audio_engine;
mod clap_audio;
mod clap_host;
mod config;
mod drum_roll;
mod midi;
mod midi_input;
mod piano_roll;
mod player;
mod project;
mod roll;
mod roll_stack;
mod sequencer;
mod soundbank;
mod synth;
mod velocity_curve;
mod window;

use gtk::Application;
use gtk::prelude::*;
use gtk4 as gtk;

pub mod audio_export;
pub mod drum_map;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "export" {
        let input_path = &args[2];
        let output_path = &args[3];
        if let Err(e) = run_cli_export(input_path, output_path) {
            eprintln!("Export error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let initial_file = args.get(1).cloned();

    let app = Application::builder()
        .application_id("com.github.midiplayer")
        // prevent GTK from trying to parse the file argument
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        window::build_ui(app, initial_file.clone());
    });

    app.run_with_args(&[] as &[&str]);
}

fn run_cli_export(input_path: &str, output_path: &str) -> anyhow::Result<()> {
    use std::path::Path;
    println!("Loading input file: {input_path}");
    let in_p = Path::new(input_path);
    let mut data = if in_p.extension().is_some_and(|e| e == project::PROJECT_EXTENSION) {
        project::ProjectFile::load(in_p)?.midi
    } else {
        midi::MidiData::load(input_path)?
    };

    let cfg = config::AppConfig::load();
    let mut player = player::Player::new(
        &cfg.soundfont_path,
        &cfg.drum_soundfont_path,
        &cfg.clap_plugin_path,
        &cfg.sfz_path,
        cfg.global_gain as f32,
    )?;

    // Route tracks to default SF if needed
    for track in &mut data.tracks {
        let is_drum = matches!(&track.mode, midi::TrackMode::Drum(_));
        if in_p.extension().is_none_or(|e| e != project::PROJECT_EXTENSION) {
            let initial_preset = match &track.synth_source {
                midi::SynthSource::SoundFont { preset, .. } => *preset,
                _ => 0,
            };
            track.synth_source = if is_drum && !cfg.drum_soundfont_path.is_empty() {
                midi::SynthSource::SoundFont {
                    path: cfg.drum_soundfont_path.clone(),
                    bank: 0,
                    preset: 0,
                }
            } else {
                midi::SynthSource::SoundFont {
                    path: cfg.soundfont_path.clone(),
                    bank: 0,
                    preset: initial_preset,
                }
            };
        }
    }

    println!("Rendering audio offline...");
    let (left, right, sample_rate) = player.render_offline(&data)?;

    let out_p = Path::new(output_path);
    let ext = out_p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "wav" => {
            println!("Encoding to WAV: {output_path}...");
            audio_export::export_wav(output_path, &left, &right, sample_rate)?;
        }
        "mp3" => {
            println!("Encoding to MP3 (320 kbps): {output_path}...");
            audio_export::export_mp3(output_path, &left, &right, sample_rate)?;
        }
        other => {
            anyhow::bail!("Unsupported export format '.{other}'. Please use '.wav' or '.mp3'.");
        }
    }

    println!("✅ Successfully exported: {output_path}");
    Ok(())
}
