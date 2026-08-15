//! GTK4 application window construction and event wiring.
//!
//! [`build_ui`] is the `connect_activate` callback registered in `main.rs`.
//! It assembles the header bar, roll stack, floating panels, and wires them
//! to the [`Player`] backend.

mod css;
mod files;
mod header;
mod helpers;
mod midi_devices;
mod modes;
mod overlay;
mod playback;
mod track_panel;
mod tracks;
mod velocity_panel;

use gtk::prelude::*;
use gtk::ApplicationWindow;
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::config::AppConfig;
use crate::midi::MidiData;
use crate::midi_input::MidiInputManager;
use crate::player::Player;
use crate::roll_stack::RollStack;
use crate::velocity_curve::VelocityCurve;

use files::{load_initial_project, wire_file_actions};
use header::build_header;
use helpers::{midi_input_target, TrackUi};
use midi_devices::wire_midi_input;
use modes::wire_edit_mode_buttons;
use playback::wire_playback;
use track_panel::attach_track_panel;
use tracks::wire_track_controls;
use velocity_panel::attach_velocity_panel;

pub fn build_ui(app: &gtk::Application, initial_file: Option<String>) {
    let config = AppConfig::load();
    let soundbank_manager = Rc::new(crate::soundbank::SoundbankManager::scan(&config.soundbank_dirs));
    let velocity_curve = VelocityCurve::default();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("MIDI Player")
        .default_width(1024)
        .default_height(768)
        .build();

    let header = build_header(&window, &config);
    let initial_gain = config.global_gain.clamp(0.0, 2.0);

    let root_overlay = gtk::Overlay::new();
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root_overlay.set_child(Some(&vbox));
    window.set_child(Some(&root_overlay));

    let track_panel = attach_track_panel(&root_overlay, &header.tracks_panel_btn);

    let piano_roll = RollStack::new();
    piano_roll.set_default_note_beats(config.default_note_beats);

    let status_bar = gtk::Label::new(Some("[Draw]"));
    status_bar.set_xalign(0.0);
    status_bar.set_hexpand(true);
    status_bar.set_vexpand(false);
    status_bar.add_css_class("status-bar");

    vbox.append(&piano_roll.stack);
    vbox.append(&status_bar);

    attach_velocity_panel(
        &root_overlay,
        &header.velocity_panel_btn,
        velocity_curve.clone(),
    );
    css::apply_stylesheet();

    wire_edit_mode_buttons(
        &piano_roll,
        &header.select_mode_btn,
        &header.typing_kb_btn,
        &status_bar,
    );

    let player = Rc::new(RefCell::new(
        match Player::new(
            &config.soundfont_path,
            &config.drum_soundfont_path,
            &config.clap_plugin_path,
            &config.sfz_path,
            initial_gain as f32,
        ) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!(
                    "Failed to initialize player: {}. Audio playback disabled.",
                    e
                );
                None
            }
        },
    ));

    let player_gain = player.clone();
    header.gain_scale.connect_value_changed(move |scale| {
        if let Some(p) = &*player_gain.borrow() {
            p.set_global_gain(scale.value() as f32);
        }
    });

    let (midi_manager, midi_ui_rx) = if let Some(p) = &*player.borrow() {
        let (manager, ui_rx) = MidiInputManager::new(p.live_midi_sender(), velocity_curve.clone());
        (Some(manager), Some(ui_rx))
    } else {
        (None, None)
    };
    let midi_manager = Rc::new(RefCell::new(midi_manager));

    wire_midi_input(
        &header.midi_input_list,
        &header.midi_input_dropdown,
        &header.midi_refresh_btn,
        midi_manager.clone(),
        midi_ui_rx,
        &piano_roll,
        &status_bar,
    );

    let current_midi_path = Rc::new(RefCell::new(None::<String>));
    let is_playing = Rc::new(RefCell::new(false));
    let syncing_tracks = Rc::new(Cell::new(false));

    let tracks = TrackUi {
        roll: piano_roll.clone(),
        model: header.track_list.clone(),
        list_box: track_panel.list_box.clone(),
        dropdown: header.track_dropdown.clone(),
        name_entry: track_panel.name_entry.clone(),
        mute: track_panel.mute_btn.clone(),
        solo: track_panel.solo_btn.clone(),
        arm: track_panel.arm_btn.clone(),
        syncing: syncing_tracks,
    };

    let synth_names = if let Some(p) = &*player.borrow() {
        p.get_synth_names()
    } else {
        vec!["Track 0".to_string()]
    };
    let mut empty_data = MidiData::new_empty(&synth_names);
    empty_data.set_bpm(config.default_bpm);
    let initial_track = empty_data.tracks[0].id;
    tracks.install(empty_data, initial_track, false);

    wire_track_controls(
        tracks.clone(),
        &track_panel,
        player.clone(),
        midi_manager.clone(),
        soundbank_manager,
        &status_bar,
    );

    wire_file_actions(
        &window,
        &header.open_btn,
        &header.save_btn,
        &header.save_project_btn,
        &tracks,
        &header.bpm_spin,
        current_midi_path.clone(),
        player.clone(),
        config.soundfont_path.clone(),
        config.drum_soundfont_path.clone(),
    );

    if let Some(midi) = piano_roll.get_data_clone()
        && let Some((track_id, synth_index)) =
            midi_input_target(&midi, piano_roll.active_track_index())
        && let Some(manager) = midi_manager.borrow().as_ref()
    {
        manager.set_target_track(track_id, synth_index);
    }

    wire_playback(
        &window,
        &header.play_btn,
        &header.pause_btn,
        &header.rewind_btn,
        &header.plugin_gui_btn,
        &header.bpm_spin,
        &piano_roll,
        player.clone(),
        is_playing,
        midi_manager,
        &header.track_dropdown,
    );

    window.present();

    if let Some(path_str) = initial_file {
        load_initial_project(
            path_str,
            &current_midi_path,
            &player,
            &header.bpm_spin,
            &tracks,
        );
    }
}
