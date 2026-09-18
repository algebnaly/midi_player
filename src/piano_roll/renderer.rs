//! Rendering functions for the piano roll grid and notes.

use super::types::Theme;
use super::viewport::Viewport;
use crate::midi::{MidiData, TrackMode};
use gtk::graphene;
use gtk::prelude::*;
use gtk4 as gtk;
use std::collections::HashSet;

/// Render horizontal pitch grid lines across the roll area.
pub fn render_pitch_lines(snapshot: &gtk::Snapshot, vp: &Viewport, theme: &Theme) {
    let kw = crate::roll::types::KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    let zy = vp.zoom_y as f32;

    for pitch in 0u8..128 {
        let y = vp.pitch_to_y(pitch) as f32;
        if y > -zy && y < height + zy {
            let is_octave = pitch % 12 == 0;
            let (color, line_width) = if is_octave {
                (&theme.octave_line, theme.octave_line_width)
            } else {
                (&theme.grid_line, 1.0)
            };
            snapshot.append_color(color, &graphene::Rect::new(kw, y, width - kw, line_width));
        }
    }
}

/// Render note rectangles. Other melodic tracks are drawn first as ghosts;
/// the active track is drawn on top.
pub fn render_notes(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    midi: &MidiData,
    active_track: usize,
    selected_notes: &HashSet<usize>,
    theme: &Theme,
) {
    let kw = crate::roll::types::KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());

    for pass_active in [false, true] {
        for (t_idx, track) in midi.tracks.iter().enumerate() {
            if matches!(track.mode, TrackMode::Drum(_)) {
                continue;
            }
            let is_active = t_idx == active_track;
            if is_active != pass_active {
                continue;
            }
            for (n_idx, note) in track.notes.iter().enumerate() {
                let (x, y, w, h) = vp.note_rect(note, tps);
                let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);

                if x + w > kw && x < width && y + h > 0.0 && y < height {
                    let base_color = if is_active {
                        if selected_notes.contains(&n_idx) {
                            &theme.note_selected
                        } else {
                            &theme.note_active
                        }
                    } else {
                        &theme.note_inactive
                    };
                    let note_color = if is_active && !selected_notes.contains(&n_idx) {
                        let vel_scale = 0.65 + 0.5 * (note.velocity as f32 / 127.0);
                        gtk::gdk::RGBA::new(
                            (base_color.red() * vel_scale).clamp(0.0, 1.0),
                            (base_color.green() * vel_scale).clamp(0.0, 1.0),
                            (base_color.blue() * vel_scale).clamp(0.0, 1.0),
                            base_color.alpha(),
                        )
                    } else {
                        *base_color
                    };
                    snapshot.append_color(&note_color, &graphene::Rect::new(x, y, w, h));

                    let bc = if is_active {
                        &theme.note_border_active
                    } else {
                        &theme.note_border_inactive
                    };
                    snapshot.append_color(bc, &graphene::Rect::new(x, y, w, 1.0));
                    snapshot.append_color(bc, &graphene::Rect::new(x, y + h - 1.0, w, 1.0));
                    snapshot.append_color(bc, &graphene::Rect::new(x, y, 1.0, h));
                    snapshot.append_color(bc, &graphene::Rect::new(x + w - 1.0, y, 1.0, h));
                }
            }
        }
    }
}

