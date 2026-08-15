//! Shared input handling for roll widgets, parameterized by [`RollLayout`].

use super::keys::{
    ModeKeyAction, ScrollAction, is_typing_octave_down_key, is_typing_octave_up_key,
    mode_key_action_from_state, playhead_time_for_click, remove_released_typing_key, scroll_action,
    typing_key_to_pitch_with_octave,
};
use super::layout::RollLayout;
use super::types::*;
use super::view::RollView;
use super::viewport::Viewport;
use crate::midi::Note;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use std::collections::HashMap;

pub fn get_target_pitch<W: RollView>(widget: &W, y: f64, track_idx: usize) -> Option<u8> {
    let vp = widget.build_viewport();
    let midi = widget.state().data.borrow();
    W::Layout::y_to_pitch(&vp, y, midi.as_ref(), track_idx)
}

pub fn hit_test_note<L: RollLayout>(
    midi: &crate::midi::MidiData,
    track: usize,
    zoom_x: f64,
    zoom_y: f64,
    abs_x: f64,
    target_pitch: u8,
) -> Option<HitTestResult> {
    if track >= midi.tracks.len() {
        return None;
    }
    let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
    let synth_index = midi.tracks[track].synth_index;

    for (i, note) in midi.tracks[track].notes.iter().enumerate() {
        let nx = (note.start_tick as f64 / tps) * zoom_x;
        let nw = L::hit_width(
            ((note.end_tick - note.start_tick) as f64 / tps) * zoom_x,
            zoom_y,
        );

        if abs_x >= nx && abs_x <= nx + nw && target_pitch == note.pitch {
            let mut edge_threshold = NOTE_EDGE_THRESHOLD;
            if nw < edge_threshold * 2.0 {
                edge_threshold = nw / 2.0;
            }
            let drag_mode = if abs_x >= nx + nw - edge_threshold {
                DragMode::ResizeDuration
            } else {
                DragMode::MoveNote
            };
            return Some(HitTestResult {
                note_index: i,
                drag_mode,
                synth_index,
            });
        }
    }
    None
}

pub fn setup_controllers<W: RollView>(widget: &W) {
    let gtk_widget = widget.gtk_widget();

    let right_click = gtk::GestureClick::new();
    right_click.set_button(3);
    let w = widget.clone();
    right_click.connect_pressed(move |_, _n_press, x, y| {
        handle_right_click(&w, x, y);
    });

    let drag = gtk::GestureDrag::new();
    let w = widget.clone();
    drag.connect_drag_begin(move |gesture, start_x, start_y| {
        let state = gesture.current_event_state();
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let control = state.contains(gdk::ModifierType::CONTROL_MASK);
        handle_drag_begin(&w, start_x, start_y, shift, control);
    });
    let w = widget.clone();
    drag.connect_drag_update(move |_, dx, dy| {
        handle_drag_update(&w, dx, dy);
    });
    let w = widget.clone();
    drag.connect_drag_end(move |_, dx, dy| {
        handle_drag_end(&w, dx, dy);
    });

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let w = widget.clone();
    scroll.connect_scroll(move |controller, dx, dy| handle_scroll(&w, controller, dx, dy));

    let key_ctrl = gtk::EventControllerKey::new();
    let w = widget.clone();
    key_ctrl.connect_key_pressed(move |_, keyval, _, _| handle_key_press(&w, keyval));
    let w = widget.clone();
    key_ctrl.connect_key_released(move |_, keyval, _, _| handle_key_released(&w, keyval));

    let motion = gtk::EventControllerMotion::new();
    let w = widget.clone();
    motion.connect_motion(move |_, x, y| {
        handle_motion(&w, x, y);
    });

    gtk_widget.add_controller(right_click);
    gtk_widget.add_controller(drag);
    gtk_widget.add_controller(scroll);
    gtk_widget.add_controller(key_ctrl);
    gtk_widget.add_controller(motion);
}

