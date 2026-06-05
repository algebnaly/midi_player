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
use std::cell::RefCell;
use std::collections::HashMap;

// ────────────────────────────────────────────────────────────────────────
// Typing-keyboard key → MIDI pitch mapping
// ────────────────────────────────────────────────────────────────────────

/// Map a GDK keyval to a MIDI pitch when the typing keyboard mode is
/// active.  Returns `None` for unmapped keys.
///
/// Two octave rows, each with white keys and black keys:
///
/// ```text
///   1  2  3  4  5       → C#4 D#4 F#4 G#4 A#4
///   Q  W  E  R  T  Y  U → C4  D4  E4  F4  G4  A4  B4
///   A  S  D  F  G       → C#3 D#3 F#3 G#3 A#3
///   Z  X  C  V  B  N  M → C3  D3  E3  F3  G3  A3  B3
/// ```
fn typing_key_to_pitch(keyval: gdk::Key) -> Option<u8> {
    typing_key_to_pitch_with_octave(keyval, 0)
}

fn typing_key_to_pitch_with_octave(keyval: gdk::Key, octave_offset: i8) -> Option<u8> {
    let base_pitch = match keyval {
        // ── Upper black row → C#4 D#4 F#4 G#4 A#4 ────────────────
        gdk::Key::_1 => 61,
        gdk::Key::_2 => 63,
        gdk::Key::_3 => 66,
        gdk::Key::_4 => 68,
        gdk::Key::_5 => 70,
        // ── Upper white row → C4 D4 E4 F4 G4 A4 B4 ──────────────
        gdk::Key::q | gdk::Key::Q => 60,
        gdk::Key::w | gdk::Key::W => 62,
        gdk::Key::e | gdk::Key::E => 64,
        gdk::Key::r | gdk::Key::R => 65,
        gdk::Key::t | gdk::Key::T => 67,
        gdk::Key::y | gdk::Key::Y => 69,
        gdk::Key::u | gdk::Key::U => 71,
        // ── Lower black row → C#3 D#3 F#3 G#3 A#3 ────────────────
        gdk::Key::a | gdk::Key::A => 49,
        gdk::Key::s | gdk::Key::S => 51,
        gdk::Key::d | gdk::Key::D => 54,
        gdk::Key::f | gdk::Key::F => 56,
        gdk::Key::g | gdk::Key::G => 58,
        // ── Lower white row → C3 D3 E3 F3 G3 A3 B3 ──────────────
        gdk::Key::z | gdk::Key::Z => 48,
        gdk::Key::x | gdk::Key::X => 50,
        gdk::Key::c | gdk::Key::C => 52,
        gdk::Key::v | gdk::Key::V => 53,
        gdk::Key::b | gdk::Key::B => 55,
        gdk::Key::n | gdk::Key::N => 57,
        gdk::Key::m | gdk::Key::M => 59,
        _ => return None,
    };
    let shifted = base_pitch + i16::from(octave_offset) * 12;
    u8::try_from(shifted).ok().filter(|pitch| *pitch <= 127)
}

fn is_typing_octave_up_key(keyval: gdk::Key) -> bool {
    match keyval {
        gdk::Key::Up => true,
        _ => false,
    }
}

fn is_typing_octave_down_key(keyval: gdk::Key) -> bool {
    match keyval {
        gdk::Key::Down => true,
        _ => false,
    }
}

fn remove_released_typing_key(
    pressed_keys: &mut HashMap<gdk::Key, u8>,
    keyval: gdk::Key,
) -> Option<(u8, Option<u8>)> {
    let pitch = pressed_keys.remove(&keyval)?;
    let remaining = pressed_keys.values().copied().next();
    Some((pitch, remaining))
}

#[derive(Debug, PartialEq, Eq)]
enum ModeKeyAction {
    EnterSelect,
    EnterKeyboard,
    ReturnNormal,
}

fn mode_key_action(
    keyval: gdk::Key,
    edit_mode: EditMode,
    typing_keyboard_enabled: bool,
) -> Option<ModeKeyAction> {
    if keyval == gdk::Key::Escape {
        return Some(ModeKeyAction::ReturnNormal);
    }
    if typing_keyboard_enabled || edit_mode != EditMode::Draw {
        return None;
    }
    match keyval {
        gdk::Key::b | gdk::Key::B => Some(ModeKeyAction::EnterSelect),
        gdk::Key::k | gdk::Key::K => Some(ModeKeyAction::EnterKeyboard),
        _ => None,
    }
}

