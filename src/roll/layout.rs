//! Per-view vertical layout. Piano and drum rolls keep separate Views and
//! implement this trait for Y-axis, channel, hit-testing, and note placement.

use super::types::snap_tick_to_beat;
use super::viewport::Viewport;
use crate::drum_map::DrumMap;
use crate::midi::{MidiData, TrackMode};

fn track_drum_map(midi: Option<&MidiData>, track: usize) -> Option<&DrumMap> {
    midi.and_then(|midi| midi.tracks.get(track))
        .and_then(|track| match &track.mode {
            TrackMode::Drum(dm) => Some(dm),
            TrackMode::Melodic => None,
        })
}

/// Geometry and editing policy that differs between roll Views.
pub trait RollLayout {
    fn note_channel() -> u8;

    fn y_to_pitch(vp: &Viewport, y: f64, midi: Option<&MidiData>, track: usize) -> Option<u8>;

    fn y_to_lane(vp: &Viewport, y: f64, midi: Option<&MidiData>, track: usize) -> u8;

    fn note_in_lanes(
        note_pitch: u8,
        lane_lo: u8,
        lane_hi: u8,
        midi: Option<&MidiData>,
        track: usize,
    ) -> bool;

    fn pitch_after_lane_delta(
        orig_pitch: u8,
        start_lane: u8,
        current_lane: u8,
        midi: &MidiData,
        track: usize,
    ) -> u8;

    fn place_new_note(raw_tick: u64, ticks_per_beat: u16, default_note_beats: f64) -> (u64, u64);

    fn hit_width(note_px: f64, zoom_y: f64) -> f64;

    fn lane_count(midi: Option<&MidiData>, track: usize) -> f64;

    fn selection_y_range(
        vp: &Viewport,
        lane_lo: u8,
        lane_hi: u8,
        midi: Option<&MidiData>,
        track: usize,
    ) -> (f64, f64);
}

pub struct MelodicLayout;

impl RollLayout for MelodicLayout {
    fn note_channel() -> u8 {
        0
    }

    fn y_to_pitch(vp: &Viewport, y: f64, _midi: Option<&MidiData>, _track: usize) -> Option<u8> {
        Some(vp.y_to_pitch(y))
    }

    fn y_to_lane(vp: &Viewport, y: f64, _midi: Option<&MidiData>, _track: usize) -> u8 {
        vp.y_to_pitch(y)
    }

    fn note_in_lanes(
        note_pitch: u8,
        lane_lo: u8,
        lane_hi: u8,
        _midi: Option<&MidiData>,
        _track: usize,
    ) -> bool {
        note_pitch >= lane_lo && note_pitch <= lane_hi
    }

    fn pitch_after_lane_delta(
        orig_pitch: u8,
        start_lane: u8,
        current_lane: u8,
        _midi: &MidiData,
        _track: usize,
    ) -> u8 {
        (orig_pitch as i32 + (current_lane as i32 - start_lane as i32)).clamp(0, 127) as u8
    }

    fn place_new_note(raw_tick: u64, ticks_per_beat: u16, default_note_beats: f64) -> (u64, u64) {
        let start_tick = snap_tick_to_beat(raw_tick, ticks_per_beat);
        let note_len = (default_note_beats * f64::from(ticks_per_beat)).round() as u64;
        (start_tick, start_tick + note_len.max(1))
    }

    fn hit_width(note_px: f64, _zoom_y: f64) -> f64 {
        note_px
    }

    fn lane_count(_midi: Option<&MidiData>, _track: usize) -> f64 {
        128.0
    }

    fn selection_y_range(
        vp: &Viewport,
        lane_lo: u8,
        lane_hi: u8,
        _midi: Option<&MidiData>,
        _track: usize,
    ) -> (f64, f64) {
        let y_top = vp.pitch_to_y(lane_hi) - vp.zoom_y;
        let y_bot = vp.pitch_to_y(lane_lo);
        (y_top, y_bot)
    }
}

pub struct DrumLayout;

impl RollLayout for DrumLayout {
    fn note_channel() -> u8 {
        9
    }

    fn y_to_pitch(vp: &Viewport, y: f64, midi: Option<&MidiData>, track: usize) -> Option<u8> {
        let dm = track_drum_map(midi, track)?;
        dm.row_to_pitch(vp.y_to_drum_row(y, dm.row_count()))
    }

    fn y_to_lane(vp: &Viewport, y: f64, midi: Option<&MidiData>, track: usize) -> u8 {
        let rows = track_drum_map(midi, track)
            .map(|dm| dm.row_count())
            .unwrap_or(128);
        vp.y_to_drum_row(y, rows) as u8
    }

    fn note_in_lanes(
        note_pitch: u8,
        lane_lo: u8,
        lane_hi: u8,
        midi: Option<&MidiData>,
        track: usize,
    ) -> bool {
        track_drum_map(midi, track)
            .and_then(|dm| dm.pitch_to_row(note_pitch))
            .is_some_and(|row| row >= lane_lo as usize && row <= lane_hi as usize)
    }

    fn pitch_after_lane_delta(
        orig_pitch: u8,
        start_lane: u8,
        current_lane: u8,
        midi: &MidiData,
        track: usize,
    ) -> u8 {
        let Some(dm) = track_drum_map(Some(midi), track) else {
            return orig_pitch;
        };
        let Some(row) = dm.pitch_to_row(orig_pitch) else {
            return orig_pitch;
        };
        let last = dm.row_count().saturating_sub(1) as i32;
        let new_row =
            (row as i32 + current_lane as i32 - start_lane as i32).clamp(0, last) as usize;
        dm.row_to_pitch(new_row).unwrap_or(orig_pitch)
    }

    fn place_new_note(raw_tick: u64, ticks_per_beat: u16, _default_note_beats: f64) -> (u64, u64) {
        let note_len = (f64::from(ticks_per_beat) / 8.0).max(1.0).round() as u64;
        let start_tick = (raw_tick / note_len) * note_len;
        (start_tick, start_tick + note_len)
    }

    fn hit_width(note_px: f64, zoom_y: f64) -> f64 {
        note_px.max((zoom_y * 0.7).max(4.0).min(20.0))
    }

    fn lane_count(midi: Option<&MidiData>, track: usize) -> f64 {
        track_drum_map(midi, track)
            .map(|dm| dm.row_count() as f64)
            .unwrap_or(128.0)
    }

    fn selection_y_range(
        vp: &Viewport,
        lane_lo: u8,
        lane_hi: u8,
        midi: Option<&MidiData>,
        track: usize,
    ) -> (f64, f64) {
        let total = Self::lane_count(midi, track) as usize;
        let y_top = vp.drum_row_to_y(lane_hi as usize, total);
        let y_bot = vp.drum_row_to_y(lane_lo as usize, total) + vp.zoom_y;
        (y_top, y_bot)
    }
}
