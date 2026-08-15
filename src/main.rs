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
mod midi;
mod midi_input;
mod piano_roll;
mod drum_roll;
mod roll;
mod roll_stack;
mod player;
mod project;
mod sequencer;
mod soundbank;
mod synth;
mod velocity_curve;
mod window;

use gtk::Application;
use gtk::prelude::*;
use gtk4 as gtk;

fn main() {
    let args: Vec<String> = std::env::args().collect();
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
pub mod drum_map;
