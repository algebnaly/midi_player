//! Rendering functions for the drum roll grid and hits.

use super::types::{KEY_WIDTH, Theme};
use super::viewport::Viewport;
use crate::drum_map::DrumMap;
use crate::midi::MidiData;
use gtk::gdk;
use gtk::graphene;
use gtk::prelude::*;
use gtk4 as gtk;
use std::collections::HashSet;

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

            if x + hit_size < kw
                || x > width
                || center_y + hit_size < 0.0
                || center_y - hit_size > height
            {
                continue;
            }

            let (cr, cg, cb) = drum_map
                .entry_for_row(row)
                .map(|e| e.category.color())
                .unwrap_or((0.5, 0.5, 0.5));

            let note_color = if selected_notes.contains(&n_idx) {
                theme.note_selected
            } else {
                gdk::RGBA::new(cr, cg, cb, 1.0)
            };

            let nw = ((note.end_tick - note.start_tick) as f64 / tps) * vp.zoom_x;
            let nw_f = nw as f32;
            let vel_scale = 0.5 + 0.5 * (note.velocity as f32 / 127.0);
            let size_y = hit_size * vel_scale;
            let half_y = size_y / 2.0;
            let final_w = nw_f.max(size_y);

            let rect = graphene::Rect::new(x, center_y - half_y, final_w, size_y);
            snapshot.append_color(&note_color, &rect);

            let bc = theme.note_border_active;
            snapshot.append_color(
                &bc,
                &graphene::Rect::new(x, center_y - half_y, final_w, 1.0),
            );
            snapshot.append_color(
                &bc,
                &graphene::Rect::new(x, center_y + half_y - 1.0, final_w, 1.0),
            );
            snapshot.append_color(&bc, &graphene::Rect::new(x, center_y - half_y, 1.0, size_y));
            snapshot.append_color(
                &bc,
                &graphene::Rect::new(x + final_w - 1.0, center_y - half_y, 1.0, size_y),
            );
        }
    }
}

pub fn render_ghost_drum_notes(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    midi: &MidiData,
    drum_map: &DrumMap,
    ghost: &crate::roll::types::GhostNotes,
    cursor_x: f64,
    cursor_y: f64,
    _theme: &Theme,
) {
    let kw = KEY_WIDTH as f32;
    let width = vp.width as f32;
    let height = vp.height as f32;
    let zy = vp.zoom_y as f32;
    if (cursor_x as f32) < kw {
        return;
    }

    let tps = Viewport::ticks_per_sec(midi.ticks_per_beat, midi.get_bpm());
    let total_rows = drum_map.row_count();
    let hit_size = (zy * 0.7).max(4.0).min(20.0);

    let raw_tick = vp.x_to_tick(cursor_x, tps);
    let target_tick = crate::roll::types::snap_tick(raw_tick.max(0.0) as u64, midi.ticks_per_beat);
    let target_row = vp.y_to_drum_row(cursor_y, total_rows);
    let target_pitch = drum_map.row_to_pitch(target_row).unwrap_or(36);

    let ghost_fill = gtk::gdk::RGBA::new(0.25, 0.7, 1.0, 0.45);
    let ghost_border = gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 0.85);

    for note in &ghost.notes {
        let delta_tick = note.start_tick as i64 - ghost.anchor_tick as i64;
        let start_tick = (target_tick as i64 + delta_tick).max(0) as u64;
        let dur = note.end_tick.saturating_sub(note.start_tick);
        let delta_pitch = note.pitch as i16 - ghost.anchor_pitch as i16;
        let pitch = (target_pitch as i16 + delta_pitch).clamp(0, 127) as u8;

        let row = match drum_map.pitch_to_row(pitch) {
            Some(r) => r,
            None => continue,
        };

        let x = vp.tick_to_x(start_tick, tps) as f32;
        let row_top = vp.drum_row_to_y(row, total_rows) as f32;
        let center_y = row_top + zy / 2.0;

        if x + hit_size < kw || x > width || center_y + hit_size < 0.0 || center_y - hit_size > height {
            continue;
        }

        let nw = (dur as f64 / tps) * vp.zoom_x;
        let nw_f = nw as f32;
        let size_y = hit_size;
        let half_y = size_y / 2.0;
        let final_w = nw_f.max(size_y);

        let rect = graphene::Rect::new(x, center_y - half_y, final_w, size_y);
        snapshot.append_color(&ghost_fill, &rect);
        snapshot.append_color(&ghost_border, &graphene::Rect::new(x, center_y - half_y, final_w, 1.0));
        snapshot.append_color(&ghost_border, &graphene::Rect::new(x, center_y + half_y - 1.0, final_w, 1.0));
        snapshot.append_color(&ghost_border, &graphene::Rect::new(x, center_y - half_y, 1.0, size_y));
        snapshot.append_color(&ghost_border, &graphene::Rect::new(x + final_w - 1.0, center_y - half_y, 1.0, size_y));
    }
}
