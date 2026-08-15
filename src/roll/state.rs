//! Mutable roll state shared by piano and drum GObject widgets.

use super::types::{
    DragState, EditMode, SelectionRect, has_exact_note, note_name, put_note_length,
    snap_tick_to_beat,
};
use crate::midi::{MidiData, Note, TrackId};
use gtk::gdk;
use gtk4 as gtk;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Debug)]
pub struct PendingPutNote {
    pub track_index: usize,
    pub note_index: usize,
    pub start_tick: u64,
    pub pitch: u8,
    pub channel: u8,
    pub started_at: Instant,
    pub length_quantization_enabled: bool,
}

pub struct RollState {
    pub data: RefCell<Option<MidiData>>,
    pub active_track: RefCell<usize>,
    pub selected_notes: RefCell<HashSet<usize>>,

    pub playhead_time: RefCell<f64>,
    pub zoom_x: RefCell<f64>,
    pub zoom_y: RefCell<f64>,
    pub scroll_x: RefCell<f64>,
    pub scroll_y: RefCell<f64>,

    pub edit_mode: RefCell<EditMode>,
    pub drag_state: RefCell<DragState>,
    pub preview_active_pitch: RefCell<Option<u8>>,
    pub selection_rect: RefCell<Option<SelectionRect>>,
    pub cursor_x: RefCell<f64>,
    pub cursor_y: RefCell<f64>,

    pub typing_keyboard_enabled: RefCell<bool>,
    pub typing_pressed_keys: RefCell<HashMap<gdk::Key, u8>>,
    pub typing_octave_offset: RefCell<i8>,
    pub external_pressed_notes: RefCell<HashSet<(u8, u8)>>,
    pub playback_active_pitches: RefCell<HashSet<u8>>,
    pub pending_put_notes: RefCell<HashMap<(u8, u8), PendingPutNote>>,
    pub put_length_quantization_enabled: RefCell<bool>,

    pub default_note_beats: RefCell<f64>,

    #[allow(clippy::type_complexity)]
    pub seek_callback: RefCell<Option<Box<dyn Fn(f64)>>>,
    #[allow(clippy::type_complexity)]
    pub data_changed_callback: RefCell<Option<Box<dyn Fn()>>>,
    #[allow(clippy::type_complexity)]
    pub preview_note_on_callback: RefCell<Option<Box<dyn Fn(usize, u8, u8, u8)>>>,
    #[allow(clippy::type_complexity)]
    pub preview_note_off_callback: RefCell<Option<Box<dyn Fn(usize, u8, u8)>>>,
    #[allow(clippy::type_complexity)]
    pub status_callback: RefCell<Option<Box<dyn Fn(&str)>>>,
}

impl Default for RollState {
    fn default() -> Self {
        Self {
            data: RefCell::new(None),
            active_track: RefCell::new(0),
            selected_notes: RefCell::new(HashSet::new()),
            playhead_time: RefCell::new(0.0),
            zoom_x: RefCell::new(150.0),
            zoom_y: RefCell::new(24.0),
            scroll_x: RefCell::new(0.0),
            scroll_y: RefCell::new(0.0),
            edit_mode: RefCell::new(EditMode::Draw),
            drag_state: RefCell::new(DragState::default()),
            preview_active_pitch: RefCell::new(None),
            selection_rect: RefCell::new(None),
            cursor_x: RefCell::new(0.0),
            cursor_y: RefCell::new(0.0),
            typing_keyboard_enabled: RefCell::new(false),
            typing_pressed_keys: RefCell::new(HashMap::new()),
            typing_octave_offset: RefCell::new(0),
            external_pressed_notes: RefCell::new(HashSet::new()),
            playback_active_pitches: RefCell::new(HashSet::new()),
            pending_put_notes: RefCell::new(HashMap::new()),
            put_length_quantization_enabled: RefCell::new(false),
            default_note_beats: RefCell::new(1.0),
            seek_callback: RefCell::new(None),
            data_changed_callback: RefCell::new(None),
            preview_note_on_callback: RefCell::new(None),
            preview_note_off_callback: RefCell::new(None),
            status_callback: RefCell::new(None),
        }
    }
}