fn handle_right_click<W: RollView>(widget: &W, x: f64, y: f64) {
    if x < KEY_WIDTH {
        return;
    }
    let s = widget.state();
    if *s.edit_mode.borrow() == EditMode::Select {
        return;
    }
    widget.focus_roll();

    let zx = *s.zoom_x.borrow();
    let scroll_x = *s.scroll_x.borrow();
    let act_track = *s.active_track.borrow();
    let abs_x = x - KEY_WIDTH + scroll_x;
    let target_pitch = get_target_pitch(widget, y, act_track);

    let mut changed = false;
    let mut deleted_note_index = None;
    if let Some(target_pitch) = target_pitch {
        let zy = *s.zoom_y.borrow();
        if let Some(midi) = &mut *s.data.borrow_mut() {
            if let Some(hit) = hit_test_note::<W::Layout>(midi, act_track, zx, zy, abs_x, target_pitch)
            {
                midi.tracks[act_track].notes.remove(hit.note_index);
                deleted_note_index = Some(hit.note_index);
                let mut new_sel = std::collections::HashSet::new();
                for &idx in s.selected_notes.borrow().iter() {
                    if idx == hit.note_index {
                        continue;
                    } else if idx > hit.note_index {
                        new_sel.insert(idx - 1);
                    } else {
                        new_sel.insert(idx);
                    }
                }
                *s.selected_notes.borrow_mut() = new_sel;
                widget.redraw();
                changed = true;
            }
        }
    }
    if changed {
        if let Some(note_index) = deleted_note_index {
            widget.handle_note_deleted(act_track, note_index);
        }
        s.notify_data_changed();
        widget.update_status();
    }
}

fn handle_drag_begin<W: RollView>(
    widget: &W,
    start_x: f64,
    start_y: f64,
    shift_held: bool,
    control_held: bool,
) {
    if start_x < KEY_WIDTH {
        return;
    }
    let s = widget.state();
    widget.focus_roll();

    {
        let mut ds = s.drag_state.borrow_mut();
        ds.is_dragging_playhead = false;
        ds.orig_note = None;
        ds.orig_notes.clear();
        ds.mode = DragMode::None;
        ds.start_x = start_x - KEY_WIDTH;
        ds.start_y = start_y;
    }

    let p_time = *s.playhead_time.borrow();
    let zx = *s.zoom_x.borrow();
    let scroll_x = *s.scroll_x.borrow();
    let sx_adj = start_x - KEY_WIDTH;
    let p_x = p_time * zx - scroll_x;
    let abs_x = sx_adj + scroll_x;
    let edit_mode = *s.edit_mode.borrow();

    if control_held {
        let time = playhead_time_for_click(start_x, scroll_x, zx);
        *s.playhead_time.borrow_mut() = time;
        if let Some(callback) = &*s.seek_callback.borrow() {
            callback(time);
        }
        widget.redraw();
        return;
    }

    if (sx_adj - p_x).abs() < PLAYHEAD_HIT_RADIUS || start_y < TOP_REGION_HEIGHT {
        s.drag_state.borrow_mut().is_dragging_playhead = true;
        let mut t = abs_x / zx;
        if t < 0.0 {
            t = 0.0;
        }
        *s.playhead_time.borrow_mut() = t;
        widget.redraw();
        return;
    }

    let act_track = *s.active_track.borrow();
    let target_pitch = match get_target_pitch(widget, start_y, act_track) {
        Some(p) => p,
        None => return,
    };

    let vp = widget.build_viewport();
    let start_lane = {
        let midi = s.data.borrow();
        W::Layout::y_to_lane(&vp, start_y, midi.as_ref(), act_track)
    };
    s.drag_state.borrow_mut().start_lane = start_lane;

    match edit_mode {
        EditMode::Select => {
            handle_drag_begin_select(widget, abs_x, target_pitch, act_track, shift_held)
        }
        EditMode::Draw | EditMode::Put => {
            handle_drag_begin_draw(widget, abs_x, target_pitch, act_track)
        }
    }
}

