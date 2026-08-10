//! Physical MIDI input device discovery and connection management.
//!
//! [`MidiInputManager`] owns the selected `midir` connection. Incoming
//! messages are parsed on the backend's MIDI callback thread and forwarded to
//! the audio callback through a channel. A second channel carries lightweight
//! note-state updates to the GTK main thread for keyboard highlighting.

use crate::midi::TrackId;
use crate::velocity_curve::VelocityCurve;
use anyhow::{Context, Result, anyhow};
use crossbeam_channel::{Receiver, Sender, unbounded};
use midir::{Ignore, MidiInput, MidiInputConnection};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

/// A sounding note routed to a concrete synth instance.
pub type LiveNoteKey = (TrackId, u8, u8); // track, channel, pitch
pub type OutputNoteKey = (usize, u8, u8); // synth, channel, pitch

/// Note event consumed by the real-time audio callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveMidiEvent {
    NoteOn {
        track_id: TrackId,
        synth_index: usize,
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    NoteOff {
        track_id: TrackId,
        synth_index: usize,
        channel: u8,
        pitch: u8,
    },
}

impl LiveMidiEvent {
    pub fn key(self) -> LiveNoteKey {
        match self {
            Self::NoteOn {
                track_id,
                channel,
                pitch,
                ..
            }
            | Self::NoteOff {
                track_id,
                channel,
                pitch,
                ..
            } => (track_id, channel, pitch),
        }
    }

    pub fn output_key(self) -> OutputNoteKey {
        match self {
            Self::NoteOn {
                synth_index,
                channel,
                pitch,
                ..
            }
            | Self::NoteOff {
                synth_index,
                channel,
                pitch,
                ..
            } => (synth_index, channel, pitch),
        }
    }
}

/// Visual note-state update consumed on the GTK main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiUiEvent {
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
    pub active: bool,
    pub occurred_at: Instant,
}

/// Notes held by the connected device and the synth chosen at Note-On time.
type HeldNotes = HashMap<(u8, u8), (TrackId, usize)>;

/// Stable identity and user-facing name of an available MIDI input port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiInputPortInfo {
    pub id: String,
    pub name: String,
}

pub struct MidiInputManager {
    connection: Option<MidiInputConnection<HeldNotes>>,
    target_track: Arc<AtomicU64>,
    target_synth: Arc<AtomicUsize>,
    audio_tx: Sender<LiveMidiEvent>,
    ui_tx: Sender<MidiUiEvent>,
    velocity_curve: VelocityCurve,
}

impl MidiInputManager {
    pub fn new(
        audio_tx: Sender<LiveMidiEvent>,
        velocity_curve: VelocityCurve,
    ) -> (Self, Receiver<MidiUiEvent>) {
        let (ui_tx, ui_rx) = unbounded();
        (
            Self {
                connection: None,
                target_track: Arc::new(AtomicU64::new(0)),
                target_synth: Arc::new(AtomicUsize::new(0)),
                audio_tx,
                ui_tx,
                velocity_curve,
            },
            ui_rx,
        )
    }

    /// Return stable IDs and display names for the current input ports.
    pub fn port_infos() -> Result<Vec<MidiInputPortInfo>> {
        let input =
            MidiInput::new("midi-player-port-list").context("failed to initialise MIDI input")?;
        input
            .ports()
            .iter()
            .map(|port| {
                let name = input
                    .port_name(port)
                    .context("failed to read MIDI port name")?;
                Ok(MidiInputPortInfo {
                    id: port.id(),
                    name,
                })
            })
            .collect()
    }

    pub fn set_target_track(&self, track_id: TrackId, synth_index: usize) {
        self.target_track.store(track_id.0, Ordering::Relaxed);
        self.target_synth.store(synth_index, Ordering::Relaxed);
    }

