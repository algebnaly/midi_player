//! Rendering functions for the piano roll grid, notes, and playhead.

use super::types::{BEATS_PER_BAR, KEY_WIDTH, SNAP_SUBDIVISIONS, SelectionRect, Theme};
use super::viewport::Viewport;
use crate::drum_map::DrumMap;
use crate::midi::MidiData;
use gtk::gdk;
use gtk::graphene;
use gtk::prelude::*;
use gtk4 as gtk;
use std::collections::HashSet;


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

pub fn render_selection_rect(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    sel: &SelectionRect,
    total_rows: usize,
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

    // Convert drum row to screen Y. row_hi is physically higher on screen (lower Y).
    let y_top = vp.drum_row_to_y(sel.row_hi, total_rows) as f32;
    let y_bot = vp.drum_row_to_y(sel.row_lo, total_rows) as f32 + vp.zoom_y as f32;

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

/// Render horizontal grid lines for drum rows.
pub fn render_drum_grid(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    drum_map: &DrumMap,
    theme: &Theme,
) {
    let kw = KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    let zy = vp.zoom_y as f32;
    let total_rows = drum_map.row_count();

    for row in 0..total_rows {
        let y = vp.drum_row_to_y(row, total_rows) as f32;
        if y > -zy && y < height + zy {
            snapshot.append_color(
                &theme.grid_line,
                &graphene::Rect::new(kw, y + zy, width - kw, 1.0),
            );
        }
    }
}

/// Render drum hits as fixed-width diamonds/circles instead of variable-length bars.
pub fn render_drum_notes(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    midi: &MidiData,
    drum_map: &DrumMap,
    active_track: usize,
    selected_notes: &HashSet<usize>,
    theme: &Theme,
) {
    let kw = KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    let zy = vp.zoom_y as f32;
    let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
    let total_rows = drum_map.row_count();

    // Drum hit size: slightly smaller than the row height
    let hit_size = (zy * 0.7).max(4.0).min(20.0);

    if let Some(track) = midi.tracks.get(active_track) {
        for (n_idx, note) in track.notes.iter().enumerate() {
            let row = match drum_map.pitch_to_row(note.pitch) {
                Some(r) => r,
                None => continue,
            };

            let x = vp.tick_to_x(note.start_tick, tps) as f32;
            let row_top = vp.drum_row_to_y(row, total_rows) as f32;
            let center_y = row_top + zy / 2.0;

            // Skip if off-screen
            if x + hit_size < kw || x > width || center_y + hit_size < 0.0 || center_y - hit_size > height {
                continue;
            }

            // Color based on category and selection state
            let (cr, cg, cb) = drum_map.entry_for_row(row)
                .map(|e| e.category.color())
                .unwrap_or((0.5, 0.5, 0.5));

            let note_color = if selected_notes.contains(&n_idx) {
                theme.note_selected
            } else {
                // Use category color at full brightness
                gdk::RGBA::new(cr, cg, cb, 1.0)
            };

            let nw = ((note.end_tick - note.start_tick) as f64 / tps) * vp.zoom_x;
            let nw_f = nw as f32;

            // Velocity affects height: louder hits are taller
            let vel_scale = 0.5 + 0.5 * (note.velocity as f32 / 127.0);
            let size_y = hit_size * vel_scale;
            let half_y = size_y / 2.0;
            let final_w = nw_f.max(size_y);

            // Draw as a rounded rectangle
            let rect = graphene::Rect::new(x, center_y - half_y, final_w, size_y);
            snapshot.append_color(&note_color, &rect);

            // Border
            let bc = theme.note_border_active;
            snapshot.append_color(&bc, &graphene::Rect::new(x, center_y - half_y, final_w, 1.0));
            snapshot.append_color(&bc, &graphene::Rect::new(x, center_y + half_y - 1.0, final_w, 1.0));
            snapshot.append_color(&bc, &graphene::Rect::new(x, center_y - half_y, 1.0, size_y));
            snapshot.append_color(&bc, &graphene::Rect::new(x + final_w - 1.0, center_y - half_y, 1.0, size_y));
        }
    }
}