impl RollState {
    pub fn connect_seek<F: Fn(f64) + 'static>(&self, f: F) {
        *self.seek_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_data_changed<F: Fn() + 'static>(&self, f: F) {
        *self.data_changed_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_preview_note_on<F: Fn(usize, u8, u8, u8) + 'static>(&self, f: F) {
        *self.preview_note_on_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_preview_note_off<F: Fn(usize, u8, u8) + 'static>(&self, f: F) {
        *self.preview_note_off_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_status<F: Fn(&str) + 'static>(&self, f: F) {
        *self.status_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_data(&self, midi: MidiData) {
        *self.data.borrow_mut() = Some(midi);
        *self.active_track.borrow_mut() = 0;
        self.selected_notes.borrow_mut().clear();
        self.pending_put_notes.borrow_mut().clear();
    }

    pub fn update_data(&self, midi: MidiData) {
        *self.data.borrow_mut() = Some(midi);
    }

    pub fn notify_data_changed(&self) {
        if let Some(callback) = &*self.data_changed_callback.borrow() {
            callback();
        }
    }

    pub fn get_data_clone(&self) -> Option<MidiData> {
        self.data.borrow().clone()
    }

    pub fn get_playhead_tick(&self) -> f64 {
        let time = *self.playhead_time.borrow();
        if let Some(midi) = &*self.data.borrow() {
            time * midi.ticks_per_beat as f64 * (midi.get_bpm() / 60.0)
        } else {
            0.0
        }
    }

    pub fn active_track_index(&self) -> usize {
        *self.active_track.borrow()
    }

    pub fn active_track_id(&self) -> Option<TrackId> {
        let index = self.active_track_index();
        self.data
            .borrow()
            .as_ref()
            .and_then(|midi| midi.tracks.get(index))
            .map(|track| track.id)
    }

    pub fn track_synth_index(&self, track_idx: usize) -> usize {
        self.data
            .borrow()
            .as_ref()
            .and_then(|midi| midi.tracks.get(track_idx))
            .map(|track| track.synth_index)
            .unwrap_or(track_idx)
    }

    pub fn active_synth_index(&self) -> usize {
        self.track_synth_index(self.active_track_index())
    }

    pub fn set_external_note_active(&self, channel: u8, pitch: u8, active: bool) -> bool {
        let key = (channel, pitch);
        let mut notes = self.external_pressed_notes.borrow_mut();
        if active {
            notes.insert(key)
        } else {
            notes.remove(&key)
        }
    }

    pub fn clear_external_notes(&self) -> bool {
        let mut notes = self.external_pressed_notes.borrow_mut();
        if notes.is_empty() {
            false
        } else {
            notes.clear();
            true
        }
    }

    pub fn set_playback_active_pitches(&self, pitches: impl IntoIterator<Item = u8>) -> bool {
        let next: HashSet<u8> = pitches.into_iter().collect();
        let mut current = self.playback_active_pitches.borrow_mut();
        if *current == next {
            false
        } else {
            *current = next;
            true
        }
    }

    pub fn set_default_note_beats(&self, beats: f64) {
        *self.default_note_beats.borrow_mut() = beats.max(0.0625);
    }

    pub fn get_edit_mode(&self) -> EditMode {
        *self.edit_mode.borrow()
    }

    pub fn is_typing_keyboard_enabled(&self) -> bool {
        *self.typing_keyboard_enabled.borrow()
    }

    pub fn is_normal_mode(&self) -> bool {
        *self.edit_mode.borrow() == EditMode::Draw && !*self.typing_keyboard_enabled.borrow()
    }

    pub fn update_status(&self) {
        let keyboard_status = if *self.typing_keyboard_enabled.borrow() {
            let octave_offset = *self.typing_octave_offset.borrow();
            Some(format!(
                "[Keyboard] Oct {octave_offset:+}  Q={}  Z={}",
                note_name(60 + i16::from(octave_offset) * 12),
                note_name(48 + i16::from(octave_offset) * 12)
            ))
        } else {
            None
        };
        let sel_count = self.selected_notes.borrow().len();
        let mode_status = if *self.edit_mode.borrow() == EditMode::Put {
            if *self.put_length_quantization_enabled.borrow() {
                "Put | Length Quantize: ON"
            } else {
                "Put | Length Quantize: OFF (Quarter)"
            }
        } else {
            self.edit_mode.borrow().label()
        };
        let msg = if let Some(status) = keyboard_status {
            if sel_count > 0 {
                format!("{status}  {sel_count} note(s) selected")
            } else {
                status
            }
        } else if sel_count > 0 {
            format!("[{mode_status}] {sel_count} note(s) selected")
        } else {
            format!("[{mode_status}]")
        };
        if let Some(cb) = &*self.status_callback.borrow() {
            cb(&msg);
        }
    }

