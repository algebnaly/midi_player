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
pub mod types;
mod viewport;

use crate::midi::{MidiData, Note};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene};
use gtk4 as gtk;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use types::{DragState, EditMode, KEY_WIDTH, SelectionRect, put_note_length, snap_tick_to_beat};
use viewport::Viewport;

#[derive(Debug)]
struct PendingPutNote {
    track_index: usize,
    note_index: usize,
    start_tick: u64,
    pitch: u8,
    channel: u8,
    started_at: Instant,
    length_quantization_enabled: bool,
}

// ────────────────────────────────────────────────────────────────────────
// GObject private implementation
// ────────────────────────────────────────────────────────────────────────

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PianoRollWidget {
        // ── Data ──────────────────────────────────────────────
        pub data: RefCell<Option<MidiData>>,
        pub active_track: RefCell<usize>,
        /// Multi-note selection (indices into the active track's note list).
        pub selected_notes: RefCell<HashSet<usize>>,

        // ── Viewport ──────────────────────────────────────────────
        pub playhead_time: RefCell<f64>,
        pub zoom_x: RefCell<f64>,
        pub zoom_y: RefCell<f64>,
        pub scroll_x: RefCell<f64>,
        pub scroll_y: RefCell<f64>,

        // ── Interaction ───────────────────────────────────────────
        pub edit_mode: RefCell<EditMode>,
        pub drag_state: RefCell<DragState>,
        pub preview_active_pitch: RefCell<Option<u8>>,
        pub selection_rect: RefCell<Option<SelectionRect>>,
        /// Last known cursor position in widget coords (updated by motion + drag).
        pub cursor_x: RefCell<f64>,
        pub cursor_y: RefCell<f64>,

        // ── Typing keyboard ───────────────────────────────────────
        /// When true, QWERTY keys trigger note-on/off.
        pub typing_keyboard_enabled: RefCell<bool>,
        /// Currently held typing keys and their original pitch, so note-off is stable.
        pub typing_pressed_keys: RefCell<HashMap<gdk::Key, u8>>,
        /// Persistent octave offset for typing-keyboard mode.
        pub typing_octave_offset: RefCell<i8>,
        /// Notes currently held by an external MIDI input device.
        pub external_pressed_notes: RefCell<HashSet<(u8, u8)>>,
        /// Pitches currently sounding from automatic sequencer playback.
        pub playback_active_pitches: RefCell<HashSet<u8>>,
        /// Put-mode notes waiting for their physical MIDI Note-Off.
        pub(super) pending_put_notes: RefCell<HashMap<(u8, u8), PendingPutNote>>,
        /// L-controlled Put duration policy. Defaults to fixed quarter notes.
        pub put_length_quantization_enabled: RefCell<bool>,

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
        #[allow(clippy::type_complexity)]
        pub status_callback: RefCell<Option<Box<dyn Fn(&str)>>>,
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
                    &*self.selected_notes.borrow(),
                    &theme,
                );
            }

            renderer::render_playhead(snapshot, &vp, *self.playhead_time.borrow(), &theme);

            // Render selection rectangle overlay (if active)
            if let Some(sel) = &*self.selection_rect.borrow() {
                renderer::render_selection_rect(snapshot, &vp, sel, &theme);
            }

            snapshot.pop(); // end clip

            // Piano keyboard (left side, on top of everything)
            let pango_ctx = obj.pango_context();
            let active_pitches = keyboard::keyboard_active_pitches(
                *self.preview_active_pitch.borrow(),
                self.typing_pressed_keys
                    .borrow()
                    .values()
                    .copied()
                    .chain(
                        self.external_pressed_notes
                            .borrow()
                            .iter()
                            .map(|(_, pitch)| *pitch),
                    )
                    .chain(self.playback_active_pitches.borrow().iter().copied()),
            );
            keyboard::render_keyboard(snapshot, &vp, &pango_ctx, &active_pitches, &theme);
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

    pub fn connect_status<F: Fn(&str) + 'static>(&self, f: F) {
        *self.imp().status_callback.borrow_mut() = Some(Box::new(f));
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
        self.imp().selected_notes.borrow_mut().clear();
        self.imp().pending_put_notes.borrow_mut().clear();
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
        self.imp().selected_notes.borrow_mut().clear();
        self.imp().playback_active_pitches.borrow_mut().clear();
        self.queue_draw();
    }

    pub fn active_track_index(&self) -> usize {
        *self.imp().active_track.borrow()
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

    /// Update the keyboard-strip highlight for a physical MIDI note.
    pub fn set_external_note_active(&self, channel: u8, pitch: u8, active: bool) {
        let key = (channel, pitch);
        if active {
            self.imp().external_pressed_notes.borrow_mut().insert(key);
        } else {
            self.imp().external_pressed_notes.borrow_mut().remove(&key);
        }
        self.queue_draw();
    }

    pub fn clear_external_notes(&self) {
        self.imp().external_pressed_notes.borrow_mut().clear();
        self.queue_draw();
    }

    /// Replace the keyboard highlights owned by automatic playback.
    pub fn set_playback_active_pitches(&self, pitches: impl IntoIterator<Item = u8>) {
        let next: HashSet<u8> = pitches.into_iter().collect();
        let mut current = self.imp().playback_active_pitches.borrow_mut();
        if *current != next {
            *current = next;
            drop(current);
            self.queue_draw();
        }
    }

    /// Begin a physical-MIDI note at the playhead while Put mode is active.
    ///
    /// The playhead itself remains continuous. Only the inserted note is
    /// floored to a quarter-note boundary. The provisional configured duration
    /// is replaced with a quantized key-hold duration on Note-Off.
    pub fn put_midi_note_on(
        &self,
        channel: u8,
        pitch: u8,
        velocity: u8,
        occurred_at: Instant,
    ) -> bool {
        let imp = self.imp();
        if *imp.edit_mode.borrow() != EditMode::Put {
            return false;
        }

        // Finalize an unexpected repeated Note-On before starting another.
        if imp
            .pending_put_notes
            .borrow()
            .contains_key(&(channel, pitch))
        {
            self.put_midi_note_off(channel, pitch, occurred_at);
        }

        let playhead_tick = self.get_playhead_tick().max(0.0).floor() as u64;
        let active_track = *imp.active_track.borrow();
        let length_quantization_enabled = *imp.put_length_quantization_enabled.borrow();
        let pending = if let Some(midi) = &mut *imp.data.borrow_mut()
            && let Some(track) = midi.tracks.get_mut(active_track)
        {
            let start_tick = snap_tick_to_beat(playhead_tick, midi.ticks_per_beat);
            let duration = u64::from(midi.ticks_per_beat).max(1);
            track.notes.push(Note {
                pitch,
                velocity,
                start_tick,
                end_tick: start_tick + duration,
                channel,
            });
            Some(PendingPutNote {
                track_index: active_track,
                note_index: track.notes.len() - 1,
                start_tick,
                pitch,
                channel,
                started_at: occurred_at,
                length_quantization_enabled,
            })
        } else {
            None
        };

        if let Some(pending) = pending {
            imp.pending_put_notes
                .borrow_mut()
                .insert((channel, pitch), pending);
            imp.selected_notes.borrow_mut().clear();
            self.queue_draw();
            self.update_status();
            if let Some(callback) = &*imp.data_changed_callback.borrow() {
                callback();
            }
            true
        } else {
            false
        }
    }

    /// Finalize a Put-mode note using the physical key's actual hold time.
    pub fn put_midi_note_off(&self, channel: u8, pitch: u8, occurred_at: Instant) -> bool {
        let imp = self.imp();
        let Some(pending) = imp.pending_put_notes.borrow_mut().remove(&(channel, pitch)) else {
            return false;
        };
        let elapsed_seconds = occurred_at
            .saturating_duration_since(pending.started_at)
            .as_secs_f64();

        let mut deleted_duplicate = None;
        let changed = if let Some(midi) = &mut *imp.data.borrow_mut() {
            let duration = put_note_length(
                pending.length_quantization_enabled,
                elapsed_seconds,
                midi.get_bpm(),
                midi.ticks_per_beat,
            );
            let end_tick = pending.start_tick + duration;
            if let Some(track) = midi.tracks.get_mut(pending.track_index) {
                let pending_still_exists =
                    track.notes.get(pending.note_index).is_some_and(|note| {
                        note.start_tick == pending.start_tick
                            && note.pitch == pending.pitch
                            && note.channel == pending.channel
                    });
                if !pending_still_exists {
                    false
                } else if has_exact_note(
                    &track.notes,
                    Some(pending.note_index),
                    pending.channel,
                    pending.pitch,
                    pending.start_tick,
                    end_tick,
                ) {
                    track.notes.remove(pending.note_index);
                    deleted_duplicate = Some((pending.track_index, pending.note_index));
                    true
                } else {
                    track.notes[pending.note_index].end_tick = end_tick;
                    true
                }
            } else {
                false
            }
        } else {
            false
        };

        if changed {
            if let Some((track_index, note_index)) = deleted_duplicate {
                self.handle_note_deleted(track_index, note_index);
            }
            self.queue_draw();
            if let Some(callback) = &*imp.data_changed_callback.borrow() {
                callback();
            }
        }
        changed
    }

    /// Keep pending Put-note indices valid when right-click deletes a note.
    pub(crate) fn handle_note_deleted(&self, track_index: usize, note_index: usize) {
        self.imp()
            .pending_put_notes
            .borrow_mut()
            .retain(|_, pending| {
                if pending.track_index != track_index {
                    return true;
                }
                if pending.note_index == note_index {
                    return false;
                }
                if pending.note_index > note_index {
                    pending.note_index -= 1;
                }
                true
            });
    }

    // ── Configuration ─────────────────────────────────────────────

    /// Set the default note duration in beats (from user config).
    pub fn set_default_note_beats(&self, beats: f64) {
        *self.imp().default_note_beats.borrow_mut() = beats.max(0.0625);
    }

    // ── Edit mode ──────────────────────────────────────────────

    pub fn set_edit_mode(&self, mode: EditMode) {
        *self.imp().edit_mode.borrow_mut() = mode;
        if mode != EditMode::Draw {
            self.set_typing_keyboard_enabled(false);
        }
        // Clear selection rect when switching modes
        *self.imp().selection_rect.borrow_mut() = None;
        self.update_status();
        self.queue_draw();
    }

    pub fn get_edit_mode(&self) -> EditMode {
        *self.imp().edit_mode.borrow()
    }

    /// Toggle held-time duration quantization while Put mode is active.
    /// Returns `true` when the key was accepted in the current mode.
    pub fn toggle_put_length_quantization(&self) -> bool {
        let imp = self.imp();
        if *imp.edit_mode.borrow() != EditMode::Put {
            return false;
        }
        let enabled = !*imp.put_length_quantization_enabled.borrow();
        *imp.put_length_quantization_enabled.borrow_mut() = enabled;
        self.update_status();
        true
    }

    /// Push a status update through the callback.
    pub(crate) fn update_status(&self) {
        let imp = self.imp();
        let keyboard_status = if *imp.typing_keyboard_enabled.borrow() {
            let octave_offset = *imp.typing_octave_offset.borrow();
            Some(format!(
                "[Keyboard] Oct {octave_offset:+}  Q={}  Z={}",
                note_name(60 + i16::from(octave_offset) * 12),
                note_name(48 + i16::from(octave_offset) * 12)
            ))
        } else {
            None
        };
        let sel_count = imp.selected_notes.borrow().len();
        let mode_status = if *imp.edit_mode.borrow() == EditMode::Put {
            if *imp.put_length_quantization_enabled.borrow() {
                "Put | Length Quantize: ON"
            } else {
                "Put | Length Quantize: OFF (Quarter)"
            }
        } else {
            imp.edit_mode.borrow().label()
        };
        let msg = if let Some(status) = keyboard_status {
            if sel_count > 0 {
                format!("{status}  {sel_count} note(s) selected")
            } else {
                status
            }
        } else if sel_count > 0 {
            format!("[{mode_status}] {sel_count} note(s) selected")
        } else {
            format!("[{mode_status}]")
        };
        if let Some(cb) = &*imp.status_callback.borrow() {
            cb(&msg);
        }
    }

    // ── Typing keyboard ──────────────────────────────────────────

    /// Enable or disable the typing-keyboard-to-piano mode.
    pub fn set_typing_keyboard_enabled(&self, enabled: bool) {
        *self.imp().typing_keyboard_enabled.borrow_mut() = enabled;
        if enabled {
            *self.imp().edit_mode.borrow_mut() = EditMode::Draw;
            *self.imp().selection_rect.borrow_mut() = None;
        }
        // When disabling, release all held notes.
        if !enabled {
            self.release_all_typing_keys();
        }
        self.update_status();
        self.queue_draw();
    }

    /// Query whether the typing keyboard mode is active.
    pub fn is_typing_keyboard_enabled(&self) -> bool {
        *self.imp().typing_keyboard_enabled.borrow()
    }

    /// Normal is the sole hub from which another interaction mode may start.
    pub fn is_normal_mode(&self) -> bool {
        *self.imp().edit_mode.borrow() == EditMode::Draw
            && !*self.imp().typing_keyboard_enabled.borrow()
    }

    pub fn enter_normal_mode(&self) {
        self.set_typing_keyboard_enabled(false);
        self.set_edit_mode(EditMode::Draw);
    }

    pub fn enter_select_mode(&self) {
        if self.is_normal_mode() {
            self.set_edit_mode(EditMode::Select);
        }
    }

    pub fn enter_put_mode(&self) {
        if self.is_normal_mode() {
            self.set_edit_mode(EditMode::Put);
        }
    }

    pub fn enter_typing_keyboard_mode(&self) {
        if self.is_normal_mode() {
            self.set_typing_keyboard_enabled(true);
        }
    }

    /// Release all currently held typing-keyboard notes.
    fn release_all_typing_keys(&self) {
        let imp = self.imp();
        let pitches: Vec<u8> = imp.typing_pressed_keys.borrow().values().copied().collect();
        let synth_index = self.active_synth_index();
        for pitch in pitches {
            if let Some(cb) = &*imp.preview_note_off_callback.borrow() {
                cb(synth_index, pitch);
            }
        }
        imp.typing_pressed_keys.borrow_mut().clear();
        // Clear visual highlight
        *imp.preview_active_pitch.borrow_mut() = None;
        self.queue_draw();
    }
}

