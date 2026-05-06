use crate::midi::{MidiData, Note};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene};
use gtk4 as gtk;
use std::cell::RefCell;

const KEY_WIDTH: f64 = 50.0;
const BEATS_PER_BAR: u64 = 4;
const SNAP_SUBDIVISIONS: u64 = 4; // snap to 1/4 beat (16th notes)

fn snap_tick(tick: u64, ticks_per_beat: u16) -> u64 {
    let grid = ticks_per_beat as u64 / SNAP_SUBDIVISIONS;
    if grid == 0 {
        return tick;
    }
    ((tick + grid / 2) / grid) * grid
}

mod imp {
    use super::*;

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
        pub is_dragging_playhead: RefCell<bool>,
        pub drag_start_x: RefCell<f64>,
        #[allow(clippy::type_complexity)]
        pub seek_callback: RefCell<Option<Box<dyn Fn(f64)>>>,
        #[allow(clippy::type_complexity)]
        pub data_changed_callback: RefCell<Option<Box<dyn Fn()>>>,
        #[allow(clippy::type_complexity)]
        pub preview_note_on_callback: RefCell<Option<Box<dyn Fn(u8, u8)>>>,
        #[allow(clippy::type_complexity)]
        pub preview_note_off_callback: RefCell<Option<Box<dyn Fn(u8)>>>,
        pub preview_active_pitch: RefCell<Option<u8>>,
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

            *self.zoom_x.borrow_mut() = 100.0;
            *self.zoom_y.borrow_mut() = 12.0;
            // Default scroll to middle C area (pitch ~60)
            *self.scroll_y.borrow_mut() = 60.0 * 12.0 - 300.0;

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
                let ticks_per_sec = (midi.ticks_per_beat as f32) * 2.0;
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
            let pango_ctx = self.obj().pango_context();
            let font_desc = gtk::pango::FontDescription::from_string("Sans 8");

            for pitch in 0u8..128 {
                let y = pitch_to_y(pitch);
                if y < -zy || y > height + zy {
                    continue;
                }
                let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                let color = if is_black {
                    &black_key_color
                } else {
                    &white_key_color
                };
                let kw_actual = if is_black { kw * 0.6 } else { kw };
                snapshot.append_color(color, &graphene::Rect::new(0.0, y - zy, kw_actual, zy));
                snapshot.append_color(&key_border, &graphene::Rect::new(0.0, y, kw, 1.0));

                if pitch % 12 == 0 {
                    let layout = gtk::pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let octave = pitch as i32 / 12 - 1;
                    layout.set_text(&format!("C{}", octave));
                    snapshot.save();
                    snapshot.translate(&graphene::Point::new(2.0, y - zy + 1.0));
                    snapshot.append_layout(&layout, &text_color);
                    snapshot.restore();
                }
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

    pub fn connect_preview_note_on<F: Fn(u8, u8) + 'static>(&self, f: F) {
        *self.imp().preview_note_on_callback.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_preview_note_off<F: Fn(u8) + 'static>(&self, f: F) {
        *self.imp().preview_note_off_callback.borrow_mut() = Some(Box::new(f));
    }

