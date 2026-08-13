//! Rendering functions for the piano roll grid, notes, and playhead.

use super::types::{BEATS_PER_BAR, KEY_WIDTH, SNAP_SUBDIVISIONS, SelectionRect, Theme};
use super::viewport::Viewport;
use crate::midi::MidiData;
use gtk::graphene;
use gtk::prelude::*;
use gtk4 as gtk;
use std::collections::HashSet;

/// Render horizontal pitch grid lines across the roll area.
pub fn render_pitch_lines(snapshot: &gtk::Snapshot, vp: &Viewport, theme: &Theme) {
    let kw = KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    let zy = vp.zoom_y as f32;

    for pitch in 0u8..128 {
        let y = vp.pitch_to_y(pitch) as f32;
        if y > -zy && y < height + zy {
            snapshot.append_color(
                &theme.grid_line,
                &graphene::Rect::new(kw, y, width - kw, 1.0),
            );
        }
    }
}

/// Render vertical beat / bar / subdivision grid lines.
pub fn render_beat_grid(snapshot: &gtk::Snapshot, vp: &Viewport, midi: &MidiData, theme: &Theme) {
    let kw = KEY_WIDTH as f32;
    let height = vp.height as f32;
    let width = vp.width as f32;

    let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
    let tpb = midi.ticks_per_beat as f64;
    let bar_ticks = tpb * BEATS_PER_BAR as f64;
    let sub_ticks = tpb / SNAP_SUBDIVISIONS as f64;

    let (tick_start, tick_end) = vp.visible_tick_range(tps);

    let sub_grid = sub_ticks as u64;
    if sub_grid == 0 {
        return;
    }

    let first = (tick_start / sub_grid) * sub_grid;
    let mut tick = first;
    while tick <= tick_end + sub_grid {
        let x = vp.tick_to_x(tick, tps) as f32;
        if x >= kw && x <= width {
            let is_bar = ((tick as f64) % bar_ticks).abs() < 0.5;
            let is_beat = ((tick as f64) % tpb).abs() < 0.5;
            let (color, w) = if is_bar {
                (&theme.bar_line, theme.bar_line_width)
            } else if is_beat {
                (&theme.beat_line, theme.beat_line_width)
            } else {
                (&theme.sub_beat_line, theme.sub_beat_line_width)
            };
            snapshot.append_color(color, &graphene::Rect::new(x, 0.0, w, height));
        }
        tick += sub_grid;
    }
}

/// Render note rectangles for every track (inactive tracks are dimmed).
pub fn render_notes(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    midi: &MidiData,
    active_track: usize,
    selected_notes: &HashSet<usize>,
    theme: &Theme,
) {
    let kw = KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());

    if let Some(track) = midi.tracks.get(active_track) {
        for (n_idx, note) in track.notes.iter().enumerate() {
            let (x, y, w, h) = vp.note_rect(note, tps);
            let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);

            if x + w > kw && x < width && y + h > 0.0 && y < height {
                // Fill
                let note_color = if selected_notes.contains(&n_idx) {
                    &theme.note_selected
                } else {
                    &theme.note_active
                };
                snapshot.append_color(note_color, &graphene::Rect::new(x, y, w, h));

                // Border
                let bc = &theme.note_border_active;
                snapshot.append_color(bc, &graphene::Rect::new(x, y, w, 1.0));
                snapshot.append_color(bc, &graphene::Rect::new(x, y + h - 1.0, w, 1.0));
                snapshot.append_color(bc, &graphene::Rect::new(x, y, 1.0, h));
                snapshot.append_color(bc, &graphene::Rect::new(x + w - 1.0, y, 1.0, h));
            }
        }
    }
}

/// Render the playhead (vertical red line).
pub fn render_playhead(snapshot: &gtk::Snapshot, vp: &Viewport, time: f64, theme: &Theme) {
    let kw = KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;

    let p_x = vp.time_to_x(time) as f32;
    if p_x >= kw - 2.0 && p_x <= width + 2.0 {
        snapshot.append_color(
            &theme.playhead,
            &graphene::Rect::new(p_x - 1.0, 0.0, 3.0, height),
        );
    }
}

/// Render the rubber-band selection rectangle.
pub fn render_selection_rect(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    sel: &SelectionRect,
    theme: &Theme,
) {
    let kw = KEY_WIDTH as f32;

    // Convert absolute X to screen X
    let screen_x0 = (sel.abs_x0 - vp.scroll_x + KEY_WIDTH) as f32;
    let screen_x1 = (sel.abs_x1 - vp.scroll_x + KEY_WIDTH) as f32;
    let (sx0, sx1) = if screen_x0 < screen_x1 {
        (screen_x0, screen_x1)
    } else {
        (screen_x1, screen_x0)
    };

    // Convert pitch to screen Y
    let y_top = vp.pitch_to_y(sel.pitch_hi) as f32 - vp.zoom_y as f32;
    let y_bot = vp.pitch_to_y(sel.pitch_lo) as f32;

    let rx = sx0.max(kw);
    let ry = y_top;
    let rw = sx1 - sx0;
    let rh = y_bot - y_top;

    if rw > 0.0 && rh > 0.0 {
        // Fill
        snapshot.append_color(
            &theme.selection_rect_fill,
            &graphene::Rect::new(rx, ry, rw, rh),
        );
        // Border (4 edges)
        let bc = &theme.selection_rect_border;
        snapshot.append_color(bc, &graphene::Rect::new(rx, ry, rw, 1.0));
        snapshot.append_color(bc, &graphene::Rect::new(rx, ry + rh - 1.0, rw, 1.0));
        snapshot.append_color(bc, &graphene::Rect::new(rx, ry, 1.0, rh));
        snapshot.append_color(bc, &graphene::Rect::new(rx + rw - 1.0, ry, 1.0, rh));
    }
}

