//! Input event handling and hit-testing for the piano roll.
//!
//! This module owns all GTK controller setup and event handler logic.
//! A unified [`hit_test_note`] replaces the three ad-hoc note-search loops
//! that previously existed in the monolithic file.

use super::types::*;
use super::viewport::Viewport;
use crate::midi::Note;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk4 as gtk;

// ────────────────────────────────────────────────────────────────────────
// Typing-keyboard key → MIDI pitch mapping
// ────────────────────────────────────────────────────────────────────────

/// Map a GDK keyval to a MIDI pitch when the typing keyboard mode is
/// active.  Returns `None` for unmapped keys.
///
/// White keys only, 7 per row, 4 octaves:
///
/// ```text
///   1  2  3  4  5  6  7       → C5 D5 E5 F5 G5 A5 B5
///   Q  W  E  R  T  Y  U       → C4 D4 E4 F4 G4 A4 B4
///   A  S  D  F  G  H  J       → C3 D3 E3 F3 G3 A3 B3
///   Z  X  C  V  B  N  M       → C2 D2 E2 F2 G2 A2 B2
/// ```
fn typing_key_to_pitch(keyval: gdk::Key) -> Option<u8> {
    // White-key semitone offsets within an octave: C=0 D=2 E=4 F=5 G=7 A=9 B=11
    const W: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    match keyval {
        // ── Number row → C5 (base 72) ─────────────────────────────
        gdk::Key::_1 => Some(72 + W[0]),
        gdk::Key::_2 => Some(72 + W[1]),
        gdk::Key::_3 => Some(72 + W[2]),
        gdk::Key::_4 => Some(72 + W[3]),
        gdk::Key::_5 => Some(72 + W[4]),
        gdk::Key::_6 => Some(72 + W[5]),
        gdk::Key::_7 => Some(72 + W[6]),
        // ── QWERTY row → C4 (base 60) ────────────────────────────
        gdk::Key::q | gdk::Key::Q => Some(60 + W[0]),
        gdk::Key::w | gdk::Key::W => Some(60 + W[1]),
        gdk::Key::e | gdk::Key::E => Some(60 + W[2]),
        gdk::Key::r | gdk::Key::R => Some(60 + W[3]),
        gdk::Key::t | gdk::Key::T => Some(60 + W[4]),
        gdk::Key::y | gdk::Key::Y => Some(60 + W[5]),
        gdk::Key::u | gdk::Key::U => Some(60 + W[6]),
        // ── Home row → C3 (base 48) ──────────────────────────────
        gdk::Key::a | gdk::Key::A => Some(48 + W[0]),
        gdk::Key::s | gdk::Key::S => Some(48 + W[1]),
        gdk::Key::d | gdk::Key::D => Some(48 + W[2]),
        gdk::Key::f | gdk::Key::F => Some(48 + W[3]),
        gdk::Key::g | gdk::Key::G => Some(48 + W[4]),
        gdk::Key::h | gdk::Key::H => Some(48 + W[5]),
        gdk::Key::j | gdk::Key::J => Some(48 + W[6]),
        // ── Bottom row → C2 (base 36) ─────────────────────────────
        gdk::Key::z | gdk::Key::Z => Some(36 + W[0]),
        gdk::Key::x | gdk::Key::X => Some(36 + W[1]),
        gdk::Key::c | gdk::Key::C => Some(36 + W[2]),
        gdk::Key::v | gdk::Key::V => Some(36 + W[3]),
        gdk::Key::b | gdk::Key::B => Some(36 + W[4]),
        gdk::Key::n | gdk::Key::N => Some(36 + W[5]),
        gdk::Key::m | gdk::Key::M => Some(36 + W[6]),
        _ => None,
    }
}

// ────────────────────────────────────────────────────────────────────────
// Unified hit-test
// ────────────────────────────────────────────────────────────────────────

