//! Track list, mixer toggles, and track CRUD wiring.

use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

use crate::midi_input::MidiInputManager;
use crate::player::Player;
use crate::soundbank::SoundbankManager;

use super::helpers::{TrackUi, midi_input_target};
use super::track_panel::TrackPanel;

pub fn wire_track_controls(
    tracks: TrackUi,
    panel: &TrackPanel,
    player: Rc<RefCell<Option<Player>>>,
    midi_manager: Rc<RefCell<Option<MidiInputManager>>>,
    soundbank: Rc<SoundbankManager>,
    status_bar: &gtk::Label,
) {
    let tracks_from_list = tracks.clone();
    panel.list_box.connect_row_selected(move |_, row| {
        if tracks_from_list.syncing.get() {
            return;
        }
        let Some(row) = row else {
            return;
        };
        let index = row.index() as usize;
        tracks_from_list.dropdown.set_selected(index as u32);
        if let Some(midi) = tracks_from_list.roll.get_data_clone()
            && let Some(track) = midi.tracks.get(index)
        {
            tracks_from_list.name_entry.set_text(&track.name);
        }
    });

    let tracks_mute = tracks.clone();
    panel.mute_btn.connect_toggled(move |button| {
        if tracks_mute.syncing.get() {
            return;
        }
        let Some(mut midi) = tracks_mute.roll.get_data_clone() else {
            return;
        };
        let index = tracks_mute.roll.active_track_index();
        let Some(track) = midi.tracks.get_mut(index) else {
            return;
        };
        track.mixer.mute = button.is_active();
        tracks_mute.roll.update_data_and_notify(midi);
    });

    let tracks_solo = tracks.clone();
    panel.solo_btn.connect_toggled(move |button| {
        if tracks_solo.syncing.get() {
            return;
        }
        let Some(mut midi) = tracks_solo.roll.get_data_clone() else {
            return;
        };
        let index = tracks_solo.roll.active_track_index();
        let Some(track) = midi.tracks.get_mut(index) else {
            return;
        };
        track.mixer.solo = button.is_active();
        tracks_solo.roll.update_data_and_notify(midi);
    });

    let tracks_arm = tracks.clone();
    let midi_manager_arm = midi_manager.clone();
    panel.arm_btn.connect_toggled(move |button| {
        if tracks_arm.syncing.get() {
            return;
        }
        let Some(mut midi) = tracks_arm.roll.get_data_clone() else {
            return;
        };
        let active_index = tracks_arm.roll.active_track_index();
        for track in &mut midi.tracks {
            track.input.armed = false;
        }
        let Some(active_track) = midi.tracks.get_mut(active_index) else {
            return;
        };
        active_track.input.armed = button.is_active();
        let track_id = active_track.id;
        let synth_index = active_track.synth_index;
        tracks_arm.roll.update_data(midi);
        if let Some(manager) = midi_manager_arm.borrow().as_ref() {
            manager.set_target_track(track_id, synth_index);
        }
    });

    let tracks_add = tracks.clone();
    panel.add_btn.connect_clicked(move |_| {
        let Some(mut midi) = tracks_add.roll.get_data_clone() else {
            return;
        };
        let (synth_index, synth_source) = midi
            .tracks
            .get(tracks_add.roll.active_track_index())
            .map(|track| (track.synth_index, track.synth_source.clone()))
            .unwrap_or_else(|| (0, crate::midi::SynthSource::default()));
        let name = format!("Track {}", midi.tracks.len() + 1);
        let new_track = midi.add_track(name, synth_index);
        if let Some(track) = midi.tracks.last_mut() {
            track.synth_source = synth_source;
        }
        tracks_add.install(midi, new_track, true);
    });

    let tracks_instrument = tracks.clone();
    let instrument_manager = soundbank;
    let player_instrument = player;
    panel.instrument_btn.connect_clicked(move |btn| {
        let Some(active_track) = tracks_instrument.roll.active_track_id() else {
            return;
        };

        let mut dialog_builder = gtk::Window::builder()
            .title("Select Instrument")
            .modal(true)
            .default_width(400)
            .default_height(500);
        if let Some(parent) = btn
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        {
            dialog_builder = dialog_builder.transient_for(&parent);
        }
        let dialog = dialog_builder.build();

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        dialog.set_child(Some(&content));

        let search_entry = gtk::SearchEntry::new();
        search_entry.set_margin_top(8);
        search_entry.set_margin_bottom(8);
        search_entry.set_margin_start(8);
        search_entry.set_margin_end(8);
        content.append(&search_entry);

        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_vexpand(true);
        let listbox = gtk::ListBox::new();
        listbox.set_selection_mode(gtk::SelectionMode::Single);
        scrolled.set_child(Some(&listbox));
        content.append(&scrolled);

        for bank in &instrument_manager.banks {
            let row = gtk::ListBoxRow::new();
            let label = gtk::Label::new(Some(&bank.name));
            label.set_halign(gtk::Align::Start);
            label.set_margin_start(8);
            label.set_margin_top(8);
            label.set_margin_bottom(8);
            row.set_child(Some(&label));
            listbox.append(&row);
        }

        let listbox_filter = listbox.clone();
        search_entry.connect_search_changed(move |entry| {
            let text = entry.text().to_lowercase();
            listbox_filter.set_filter_func(move |row| {
                if let Some(child) = row.child()
                    && let Some(label) = child.downcast_ref::<gtk::Label>()
                {
                    return label.text().to_lowercase().contains(&text);
                }
                true
            });
        });

        let dialog_clone = dialog.clone();
        let p_inst = player_instrument.clone();
        let tracks_inst = tracks_instrument.clone();
        let manager_clone = instrument_manager.clone();

        listbox.connect_row_activated(move |_, row| {
            if let Some(child) = row.child()
                && let Some(label) = child.downcast_ref::<gtk::Label>()
            {
                let text = label.text().to_string();
                if let Some(bank) = manager_clone.banks.iter().find(|b| b.name == text)
                    && let Some(p) = &mut *p_inst.borrow_mut()
                {
                    match p.add_or_get_synth(&bank.source) {
                        Ok(new_synth_idx) => {
                            if let Some(mut midi) = tracks_inst.roll.get_data_clone() {
                                if let Some(track) =
                                    midi.tracks.iter_mut().find(|t| t.id == active_track)
                                {
                                    track.synth_source = bank.source.clone();
                                    track.synth_index = new_synth_idx;
                                }
                                tracks_inst.install(midi, active_track, true);
                            }
                        }
                        Err(e) => eprintln!("Failed to load synth: {}", e),
                    }
                }
            }
            dialog_clone.close();
        });

        dialog.present();
    });

    let tracks_duplicate = tracks.clone();
    panel.duplicate_btn.connect_clicked(move |_| {
        let Some(mut midi) = tracks_duplicate.roll.get_data_clone() else {
            return;
        };
        let Some(active_track) = tracks_duplicate.roll.active_track_id() else {
            return;
        };
        let Some(new_track) = midi.duplicate_track(active_track) else {
            return;
        };
        tracks_duplicate.install(midi, new_track, true);
    });

    let tracks_delete = tracks.clone();
    let status_delete = status_bar.clone();
    panel.delete_btn.connect_clicked(move |_| {
        let Some(mut midi) = tracks_delete.roll.get_data_clone() else {
            return;
        };
        let Some(active_track) = tracks_delete.roll.active_track_id() else {
            return;
        };
        let old_index = midi.track_index(active_track).unwrap_or(0);
        if !midi.remove_track(active_track) {
            status_delete.set_text("[Tracks] At least one track must remain");
            return;
        }
        let next_track = midi.tracks[old_index.min(midi.tracks.len() - 1)].id;
        tracks_delete.install(midi, next_track, true);
    });

    let tracks_rename = tracks.clone();
    panel.rename_btn.connect_clicked(move |_| {
        let new_name = tracks_rename.name_entry.text().trim().to_string();
        if new_name.is_empty() {
            return;
        }
        let Some(mut midi) = tracks_rename.roll.get_data_clone() else {
            return;
        };
        let Some(active_track) = tracks_rename.roll.active_track_id() else {
            return;
        };
        let Some(index) = midi.track_index(active_track) else {
            return;
        };
        midi.tracks[index].name = new_name;
        tracks_rename.install(midi, active_track, true);
    });

    for (button, direction) in [(&panel.move_up_btn, -1isize), (&panel.move_down_btn, 1)] {
        let tracks_move = tracks.clone();
        button.connect_clicked(move |_| {
            let Some(mut midi) = tracks_move.roll.get_data_clone() else {
                return;
            };
            let Some(active_track) = tracks_move.roll.active_track_id() else {
                return;
            };
            let Some(old_index) = midi.track_index(active_track) else {
                return;
            };
            let new_index =
                (old_index as isize + direction).clamp(0, midi.tracks.len() as isize - 1) as usize;
            if !midi.move_track(active_track, new_index) {
                return;
            }
            tracks_move.install(midi, active_track, true);
        });
    }

    let tracks_select = tracks;
    let midi_manager_track = midi_manager;
    tracks_select.dropdown.connect_selected_notify(move |dd| {
        let selected = dd.selected();
        if selected != gtk::INVALID_LIST_POSITION {
            tracks_select.roll.set_active_track(selected as usize);
            let track_data = tracks_select
                .roll
                .get_data_clone()
                .and_then(|midi| midi.tracks.get(selected as usize).cloned());
            let target_track = tracks_select
                .roll
                .get_data_clone()
                .and_then(|midi| midi_input_target(&midi, selected as usize));
            if let Some(manager) = midi_manager_track.borrow().as_ref()
                && let Some((track_id, synth_index)) = target_track
            {
                manager.set_target_track(track_id, synth_index);
            }
            if let Some(track) = &track_data {
                let was_syncing = tracks_select.syncing.replace(true);
                tracks_select.mute.set_active(track.mixer.mute);
                tracks_select.solo.set_active(track.mixer.solo);
                tracks_select.arm.set_active(track.input.armed);
                tracks_select.syncing.set(was_syncing);
            }
            if !tracks_select.syncing.get() {
                if let Some(row) = tracks_select.list_box.row_at_index(selected as i32) {
                    tracks_select.list_box.select_row(Some(&row));
                }
                if let Some(midi) = tracks_select.roll.get_data_clone()
                    && let Some(track) = midi.tracks.get(selected as usize)
                {
                    tracks_select.name_entry.set_text(&track.name);
                }
            }
        }
    });
}