    /// Connect to the current port with `port_id`, returning its display name.
    pub fn connect(&mut self, port_id: &str) -> Result<String> {
        self.disconnect();

        let mut input =
            MidiInput::new("midi-player-input").context("failed to initialise MIDI input")?;
        input.ignore(Ignore::All);
        let ports = input.ports();
        let port = ports
            .iter()
            .find(|port| port.id() == port_id)
            .cloned()
            .ok_or_else(|| anyhow!("MIDI input port '{port_id}' no longer exists"))?;
        let port_name = input
            .port_name(&port)
            .context("failed to read MIDI port name")?;

        let target_track = self.target_track.clone();
        let target_synth = self.target_synth.clone();
        let audio_tx = self.audio_tx.clone();
        let ui_tx = self.ui_tx.clone();
        let velocity_curve = self.velocity_curve.clone();
        let connection = input
            .connect(
                &port,
                "midi-player-read",
                move |_timestamp, bytes, held_notes| {
                    let current_track = TrackId(target_track.load(Ordering::Relaxed));
                    let current_target = target_synth.load(Ordering::Relaxed);
                    handle_message(
                        bytes,
                        current_track,
                        current_target,
                        held_notes,
                        &audio_tx,
                        &ui_tx,
                        &velocity_curve,
                    );
                },
                HeldNotes::new(),
            )
            .map_err(|err| anyhow!("failed to connect to '{port_name}': {err}"))?;

        self.connection = Some(connection);
        Ok(port_name)
    }

    /// Close the current port and release every note still held by that device.
    pub fn disconnect(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        let (_input, held_notes) = connection.close();
        for ((channel, pitch), (track_id, synth_index)) in held_notes {
            send_event(
                LiveMidiEvent::NoteOff {
                    track_id,
                    synth_index,
                    channel,
                    pitch,
                },
                &self.audio_tx,
                &self.ui_tx,
            );
        }
    }
}