fn handle_drag_begin_select<W: RollView>(
    widget: &W,
    abs_x: f64,
    target_pitch: u8,
    act_track: usize,
    shift_held: bool,
) {
    let s = widget.state();
    let zx = *s.zoom_x.borrow();
    let tps = Viewport::ticks_per_sec(
        s.data.borrow().as_ref().map_or(480, |m| m.ticks_per_beat),
        s.data.borrow().as_ref().map_or(120.0, |m| m.get_bpm()),
    );
    s.drag_state.borrow_mut().start_cursor_tick = (abs_x / zx) * tps;

    if let Some(midi) = &*s.data.borrow() {
        let zy = *s.zoom_y.borrow();
        if let Some(hit) = hit_test_note::<W::Layout>(midi, act_track, zx, zy, abs_x, target_pitch) {
            if s.selected_notes.borrow().contains(&hit.note_index) {
                let mut orig_notes = HashMap::new();
                for &idx in s.selected_notes.borrow().iter() {
                    if idx < midi.tracks[act_track].notes.len() {
                        orig_notes.insert(idx, midi.tracks[act_track].notes[idx].clone());
                    }
                }
                {
                    let mut ds = s.drag_state.borrow_mut();
                    ds.mode = DragMode::BulkMove;
                    ds.orig_notes = orig_notes;
                }
                widget.set_roll_cursor(Some("grabbing"));
                return;
            }
        }
    }

    let lane = s.drag_state.borrow().start_lane;
    *s.selection_rect.borrow_mut() = Some(SelectionRect {
        abs_x0: abs_x,
        abs_x1: abs_x,
        lane_lo: lane,
        lane_hi: lane,
    });
    s.drag_state.borrow_mut().mode = DragMode::BoxSelect;
    if shift_held {
        s.drag_state.borrow_mut().base_selection = s.selected_notes.borrow().clone();
    } else {
        s.drag_state.borrow_mut().base_selection.clear();
        s.selected_notes.borrow_mut().clear();
    }
    widget.set_roll_cursor(Some("crosshair"));
    widget.redraw();
}

fn handle_drag_begin_draw<W: RollView>(widget: &W, abs_x: f64, target_pitch: u8, act_track: usize) {
    let s = widget.state();
    let zx = *s.zoom_x.borrow();
    let zy = *s.zoom_y.borrow();
    let channel = W::Layout::note_channel();

    let mut found: Option<(HitTestResult, Note)> = None;
    let mut tps = 1.0;
    if let Some(midi) = &*s.data.borrow() {
        tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
        if let Some(hit) = hit_test_note::<W::Layout>(midi, act_track, zx, zy, abs_x, target_pitch) {
            let note = midi.tracks[act_track].notes[hit.note_index].clone();
            found = Some((hit, note));
        }
    }

    if let Some((hit, note)) = found {
        {
            let mut sel = s.selected_notes.borrow_mut();
            sel.clear();
            sel.insert(hit.note_index);
        }
        {
            let mut ds = s.drag_state.borrow_mut();
            ds.orig_note = Some(note.clone());
            ds.mode = hit.drag_mode;
            ds.start_cursor_tick = (abs_x / zx) * tps;
        }
        if let Some(cb) = &*s.preview_note_on_callback.borrow() {
            cb(hit.synth_index, note.pitch, note.velocity, channel);
        }
        *s.preview_active_pitch.borrow_mut() = Some(note.pitch);
        widget.set_roll_cursor(if hit.drag_mode == DragMode::ResizeDuration {
            Some("col-resize")
        } else {
            Some("grabbing")
        });
        widget.redraw();
    } else {
        let mut synth_index = act_track;
        if let Some(midi) = &mut *s.data.borrow_mut() {
            if act_track < midi.tracks.len() {
                synth_index = midi.tracks[act_track].synth_index;
                let raw_tick = ((abs_x / zx) * tps) as u64;
                let (start_tick, end_tick) = W::Layout::place_new_note(
                    raw_tick,
                    midi.ticks_per_beat,
                    *s.default_note_beats.borrow(),
                );
                let new_note = Note {
                    pitch: target_pitch,
                    velocity: 100,
                    start_tick,
                    end_tick,
                    channel,
                };
                midi.tracks[act_track].notes.push(new_note.clone());
                let new_idx = midi.tracks[act_track].notes.len() - 1;
                {
                    let mut sel = s.selected_notes.borrow_mut();
                    sel.clear();
                    sel.insert(new_idx);
                }
                {
                    let mut ds = s.drag_state.borrow_mut();
                    ds.orig_note = Some(new_note);
                    ds.mode = DragMode::MoveNote;
                    ds.start_cursor_tick = (abs_x / zx) * tps;
                }
                *s.preview_active_pitch.borrow_mut() = Some(target_pitch);
                widget.redraw();
            }
        }
        s.notify_data_changed();
        if let Some(cb) = &*s.preview_note_on_callback.borrow() {
            cb(synth_index, target_pitch, 100, channel);
        }
    }
    widget.update_status();
}

