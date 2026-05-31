//! Custom GTK4 piano-roll widget.
//!
//! [`PianoRollWidget`] is a GObject subclass that renders a grid-based MIDI
//! piano roll with:
//!
//! * A keyboard strip on the left edge.
//! * Horizontal beat / bar grid lines.
//! * Editable note rectangles that can be placed, moved, and resized.
//! * A draggable playhead.
//! * Live note preview callbacks for auditioning.
//!
//! The widget communicates changes back to the main window through closures
//! registered via `connect_*` methods.

use crate::midi::{MidiData, Note};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene};
use gtk4 as gtk;
use std::cell::RefCell;

const KEY_WIDTH: f64 = 70.0;
const BEATS_PER_BAR: u64 = 4;
const SNAP_SUBDIVISIONS: u64 = 8; // snap to 1/8 beat (32nd notes)

fn snap_tick(tick: u64, ticks_per_beat: u16) -> u64 {
    let grid = ticks_per_beat as u64 / SNAP_SUBDIVISIONS;
    if grid == 0 {
        return tick;
    }
    ((tick + grid / 2) / grid) * grid
}

mod imp {
    use super::*;

    #[derive(Default, PartialEq, Copy, Clone)]
    pub enum NoteDragMode {
        #[default]
        None,
        Move,
        ResizeDuration,
    }

    #[derive(Default)]
    pub struct PianoRollWidget {
        pub data: RefCell<Option<MidiData>>,
        pub playhead_time: RefCell<f64>,
        pub zoom_x: RefCell<f64>,
        pub zoom_y: RefCell<f64>,
        pub scroll_x: RefCell<f64>,
        pub scroll_y: RefCell<f64>,
        pub active_track: RefCell<usize>,
        pub selected_note: RefCell<Option<usize>>,
        pub drag_orig_note: RefCell<Option<Note>>,
        pub note_drag_mode: RefCell<NoteDragMode>,
        pub is_dragging_playhead: RefCell<bool>,
        pub drag_start_x: RefCell<f64>,
        /// Offset in ticks from note start_tick to where the click landed (for move mode).
        pub drag_click_offset_ticks: RefCell<f64>,
        /// Last dx from GestureDrag (so scroll handler can re-run position update).
        pub drag_last_dx: RefCell<f64>,
        /// Last dy from GestureDrag.
        pub drag_last_dy: RefCell<f64>,
        #[allow(clippy::type_complexity)]
        pub seek_callback: RefCell<Option<Box<dyn Fn(f64)>>>,
        #[allow(clippy::type_complexity)]
        pub data_changed_callback: RefCell<Option<Box<dyn Fn()>>>,
        #[allow(clippy::type_complexity)]
        pub preview_note_on_callback: RefCell<Option<Box<dyn Fn(usize, u8, u8)>>>,
        #[allow(clippy::type_complexity)]
        pub preview_note_off_callback: RefCell<Option<Box<dyn Fn(usize, u8)>>>,
        pub preview_active_pitch: RefCell<Option<u8>>,
        /// Default note duration in beats (from config).
        pub default_note_beats: RefCell<f64>,
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
            self.obj().set_hexpand(true);
            self.obj().set_vexpand(true);
            self.obj().set_size_request(800, 600);
            self.obj().set_focusable(true);

            *self.zoom_x.borrow_mut() = 150.0;
            *self.zoom_y.borrow_mut() = 20.0;
            *self.default_note_beats.borrow_mut() = 1.0;
            // Default scroll to middle C area (pitch ~60)
            *self.scroll_y.borrow_mut() = 60.0 * 20.0 - 300.0;

