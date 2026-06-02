//! Custom GTK4 piano-roll widget.
//!
//! [`PianoRollWidget`] is a GObject subclass that renders a grid-based MIDI
//! piano roll with:
//!
//! * A keyboard strip on the left edge.
//! * A "typing keyboard" mode mapping QWERTY keys to piano notes.
//! * Horizontal beat / bar grid lines.
//! * Editable note rectangles that can be placed, moved, and resized.
//! * A draggable playhead.
//! * Live note preview callbacks for auditioning.
//!
//! The widget communicates changes back to the main window through closures
//! registered via `connect_*` methods.

mod input;
mod keyboard;
mod renderer;
mod types;
mod viewport;

use crate::midi::MidiData;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene};
use gtk4 as gtk;
use std::cell::RefCell;
use std::collections::HashSet;

use types::{DragState, KEY_WIDTH};
use viewport::Viewport;

// ────────────────────────────────────────────────────────────────────────
// GObject private implementation
// ────────────────────────────────────────────────────────────────────────

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PianoRollWidget {
        // ── Data ──────────────────────────────────────────────────
        pub data: RefCell<Option<MidiData>>,
        pub active_track: RefCell<usize>,
        pub selected_note: RefCell<Option<usize>>,

        // ── Viewport ──────────────────────────────────────────────
        pub playhead_time: RefCell<f64>,
        pub zoom_x: RefCell<f64>,
        pub zoom_y: RefCell<f64>,
        pub scroll_x: RefCell<f64>,
        pub scroll_y: RefCell<f64>,

        // ── Interaction ───────────────────────────────────────────
        pub drag_state: RefCell<DragState>,
        pub preview_active_pitch: RefCell<Option<u8>>,

        // ── Typing keyboard ───────────────────────────────────────
        /// When true, QWERTY keys trigger note-on/off.
        pub typing_keyboard_enabled: RefCell<bool>,
        /// Currently held keys (by MIDI pitch) so we can send note-off.
        pub typing_pressed_pitches: RefCell<HashSet<u8>>,

        // ── Configuration ─────────────────────────────────────────
        /// Default note duration in beats (from config).
        pub default_note_beats: RefCell<f64>,

        // ── Callbacks ─────────────────────────────────────────────
        #[allow(clippy::type_complexity)]
        pub seek_callback: RefCell<Option<Box<dyn Fn(f64)>>>,
        #[allow(clippy::type_complexity)]
        pub data_changed_callback: RefCell<Option<Box<dyn Fn()>>>,
        #[allow(clippy::type_complexity)]
        pub preview_note_on_callback: RefCell<Option<Box<dyn Fn(usize, u8, u8)>>>,
        #[allow(clippy::type_complexity)]
        pub preview_note_off_callback: RefCell<Option<Box<dyn Fn(usize, u8)>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PianoRollWidget {
        const NAME: &'static str = "PianoRollWidget";
        type Type = super::PianoRollWidget;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for PianoRollWidget {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.set_size_request(800, 600);
            obj.set_focusable(true);

            *self.zoom_x.borrow_mut() = 150.0;
            *self.zoom_y.borrow_mut() = 20.0;
            *self.default_note_beats.borrow_mut() = 1.0;
            // Default scroll to middle C area (pitch ~60)
            *self.scroll_y.borrow_mut() = 60.0 * 20.0 - 300.0;

            input::setup_controllers(&obj);
        }
    }

    impl WidgetImpl for PianoRollWidget {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            let kw = KEY_WIDTH as f32;
            let vp = obj.build_viewport();
            let theme = types::default_theme();

            // Background
            snapshot.append_color(
                &theme.background,
                &graphene::Rect::new(kw, 0.0, width - kw, height),
            );

            // Clip to the grid area (right of keyboard)
            snapshot.push_clip(&graphene::Rect::new(kw, 0.0, width - kw, height));

            renderer::render_pitch_lines(snapshot, &vp, &theme);

            if let Some(midi) = &*self.data.borrow() {
                renderer::render_beat_grid(snapshot, &vp, midi, &theme);
                renderer::render_notes(
                    snapshot,
                    &vp,
                    midi,
                    *self.active_track.borrow(),
                    *self.selected_note.borrow(),
                    &theme,
                );
            }

            renderer::render_playhead(snapshot, &vp, *self.playhead_time.borrow(), &theme);

            snapshot.pop(); // end clip

            // Piano keyboard (left side, on top of everything)
            let pango_ctx = obj.pango_context();
            keyboard::render_keyboard(
                snapshot,
                &vp,
                &pango_ctx,
                *self.preview_active_pitch.borrow(),
                &theme,
            );
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// GObject wrapper
// ────────────────────────────────────────────────────────────────────────

glib::wrapper! {
    pub struct PianoRollWidget(ObjectSubclass<imp::PianoRollWidget>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

// ────────────────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────────────────

impl PianoRollWidget {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    // ── Callback registration ─────────────────────────────────────

    pub fn connect_seek<F: Fn(f64) + 'static>(&self, f: F) {
        *self.imp().seek_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_data_changed<F: Fn() + 'static>(&self, f: F) {
        *self.imp().data_changed_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_preview_note_on<F: Fn(usize, u8, u8) + 'static>(&self, f: F) {
        *self.imp().preview_note_on_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_preview_note_off<F: Fn(usize, u8) + 'static>(&self, f: F) {
        *self.imp().preview_note_off_callback.borrow_mut() = Some(Box::new(f));
    }

    // ── Viewport helpers ──────────────────────────────────────────

    /// Build a [`Viewport`] snapshot from the current widget state.
    pub(crate) fn build_viewport(&self) -> Viewport {
        Viewport {
            zoom_x: *self.imp().zoom_x.borrow(),
            zoom_y: *self.imp().zoom_y.borrow(),
            scroll_x: *self.imp().scroll_x.borrow(),
            scroll_y: *self.imp().scroll_y.borrow(),
            width: self.width() as f64,
            height: self.height() as f64,
        }
    }

    pub(crate) fn active_synth_index(&self) -> usize {
        let track_idx = *self.imp().active_track.borrow();
        self.track_synth_index(track_idx)
    }

    // ── Data accessors ────────────────────────────────────────────

    pub fn set_data(&self, midi: MidiData) {
        *self.imp().data.borrow_mut() = Some(midi);
        *self.imp().active_track.borrow_mut() = 0;
        *self.imp().selected_note.borrow_mut() = None;
        self.queue_draw();
    }

    pub fn update_data(&self, midi: MidiData) {
        *self.imp().data.borrow_mut() = Some(midi);
        self.queue_draw();
    }

    pub fn get_data_clone(&self) -> Option<MidiData> {
        self.imp().data.borrow().clone()
    }

    // ── Playhead ──────────────────────────────────────────────────

    pub fn get_playhead_tick(&self) -> f64 {
        let time = *self.imp().playhead_time.borrow();
        if let Some(midi) = &*self.imp().data.borrow() {
            time * midi.ticks_per_beat as f64 * (midi.get_bpm() / 60.0)
        } else {
            0.0
        }
    }

    pub fn set_playhead_tick(&self, tick: f64) {
        if let Some(midi) = &*self.imp().data.borrow() {
            let tps = midi.ticks_per_beat as f64 * (midi.get_bpm() / 60.0);
            if tps > 0.0 {
                self.set_playhead(tick / tps);
            }
        }
    }

    pub fn set_playhead(&self, time: f64) {
        if self.imp().drag_state.borrow().is_dragging_playhead {
            return;
        }
        *self.imp().playhead_time.borrow_mut() = time;
        let zx = *self.imp().zoom_x.borrow();
        let p_x = time * zx;
        let mut sx = *self.imp().scroll_x.borrow();
        let width = self.width() as f64 - KEY_WIDTH;
        if p_x > sx + width * 0.9 {
            sx = p_x - width * 0.1;
        } else if p_x < sx {
            sx = p_x - width * 0.1;
        }
        if sx < 0.0 {
            sx = 0.0;
        }
        *self.imp().scroll_x.borrow_mut() = sx;
        self.queue_draw();
    }

    // ── Track selection ───────────────────────────────────────────

    pub fn set_active_track(&self, track_idx: usize) {
        *self.imp().active_track.borrow_mut() = track_idx;
        *self.imp().selected_note.borrow_mut() = None;
        self.queue_draw();
    }

    pub fn track_synth_index(&self, track_idx: usize) -> usize {
        self.imp()
            .data
            .borrow()
            .as_ref()
            .and_then(|midi| midi.tracks.get(track_idx))
            .map(|track| track.synth_index)
            .unwrap_or(track_idx)
    }

    // ── Configuration ─────────────────────────────────────────────

    /// Set the default note duration in beats (from user config).
    pub fn set_default_note_beats(&self, beats: f64) {
        *self.imp().default_note_beats.borrow_mut() = beats.max(0.0625);
    }

    // ── Typing keyboard ──────────────────────────────────────────

    /// Enable or disable the typing-keyboard-to-piano mode.
    pub fn set_typing_keyboard_enabled(&self, enabled: bool) {
        *self.imp().typing_keyboard_enabled.borrow_mut() = enabled;
        // When disabling, release all held notes.
        if !enabled {
            self.release_all_typing_keys();
        }
    }

    /// Query whether the typing keyboard mode is active.
    pub fn is_typing_keyboard_enabled(&self) -> bool {
        *self.imp().typing_keyboard_enabled.borrow()
    }

    /// Release all currently held typing-keyboard notes.
    fn release_all_typing_keys(&self) {
        let imp = self.imp();
        let pitches: Vec<u8> = imp.typing_pressed_pitches.borrow().iter().copied().collect();
        let synth_index = self.active_synth_index();
        for pitch in pitches {
            if let Some(cb) = &*imp.preview_note_off_callback.borrow() {
                cb(synth_index, pitch);
            }
        }
        imp.typing_pressed_pitches.borrow_mut().clear();
        // Clear visual highlight
        *imp.preview_active_pitch.borrow_mut() = None;
        self.queue_draw();
    }
}
