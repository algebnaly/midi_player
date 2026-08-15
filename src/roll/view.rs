//! Shared public API implemented by piano and drum roll widgets.

use super::layout::RollLayout;
use super::state::RollState;
use super::types::{EditMode, KEY_WIDTH};
use super::viewport::Viewport;
use crate::midi::{MidiData, TrackId};
use gtk::prelude::*;
use gtk4 as gtk;
use std::time::Instant;

pub trait RollView: Clone + 'static {
    type Layout: RollLayout;

    fn state(&self) -> &RollState;
    fn gtk_widget(&self) -> gtk::Widget;

    fn redraw(&self) {
        self.gtk_widget().queue_draw();
    }

    fn focus_roll(&self) {
        let _ = self.gtk_widget().grab_focus();
    }

    fn set_roll_cursor(&self, name: Option<&str>) {
        self.gtk_widget().set_cursor_from_name(name);
    }

    fn widget_width(&self) -> i32 {
        self.gtk_widget().width()
    }

    fn widget_height(&self) -> i32 {
        self.gtk_widget().height()
    }

    fn build_viewport(&self) -> Viewport {
        let s = self.state();
        Viewport {
            zoom_x: *s.zoom_x.borrow(),
            zoom_y: *s.zoom_y.borrow(),
            scroll_x: *s.scroll_x.borrow(),
            scroll_y: *s.scroll_y.borrow(),
            width: self.widget_width() as f64,
            height: self.widget_height() as f64,
        }
    }

    fn connect_seek<F: Fn(f64) + 'static>(&self, f: F) {
        self.state().connect_seek(f);
    }

    fn connect_data_changed<F: Fn() + 'static>(&self, f: F) {
        self.state().connect_data_changed(f);
    }

    fn connect_preview_note_on<F: Fn(usize, u8, u8, u8) + 'static>(&self, f: F) {
        self.state().connect_preview_note_on(f);
    }

    fn connect_preview_note_off<F: Fn(usize, u8, u8) + 'static>(&self, f: F) {
        self.state().connect_preview_note_off(f);
    }

    fn connect_status<F: Fn(&str) + 'static>(&self, f: F) {
        self.state().connect_status(f);
    }

    fn set_midi(&self, midi: MidiData) {
        self.state().set_data(midi);
        self.redraw();
    }

    fn update_data(&self, midi: MidiData) {
        self.state().update_data(midi);
        self.redraw();
    }

    fn notify_data_changed(&self) {
        self.state().notify_data_changed();
    }

    fn get_data_clone(&self) -> Option<MidiData> {
        self.state().get_data_clone()
    }

    fn get_playhead_tick(&self) -> f64 {
        self.state().get_playhead_tick()
    }

    fn set_playhead_tick(&self, tick: f64) {
        if let Some(midi) = &*self.state().data.borrow() {
            let tps = midi.ticks_per_beat as f64 * (midi.get_bpm() / 60.0);
            if tps > 0.0 {
                self.set_playhead(tick / tps);
            }
        }
    }

    fn set_playhead(&self, time: f64) {
        let s = self.state();
        if s.drag_state.borrow().is_dragging_playhead {
            return;
        }
        *s.playhead_time.borrow_mut() = time;
        let zx = *s.zoom_x.borrow();
        let p_x = time * zx;
        let mut sx = *s.scroll_x.borrow();
        let width = self.widget_width() as f64 - KEY_WIDTH;
        if p_x > sx + width * 0.9 {
            sx = p_x - width * 0.1;
        } else if p_x < sx {
            sx = p_x - width * 0.1;
        }
        if sx < 0.0 {
            sx = 0.0;
        }
        *s.scroll_x.borrow_mut() = sx;
        self.redraw();
    }

    fn set_active_track(&self, track_idx: usize) {
        let s = self.state();
        *s.active_track.borrow_mut() = track_idx;
        s.selected_notes.borrow_mut().clear();
        s.playback_active_pitches.borrow_mut().clear();
        self.redraw();
    }

    fn active_track_id(&self) -> Option<TrackId> {
        self.state().active_track_id()
    }

    fn track_synth_index(&self, track_idx: usize) -> usize {
        self.state().track_synth_index(track_idx)
    }

    fn active_synth_index(&self) -> usize {
        self.state().active_synth_index()
    }

    fn set_external_note_active(&self, channel: u8, pitch: u8, active: bool) {
        self.state()
            .set_external_note_active(channel, pitch, active);
        self.redraw();
    }

    fn clear_external_notes(&self) {
        if self.state().clear_external_notes() {
            self.redraw();
        }
    }

    fn set_playback_active_pitches(&self, pitches: impl IntoIterator<Item = u8>) {
        if self.state().set_playback_active_pitches(pitches) {
            self.redraw();
        }
    }

    fn put_midi_note_on(&self, channel: u8, pitch: u8, velocity: u8, occurred_at: Instant) -> bool {
        if self
            .state()
            .put_midi_note_on(channel, pitch, velocity, occurred_at)
        {
            self.redraw();
            true
        } else {
            false
        }
    }

    fn put_midi_note_off(&self, channel: u8, pitch: u8, occurred_at: Instant) -> bool {
        if self.state().put_midi_note_off(channel, pitch, occurred_at) {
            self.redraw();
            true
        } else {
            false
        }
    }

    fn handle_note_deleted(&self, track_index: usize, note_index: usize) {
        self.state().handle_note_deleted(track_index, note_index);
    }

    fn set_default_note_beats(&self, beats: f64) {
        self.state().set_default_note_beats(beats);
    }

    fn set_edit_mode(&self, mode: EditMode) {
        *self.state().edit_mode.borrow_mut() = mode;
        if mode != EditMode::Draw {
            self.set_typing_keyboard_enabled(false);
        }
        *self.state().selection_rect.borrow_mut() = None;
        self.update_status();
        self.redraw();
    }

    fn get_edit_mode(&self) -> EditMode {
        self.state().get_edit_mode()
    }

    fn toggle_put_length_quantization(&self) -> bool {
        self.state().toggle_put_length_quantization()
    }

    fn update_status(&self) {
        self.state().update_status();
    }

    fn set_typing_keyboard_enabled(&self, enabled: bool) {
        let s = self.state();
        *s.typing_keyboard_enabled.borrow_mut() = enabled;
        if enabled {
            *s.edit_mode.borrow_mut() = EditMode::Draw;
            *s.selection_rect.borrow_mut() = None;
        }
        if !enabled {
            self.release_all_typing_keys();
        }
        self.update_status();
        self.redraw();
    }

    fn is_typing_keyboard_enabled(&self) -> bool {
        self.state().is_typing_keyboard_enabled()
    }

    fn is_normal_mode(&self) -> bool {
        self.state().is_normal_mode()
    }

    fn enter_normal_mode(&self) {
        self.set_typing_keyboard_enabled(false);
        self.set_edit_mode(EditMode::Draw);
    }

    fn enter_select_mode(&self) {
        if self.is_normal_mode() {
            self.set_edit_mode(EditMode::Select);
        }
    }

    fn enter_put_mode(&self) {
        if self.is_normal_mode() {
            self.set_edit_mode(EditMode::Put);
        }
    }

    fn enter_typing_keyboard_mode(&self) {
        if self.is_normal_mode() {
            self.set_typing_keyboard_enabled(true);
        }
    }

    fn release_all_typing_keys(&self) {
        let s = self.state();
        let pitches: Vec<u8> = s.typing_pressed_keys.borrow().values().copied().collect();
        let synth_index = self.active_synth_index();
        let channel = Self::Layout::note_channel();
        for pitch in pitches {
            if let Some(cb) = &*s.preview_note_off_callback.borrow() {
                cb(synth_index, pitch, channel);
            }
        }
        s.typing_pressed_keys.borrow_mut().clear();
        *s.preview_active_pitch.borrow_mut() = None;
        self.redraw();
    }
}
