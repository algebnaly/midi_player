//! Native project persistence for state that Standard MIDI files cannot keep.

use crate::midi::MidiData;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const PROJECT_EXTENSION: &str = "midiproj";
const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub schema_version: u32,
    pub midi: MidiData,
}

impl ProjectFile {
    pub fn new(midi: MidiData) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            midi,
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read project {}", path.display()))?;
        let project: Self = toml::from_str(&contents)
            .with_context(|| format!("failed to parse project {}", path.display()))?;
        if project.schema_version > CURRENT_SCHEMA_VERSION {
            bail!(
                "project schema {} is newer than supported schema {}",
                project.schema_version,
                CURRENT_SCHEMA_VERSION
            );
        }
        if project.midi.tracks.is_empty() {
            bail!("project contains no MIDI tracks");
        }
        Ok(project)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let contents = toml::to_string_pretty(self).context("failed to serialize project")?;
        std::fs::write(path, contents)
            .with_context(|| format!("failed to write project {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_round_trip_keeps_track_identity_and_mixer_state() {
        let mut midi = MidiData::new_empty(&["Piano".into(), "Strings".into()]);
        midi.tracks[1].mixer.solo = true;
        midi.tracks[1].mixer.pan = 0.4;
        midi.tracks[1].input.armed = true;
        let expected_id = midi.tracks[1].id;

        let encoded = toml::to_string_pretty(&ProjectFile::new(midi)).unwrap();
        let decoded: ProjectFile = toml::from_str(&encoded).unwrap();

        assert_eq!(decoded.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(decoded.midi.tracks[1].id, expected_id);
        assert!(decoded.midi.tracks[1].mixer.solo);
        assert_eq!(decoded.midi.tracks[1].mixer.pan, 0.4);
        assert!(decoded.midi.tracks[1].input.armed);
    }
}
