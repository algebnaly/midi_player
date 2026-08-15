//! Piano keyboard strip rendering (left edge of the roll).

use super::types::{KEY_WIDTH, Theme};
use super::viewport::Viewport;
use gtk::graphene;
use gtk::prelude::*;
use gtk4 as gtk;
use std::collections::HashSet;

const WHITE_KEY_OFFSETS: [(f32, f32); 7] = [
    (0.0, 5.0 / 3.0),
    (5.0 / 3.0, 10.0 / 3.0),
    (10.0 / 3.0, 5.0),
    (5.0, 5.0 + 1.75),
    (5.0 + 1.75, 5.0 + 3.5),
    (5.0 + 3.5, 5.0 + 5.25),
    (5.0 + 5.25, 12.0),
];

pub fn render_keyboard(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    pango_ctx: &gtk::pango::Context,
    active_pitches: &HashSet<u8>,
    theme: &Theme,
) {
    let kw = KEY_WIDTH as f32;
    let height = vp.height as f32;
    let zy = vp.zoom_y as f32;
    let font_desc = gtk::pango::FontDescription::from_string("Sans 11");

    for pitch in 0u8..128 {
        if is_black_key(pitch) {
            continue;
        }

        let wk_idx = white_key_index(pitch);
        let (bottom_rel, top_rel) = WHITE_KEY_OFFSETS[wk_idx];
        let octave = pitch / 12;
        let octave_bottom_y = vp.pitch_to_y(octave * 12) as f32;
        let bottom_y = octave_bottom_y - bottom_rel * zy;
        let top_y = octave_bottom_y - top_rel * zy;
        let h = bottom_y - top_y;

        if bottom_y < -zy || top_y > height + zy {
            continue;
        }

        let is_active = active_pitches.contains(&pitch);
        let color = if is_active {
            &theme.active_white_key
        } else {
            &theme.white_key
        };

        snapshot.append_color(color, &graphene::Rect::new(0.0, top_y, kw, h));
        snapshot.append_color(
            &theme.key_border,
            &graphene::Rect::new(0.0, bottom_y, kw, 1.0),
        );

        if pitch % 12 == 0 {
            let layout = gtk::pango::Layout::new(pango_ctx);
            layout.set_font_description(Some(&font_desc));
            let oct_num = pitch as i32 / 12 - 1;
            layout.set_text(&format!("C{oct_num}"));
            snapshot.save();
            snapshot.translate(&graphene::Point::new(4.0, bottom_y - zy / 2.0 - 6.0));
            snapshot.append_layout(&layout, &theme.key_text);
            snapshot.restore();
        }
    }

    for pitch in 0u8..128 {
        if !is_black_key(pitch) {
            continue;
        }

        let y = vp.pitch_to_y(pitch) as f32;
        if y < -zy || y > height + zy {
            continue;
        }

        let is_active = active_pitches.contains(&pitch);
        let color = if is_active {
            &theme.active_black_key
        } else {
            &theme.black_key
        };
        let kw_actual = kw * 0.6;

        snapshot.append_color(color, &graphene::Rect::new(0.0, y - zy, kw_actual, zy));
        snapshot.append_color(
            &theme.key_border,
            &graphene::Rect::new(0.0, y - zy, kw_actual, 1.0),
        );
        snapshot.append_color(
            &theme.key_border,
            &graphene::Rect::new(0.0, y, kw_actual, 1.0),
        );
        snapshot.append_color(
            &theme.key_border,
            &graphene::Rect::new(kw_actual, y - zy, 1.0, zy),
        );
    }
}

#[inline]
fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

#[inline]
fn white_key_index(pitch: u8) -> usize {
    match pitch % 12 {
        0 => 0,
        2 => 1,
        4 => 2,
        5 => 3,
        7 => 4,
        9 => 5,
        11 => 6,
        _ => unreachable!(),
    }
}
