//! Application configuration loaded from `~/.config/midi_player/config.toml`.
//!
//! If the config file does not exist, a default one is created automatically.
//! All fields are optional in the TOML file — missing fields use built-in defaults.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Default BPM for new projects.
    pub default_bpm: f64,
    /// Default note duration in beats (e.g. 1.0 = quarter note, 0.125 = 32nd).
    pub default_note_beats: f64,
    /// Path to the default SoundFont file.
    pub soundfont_path: String,
    /// Path to the default CLAP plugin file.
    pub clap_plugin_path: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_bpm: 120.0,
            default_note_beats: 1.0,
            soundfont_path: "default.sf2".to_string(),
            clap_plugin_path: "./plugin.clap".to_string(),
        }
    }
}

impl AppConfig {
    /// Returns the config directory: `~/.config/midi_player/`.
    fn config_dir() -> Option<PathBuf> {
        dirs_fallback().map(|base| base.join("midi_player"))
    }

    /// Returns the full path to the config file.
    fn config_path() -> Option<PathBuf> {
        Self::config_dir().map(|dir| dir.join("config.toml"))
    }

    /// Load the config from disk, or create a default config file if none exists.
    pub fn load() -> Self {
        let path = match Self::config_path() {
            Some(p) => p,
            None => {
                eprintln!("Could not determine config directory, using defaults.");
                return Self::default();
            }
        };

        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => match toml::from_str::<AppConfig>(&contents) {
                    Ok(config) => {
                        println!("Loaded config from {}", path.display());
                        return config;
                    }
                    Err(e) => {
                        eprintln!("Failed to parse {}: {}. Using defaults.", path.display(), e);
                        return Self::default();
                    }
                },
                Err(e) => {
                    eprintln!("Failed to read {}: {}. Using defaults.", path.display(), e);
                    return Self::default();
                }
            }
        }

        // File doesn't exist — create it with defaults.
        let config = Self::default();
        if let Some(dir) = Self::config_dir() {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("Could not create config dir {}: {}", dir.display(), e);
                return config;
            }
        }
        match toml::to_string_pretty(&config) {
            Ok(contents) => {
                if let Err(e) = std::fs::write(&path, &contents) {
                    eprintln!(
                        "Could not write default config to {}: {}",
                        path.display(),
                        e
                    );
                } else {
                    println!("Created default config at {}", path.display());
                }
            }
            Err(e) => eprintln!("Could not serialize default config: {}", e),
        }
        config
    }

    /// Convert `default_note_beats` to ticks for a given `ticks_per_beat`.
    #[allow(dead_code)]
    pub fn default_note_ticks(&self, ticks_per_beat: u16) -> u64 {
        ((self.default_note_beats * ticks_per_beat as f64).round() as u64).max(1)
    }
}

/// Get the XDG config home (or fallback to `~/.config`).
fn dirs_fallback() -> Option<PathBuf> {
    // Check $XDG_CONFIG_HOME first
    if let Ok(val) = std::env::var("XDG_CONFIG_HOME") {
        let p = PathBuf::from(val);
        if p.is_absolute() {
            return Some(p);
        }
    }
    // Fallback: ~/.config
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}
