//! Typing-keyboard mapping, mode keys, and scroll modifiers.

use super::types::EditMode;
use gtk::gdk;
use gtk4 as gtk;
use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(test)]
fn typing_key_to_pitch(keyval: gdk::Key) -> Option<u8> {
    typing_key_to_pitch_with_octave(keyval, 0)
}

pub fn typing_key_to_pitch_with_octave(keyval: gdk::Key, octave_offset: i8) -> Option<u8> {
    let base_pitch = match keyval {
        gdk::Key::_1 => 61,
        gdk::Key::_2 => 63,
        gdk::Key::_3 => 66,
        gdk::Key::_4 => 68,
        gdk::Key::_5 => 70,
        gdk::Key::q | gdk::Key::Q => 60,
        gdk::Key::w | gdk::Key::W => 62,
        gdk::Key::e | gdk::Key::E => 64,
        gdk::Key::r | gdk::Key::R => 65,
        gdk::Key::t | gdk::Key::T => 67,
        gdk::Key::y | gdk::Key::Y => 69,
        gdk::Key::u | gdk::Key::U => 71,
        gdk::Key::a | gdk::Key::A => 49,
        gdk::Key::s | gdk::Key::S => 51,
        gdk::Key::d | gdk::Key::D => 54,
        gdk::Key::f | gdk::Key::F => 56,
        gdk::Key::g | gdk::Key::G => 58,
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

pub fn is_typing_octave_up_key(keyval: gdk::Key) -> bool {
    matches!(keyval, gdk::Key::Up)
}

pub fn is_typing_octave_down_key(keyval: gdk::Key) -> bool {
    matches!(keyval, gdk::Key::Down)
}

pub fn remove_released_typing_key(
    pressed_keys: &mut HashMap<gdk::Key, u8>,
    keyval: gdk::Key,
) -> Option<(u8, Option<u8>)> {
    let pitch = pressed_keys.remove(&keyval)?;
    let remaining = pressed_keys.values().copied().next();
    Some((pitch, remaining))
}

#[derive(Debug, PartialEq, Eq)]
pub enum ModeKeyAction {
    EnterSelect,
    EnterKeyboard,
    EnterPut,
    ReturnNormal,
}

pub fn mode_key_action(
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
        gdk::Key::p | gdk::Key::P => Some(ModeKeyAction::EnterPut),
        _ => None,
    }
}

pub fn mode_key_action_from_state(
    keyval: gdk::Key,
    edit_mode: &RefCell<EditMode>,
    typing_keyboard_enabled: &RefCell<bool>,
) -> Option<ModeKeyAction> {
    let edit_mode = *edit_mode.borrow();
    let typing_keyboard_enabled = *typing_keyboard_enabled.borrow();
    mode_key_action(keyval, edit_mode, typing_keyboard_enabled)
}

#[derive(Debug, PartialEq, Eq)]
pub enum ScrollAction {
    Pan,
    HorizontalPan,
    Zoom,
}

pub fn scroll_action(state: gdk::ModifierType) -> ScrollAction {
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        ScrollAction::Zoom
    } else if state.contains(gdk::ModifierType::SHIFT_MASK) {
        ScrollAction::HorizontalPan
    } else {
        ScrollAction::Pan
    }
}

pub fn playhead_time_for_click(pointer_x: f64, scroll_x: f64, zoom_x: f64) -> f64 {
    if zoom_x <= 0.0 {
        return 0.0;
    }
    ((pointer_x - super::types::KEY_WIDTH + scroll_x) / zoom_x).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roll::types::KEY_WIDTH;

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
        assert_eq!(
            mode_key_action(gdk::Key::p, EditMode::Draw, false),
            Some(ModeKeyAction::EnterPut)
        );
    }

    #[test]
    fn b_does_not_toggle_select_back_to_draw() {
        assert_eq!(mode_key_action(gdk::Key::b, EditMode::Select, false), None);
    }

    #[test]
    fn put_mode_requires_returning_to_normal_before_switching_modes() {
        assert_eq!(mode_key_action(gdk::Key::b, EditMode::Put, false), None);
        assert_eq!(mode_key_action(gdk::Key::k, EditMode::Put, false), None);
        assert_eq!(mode_key_action(gdk::Key::p, EditMode::Put, false), None);
    }

    #[test]
    fn escape_returns_to_normal_from_any_mode() {
        assert_eq!(
            mode_key_action(gdk::Key::Escape, EditMode::Select, false),
            Some(ModeKeyAction::ReturnNormal)
        );
        assert_eq!(
            mode_key_action(gdk::Key::Escape, EditMode::Put, false),
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

    #[test]
    fn ctrl_click_playhead_position_is_continuous_and_unsnapped() {
        assert_eq!(playhead_time_for_click(KEY_WIDTH + 50.0, 25.0, 150.0), 0.5);
        assert_eq!(
            playhead_time_for_click(KEY_WIDTH + 1.0, 0.0, 150.0),
            1.0 / 150.0
        );
        assert_eq!(playhead_time_for_click(KEY_WIDTH, 0.0, 0.0), 0.0);
    }
}
