//! Physical MIDI input dropdown, cache restore, and device refresh.

use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::app_cache::{CachedMidiInput, clear_midi_input, load_midi_input, save_midi_input};
use crate::midi_input::{MidiInputManager, MidiInputPortInfo, MidiUiEvent};
use crate::roll_stack::RollStack;

use super::helpers::cached_midi_port_position;

pub fn wire_midi_input(
    midi_input_list: &gtk::StringList,
    midi_input_dropdown: &gtk::DropDown,
    midi_refresh_btn: &gtk::Button,
    midi_manager: Rc<RefCell<Option<MidiInputManager>>>,
    midi_ui_rx: Option<crossbeam_channel::Receiver<MidiUiEvent>>,
    piano_roll: &RollStack,
    status_bar: &gtk::Label,
) -> Rc<RefCell<Vec<MidiInputPortInfo>>> {
    let midi_ports = Rc::new(RefCell::new(Vec::<MidiInputPortInfo>::new()));
    match MidiInputManager::port_infos() {
        Ok(ports) => {
            for port in &ports {
                midi_input_list.append(&port.name);
            }
            *midi_ports.borrow_mut() = ports;
        }
        Err(err) => eprintln!("Failed to enumerate MIDI inputs: {err}"),
    }

    if let Some(midi_ui_rx) = midi_ui_rx {
        let pr_midi_visual = piano_roll.clone();
        glib::timeout_add_local(Duration::from_millis(8), move || {
            for event in midi_ui_rx.try_iter() {
                pr_midi_visual.set_external_note_active(event.channel, event.pitch, event.active);
                if event.active {
                    pr_midi_visual.put_midi_note_on(
                        event.channel,
                        event.pitch,
                        event.velocity,
                        event.occurred_at,
                    );
                } else {
                    pr_midi_visual.put_midi_note_off(event.channel, event.pitch, event.occurred_at);
                }
            }
            glib::ControlFlow::Continue
        });
    }

    let midi_manager_select = midi_manager.clone();
    let midi_ports_select = midi_ports.clone();
    let pr_midi_select = piano_roll.clone();
    let status_midi_select = status_bar.clone();
    let suppress_midi_selection = Rc::new(Cell::new(false));
    let suppress_midi_select = suppress_midi_selection.clone();
    midi_input_dropdown.connect_selected_notify(move |dropdown| {
        if suppress_midi_select.get() {
            return;
        }
        let selected = dropdown.selected();
        if selected == gtk::INVALID_LIST_POSITION {
            return;
        }

        if selected == 0 {
            if let Some(manager) = midi_manager_select.borrow_mut().as_mut() {
                manager.disconnect();
            }
            pr_midi_select.clear_external_notes();
            if let Err(err) = clear_midi_input() {
                eprintln!("Failed to clear cached MIDI input: {err}");
            }
            status_midi_select.set_text("[MIDI] Disconnected");
            return;
        }

        let Some(port) = midi_ports_select
            .borrow()
            .get(selected as usize - 1)
            .cloned()
        else {
            status_midi_select.set_text("[MIDI] Selected input is no longer available");
            return;
        };

        let mut manager_ref = midi_manager_select.borrow_mut();
        let Some(manager) = manager_ref.as_mut() else {
            status_midi_select.set_text("[MIDI] Audio engine unavailable");
            return;
        };

        match manager.connect(&port.id) {
            Ok(name) => {
                if let Err(err) = save_midi_input(&CachedMidiInput {
                    port_id: port.id,
                    port_name: port.name,
                }) {
                    eprintln!("Failed to cache MIDI input: {err}");
                }
                status_midi_select.set_text(&format!("[MIDI] Connected: {name}"));
            }
            Err(err) => {
                pr_midi_select.clear_external_notes();
                status_midi_select.set_text(&format!("[MIDI] Connection failed: {err}"));
                eprintln!("Failed to connect MIDI input: {err}");
            }
        }
    });

    match load_midi_input() {
        Ok(Some(cached)) => {
            let selected = cached_midi_port_position(&midi_ports.borrow(), &cached);
            if let Some(selected) = selected {
                midi_input_dropdown.set_selected(selected);
            } else {
                status_bar.set_text(&format!(
                    "[MIDI] Last input unavailable: {}",
                    cached.port_name
                ));
            }
        }
        Ok(None) => {}
        Err(err) => eprintln!("Failed to load cached MIDI input: {err}"),
    }

    let midi_manager_refresh = midi_manager;
    let midi_ports_refresh = midi_ports.clone();
    let midi_list_refresh = midi_input_list.clone();
    let midi_dropdown_refresh = midi_input_dropdown.clone();
    let pr_midi_refresh = piano_roll.clone();
    let status_midi_refresh = status_bar.clone();
    let suppress_midi_refresh = suppress_midi_selection;
    midi_refresh_btn.connect_clicked(move |_| {
        if let Some(manager) = midi_manager_refresh.borrow_mut().as_mut() {
            manager.disconnect();
        }
        pr_midi_refresh.clear_external_notes();
        suppress_midi_refresh.set(true);
        midi_dropdown_refresh.set_selected(0);
        midi_list_refresh.splice(0, midi_list_refresh.n_items(), &["No MIDI Input"]);
        match MidiInputManager::port_infos() {
            Ok(ports) => {
                for port in &ports {
                    midi_list_refresh.append(&port.name);
                }
                let port_count = ports.len();
                *midi_ports_refresh.borrow_mut() = ports;
                suppress_midi_refresh.set(false);

                let mut cache_status_set = false;
                match load_midi_input() {
                    Ok(Some(cached)) => {
                        let selected =
                            cached_midi_port_position(&midi_ports_refresh.borrow(), &cached);
                        if let Some(selected) = selected {
                            midi_dropdown_refresh.set_selected(selected);
                            cache_status_set = true;
                        } else {
                            status_midi_refresh.set_text(&format!(
                                "[MIDI] Last input unavailable: {}",
                                cached.port_name
                            ));
                            cache_status_set = true;
                        }
                    }
                    Ok(None) => {}
                    Err(err) => eprintln!("Failed to load cached MIDI input: {err}"),
                }
                if !cache_status_set {
                    status_midi_refresh
                        .set_text(&format!("[MIDI] Found {port_count} input device(s)"));
                }
            }
            Err(err) => {
                suppress_midi_refresh.set(false);
                status_midi_refresh.set_text(&format!("[MIDI] Refresh failed: {err}"));
                eprintln!("Failed to refresh MIDI inputs: {err}");
            }
        }
    });

    midi_ports
}