/// Test whether `(abs_x, target_pitch)` hits a note in the given track.
///
/// `abs_x` is in *absolute* pixel coordinates (time=0 is 0px, no KEY_WIDTH,
/// no scroll offset).  Returns the first matching note together with the
/// drag mode (Move vs. Resize) determined by how close the click is to the
/// right edge of the note.
pub fn hit_test_note(
    midi: &crate::midi::MidiData,
    track: usize,
    zoom_x: f64,
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
        let nw = ((note.end_tick - note.start_tick) as f64 / tps) * zoom_x;
        if abs_x >= nx && abs_x <= nx + nw && target_pitch == note.pitch {
            let mut edge_threshold = NOTE_EDGE_THRESHOLD;
            if nw < 16.0 {
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

// ────────────────────────────────────────────────────────────────────────
// GTK controller setup
// ────────────────────────────────────────────────────────────────────────

/// Register all GTK event controllers on the widget.  Each controller's
/// closure simply forwards to the corresponding `handle_*` function.
pub fn setup_controllers(widget: &super::PianoRollWidget) {
    // Right-click: delete notes
    let right_click = gtk::GestureClick::new();
    right_click.set_button(3);
    let w = widget.clone();
    right_click.connect_pressed(move |_, _n_press, x, y| {
        handle_right_click(&w, x, y);
    });

    // Left-button drag: playhead, note create/select/move/resize
    let drag = gtk::GestureDrag::new();
    let w = widget.clone();
    drag.connect_drag_begin(move |_, start_x, start_y| {
        handle_drag_begin(&w, start_x, start_y);
    });
    let w = widget.clone();
    drag.connect_drag_update(move |_, dx, dy| {
        handle_drag_update(&w, dx, dy);
    });
    let w = widget.clone();
    drag.connect_drag_end(move |_, dx, dy| {
        handle_drag_end(&w, dx, dy);
    });

    // Scroll: pan & zoom
    let scroll =
        gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let w = widget.clone();
    scroll.connect_scroll(move |controller, dx, dy| handle_scroll(&w, controller, dx, dy));

    // Keyboard: delete selected note / typing keyboard
    let key_ctrl = gtk::EventControllerKey::new();
    let w = widget.clone();
    key_ctrl.connect_key_pressed(move |_, keyval, _, _| handle_key_press(&w, keyval));
    let w = widget.clone();
    key_ctrl.connect_key_released(move |_, keyval, _, _| handle_key_released(&w, keyval));

    // Motion: cursor hints (resize vs. default)
    let motion = gtk::EventControllerMotion::new();
    let w = widget.clone();
    motion.connect_motion(move |_, x, y| {
        handle_motion(&w, x, y);
    });

    widget.add_controller(right_click);
    widget.add_controller(drag);
    widget.add_controller(scroll);
    widget.add_controller(key_ctrl);
    widget.add_controller(motion);
}

// ────────────────────────────────────────────────────────────────────────
// Event handlers
// ────────────────────────────────────────────────────────────────────────

/// Right-click: delete the note under the cursor.
fn handle_right_click(widget: &super::PianoRollWidget, x: f64, y: f64) {
    if x < KEY_WIDTH {
        return;
    }
    let imp = widget.imp();
    widget.grab_focus();

    let zx = *imp.zoom_x.borrow();
    let scroll_x = *imp.scroll_x.borrow();
    let act_track = *imp.active_track.borrow();
    let abs_x = x - KEY_WIDTH + scroll_x;
    let vp = widget.build_viewport();
    let target_pitch = vp.y_to_pitch(y);

    let mut changed = false;
    if let Some(midi) = &mut *imp.data.borrow_mut() {
        if let Some(hit) = hit_test_note(midi, act_track, zx, abs_x, target_pitch) {
            midi.tracks[act_track].notes.remove(hit.note_index);
            *imp.selected_note.borrow_mut() = None;
            widget.queue_draw();
            changed = true;
        }
    }
    if changed {
        if let Some(cb) = &*imp.data_changed_callback.borrow() {
            cb();
        }
    }
}

/// Left-button drag begin: start playhead drag, select existing note, or
/// create a new note.
fn handle_drag_begin(widget: &super::PianoRollWidget, start_x: f64, start_y: f64) {
    if start_x < KEY_WIDTH {
        return;
    }
    let imp = widget.imp();
    widget.grab_focus();

    // Reset drag state
    {
        let mut ds = imp.drag_state.borrow_mut();
        ds.is_dragging_playhead = false;
        ds.orig_note = None;
        ds.mode = DragMode::None;
        ds.start_x = start_x - KEY_WIDTH;
    }

    let p_time = *imp.playhead_time.borrow();
    let zx = *imp.zoom_x.borrow();
    let scroll_x = *imp.scroll_x.borrow();
    let sx_adj = start_x - KEY_WIDTH;
    let p_x = p_time * zx - scroll_x;
    let abs_x = sx_adj + scroll_x;

    // ── Playhead drag ──────────────────────────────────────────────

    if (sx_adj - p_x).abs() < PLAYHEAD_HIT_RADIUS || start_y < TOP_REGION_HEIGHT {
        imp.drag_state.borrow_mut().is_dragging_playhead = true;
        let mut t = abs_x / zx;
        if t < 0.0 {
            t = 0.0;
        }
        *imp.playhead_time.borrow_mut() = t;
        widget.queue_draw();
        return;
    }

    // ── Note selection / creation ──────────────────────────────────

    let act_track = *imp.active_track.borrow();
    let vp = widget.build_viewport();
    let target_pitch = vp.y_to_pitch(start_y);

    // Try to hit an existing note
    let mut found: Option<(HitTestResult, Note)> = None;
    let mut tps = 1.0;
    if let Some(midi) = &*imp.data.borrow() {
        tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
        if let Some(hit) = hit_test_note(midi, act_track, zx, abs_x, target_pitch) {
            let note = midi.tracks[act_track].notes[hit.note_index].clone();
            found = Some((hit, note));
        }
    }

    if let Some((hit, note)) = found {
        // ── Select existing note ───────────────────────────────────
        *imp.selected_note.borrow_mut() = Some(hit.note_index);
        {
            let mut ds = imp.drag_state.borrow_mut();
            ds.orig_note = Some(note.clone());
            ds.mode = hit.drag_mode;
            let click_tick = (abs_x / zx) * tps;
            ds.click_offset_ticks = click_tick - note.start_tick as f64;
        }
        if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
            cb(hit.synth_index, note.pitch, note.velocity);
        }
        *imp.preview_active_pitch.borrow_mut() = Some(note.pitch);
        widget.set_cursor_from_name(if hit.drag_mode == DragMode::ResizeDuration {
            Some("col-resize")
        } else {
            Some("grabbing")
        });
        widget.queue_draw();
    } else {
        // ── Create new note ────────────────────────────────────────
        let mut synth_index = act_track;
        if let Some(midi) = &mut *imp.data.borrow_mut() {
            if act_track < midi.tracks.len() {
                synth_index = midi.tracks[act_track].synth_index;
                let raw_tick = ((abs_x / zx) * tps) as u64;
                let start_tick = snap_tick_to_beat(raw_tick, midi.ticks_per_beat);
                let note_len = (*imp.default_note_beats.borrow()
                    * midi.ticks_per_beat as f64)
                    .round() as u64;
                let end_tick = start_tick + note_len.max(1);
                let new_note = Note {
                    pitch: target_pitch,
                    velocity: 100,
                    start_tick,
                    end_tick,
                    channel: 0,
                };
                midi.tracks[act_track].notes.push(new_note.clone());
                let new_idx = midi.tracks[act_track].notes.len() - 1;
                *imp.selected_note.borrow_mut() = Some(new_idx);
                {
                    let mut ds = imp.drag_state.borrow_mut();
                    ds.orig_note = Some(new_note);
                    ds.mode = DragMode::MoveNote;
                }
                *imp.preview_active_pitch.borrow_mut() = Some(target_pitch);
                widget.queue_draw();
            }
        }
        // Fire data_changed FIRST so hot_swap (→ all_notes_off) runs
        // before the preview note is sent.
        if let Some(cb) = &*imp.data_changed_callback.borrow() {
            cb();
        }
        // Now send preview NoteOn — after hot_swap is done.
        if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
            cb(synth_index, target_pitch, 100);
        }
    }
}

/// Shared drag-position update logic, called from both `drag_update` and
/// the scroll handler so dragged items track the cursor even when the
/// viewport is scrolled mid-drag.
pub fn update_drag_position(widget: &super::PianoRollWidget, dx: f64, dy: f64) {
    let imp = widget.imp();

    // ── Playhead drag ──────────────────────────────────────────────

    if imp.drag_state.borrow().is_dragging_playhead {
        let sx = imp.drag_state.borrow().start_x;
        let zx = *imp.zoom_x.borrow();
        let ox = *imp.scroll_x.borrow();
        let mut t = (sx + dx + ox) / zx;
        if t < 0.0 {
            t = 0.0;
        }
        *imp.playhead_time.borrow_mut() = t;
        widget.queue_draw();
        return;
    }

    // ── Note drag ──────────────────────────────────────────────────

    let orig = match imp.drag_state.borrow().orig_note.clone() {
        Some(o) => o,
        None => return,
    };
    let drag_mode = imp.drag_state.borrow().mode;
    let idx = match *imp.selected_note.borrow() {
        Some(i) => i,
        None => return,
    };

    if let Some(midi) = &mut *imp.data.borrow_mut() {
        let act = *imp.active_track.borrow();
        if act >= midi.tracks.len() || idx >= midi.tracks[act].notes.len() {
            return;
        }
        let zx = *imp.zoom_x.borrow();
        let zy = *imp.zoom_y.borrow();
        let ox = *imp.scroll_x.borrow();
        let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
        let sx = imp.drag_state.borrow().start_x;

        let current_abs_x = sx + dx + ox;
        let cursor_tick = (current_abs_x / zx) * tps;
        let synth_index = midi.tracks[act].synth_index;

        let n = &mut midi.tracks[act].notes[idx];

        if drag_mode == DragMode::ResizeDuration {
            let min_dur = (midi.ticks_per_beat as u64 / SNAP_SUBDIVISIONS).max(1);
            let ne_raw = cursor_tick.max(0.0) as u64;
            let ne_snapped = snap_tick(ne_raw, midi.ticks_per_beat);
            n.end_tick = ne_snapped.max(n.start_tick + min_dur);
        } else if drag_mode == DragMode::MoveNote {
            let dur = orig.end_tick as i64 - orig.start_tick as i64;
            let click_offset = imp.drag_state.borrow().click_offset_ticks;
            let target_start = cursor_tick - click_offset;
            let ns = if target_start < 0.0 {
                0
            } else {
                target_start as u64
            };
            let ns_snapped = snap_tick(ns, midi.ticks_per_beat);
            let ne = ns_snapped as i64 + dur;

            let dpitch = -(dy / zy).round() as i32;
            let np = (orig.pitch as i32 + dpitch).clamp(0, 127) as u8;

            // Preview pitch change
            let active_opt = *imp.preview_active_pitch.borrow();
            if let Some(active) = active_opt {
                if active != np {
                    if let Some(cb) = &*imp.preview_note_off_callback.borrow() {
                        cb(synth_index, active);
                    }
                    if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
                        cb(synth_index, np, orig.velocity);
                    }
                    *imp.preview_active_pitch.borrow_mut() = Some(np);
                }
            }

            n.start_tick = ns_snapped;
            n.end_tick = ne.max(0) as u64;
            n.pitch = np;
        }
        widget.queue_draw();
    }
}