pub fn update_drag_position<W: RollView>(widget: &W, dx: f64, _dy: f64) {
    let s = widget.state();
    let drag_mode = s.drag_state.borrow().mode;

    if s.drag_state.borrow().is_dragging_playhead {
        let sx = s.drag_state.borrow().start_x;
        let zx = *s.zoom_x.borrow();
        let ox = *s.scroll_x.borrow();
        let mut t = (sx + dx + ox) / zx;
        if t < 0.0 {
            t = 0.0;
        }
        *s.playhead_time.borrow_mut() = t;
        widget.redraw();
        return;
    }

    if drag_mode == DragMode::BoxSelect {
        let sx = s.drag_state.borrow().start_x;
        let sy = s.drag_state.borrow().start_y;
        let ox = *s.scroll_x.borrow();
        let vp = widget.build_viewport();
        let act = *s.active_track.borrow();

        let cursor_x = *s.cursor_x.borrow();
        let cursor_y = *s.cursor_y.borrow();

        let abs_x0 = sx + ox;
        let abs_x1 = cursor_x - KEY_WIDTH + ox;

        let (lane0, lane1) = {
            let midi = s.data.borrow();
            (
                W::Layout::y_to_lane(&vp, sy, midi.as_ref(), act),
                W::Layout::y_to_lane(&vp, cursor_y, midi.as_ref(), act),
            )
        };
        let lane_lo = lane0.min(lane1);
        let lane_hi = lane0.max(lane1);

        *s.selection_rect.borrow_mut() = Some(SelectionRect {
            abs_x0: abs_x0.min(abs_x1),
            abs_x1: abs_x0.max(abs_x1),
            lane_lo,
            lane_hi,
        });

        let zx = *s.zoom_x.borrow();
        let mut new_sel = std::collections::HashSet::new();
        if let Some(midi) = &*s.data.borrow() {
            let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
            if act < midi.tracks.len() {
                let x_lo = abs_x0.min(abs_x1);
                let x_hi = abs_x0.max(abs_x1);
                let track = &midi.tracks[act];
                for (i, note) in track.notes.iter().enumerate() {
                    let nx = (note.start_tick as f64 / tps) * zx;
                    let nw = W::Layout::hit_width(
                        ((note.end_tick - note.start_tick) as f64 / tps) * zx,
                        vp.zoom_y,
                    );
                    if nx + nw >= x_lo
                        && nx <= x_hi
                        && W::Layout::note_in_lanes(
                            note.pitch,
                            lane_lo,
                            lane_hi,
                            Some(midi),
                            act,
                        )
                    {
                        new_sel.insert(i);
                    }
                }
            }
        }
        let base = &s.drag_state.borrow().base_selection;
        for &idx in base.iter() {
            new_sel.insert(idx);
        }
        *s.selected_notes.borrow_mut() = new_sel;
        widget.update_status();
        widget.redraw();
        return;
    }

    if drag_mode == DragMode::BulkMove {
        if let Some(midi) = &mut *s.data.borrow_mut() {
            let act = *s.active_track.borrow();
            if act >= midi.tracks.len() {
                return;
            }
            let zx = *s.zoom_x.borrow();
            let ox = *s.scroll_x.borrow();
            let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
            let cursor_x = *s.cursor_x.borrow();
            let cursor_y = *s.cursor_y.borrow();
            let current_abs_x = cursor_x - KEY_WIDTH + ox;
            let cursor_tick = (current_abs_x / zx) * tps;
            let vp = widget.build_viewport();
            let start_lane = s.drag_state.borrow().start_lane;
            let current_lane = W::Layout::y_to_lane(&vp, cursor_y, Some(midi), act);
            let start_cursor_tick = s.drag_state.borrow().start_cursor_tick;
            let delta_ticks = cursor_tick - start_cursor_tick;
            let orig_notes = s.drag_state.borrow().orig_notes.clone();

            for (&idx, orig) in &orig_notes {
                if idx >= midi.tracks[act].notes.len() {
                    continue;
                }
                let np =
                    W::Layout::pitch_after_lane_delta(orig.pitch, start_lane, current_lane, midi, act);
                let n = &mut midi.tracks[act].notes[idx];
                let dur = orig.end_tick as i64 - orig.start_tick as i64;
                let ns = (orig.start_tick as f64 + delta_ticks).max(0.0) as u64;
                let ns_snapped = snap_tick(ns, midi.ticks_per_beat);
                let ne = ns_snapped as i64 + dur;
                n.start_tick = ns_snapped;
                n.end_tick = ne.max(0) as u64;
                n.pitch = np;
            }
        }
        widget.redraw();
        return;
    }

    let orig = match s.drag_state.borrow().orig_note.clone() {
        Some(o) => o,
        None => return,
    };
    let idx = match s.selected_notes.borrow().iter().next().copied() {
        Some(i) => i,
        None => return,
    };

    if let Some(midi) = &mut *s.data.borrow_mut() {
        let act = *s.active_track.borrow();
        if act >= midi.tracks.len() || idx >= midi.tracks[act].notes.len() {
            return;
        }
        let zx = *s.zoom_x.borrow();
        let ox = *s.scroll_x.borrow();
        let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
        let cursor_x = *s.cursor_x.borrow();
        let cursor_y = *s.cursor_y.borrow();
        let current_abs_x = cursor_x - KEY_WIDTH + ox;
        let cursor_tick = (current_abs_x / zx) * tps;
        let synth_index = midi.tracks[act].synth_index;
        let channel = W::Layout::note_channel();

        if drag_mode == DragMode::ResizeDuration {
            let n = &mut midi.tracks[act].notes[idx];
            let min_dur = (midi.ticks_per_beat as u64 / SNAP_SUBDIVISIONS).max(1);
            let ne_raw = cursor_tick.max(0.0) as u64;
            let ne_snapped = snap_tick(ne_raw, midi.ticks_per_beat);
            n.end_tick = ne_snapped.max(n.start_tick + min_dur);
        } else if drag_mode == DragMode::MoveNote {
            let dur = orig.end_tick as i64 - orig.start_tick as i64;
            let start_cursor_tick = s.drag_state.borrow().start_cursor_tick;
            let delta_ticks = cursor_tick - start_cursor_tick;
            let target_start = orig.start_tick as f64 + delta_ticks;
            let ns = if target_start < 0.0 {
                0
            } else {
                target_start as u64
            };
            let ns_snapped = snap_tick(ns, midi.ticks_per_beat);
            let ne = ns_snapped as i64 + dur;
            let vp = widget.build_viewport();
            let start_lane = s.drag_state.borrow().start_lane;
            let current_lane = W::Layout::y_to_lane(&vp, cursor_y, Some(midi), act);
            let np =
                W::Layout::pitch_after_lane_delta(orig.pitch, start_lane, current_lane, midi, act);

            let active_opt = *s.preview_active_pitch.borrow();
            if let Some(active) = active_opt {
                if active != np {
                    if let Some(cb) = &*s.preview_note_off_callback.borrow() {
                        cb(synth_index, active, channel);
                    }
                    if let Some(cb) = &*s.preview_note_on_callback.borrow() {
                        cb(synth_index, np, orig.velocity, channel);
                    }
                    *s.preview_active_pitch.borrow_mut() = Some(np);
                }
            }

            let n = &mut midi.tracks[act].notes[idx];
            n.start_tick = ns_snapped;
            n.end_tick = ne.max(0) as u64;
            n.pitch = np;
        }
        widget.redraw();
    }
}

