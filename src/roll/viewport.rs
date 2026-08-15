//! Viewport coordinate system shared by piano and drum rolls.
//!
//! Horizontal (tick / time) conversions are identical. Vertical conversions
//! are provided for both MIDI pitch and drum-map rows; each View picks one.

use super::types::{KEY_WIDTH, MIN_NOTE_WIDTH_PX};
use crate::midi::Note;

/// Immutable snapshot of the current viewport parameters.
#[derive(Debug, Clone)]
pub struct Viewport {
    /// Pixels per second on the horizontal axis.
    pub zoom_x: f64,
    /// Pixels per vertical lane (semitone or drum row).
    pub zoom_y: f64,
    /// Horizontal scroll offset in pixels.
    pub scroll_x: f64,
    /// Vertical scroll offset in pixels.
    pub scroll_y: f64,
    /// Widget width in pixels.
    pub width: f64,
    /// Widget height in pixels.
    pub height: f64,
}

impl Viewport {
    /// Calculate ticks-per-second from MIDI timing info.
    #[inline]
    pub fn ticks_per_sec(ticks_per_beat: u16, bpm: f64) -> f64 {
        ticks_per_beat as f64 * (bpm / 60.0)
    }

    /// Convert a tick position to a screen X coordinate.
    #[inline]
    pub fn tick_to_x(&self, tick: u64, tps: f64) -> f64 {
        let time_sec = tick as f64 / tps;
        time_sec * self.zoom_x - self.scroll_x + KEY_WIDTH
    }

    /// Convert a screen X coordinate to ticks (fractional).
    #[inline]
    pub fn x_to_tick(&self, screen_x: f64, tps: f64) -> f64 {
        let abs_x = screen_x - KEY_WIDTH + self.scroll_x;
        (abs_x / self.zoom_x) * tps
    }

    /// Convert a time value (seconds) to screen X.
    #[inline]
    pub fn time_to_x(&self, time_sec: f64) -> f64 {
        time_sec * self.zoom_x - self.scroll_x + KEY_WIDTH
    }

    /// Convert a MIDI pitch (0–127) to the screen Y of the *bottom* of its
    /// grid row.
    #[inline]
    pub fn pitch_to_y(&self, pitch: u8) -> f64 {
        self.height - (pitch as f64 * self.zoom_y) + self.scroll_y
    }

    /// Convert a screen Y coordinate to a MIDI pitch (0–127).
    #[inline]
    pub fn y_to_pitch(&self, screen_y: f64) -> u8 {
        let pitch_f = (self.height - screen_y + self.scroll_y) / self.zoom_y;
        (pitch_f.floor() as i32).clamp(0, 127) as u8
    }

    /// Screen rectangle `(x, y, width, height)` for a melodic note.
    pub fn note_rect(&self, note: &Note, tps: f64) -> (f64, f64, f64, f64) {
        let x = self.tick_to_x(note.start_tick, tps);
        let y = self.pitch_to_y(note.pitch) - self.zoom_y;
        let dur_sec = (note.end_tick - note.start_tick) as f64 / tps;
        let w = (dur_sec * self.zoom_x).max(MIN_NOTE_WIDTH_PX);
        let h = self.zoom_y - 1.0;
        (x, y, w, h)
    }

    /// Convert a drum row index to the screen Y of the *top* of its grid row.
    /// Row 0 is at the bottom of the grid.
    pub fn drum_row_to_y(&self, row: usize, _total_rows: usize) -> f64 {
        self.height - ((row as f64 + 1.0) * self.zoom_y) + self.scroll_y
    }

    /// Convert a screen Y coordinate to a drum row index.
    #[inline]
    pub fn y_to_drum_row(&self, screen_y: f64, total_rows: usize) -> usize {
        let dist_from_bottom = self.height - screen_y + self.scroll_y;
        let row = (dist_from_bottom / self.zoom_y).floor() as i32;
        row.clamp(0, (total_rows as i32 - 1).max(0)) as usize
    }

    /// Return the tick range currently visible in the viewport.
    pub fn visible_tick_range(&self, tps: f64) -> (u64, u64) {
        let t_start = self.scroll_x / self.zoom_x;
        let t_end = (self.scroll_x + self.width) / self.zoom_x;
        let tick_start = (t_start * tps) as u64;
        let tick_end = (t_end * tps) as u64;
        (tick_start, tick_end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vp() -> Viewport {
        Viewport {
            zoom_x: 100.0,
            zoom_y: 20.0,
            scroll_x: 0.0,
            scroll_y: 1200.0,
            width: 800.0,
            height: 600.0,
        }
    }

    #[test]
    fn ticks_per_sec_120bpm() {
        let tps = Viewport::ticks_per_sec(480, 120.0);
        assert!((tps - 960.0).abs() < 0.001);
    }

    #[test]
    fn tick_to_x_roundtrip() {
        let vp = make_vp();
        let tps = Viewport::ticks_per_sec(480, 120.0);
        let tick = 960u64;
        let x = vp.tick_to_x(tick, tps);
        let recovered = vp.x_to_tick(x, tps);
        assert!(
            (recovered - tick as f64).abs() < 0.001,
            "expected ~{}, got {}",
            tick,
            recovered
        );
    }

    #[test]
    fn pitch_to_y_roundtrip() {
        let vp = make_vp();
        for pitch in 0..=127u8 {
            let y = vp.pitch_to_y(pitch);
            let recovered = vp.y_to_pitch(y - 0.5);
            assert_eq!(recovered, pitch, "pitch roundtrip failed for {pitch}");
        }
    }

    #[test]
    fn drum_row_to_y_roundtrip() {
        let vp = make_vp();
        let total = 47;
        for row in 0..total {
            let y = vp.drum_row_to_y(row, total);
            let recovered = vp.y_to_drum_row(y + 0.5, total);
            assert_eq!(recovered, row, "drum row roundtrip failed for {row}");
        }
    }

    #[test]
    fn visible_tick_range_at_origin() {
        let vp = Viewport {
            zoom_x: 100.0,
            zoom_y: 20.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let tps = Viewport::ticks_per_sec(480, 120.0);
        let (start, end) = vp.visible_tick_range(tps);
        assert_eq!(start, 0);
        assert_eq!(end, 7680);
    }
}
