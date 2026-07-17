//! Rust MIDI Player & Editor
//!
//! A GTK4-based MIDI piano-roll editor with multi-track playback support for
//! both SoundFont (`.sf2`) and CLAP plugin (`.clap`) synthesizers.
//!
//! # Architecture
//!
//! ```text
//! main.rs  →  window.rs (GTK UI)
//!                 ├── piano_roll.rs   (custom piano-roll widget)
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
mod player;
mod sequencer;
mod synth;
mod window;

use gtk::Application;
use gtk::prelude::*;
use gtk4 as gtk;

fn main() {
    let app = Application::builder()
        .application_id("com.github.midiplayer")
        .build();

    app.connect_activate(window::build_ui);

    app.run();
}