fn handle_drag_update(widget: &super::PianoRollWidget, dx: f64, dy: f64) {
    {
        let mut ds = widget.imp().drag_state.borrow_mut();
        ds.last_dx = dx;
        ds.last_dy = dy;
    }
    update_drag_position(widget, dx, dy);
}

fn handle_drag_end(widget: &super::PianoRollWidget, dx: f64, _dy: f64) {
    let imp = widget.imp();

    let is_playhead_drag = imp.drag_state.borrow().is_dragging_playhead;
    let has_orig_note = imp.drag_state.borrow().orig_note.is_some();

    if is_playhead_drag {
        imp.drag_state.borrow_mut().is_dragging_playhead = false;
        let sx = imp.drag_state.borrow().start_x;
        let zx = *imp.zoom_x.borrow();
        let ox = *imp.scroll_x.borrow();
        let mut t = (sx + dx + ox) / zx;
        if t < 0.0 {
            t = 0.0;
        }
        if let Some(cb) = &*imp.seek_callback.borrow() {
            cb(t);
        }
    } else if has_orig_note {
        let active_opt = *imp.preview_active_pitch.borrow();
        if let Some(active) = active_opt {
            if let Some(cb) = &*imp.preview_note_off_callback.borrow() {
                cb(widget.active_synth_index(), active);
            }
            *imp.preview_active_pitch.borrow_mut() = None;
        }
        if let Some(cb) = &*imp.data_changed_callback.borrow() {
            cb();
        }
        widget.queue_draw();
    }

    // Clear drag state
    {
        let mut ds = imp.drag_state.borrow_mut();
        ds.orig_note = None;
        ds.mode = DragMode::None;
    }
    // Reset cursor to default when drag ends
    widget.set_cursor_from_name(None);
}