fn handle_drag_update<W: RollView>(widget: &W, dx: f64, dy: f64) {
    let s = widget.state();
    {
        let ds = s.drag_state.borrow();
        *s.cursor_x.borrow_mut() = ds.start_x + dx + KEY_WIDTH;
        *s.cursor_y.borrow_mut() = ds.start_y + dy;
    }
    {
        let mut ds = s.drag_state.borrow_mut();
        ds.last_dx = dx;
        ds.last_dy = dy;
    }
    update_drag_position(widget, dx, dy);
}

fn handle_drag_end<W: RollView>(widget: &W, dx: f64, _dy: f64) {
    let s = widget.state();
    let drag_mode = s.drag_state.borrow().mode;
    let is_playhead_drag = s.drag_state.borrow().is_dragging_playhead;
    let has_orig_note = s.drag_state.borrow().orig_note.is_some();
    let channel = W::Layout::note_channel();

    if is_playhead_drag {
        s.drag_state.borrow_mut().is_dragging_playhead = false;
        let sx = s.drag_state.borrow().start_x;
        let zx = *s.zoom_x.borrow();
        let ox = *s.scroll_x.borrow();
        let mut t = (sx + dx + ox) / zx;
        if t < 0.0 {
            t = 0.0;
        }
        if let Some(cb) = &*s.seek_callback.borrow() {
            cb(t);
        }
    } else if drag_mode == DragMode::BoxSelect {
        *s.selection_rect.borrow_mut() = None;
        widget.update_status();
        widget.redraw();
    } else if drag_mode == DragMode::BulkMove {
        s.notify_data_changed();
        widget.redraw();
    } else if has_orig_note {
        let active_opt = *s.preview_active_pitch.borrow();
        if let Some(active) = active_opt {
            if let Some(cb) = &*s.preview_note_off_callback.borrow() {
                cb(widget.active_synth_index(), active, channel);
            }
            *s.preview_active_pitch.borrow_mut() = None;
        }
        s.notify_data_changed();
        widget.redraw();
    }

    {
        let mut ds = s.drag_state.borrow_mut();
        ds.orig_note = None;
        ds.orig_notes.clear();
        ds.base_selection.clear();
        ds.mode = DragMode::None;
    }
    widget.set_roll_cursor(None);
}

