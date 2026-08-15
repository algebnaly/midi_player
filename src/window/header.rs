//! Header bar: file, MIDI, tracks, edit modes, BPM, gain, and playback.

use gtk::prelude::*;
use gtk::{ApplicationWindow, Box, Button, DropDown, HeaderBar, StringList, ToggleButton};
use gtk4 as gtk;

use crate::config::AppConfig;

pub struct HeaderWidgets {
    pub open_btn: Button,
    pub save_project_btn: Button,
    pub save_btn: Button,
    pub play_btn: Button,
    pub pause_btn: Button,
    pub rewind_btn: Button,
    pub track_list: StringList,
    pub track_dropdown: DropDown,
    pub midi_input_list: StringList,
    pub midi_input_dropdown: DropDown,
    pub midi_refresh_btn: Button,
    pub bpm_spin: gtk::SpinButton,
    pub gain_scale: gtk::Scale,
    pub plugin_gui_btn: Button,
    pub tracks_panel_btn: ToggleButton,
    pub velocity_panel_btn: ToggleButton,
    pub typing_kb_btn: ToggleButton,
    pub select_mode_btn: ToggleButton,
}

pub fn build_header(window: &ApplicationWindow, config: &AppConfig) -> HeaderWidgets {
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

    HeaderWidgets {
        open_btn,
        save_project_btn,
        save_btn,
        play_btn,
        pause_btn,
        rewind_btn,
        track_list,
        track_dropdown,
        midi_input_list,
        midi_input_dropdown,
        midi_refresh_btn,
        bpm_spin,
        gain_scale,
        plugin_gui_btn,
        tracks_panel_btn,
        velocity_panel_btn,
        typing_kb_btn,
        select_mode_btn,
    }
}