fn handle_scroll(
    widget: &super::PianoRollWidget,
    controller: &gtk::EventControllerScroll,
    dx: f64,
    dy: f64,
) -> glib::Propagation {
    let imp = widget.imp();
    let state = controller.current_event_state();

    // ── Ctrl+scroll: zoom ──────────────────────────────────────────

    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        let mut zx = *imp.zoom_x.borrow();
        let old_zx = zx;

        let zoom_factor = 1.0 - dy * 0.1;
        zx *= zoom_factor;
        zx = zx.clamp(10.0, 1000.0);
        *imp.zoom_x.borrow_mut() = zx;

        let center_x_pixels = widget.width() as f64 / 2.0;
        let mut sx = *imp.scroll_x.borrow();
        let center_time = (sx + center_x_pixels) / old_zx;
        sx = center_time * zx - center_x_pixels;
        if sx < 0.0 {
            sx = 0.0;
        }
        *imp.scroll_x.borrow_mut() = sx;

        widget.queue_draw();
        return glib::Propagation::Stop;
    }

    // ── Normal scroll: pan ─────────────────────────────────────────

    let zy = *imp.zoom_y.borrow();
    let max_scroll_y = 128.0 * zy - widget.height() as f64;

    let mut sy = *imp.scroll_y.borrow();
    let mut sx = *imp.scroll_x.borrow();

    if state.contains(gdk::ModifierType::SHIFT_MASK) {
        sx += (dx + dy) * 50.0;
    } else {
        sy -= dy * 40.0;
        sx += dx * 50.0;
    }

    sy = sy.clamp(0.0, max_scroll_y.max(0.0));
    *imp.scroll_y.borrow_mut() = sy;

    // If a drag is active, re-run position update with stored dx/dy
    // so the dragged item tracks the cursor after scroll.
    let is_dragging = {
        let ds = imp.drag_state.borrow();
        ds.is_dragging_playhead || ds.orig_note.is_some()
    };
    if is_dragging {
        let (last_dx, last_dy) = {
            let ds = imp.drag_state.borrow();
            (ds.last_dx, ds.last_dy)
        };
        // Must update scroll_x BEFORE calling update_drag_position
        if sx < 0.0 {
            sx = 0.0;
        }
        *imp.scroll_x.borrow_mut() = sx;
        update_drag_position(widget, last_dx, last_dy);
        widget.queue_draw();
        return glib::Propagation::Stop;
    }

    if sx < 0.0 {
        sx = 0.0;
    }
    *imp.scroll_x.borrow_mut() = sx;

    widget.queue_draw();
    glib::Propagation::Stop
}

