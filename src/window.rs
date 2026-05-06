use glib::clone;
use gtk::prelude::*;
use gtk::{ApplicationWindow, Box, Button, DropDown, HeaderBar, StringList};
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::midi::MidiData;
use crate::piano_roll::PianoRollWidget;
use crate::player::Player;

pub fn build_ui(app: &gtk::Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Rust MIDI Player & Editor")
        .default_width(1024)
        .default_height(768)
        .build();

    let header_bar = HeaderBar::new();
    window.set_titlebar(Some(&header_bar));

    let open_btn = Button::with_label("Open");
    let save_btn = Button::with_label("Export");
    let play_btn = Button::with_label("Play");
    let stop_btn = Button::with_label("Stop");

    let track_list = StringList::new(&[]);
    let track_dropdown = DropDown::new(Some(track_list.clone()), gtk::Expression::NONE);

    header_bar.pack_start(&open_btn);
    header_bar.pack_start(&save_btn);
    header_bar.pack_start(&track_dropdown);

    header_bar.pack_end(&stop_btn);
    header_bar.pack_end(&play_btn);

    let vbox = Box::new(gtk::Orientation::Vertical, 0);
    window.set_child(Some(&vbox));

    let piano_roll = Rc::new(PianoRollWidget::new());
    vbox.append(&piano_roll.widget);

    let player = Rc::new(RefCell::new(match Player::new("default.sf2") {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!(
                "Failed to initialize player: {}. Audio playback disabled.",
                e
            );
            None
        }
    }));

    let current_midi_path = Rc::new(RefCell::new(None::<String>));
    let is_playing = Rc::new(RefCell::new(false));

    // Open action
    let window_clone = window.clone();
    let current_midi_clone = current_midi_path.clone();
    let piano_roll_clone = piano_roll.clone();
    let track_list_clone = track_list.clone();

    open_btn.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::new();
        let window = window_clone.clone();
        let midi_path = current_midi_clone.clone();
        let pr = piano_roll_clone.clone();
        let tl = track_list_clone.clone();

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
                        pr.set_data(data);
                        pr.set_playhead(0.0);
                    }
                    Err(e) => eprintln!("Failed to load midi: {}", e),
                }
            }
        });
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
                let buf = midi.to_buffer();
                if let Err(e) = p.play_buffer(&buf) {
                    eprintln!("Failed to play: {}", e);
                } else {
                    *is_playing_clone.borrow_mut() = true;
                }
            }
        }
    });

    let player_clone_stop = player.clone();
    let is_playing_stop = is_playing.clone();
    stop_btn.connect_clicked(move |_| {
        if let Some(p) = &*player_clone_stop.borrow() {
            p.stop();
            *is_playing_stop.borrow_mut() = false;
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
            if *playing {
                if let Some(p) = &*player_key.borrow() {
                    p.stop();
                }
                *playing = false;
            } else {
                if let Some(midi) = pr_key.get_data_clone() {
                    if let Some(p) = &*player_key.borrow() {
                        let buf = midi.to_buffer();
                        if let Err(e) = p.play_buffer(&buf) {
                            eprintln!("Failed to play: {}", e);
                        } else {
                            *playing = true;
                        }
                    }
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

    window.present();
}
