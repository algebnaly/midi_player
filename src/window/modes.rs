//! Select-mode and typing-keyboard header toggles.

use gtk::ToggleButton;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use crate::piano_roll::types::EditMode;
use crate::roll_stack::RollStack;

pub fn wire_edit_mode_buttons(
    piano_roll: &RollStack,
    select_mode_btn: &ToggleButton,
    typing_kb_btn: &ToggleButton,
    status_bar: &gtk::Label,
) {
    let status_bar_clone = status_bar.clone();
    piano_roll.connect_status(move |msg| {
        status_bar_clone.set_text(&msg);
    });

    let pr_select = piano_roll.clone();
    let syncing_mode_buttons = Rc::new(Cell::new(false));
    let select_sync_guard = syncing_mode_buttons.clone();
    select_mode_btn.connect_toggled(move |btn| {
        if select_sync_guard.get() {
            return;
        }
        if btn.is_active() {
            pr_select.enter_select_mode();
        } else {
            pr_select.enter_normal_mode();
        }
        pr_select.grab_focus();
    });

    let pr_mode_poll = piano_roll.clone();
    let select_btn_poll = select_mode_btn.clone();
    let typing_btn_poll = typing_kb_btn.clone();
    let poll_sync_guard = syncing_mode_buttons.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let is_select = pr_mode_poll.get_edit_mode() == EditMode::Select;
        if select_btn_poll.is_active() != is_select {
            poll_sync_guard.set(true);
            select_btn_poll.set_active(is_select);
            poll_sync_guard.set(false);
        }
        let is_typing = pr_mode_poll.is_typing_keyboard_enabled();
        if typing_btn_poll.is_active() != is_typing {
            poll_sync_guard.set(true);
            typing_btn_poll.set_active(is_typing);
            poll_sync_guard.set(false);
        }
        glib::ControlFlow::Continue
    });

    let pr_typing = piano_roll.clone();
    let typing_sync_guard = syncing_mode_buttons;
    typing_kb_btn.connect_toggled(move |btn| {
        if typing_sync_guard.get() {
            return;
        }
        if btn.is_active() {
            pr_typing.enter_typing_keyboard_mode();
        } else {
            pr_typing.enter_normal_mode();
        }
        if btn.is_active() {
            pr_typing.grab_focus();
        }
    });
}