fn handle_key_press(widget: &super::PianoRollWidget, keyval: gdk::Key) -> glib::Propagation {
    let imp = widget.imp();

    // ── Typing keyboard mode ──────────────────────────────────────
    if *imp.typing_keyboard_enabled.borrow() {
        if let Some(pitch) = typing_key_to_pitch(keyval) {
            // Ignore auto-repeat: if pitch is already held, skip.
            if imp.typing_pressed_pitches.borrow().contains(&pitch) {
                return glib::Propagation::Stop;
            }
            let synth_index = widget.active_synth_index();
            imp.typing_pressed_pitches.borrow_mut().insert(pitch);
            // Visual highlight on the keyboard strip
            *imp.preview_active_pitch.borrow_mut() = Some(pitch);
            if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
                cb(synth_index, pitch, 100);
            }
            widget.queue_draw();
            return glib::Propagation::Stop;
        }
    }

    // ── Delete note ────────────────────────────────────────────────
    let mut changed = false;

    if keyval == gdk::Key::Delete || keyval == gdk::Key::BackSpace {
        if let Some(idx) = *imp.selected_note.borrow() {
            if let Some(midi) = &mut *imp.data.borrow_mut() {
                let act = *imp.active_track.borrow();
                if act < midi.tracks.len() {
                    midi.tracks[act].notes.remove(idx);
                    *imp.selected_note.borrow_mut() = None;
                    widget.queue_draw();
                    changed = true;
                }
            }
        }
    }

    if changed {
        if let Some(cb) = &*imp.data_changed_callback.borrow() {
            cb();
        }
        return glib::Propagation::Stop;
    }
    glib::Propagation::Proceed
}

