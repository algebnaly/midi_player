//! Constants, enumerations, and shared types for roll widgets.

use crate::midi::Note;
use gtk::gdk;
use gtk4 as gtk;
use std::collections::HashMap;

// ── Layout constants ───────────────────────────────────────────────────

pub const KEY_WIDTH: f64 = 70.0;
pub const BEATS_PER_BAR: u64 = 4;
/// Snap resolution: 1/8 beat (32nd notes).
pub const SNAP_SUBDIVISIONS: u64 = 8;

// ── Interaction thresholds ─────────────────────────────────────────────

/// Pixels from the playhead line that still count as a "click on playhead".
pub const PLAYHEAD_HIT_RADIUS: f64 = 10.0;
/// Pixels from the right edge of a note that trigger resize mode.
pub const NOTE_EDGE_THRESHOLD: f64 = 8.0;
/// Height (px) of the top region where a click always drags the playhead.
pub const TOP_REGION_HEIGHT: f64 = 20.0;
/// Minimum rendered width of a note so it stays visible at high zoom-out.
pub const MIN_NOTE_WIDTH_PX: f64 = 2.0;

// ── Snap helpers ───────────────────────────────────────────────────────

/// Snap a tick to the nearest subdivision grid point.
pub fn snap_tick(tick: u64, ticks_per_beat: u16) -> u64 {
    let grid = ticks_per_beat as u64 / SNAP_SUBDIVISIONS;
    if grid == 0 {
        return tick;
    }
    ((tick + grid / 2) / grid) * grid
}

/// Snap a tick down to the nearest beat boundary.
pub fn snap_tick_to_beat(tick: u64, ticks_per_beat: u16) -> u64 {
    let align_grid = ticks_per_beat as u64;
    tick / align_grid * align_grid
}

/// Quantize a configured duration to a binary musical note length.
///
/// One beat is a quarter note, so powers of two in beat space represent
/// 64th, 32nd, 16th, 8th, quarter, half and whole notes (and longer). This
/// deliberately excludes dotted values such as three eighth notes.
pub fn quantize_binary_note_length(beats: f64, ticks_per_beat: u16) -> u64 {
    if !beats.is_finite() || beats <= 0.0 {
        return (ticks_per_beat as u64 / 8).max(1);
    }
    let mut quantized_beats = 2.0f64.powi(-4);
    let mut best_distance = (beats - quantized_beats).abs();
    for exponent in -3..=4 {
        let candidate = 2.0f64.powi(exponent);
        let distance = (beats - candidate).abs();
        // Prefer the longer value for an exact midpoint tie.
        if distance <= best_distance {
            quantized_beats = candidate;
            best_distance = distance;
        }
    }
    (quantized_beats * f64::from(ticks_per_beat))
        .round()
        .max(1.0) as u64
}

/// Convert a real key-hold duration to beats at the current tempo and then
/// quantize it to a binary musical note length.
pub fn quantize_held_note_length(elapsed_seconds: f64, bpm: f64, ticks_per_beat: u16) -> u64 {
    let safe_bpm = if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        120.0
    };
    quantize_binary_note_length(elapsed_seconds.max(0.0) * safe_bpm / 60.0, ticks_per_beat)
}

/// Resolve the Put-mode duration policy. Without quantization every note is
/// exactly one quarter note; otherwise held time selects a binary note value.
pub fn put_note_length(
    quantization_enabled: bool,
    elapsed_seconds: f64,
    bpm: f64,
    ticks_per_beat: u16,
) -> u64 {
    if quantization_enabled {
        quantize_held_note_length(elapsed_seconds, bpm, ticks_per_beat)
    } else {
        u64::from(ticks_per_beat).max(1)
    }
}

// ── Edit mode ──────────────────────────────────────────────────────────

/// Top-level interaction mode for a roll widget.
#[derive(Default, Debug, PartialEq, Copy, Clone)]
pub enum EditMode {
    #[default]
    Draw,
    Select,
    Put,
}

impl EditMode {
    pub fn label(self) -> &'static str {
        match self {
            EditMode::Draw => "Normal",
            EditMode::Select => "Select",
            EditMode::Put => "Put",
        }
    }

    /// Put mode retains every Normal editing interaction and only adds
    /// physical-MIDI note capture.
    pub fn supports_normal_editing(self) -> bool {
        matches!(self, EditMode::Draw | EditMode::Put)
    }
}

// ── Drag mode ──────────────────────────────────────────────────────────

