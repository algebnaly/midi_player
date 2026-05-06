use crate::midi::{MidiData, Note};
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

pub struct PianoRollWidget {
    pub widget: gtk::DrawingArea,
    data: Rc<RefCell<Option<MidiData>>>,
    playhead_time: Rc<RefCell<f64>>,
    zoom_x: Rc<RefCell<f64>>,
    zoom_y: Rc<RefCell<f64>>,
    scroll_x: Rc<RefCell<f64>>,
    active_track: Rc<RefCell<usize>>,
    selected_note: Rc<RefCell<Option<usize>>>,
    drag_orig_note: Rc<RefCell<Option<Note>>>,
}

impl PianoRollWidget {
    pub fn new() -> Self {
        let widget = gtk::DrawingArea::new();
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        widget.set_size_request(800, 600);
        widget.set_focusable(true);

        let data: Rc<RefCell<Option<MidiData>>> = Rc::new(RefCell::new(None));
        let playhead_time = Rc::new(RefCell::new(0.0));
        let zoom_x = Rc::new(RefCell::new(100.0)); // px per second
        let zoom_y = Rc::new(RefCell::new(12.0)); // px per pitch
        let scroll_x = Rc::new(RefCell::new(0.0));
        let active_track = Rc::new(RefCell::new(0));
        let selected_note = Rc::new(RefCell::new(None));
        let drag_orig_note = Rc::new(RefCell::new(None));

        let data_clone = data.clone();
        let playhead_clone = playhead_time.clone();
        let zoom_x_clone = zoom_x.clone();
        let zoom_y_clone = zoom_y.clone();
        let scroll_x_clone = scroll_x.clone();
        let active_track_clone = active_track.clone();
        let selected_clone = selected_note.clone();

        widget.set_draw_func(move |_area, cr, width, height| {
            cr.set_source_rgb(0.1, 0.1, 0.1);
            cr.paint().unwrap();

            let zx = *zoom_x_clone.borrow();
            let zy = *zoom_y_clone.borrow();
            let p_time = *playhead_clone.borrow();
            let act_track = *active_track_clone.borrow();
            let sel_note = *selected_clone.borrow();
            let offset_x = *scroll_x_clone.borrow();

            // Draw grid and keys
            cr.set_line_width(1.0);
            for pitch in 0..128 {
                let y = height as f64 - (pitch as f64 * zy);

                let is_black = match pitch % 12 {
                    1 | 3 | 6 | 8 | 10 => true,
                    _ => false,
                };

                if is_black {
                    cr.set_source_rgba(0.05, 0.05, 0.05, 1.0);
                } else {
                    cr.set_source_rgba(0.15, 0.15, 0.15, 1.0);
                }
                cr.rectangle(0.0, y - zy, width as f64, zy);
                cr.fill().unwrap();

                cr.set_source_rgba(0.2, 0.2, 0.2, 1.0);
                cr.move_to(0.0, y);
                cr.line_to(width as f64, y);
                cr.stroke().unwrap();
            }

            // Draw Notes
            if let Some(midi) = &*data_clone.borrow() {
                let ticks_per_sec = (midi.ticks_per_beat as f64) * 2.0;

                for (t_idx, track) in midi.tracks.iter().enumerate() {
                    let is_active = t_idx == act_track;

                    for (n_idx, note) in track.notes.iter().enumerate() {
                        let start_sec = note.start_tick as f64 / ticks_per_sec;
                        let end_sec = note.end_tick as f64 / ticks_per_sec;
                        let dur_sec = end_sec - start_sec;

                        let x = start_sec * zx - offset_x;
                        let y = height as f64 - (note.pitch as f64 * zy) - zy;
                        let w = (dur_sec * zx).max(2.0);
                        let h = zy - 1.0;

                        if x + w > 0.0 && x < width as f64 {
                            if is_active {
                                if sel_note == Some(n_idx) {
                                    cr.set_source_rgba(1.0, 0.8, 0.2, 1.0); // Selected (Yellow/Orange)
                                } else {
                                    cr.set_source_rgba(0.2, 0.6, 1.0, 1.0); // Active (Blue)
                                }
                            } else {
                                cr.set_source_rgba(0.5, 0.5, 0.5, 0.4); // Inactive (Ghosted)
                            }

                            cr.rectangle(x, y, w, h);
                            cr.fill_preserve().unwrap();

                            cr.set_source_rgba(1.0, 1.0, 1.0, if is_active { 1.0 } else { 0.2 });
                            cr.set_line_width(0.5);
                            cr.stroke().unwrap();
                        }
                    }
                }
            }

            // Draw Playhead
            let p_x = p_time * zx - offset_x;
            cr.set_source_rgb(1.0, 0.2, 0.2);
            cr.set_line_width(2.0);
            cr.move_to(p_x, 0.0);
            cr.line_to(p_x, height as f64);
            cr.stroke().unwrap();
        });

        // Event controllers
        let click = gtk::GestureClick::new();
        click.set_button(0); // any button

        let c_data = data.clone();
        let c_zoom_x = zoom_x.clone();
        let c_zoom_y = zoom_y.clone();
        let c_scroll_x = scroll_x.clone();
        let c_act_track = active_track.clone();
        let c_sel_note = selected_note.clone();
        let c_widget = widget.clone();

        click.connect_pressed(move |gesture, n_press, x, y| {
            c_widget.grab_focus();
            let button = gesture.current_button();
            let zx = *c_zoom_x.borrow();
            let zy = *c_zoom_y.borrow();
            let offset_x = *c_scroll_x.borrow();
            let act_track = *c_act_track.borrow();
            let height = c_widget.height() as f64;

            let abs_x = x + offset_x;
            let pitch_f = (height - y) / zy;
            let target_pitch = pitch_f.floor() as u8;

            let mut clicked_note = None;
            let mut ticks_per_sec = 1.0;

            if let Some(midi) = &*c_data.borrow() {
                ticks_per_sec = (midi.ticks_per_beat as f64) * 2.0;
                if act_track < midi.tracks.len() {
                    let track = &midi.tracks[act_track];
                    for (i, note) in track.notes.iter().enumerate() {
                        let nx = (note.start_tick as f64 / ticks_per_sec) * zx;
                        let nw = ((note.end_tick - note.start_tick) as f64 / ticks_per_sec) * zx;
                        if abs_x >= nx && abs_x <= nx + nw && target_pitch == note.pitch {
                            clicked_note = Some(i);
                            break;
                        }
                    }
                }
            }

            if button == 1 {
                // Left click
                if clicked_note.is_none() && n_press == 1 {
                    // Left click empty space to add note
                    if let Some(midi) = &mut *c_data.borrow_mut() {
                        if act_track < midi.tracks.len() {
                            let start_tick = ((abs_x / zx) * ticks_per_sec) as u64;
                            let end_tick = start_tick + (ticks_per_sec * 0.5) as u64; // default 0.5s
                            midi.tracks[act_track].notes.push(Note {
                                pitch: target_pitch,
                                velocity: 100,
                                start_tick,
                                end_tick,
                                channel: 0,
                            });
                            *c_sel_note.borrow_mut() = Some(midi.tracks[act_track].notes.len() - 1);
                            c_widget.queue_draw();
                        }
                    }
                } else if n_press == 1 {
                    *c_sel_note.borrow_mut() = clicked_note;
                    c_widget.queue_draw();
                }
            } else if button == 3 {
                // Right click
                if let Some(idx) = clicked_note {
                    if let Some(midi) = &mut *c_data.borrow_mut() {
                        if act_track < midi.tracks.len() {
                            midi.tracks[act_track].notes.remove(idx);
                            *c_sel_note.borrow_mut() = None;
                            c_widget.queue_draw();
                        }
                    }
                }
            }
        });

        let drag = gtk::GestureDrag::new();
        let d_data = data.clone();
        let d_act_track = active_track.clone();
        let d_sel_note = selected_note.clone();
        let d_drag_orig = drag_orig_note.clone();

        drag.connect_drag_begin(move |_, _, _| {
            let act_track = *d_act_track.borrow();
            let sel_note = *d_sel_note.borrow();
            *d_drag_orig.borrow_mut() = None;

            if let Some(idx) = sel_note {
                if let Some(midi) = &*d_data.borrow() {
                    if act_track < midi.tracks.len() {
                        if idx < midi.tracks[act_track].notes.len() {
                            *d_drag_orig.borrow_mut() =
                                Some(midi.tracks[act_track].notes[idx].clone());
                        }
                    }
                }
            }
        });

        let u_data = data.clone();
        let u_zoom_x = zoom_x.clone();
        let u_zoom_y = zoom_y.clone();
        let u_act_track = active_track.clone();
        let u_sel_note = selected_note.clone();
        let u_drag_orig = drag_orig_note.clone();
        let u_widget = widget.clone();

        drag.connect_drag_update(move |_, offset_x, offset_y| {
            if let Some(orig_note) = &*u_drag_orig.borrow() {
                if let Some(idx) = *u_sel_note.borrow() {
                    if let Some(midi) = &mut *u_data.borrow_mut() {
                        let act_track = *u_act_track.borrow();
                        if act_track < midi.tracks.len() && idx < midi.tracks[act_track].notes.len()
                        {
                            let zx = *u_zoom_x.borrow();
                            let zy = *u_zoom_y.borrow();
                            let ticks_per_sec = (midi.ticks_per_beat as f64) * 2.0;

                            // Calculate delta
                            let dt_sec = offset_x / zx;
                            let dt_ticks = (dt_sec * ticks_per_sec) as i64;
                            let dpitch = -(offset_y / zy).round() as i32;

                            let mut new_start = orig_note.start_tick as i64 + dt_ticks;
                            let mut new_end = orig_note.end_tick as i64 + dt_ticks;
                            if new_start < 0 {
                                new_end -= new_start;
                                new_start = 0;
                            }

                            let mut new_pitch = orig_note.pitch as i32 + dpitch;
                            new_pitch = new_pitch.clamp(0, 127);

                            let n = &mut midi.tracks[act_track].notes[idx];
                            n.start_tick = new_start as u64;
                            n.end_tick = new_end as u64;
                            n.pitch = new_pitch as u8;

                            u_widget.queue_draw();
                        }
                    }
                }
            }
        });

        // Scroll controller
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let s_scroll_x = scroll_x.clone();
        let s_widget = widget.clone();
        scroll.connect_scroll(move |_, dx, _dy| {
            let mut sx = *s_scroll_x.borrow();
            sx += dx * 50.0; // adjust scroll speed
            if sx < 0.0 {
                sx = 0.0;
            }
            *s_scroll_x.borrow_mut() = sx;
            s_widget.queue_draw();
            glib::Propagation::Stop
        });

        // Key controller for Delete
        let key_ctrl = gtk::EventControllerKey::new();
        let k_data = data.clone();
        let k_act_track = active_track.clone();
        let k_sel_note = selected_note.clone();
        let k_widget = widget.clone();
        key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Delete || keyval == gtk::gdk::Key::BackSpace {
                if let Some(idx) = *k_sel_note.borrow() {
                    if let Some(midi) = &mut *k_data.borrow_mut() {
                        let act_track = *k_act_track.borrow();
                        if act_track < midi.tracks.len() {
                            midi.tracks[act_track].notes.remove(idx);
                            *k_sel_note.borrow_mut() = None;
                            k_widget.queue_draw();
                            return glib::Propagation::Stop;
                        }
                    }
                }
            }
            glib::Propagation::Proceed
        });

        widget.add_controller(click);
        widget.add_controller(drag);
        widget.add_controller(scroll);
        widget.add_controller(key_ctrl);

        Self {
            widget,
            data,
            playhead_time,
            zoom_x,
            zoom_y,
            scroll_x,
            active_track,
            selected_note,
            drag_orig_note,
        }
    }

    pub fn set_data(&self, midi: MidiData) {
        *self.data.borrow_mut() = Some(midi);
        *self.selected_note.borrow_mut() = None;
        self.widget.queue_draw();
    }

    pub fn set_playhead(&self, time: f64) {
        *self.playhead_time.borrow_mut() = time;

        let zx = *self.zoom_x.borrow();
        let p_x = time * zx;
        let mut sx = *self.scroll_x.borrow();
        let width = self.widget.width() as f64;

        if p_x > sx + width * 0.9 {
            sx = p_x - width * 0.1;
        } else if p_x < sx {
            sx = p_x - width * 0.1;
        }
        if sx < 0.0 {
            sx = 0.0;
        }

        *self.scroll_x.borrow_mut() = sx;
        self.widget.queue_draw();
    }

    pub fn set_active_track(&self, track_idx: usize) {
        *self.active_track.borrow_mut() = track_idx;
        *self.selected_note.borrow_mut() = None;
        self.widget.queue_draw();
    }

    pub fn get_data_clone(&self) -> Option<MidiData> {
        self.data.borrow().clone()
    }
}