fn mode_key_action_from_state(
    keyval: gdk::Key,
    edit_mode: &RefCell<EditMode>,
    typing_keyboard_enabled: &RefCell<bool>,
) -> Option<ModeKeyAction> {
    let edit_mode = *edit_mode.borrow();
    let typing_keyboard_enabled = *typing_keyboard_enabled.borrow();
    mode_key_action(keyval, edit_mode, typing_keyboard_enabled)
}

#[derive(Debug, PartialEq, Eq)]
enum ScrollAction {
    Pan,
    HorizontalPan,
    Zoom,
}

fn scroll_action(state: gdk::ModifierType) -> ScrollAction {
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        ScrollAction::Zoom
    } else if state.contains(gdk::ModifierType::SHIFT_MASK) {
        ScrollAction::HorizontalPan
    } else {
        ScrollAction::Pan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_upper_white_and_black_key_rows() {
        assert_eq!(typing_key_to_pitch(gdk::Key::q), Some(60));
        assert_eq!(typing_key_to_pitch(gdk::Key::w), Some(62));
        assert_eq!(typing_key_to_pitch(gdk::Key::u), Some(71));

        assert_eq!(typing_key_to_pitch(gdk::Key::_1), Some(61));
        assert_eq!(typing_key_to_pitch(gdk::Key::_2), Some(63));
        assert_eq!(typing_key_to_pitch(gdk::Key::_3), Some(66));
        assert_eq!(typing_key_to_pitch(gdk::Key::_4), Some(68));
        assert_eq!(typing_key_to_pitch(gdk::Key::_5), Some(70));
    }

    #[test]
    fn maps_lower_white_and_black_key_rows() {
        assert_eq!(typing_key_to_pitch(gdk::Key::z), Some(48));
        assert_eq!(typing_key_to_pitch(gdk::Key::x), Some(50));
        assert_eq!(typing_key_to_pitch(gdk::Key::m), Some(59));

        assert_eq!(typing_key_to_pitch(gdk::Key::a), Some(49));
        assert_eq!(typing_key_to_pitch(gdk::Key::s), Some(51));
        assert_eq!(typing_key_to_pitch(gdk::Key::d), Some(54));
        assert_eq!(typing_key_to_pitch(gdk::Key::f), Some(56));
        assert_eq!(typing_key_to_pitch(gdk::Key::g), Some(58));
    }

    #[test]
    fn applies_octave_offset() {
        assert_eq!(typing_key_to_pitch_with_octave(gdk::Key::q, 1), Some(72));
        assert_eq!(typing_key_to_pitch_with_octave(gdk::Key::z, -1), Some(36));
    }

    #[test]
    fn removes_released_key_and_returns_remaining_pitch() {
        let mut pressed = HashMap::from([(gdk::Key::q, 60), (gdk::Key::w, 62)]);

        let released = remove_released_typing_key(&mut pressed, gdk::Key::q);

        assert_eq!(released, Some((60, Some(62))));
        assert_eq!(pressed, HashMap::from([(gdk::Key::w, 62)]));
    }

    #[test]
    fn normal_mode_keys_enter_select_or_keyboard() {
        assert_eq!(
            mode_key_action(gdk::Key::b, EditMode::Draw, false),
            Some(ModeKeyAction::EnterSelect)
        );
        assert_eq!(
            mode_key_action(gdk::Key::k, EditMode::Draw, false),
            Some(ModeKeyAction::EnterKeyboard)
        );
    }

    #[test]
    fn b_does_not_toggle_select_back_to_draw() {
        assert_eq!(mode_key_action(gdk::Key::b, EditMode::Select, false), None);
    }

    #[test]
    fn escape_returns_to_normal_from_any_mode() {
        assert_eq!(
            mode_key_action(gdk::Key::Escape, EditMode::Select, false),
            Some(ModeKeyAction::ReturnNormal)
        );
        assert_eq!(
            mode_key_action(gdk::Key::Escape, EditMode::Draw, true),
            Some(ModeKeyAction::ReturnNormal)
        );
    }

    #[test]
    fn mode_key_action_from_state_drops_refcell_borrows_before_mutation() {
        let edit_mode = std::cell::RefCell::new(EditMode::Draw);
        let typing_keyboard_enabled = std::cell::RefCell::new(false);

        if mode_key_action_from_state(gdk::Key::Escape, &edit_mode, &typing_keyboard_enabled)
            .is_some()
        {
            *typing_keyboard_enabled.borrow_mut() = false;
        }
    }

    #[test]
    fn ctrl_scroll_keeps_zoom_behavior() {
        assert_eq!(
            scroll_action(gdk::ModifierType::CONTROL_MASK),
            ScrollAction::Zoom
        );
    }

    #[test]
    fn shift_scroll_keeps_horizontal_pan_behavior() {
        assert_eq!(
            scroll_action(gdk::ModifierType::SHIFT_MASK),
            ScrollAction::HorizontalPan
        );
    }

    #[test]
    fn arrow_keys_change_keyboard_octave() {
        assert!(is_typing_octave_up_key(gdk::Key::Up));
        assert!(is_typing_octave_down_key(gdk::Key::Down));
    }

    #[test]
    fn shift_and_ctrl_are_not_keyboard_octave_controls() {
        assert!(!is_typing_octave_up_key(gdk::Key::Shift_L));
        assert!(!is_typing_octave_down_key(gdk::Key::Control_L));
        assert_eq!(
            scroll_action(gdk::ModifierType::CONTROL_MASK),
            ScrollAction::Zoom
        );
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
    drag.connect_drag_begin(move |gesture, start_x, start_y| {
        let shift = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);
        handle_drag_begin(&w, start_x, start_y, shift);
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
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
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

/// Right-click: delete the note under the cursor (Draw mode only).
fn handle_right_click(widget: &super::PianoRollWidget, x: f64, y: f64) {
    if x < KEY_WIDTH {
        return;
    }
    let imp = widget.imp();
    if *imp.edit_mode.borrow() == EditMode::Select {
        return;
    }
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
            let mut new_sel = std::collections::HashSet::new();
            for &idx in imp.selected_notes.borrow().iter() {
                if idx == hit.note_index {
                    continue;
                } else if idx > hit.note_index {
                    new_sel.insert(idx - 1);
                } else {
                    new_sel.insert(idx);
                }
            }
            *imp.selected_notes.borrow_mut() = new_sel;
            widget.queue_draw();
            changed = true;
        }
    }
    if changed {
        if let Some(cb) = &*imp.data_changed_callback.borrow() {
            cb();
        }
        widget.update_status();
    }
}

/// Left-button drag begin.
fn handle_drag_begin(
    widget: &super::PianoRollWidget,
    start_x: f64,
    start_y: f64,
    shift_held: bool,
) {
    if start_x < KEY_WIDTH {
        return;
    }
    let imp = widget.imp();
    widget.grab_focus();

    {
        let mut ds = imp.drag_state.borrow_mut();
        ds.is_dragging_playhead = false;
        ds.orig_note = None;
        ds.orig_notes.clear();
        ds.mode = DragMode::None;
        ds.start_x = start_x - KEY_WIDTH;
        ds.start_y = start_y;
    }

    let p_time = *imp.playhead_time.borrow();
    let zx = *imp.zoom_x.borrow();
    let scroll_x = *imp.scroll_x.borrow();
    let sx_adj = start_x - KEY_WIDTH;
    let p_x = p_time * zx - scroll_x;
    let abs_x = sx_adj + scroll_x;
    let edit_mode = *imp.edit_mode.borrow();

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

    let act_track = *imp.active_track.borrow();
    let vp = widget.build_viewport();
    let target_pitch = vp.y_to_pitch(start_y);
    imp.drag_state.borrow_mut().start_pitch = target_pitch;

    match edit_mode {
        EditMode::Select => {
            handle_drag_begin_select(widget, abs_x, target_pitch, act_track, shift_held)
        }
        EditMode::Draw => handle_drag_begin_draw(widget, abs_x, target_pitch, act_track),
    }
}

fn handle_drag_begin_select(
    widget: &super::PianoRollWidget,
    abs_x: f64,
    target_pitch: u8,
    act_track: usize,
    shift_held: bool,
) {
    let imp = widget.imp();
    let zx = *imp.zoom_x.borrow();
    let tps = Viewport::ticks_per_sec(
        imp.data.borrow().as_ref().map_or(480, |m| m.ticks_per_beat),
        imp.data.borrow().as_ref().map_or(120.0, |m| m.get_bpm()),
    );
    imp.drag_state.borrow_mut().start_cursor_tick = (abs_x / zx) * tps;

    if let Some(midi) = &*imp.data.borrow() {
        if let Some(hit) = hit_test_note(midi, act_track, zx, abs_x, target_pitch) {
            if imp.selected_notes.borrow().contains(&hit.note_index) {
                let mut orig_notes = HashMap::new();
                for &idx in imp.selected_notes.borrow().iter() {
                    if idx < midi.tracks[act_track].notes.len() {
                        orig_notes.insert(idx, midi.tracks[act_track].notes[idx].clone());
                    }
                }
                {
                    let mut ds = imp.drag_state.borrow_mut();
                    ds.mode = DragMode::BulkMove;
                    ds.orig_notes = orig_notes;
                }
                widget.set_cursor_from_name(Some("grabbing"));
                return;
            }
        }
    }

    *imp.selection_rect.borrow_mut() = Some(SelectionRect {
        abs_x0: abs_x,
        abs_x1: abs_x,
        pitch_lo: target_pitch,
        pitch_hi: target_pitch,
    });
    imp.drag_state.borrow_mut().mode = DragMode::BoxSelect;
    // Store the base selection for Shift+append
    if shift_held {
        imp.drag_state.borrow_mut().base_selection = imp.selected_notes.borrow().clone();
    } else {
        imp.drag_state.borrow_mut().base_selection.clear();
        imp.selected_notes.borrow_mut().clear();
    }
    widget.set_cursor_from_name(Some("crosshair"));
    widget.queue_draw();
}

fn handle_drag_begin_draw(
    widget: &super::PianoRollWidget,
    abs_x: f64,
    target_pitch: u8,
    act_track: usize,
) {
    let imp = widget.imp();
    let zx = *imp.zoom_x.borrow();

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
        {
            let mut sel = imp.selected_notes.borrow_mut();
            sel.clear();
            sel.insert(hit.note_index);
        }
        {
            let mut ds = imp.drag_state.borrow_mut();
            ds.orig_note = Some(note.clone());
            ds.mode = hit.drag_mode;
            ds.start_cursor_tick = (abs_x / zx) * tps;
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
        let mut synth_index = act_track;
        if let Some(midi) = &mut *imp.data.borrow_mut() {
            if act_track < midi.tracks.len() {
                synth_index = midi.tracks[act_track].synth_index;
                let raw_tick = ((abs_x / zx) * tps) as u64;
                let start_tick = snap_tick_to_beat(raw_tick, midi.ticks_per_beat);
                let note_len =
                    (*imp.default_note_beats.borrow() * midi.ticks_per_beat as f64).round() as u64;
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
                {
                    let mut sel = imp.selected_notes.borrow_mut();
                    sel.clear();
                    sel.insert(new_idx);
                }
                {
                    let mut ds = imp.drag_state.borrow_mut();
                    ds.orig_note = Some(new_note);
                    ds.mode = DragMode::MoveNote;
                    ds.start_cursor_tick = (abs_x / zx) * tps;
                }
                *imp.preview_active_pitch.borrow_mut() = Some(target_pitch);
                widget.queue_draw();
            }
        }
        if let Some(cb) = &*imp.data_changed_callback.borrow() {
            cb();
        }
        if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
            cb(synth_index, target_pitch, 100);
        }
    }
    widget.update_status();
}

