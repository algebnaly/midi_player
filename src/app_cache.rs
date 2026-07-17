//! Small pieces of disposable application state stored in the OS cache dir.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MIDI_INPUT_CACHE_FILE: &str = "midi_input.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedMidiInput {
    pub port_id: String,
    pub port_name: String,
}

fn midi_input_cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|base| base.join("midi_player").join(MIDI_INPUT_CACHE_FILE))
}

pub fn load_midi_input() -> Result<Option<CachedMidiInput>> {
    let Some(path) = midi_input_cache_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let cached =
        toml::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(cached))
}

pub fn save_midi_input(device: &CachedMidiInput) -> Result<()> {
    let path = midi_input_cache_path().context("could not determine the cache directory")?;
    let parent = path
        .parent()
        .context("MIDI input cache path has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let contents =
        toml::to_string_pretty(device).context("failed to serialize MIDI input cache")?;
    std::fs::write(&path, contents)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub fn clear_midi_input() -> Result<()> {
    let Some(path) = midi_input_cache_path() else {
        return Ok(());
    };
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}