            self.obj().setup_controllers();
        }
    }

    impl WidgetImpl for PianoRollWidget {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let width = self.obj().width() as f32;
            let height = self.obj().height() as f32;
            let kw = KEY_WIDTH as f32;

            let bg_color = gdk::RGBA::new(0.1, 0.1, 0.1, 1.0);
            snapshot.append_color(&bg_color, &graphene::Rect::new(kw, 0.0, width - kw, height));

            let zx = *self.zoom_x.borrow() as f32;
            let zy = *self.zoom_y.borrow() as f32;
            let p_time = *self.playhead_time.borrow() as f32;
            let act_track = *self.active_track.borrow();
            let sel_note = *self.selected_note.borrow();
            let offset_x = *self.scroll_x.borrow() as f32;
            let offset_y = *self.scroll_y.borrow() as f32;

            let pitch_to_y = |pitch: u8| -> f32 { height - (pitch as f32 * zy) + offset_y };

            let line_color = gdk::RGBA::new(0.2, 0.2, 0.2, 1.0);

            snapshot.push_clip(&graphene::Rect::new(kw, 0.0, width - kw, height));

            // Horizontal pitch lines
            for pitch in 0..128 {
                let y = pitch_to_y(pitch);
                if y > -zy && y < height + zy {
                    snapshot
                        .append_color(&line_color, &graphene::Rect::new(kw, y, width - kw, 1.0));
                }
            }

            // Vertical beat/bar grid lines
            if let Some(midi) = &*self.data.borrow() {
                let ticks_per_sec = (midi.ticks_per_beat as f32) * (midi.get_bpm() as f32 / 60.0);
                let tpb = midi.ticks_per_beat as f32;
                let bar_ticks = tpb * BEATS_PER_BAR as f32;
                let sub_ticks = tpb / SNAP_SUBDIVISIONS as f32;

                // Calculate visible time range
                let t_start = offset_x / zx;
                let t_end = (offset_x + width) / zx;
                let tick_start = (t_start * ticks_per_sec) as u64;
                let tick_end = (t_end * ticks_per_sec) as u64;

                let sub_grid = sub_ticks as u64;
                if sub_grid > 0 {
                    let first = (tick_start / sub_grid) * sub_grid;
                    let mut tick = first;
                    while tick <= tick_end + sub_grid {
                        let x = (tick as f32 / ticks_per_sec) * zx - offset_x + kw;
                        if x >= kw && x <= width {
                            let is_bar = (tick as f32 % bar_ticks).abs() < 0.5;
                            let is_beat = (tick as f32 % tpb).abs() < 0.5;
                            let (color, w) = if is_bar {
                                (gdk::RGBA::new(0.5, 0.5, 0.5, 0.8), 2.0)
                            } else if is_beat {
                                (gdk::RGBA::new(0.35, 0.35, 0.35, 0.6), 1.0)
                            } else {
                                (gdk::RGBA::new(0.2, 0.2, 0.2, 0.4), 1.0)
                            };
                            snapshot.append_color(&color, &graphene::Rect::new(x, 0.0, w, height));
                        }
                        tick += sub_grid;
                    }
                }

                // Draw notes
                for (t_idx, track) in midi.tracks.iter().enumerate() {
                    let is_active = t_idx == act_track;
                    for (n_idx, note) in track.notes.iter().enumerate() {
                        let start_sec = note.start_tick as f32 / ticks_per_sec;
                        let end_sec = note.end_tick as f32 / ticks_per_sec;
                        let dur_sec = end_sec - start_sec;

                        let x = start_sec * zx - offset_x + kw;
                        let y = pitch_to_y(note.pitch) - zy;
                        let mut w = dur_sec * zx;
                        if w < 2.0 {
                            w = 2.0;
                        }
                        let h = zy - 1.0;

                        if x + w > kw && x < width && y + h > 0.0 && y < height {
                            let note_color = if is_active {
                                if sel_note == Some(n_idx) {
                                    gdk::RGBA::new(1.0, 0.8, 0.2, 1.0)
                                } else {
                                    gdk::RGBA::new(0.2, 0.6, 1.0, 1.0)
                                }
                            } else {
                                gdk::RGBA::new(0.5, 0.5, 0.5, 0.4)
                            };
                            snapshot.append_color(&note_color, &graphene::Rect::new(x, y, w, h));
                            let bc =
                                gdk::RGBA::new(1.0, 1.0, 1.0, if is_active { 0.5 } else { 0.1 });
                            snapshot.append_color(&bc, &graphene::Rect::new(x, y, w, 1.0));
                            snapshot
                                .append_color(&bc, &graphene::Rect::new(x, y + h - 1.0, w, 1.0));
                            snapshot.append_color(&bc, &graphene::Rect::new(x, y, 1.0, h));
                            snapshot
                                .append_color(&bc, &graphene::Rect::new(x + w - 1.0, y, 1.0, h));
                        }
                    }
                }
            }

            // Playhead
            let p_x = p_time * zx - offset_x + kw;
            if p_x >= kw - 2.0 && p_x <= width + 2.0 {
                let pc = gdk::RGBA::new(1.0, 0.2, 0.2, 1.0);
                snapshot.append_color(&pc, &graphene::Rect::new(p_x - 1.0, 0.0, 3.0, height));
            }

            snapshot.pop(); // end clip

            // Piano keys (left side)
            let black_key_color = gdk::RGBA::new(0.05, 0.05, 0.05, 1.0);
            let white_key_color = gdk::RGBA::new(0.9, 0.9, 0.9, 1.0);
            let key_border = gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);
            let text_color = gdk::RGBA::new(0.2, 0.2, 0.2, 1.0);
            let active_white_color = gdk::RGBA::new(1.0, 0.8, 0.2, 1.0);
            let active_black_color = gdk::RGBA::new(0.8, 0.6, 0.1, 1.0);
            let pango_ctx = self.obj().pango_context();
            let font_desc = gtk::pango::FontDescription::from_string("Sans 11");
            let active_pitch = *self.preview_active_pitch.borrow();

            let wk_offsets: [(f32, f32); 7] = [
                (0.0, 5.0 / 3.0),
                (5.0 / 3.0, 10.0 / 3.0),
                (10.0 / 3.0, 5.0),
                (5.0, 5.0 + 1.75),
                (5.0 + 1.75, 5.0 + 3.5),
                (5.0 + 3.5, 5.0 + 5.25),
                (5.0 + 5.25, 12.0),
            ];

            // First pass: White keys
            for pitch in 0u8..128 {
                let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                if is_black {
                    continue;
                }

                let octave = pitch / 12;
                let wk_idx = match pitch % 12 {
                    0 => 0,
                    2 => 1,
                    4 => 2,
                    5 => 3,
                    7 => 4,
                    9 => 5,
                    11 => 6,
                    _ => unreachable!(),
                };
                let (bottom_rel, top_rel) = wk_offsets[wk_idx];
                let octave_bottom_y = pitch_to_y(octave * 12);
                let bottom_y = octave_bottom_y - bottom_rel * zy;
                let top_y = octave_bottom_y - top_rel * zy;
                let h = bottom_y - top_y;

                if bottom_y < -zy || top_y > height + zy {
                    continue;
                }

                let is_active = active_pitch == Some(pitch);
                let color = if is_active {
                    &active_white_color
                } else {
                    &white_key_color
                };

                snapshot.append_color(color, &graphene::Rect::new(0.0, top_y, kw, h));
                snapshot.append_color(&key_border, &graphene::Rect::new(0.0, bottom_y, kw, 1.0));

                if pitch % 12 == 0 {
                    let layout = gtk::pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let oct_num = pitch as i32 / 12 - 1;
                    layout.set_text(&format!("C{}", oct_num));
                    snapshot.save();
                    snapshot.translate(&graphene::Point::new(4.0, bottom_y - zy / 2.0 - 6.0));
                    snapshot.append_layout(&layout, &text_color);
                    snapshot.restore();
                }
            }

            // Second pass: Black keys
            for pitch in 0u8..128 {
                let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                if !is_black {
                    continue;
                }

                let y = pitch_to_y(pitch);
                if y < -zy || y > height + zy {
                    continue;
                }

                let is_active = active_pitch == Some(pitch);
                let color = if is_active {
                    &active_black_color
                } else {
                    &black_key_color
                };
                let kw_actual = kw * 0.6;

                snapshot.append_color(color, &graphene::Rect::new(0.0, y - zy, kw_actual, zy));
                snapshot.append_color(
                    &key_border,
                    &graphene::Rect::new(0.0, y - zy, kw_actual, 1.0),
                );
                snapshot.append_color(&key_border, &graphene::Rect::new(0.0, y, kw_actual, 1.0));
                snapshot.append_color(
                    &key_border,
                    &graphene::Rect::new(kw_actual, y - zy, 1.0, zy),
                );
            }
        }
    }
}