/// Shared drag-position update logic, called from both `drag_update` and
/// the scroll handler so dragged items track the cursor even when the
/// viewport is scrolled mid-drag.
pub fn update_drag_position(widget: &super::PianoRollWidget, dx: f64, _dy: f64) {
    let imp = widget.imp();
    let drag_mode = imp.drag_state.borrow().mode;

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

    // ── Box selection drag ─────────────────────────────────────────

    if drag_mode == DragMode::BoxSelect {
        let sx = imp.drag_state.borrow().start_x;
        let sy = imp.drag_state.borrow().start_y;
        let ox = *imp.scroll_x.borrow();
        let vp = widget.build_viewport();

        let cursor_x = *imp.cursor_x.borrow();
        let cursor_y = *imp.cursor_y.borrow();

        let abs_x0 = sx + ox;
        let abs_x1 = cursor_x - KEY_WIDTH + ox;
        let y0 = sy;
        let y1 = cursor_y;

        let pitch0 = vp.y_to_pitch(y0);
        let pitch1 = vp.y_to_pitch(y1);
        let pitch_lo = pitch0.min(pitch1);
        let pitch_hi = pitch0.max(pitch1);

        *imp.selection_rect.borrow_mut() = Some(SelectionRect {
            abs_x0: abs_x0.min(abs_x1),
            abs_x1: abs_x0.max(abs_x1),
            pitch_lo,
            pitch_hi,
        });

        // Find notes inside the selection rectangle
        let zx = *imp.zoom_x.borrow();
        let act = *imp.active_track.borrow();
        let mut new_sel = std::collections::HashSet::new();
        if let Some(midi) = &*imp.data.borrow() {
            let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
            if act < midi.tracks.len() {
                let x_lo = abs_x0.min(abs_x1);
                let x_hi = abs_x0.max(abs_x1);
                for (i, note) in midi.tracks[act].notes.iter().enumerate() {
                    let nx = (note.start_tick as f64 / tps) * zx;
                    let nw = ((note.end_tick - note.start_tick) as f64 / tps) * zx;
                    // Note intersects rect if ranges overlap
                    if nx + nw >= x_lo
                        && nx <= x_hi
                        && note.pitch >= pitch_lo
                        && note.pitch <= pitch_hi
                    {
                        new_sel.insert(i);
                    }
                }
            }
        }
        // Union with base_selection (for Shift+append)
        let base = &imp.drag_state.borrow().base_selection;
        for &idx in base.iter() {
            new_sel.insert(idx);
        }
        *imp.selected_notes.borrow_mut() = new_sel;
        widget.update_status();
        widget.queue_draw();
        return;
    }

    // ── Bulk move ────────────────────────────────────────────────

    if drag_mode == DragMode::BulkMove {
        if let Some(midi) = &mut *imp.data.borrow_mut() {
            let act = *imp.active_track.borrow();
            if act >= midi.tracks.len() {
                return;
            }
            let zx = *imp.zoom_x.borrow();
            let ox = *imp.scroll_x.borrow();
            let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());

            let cursor_x = *imp.cursor_x.borrow();
            let cursor_y = *imp.cursor_y.borrow();

            let current_abs_x = cursor_x - KEY_WIDTH + ox;
            let cursor_tick = (current_abs_x / zx) * tps;

            let start_pitch = imp.drag_state.borrow().start_pitch;
            let vp = widget.build_viewport();
            let current_cursor_pitch = vp.y_to_pitch(cursor_y);
            let dpitch = current_cursor_pitch as i32 - start_pitch as i32;

            let start_cursor_tick = imp.drag_state.borrow().start_cursor_tick;
            let delta_ticks = cursor_tick - start_cursor_tick;

            let orig_notes = imp.drag_state.borrow().orig_notes.clone();

            for (&idx, orig) in &orig_notes {
                if idx >= midi.tracks[act].notes.len() {
                    continue;
                }
                let n = &mut midi.tracks[act].notes[idx];
                let dur = orig.end_tick as i64 - orig.start_tick as i64;
                let ns = (orig.start_tick as f64 + delta_ticks).max(0.0) as u64;
                let ns_snapped = snap_tick(ns, midi.ticks_per_beat);
                let ne = ns_snapped as i64 + dur;
                let np = (orig.pitch as i32 + dpitch).clamp(0, 127) as u8;
                n.start_tick = ns_snapped;
                n.end_tick = ne.max(0) as u64;
                n.pitch = np;
            }
        }
        widget.queue_draw();
        return;
    }

    // ── Single note drag (Draw mode) ───────────────────────────────

    let orig = match imp.drag_state.borrow().orig_note.clone() {
        Some(o) => o,
        None => return,
    };
    // Get the single selected note index
    let idx = match imp.selected_notes.borrow().iter().next().copied() {
        Some(i) => i,
        None => return,
    };

    if let Some(midi) = &mut *imp.data.borrow_mut() {
        let act = *imp.active_track.borrow();
        if act >= midi.tracks.len() || idx >= midi.tracks[act].notes.len() {
            return;
        }
        let zx = *imp.zoom_x.borrow();
        let ox = *imp.scroll_x.borrow();
        let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
        let cursor_x = *imp.cursor_x.borrow();
        let cursor_y = *imp.cursor_y.borrow();
        let current_abs_x = cursor_x - KEY_WIDTH + ox;
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
            let start_cursor_tick = imp.drag_state.borrow().start_cursor_tick;
            let delta_ticks = cursor_tick - start_cursor_tick;
            let target_start = orig.start_tick as f64 + delta_ticks;
            let ns = if target_start < 0.0 {
                0
            } else {
                target_start as u64
            };
            let ns_snapped = snap_tick(ns, midi.ticks_per_beat);
            let ne = ns_snapped as i64 + dur;

            let start_pitch = imp.drag_state.borrow().start_pitch;
            let vp = widget.build_viewport();
            let current_cursor_pitch = vp.y_to_pitch(cursor_y);
            let dpitch = current_cursor_pitch as i32 - start_pitch as i32;
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
    let imp = widget.imp();
    {
        let ds = imp.drag_state.borrow();
        // start_x is relative to KEY_WIDTH, so cursor_x = start_x + dx + KEY_WIDTH
        *imp.cursor_x.borrow_mut() = ds.start_x + dx + KEY_WIDTH;
        *imp.cursor_y.borrow_mut() = ds.start_y + dy;
    }
    {
        let mut ds = imp.drag_state.borrow_mut();
        ds.last_dx = dx;
        ds.last_dy = dy;
    }
    update_drag_position(widget, dx, dy);
}