/// Render the sustain pedal (CC 64) lane at the bottom of the roll.
pub fn render_pedal_lane(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    midi: &MidiData,
    active_track: usize,
    drag_state: &crate::roll::types::DragState,
    theme: &Theme,
) {
    let kw = crate::roll::types::KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    let lane_h = crate::roll::types::PEDAL_LANE_HEIGHT as f32;
    let lane_y = height - lane_h;
    let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());

    // Background of pedal lane
    snapshot.append_color(
        &theme.pedal_lane_bg,
        &graphene::Rect::new(kw, lane_y, width - kw, lane_h),
    );
    // Top border separator line
    snapshot.append_color(
        &theme.pedal_lane_border,
        &graphene::Rect::new(kw, lane_y, width - kw, 1.0),
    );

    if active_track >= midi.tracks.len() {
        return;
    }

    let track = &midi.tracks[active_track];
    let intervals = track.pedal_intervals();
    let bar_y = lane_y + 3.0;
    let bar_h = lane_h - 6.0;

    for interval in &intervals {
        let x0 = vp.tick_to_x(interval.start_tick, tps) as f32;
        let x1 = vp.tick_to_x(interval.end_tick, tps) as f32;
        let w = (x1 - x0).max(crate::roll::types::MIN_NOTE_WIDTH_PX as f32);

        if x0 + w > kw && x0 < width {
            snapshot.append_color(
                &theme.pedal_block,
                &graphene::Rect::new(x0, bar_y, w, bar_h),
            );
            snapshot.append_color(
                &theme.pedal_block_border,
                &graphene::Rect::new(x0, bar_y, w, 1.0),
            );
            snapshot.append_color(
                &theme.pedal_block_border,
                &graphene::Rect::new(x0, bar_y + bar_h - 1.0, w, 1.0),
            );
            snapshot.append_color(
                &theme.pedal_block_border,
                &graphene::Rect::new(x0, bar_y, 1.0, bar_h),
            );
            snapshot.append_color(
                &theme.pedal_block_border,
                &graphene::Rect::new(x0 + w - 1.0, bar_y, 1.0, bar_h),
            );
        }
    }

    // If currently dragging to draw/resize/move pedal, render ghost
    if let Some(orig) = drag_state.orig_pedal {
        let ghost_range = match drag_state.mode {
            crate::roll::types::DragMode::DrawPedal => {
                let cur_tick =
                    vp.x_to_tick(vp.scroll_x + drag_state.start_x + drag_state.last_dx, tps) as u64;
                let cur_snapped = crate::roll::types::snap_tick(cur_tick, midi.ticks_per_beat);
                if cur_snapped >= orig.0 {
                    Some((orig.0, cur_snapped))
                } else {
                    Some((cur_snapped, orig.0))
                }
            }
            crate::roll::types::DragMode::ResizePedal => {
                let cur_tick =
                    vp.x_to_tick(vp.scroll_x + drag_state.start_x + drag_state.last_dx, tps) as u64;
                let min_len =
                    (midi.ticks_per_beat as u64 / crate::roll::types::SNAP_SUBDIVISIONS).max(1);
                let cur_snapped = crate::roll::types::snap_tick(cur_tick, midi.ticks_per_beat)
                    .max(orig.0 + min_len);
                Some((orig.0, cur_snapped))
            }
            crate::roll::types::DragMode::MovePedal => {
                let delta_ticks = ((drag_state.last_dx / vp.zoom_x) * tps).round() as i64;
                let dur = orig.1.saturating_sub(orig.0);
                let new_start = (orig.0 as i64 + delta_ticks).max(0) as u64;
                let snapped_start = crate::roll::types::snap_tick(new_start, midi.ticks_per_beat);
                Some((snapped_start, snapped_start + dur))
            }
            _ => None,
        };

        if let Some((g_start, g_end)) = ghost_range {
            let gx0 = vp.tick_to_x(g_start, tps) as f32;
            let gx1 = vp.tick_to_x(g_end, tps) as f32;
            let gw = (gx1 - gx0).max(crate::roll::types::MIN_NOTE_WIDTH_PX as f32);
            if gx0 + gw > kw && gx0 < width {
                snapshot.append_color(
                    &theme.pedal_block_active,
                    &graphene::Rect::new(gx0, bar_y, gw, bar_h),
                );
                snapshot.append_color(
                    &theme.pedal_block_border,
                    &graphene::Rect::new(gx0, bar_y, gw, 1.0),
                );
                snapshot.append_color(
                    &theme.pedal_block_border,
                    &graphene::Rect::new(gx0, bar_y + bar_h - 1.0, gw, 1.0),
                );
                snapshot.append_color(
                    &theme.pedal_block_border,
                    &graphene::Rect::new(gx0, bar_y, 1.0, bar_h),
                );
                snapshot.append_color(
                    &theme.pedal_block_border,
                    &graphene::Rect::new(gx0 + gw - 1.0, bar_y, 1.0, bar_h),
                );
            }
        }
    }
}

/// Render translucent ghost notes that follow the mouse cursor during paste/drop mode.
pub fn render_ghost_notes(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    midi: &MidiData,
    ghost: &crate::roll::types::GhostNotes,
    cursor_x: f64,
    cursor_y: f64,
    _theme: &Theme,
) {
    let kw = crate::roll::types::KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    if (cursor_x as f32) < kw {
        return;
    }

    let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
    let raw_tick = vp.x_to_tick(cursor_x, tps);
    let target_tick = crate::roll::types::snap_tick(raw_tick.max(0.0) as u64, midi.ticks_per_beat);
    let target_pitch = vp.y_to_pitch(cursor_y).clamp(0, 127) as u8;

    let ghost_fill = gtk::gdk::RGBA::new(0.25, 0.7, 1.0, 0.45);
    let ghost_border = gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 0.85);

    for note in &ghost.notes {
        let delta_tick = note.start_tick as i64 - ghost.anchor_tick as i64;
        let start_tick = (target_tick as i64 + delta_tick).max(0) as u64;
        let dur = note.end_tick.saturating_sub(note.start_tick);
        let delta_pitch = note.pitch as i16 - ghost.anchor_pitch as i16;
        let pitch = (target_pitch as i16 + delta_pitch).clamp(0, 127) as u8;

        let ghost_note = crate::midi::Note {
            pitch,
            velocity: note.velocity,
            start_tick,
            end_tick: start_tick + dur,
            channel: note.channel,
        };

        let (x, y, w, h) = vp.note_rect(&ghost_note, tps);
        let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);

        if x + w > kw && x < width && y + h > 0.0 && y < height {
            snapshot.append_color(&ghost_fill, &graphene::Rect::new(x, y, w, h));
            snapshot.append_color(&ghost_border, &graphene::Rect::new(x, y, w, 1.0));
            snapshot.append_color(&ghost_border, &graphene::Rect::new(x, y + h - 1.0, w, 1.0));
            snapshot.append_color(&ghost_border, &graphene::Rect::new(x, y, 1.0, h));
            snapshot.append_color(&ghost_border, &graphene::Rect::new(x + w - 1.0, y, 1.0, h));
        }
    }
}