fn handle_scroll<W: RollView>(
    widget: &W,
    controller: &gtk::EventControllerScroll,
    dx: f64,
    dy: f64,
) -> glib::Propagation {
    let s = widget.state();
    let state = controller.current_event_state();
    let action = scroll_action(state);

    if action == ScrollAction::Zoom {
        let mut zx = *s.zoom_x.borrow();
        let old_zx = zx;
        let zoom_factor = 1.0 - dy * 0.1;
        zx *= zoom_factor;
        zx = zx.clamp(10.0, 1000.0);
        *s.zoom_x.borrow_mut() = zx;
        let mut sx = *s.scroll_x.borrow();
        sx = sx / old_zx * zx;
        if sx < 0.0 {
            sx = 0.0;
        }
        *s.scroll_x.borrow_mut() = sx;
        widget.redraw();
        return glib::Propagation::Stop;
    }

    let zy = *s.zoom_y.borrow();
    let total_rows = {
        let midi = s.data.borrow();
        W::Layout::lane_count(midi.as_ref(), *s.active_track.borrow())
    };
    let max_scroll_y = total_rows * zy - widget.widget_height() as f64;
    let mut sy = *s.scroll_y.borrow();
    let mut sx = *s.scroll_x.borrow();

    if action == ScrollAction::HorizontalPan {
        sx += (dx + dy) * 50.0;
    } else {
        sy -= dy * 40.0;
        sx += dx * 50.0;
    }
    sy = sy.clamp(0.0, max_scroll_y.max(0.0));

    let is_dragging = {
        let ds = s.drag_state.borrow();
        ds.is_dragging_playhead
            || ds.orig_note.is_some()
            || ds.mode == DragMode::BoxSelect
            || ds.mode == DragMode::BulkMove
    };
    if is_dragging {
        if sx < 0.0 {
            sx = 0.0;
        }
        *s.scroll_x.borrow_mut() = sx;
        *s.scroll_y.borrow_mut() = sy;
        let cursor_x = *s.cursor_x.borrow();
        let cursor_y = *s.cursor_y.borrow();
        let ds = s.drag_state.borrow();
        let new_dx = cursor_x - KEY_WIDTH - ds.start_x;
        let new_dy = cursor_y - ds.start_y;
        drop(ds);
        update_drag_position(widget, new_dx, new_dy);
        widget.redraw();
        return glib::Propagation::Stop;
    }

    *s.scroll_y.borrow_mut() = sy;
    if sx < 0.0 {
        sx = 0.0;
    }
    *s.scroll_x.borrow_mut() = sx;
    widget.redraw();
    glib::Propagation::Stop
}

