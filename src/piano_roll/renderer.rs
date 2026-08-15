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
                    let note_color = if is_active {
                        if selected_notes.contains(&n_idx) {
                            &theme.note_selected
                        } else {
                            &theme.note_active
                        }
                    } else {
                        &theme.note_inactive
                    };
                    snapshot.append_color(note_color, &graphene::Rect::new(x, y, w, h));

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