#[derive(Default, Debug, PartialEq, Copy, Clone)]
pub enum DragMode {
    #[default]
    None,
    MoveNote,
    ResizeDuration,
    /// Rubber-band box selection.
    BoxSelect,
    /// Bulk-dragging all selected notes.
    BulkMove,
}

// ── Selection rectangle ───────────────────────────────────────────────

/// Screen-space rectangle for the rubber-band selection overlay.
///
/// Vertical bounds are in **lanes**: MIDI pitch for the piano roll, drum-map
/// row index for the drum roll.
#[derive(Debug, Clone, Copy)]
pub struct SelectionRect {
    /// Absolute X in pixels (time=0 → 0px, no KEY_WIDTH, no scroll).
    pub abs_x0: f64,
    pub abs_x1: f64,
    pub lane_lo: u8,
    pub lane_hi: u8,
}

// ── Drag state ─────────────────────────────────────────────────────────

/// All mutable state associated with an in-progress drag gesture.
#[derive(Default, Debug)]
pub struct DragState {
    pub mode: DragMode,
    pub is_dragging_playhead: bool,
    pub start_x: f64,
    pub start_y: f64,
    /// Vertical lane at drag start (pitch or drum row).
    pub start_lane: u8,
    /// The absolute tick value the cursor was at when the drag started.
    pub start_cursor_tick: f64,
    /// Last dx from GestureDrag (for scroll-during-drag re-sync).
    pub last_dx: f64,
    /// Last dy from GestureDrag.
    pub last_dy: f64,
    /// Snapshot of the note at drag-begin, used as a reference for moves.
    pub orig_note: Option<Note>,
    /// Snapshots of all selected notes at bulk-drag begin (index → note).
    pub orig_notes: HashMap<usize, Note>,
    /// Pre-existing selection at box-select begin (for Shift+select append).
    pub base_selection: std::collections::HashSet<usize>,
}

// ── Hit-test result ────────────────────────────────────────────────────

/// Result of a spatial query: which note was hit and how.
pub struct HitTestResult {
    pub note_index: usize,
    pub drag_mode: DragMode,
    pub synth_index: usize,
}

// ── Color theme ────────────────────────────────────────────────────────

/// Named color palette for roll widgets.
pub struct Theme {
    pub background: gdk::RGBA,
    pub grid_line: gdk::RGBA,
    pub bar_line: gdk::RGBA,
    pub bar_line_width: f32,
    pub beat_line: gdk::RGBA,
    pub beat_line_width: f32,
    pub sub_beat_line: gdk::RGBA,
    pub sub_beat_line_width: f32,
    pub note_active: gdk::RGBA,
    pub note_selected: gdk::RGBA,
    pub note_inactive: gdk::RGBA,
    pub note_border_active: gdk::RGBA,
    pub note_border_inactive: gdk::RGBA,
    pub playhead: gdk::RGBA,
    pub white_key: gdk::RGBA,
    pub black_key: gdk::RGBA,
    pub active_white_key: gdk::RGBA,
    pub active_black_key: gdk::RGBA,
    pub key_border: gdk::RGBA,
    pub key_text: gdk::RGBA,
    pub selection_rect_fill: gdk::RGBA,
    pub selection_rect_border: gdk::RGBA,
}

/// Create the default dark theme matching the original hard-coded colors.
pub fn default_theme() -> Theme {
    Theme {
        background: gdk::RGBA::new(0.1, 0.1, 0.1, 1.0),
        grid_line: gdk::RGBA::new(0.2, 0.2, 0.2, 1.0),
        bar_line: gdk::RGBA::new(0.5, 0.5, 0.5, 0.8),
        bar_line_width: 2.0,
        beat_line: gdk::RGBA::new(0.35, 0.35, 0.35, 0.6),
        beat_line_width: 1.0,
        sub_beat_line: gdk::RGBA::new(0.2, 0.2, 0.2, 0.4),
        sub_beat_line_width: 1.0,
        note_active: gdk::RGBA::new(0.2, 0.6, 1.0, 1.0),
        note_selected: gdk::RGBA::new(1.0, 0.8, 0.2, 1.0),
        note_inactive: gdk::RGBA::new(0.5, 0.5, 0.5, 0.4),
        note_border_active: gdk::RGBA::new(1.0, 1.0, 1.0, 0.5),
        note_border_inactive: gdk::RGBA::new(1.0, 1.0, 1.0, 0.1),
        playhead: gdk::RGBA::new(1.0, 0.2, 0.2, 1.0),
        white_key: gdk::RGBA::new(0.9, 0.9, 0.9, 1.0),
        black_key: gdk::RGBA::new(0.05, 0.05, 0.05, 1.0),
        active_white_key: gdk::RGBA::new(1.0, 0.8, 0.2, 1.0),
        active_black_key: gdk::RGBA::new(0.8, 0.6, 0.1, 1.0),
        key_border: gdk::RGBA::new(0.0, 0.0, 0.0, 1.0),
        key_text: gdk::RGBA::new(0.2, 0.2, 0.2, 1.0),
        selection_rect_fill: gdk::RGBA::new(0.3, 0.5, 1.0, 0.15),
        selection_rect_border: gdk::RGBA::new(0.4, 0.6, 1.0, 0.6),
    }
}

