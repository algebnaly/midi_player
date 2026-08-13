//! GTK4 application window construction and event wiring.
//!
//! [`build_ui`] is the `connect_activate` callback registered in `main.rs`.
//! It assembles the header bar (open / save / play / pause / rewind / BPM /
//! track selector), the [`PianoRollWidget`], and wires up all user
//! interactions to the [`Player`] backend.

use gtk::prelude::*;
use gtk::{ApplicationWindow, Box, Button, DropDown, HeaderBar, Label, StringList, ToggleButton};
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::app_cache::{CachedMidiInput, clear_midi_input, load_midi_input, save_midi_input};
use crate::config::AppConfig;
use crate::midi::{MidiData, TrackId, TrackMode};
use crate::midi_input::{MidiInputManager, MidiInputPortInfo};
use crate::roll_stack::RollStack;
use crate::piano_roll::types::EditMode;
use crate::player::Player;
use crate::project::{PROJECT_EXTENSION, ProjectFile};
use crate::velocity_curve::{VelocityCurve, VelocityPoint, default_velocity_points};

fn cached_midi_port_position(ports: &[MidiInputPortInfo], cached: &CachedMidiInput) -> Option<u32> {
    ports
        .iter()
        .position(|port| port.id == cached.port_id)
        .or_else(|| ports.iter().position(|port| port.name == cached.port_name))
        .map(|index| index as u32 + 1)
}

fn midi_input_target(midi: &MidiData, active_index: usize) -> Option<(TrackId, usize)> {
    midi.tracks
        .iter()
        .find(|track| track.input.armed)
        .or_else(|| midi.tracks.get(active_index))
        .map(|track| (track.id, track.synth_index))
}

