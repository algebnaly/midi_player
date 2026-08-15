//! Shared roll rendering: beat grid, playhead, and selection rectangle.

use super::layout::RollLayout;
use super::types::{BEATS_PER_BAR, KEY_WIDTH, SNAP_SUBDIVISIONS, SelectionRect, Theme};
use super::viewport::Viewport;
use crate::midi::MidiData;
use gtk::graphene;
use gtk::prelude::*;
use gtk4 as gtk;

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

pub fn render_selection_rect<L: RollLayout>(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    sel: &SelectionRect,
    midi: Option<&MidiData>,
    track: usize,
    theme: &Theme,
) {
    let kw = KEY_WIDTH as f32;

    let screen_x0 = (sel.abs_x0 - vp.scroll_x + KEY_WIDTH) as f32;
    let screen_x1 = (sel.abs_x1 - vp.scroll_x + KEY_WIDTH) as f32;
    let (sx0, sx1) = if screen_x0 < screen_x1 {
        (screen_x0, screen_x1)
    } else {
        (screen_x1, screen_x0)
    };

    let (y_top, y_bot) = L::selection_y_range(vp, sel.lane_lo, sel.lane_hi, midi, track);
    let (y_top, y_bot) = (y_top as f32, y_bot as f32);

    let rx = sx0.max(kw);
    let ry = y_top;
    let rw = sx1 - sx0;
    let rh = y_bot - y_top;

    if rw > 0.0 && rh > 0.0 {
        snapshot.append_color(
            &theme.selection_rect_fill,
            &graphene::Rect::new(rx, ry, rw, rh),
        );
        let bc = &theme.selection_rect_border;
        snapshot.append_color(bc, &graphene::Rect::new(rx, ry, rw, 1.0));
        snapshot.append_color(bc, &graphene::Rect::new(rx, ry + rh - 1.0, rw, 1.0));
        snapshot.append_color(bc, &graphene::Rect::new(rx, ry, 1.0, rh));
        snapshot.append_color(bc, &graphene::Rect::new(rx + rw - 1.0, ry, 1.0, rh));
    }
}

pub fn keyboard_active_pitches(
    preview_active_pitch: Option<u8>,
    typing_pressed_pitches: impl IntoIterator<Item = u8>,
) -> std::collections::HashSet<u8> {
    let mut active_pitches: std::collections::HashSet<u8> =
        typing_pressed_pitches.into_iter().collect();
    if let Some(pitch) = preview_active_pitch {
        active_pitches.insert(pitch);
    }
    active_pitches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_pitch_set_supports_multiple_highlights() {
        let active_pitches = keyboard_active_pitches(Some(64), [60, 67]);
        assert!(active_pitches.contains(&60));
        assert!(active_pitches.contains(&64));
        assert!(active_pitches.contains(&67));
        assert_eq!(active_pitches.len(), 3);
    }
}