fn handle_drag_end(widget: &super::PianoRollWidget, dx: f64, _dy: f64) {
    let imp = widget.imp();
    let drag_mode = imp.drag_state.borrow().mode;

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
    } else if drag_mode == DragMode::BoxSelect {
        // Finalize box selection — rect is already computed in update
        *imp.selection_rect.borrow_mut() = None;
        widget.update_status();
        widget.queue_draw();
    } else if drag_mode == DragMode::BulkMove {
        // Finalize bulk move
        if let Some(cb) = &*imp.data_changed_callback.borrow() {
            cb();
        }
        widget.queue_draw();
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
        ds.orig_notes.clear();
        ds.base_selection.clear();
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
    let action = scroll_action(state);

    // ── Ctrl+scroll: zoom ──────────────────────────────────────────

    if action == ScrollAction::Zoom {
        let mut zx = *imp.zoom_x.borrow();
        let old_zx = zx;

        let zoom_factor = 1.0 - dy * 0.1;
        zx *= zoom_factor;
        zx = zx.clamp(10.0, 1000.0);
        *imp.zoom_x.borrow_mut() = zx;

        // Keep the left edge fixed: scale scroll_x by the same ratio.
        let mut sx = *imp.scroll_x.borrow();
        sx = sx / old_zx * zx;
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

    if action == ScrollAction::HorizontalPan {
        sx += (dx + dy) * 50.0;
    } else {
        sy -= dy * 40.0;
        sx += dx * 50.0;
    }

    sy = sy.clamp(0.0, max_scroll_y.max(0.0));

    // If a drag is active, update scroll and re-derive dx/dy from the
    // actual cursor position so the dragged item follows the pointer.
    let is_dragging = {
        let ds = imp.drag_state.borrow();
        ds.is_dragging_playhead
            || ds.orig_note.is_some()
            || ds.mode == DragMode::BoxSelect
            || ds.mode == DragMode::BulkMove
    };
    if is_dragging {
        if sx < 0.0 {
            sx = 0.0;
        }
        *imp.scroll_x.borrow_mut() = sx;
        *imp.scroll_y.borrow_mut() = sy;
        // Derive dx/dy from the known cursor position
        let cursor_x = *imp.cursor_x.borrow();
        let cursor_y = *imp.cursor_y.borrow();
        let ds = imp.drag_state.borrow();
        let new_dx = cursor_x - KEY_WIDTH - ds.start_x;
        let new_dy = cursor_y - ds.start_y;
        drop(ds);
        update_drag_position(widget, new_dx, new_dy);
        widget.queue_draw();
        return glib::Propagation::Stop;
    }

    *imp.scroll_y.borrow_mut() = sy;

    if sx < 0.0 {
        sx = 0.0;
    }
    *imp.scroll_x.borrow_mut() = sx;

    widget.queue_draw();
    glib::Propagation::Stop
}

fn handle_key_press(widget: &super::PianoRollWidget, keyval: gdk::Key) -> glib::Propagation {
    let imp = widget.imp();

    if let Some(action) =
        mode_key_action_from_state(keyval, &imp.edit_mode, &imp.typing_keyboard_enabled)
    {
        match action {
            ModeKeyAction::EnterSelect => widget.enter_select_mode(),
            ModeKeyAction::EnterKeyboard => widget.enter_typing_keyboard_mode(),
            ModeKeyAction::ReturnNormal => widget.enter_normal_mode(),
        }
        widget.grab_focus();
        return glib::Propagation::Stop;
    }

    // ── Typing keyboard mode ──────────────────────────────────────
    if *imp.typing_keyboard_enabled.borrow() {
        if is_typing_octave_up_key(keyval) {
            let mut offset = imp.typing_octave_offset.borrow_mut();
            *offset = (*offset + 1).min(5);
            drop(offset);
            widget.update_status();
            return glib::Propagation::Stop;
        }
        if is_typing_octave_down_key(keyval) {
            let mut offset = imp.typing_octave_offset.borrow_mut();
            *offset = (*offset - 1).max(-4);
            drop(offset);
            widget.update_status();
            return glib::Propagation::Stop;
        }

        let octave_offset = *imp.typing_octave_offset.borrow();
        if let Some(pitch) = typing_key_to_pitch_with_octave(keyval, octave_offset) {
            // Ignore auto-repeat: if this physical key is already held, skip.
            if imp.typing_pressed_keys.borrow().contains_key(&keyval) {
                return glib::Propagation::Stop;
            }
            let synth_index = widget.active_synth_index();
            imp.typing_pressed_keys.borrow_mut().insert(keyval, pitch);
            // Visual highlight on the keyboard strip
            *imp.preview_active_pitch.borrow_mut() = Some(pitch);
            if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
                cb(synth_index, pitch, 100);
            }
            widget.queue_draw();
            return glib::Propagation::Stop;
        }
    }

    // ── Delete selected notes ──────────────────────────────────────
    let mut changed = false;

    if keyval == gdk::Key::Delete || keyval == gdk::Key::BackSpace {
        let indices: Vec<usize> = imp.selected_notes.borrow().iter().copied().collect();
        if !indices.is_empty() {
            if let Some(midi) = &mut *imp.data.borrow_mut() {
                let act = *imp.active_track.borrow();
                if act < midi.tracks.len() {
                    // Remove in reverse order to keep indices valid
                    let mut sorted = indices;
                    sorted.sort_unstable_by(|a, b| b.cmp(a));
                    for idx in sorted {
                        if idx < midi.tracks[act].notes.len() {
                            midi.tracks[act].notes.remove(idx);
                        }
                    }
                    imp.selected_notes.borrow_mut().clear();
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
        widget.update_status();
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
    let released = {
        let mut pressed_keys = imp.typing_pressed_keys.borrow_mut();
        remove_released_typing_key(&mut pressed_keys, keyval)
    };
    if let Some((pitch, remaining)) = released {
        let synth_index = widget.active_synth_index();
        if let Some(cb) = &*imp.preview_note_off_callback.borrow() {
            cb(synth_index, pitch);
        }
        *imp.preview_active_pitch.borrow_mut() = remaining;
        widget.queue_draw();
    }
}

fn handle_motion(widget: &super::PianoRollWidget, x: f64, y: f64) {
    let imp = widget.imp();
    *imp.cursor_x.borrow_mut() = x;
    *imp.cursor_y.borrow_mut() = y;

    // Don't change cursor during an active drag
    {
        let ds = imp.drag_state.borrow();
        if ds.is_dragging_playhead
            || ds.orig_note.is_some()
            || ds.mode == DragMode::BoxSelect
            || ds.mode == DragMode::BulkMove
        {
            return;
        }
    }

    // In Select mode, default cursor is crosshair
    let is_select_mode = *imp.edit_mode.borrow() == EditMode::Select;

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
    let mut cursor_name = if is_select_mode {
        Some("crosshair")
    } else {
        None
    };

    if let Some(midi) = &*imp.data.borrow() {
        if let Some(hit) = hit_test_note(midi, act_track, zx, abs_x, target_pitch) {
            if is_select_mode && imp.selected_notes.borrow().contains(&hit.note_index) {
                cursor_name = Some("grab");
            } else if hit.drag_mode == DragMode::ResizeDuration {
                cursor_name = Some("col-resize");
            }
        }
    }

    widget.set_cursor_from_name(cursor_name);
}