fn rebuild_track_widgets(
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
            
            // Background
            cr.set_source_rgba(0.15, 0.15, 0.15, 1.0);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
            
            if notes.is_empty() {
                return;
            }
            
            let max_tick = notes.iter().map(|n| n.end_tick).max().unwrap_or(1) as f64;
            let min_pitch = notes.iter().map(|n| n.pitch).min().unwrap_or(0) as f64;
            let max_pitch = notes.iter().map(|n| n.pitch).max().unwrap_or(127) as f64;
            let pitch_range = (max_pitch - min_pitch).max(24.0); // Show at least 2 octaves range
            let pitch_padding = pitch_range * 0.1;
            let range_min = min_pitch - pitch_padding;
            let range_range = pitch_range + 2.0 * pitch_padding;
            
            cr.set_source_rgba(0.2, 0.6, 1.0, 0.8);
            for note in &notes {
                let x = (note.start_tick as f64 / max_tick) * w;
                let note_w = (((note.end_tick - note.start_tick) as f64 / max_tick) * w).max(1.0);
                
                // For drums, just draw diamonds or fixed size rectangles
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

#[allow(clippy::too_many_arguments)]
fn install_track_data(
    piano_roll: &RollStack,
    model: &StringList,
    list_box: &gtk::ListBox,
    dropdown: &DropDown,
    name_entry: &gtk::Entry,
    mute_button: &ToggleButton,
    solo_button: &ToggleButton,
    arm_button: &ToggleButton,
    syncing: &Cell<bool>,
    midi: MidiData,
    selected_track: TrackId,
    notify: bool,
) {
    let selected_index = midi.track_index(selected_track).unwrap_or(0);
    let selected_track = midi.tracks[selected_index].id;
    let selected_name = midi.tracks[selected_index].name.clone();

    syncing.set(true);
    rebuild_track_widgets(&midi, model, list_box, selected_track);
    piano_roll.set_data(midi);
    piano_roll.set_active_track(selected_index);
    dropdown.set_selected(selected_index as u32);
    name_entry.set_text(&selected_name);
    let selected = &piano_roll.get_data_clone().unwrap().tracks[selected_index];
    mute_button.set_active(selected.mixer.mute);
    solo_button.set_active(selected.mixer.solo);
    arm_button.set_active(selected.input.armed);
    syncing.set(false);

    if notify {
        piano_roll.notify_data_changed();
    }
}

pub fn build_ui(app: &gtk::Application, initial_file: Option<String>) {
    // Load user configuration
    let config = AppConfig::load();
    let soundbank_manager = Rc::new(crate::soundbank::SoundbankManager::scan(&config.soundbank_dirs));
    let velocity_curve = VelocityCurve::default();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("MIDI Player")
        .default_width(1024)
        .default_height(768)
        .build();

    let header_bar = HeaderBar::new();
    window.set_titlebar(Some(&header_bar));

    let open_btn = Button::with_label("Open");
    let save_project_btn = Button::with_label("Save Project");
    let save_btn = Button::with_label("Export");
    let play_btn = Button::with_label("Play");
    let pause_btn = Button::with_label("Pause");
    let rewind_btn = Button::with_label("Start Over");

    let track_list = StringList::new(&[]);
    let track_dropdown = DropDown::new(Some(track_list.clone()), gtk::Expression::NONE);
    let midi_input_list = StringList::new(&["No MIDI Input"]);
    let midi_input_dropdown = DropDown::new(Some(midi_input_list.clone()), gtk::Expression::NONE);
    midi_input_dropdown.set_tooltip_text(Some("Physical MIDI keyboard input"));
    let midi_refresh_btn = Button::with_label("↻ MIDI");
    midi_refresh_btn.set_tooltip_text(Some("Refresh MIDI input devices"));

    let bpm_adj = gtk::Adjustment::new(config.default_bpm, 20.0, 999.0, 1.0, 10.0, 0.0);
    let bpm_spin = gtk::SpinButton::new(Some(&bpm_adj), 1.0, 0);
    bpm_spin.set_tooltip_text(Some("BPM"));
    let bpm_box = Box::new(gtk::Orientation::Horizontal, 5);
    bpm_box.append(&gtk::Label::new(Some("BPM: ")));
    bpm_box.append(&bpm_spin);

    let initial_gain = config.global_gain.clamp(0.0, 2.0);
    let gain_adj = gtk::Adjustment::new(initial_gain, 0.0, 2.0, 0.01, 0.1, 0.0);
    let gain_scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&gain_adj));
    gain_scale.set_digits(2);
    gain_scale.set_draw_value(true);
    gain_scale.set_value_pos(gtk::PositionType::Right);
    gain_scale.set_width_request(140);
    gain_scale.set_tooltip_text(Some("Global output gain"));
    let gain_box = Box::new(gtk::Orientation::Horizontal, 5);
    gain_box.append(&gtk::Label::new(Some("Gain: ")));
    gain_box.append(&gain_scale);

    let plugin_gui_btn = Button::with_label("Plugin GUI");
    let tracks_panel_btn = ToggleButton::with_label("Tracks");
    tracks_panel_btn.set_tooltip_text(Some("Show or hide the floating track editor"));
    let velocity_panel_btn = ToggleButton::with_label("Velocity Curve");
    velocity_panel_btn.set_tooltip_text(Some("Edit physical MIDI keyboard velocity response"));

    let typing_kb_btn = ToggleButton::with_label("⌨ Typing Keyboard");
    typing_kb_btn.set_tooltip_text(Some(
        "Play notes with your keyboard:\n\
         1 2 3 4 5 → C#4 D#4 F#4 G#4 A#4\n\
         Q W E R T Y U → C4 D4 E4 F4 G4 A4 B4\n\
         A S D F G → C#3 D#3 F#3 G#3 A#3\n\
         Z X C V B N M → C3 D3 E3 F3 G3 A3 B3\n\
         K from Normal: enter  |  Esc: return\n\
         ↑: octave up  |  ↓: octave down",
    ));

    let select_mode_btn = ToggleButton::with_label("⬚ Select");
    select_mode_btn.set_tooltip_text(Some(
        "Select mode (B from Normal, Esc to return):\n\
         Draw a box to select notes, then drag to move them.",
    ));

    header_bar.pack_start(&open_btn);
    header_bar.pack_start(&save_project_btn);
    header_bar.pack_start(&save_btn);
    header_bar.pack_start(&midi_input_dropdown);
    header_bar.pack_start(&midi_refresh_btn);
    header_bar.pack_start(&tracks_panel_btn);
    header_bar.pack_start(&velocity_panel_btn);
    header_bar.pack_start(&plugin_gui_btn);
    header_bar.pack_start(&select_mode_btn);
    header_bar.pack_start(&typing_kb_btn);
    header_bar.pack_start(&bpm_box);
    header_bar.pack_start(&gain_box);

    header_bar.pack_end(&rewind_btn);
    header_bar.pack_end(&pause_btn);
    header_bar.pack_end(&play_btn);

    // Keep the existing application UI as the main child of an Overlay.
    // Settings panels and other in-window floating surfaces can later be
    // attached with `root_overlay.add_overlay(...)` without restructuring
    // the window again.
    let root_overlay = gtk::Overlay::new();
    let vbox = Box::new(gtk::Orientation::Vertical, 0);
    root_overlay.set_child(Some(&vbox));
    window.set_child(Some(&root_overlay));

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

    let mute_track_btn = ToggleButton::with_label("M");
    mute_track_btn.set_tooltip_text(Some("Mute selected track"));
    let solo_track_btn = ToggleButton::with_label("S");
    solo_track_btn.set_tooltip_text(Some("Solo selected track"));
    let arm_track_btn = ToggleButton::with_label("R");
    arm_track_btn.set_tooltip_text(Some("Route physical MIDI input to this track"));
    let track_state_row = Box::new(gtk::Orientation::Horizontal, 4);
    track_state_row.append(&mute_track_btn);
    track_state_row.append(&solo_track_btn);
    track_state_row.append(&arm_track_btn);
    track_panel.append(&track_state_row);

    let track_list_box = gtk::ListBox::new();
    track_list_box.set_selection_mode(gtk::SelectionMode::Single);
    track_list_box.add_css_class("boxed-list");
    let track_scroller = gtk::ScrolledWindow::new();
    track_scroller.set_vexpand(true);
    track_scroller.set_child(Some(&track_list_box));
    track_panel.append(&track_scroller);

    let track_name_entry = gtk::Entry::new();
    track_name_entry.set_placeholder_text(Some("Track name"));
    track_panel.append(&track_name_entry);

    let rename_track_btn = Button::with_label("Rename");
    let add_track_btn = Button::with_label("+");
    add_track_btn.set_tooltip_text(Some("Add track"));
    let duplicate_track_btn = Button::with_label("Duplicate");
    let delete_track_btn = Button::with_label("−");
    delete_track_btn.set_tooltip_text(Some("Delete track"));
    let move_track_up_btn = Button::with_label("↑");
    move_track_up_btn.set_tooltip_text(Some("Move track up"));
    let move_track_down_btn = Button::with_label("↓");
    move_track_down_btn.set_tooltip_text(Some("Move track down"));

    let track_edit_row = Box::new(gtk::Orientation::Horizontal, 4);
    track_edit_row.append(&rename_track_btn);
    track_edit_row.append(&duplicate_track_btn);
    let instrument_btn = Button::with_label("Instrument 🎹");
    track_edit_row.append(&instrument_btn);
    track_panel.append(&track_edit_row);
    let track_action_row = Box::new(gtk::Orientation::Horizontal, 4);
    track_action_row.append(&add_track_btn);
    track_action_row.append(&delete_track_btn);
    track_action_row.append(&move_track_up_btn);
    track_action_row.append(&move_track_down_btn);
    track_panel.append(&track_action_row);

    let piano_roll = RollStack::new();
    piano_roll.set_default_note_beats(config.default_note_beats);

    // Status bar at the bottom — full width, fixed height
    let status_bar = Label::new(Some("[Draw]"));
    status_bar.set_xalign(0.0);
    status_bar.set_hexpand(true);
    status_bar.set_vexpand(false);
    status_bar.add_css_class("status-bar");

    vbox.append(&piano_roll.stack);
    vbox.append(&status_bar);
    root_overlay.add_overlay(&track_panel);

    let track_panel_toggle = track_panel.clone();
    tracks_panel_btn.connect_toggled(move |button| {
        track_panel_toggle.set_visible(button.is_active());
    });
    let tracks_panel_close_toggle = tracks_panel_btn.clone();
    close_track_panel_btn.connect_clicked(move |_| {
        tracks_panel_close_toggle.set_active(false);
    });

    // Drag the floating panel by its title bar while keeping it inside the
    // Overlay's visible allocation.
    let panel_drag = gtk::GestureDrag::new();
    let panel_drag_origin = Rc::new(Cell::new(None::<(i32, i32, f64, f64)>));
    let track_panel_drag_begin = track_panel.clone();
    let panel_drag_origin_begin = panel_drag_origin.clone();
    panel_drag.connect_drag_begin(move |gesture, _, _| {
        panel_drag_origin_begin.set(gesture.current_event().and_then(|event| {
            event.position().map(|(pointer_x, pointer_y)| {
                (
                    track_panel_drag_begin.margin_start(),
                    track_panel_drag_begin.margin_top(),
                    pointer_x,
                    pointer_y,
                )
            })
        }));
    });
    let track_panel_drag_update = track_panel.clone();
    let root_overlay_drag_update = root_overlay.clone();
    panel_drag.connect_drag_update(move |gesture, _, _| {
        let Some((origin_x, origin_y, pointer_origin_x, pointer_origin_y)) =
            panel_drag_origin.get()
        else {
            return;
        };
        let Some((pointer_x, pointer_y)) =
            gesture.current_event().and_then(|event| event.position())
        else {
            return;
        };
        let max_x = (root_overlay_drag_update.width() - track_panel_drag_update.width()).max(0);
        let max_y = (root_overlay_drag_update.height() - track_panel_drag_update.height()).max(0);
        let target_x = origin_x + (pointer_x - pointer_origin_x).round() as i32;
        let target_y = origin_y + (pointer_y - pointer_origin_y).round() as i32;
        track_panel_drag_update.set_margin_start(target_x.clamp(0, max_x));
        track_panel_drag_update.set_margin_top(target_y.clamp(0, max_y));
    });
    track_panel_header.add_controller(panel_drag);

    let velocity_panel = Box::new(gtk::Orientation::Vertical, 6);
    velocity_panel.set_size_request(430, 360);
    velocity_panel.set_halign(gtk::Align::Start);
    velocity_panel.set_valign(gtk::Align::Start);
    velocity_panel.set_margin_top(48);
    velocity_panel.set_margin_start(48);
    velocity_panel.add_css_class("floating-panel");
    velocity_panel.set_visible(false);

    let velocity_panel_title = Label::new(Some("MIDI Velocity Curve"));
    velocity_panel_title.set_hexpand(true);
    velocity_panel_title.set_xalign(0.0);
    velocity_panel_title.add_css_class("panel-title");
    let reset_velocity_btn = Button::with_label("Reset");
    reset_velocity_btn.set_tooltip_text(Some("Restore the default linear curve"));
    let close_velocity_panel_btn = Button::with_label("×");
    close_velocity_panel_btn.add_css_class("flat");
    close_velocity_panel_btn.add_css_class("panel-close-button");
    let velocity_panel_header = Box::new(gtk::Orientation::Horizontal, 6);
    velocity_panel_header.append(&velocity_panel_title);
    velocity_panel_header.append(&reset_velocity_btn);
    velocity_panel_header.append(&close_velocity_panel_btn);
    velocity_panel.append(&velocity_panel_header);

    let velocity_hint = Label::new(Some(
        "Drag points to shape the response · Double-click to add a point",
    ));
    velocity_hint.set_xalign(0.0);
    velocity_hint.add_css_class("panel-hint");
    velocity_panel.append(&velocity_hint);

    let velocity_area = gtk::DrawingArea::new();
    velocity_area.set_content_width(400);
    velocity_area.set_content_height(280);
    velocity_area.set_hexpand(true);
    velocity_area.set_vexpand(true);
    velocity_area.set_cursor_from_name(Some("crosshair"));
    velocity_panel.append(&velocity_area);
    root_overlay.add_overlay(&velocity_panel);

    let velocity_points = Rc::new(RefCell::new(default_velocity_points()));
    let points_draw = velocity_points.clone();
    velocity_area.set_draw_func(move |_, context, width, height| {
        let padding = 28.0;
        let graph_width = (width as f64 - padding * 2.0).max(1.0);
        let graph_height = (height as f64 - padding * 2.0).max(1.0);

        context.set_source_rgb(0.09, 0.09, 0.11);
        let _ = context.paint();
        context.set_line_width(1.0);
        context.set_source_rgb(0.25, 0.25, 0.28);
        for step in 0..=4 {
            let amount = step as f64 / 4.0;
            let x = padding + graph_width * amount;
            let y = padding + graph_height * amount;
            context.move_to(x, padding);
            context.line_to(x, padding + graph_height);
            context.move_to(padding, y);
            context.line_to(padding + graph_width, y);
        }
        let _ = context.stroke();

        let points = points_draw.borrow();
        context.set_source_rgb(0.30, 0.72, 1.0);
        context.set_line_width(2.5);
        for (index, point) in points.iter().enumerate() {
            let x = padding + point.input * graph_width;
            let y = padding + (1.0 - point.output) * graph_height;
            if index == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        let _ = context.stroke();

        for point in points.iter() {
            let x = padding + point.input * graph_width;
            let y = padding + (1.0 - point.output) * graph_height;
            context.arc(x, y, 6.0, 0.0, std::f64::consts::TAU);
            context.set_source_rgb(0.93, 0.96, 1.0);
            let _ = context.fill_preserve();
            context.set_source_rgb(0.12, 0.50, 0.82);
            context.set_line_width(2.0);
            let _ = context.stroke();
        }

        context.set_source_rgb(0.75, 0.75, 0.78);
        context.set_font_size(12.0);
        context.move_to(padding, height as f64 - 7.0);
        let _ = context.show_text("Input velocity  0");
        context.move_to(width as f64 - 48.0, height as f64 - 7.0);
        let _ = context.show_text("127");
        context.move_to(4.0, padding + 4.0);
        let _ = context.show_text("127");
        context.move_to(12.0, height as f64 - padding);
        let _ = context.show_text("0");
    });

    let active_velocity_point = Rc::new(Cell::new(None::<usize>));
    let velocity_drag = gtk::GestureDrag::new();
    let points_drag_begin = velocity_points.clone();
    let active_drag_begin = active_velocity_point.clone();
    velocity_drag.connect_drag_begin(move |gesture, x, y| {
        let Some(widget) = gesture.widget() else {
            return;
        };
        let padding = 28.0;
        let graph_width = (widget.width() as f64 - padding * 2.0).max(1.0);
        let graph_height = (widget.height() as f64 - padding * 2.0).max(1.0);
        let selected = points_drag_begin
            .borrow()
            .iter()
            .enumerate()
            .map(|(index, point)| {
                let point_x = padding + point.input * graph_width;
                let point_y = padding + (1.0 - point.output) * graph_height;
                (index, (point_x - x).hypot(point_y - y))
            })
            .filter(|(_, distance)| *distance <= 14.0)
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index);
        active_drag_begin.set(selected);
    });
    let points_drag_update = velocity_points.clone();
    let active_drag_update = active_velocity_point.clone();
    let area_drag_update = velocity_area.clone();
    let curve_drag_update = velocity_curve.clone();
    velocity_drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        let Some(index) = active_drag_update.get() else {
            return;
        };
        let Some((start_x, start_y)) = gesture.start_point() else {
            return;
        };
        let padding = 28.0;
        let graph_width = (area_drag_update.width() as f64 - padding * 2.0).max(1.0);
        let graph_height = (area_drag_update.height() as f64 - padding * 2.0).max(1.0);
        let input = ((start_x + offset_x - padding) / graph_width).clamp(0.0, 1.0);
        let output = (1.0 - (start_y + offset_y - padding) / graph_height).clamp(0.0, 1.0);
        let mut points = points_drag_update.borrow_mut();
        let last = points.len() - 1;
        let min_input = index
            .checked_sub(1)
            .map(|previous| points[previous].input + 0.005)
            .unwrap_or(0.0);
        let max_input = points
            .get(index + 1)
            .map(|next| next.input - 0.005)
            .unwrap_or(1.0);
        points[index].input = if index == 0 {
            0.0
        } else if index == last {
            1.0
        } else {
            input.clamp(min_input, max_input)
        };
        points[index].output = output;
        curve_drag_update.set_points(&points);
        drop(points);
        area_drag_update.queue_draw();
    });
    velocity_area.add_controller(velocity_drag);

    let velocity_click = gtk::GestureClick::new();
    let points_double_click = velocity_points.clone();
    let area_double_click = velocity_area.clone();
    let curve_double_click = velocity_curve.clone();
    velocity_click.connect_pressed(move |_, press_count, x, y| {
        if press_count != 2 {
            return;
        }
        let padding = 28.0;
        let graph_width = (area_double_click.width() as f64 - padding * 2.0).max(1.0);
        let graph_height = (area_double_click.height() as f64 - padding * 2.0).max(1.0);
        let input = ((x - padding) / graph_width).clamp(0.0, 1.0);
        let output = (1.0 - (y - padding) / graph_height).clamp(0.0, 1.0);
        if input <= 0.005 || input >= 0.995 {
            return;
        }
        let mut points = points_double_click.borrow_mut();
        if points
            .iter()
            .any(|point| (point.input - input).abs() < 0.01)
        {
            return;
        }
        points.push(VelocityPoint::new(input, output));
        points.sort_by(|left, right| left.input.total_cmp(&right.input));
        curve_double_click.set_points(&points);
        drop(points);
        area_double_click.queue_draw();
    });
    velocity_area.add_controller(velocity_click);

    let points_reset = velocity_points.clone();
    let area_reset = velocity_area.clone();
    let curve_reset = velocity_curve.clone();
    reset_velocity_btn.connect_clicked(move |_| {
        *points_reset.borrow_mut() = default_velocity_points();
        curve_reset.set_points(&points_reset.borrow());
        area_reset.queue_draw();
    });

    let velocity_panel_toggle = velocity_panel.clone();
    velocity_panel_btn.connect_toggled(move |button| {
        velocity_panel_toggle.set_visible(button.is_active());
    });
    let velocity_panel_close_toggle = velocity_panel_btn.clone();
    close_velocity_panel_btn.connect_clicked(move |_| {
        velocity_panel_close_toggle.set_active(false);
    });

    let velocity_panel_drag = gtk::GestureDrag::new();
    let velocity_panel_drag_origin = Rc::new(Cell::new(None::<(i32, i32, f64, f64)>));
    let velocity_panel_drag_begin = velocity_panel.clone();
    let velocity_panel_origin_begin = velocity_panel_drag_origin.clone();
    velocity_panel_drag.connect_drag_begin(move |gesture, _, _| {
        velocity_panel_origin_begin.set(gesture.current_event().and_then(|event| {
            event.position().map(|(pointer_x, pointer_y)| {
                (
                    velocity_panel_drag_begin.margin_start(),
                    velocity_panel_drag_begin.margin_top(),
                    pointer_x,
                    pointer_y,
                )
            })
        }));
    });
    let velocity_panel_drag_update = velocity_panel.clone();
    let root_velocity_drag_update = root_overlay.clone();
    velocity_panel_drag.connect_drag_update(move |gesture, _, _| {
        let Some((origin_x, origin_y, pointer_origin_x, pointer_origin_y)) =
            velocity_panel_drag_origin.get()
        else {
            return;
        };
        let Some((pointer_x, pointer_y)) =
            gesture.current_event().and_then(|event| event.position())
        else {
            return;
        };
        let max_x = (root_velocity_drag_update.width() - velocity_panel_drag_update.width()).max(0);
        let max_y =
            (root_velocity_drag_update.height() - velocity_panel_drag_update.height()).max(0);
        let target_x = origin_x + (pointer_x - pointer_origin_x).round() as i32;
        let target_y = origin_y + (pointer_y - pointer_origin_y).round() as i32;
        velocity_panel_drag_update.set_margin_start(target_x.clamp(0, max_x));
        velocity_panel_drag_update.set_margin_top(target_y.clamp(0, max_y));
    });
    velocity_panel_header.add_controller(velocity_panel_drag);

    // Apply minimal CSS for the status bar
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_data(
        ".status-bar { font-family: monospace; font-size: 14px; font-weight: bold; \
         padding: 6px 10px; background: #1a1a1a; color: #eee; \
         min-height: 22px; } \
         .floating-panel { padding: 12px; color: #18202a; \
         background: rgba(248, 250, 252, 0.98); border: 1px solid #8795a6; \
         border-radius: 10px; box-shadow: 0 7px 22px rgba(0, 0, 0, 0.42); } \
         .floating-panel label { color: #18202a; } \
         .floating-panel .panel-title { color: #101820; font-size: 16px; font-weight: 700; } \
         .floating-panel .panel-hint { color: #425466; font-size: 13px; } \
         .floating-panel button { color: #17202a; background: #e6ecf3; \
         border: 1px solid #a5b1bf; border-radius: 6px; } \
         .floating-panel button:hover { background: #d5e4f3; border-color: #5685ad; } \
         .floating-panel button:active, .floating-panel button:checked { \
         color: #ffffff; background: #21699b; border-color: #15547f; } \
         .floating-panel .panel-close-button { color: #334155; background: transparent; \
         border-color: transparent; font-size: 18px; font-weight: 700; } \
         .floating-panel .panel-close-button:hover { color: #ffffff; background: #b43c45; } \
         .floating-panel entry { color: #111827; background: #ffffff; \
         border: 1px solid #93a1b2; caret-color: #111827; } \
         .floating-panel list { color: #18202a; background: #ffffff; \
         border: 1px solid #a5b1bf; } \
         .floating-panel row { color: #18202a; background: #ffffff; } \
         .floating-panel row:hover { background: #e4eef7; } \
         .floating-panel row:selected { color: #ffffff; background: #21699b; } \
         .floating-panel row:selected label { color: #ffffff; }",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Wire status callback
    let status_bar_clone = status_bar.clone();
    piano_roll.connect_status(move |msg| {
        status_bar_clone.set_text(&msg);
    });

    // Wire select mode toggle button
    let pr_select = piano_roll.clone();
    let _select_mode_btn_clone = select_mode_btn.clone();
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

    // Also sync the button when edit mode changes from keyboard shortcut
    // (We'll use a timer to poll — simpler than a custom signal)
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

    // Wire typing keyboard toggle
    let pr_typing = piano_roll.clone();
    let typing_sync_guard = syncing_mode_buttons.clone();
    typing_kb_btn.connect_toggled(move |btn| {
        if typing_sync_guard.get() {
            return;
        }
        if btn.is_active() {
            pr_typing.enter_typing_keyboard_mode();
        } else {
            pr_typing.enter_normal_mode();
        }
        // When enabling, grab focus on the piano roll so it receives key events.
        if btn.is_active() {
            pr_typing.grab_focus();
        }
    });

    let player = Rc::new(RefCell::new(
        match Player::new(
            &config.soundfont_path,
            &config.drum_soundfont_path,
            &config.clap_plugin_path,
            &config.sfz_path,
            initial_gain as f32,
        ) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!(
                    "Failed to initialize player: {}. Audio playback disabled.",
                    e
                );
                None
            }
        },
    ));

    // Drum synth index: if a dedicated drum SF2 was loaded, drum tracks use it.
    let drum_synth_idx = Rc::new(Cell::new(0));
    let sfz_synth_idx = Rc::new(Cell::new(0));

    let player_gain = player.clone();
    gain_scale.connect_value_changed(move |scale| {
        if let Some(p) = &*player_gain.borrow() {
            p.set_global_gain(scale.value() as f32);
        }
    });

    // Physical MIDI input. The manager owns the backend connection while its
    // event producer feeds the Player's audio-thread queue.
    let (midi_manager, midi_ui_rx) = if let Some(p) = &*player.borrow() {
        let (manager, ui_rx) = MidiInputManager::new(p.live_midi_sender(), velocity_curve.clone());
        (Some(manager), Some(ui_rx))
    } else {
        (None, None)
    };
    let midi_manager = Rc::new(RefCell::new(midi_manager));

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

    // Keep physical-key highlights on the GTK thread.
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

    let midi_manager_refresh = midi_manager.clone();
    let midi_ports_refresh = midi_ports.clone();
    let midi_list_refresh = midi_input_list.clone();
    let midi_dropdown_refresh = midi_input_dropdown.clone();
    let pr_midi_refresh = piano_roll.clone();
    let status_midi_refresh = status_bar.clone();
    let suppress_midi_refresh = suppress_midi_selection.clone();
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

    let current_midi_path = Rc::new(RefCell::new(None::<String>));
    let is_playing = Rc::new(RefCell::new(false));
    let syncing_tracks = Rc::new(Cell::new(false));

    // Initialize with an empty project
    let synth_names = if let Some(p) = &*player.borrow() {
        p.get_synth_names()
    } else {
        vec!["Track 0".to_string()]
    };
    let mut empty_data = MidiData::new_empty(&synth_names);
    empty_data.set_bpm(config.default_bpm);
    let initial_track = empty_data.tracks[0].id;
    install_track_data(
        &piano_roll,
        &track_list,
        &track_list_box,
        &track_dropdown,
        &track_name_entry,
        &mute_track_btn,
        &solo_track_btn,
        &arm_track_btn,
        &syncing_tracks,
        empty_data,
        initial_track,
        false,
    );

    let track_dropdown_from_list = track_dropdown.clone();
    let track_name_from_list = track_name_entry.clone();
    let pr_track_list = piano_roll.clone();
    let syncing_track_list = syncing_tracks.clone();
    track_list_box.connect_row_selected(move |_, row| {
        if syncing_track_list.get() {
            return;
        }
        let Some(row) = row else {
            return;
        };
        let index = row.index() as usize;
        track_dropdown_from_list.set_selected(index as u32);
        if let Some(midi) = pr_track_list.get_data_clone()
            && let Some(track) = midi.tracks.get(index)
        {
            track_name_from_list.set_text(&track.name);
        }
    });

    let pr_mute_track = piano_roll.clone();
    let syncing_mute_track = syncing_tracks.clone();
    mute_track_btn.connect_toggled(move |button| {
        if syncing_mute_track.get() {
            return;
        }
        let Some(mut midi) = pr_mute_track.get_data_clone() else {
            return;
        };
        let index = pr_mute_track.active_track_index();
        let Some(track) = midi.tracks.get_mut(index) else {
            return;
        };
        track.mixer.mute = button.is_active();
        pr_mute_track.update_data_and_notify(midi);
    });

    let pr_solo_track = piano_roll.clone();
    let syncing_solo_track = syncing_tracks.clone();
    solo_track_btn.connect_toggled(move |button| {
        if syncing_solo_track.get() {
            return;
        }
        let Some(mut midi) = pr_solo_track.get_data_clone() else {
            return;
        };
        let index = pr_solo_track.active_track_index();
        let Some(track) = midi.tracks.get_mut(index) else {
            return;
        };
        track.mixer.solo = button.is_active();
        pr_solo_track.update_data_and_notify(midi);
    });

    let pr_arm_track = piano_roll.clone();
    let syncing_arm_track = syncing_tracks.clone();
    let midi_manager_arm_track = midi_manager.clone();
    arm_track_btn.connect_toggled(move |button| {
        if syncing_arm_track.get() {
            return;
        }
        let Some(mut midi) = pr_arm_track.get_data_clone() else {
            return;
        };
        let active_index = pr_arm_track.active_track_index();
        for track in &mut midi.tracks {
            track.input.armed = false;
        }
        let Some(active_track) = midi.tracks.get_mut(active_index) else {
            return;
        };
        active_track.input.armed = button.is_active();
        let track_id = active_track.id;
        let synth_index = active_track.synth_index;
        pr_arm_track.update_data(midi);
        if let Some(manager) = midi_manager_arm_track.borrow().as_ref() {
            manager.set_target_track(track_id, synth_index);
        }
    });

    let pr_add_track = piano_roll.clone();
    let model_add_track = track_list.clone();
    let list_add_track = track_list_box.clone();
    let dropdown_add_track = track_dropdown.clone();
    let name_add_track = track_name_entry.clone();
    let mute_add_track = mute_track_btn.clone();
    let solo_add_track = solo_track_btn.clone();
    let arm_add_track = arm_track_btn.clone();
    let syncing_add_track = syncing_tracks.clone();
    add_track_btn.connect_clicked(move |_| {
        let Some(mut midi) = pr_add_track.get_data_clone() else {
            return;
        };
        let (synth_index, synth_source) = midi
            .tracks
            .get(pr_add_track.active_track_index())
            .map(|track| (track.synth_index, track.synth_source.clone()))
            .unwrap_or_else(|| (0, crate::midi::SynthSource::default()));
        let name = format!("Track {}", midi.tracks.len() + 1);
        let new_track = midi.add_track(name, synth_index);
        if let Some(track) = midi.tracks.last_mut() {
            track.synth_source = synth_source;
        }
        install_track_data(
            &pr_add_track,
            &model_add_track,
            &list_add_track,
            &dropdown_add_track,
            &name_add_track,
            &mute_add_track,
            &solo_add_track,
            &arm_add_track,
            &syncing_add_track,
            midi,
            new_track,
            true,
        );
    });

    let instrument_manager = soundbank_manager.clone();
    let player_instrument = player.clone();
    let pr_instrument = piano_roll.clone();
    let model_instrument = track_list.clone();
    let list_instrument = track_list_box.clone();
    let dropdown_instrument = track_dropdown.clone();
    let name_instrument = track_name_entry.clone();
    let mute_instrument = mute_track_btn.clone();
    let solo_instrument = solo_track_btn.clone();
    let arm_instrument = arm_track_btn.clone();
    let syncing_instrument = syncing_tracks.clone();
    
    instrument_btn.connect_clicked(move |_| {
        let Some(active_track) = pr_instrument.active_track_id() else {
            return;
        };
        
        let dialog = gtk::Dialog::builder()
            .title("Select Instrument")
            .modal(true)
            .default_width(400)
            .default_height(500)
            .build();
            
        let content_area = dialog.content_area();
        let search_entry = gtk::SearchEntry::new();
        search_entry.set_margin_top(8);
        search_entry.set_margin_bottom(8);
        search_entry.set_margin_start(8);
        search_entry.set_margin_end(8);
        content_area.append(&search_entry);
        
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_vexpand(true);
        let listbox = gtk::ListBox::new();
        listbox.set_selection_mode(gtk::SelectionMode::Single);
        scrolled.set_child(Some(&listbox));
        content_area.append(&scrolled);
        
        for bank in &instrument_manager.banks {
            let row = gtk::ListBoxRow::new();
            let label = gtk::Label::new(Some(&bank.name));
            label.set_halign(gtk::Align::Start);
            label.set_margin_start(8);
            label.set_margin_top(8);
            label.set_margin_bottom(8);
            row.set_child(Some(&label));
            listbox.append(&row);
        }
        
        let listbox_filter = listbox.clone();
        search_entry.connect_search_changed(move |entry| {
            let text = entry.text().to_lowercase();
            listbox_filter.set_filter_func(move |row| {
                if let Some(child) = row.child() {
                    if let Some(label) = child.downcast_ref::<gtk::Label>() {
                        return label.text().to_lowercase().contains(&text);
                    }
                }
                true
            });
        });
        
        let dialog_clone = dialog.clone();
        let p_inst = player_instrument.clone();
        let pr_inst = pr_instrument.clone();
        let manager_clone = instrument_manager.clone();
        
        let model_inst = model_instrument.clone();
        let list_inst = list_instrument.clone();
        let drop_inst = dropdown_instrument.clone();
        let name_inst = name_instrument.clone();
        let mute_inst = mute_instrument.clone();
        let solo_inst = solo_instrument.clone();
        let arm_inst = arm_instrument.clone();
        let sync_inst = syncing_instrument.clone();
        
        listbox.connect_row_activated(move |_, row| {
            if let Some(child) = row.child() {
                if let Some(label) = child.downcast_ref::<gtk::Label>() {
                    let text = label.text().to_string();
                    if let Some(bank) = manager_clone.banks.iter().find(|b| b.name == text) {
                        if let Some(p) = &mut *p_inst.borrow_mut() {
                            match p.add_or_get_synth(&bank.source) {
                                Ok(new_synth_idx) => {
                                    if let Some(mut midi) = pr_inst.get_data_clone() {
                                        if let Some(track) = midi.tracks.iter_mut().find(|t| t.id == active_track) {
                                            track.synth_source = bank.source.clone();
                                            track.synth_index = new_synth_idx;
                                        }
                                        install_track_data(
                                            &pr_inst,
                                            &model_inst,
                                            &list_inst,
                                            &drop_inst,
                                            &name_inst,
                                            &mute_inst,
                                            &solo_inst,
                                            &arm_inst,
                                            &sync_inst,
                                            midi,
                                            active_track,
                                            true,
                                        );
                                    }
                                }
                                Err(e) => eprintln!("Failed to load synth: {}", e),
                            }
                        }
                    }
                }
            }
            dialog_clone.close();
        });
        
        dialog.present();
    });

    let pr_duplicate_track = piano_roll.clone();
    let model_duplicate_track = track_list.clone();
    let list_duplicate_track = track_list_box.clone();
    let dropdown_duplicate_track = track_dropdown.clone();
    let name_duplicate_track = track_name_entry.clone();
    let mute_duplicate_track = mute_track_btn.clone();
    let solo_duplicate_track = solo_track_btn.clone();
    let arm_duplicate_track = arm_track_btn.clone();
    let syncing_duplicate_track = syncing_tracks.clone();
    duplicate_track_btn.connect_clicked(move |_| {
        let Some(mut midi) = pr_duplicate_track.get_data_clone() else {
            return;
        };
        let Some(active_track) = pr_duplicate_track.active_track_id() else {
            return;
        };
        let Some(new_track) = midi.duplicate_track(active_track) else {
            return;
        };
        install_track_data(
            &pr_duplicate_track,
            &model_duplicate_track,
            &list_duplicate_track,
            &dropdown_duplicate_track,
            &name_duplicate_track,
            &mute_duplicate_track,
            &solo_duplicate_track,
            &arm_duplicate_track,
            &syncing_duplicate_track,
            midi,
            new_track,
            true,
        );
    });

    let pr_delete_track = piano_roll.clone();
    let model_delete_track = track_list.clone();
    let list_delete_track = track_list_box.clone();
    let dropdown_delete_track = track_dropdown.clone();
    let name_delete_track = track_name_entry.clone();
    let mute_delete_track = mute_track_btn.clone();
    let solo_delete_track = solo_track_btn.clone();
    let arm_delete_track = arm_track_btn.clone();
    let syncing_delete_track = syncing_tracks.clone();
    let status_delete_track = status_bar.clone();
    delete_track_btn.connect_clicked(move |_| {
        let Some(mut midi) = pr_delete_track.get_data_clone() else {
            return;
        };
        let Some(active_track) = pr_delete_track.active_track_id() else {
            return;
        };
        let old_index = midi.track_index(active_track).unwrap_or(0);
        if !midi.remove_track(active_track) {
            status_delete_track.set_text("[Tracks] At least one track must remain");
            return;
        }
        let next_track = midi.tracks[old_index.min(midi.tracks.len() - 1)].id;
        install_track_data(
            &pr_delete_track,
            &model_delete_track,
            &list_delete_track,
            &dropdown_delete_track,
            &name_delete_track,
            &mute_delete_track,
            &solo_delete_track,
            &arm_delete_track,
            &syncing_delete_track,
            midi,
            next_track,
            true,
        );
    });

    let pr_rename_track = piano_roll.clone();
    let model_rename_track = track_list.clone();
    let list_rename_track = track_list_box.clone();
    let dropdown_rename_track = track_dropdown.clone();
    let name_rename_track = track_name_entry.clone();
    let mute_rename_track = mute_track_btn.clone();
    let solo_rename_track = solo_track_btn.clone();
    let arm_rename_track = arm_track_btn.clone();
    let syncing_rename_track = syncing_tracks.clone();
    rename_track_btn.connect_clicked(move |_| {
        let new_name = name_rename_track.text().trim().to_string();
        if new_name.is_empty() {
            return;
        }
        let Some(mut midi) = pr_rename_track.get_data_clone() else {
            return;
        };
        let Some(active_track) = pr_rename_track.active_track_id() else {
            return;
        };
        let Some(index) = midi.track_index(active_track) else {
            return;
        };
        midi.tracks[index].name = new_name;
        install_track_data(
            &pr_rename_track,
            &model_rename_track,
            &list_rename_track,
            &dropdown_rename_track,
            &name_rename_track,
            &mute_rename_track,
            &solo_rename_track,
            &arm_rename_track,
            &syncing_rename_track,
            midi,
            active_track,
            true,
        );
    });

    for (button, direction) in [(&move_track_up_btn, -1isize), (&move_track_down_btn, 1)] {
        let pr_move_track = piano_roll.clone();
        let model_move_track = track_list.clone();
        let list_move_track = track_list_box.clone();
        let dropdown_move_track = track_dropdown.clone();
        let name_move_track = track_name_entry.clone();
        let mute_move_track = mute_track_btn.clone();
        let solo_move_track = solo_track_btn.clone();
        let arm_move_track = arm_track_btn.clone();
        let syncing_move_track = syncing_tracks.clone();
        button.connect_clicked(move |_| {
            let Some(mut midi) = pr_move_track.get_data_clone() else {
                return;
            };
            let Some(active_track) = pr_move_track.active_track_id() else {
                return;
            };
            let Some(old_index) = midi.track_index(active_track) else {
                return;
            };
            let new_index =
                (old_index as isize + direction).clamp(0, midi.tracks.len() as isize - 1) as usize;
            if !midi.move_track(active_track, new_index) {
                return;
            }
            install_track_data(
                &pr_move_track,
                &model_move_track,
                &list_move_track,
                &dropdown_move_track,
                &name_move_track,
                &mute_move_track,
                &solo_move_track,
                &arm_move_track,
                &syncing_move_track,
                midi,
                active_track,
                true,
            );
        });
    }

    // Open action
    let window_clone = window.clone();
    let current_midi_clone = current_midi_path.clone();
    let piano_roll_clone = piano_roll.clone();
    let track_list_clone = track_list.clone();
    let track_list_box_open = track_list_box.clone();
    let track_dropdown_open = track_dropdown.clone();
    let track_name_open = track_name_entry.clone();
    let mute_track_open = mute_track_btn.clone();
    let solo_track_open = solo_track_btn.clone();
    let arm_track_open = arm_track_btn.clone();
    let syncing_tracks_open = syncing_tracks.clone();
    let bpm_spin_open = bpm_spin.clone();
    let _drum_idx_open = drum_synth_idx.clone();
    let _sfz_idx_open = sfz_synth_idx.clone();
    let def_sf2 = config.soundfont_path.clone();
    let def_drum_sf2 = config.drum_soundfont_path.clone();
    let player_open = player.clone();

    open_btn.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::new();
        let window = window_clone.clone();
        let midi_path = current_midi_clone.clone();
        let pr = piano_roll_clone.clone();
        let tl = track_list_clone.clone();
        let tlb = track_list_box_open.clone();
        let td = track_dropdown_open.clone();
        let track_name = track_name_open.clone();
        let mute_track = mute_track_open.clone();
        let solo_track = solo_track_open.clone();
        let arm_track = arm_track_open.clone();
        let syncing = syncing_tracks_open.clone();
        let bpm_spin_inner = bpm_spin_open.clone();
        let def_sf2 = def_sf2.clone();
        let def_drum_sf2 = def_drum_sf2.clone();
        let p_open = player_open.clone();

        dialog.open(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
            if let Ok(file) = res {
                let path = file.path().unwrap();
                let path_str = path.to_string_lossy().to_string();
                *midi_path.borrow_mut() = Some(path_str.clone());

                let is_project = path
                    .extension()
                    .is_some_and(|extension| extension == PROJECT_EXTENSION);
                let loaded = if is_project {
                    ProjectFile::load(&path).map(|project| project.midi)
                } else {
                    MidiData::load(&path_str)
                };
                match loaded {
                    Ok(mut data) => {
                        if let Some(p) = &mut *p_open.borrow_mut() {
                            for track in &mut data.tracks {
                                let is_drum = matches!(&track.mode, TrackMode::Drum(_));
                                if !is_project {
                                    track.synth_source = if is_drum && !def_drum_sf2.is_empty() {
                                        crate::midi::SynthSource::SoundFont { path: def_drum_sf2.clone() }
                                    } else {
                                        crate::midi::SynthSource::SoundFont { path: def_sf2.clone() }
                                    };
                                }
                                
                                match p.add_or_get_synth(&track.synth_source) {
                                    Ok(idx) => track.synth_index = idx,
                                    Err(e) => eprintln!("Failed to load synth for track: {}", e),
                                }
                            }
                        }
                        let bpm = data.get_bpm();
                        let first_track = data.tracks[0].id;
                        bpm_spin_inner.set_value(bpm);
                        install_track_data(
                            &pr,
                            &tl,
                            &tlb,
                            &td,
                            &track_name,
                            &mute_track,
                            &solo_track,
                            &arm_track,
                            &syncing,
                            data,
                            first_track,
                            true,
                        );
                        pr.set_playhead(0.0);
                    }
                    Err(e) => eprintln!("Failed to load midi: {}", e),
                }
            }
        });
    });

    let pr_bpm = piano_roll.clone();
    let player_bpm = player.clone();
    let is_playing_bpm = is_playing.clone();
    bpm_spin.connect_value_changed(move |spin| {
        let new_bpm = spin.value();
        if let Some(mut midi) = pr_bpm.get_data_clone() {
            if (midi.get_bpm() - new_bpm).abs() < 0.1 {
                return;
            }
            let current_tick = pr_bpm.get_playhead_tick();
            midi.set_bpm(new_bpm);
            pr_bpm.update_data(midi.clone());
            pr_bpm.set_playhead_tick(current_tick);

            if *is_playing_bpm.borrow() {
                if let Some(p) = &*player_bpm.borrow() {
                    let new_time = current_tick / (midi.ticks_per_beat as f64 * (new_bpm / 60.0));
                    if let Err(e) = p.hot_swap(midi.clone(), new_time) {
                        eprintln!("Failed to hot-swap on BPM change: {}", e);
                    }
                }
            }
        }
    });

    let pr_save = piano_roll.clone();
    let window_save = window.clone();
    save_btn.connect_clicked(move |_| {
        if let Some(midi) = pr_save.get_data_clone() {
            let dialog = gtk::FileDialog::new();
            let window = window_save.clone();
            dialog.save(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().to_string();
                        if let Err(e) = midi.export_to_file(&path_str) {
                            eprintln!("Failed to export: {}", e);
                        }
                    }
                }
            });
        }
    });

    let pr_save_project = piano_roll.clone();
    let window_save_project = window.clone();
    save_project_btn.connect_clicked(move |_| {
        let Some(midi) = pr_save_project.get_data_clone() else {
            return;
        };
        let dialog = gtk::FileDialog::new();
        let window = window_save_project.clone();
        dialog.save(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
            if let Ok(file) = res
                && let Some(mut path) = file.path()
            {
                if path
                    .extension()
                    .is_none_or(|extension| extension != PROJECT_EXTENSION)
                {
                    path.set_extension(PROJECT_EXTENSION);
                }
                if let Err(err) = ProjectFile::new(midi).save(&path) {
                    eprintln!("Failed to save project: {err}");
                }
            }
        });
    });

    let player_clone = player.clone();
    let is_playing_clone = is_playing.clone();
    let pr_play = piano_roll.clone();

    play_btn.connect_clicked(move |_| {
        if let Some(midi) = pr_play.get_data_clone() {
            if let Some(p) = &*player_clone.borrow() {
                if p.is_paused() {
                    // Re-sync sequencer with current piano roll data before
                    // resuming so that edits made while paused take effect.
                    let current_time = p.get_time();
                    if let Err(e) = p.hot_swap(midi, current_time) {
                        eprintln!("Failed to hot-swap on resume: {}", e);
                    }
                    p.resume();
                } else if !p.is_playing() {
                    if let Err(e) = p.play(midi) {
                        eprintln!("Failed to play: {}", e);
                    }
                }
                *is_playing_clone.borrow_mut() = true;
            }
        }
    });

    let player_clone_stop = player.clone();
    let is_playing_stop = is_playing.clone();
    pause_btn.connect_clicked(move |_| {
        if let Some(p) = &*player_clone_stop.borrow() {
            p.pause();
            *is_playing_stop.borrow_mut() = false;
        }
    });

    let player_clone_rewind = player.clone();
    let is_playing_rewind = is_playing.clone();
    let pr_rewind = piano_roll.clone();
    rewind_btn.connect_clicked(move |_| {
        if let Some(midi) = pr_rewind.get_data_clone() {
            if let Some(p) = &*player_clone_rewind.borrow() {
                if let Err(e) = p.play(midi) {
                    eprintln!("Failed to play: {}", e);
                } else {
                    *is_playing_rewind.borrow_mut() = true;
                }
            }
        }
    });

    let pr_track = piano_roll.clone();
    let midi_manager_track = midi_manager.clone();
    let track_list_box_select = track_list_box.clone();
    let track_name_select = track_name_entry.clone();
    let syncing_track_select = syncing_tracks.clone();
    let mute_track_select = mute_track_btn.clone();
    let solo_track_select = solo_track_btn.clone();
    let arm_track_select = arm_track_btn.clone();
    track_dropdown.connect_selected_notify(move |dd| {
        let selected = dd.selected();
        if selected != gtk::INVALID_LIST_POSITION {
            pr_track.set_active_track(selected as usize);
            let track_data = pr_track
                .get_data_clone()
                .and_then(|midi| midi.tracks.get(selected as usize).cloned());
            let target_track = pr_track
                .get_data_clone()
                .and_then(|midi| midi_input_target(&midi, selected as usize));
            if let Some(manager) = midi_manager_track.borrow().as_ref() {
                if let Some((track_id, synth_index)) = target_track {
                    manager.set_target_track(track_id, synth_index);
                }
            }
            if let Some(track) = &track_data {
                let was_syncing = syncing_track_select.replace(true);
                mute_track_select.set_active(track.mixer.mute);
                solo_track_select.set_active(track.mixer.solo);
                arm_track_select.set_active(track.input.armed);
                syncing_track_select.set(was_syncing);
            }
            if !syncing_track_select.get() {
                if let Some(row) = track_list_box_select.row_at_index(selected as i32) {
                    track_list_box_select.select_row(Some(&row));
                }
                if let Some(midi) = pr_track.get_data_clone()
                    && let Some(track) = midi.tracks.get(selected as usize)
                {
                    track_name_select.set_text(&track.name);
                }
            }
        }
    });

    if let Some(midi) = piano_roll.get_data_clone()
        && let Some((track_id, synth_index)) =
            midi_input_target(&midi, piano_roll.active_track_index())
        && let Some(manager) = midi_manager.borrow().as_ref()
    {
        manager.set_target_track(track_id, synth_index);
    }

    let key_ctrl = gtk::EventControllerKey::new();
    let player_key = player.clone();
    let is_playing_key = is_playing.clone();
    let pr_key = piano_roll.clone();
    key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
        if (keyval == gtk::gdk::Key::l || keyval == gtk::gdk::Key::L)
            && pr_key.toggle_put_length_quantization()
        {
            pr_key.grab_focus();
            return glib::Propagation::Stop;
        }
        if keyval == gtk::gdk::Key::p || keyval == gtk::gdk::Key::P {
            if pr_key.is_normal_mode() {
                pr_key.enter_put_mode();
                pr_key.grab_focus();
                return glib::Propagation::Stop;
            }
        }
        if keyval == gtk::gdk::Key::space {
            let mut playing = is_playing_key.borrow_mut();
            if let Some(p) = &*player_key.borrow() {
                if p.is_playing() && !p.is_paused() {
                    p.pause();
                    *playing = false;
                } else {
                    if p.is_paused() {
                        // Re-sync sequencer with current piano roll data
                        // before resuming so that edits made while paused
                        // take effect.
                        if let Some(midi) = pr_key.get_data_clone() {
                            let current_time = p.get_time();
                            if let Err(e) = p.hot_swap(midi, current_time) {
                                eprintln!("Failed to hot-swap on resume: {}", e);
                            }
                        }
                        p.resume();
                    } else if let Some(midi) = pr_key.get_data_clone() {
                        if let Err(e) = p.play(midi) {
                            eprintln!("Failed to play: {}", e);
                        }
                    }
                    *playing = true;
                }
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_ctrl);

    // GUI update timer
    let pr_timer = piano_roll.clone();
    let player_timer = player.clone();
    let is_playing_timer = is_playing.clone();

    glib::timeout_add_local(Duration::from_millis(16), move || {
        if *is_playing_timer.borrow() {
            if let Some(p) = &*player_timer.borrow() {
                if p.is_playing() {
                    let Some(track_id) = pr_timer.active_track_id() else {
                        return glib::ControlFlow::Continue;
                    };
                    let (time, active_pitches) = p.playback_snapshot(track_id);
                    pr_timer.set_playhead(time);
                    pr_timer.set_playback_active_pitches(active_pitches);
                } else {
                    *is_playing_timer.borrow_mut() = false;
                    pr_timer.set_playback_active_pitches([]);
                }
            }
        } else {
            pr_timer.set_playback_active_pitches([]);
        }
        glib::ControlFlow::Continue
    });

    // Provide player to piano roll for seek callback
    let player_seek = player.clone();
    piano_roll.connect_seek(move |time| {
        if let Some(p) = &*player_seek.borrow() {
            p.seek(time);
        }
    });

    let player_data_changed = player.clone();
    let pr_data_changed = piano_roll.clone();
    let is_playing_changed = is_playing.clone();
    let midi_manager_data_changed = midi_manager.clone();
    piano_roll.connect_data_changed(move || {
        if let Some(midi) = pr_data_changed.get_data_clone()
            && let Some((track_id, synth_index)) =
                midi_input_target(&midi, pr_data_changed.active_track_index())
            && let Some(manager) = midi_manager_data_changed.borrow().as_ref()
        {
            manager.set_target_track(track_id, synth_index);
        }
        if *is_playing_changed.borrow() {
            if let Some(p) = &*player_data_changed.borrow() {
                if let Some(midi) = pr_data_changed.get_data_clone() {
                    let current_time = p.get_time();
                    if let Err(e) = p.hot_swap(midi, current_time) {
                        eprintln!("Failed to hot-swap: {}", e);
                    }
                }
            }
        }
    });

    let player_preview_on = player.clone();
    piano_roll.connect_preview_note_on(move |synth_index, pitch, vel, channel| {
        if let Some(p) = &*player_preview_on.borrow() {
            p.preview_note_on(synth_index, channel, pitch, vel);
        }
    });

    let player_preview_off = player.clone();
    piano_roll.connect_preview_note_off(move |synth_index, pitch, channel| {
        if let Some(p) = &*player_preview_off.borrow() {
            p.preview_note_off(synth_index, channel, pitch);
        }
    });

    // Plugin GUI button: toggle the CLAP plugin's floating window.
    let player_gui = player.clone();
    let track_dropdown_gui = track_dropdown.clone();
    let piano_roll_gui = piano_roll.clone();
    plugin_gui_btn.connect_clicked(move |_btn| {
        let track = track_dropdown_gui.selected() as usize;
        let synth_index = piano_roll_gui.track_synth_index(track);
        if let Some(p) = &mut *player_gui.borrow_mut() {
            if p.is_plugin_gui_open(synth_index) {
                p.close_plugin_gui(synth_index);
            } else {
                p.open_plugin_gui(synth_index);
            }
        }
    });

    // Gracefully shut down audio on window close to avoid pop/click.
    let player_shutdown = player.clone();
    let midi_manager_shutdown = midi_manager.clone();
    window.connect_close_request(move |_| {
        if let Some(manager) = midi_manager_shutdown.borrow_mut().as_mut() {
            manager.disconnect();
        }
        if let Some(p) = &mut *player_shutdown.borrow_mut() {
            // Close any open plugin GUIs before shutting down audio.
            for i in 0..p.gui_handle_count() {
                p.close_plugin_gui(i);
            }
            p.shutdown();
        }
        *player_shutdown.borrow_mut() = None;
        glib::Propagation::Proceed
    });

    // Poll CLAP plugin callbacks every 16 ms (~60 Hz).
    // This ensures `on_main_thread()` is called when plugins request it
    // (e.g. to sync GUI state changes to the audio thread).
    let player_poll = player.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        if let Some(p) = &mut *player_poll.borrow_mut() {
            p.poll_plugin_callbacks();
        }
        glib::ControlFlow::Continue
    });

    window.present();

    if let Some(path_str) = initial_file {
        let path = std::path::Path::new(&path_str);
        if !path.extension().is_some_and(|ext| ext == PROJECT_EXTENSION) {
            eprintln!("Command line loading only supports project files (.midiproj)");
            return;
        }

        *current_midi_path.borrow_mut() = Some(path_str.clone());
        match ProjectFile::load(path) {
            Ok(project) => {
                let mut data = project.midi;
                if let Some(p) = &mut *player.borrow_mut() {
                    for track in &mut data.tracks {
                        match p.add_or_get_synth(&track.synth_source) {
                            Ok(idx) => track.synth_index = idx,
                            Err(e) => eprintln!("Failed to load synth for track: {}", e),
                        }
                    }
                }
                let bpm = data.get_bpm();
                let first_track = data.tracks.first().map(|t| t.id).unwrap_or_else(|| crate::midi::TrackId(1));
                bpm_spin.set_value(bpm);
                install_track_data(
                    &piano_roll,
                    &track_list,
                    &track_list_box,
                    &track_dropdown,
                    &track_name_entry,
                    &mute_track_btn,
                    &solo_track_btn,
                    &arm_track_btn,
                    &syncing_tracks,
                    data,
                    first_track,
                    true,
                );
                piano_roll.set_playhead(0.0);
            }
            Err(e) => eprintln!("Failed to load initial file {}: {}", path_str, e),
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