fn handle_key_press<W: RollView>(widget: &W, keyval: gdk::Key) -> glib::Propagation {
    let s = widget.state();
    let channel = W::Layout::note_channel();

    if (keyval == gdk::Key::l || keyval == gdk::Key::L) && widget.toggle_put_length_quantization() {
        return glib::Propagation::Stop;
    }

    if let Some(action) =
        mode_key_action_from_state(keyval, &s.edit_mode, &s.typing_keyboard_enabled)
    {
        match action {
            ModeKeyAction::EnterSelect => widget.enter_select_mode(),
            ModeKeyAction::EnterKeyboard => widget.enter_typing_keyboard_mode(),
            ModeKeyAction::EnterPut => widget.enter_put_mode(),
            ModeKeyAction::ReturnNormal => widget.enter_normal_mode(),
        }
        widget.focus_roll();
        return glib::Propagation::Stop;
    }

    if *s.typing_keyboard_enabled.borrow() {
        if is_typing_octave_up_key(keyval) {
            let mut offset = s.typing_octave_offset.borrow_mut();
            *offset = (*offset + 1).min(5);
            drop(offset);
            widget.update_status();
            return glib::Propagation::Stop;
        }
        if is_typing_octave_down_key(keyval) {
            let mut offset = s.typing_octave_offset.borrow_mut();
            *offset = (*offset - 1).max(-4);
            drop(offset);
            widget.update_status();
            return glib::Propagation::Stop;
        }

        let octave_offset = *s.typing_octave_offset.borrow();
        if let Some(pitch) = typing_key_to_pitch_with_octave(keyval, octave_offset) {
            if s.typing_pressed_keys.borrow().contains_key(&keyval) {
                return glib::Propagation::Stop;
            }
            let synth_index = widget.active_synth_index();
            s.typing_pressed_keys.borrow_mut().insert(keyval, pitch);
            *s.preview_active_pitch.borrow_mut() = Some(pitch);
            if let Some(cb) = &*s.preview_note_on_callback.borrow() {
                cb(synth_index, pitch, 100, channel);
            }
            widget.redraw();
            return glib::Propagation::Stop;
        }
    }

    let mut changed = false;
    if keyval == gdk::Key::Delete || keyval == gdk::Key::BackSpace {
        let indices: Vec<usize> = s.selected_notes.borrow().iter().copied().collect();
        if !indices.is_empty() {
            if let Some(midi) = &mut *s.data.borrow_mut() {
                let act = *s.active_track.borrow();
                if act < midi.tracks.len() {
                    let mut sorted = indices;
                    sorted.sort_unstable_by(|a, b| b.cmp(a));
                    for idx in sorted {
                        if idx < midi.tracks[act].notes.len() {
                            midi.tracks[act].notes.remove(idx);
                        }
                    }
                    s.selected_notes.borrow_mut().clear();
                    widget.redraw();
                    changed = true;
                }
            }
        }
    }

    if changed {
        s.notify_data_changed();
        widget.update_status();
        return glib::Propagation::Stop;
    }
    glib::Propagation::Proceed
}