glib::wrapper! {
    pub struct PianoRollWidget(ObjectSubclass<imp::PianoRollWidget>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PianoRollWidget {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

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

    fn screen_to_pitch(&self, screen_y: f64) -> u8 {
        let height = self.height() as f64;
        let zy = *self.imp().zoom_y.borrow();
        let offset_y = *self.imp().scroll_y.borrow();
        let pitch_f = (height - screen_y + offset_y) / zy;
        (pitch_f.floor() as i32).clamp(0, 127) as u8
    }

    fn active_synth_index(&self) -> usize {
        let imp = self.imp();
        let track_idx = *imp.active_track.borrow();
        self.track_synth_index(track_idx)
    }

    /// Shared drag position update logic, called from both `drag_update` and
    /// the scroll handler (so dragged items track the cursor even when the
    /// viewport is scrolled mid-drag).
    fn update_drag_position(&self, dx: f64, dy: f64) {
        let imp = self.imp();

        // --- Playhead drag (uses absolute coords, already correct) ---
        if *imp.is_dragging_playhead.borrow() {
            let sx = *imp.drag_start_x.borrow();
            let zx = *imp.zoom_x.borrow();
            let ox = *imp.scroll_x.borrow();
            let mut t = (sx + dx + ox) / zx;
            if t < 0.0 {
                t = 0.0;
            }
            *imp.playhead_time.borrow_mut() = t;
            self.queue_draw();
            return;
        }

        // --- Note drag (use absolute coords to survive scroll/zoom) ---
        let orig = match &*imp.drag_orig_note.borrow() {
            Some(o) => o.clone(),
            None => return,
        };
        let drag_mode = *imp.note_drag_mode.borrow();
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
            let tps = (midi.ticks_per_beat as f64) * (midi.get_bpm() / 60.0);
            let sx = *imp.drag_start_x.borrow();

            // Current absolute position of the cursor (in pixels from time=0)
            let current_abs_x = sx + dx + ox;
            // Convert to ticks
            let cursor_tick = (current_abs_x / zx) * tps;
            let synth_index = midi.tracks[act].synth_index;

            let n = &mut midi.tracks[act].notes[idx];

            if drag_mode == imp::NoteDragMode::ResizeDuration {
                let min_dur = (midi.ticks_per_beat as u64 / SNAP_SUBDIVISIONS).max(1);
                let ne_raw = cursor_tick.max(0.0) as u64;
                let ne_snapped = snap_tick(ne_raw, midi.ticks_per_beat);
                n.end_tick = ne_snapped.max(n.start_tick + min_dur);
            } else if drag_mode == imp::NoteDragMode::Move {
                let dur = orig.end_tick as i64 - orig.start_tick as i64;
                let click_offset = *imp.drag_click_offset_ticks.borrow();
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
            self.queue_draw();
        }
    }

    fn setup_controllers(&self) {
        // Right-click: delete notes
        let right_click = gtk::GestureClick::new();
        right_click.set_button(3);
        let obj_rc = self.clone();
        right_click.connect_pressed(move |_, _n_press, x, y| {
            if x < KEY_WIDTH {
                return;
            }
            let imp = obj_rc.imp();
            obj_rc.grab_focus();
            let zx = *imp.zoom_x.borrow();
            let offset_x = *imp.scroll_x.borrow();
            let act_track = *imp.active_track.borrow();
            let abs_x = x - KEY_WIDTH + offset_x;
            let target_pitch = obj_rc.screen_to_pitch(y);

            let mut changed = false;
            if let Some(midi) = &mut *imp.data.borrow_mut() {
                if act_track < midi.tracks.len() {
                    let tps = (midi.ticks_per_beat as f64) * (midi.get_bpm() / 60.0);
                    let mut found = None;
                    for (i, note) in midi.tracks[act_track].notes.iter().enumerate() {
                        let nx = (note.start_tick as f64 / tps) * zx;
                        let nw = ((note.end_tick - note.start_tick) as f64 / tps) * zx;
                        if abs_x >= nx && abs_x <= nx + nw && target_pitch == note.pitch {
                            found = Some(i);
                            break;
                        }
                    }
                    if let Some(idx) = found {
                        midi.tracks[act_track].notes.remove(idx);
                        *imp.selected_note.borrow_mut() = None;
                        obj_rc.queue_draw();
                        changed = true;
                    }
                }
            }
            if changed {
                if let Some(cb) = &*imp.data_changed_callback.borrow() {
                    cb();
                }
            }
        });

        // Left-button: playhead, note creation/selection/dragging
        let drag = gtk::GestureDrag::new();
        let obj_d = self.clone();
        drag.connect_drag_begin(move |_, start_x, start_y| {
            if start_x < KEY_WIDTH {
                return;
            }
            let imp = obj_d.imp();
            obj_d.grab_focus();
            *imp.is_dragging_playhead.borrow_mut() = false;
            *imp.drag_orig_note.borrow_mut() = None;
            *imp.note_drag_mode.borrow_mut() = imp::NoteDragMode::None;
            let sx_adj = start_x - KEY_WIDTH;
            *imp.drag_start_x.borrow_mut() = sx_adj;

            let p_time = *imp.playhead_time.borrow();
            let zx = *imp.zoom_x.borrow();
            let offset_x = *imp.scroll_x.borrow();
            let p_x = p_time * zx - offset_x;
            let abs_x = sx_adj + offset_x;

            if (sx_adj - p_x).abs() < 10.0 || start_y < 20.0 {
                *imp.is_dragging_playhead.borrow_mut() = true;
                let mut t = abs_x / zx;
                if t < 0.0 {
                    t = 0.0;
                }
                *imp.playhead_time.borrow_mut() = t;
                obj_d.queue_draw();
                return;
            }

            let act_track = *imp.active_track.borrow();
            let target_pitch = obj_d.screen_to_pitch(start_y);

            let mut found_note: Option<(usize, Note, imp::NoteDragMode, usize)> = None;
            let mut tps = 1.0;
            if let Some(midi) = &*imp.data.borrow() {
                tps = (midi.ticks_per_beat as f64) * (midi.get_bpm() / 60.0);
                if act_track < midi.tracks.len() {
                    let synth_index = midi.tracks[act_track].synth_index;
                    for (i, note) in midi.tracks[act_track].notes.iter().enumerate() {
                        let nx = (note.start_tick as f64 / tps) * zx;
                        let nw = ((note.end_tick - note.start_tick) as f64 / tps) * zx;
                        if abs_x >= nx && abs_x <= nx + nw && target_pitch == note.pitch {
                            let mut edge_threshold = 8.0;
                            if nw < 16.0 {
                                edge_threshold = nw / 2.0;
                            }
                            let drag_mode = if abs_x >= nx + nw - edge_threshold {
                                imp::NoteDragMode::ResizeDuration
                            } else {
                                imp::NoteDragMode::Move
                            };
                            found_note = Some((i, note.clone(), drag_mode, synth_index));
                            break;
                        }
                    }
                }
            }

            if let Some((idx, note, drag_mode, synth_index)) = found_note {
                *imp.selected_note.borrow_mut() = Some(idx);
                *imp.drag_orig_note.borrow_mut() = Some(note.clone());
                *imp.note_drag_mode.borrow_mut() = drag_mode;
                // Compute click offset within the note (in ticks) for absolute positioning
                let click_tick = (abs_x / zx) * tps;
                *imp.drag_click_offset_ticks.borrow_mut() = click_tick - note.start_tick as f64;
                if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
                    cb(synth_index, note.pitch, note.velocity);
                }
                *imp.preview_active_pitch.borrow_mut() = Some(note.pitch);
                obj_d.set_cursor_from_name(if drag_mode == imp::NoteDragMode::ResizeDuration {
                    Some("col-resize")
                } else {
                    Some("grabbing")
                });
                obj_d.queue_draw();
            } else {
                // Create new note with snap
                let mut synth_index = act_track;
                if let Some(midi) = &mut *imp.data.borrow_mut() {
                    if act_track < midi.tracks.len() {
                        synth_index = midi.tracks[act_track].synth_index;
                        let raw_tick = ((abs_x / zx) * tps) as u64;
                        let start_tick = snap_tick(raw_tick, midi.ticks_per_beat);
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
                        *imp.drag_orig_note.borrow_mut() = Some(new_note);
                        *imp.note_drag_mode.borrow_mut() = imp::NoteDragMode::Move;

                        *imp.preview_active_pitch.borrow_mut() = Some(target_pitch);

                        obj_d.queue_draw();
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
        });

        let obj_u = self.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let imp = obj_u.imp();
            *imp.drag_last_dx.borrow_mut() = dx;
            *imp.drag_last_dy.borrow_mut() = dy;
            obj_u.update_drag_position(dx, dy);
        });

        let obj_e = self.clone();
        drag.connect_drag_end(move |_, dx, _dy| {
            let imp = obj_e.imp();
            if *imp.is_dragging_playhead.borrow() {
                *imp.is_dragging_playhead.borrow_mut() = false;
                let sx = *imp.drag_start_x.borrow();
                let zx = *imp.zoom_x.borrow();
                let ox = *imp.scroll_x.borrow();
                let mut t = (sx + dx + ox) / zx;
                if t < 0.0 {
                    t = 0.0;
                }
                if let Some(cb) = &*imp.seek_callback.borrow() {
                    cb(t);
                }
            } else if imp.drag_orig_note.borrow().is_some() {
                let active_opt = *imp.preview_active_pitch.borrow();
                if let Some(active) = active_opt {
                    if let Some(cb) = &*imp.preview_note_off_callback.borrow() {
                        cb(obj_e.active_synth_index(), active);
                    }
                    *imp.preview_active_pitch.borrow_mut() = None;
                }
                if let Some(cb) = &*imp.data_changed_callback.borrow() {
                    cb();
                }
                obj_e.queue_draw();
            }
            // Clear drag state so motion handler resumes hover detection
            *imp.drag_orig_note.borrow_mut() = None;
            *imp.note_drag_mode.borrow_mut() = imp::NoteDragMode::None;
            // Reset cursor to default when drag ends
            obj_e.set_cursor_from_name(None);
        });

        // Scroll: vertical = up/down pitch, horizontal = left/right time
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let obj_s = self.clone();
        scroll.connect_scroll(move |controller, dx, dy| {
            let imp = obj_s.imp();

            let state = controller.current_event_state();
            if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                let mut zx = *imp.zoom_x.borrow();
                let old_zx = zx;

                let zoom_factor = 1.0 - dy * 0.1;
                zx *= zoom_factor;
                zx = zx.clamp(10.0, 1000.0);
                *imp.zoom_x.borrow_mut() = zx;

                let center_x_pixels = obj_s.width() as f64 / 2.0;
                let mut sx = *imp.scroll_x.borrow();
                let center_time = (sx + center_x_pixels) / old_zx;
                sx = center_time * zx - center_x_pixels;
                if sx < 0.0 {
                    sx = 0.0;
                }
                *imp.scroll_x.borrow_mut() = sx;

                obj_s.queue_draw();
                return glib::Propagation::Stop;
            }

            let zy = *imp.zoom_y.borrow();
            let max_scroll_y = 128.0 * zy - obj_s.height() as f64;

            let mut sy = *imp.scroll_y.borrow();
            let mut sx = *imp.scroll_x.borrow();

            if state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                sx += (dx + dy) * 50.0;
            } else {
                sy -= dy * 40.0;
                sx += dx * 50.0;
            }

            sy = sy.clamp(0.0, max_scroll_y.max(0.0));
            *imp.scroll_y.borrow_mut() = sy;

            // If a drag is active, re-run position update with stored dx/dy
            // so the dragged item tracks the cursor after scroll.
            let is_dragging =
                *imp.is_dragging_playhead.borrow() || imp.drag_orig_note.borrow().is_some();
            if is_dragging {
                let last_dx = *imp.drag_last_dx.borrow();
                let last_dy = *imp.drag_last_dy.borrow();
                // Must update scroll_x BEFORE calling update_drag_position
                if sx < 0.0 {
                    sx = 0.0;
                }
                *imp.scroll_x.borrow_mut() = sx;
                obj_s.update_drag_position(last_dx, last_dy);
                obj_s.queue_draw();
                return glib::Propagation::Stop;
            }

            if sx < 0.0 {
                sx = 0.0;
            }
            *imp.scroll_x.borrow_mut() = sx;

            obj_s.queue_draw();
            glib::Propagation::Stop
        });

        let key_ctrl = gtk::EventControllerKey::new();
        let obj_k = self.clone();
        key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
            let imp = obj_k.imp();
            let mut changed = false;
            if keyval == gtk::gdk::Key::Delete || keyval == gtk::gdk::Key::BackSpace {
                if let Some(idx) = *imp.selected_note.borrow() {
                    if let Some(midi) = &mut *imp.data.borrow_mut() {
                        let act = *imp.active_track.borrow();
                        if act < midi.tracks.len() {
                            midi.tracks[act].notes.remove(idx);
                            *imp.selected_note.borrow_mut() = None;
                            obj_k.queue_draw();
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
        });

        let motion = gtk::EventControllerMotion::new();
        let obj_m = self.clone();
        motion.connect_motion(move |_, x, y| {
            let imp = obj_m.imp();
            // Don't change cursor during an active drag
            if *imp.is_dragging_playhead.borrow() || imp.drag_orig_note.borrow().is_some() {
                return;
            }
            if x < KEY_WIDTH {
                obj_m.set_cursor_from_name(None);
                return;
            }
            let zx = *imp.zoom_x.borrow();
            let offset_x = *imp.scroll_x.borrow();
            let act_track = *imp.active_track.borrow();
            let p_time = *imp.playhead_time.borrow();

            let abs_x = x - KEY_WIDTH + offset_x;
            let p_x = p_time * zx;

            if (abs_x - p_x).abs() < 10.0 || y < 20.0 {
                obj_m.set_cursor_from_name(Some("col-resize"));
                return;
            }

            let target_pitch = obj_m.screen_to_pitch(y);
            let mut cursor_name = None;

            if let Some(midi) = &*imp.data.borrow() {
                let tps = (midi.ticks_per_beat as f64) * (midi.get_bpm() / 60.0);
                if act_track < midi.tracks.len() {
                    for note in &midi.tracks[act_track].notes {
                        let nx = (note.start_tick as f64 / tps) * zx;
                        let nw = ((note.end_tick - note.start_tick) as f64 / tps) * zx;
                        if abs_x >= nx && abs_x <= nx + nw && target_pitch == note.pitch {
                            let mut edge_threshold = 8.0;
                            if nw < 16.0 {
                                edge_threshold = nw / 2.0;
                            }
                            if abs_x >= nx + nw - edge_threshold {
                                cursor_name = Some("col-resize");
                            }
                            break;
                        }
                    }
                }
            }

            obj_m.set_cursor_from_name(cursor_name);
        });

        self.add_controller(right_click);
        self.add_controller(drag);
        self.add_controller(scroll);
        self.add_controller(key_ctrl);
        self.add_controller(motion);
    }

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
        if *self.imp().is_dragging_playhead.borrow() {
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

    pub fn set_active_track(&self, track_idx: usize) {
        *self.imp().active_track.borrow_mut() = track_idx;
        *self.imp().selected_note.borrow_mut() = None;
        self.queue_draw();
    }

    pub fn get_data_clone(&self) -> Option<MidiData> {
        self.imp().data.borrow().clone()
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

    /// Set the default note duration in beats (from user config).
    pub fn set_default_note_beats(&self, beats: f64) {
        *self.imp().default_note_beats.borrow_mut() = beats.max(0.0625);
    }
}