    pub fn handle_note_deleted(&self, track_index: usize, note_index: usize) {
        self.pending_put_notes.borrow_mut().retain(|_, pending| {
            if pending.track_index != track_index {
                return true;
            }
            if pending.note_index == note_index {
                return false;
            }
            if pending.note_index > note_index {
                pending.note_index -= 1;
            }
            true
        });
    }

    pub fn put_midi_note_on(
        &self,
        channel: u8,
        pitch: u8,
        velocity: u8,
        occurred_at: Instant,
    ) -> bool {
        if *self.edit_mode.borrow() != EditMode::Put {
            return false;
        }

        if self
            .pending_put_notes
            .borrow()
            .contains_key(&(channel, pitch))
        {
            self.put_midi_note_off(channel, pitch, occurred_at);
        }

        let playhead_tick = self.get_playhead_tick().max(0.0).floor() as u64;
        let active_track = *self.active_track.borrow();
        let length_quantization_enabled = *self.put_length_quantization_enabled.borrow();
        let pending = if let Some(midi) = &mut *self.data.borrow_mut() {
            let start_tick = snap_tick_to_beat(playhead_tick, midi.ticks_per_beat);
            let duration = u64::from(midi.ticks_per_beat).max(1);
            let Some(track) = midi.tracks.get_mut(active_track) else {
                return false;
            };
            track.notes.push(Note {
                pitch,
                velocity,
                start_tick,
                end_tick: start_tick + duration,
                channel,
            });
            Some(PendingPutNote {
                track_index: active_track,
                note_index: track.notes.len() - 1,
                start_tick,
                pitch,
                channel,
                started_at: occurred_at,
                length_quantization_enabled,
            })
        } else {
            None
        };

        if let Some(pending) = pending {
            self.pending_put_notes
                .borrow_mut()
                .insert((channel, pitch), pending);
            self.selected_notes.borrow_mut().clear();
            self.update_status();
            self.notify_data_changed();
            true
        } else {
            false
        }
    }

    pub fn put_midi_note_off(&self, channel: u8, pitch: u8, occurred_at: Instant) -> bool {
        let Some(pending) = self
            .pending_put_notes
            .borrow_mut()
            .remove(&(channel, pitch))
        else {
            return false;
        };
        let elapsed_seconds = occurred_at
            .saturating_duration_since(pending.started_at)
            .as_secs_f64();

        let mut deleted_duplicate = None;
        let changed = if let Some(midi) = &mut *self.data.borrow_mut() {
            let duration = put_note_length(
                pending.length_quantization_enabled,
                elapsed_seconds,
                midi.get_bpm(),
                midi.ticks_per_beat,
            );
            let end_tick = pending.start_tick + duration;
            if let Some(track) = midi.tracks.get_mut(pending.track_index) {
                let pending_still_exists =
                    track.notes.get(pending.note_index).is_some_and(|note| {
                        note.start_tick == pending.start_tick
                            && note.pitch == pending.pitch
                            && note.channel == pending.channel
                    });
                if !pending_still_exists {
                    false
                } else if has_exact_note(
                    &track.notes,
                    Some(pending.note_index),
                    pending.channel,
                    pending.pitch,
                    pending.start_tick,
                    end_tick,
                ) {
                    track.notes.remove(pending.note_index);
                    deleted_duplicate = Some((pending.track_index, pending.note_index));
                    true
                } else {
                    track.notes[pending.note_index].end_tick = end_tick;
                    true
                }
            } else {
                false
            }
        } else {
            false
        };

        if changed {
            if let Some((track_index, note_index)) = deleted_duplicate {
                self.handle_note_deleted(track_index, note_index);
            }
            self.notify_data_changed();
        }
        changed
    }

    pub fn toggle_put_length_quantization(&self) -> bool {
        if *self.edit_mode.borrow() != EditMode::Put {
            return false;
        }
        let enabled = !*self.put_length_quantization_enabled.borrow();
        *self.put_length_quantization_enabled.borrow_mut() = enabled;
        self.update_status();
        true
    }
}
