//! Rendering functions for the piano roll grid and notes.

use super::types::Theme;
use super::viewport::Viewport;
use crate::midi::MidiData;
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
            snapshot.append_color(
                &theme.grid_line,
                &graphene::Rect::new(kw, y, width - kw, 1.0),
            );
        }
    }
}

/// Render note rectangles for the active track.
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

    if let Some(track) = midi.tracks.get(active_track) {
        for (n_idx, note) in track.notes.iter().enumerate() {
            let (x, y, w, h) = vp.note_rect(note, tps);
            let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);

            if x + w > kw && x < width && y + h > 0.0 && y < height {
                let note_color = if selected_notes.contains(&n_idx) {
                    &theme.note_selected
                } else {
                    &theme.note_active
                };
                snapshot.append_color(note_color, &graphene::Rect::new(x, y, w, h));

                let bc = &theme.note_border_active;
                snapshot.append_color(bc, &graphene::Rect::new(x, y, w, 1.0));
                snapshot.append_color(bc, &graphene::Rect::new(x, y + h - 1.0, w, 1.0));
                snapshot.append_color(bc, &graphene::Rect::new(x, y, 1.0, h));
                snapshot.append_color(bc, &graphene::Rect::new(x + w - 1.0, y, 1.0, h));
            }
        }
    }
}
