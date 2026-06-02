//! Synthesizer abstraction layer.
//!
//! This module provides a unified interface ([`TrackSynth`]) over heterogeneous
//! audio backends.  Currently two backends are supported:
//!
//! * **SoundFont** – rendered by [`oxisynth`].
//! * **CLAP plugin** – hosted via the [`clack`](https://github.com/prokopyl/clack)
//!   framework through [`ClapPluginWrapper`](crate::clap_host::ClapPluginWrapper).
//!
//! The rest of the engine treats every track as a `TrackSynth` and never needs
//! to know which concrete backend is in use.

use crate::clap_host::ClapPluginWrapper;
use crate::midi::MidiEventType;
use oxisynth::{MidiEvent, Synth};

/// A single synthesizer track in the mixer.
///
/// Each variant wraps a concrete audio backend.  Helper methods on this enum
/// provide a backend-agnostic interface for the sequencer and preview system.
pub enum TrackSynth {
    /// An [`oxisynth`]-based SoundFont synthesizer.
    SoundFont(Synth),
    /// A CLAP plugin instance loaded through [`ClapPluginWrapper`].
    ClapPlugin(ClapPluginWrapper),
}

impl TrackSynth {
    /// Returns a human-readable label for this synth backend.
    pub fn backend_label(&self) -> &'static str {
        match self {
            TrackSynth::SoundFont(_) => "SoundFont",
            TrackSynth::ClapPlugin(_) => "CLAP",
        }
    }

    /// Send a MIDI event (NoteOn / NoteOff) to this synth, dispatching to the
    /// correct backend automatically.
    pub fn send_midi_event(&mut self, channel: u8, event: &MidiEventType) {
        match self {
            TrackSynth::SoundFont(s) => match event {
                MidiEventType::NoteOn { pitch, velocity } => {
                    let _ = s.send_event(MidiEvent::NoteOn {
                        channel,
                        key: *pitch,
                        vel: *velocity,
                    });
                }
                MidiEventType::NoteOff { pitch } => {
                    let _ = s.send_event(MidiEvent::NoteOff {
                        channel,
                        key: *pitch,
                    });
                }
            },
            TrackSynth::ClapPlugin(c) => match event {
                MidiEventType::NoteOn { pitch, velocity } => {
                    c.send_note_on(channel, *pitch, *velocity);
                }
                MidiEventType::NoteOff { pitch } => {
                    c.send_note_off(channel, *pitch);
                }
            },
        }
    }

    /// Silence all currently-sounding notes on every channel.
    ///
    /// For SoundFont this sends `AllNotesOff` on channels 0–15.
    /// For CLAP plugins this sends individual NoteOff only for notes that are
    /// currently tracked as active, avoiding the overhead of 2048 events.
    pub fn all_notes_off(&mut self) {
        match self {
            TrackSynth::SoundFont(s) => {
                for ch in 0..16 {
                    let _ = s.send_event(MidiEvent::AllNotesOff { channel: ch });
                }
            }
            TrackSynth::ClapPlugin(c) => {
                c.send_all_notes_off();
            }
        }
    }

    /// Render audio into separate left/right buffers.
    ///
    /// The caller is responsible for clearing the buffers before calling this if
    /// additive mixing is not desired.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self {
            TrackSynth::SoundFont(s) => {
                s.write((&mut left[..], &mut right[..]));
            }
            TrackSynth::ClapPlugin(c) => {
                c.render_block(left, right);
            }
        }
    }

    /// Update the transport tempo (BPM).  Only relevant for CLAP plugins.
    pub fn set_tempo(&mut self, bpm: f64) {
        if let TrackSynth::ClapPlugin(c) = self {
            c.set_tempo(bpm);
        }
    }

    /// Update the transport playing state.  Only relevant for CLAP plugins.
    #[allow(dead_code)]
    pub fn set_playing(&mut self, playing: bool) {
        if let TrackSynth::ClapPlugin(c) = self {
            c.set_playing(playing);
        }
    }
}