pub fn note_name(pitch: i16) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let pitch = pitch.clamp(0, 127);
    let octave = pitch / 12 - 1;
    format!("{}{}", NAMES[pitch as usize % 12], octave)
}

pub fn has_exact_note(
    notes: &[Note],
    excluded_index: Option<usize>,
    channel: u8,
    pitch: u8,
    start_tick: u64,
    end_tick: u64,
) -> bool {
    notes.iter().enumerate().any(|(index, note)| {
        Some(index) != excluded_index
            && note.channel == channel
            && note.pitch == pitch
            && note.start_tick == start_tick
            && note.end_tick == end_tick
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_tick_rounds_to_nearest_grid() {
        assert_eq!(snap_tick(0, 480), 0);
        assert_eq!(snap_tick(29, 480), 0);
        assert_eq!(snap_tick(30, 480), 60);
        assert_eq!(snap_tick(60, 480), 60);
        assert_eq!(snap_tick(89, 480), 60);
        assert_eq!(snap_tick(90, 480), 120);
    }

    #[test]
    fn snap_tick_to_beat_floors() {
        assert_eq!(snap_tick_to_beat(0, 480), 0);
        assert_eq!(snap_tick_to_beat(479, 480), 0);
        assert_eq!(snap_tick_to_beat(480, 480), 480);
        assert_eq!(snap_tick_to_beat(960, 480), 960);
        assert_eq!(snap_tick_to_beat(1000, 480), 960);
    }

    #[test]
    fn binary_note_length_excludes_dotted_values() {
        assert_eq!(quantize_binary_note_length(0.125, 480), 60);
        assert_eq!(quantize_binary_note_length(0.25, 480), 120);
        assert_eq!(quantize_binary_note_length(0.5, 480), 240);
        assert_eq!(quantize_binary_note_length(1.0, 480), 480);
        assert_eq!(quantize_binary_note_length(1.5, 480), 960);
        assert_eq!(quantize_binary_note_length(3.0, 480), 1920);
    }

    #[test]
    fn held_time_selects_different_note_lengths() {
        assert_eq!(quantize_held_note_length(0.0625, 120.0, 480), 60);
        assert_eq!(quantize_held_note_length(0.125, 120.0, 480), 120);
        assert_eq!(quantize_held_note_length(0.25, 120.0, 480), 240);
        assert_eq!(quantize_held_note_length(0.5, 120.0, 480), 480);
        assert_eq!(quantize_held_note_length(0.75, 120.0, 480), 960);
    }

    #[test]
    fn put_length_is_quarter_note_when_quantization_is_off() {
        assert_eq!(put_note_length(false, 0.05, 120.0, 480), 480);
        assert_eq!(put_note_length(false, 2.0, 120.0, 480), 480);
        assert_eq!(put_note_length(true, 0.125, 120.0, 480), 120);
    }

    #[test]
    fn put_mode_retains_normal_editing_capabilities() {
        assert!(EditMode::Draw.supports_normal_editing());
        assert!(EditMode::Put.supports_normal_editing());
        assert!(!EditMode::Select.supports_normal_editing());
    }

    #[test]
    fn note_name_formats_midi_octaves() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(48), "C3");
        assert_eq!(note_name(72), "C5");
    }

    #[test]
    fn exact_note_dedup_preserves_chords_and_different_lengths() {
        let notes = vec![
            Note {
                pitch: 60,
                velocity: 100,
                start_tick: 480,
                end_tick: 960,
                channel: 0,
            },
            Note {
                pitch: 64,
                velocity: 100,
                start_tick: 480,
                end_tick: 960,
                channel: 0,
            },
        ];

        assert!(has_exact_note(&notes, None, 0, 60, 480, 960));
        assert!(!has_exact_note(&notes, None, 0, 60, 480, 720));
        assert!(!has_exact_note(&notes, None, 0, 67, 480, 960));
        assert!(!has_exact_note(&notes, Some(0), 0, 60, 480, 960));
    }
}