fn note_name(pitch: i16) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let pitch = pitch.clamp(0, 127);
    let octave = pitch / 12 - 1;
    format!("{}{}", NAMES[pitch as usize % 12], octave)
}

fn has_exact_note(
    notes: &[Note],
    excluded_index: Option<usize>,
    channel: u8,
    pitch: u8,
    start_tick: u64,
    end_tick: u64,
) -> bool {
    notes.iter().enumerate().any(|(index, note)| {
        Some(index) != excluded_index
            && note.channel == channel
            && note.pitch == pitch
            && note.start_tick == start_tick
            && note.end_tick == end_tick
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_name_formats_midi_octaves() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(48), "C3");
        assert_eq!(note_name(72), "C5");
    }

    #[test]
    fn exact_note_dedup_preserves_chords_and_different_lengths() {
        let notes = vec![
            Note {
                pitch: 60,
                velocity: 100,
                start_tick: 480,
                end_tick: 960,
                channel: 0,
            },
            Note {
                pitch: 64,
                velocity: 100,
                start_tick: 480,
                end_tick: 960,
                channel: 0,
            },
        ];

        assert!(has_exact_note(&notes, None, 0, 60, 480, 960));
        assert!(!has_exact_note(&notes, None, 0, 60, 480, 720));
        assert!(!has_exact_note(&notes, None, 0, 67, 480, 960));
        assert!(!has_exact_note(&notes, Some(0), 0, 60, 480, 960));
    }
}
