//! GTK4 application window construction and event wiring.
//!
//! [`build_ui`] is the `connect_activate` callback registered in `main.rs`.
//! It assembles the header bar (open / save / play / pause / rewind / BPM /
//! track selector), the [`PianoRollWidget`], and wires up all user
//! interactions to the [`Player`] backend.

use gtk::prelude::*;
use gtk::{ApplicationWindow, Box, Button, DropDown, HeaderBar, Label, StringList, ToggleButton};
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::config::AppConfig;
use crate::midi::MidiData;
use crate::piano_roll::PianoRollWidget;
use crate::piano_roll::types::EditMode;
use crate::player::Player;

pub fn build_ui(app: &gtk::Application) {
    // Load user configuration
    let config = AppConfig::load();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("MIDI Player")
        .default_width(1024)
        .default_height(768)
        .build();

    let header_bar = HeaderBar::new();
    window.set_titlebar(Some(&header_bar));

    let open_btn = Button::with_label("Open");
    let save_btn = Button::with_label("Export");
    let play_btn = Button::with_label("Play");
    let pause_btn = Button::with_label("Pause");
    let rewind_btn = Button::with_label("Start Over");

    let track_list = StringList::new(&[]);
    let track_dropdown = DropDown::new(Some(track_list.clone()), gtk::Expression::NONE);

    let bpm_adj = gtk::Adjustment::new(config.default_bpm, 20.0, 999.0, 1.0, 10.0, 0.0);
    let bpm_spin = gtk::SpinButton::new(Some(&bpm_adj), 1.0, 0);
    bpm_spin.set_tooltip_text(Some("BPM"));
    let bpm_box = Box::new(gtk::Orientation::Horizontal, 5);
    bpm_box.append(&gtk::Label::new(Some("BPM: ")));
    bpm_box.append(&bpm_spin);

    let plugin_gui_btn = Button::with_label("Plugin GUI");

    let typing_kb_btn = ToggleButton::with_label("⌨ Typing Keyboard");
    typing_kb_btn.set_tooltip_text(Some(
        "Play notes with your keyboard:\n\
         1 2 3 4 5 → C#4 D#4 F#4 G#4 A#4\n\
         Q W E R T Y U → C4 D4 E4 F4 G4 A4 B4\n\
         A S D F G → C#3 D#3 F#3 G#3 A#3\n\
         Z X C V B N M → C3 D3 E3 F3 G3 A3 B3\n\
         K from Normal: enter  |  Esc: return\n\
         ↑: octave up  |  ↓: octave down",
    ));

    let select_mode_btn = ToggleButton::with_label("⬚ Select");
    select_mode_btn.set_tooltip_text(Some(
        "Select mode (B from Normal, Esc to return):\n\
         Draw a box to select notes, then drag to move them.",
    ));

    header_bar.pack_start(&open_btn);
    header_bar.pack_start(&save_btn);
    header_bar.pack_start(&track_dropdown);
    header_bar.pack_start(&plugin_gui_btn);
    header_bar.pack_start(&select_mode_btn);
    header_bar.pack_start(&typing_kb_btn);
    header_bar.pack_start(&bpm_box);

    header_bar.pack_end(&rewind_btn);
    header_bar.pack_end(&pause_btn);
    header_bar.pack_end(&play_btn);

    let vbox = Box::new(gtk::Orientation::Vertical, 0);
    window.set_child(Some(&vbox));

    let piano_roll = PianoRollWidget::new();
    piano_roll.set_default_note_beats(config.default_note_beats);
    vbox.append(&piano_roll);

    // Status bar at the bottom — full width, fixed height
    let status_bar = Label::new(Some("[Draw]"));
    status_bar.set_xalign(0.0);
    status_bar.set_hexpand(true);
    status_bar.set_vexpand(false);
    status_bar.add_css_class("status-bar");
    vbox.append(&status_bar);

    // Apply minimal CSS for the status bar
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_data(
        ".status-bar { font-family: monospace; font-size: 14px; font-weight: bold; \
         padding: 6px 10px; background: #1a1a1a; color: #eee; \
         min-height: 22px; }",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Wire status callback
    let status_bar_clone = status_bar.clone();
    piano_roll.connect_status(move |msg| {
        status_bar_clone.set_text(msg);
    });

    // Wire select mode toggle button
    let pr_select = piano_roll.clone();
    let _select_mode_btn_clone = select_mode_btn.clone();
    let syncing_mode_buttons = Rc::new(Cell::new(false));
    let select_sync_guard = syncing_mode_buttons.clone();
    select_mode_btn.connect_toggled(move |btn| {
        if select_sync_guard.get() {
            return;
        }
        if btn.is_active() {
            pr_select.enter_select_mode();
        } else {
            pr_select.enter_normal_mode();
        }
        pr_select.grab_focus();
    });

    // Also sync the button when edit mode changes from keyboard shortcut
    // (We'll use a timer to poll — simpler than a custom signal)
    let pr_mode_poll = piano_roll.clone();
    let select_btn_poll = select_mode_btn.clone();
    let typing_btn_poll = typing_kb_btn.clone();
    let poll_sync_guard = syncing_mode_buttons.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let is_select = pr_mode_poll.get_edit_mode() == EditMode::Select;
        if select_btn_poll.is_active() != is_select {
            poll_sync_guard.set(true);
            select_btn_poll.set_active(is_select);
            poll_sync_guard.set(false);
        }
        let is_typing = pr_mode_poll.is_typing_keyboard_enabled();
        if typing_btn_poll.is_active() != is_typing {
            poll_sync_guard.set(true);
            typing_btn_poll.set_active(is_typing);
            poll_sync_guard.set(false);
        }
        glib::ControlFlow::Continue
    });

    // Wire typing keyboard toggle
    let pr_typing = piano_roll.clone();
    let typing_sync_guard = syncing_mode_buttons.clone();
    typing_kb_btn.connect_toggled(move |btn| {
        if typing_sync_guard.get() {
            return;
        }
        if btn.is_active() {
            pr_typing.enter_typing_keyboard_mode();
        } else {
            pr_typing.enter_normal_mode();
        }
        // When enabling, grab focus on the piano roll so it receives key events.
        if btn.is_active() {
            pr_typing.grab_focus();
        }
    });

    let player = Rc::new(RefCell::new(
        match Player::new(&config.soundfont_path, &config.clap_plugin_path) {
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

    let current_midi_path = Rc::new(RefCell::new(None::<String>));
    let is_playing = Rc::new(RefCell::new(false));

    // Initialize with an empty project
    let synth_names = if let Some(p) = &*player.borrow() {
        p.get_synth_names()
    } else {
        vec!["Track 0".to_string()]
    };
    let mut empty_data = MidiData::new_empty(&synth_names);
    empty_data.set_bpm(config.default_bpm);
    for name in &synth_names {
        track_list.append(name);
    }
    piano_roll.set_data(empty_data);

    // Open action
    let window_clone = window.clone();
    let current_midi_clone = current_midi_path.clone();
    let piano_roll_clone = piano_roll.clone();
    let track_list_clone = track_list.clone();
    let track_dropdown_open = track_dropdown.clone();
    let bpm_spin_open = bpm_spin.clone();

    open_btn.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::new();
        let window = window_clone.clone();
        let midi_path = current_midi_clone.clone();
        let pr = piano_roll_clone.clone();
        let tl = track_list_clone.clone();
        let td = track_dropdown_open.clone();
        let bpm_spin_inner = bpm_spin_open.clone();

        dialog.open(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
            if let Ok(file) = res {
                let path = file.path().unwrap();
                let path_str = path.to_string_lossy().to_string();
                *midi_path.borrow_mut() = Some(path_str.clone());

                match MidiData::load(&path_str) {
                    Ok(data) => {
                        tl.splice(0, tl.n_items(), &[]);
                        for (i, t) in data.tracks.iter().enumerate() {
                            let name = if t.name.is_empty() {
                                format!("Track {}", i)
                            } else {
                                t.name.clone()
                            };
                            tl.append(&name);
                        }
                        let bpm = data.get_bpm();
                        bpm_spin_inner.set_value(bpm);
                        pr.set_data(data);
                        td.set_selected(0);
                        pr.set_playhead(0.0);
                    }
                    Err(e) => eprintln!("Failed to load midi: {}", e),
                }
            }
        });
    });

    let pr_bpm = piano_roll.clone();
    let player_bpm = player.clone();
    let is_playing_bpm = is_playing.clone();
    bpm_spin.connect_value_changed(move |spin| {
        let new_bpm = spin.value();
        if let Some(mut midi) = pr_bpm.get_data_clone() {
            if (midi.get_bpm() - new_bpm).abs() < 0.1 {
                return;
            }
            let current_tick = pr_bpm.get_playhead_tick();
            midi.set_bpm(new_bpm);
            pr_bpm.update_data(midi.clone());
            pr_bpm.set_playhead_tick(current_tick);

            if *is_playing_bpm.borrow() {
                if let Some(p) = &*player_bpm.borrow() {
                    let new_time = current_tick / (midi.ticks_per_beat as f64 * (new_bpm / 60.0));
                    if let Err(e) = p.hot_swap(midi.clone(), new_time) {
                        eprintln!("Failed to hot-swap on BPM change: {}", e);
                    }
                }
            }
        }
    });

    let pr_save = piano_roll.clone();
    let window_save = window.clone();
    save_btn.connect_clicked(move |_| {
        if let Some(midi) = pr_save.get_data_clone() {
            let dialog = gtk::FileDialog::new();
            let window = window_save.clone();
            dialog.save(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().to_string();
                        if let Err(e) = midi.export_to_file(&path_str) {
                            eprintln!("Failed to export: {}", e);
                        }
                    }
                }
            });
        }
    });

    let player_clone = player.clone();
    let is_playing_clone = is_playing.clone();
    let pr_play = piano_roll.clone();

    play_btn.connect_clicked(move |_| {
        if let Some(midi) = pr_play.get_data_clone() {
            if let Some(p) = &*player_clone.borrow() {
                if p.is_paused() {
                    // Re-sync sequencer with current piano roll data before
                    // resuming so that edits made while paused take effect.
                    let current_time = p.get_time();
                    if let Err(e) = p.hot_swap(midi, current_time) {
                        eprintln!("Failed to hot-swap on resume: {}", e);
                    }
                    p.resume();
                } else if !p.is_playing() {
                    if let Err(e) = p.play(midi) {
                        eprintln!("Failed to play: {}", e);
                    }
                }
                *is_playing_clone.borrow_mut() = true;
            }
        }
    });

    let player_clone_stop = player.clone();
    let is_playing_stop = is_playing.clone();
    pause_btn.connect_clicked(move |_| {
        if let Some(p) = &*player_clone_stop.borrow() {
            p.pause();
            *is_playing_stop.borrow_mut() = false;
        }
    });

    let player_clone_rewind = player.clone();
    let is_playing_rewind = is_playing.clone();
    let pr_rewind = piano_roll.clone();
    rewind_btn.connect_clicked(move |_| {
        if let Some(midi) = pr_rewind.get_data_clone() {
            if let Some(p) = &*player_clone_rewind.borrow() {
                if let Err(e) = p.play(midi) {
                    eprintln!("Failed to play: {}", e);
                } else {
                    *is_playing_rewind.borrow_mut() = true;
                }
            }
        }
    });

    let pr_track = piano_roll.clone();
    track_dropdown.connect_selected_notify(move |dd| {
        let selected = dd.selected();
        if selected != gtk::INVALID_LIST_POSITION {
            pr_track.set_active_track(selected as usize);
        }
    });

    let key_ctrl = gtk::EventControllerKey::new();
    let player_key = player.clone();
    let is_playing_key = is_playing.clone();
    let pr_key = piano_roll.clone();
    key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::space {
            let mut playing = is_playing_key.borrow_mut();
            if let Some(p) = &*player_key.borrow() {
                if p.is_playing() && !p.is_paused() {
                    p.pause();
                    *playing = false;
                } else {
                    if p.is_paused() {
                        // Re-sync sequencer with current piano roll data
                        // before resuming so that edits made while paused
                        // take effect.
                        if let Some(midi) = pr_key.get_data_clone() {
                            let current_time = p.get_time();
                            if let Err(e) = p.hot_swap(midi, current_time) {
                                eprintln!("Failed to hot-swap on resume: {}", e);
                            }
                        }
                        p.resume();
                    } else if let Some(midi) = pr_key.get_data_clone() {
                        if let Err(e) = p.play(midi) {
                            eprintln!("Failed to play: {}", e);
                        }
                    }
                    *playing = true;
                }
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_ctrl);

    // GUI update timer
    let pr_timer = piano_roll.clone();
    let player_timer = player.clone();
    let is_playing_timer = is_playing.clone();

    glib::timeout_add_local(Duration::from_millis(16), move || {
        if *is_playing_timer.borrow() {
            if let Some(p) = &*player_timer.borrow() {
                if p.is_playing() {
                    let time = p.get_time();
                    pr_timer.set_playhead(time);
                } else {
                    *is_playing_timer.borrow_mut() = false;
                }
            }
        }
        glib::ControlFlow::Continue
    });

    // Provide player to piano roll for seek callback
    let player_seek = player.clone();
    piano_roll.connect_seek(move |time| {
        if let Some(p) = &*player_seek.borrow() {
            p.seek(time);
        }
    });

    let player_data_changed = player.clone();
    let pr_data_changed = piano_roll.clone();
    let is_playing_changed = is_playing.clone();
    piano_roll.connect_data_changed(move || {
        if *is_playing_changed.borrow() {
            if let Some(p) = &*player_data_changed.borrow() {
                if let Some(midi) = pr_data_changed.get_data_clone() {
                    let current_time = p.get_time();
                    if let Err(e) = p.hot_swap(midi, current_time) {
                        eprintln!("Failed to hot-swap: {}", e);
                    }
                }
            }
        }
    });

    let player_preview_on = player.clone();
    piano_roll.connect_preview_note_on(move |synth_index, pitch, vel| {
        if let Some(p) = &*player_preview_on.borrow() {
            p.preview_note_on(synth_index, pitch, vel);
        }
    });

    let player_preview_off = player.clone();
    piano_roll.connect_preview_note_off(move |synth_index, pitch| {
        if let Some(p) = &*player_preview_off.borrow() {
            p.preview_note_off(synth_index, pitch);
        }
    });

    // Plugin GUI button: toggle the CLAP plugin's floating window.
    let player_gui = player.clone();
    let track_dropdown_gui = track_dropdown.clone();
    let piano_roll_gui = piano_roll.clone();
    plugin_gui_btn.connect_clicked(move |_btn| {
        let track = track_dropdown_gui.selected() as usize;
        let synth_index = piano_roll_gui.track_synth_index(track);
        if let Some(p) = &mut *player_gui.borrow_mut() {
            if p.is_plugin_gui_open(synth_index) {
                p.close_plugin_gui(synth_index);
            } else {
                p.open_plugin_gui(synth_index);
            }
        }
    });

    // Gracefully shut down audio on window close to avoid pop/click.
    let player_shutdown = player.clone();
    window.connect_close_request(move |_| {
        if let Some(p) = &mut *player_shutdown.borrow_mut() {
            // Close any open plugin GUIs before shutting down audio.
            for i in 0..p.gui_handle_count() {
                p.close_plugin_gui(i);
            }
            p.shutdown();
        }
        glib::Propagation::Proceed
    });

    // Poll CLAP plugin callbacks every 16 ms (~60 Hz).
    // This ensures `on_main_thread()` is called when plugins request it
    // (e.g. to sync GUI state changes to the audio thread).
    let player_poll = player.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        if let Some(p) = &mut *player_poll.borrow_mut() {
            p.poll_plugin_callbacks();
        }
        glib::ControlFlow::Continue
    });

    window.present();
}
