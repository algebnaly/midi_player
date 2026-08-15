//! Floating track editor panel.

use gtk::prelude::*;
use gtk::{Box, Button, Label, ToggleButton};
use gtk4 as gtk;

use super::overlay::make_floating_panel_draggable;

pub struct TrackPanel {
    pub list_box: gtk::ListBox,
    pub name_entry: gtk::Entry,
    pub mute_btn: ToggleButton,
    pub solo_btn: ToggleButton,
    pub arm_btn: ToggleButton,
    pub rename_btn: Button,
    pub add_btn: Button,
    pub duplicate_btn: Button,
    pub delete_btn: Button,
    pub move_up_btn: Button,
    pub move_down_btn: Button,
    pub instrument_btn: Button,
}

pub fn attach_track_panel(overlay: &gtk::Overlay, toggle_btn: &ToggleButton) -> TrackPanel {
    let track_panel = Box::new(gtk::Orientation::Vertical, 6);
    track_panel.set_size_request(260, 560);
    track_panel.set_halign(gtk::Align::Start);
    track_panel.set_valign(gtk::Align::Start);
    track_panel.set_margin_top(12);
    track_panel.set_margin_start(12);
    track_panel.set_margin_bottom(12);
    track_panel.set_margin_end(12);
    track_panel.add_css_class("floating-panel");
    track_panel.set_visible(false);

    let track_panel_title = Label::new(Some("Tracks"));
    track_panel_title.set_hexpand(true);
    track_panel_title.set_xalign(0.0);
    track_panel_title.add_css_class("panel-title");
    let close_track_panel_btn = Button::with_label("×");
    close_track_panel_btn.add_css_class("flat");
    close_track_panel_btn.add_css_class("panel-close-button");
    close_track_panel_btn.set_tooltip_text(Some("Close track editor"));
    let track_panel_header = Box::new(gtk::Orientation::Horizontal, 6);
    track_panel_header.append(&track_panel_title);
    track_panel_header.append(&close_track_panel_btn);
    track_panel.append(&track_panel_header);

    let mute_btn = ToggleButton::with_label("M");
    mute_btn.set_tooltip_text(Some("Mute selected track"));
    let solo_btn = ToggleButton::with_label("S");
    solo_btn.set_tooltip_text(Some("Solo selected track"));
    let arm_btn = ToggleButton::with_label("R");
    arm_btn.set_tooltip_text(Some("Route physical MIDI input to this track"));
    let track_state_row = Box::new(gtk::Orientation::Horizontal, 4);
    track_state_row.append(&mute_btn);
    track_state_row.append(&solo_btn);
    track_state_row.append(&arm_btn);
    track_panel.append(&track_state_row);

    let list_box = gtk::ListBox::new();
    list_box.set_selection_mode(gtk::SelectionMode::Single);
    list_box.add_css_class("boxed-list");
    let track_scroller = gtk::ScrolledWindow::new();
    track_scroller.set_vexpand(true);
    track_scroller.set_child(Some(&list_box));
    track_panel.append(&track_scroller);

    let name_entry = gtk::Entry::new();
    name_entry.set_placeholder_text(Some("Track name"));
    track_panel.append(&name_entry);

    let rename_btn = Button::with_label("Rename");
    let add_btn = Button::with_label("+");
    add_btn.set_tooltip_text(Some("Add track"));
    let duplicate_btn = Button::with_label("Duplicate");
    let delete_btn = Button::with_label("−");
    delete_btn.set_tooltip_text(Some("Delete track"));
    let move_up_btn = Button::with_label("↑");
    move_up_btn.set_tooltip_text(Some("Move track up"));
    let move_down_btn = Button::with_label("↓");
    move_down_btn.set_tooltip_text(Some("Move track down"));

    let track_edit_row = Box::new(gtk::Orientation::Horizontal, 4);
    track_edit_row.append(&rename_btn);
    track_edit_row.append(&duplicate_btn);
    let instrument_btn = Button::with_label("Instrument 🎹");
    track_edit_row.append(&instrument_btn);
    track_panel.append(&track_edit_row);
    let track_action_row = Box::new(gtk::Orientation::Horizontal, 4);
    track_action_row.append(&add_btn);
    track_action_row.append(&delete_btn);
    track_action_row.append(&move_up_btn);
    track_action_row.append(&move_down_btn);
    track_panel.append(&track_action_row);

    overlay.add_overlay(&track_panel);

    let track_panel_toggle = track_panel.clone();
    toggle_btn.connect_toggled(move |button| {
        track_panel_toggle.set_visible(button.is_active());
    });
    let close_toggle = toggle_btn.clone();
    close_track_panel_btn.connect_clicked(move |_| {
        close_toggle.set_active(false);
    });

    make_floating_panel_draggable(&track_panel, &track_panel_header, overlay);

    TrackPanel {
        list_box,
        name_entry,
        mute_btn,
        solo_btn,
        arm_btn,
        rename_btn,
        add_btn,
        duplicate_btn,
        delete_btn,
        move_up_btn,
        move_down_btn,
        instrument_btn,
    }
}