    fn screen_to_pitch(&self, screen_y: f64) -> u8 {
        let height = self.height() as f64;
        let zy = *self.imp().zoom_y.borrow();
        let offset_y = *self.imp().scroll_y.borrow();
        let pitch_f = (height - screen_y + offset_y) / zy;
        (pitch_f.floor() as i32).clamp(0, 127) as u8
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
                    let tps = (midi.ticks_per_beat as f64) * 2.0;
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

            let mut found_note: Option<(usize, Note)> = None;
            let mut tps = 1.0;
            if let Some(midi) = &*imp.data.borrow() {
                tps = (midi.ticks_per_beat as f64) * 2.0;
                if act_track < midi.tracks.len() {
                    for (i, note) in midi.tracks[act_track].notes.iter().enumerate() {
                        let nx = (note.start_tick as f64 / tps) * zx;
                        let nw = ((note.end_tick - note.start_tick) as f64 / tps) * zx;
                        if abs_x >= nx && abs_x <= nx + nw && target_pitch == note.pitch {
                            found_note = Some((i, note.clone()));
                            break;
                        }
                    }
                }
            }

            if let Some((idx, note)) = found_note {
                *imp.selected_note.borrow_mut() = Some(idx);
                *imp.drag_orig_note.borrow_mut() = Some(note.clone());
                if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
                    cb(note.pitch, note.velocity);
                }
                *imp.preview_active_pitch.borrow_mut() = Some(note.pitch);
                obj_d.queue_draw();
            } else {
                // Create new note with snap
                if let Some(midi) = &mut *imp.data.borrow_mut() {
                    if act_track < midi.tracks.len() {
                        let raw_tick = ((abs_x / zx) * tps) as u64;
                        let start_tick = snap_tick(raw_tick, midi.ticks_per_beat);
                        let note_len = midi.ticks_per_beat as u64 / SNAP_SUBDIVISIONS;
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

                        if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
                            cb(target_pitch, 100);
                        }
                        *imp.preview_active_pitch.borrow_mut() = Some(target_pitch);

                        obj_d.queue_draw();
                    }
                }
                if let Some(cb) = &*imp.data_changed_callback.borrow() {
                    cb();
                }
            }
        });

        let obj_u = self.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let imp = obj_u.imp();
            if *imp.is_dragging_playhead.borrow() {
                let sx = *imp.drag_start_x.borrow();
                let zx = *imp.zoom_x.borrow();
                let ox = *imp.scroll_x.borrow();
                let mut t = (sx + dx + ox) / zx;
                if t < 0.0 {
                    t = 0.0;
                }
                *imp.playhead_time.borrow_mut() = t;
                obj_u.queue_draw();
                return;
            }
            if let Some(orig) = &*imp.drag_orig_note.borrow() {
                if let Some(idx) = *imp.selected_note.borrow() {
                    if let Some(midi) = &mut *imp.data.borrow_mut() {
                        let act = *imp.active_track.borrow();
                        if act < midi.tracks.len() && idx < midi.tracks[act].notes.len() {
                            let zx = *imp.zoom_x.borrow();
                            let zy = *imp.zoom_y.borrow();
                            let tps = (midi.ticks_per_beat as f64) * 2.0;
                            let dt_ticks = ((dx / zx) * tps) as i64;
                            let dpitch = -(dy / zy).round() as i32;

                            let dur = orig.end_tick as i64 - orig.start_tick as i64;
                            let mut ns = orig.start_tick as i64 + dt_ticks;
                            if ns < 0 {
                                ns = 0;
                            }
                            let ns_snapped = snap_tick(ns as u64, midi.ticks_per_beat);
                            let ne = ns_snapped as i64 + dur;

                            let np = (orig.pitch as i32 + dpitch).clamp(0, 127) as u8;

                            let active_opt = *imp.preview_active_pitch.borrow();
                            if let Some(active) = active_opt {
                                if active != np {
                                    if let Some(cb) = &*imp.preview_note_off_callback.borrow() {
                                        cb(active);
                                    }
                                    if let Some(cb) = &*imp.preview_note_on_callback.borrow() {
                                        cb(np, orig.velocity);
                                    }
                                    *imp.preview_active_pitch.borrow_mut() = Some(np);
                                }
                            }

                            let n = &mut midi.tracks[act].notes[idx];
                            n.start_tick = ns_snapped;
                            n.end_tick = ne.max(0) as u64;
                            n.pitch = np;
                            obj_u.queue_draw();
                        }
                    }
                }
            }
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
                        cb(active);
                    }
                    *imp.preview_active_pitch.borrow_mut() = None;
                }
                if let Some(cb) = &*imp.data_changed_callback.borrow() {
                    cb();
                }
            }
        });

        // Scroll: vertical = up/down pitch, horizontal = left/right time
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let obj_s = self.clone();
        scroll.connect_scroll(move |_, dx, dy| {
            let imp = obj_s.imp();
            let zy = *imp.zoom_y.borrow();
            let max_scroll_y = 128.0 * zy - obj_s.height() as f64;

            let mut sy = *imp.scroll_y.borrow();
            sy -= dy * 40.0; // scroll up = show higher pitches
            sy = sy.clamp(0.0, max_scroll_y.max(0.0));
            *imp.scroll_y.borrow_mut() = sy;

            let mut sx = *imp.scroll_x.borrow();
            sx += dx * 50.0;
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

        self.add_controller(right_click);
        self.add_controller(drag);
        self.add_controller(scroll);
        self.add_controller(key_ctrl);
    }

    pub fn set_data(&self, midi: MidiData) {
        *self.imp().data.borrow_mut() = Some(midi);
        *self.imp().selected_note.borrow_mut() = None;
        self.queue_draw();
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
}
