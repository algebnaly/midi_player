//! Shared helpers for the GTK window: MIDI port matching, track list rebuild,
//! and the widget bundle used whenever the active track changes.

use gtk::prelude::*;
use gtk::{Box, DropDown, Label, StringList, ToggleButton};
use gtk4 as gtk;
use std::cell::Cell;
use std::rc::Rc;

use crate::app_cache::CachedMidiInput;
use crate::midi::{MidiData, TrackId, TrackMode};
use crate::midi_input::MidiInputPortInfo;
use crate::roll_stack::RollStack;

pub fn cached_midi_port_position(ports: &[MidiInputPortInfo], cached: &CachedMidiInput) -> Option<u32> {
    ports
        .iter()
        .position(|port| port.id == cached.port_id)
        .or_else(|| ports.iter().position(|port| port.name == cached.port_name))
        .map(|index| index as u32 + 1)
}

pub fn midi_input_target(midi: &MidiData, active_index: usize) -> Option<(TrackId, usize)> {
    midi.tracks
        .iter()
        .find(|track| track.input.armed)
        .or_else(|| midi.tracks.get(active_index))
        .map(|track| (track.id, track.synth_index))
}

pub fn rebuild_track_widgets(
    midi: &MidiData,
    model: &StringList,
    list_box: &gtk::ListBox,
    selected_track: TrackId,
) {
    model.splice(0, model.n_items(), &[]);
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let mut selected_index = 0;
    for (index, track) in midi.tracks.iter().enumerate() {
        let display_name = if matches!(&track.mode, TrackMode::Drum(_)) {
            format!("🥁 {}", track.name)
        } else {
            track.name.clone()
        };
        model.append(&display_name);

        let row_box = Box::new(gtk::Orientation::Vertical, 2);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);
        row_box.set_margin_start(4);
        row_box.set_margin_end(4);

        let label = Label::new(Some(&display_name));
        label.set_xalign(0.0);
        row_box.append(&label);

        let da = gtk::DrawingArea::new();
        da.set_size_request(200, 30);
        da.set_hexpand(true);

        let notes = track.notes.clone();
        da.set_draw_func(move |_, cr, width, height| {
            let w = width as f64;
            let h = height as f64;

            cr.set_source_rgba(0.15, 0.15, 0.15, 1.0);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();

            if notes.is_empty() {
                return;
            }

            let max_tick = notes.iter().map(|n| n.end_tick).max().unwrap_or(1) as f64;
            let min_pitch = notes.iter().map(|n| n.pitch).min().unwrap_or(0) as f64;
            let max_pitch = notes.iter().map(|n| n.pitch).max().unwrap_or(127) as f64;
            let pitch_range = (max_pitch - min_pitch).max(24.0);
            let pitch_padding = pitch_range * 0.1;
            let range_min = min_pitch - pitch_padding;
            let range_range = pitch_range + 2.0 * pitch_padding;

            cr.set_source_rgba(0.2, 0.6, 1.0, 0.8);
            for note in &notes {
                let x = (note.start_tick as f64 / max_tick) * w;
                let note_w = (((note.end_tick - note.start_tick) as f64 / max_tick) * w).max(1.0);

                let mut y = h - ((note.pitch as f64 - range_min) / range_range) * h;
                let note_h = (1.0 / range_range) * h;
                let note_h = note_h.max(2.0);
                y -= note_h;

                cr.rectangle(x, y, note_w, note_h);
                let _ = cr.fill();
            }
        });

        row_box.append(&da);
        list_box.append(&row_box);
        if track.id == selected_track {
            selected_index = index;
        }
    }

    if let Some(row) = list_box.row_at_index(selected_index as i32) {
        list_box.select_row(Some(&row));
    }
}

/// Widgets that must stay in sync whenever the track list or active track changes.
#[derive(Clone)]
pub struct TrackUi {
    pub roll: RollStack,
    pub model: StringList,
    pub list_box: gtk::ListBox,
    pub dropdown: DropDown,
    pub name_entry: gtk::Entry,
    pub mute: ToggleButton,
    pub solo: ToggleButton,
    pub arm: ToggleButton,
    pub syncing: Rc<Cell<bool>>,
}

impl TrackUi {
    pub fn install(&self, midi: MidiData, selected_track: TrackId, notify: bool) {
        let selected_index = midi.track_index(selected_track).unwrap_or(0);
        let selected_track = midi.tracks[selected_index].id;
        let selected_name = midi.tracks[selected_index].name.clone();

        self.syncing.set(true);
        rebuild_track_widgets(&midi, &self.model, &self.list_box, selected_track);
        self.roll.set_data(midi);
        self.roll.set_active_track(selected_index);
        self.dropdown.set_selected(selected_index as u32);
        self.name_entry.set_text(&selected_name);
        let selected = &self.roll.get_data_clone().unwrap().tracks[selected_index];
        self.mute.set_active(selected.mixer.mute);
        self.solo.set_active(selected.mixer.solo);
        self.arm.set_active(selected.input.armed);
        self.syncing.set(false);

        if notify {
            self.roll.notify_data_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_midi_port_prefers_id_and_falls_back_to_name() {
        let ports = vec![
            MidiInputPortInfo {
                id: "20:0".into(),
                name: "TINY MIDI 1".into(),
            },
            MidiInputPortInfo {
                id: "24:0".into(),
                name: "Other Keyboard".into(),
            },
        ];

        let exact_id = CachedMidiInput {
            port_id: "24:0".into(),
            port_name: "outdated name".into(),
        };
        assert_eq!(cached_midi_port_position(&ports, &exact_id), Some(2));

        let changed_id = CachedMidiInput {
            port_id: "99:0".into(),
            port_name: "TINY MIDI 1".into(),
        };
        assert_eq!(cached_midi_port_position(&ports, &changed_id), Some(1));
    }
}