impl Drop for MidiInputManager {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn handle_message(
    bytes: &[u8],
    target_track: TrackId,
    target_synth: usize,
    held_notes: &mut HeldNotes,
    audio_tx: &Sender<LiveMidiEvent>,
    ui_tx: &Sender<MidiUiEvent>,
    velocity_curve: &VelocityCurve,
) {
    let Some(mut message) = parse_note_message(bytes, target_track, target_synth) else {
        return;
    };
    if let LiveMidiEvent::NoteOn { velocity, .. } = &mut message {
        *velocity = velocity_curve.map(*velocity);
    }

    match message {
        LiveMidiEvent::NoteOn {
            track_id,
            synth_index,
            channel,
            pitch,
            ..
        } => {
            // A repeated Note-On without a matching Note-Off must release the
            // old route first, especially if the user changed the target track.
            if let Some((previous_track, previous_synth)) =
                held_notes.insert((channel, pitch), (track_id, synth_index))
            {
                send_event(
                    LiveMidiEvent::NoteOff {
                        track_id: previous_track,
                        synth_index: previous_synth,
                        channel,
                        pitch,
                    },
                    audio_tx,
                    ui_tx,
                );
            }
            send_event(message, audio_tx, ui_tx);
        }
        LiveMidiEvent::NoteOff { channel, pitch, .. } => {
            // Route Note-Off to the synth that received Note-On, even if the
            // selected track changed while the key was held.
            let (track_id, synth_index) = held_notes
                .remove(&(channel, pitch))
                .unwrap_or((target_track, target_synth));
            send_event(
                LiveMidiEvent::NoteOff {
                    track_id,
                    synth_index,
                    channel,
                    pitch,
                },
                audio_tx,
                ui_tx,
            );
        }
    }
}

fn send_event(event: LiveMidiEvent, audio_tx: &Sender<LiveMidiEvent>, ui_tx: &Sender<MidiUiEvent>) {
    let active = matches!(event, LiveMidiEvent::NoteOn { .. });
    let (_, channel, pitch) = event.key();
    let velocity = match event {
        LiveMidiEvent::NoteOn { velocity, .. } => velocity,
        LiveMidiEvent::NoteOff { .. } => 0,
    };
    let _ = audio_tx.send(event);
    let _ = ui_tx.send(MidiUiEvent {
        channel,
        pitch,
        velocity,
        active,
        occurred_at: Instant::now(),
    });
}

fn parse_note_message(
    bytes: &[u8],
    track_id: TrackId,
    synth_index: usize,
) -> Option<LiveMidiEvent> {
    if bytes.len() < 3 {
        return None;
    }
    let channel = bytes[0] & 0x0f;
    let pitch = bytes[1] & 0x7f;
    let velocity = bytes[2] & 0x7f;
    match bytes[0] & 0xf0 {
        0x90 if velocity > 0 => Some(LiveMidiEvent::NoteOn {
            track_id,
            synth_index,
            channel,
            pitch,
            velocity,
        }),
        0x80 | 0x90 => Some(LiveMidiEvent::NoteOff {
            track_id,
            synth_index,
            channel,
            pitch,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_on_with_channel_and_velocity() {
        assert_eq!(
            parse_note_message(&[0x92, 64, 91], TrackId(7), 3),
            Some(LiveMidiEvent::NoteOn {
                track_id: TrackId(7),
                synth_index: 3,
                channel: 2,
                pitch: 64,
                velocity: 91,
            })
        );
    }

    #[test]
    fn treats_zero_velocity_note_on_as_note_off() {
        assert_eq!(
            parse_note_message(&[0x9f, 60, 0], TrackId(7), 1),
            Some(LiveMidiEvent::NoteOff {
                track_id: TrackId(7),
                synth_index: 1,
                channel: 15,
                pitch: 60,
            })
        );
    }

    #[test]
    fn ignores_non_note_and_truncated_messages() {
        assert_eq!(parse_note_message(&[0xb0, 64, 127], TrackId(7), 0), None);
        assert_eq!(parse_note_message(&[0x90, 60], TrackId(7), 0), None);
    }

    #[test]
    fn note_off_returns_to_note_on_synth_after_target_changes() {
        let (audio_tx, audio_rx) = unbounded();
        let (ui_tx, ui_rx) = unbounded();
        let mut held = HeldNotes::new();
        let curve = VelocityCurve::default();

        handle_message(
            &[0x91, 67, 88],
            TrackId(7),
            1,
            &mut held,
            &audio_tx,
            &ui_tx,
            &curve,
        );
        handle_message(
            &[0x81, 67, 0],
            TrackId(9),
            2,
            &mut held,
            &audio_tx,
            &ui_tx,
            &curve,
        );

        assert_eq!(
            audio_rx.try_iter().collect::<Vec<_>>(),
            vec![
                LiveMidiEvent::NoteOn {
                    track_id: TrackId(7),
                    synth_index: 1,
                    channel: 1,
                    pitch: 67,
                    velocity: 88,
                },
                LiveMidiEvent::NoteOff {
                    track_id: TrackId(7),
                    synth_index: 1,
                    channel: 1,
                    pitch: 67,
                },
            ]
        );
        let ui_events = ui_rx.try_iter().collect::<Vec<_>>();
        assert_eq!(ui_events.len(), 2);
        assert_eq!((ui_events[0].channel, ui_events[0].pitch), (1, 67));
        assert_eq!((ui_events[0].velocity, ui_events[0].active), (88, true));
        assert_eq!((ui_events[1].channel, ui_events[1].pitch), (1, 67));
        assert_eq!((ui_events[1].velocity, ui_events[1].active), (0, false));
        assert!(ui_events[1].occurred_at >= ui_events[0].occurred_at);
        assert!(held.is_empty());
    }

    #[test]
    fn applies_velocity_curve_before_audio_and_ui_delivery() {
        let (audio_tx, audio_rx) = unbounded();
        let (ui_tx, ui_rx) = unbounded();
        let mut held = HeldNotes::new();
        let curve = VelocityCurve::default();
        curve.set_points(&[
            crate::velocity_curve::VelocityPoint::new(0.0, 0.0),
            crate::velocity_curve::VelocityPoint::new(0.5, 1.0),
            crate::velocity_curve::VelocityPoint::new(1.0, 1.0),
        ]);

        handle_message(
            &[0x90, 60, 64],
            TrackId(3),
            2,
            &mut held,
            &audio_tx,
            &ui_tx,
            &curve,
        );

        assert!(matches!(
            audio_rx.recv().unwrap(),
            LiveMidiEvent::NoteOn { velocity: 127, .. }
        ));
        assert_eq!(ui_rx.recv().unwrap().velocity, 127);
    }
}
