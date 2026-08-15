//! Drum name sidebar rendering (left edge of the roll).

use super::types::{KEY_WIDTH, Theme};
use super::viewport::Viewport;
use gtk::gdk;
use gtk::graphene;
use gtk::prelude::*;
use gtk4 as gtk;
use std::collections::HashSet;

pub fn render_drum_sidebar(
    snapshot: &gtk::Snapshot,
    vp: &Viewport,
    pango_ctx: &gtk::pango::Context,
    drum_map: &crate::drum_map::DrumMap,
    active_pitches: &HashSet<u8>,
    theme: &Theme,
) {
    let kw = KEY_WIDTH as f32;
    let zy = vp.zoom_y as f32;
    let height = vp.height as f32;
    let total_rows = drum_map.row_count();

    snapshot.append_color(
        &gdk::RGBA::new(0.0, 0.0, 0.0, 1.0),
        &graphene::Rect::new(0.0, 0.0, kw, height),
    );

    let font_desc = gtk::pango::FontDescription::from_string("Sans 9");

    for row in 0..total_rows {
        let Some(entry) = drum_map.entry_for_row(row) else {
            continue;
        };

        let top_y = vp.drum_row_to_y(row, total_rows) as f32;
        let bottom_y = top_y + zy;
        if bottom_y < 0.0 || top_y > height {
            continue;
        }

        let (cr, cg, cb) = entry.category.color();
        let is_active = active_pitches.contains(&entry.pitch);
        let bg_color = if is_active {
            gdk::RGBA::new(cr, cg, cb, 0.8)
        } else {
            gdk::RGBA::new(cr * 0.3, cg * 0.3, cb * 0.3, 1.0)
        };

        snapshot.append_color(&bg_color, &graphene::Rect::new(0.0, top_y, kw, zy));
        snapshot.append_color(
            &theme.key_border,
            &graphene::Rect::new(0.0, bottom_y - 1.0, kw, 1.0),
        );

        let layout = gtk::pango::Layout::new(pango_ctx);
        layout.set_font_description(Some(&font_desc));
        layout.set_text(&entry.short_name);
        snapshot.save();
        let text_y = top_y + (zy / 2.0) - 6.0;
        snapshot.translate(&graphene::Point::new(4.0, text_y));
        let text_color = if is_active {
            gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)
        } else {
            gdk::RGBA::new(0.85, 0.85, 0.85, 1.0)
        };
        snapshot.append_layout(&layout, &text_color);
        snapshot.restore();
    }
}