fn handle_key_released<W: RollView>(widget: &W, keyval: gdk::Key) {
    let s = widget.state();
    if !*s.typing_keyboard_enabled.borrow() {
        return;
    }
    let released = {
        let mut pressed_keys = s.typing_pressed_keys.borrow_mut();
        remove_released_typing_key(&mut pressed_keys, keyval)
    };
    if let Some((pitch, remaining)) = released {
        let synth_index = widget.active_synth_index();
        if let Some(cb) = &*s.preview_note_off_callback.borrow() {
            cb(synth_index, pitch, W::Layout::note_channel());
        }
        *s.preview_active_pitch.borrow_mut() = remaining;
        widget.redraw();
    }
}

fn handle_motion<W: RollView>(widget: &W, x: f64, y: f64) {
    let s = widget.state();
    *s.cursor_x.borrow_mut() = x;
    *s.cursor_y.borrow_mut() = y;

    {
        let ds = s.drag_state.borrow();
        if ds.is_dragging_playhead
            || ds.orig_note.is_some()
            || ds.mode == DragMode::BoxSelect
            || ds.mode == DragMode::BulkMove
        {
            return;
        }
    }

    let is_select_mode = *s.edit_mode.borrow() == EditMode::Select;
    if x < KEY_WIDTH {
        widget.set_roll_cursor(None);
        return;
    }

    let zx = *s.zoom_x.borrow();
    let scroll_x = *s.scroll_x.borrow();
    let act_track = *s.active_track.borrow();
    let p_time = *s.playhead_time.borrow();
    let abs_x = x - KEY_WIDTH + scroll_x;
    let p_x = p_time * zx;

    if (abs_x - p_x).abs() < PLAYHEAD_HIT_RADIUS || y < TOP_REGION_HEIGHT {
        widget.set_roll_cursor(Some("col-resize"));
        return;
    }

    let target_pitch = get_target_pitch(widget, y, act_track);
    let zy = *s.zoom_y.borrow();
    let mut cursor_name = if is_select_mode {
        Some("crosshair")
    } else {
        None
    };

    if let Some(target_pitch) = target_pitch {
        if let Some(midi) = &*s.data.borrow() {
            if let Some(hit) =
                hit_test_note::<W::Layout>(midi, act_track, zx, zy, abs_x, target_pitch)
            {
                if is_select_mode && s.selected_notes.borrow().contains(&hit.note_index) {
                    cursor_name = Some("grab");
                } else if hit.drag_mode == DragMode::ResizeDuration {
                    cursor_name = Some("col-resize");
                }
            }
        }
    }

    widget.set_roll_cursor(cursor_name);
}
