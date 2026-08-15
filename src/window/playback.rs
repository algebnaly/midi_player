//! Playback transport, BPM hot-swap, preview notes, and plugin GUI.

use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::midi_input::MidiInputManager;
use crate::player::Player;
use crate::roll_stack::RollStack;

use super::helpers::midi_input_target;

pub fn wire_playback(
    window: &gtk::ApplicationWindow,
    play_btn: &gtk::Button,
    pause_btn: &gtk::Button,
    rewind_btn: &gtk::Button,
    plugin_gui_btn: &gtk::Button,
    bpm_spin: &gtk::SpinButton,
    piano_roll: &RollStack,
    player: Rc<RefCell<Option<Player>>>,
    is_playing: Rc<RefCell<bool>>,
    midi_manager: Rc<RefCell<Option<MidiInputManager>>>,
    track_dropdown: &gtk::DropDown,
) {
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

            if *is_playing_bpm.borrow()
                && let Some(p) = &*player_bpm.borrow()
            {
                let new_time = current_tick / (midi.ticks_per_beat as f64 * (new_bpm / 60.0));
                if let Err(e) = p.hot_swap(midi.clone(), new_time) {
                    eprintln!("Failed to hot-swap on BPM change: {}", e);
                }
            }
        }
    });

    let player_clone = player.clone();
    let is_playing_clone = is_playing.clone();
    let pr_play = piano_roll.clone();
    play_btn.connect_clicked(move |_| {
        if let Some(midi) = pr_play.get_data_clone()
            && let Some(p) = &*player_clone.borrow()
        {
            if p.is_paused() {
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
        if let Some(midi) = pr_rewind.get_data_clone()
            && let Some(p) = &*player_clone_rewind.borrow()
        {
            if let Err(e) = p.play(midi) {
                eprintln!("Failed to play: {}", e);
            } else {
                *is_playing_rewind.borrow_mut() = true;
            }
        }
    });

    let key_ctrl = gtk::EventControllerKey::new();
    let player_key = player.clone();
    let is_playing_key = is_playing.clone();
    let pr_key = piano_roll.clone();
    key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
        if (keyval == gtk::gdk::Key::l || keyval == gtk::gdk::Key::L)
            && pr_key.toggle_put_length_quantization()
        {
            pr_key.grab_focus();
            return glib::Propagation::Stop;
        }
        if (keyval == gtk::gdk::Key::p || keyval == gtk::gdk::Key::P) && pr_key.is_normal_mode() {
            pr_key.enter_put_mode();
            pr_key.grab_focus();
            return glib::Propagation::Stop;
        }
        if keyval == gtk::gdk::Key::space {
            let mut playing = is_playing_key.borrow_mut();
            if let Some(p) = &*player_key.borrow() {
                if p.is_playing() && !p.is_paused() {
                    p.pause();
                    *playing = false;
                } else {
                    if p.is_paused() {
                        if let Some(midi) = pr_key.get_data_clone() {
                            let current_time = p.get_time();
                            if let Err(e) = p.hot_swap(midi, current_time) {
                                eprintln!("Failed to hot-swap on resume: {}", e);
                            }
                        }
                        p.resume();
                    } else if let Some(midi) = pr_key.get_data_clone()
                        && let Err(e) = p.play(midi)
                    {
                        eprintln!("Failed to play: {}", e);
                    }
                    *playing = true;
                }
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_ctrl);

    let pr_timer = piano_roll.clone();
    let player_timer = player.clone();
    let is_playing_timer = is_playing.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        if *is_playing_timer.borrow() {
            if let Some(p) = &*player_timer.borrow() {
                if p.is_playing() {
                    let Some(track_id) = pr_timer.active_track_id() else {
                        return glib::ControlFlow::Continue;
                    };
                    let (time, active_pitches) = p.playback_snapshot(track_id);
                    pr_timer.set_playhead(time);
                    pr_timer.set_playback_active_pitches(active_pitches);
                } else {
                    *is_playing_timer.borrow_mut() = false;
                    pr_timer.set_playback_active_pitches([]);
                }
            }
        } else {
            pr_timer.set_playback_active_pitches([]);
        }
        glib::ControlFlow::Continue
    });

    let player_seek = player.clone();
    piano_roll.connect_seek(move |time| {
        if let Some(p) = &*player_seek.borrow() {
            p.seek(time);
        }
    });

    let player_data_changed = player.clone();
    let pr_data_changed = piano_roll.clone();
    let is_playing_changed = is_playing;
    let midi_manager_data_changed = midi_manager.clone();
    piano_roll.connect_data_changed(move || {
        if let Some(midi) = pr_data_changed.get_data_clone()
            && let Some((track_id, synth_index)) =
                midi_input_target(&midi, pr_data_changed.active_track_index())
            && let Some(manager) = midi_manager_data_changed.borrow().as_ref()
        {
            manager.set_target_track(track_id, synth_index);
        }
        if *is_playing_changed.borrow()
            && let Some(p) = &*player_data_changed.borrow()
            && let Some(midi) = pr_data_changed.get_data_clone()
        {
            let current_time = p.get_time();
            if let Err(e) = p.hot_swap(midi, current_time) {
                eprintln!("Failed to hot-swap: {}", e);
            }
        }
    });

    let player_preview_on = player.clone();
    piano_roll.connect_preview_note_on(move |synth_index, pitch, vel, channel| {
        if let Some(p) = &*player_preview_on.borrow() {
            p.preview_note_on(synth_index, channel, pitch, vel);
        }
    });

    let player_preview_off = player.clone();
    piano_roll.connect_preview_note_off(move |synth_index, pitch, channel| {
        if let Some(p) = &*player_preview_off.borrow() {
            p.preview_note_off(synth_index, channel, pitch);
        }
    });

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

    let player_shutdown = player.clone();
    let midi_manager_shutdown = midi_manager;
    window.connect_close_request(move |_| {
        if let Some(manager) = midi_manager_shutdown.borrow_mut().as_mut() {
            manager.disconnect();
        }
        if let Some(p) = &mut *player_shutdown.borrow_mut() {
            for i in 0..p.gui_handle_count() {
                p.close_plugin_gui(i);
            }
            p.shutdown();
        }
        *player_shutdown.borrow_mut() = None;
        glib::Propagation::Proceed
    });

    let player_poll = player;
    glib::timeout_add_local(Duration::from_millis(16), move || {
        if let Some(p) = &mut *player_poll.borrow_mut() {
            p.poll_plugin_callbacks();
        }
        glib::ControlFlow::Continue
    });
}