/// Handle key release for the typing keyboard mode.
fn handle_key_released(widget: &super::PianoRollWidget, keyval: gdk::Key) {
    let imp = widget.imp();
    if !*imp.typing_keyboard_enabled.borrow() {
        return;
    }
    if let Some(pitch) = typing_key_to_pitch(keyval) {
        if imp.typing_pressed_pitches.borrow_mut().remove(&pitch) {
            let synth_index = widget.active_synth_index();
            if let Some(cb) = &*imp.preview_note_off_callback.borrow() {
                cb(synth_index, pitch);
            }
            // Update visual: show the last remaining held pitch, or None.
            let remaining: Option<u8> = imp.typing_pressed_pitches.borrow().iter().copied().next();
            *imp.preview_active_pitch.borrow_mut() = remaining;
            widget.queue_draw();
        }
    }
}

fn handle_motion(widget: &super::PianoRollWidget, x: f64, y: f64) {
    let imp = widget.imp();

    // Don't change cursor during an active drag
    {
        let ds = imp.drag_state.borrow();
        if ds.is_dragging_playhead || ds.orig_note.is_some() {
            return;
        }
    }

    if x < KEY_WIDTH {
        widget.set_cursor_from_name(None);
        return;
    }

    let zx = *imp.zoom_x.borrow();
    let scroll_x = *imp.scroll_x.borrow();
    let act_track = *imp.active_track.borrow();
    let p_time = *imp.playhead_time.borrow();

    let abs_x = x - KEY_WIDTH + scroll_x;
    let p_x = p_time * zx;

    if (abs_x - p_x).abs() < PLAYHEAD_HIT_RADIUS || y < TOP_REGION_HEIGHT {
        widget.set_cursor_from_name(Some("col-resize"));
        return;
    }

    let vp = widget.build_viewport();
    let target_pitch = vp.y_to_pitch(y);
    let mut cursor_name = None;

    if let Some(midi) = &*imp.data.borrow() {
        if let Some(hit) = hit_test_note(midi, act_track, zx, abs_x, target_pitch) {
            if hit.drag_mode == DragMode::ResizeDuration {
                cursor_name = Some("col-resize");
            }
        }
    }

    widget.set_cursor_from_name(cursor_name);
}
