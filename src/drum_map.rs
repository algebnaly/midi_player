//! Defines the drum map data model for mapping MIDI drum pitches to UI rows.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrumCategory {
    Kick,
    Snare,
    HiHat,
    Tom,
    Cymbal,
    Percussion,
}

impl DrumCategory {
    pub fn color(&self) -> (f32, f32, f32) {
        match self {
            Self::Kick => (0.9, 0.3, 0.3),
            Self::Snare => (1.0, 0.6, 0.2),
            Self::HiHat => (0.6, 0.8, 0.3),
            Self::Tom => (0.3, 0.5, 0.9),
            Self::Cymbal => (0.9, 0.8, 0.3),
            Self::Percussion => (0.7, 0.4, 0.9),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrumMapEntry {
    pub pitch: u8,
    pub name: String,
    pub short_name: String,
    pub category: DrumCategory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrumMap {
    pub entries: Vec<DrumMapEntry>,
}

impl DrumMap {
    pub fn gm_default() -> Self {
        Self {
            entries: vec![
                DrumMapEntry { pitch: 35, name: "Acoustic Bass Drum".into(), short_name: "Kick1".into(), category: DrumCategory::Kick },
                DrumMapEntry { pitch: 36, name: "Bass Drum 1".into(), short_name: "Kick2".into(), category: DrumCategory::Kick },
                DrumMapEntry { pitch: 37, name: "Side Stick".into(), short_name: "Stick".into(), category: DrumCategory::Snare },
                DrumMapEntry { pitch: 38, name: "Acoustic Snare".into(), short_name: "Snr1".into(), category: DrumCategory::Snare },
                DrumMapEntry { pitch: 39, name: "Hand Clap".into(), short_name: "Clap".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 40, name: "Electric Snare".into(), short_name: "Snr2".into(), category: DrumCategory::Snare },
                DrumMapEntry { pitch: 41, name: "Low Floor Tom".into(), short_name: "LFTom".into(), category: DrumCategory::Tom },
                DrumMapEntry { pitch: 42, name: "Closed Hi-Hat".into(), short_name: "HH Cl".into(), category: DrumCategory::HiHat },
                DrumMapEntry { pitch: 43, name: "High Floor Tom".into(), short_name: "HFTom".into(), category: DrumCategory::Tom },
                DrumMapEntry { pitch: 44, name: "Pedal Hi-Hat".into(), short_name: "HH Pd".into(), category: DrumCategory::HiHat },
                DrumMapEntry { pitch: 45, name: "Low Tom".into(), short_name: "LTom".into(), category: DrumCategory::Tom },
                DrumMapEntry { pitch: 46, name: "Open Hi-Hat".into(), short_name: "HH Op".into(), category: DrumCategory::HiHat },
                DrumMapEntry { pitch: 47, name: "Low-Mid Tom".into(), short_name: "LMTom".into(), category: DrumCategory::Tom },
                DrumMapEntry { pitch: 48, name: "Hi-Mid Tom".into(), short_name: "HMTom".into(), category: DrumCategory::Tom },
                DrumMapEntry { pitch: 49, name: "Crash Cymbal 1".into(), short_name: "Crsh1".into(), category: DrumCategory::Cymbal },
                DrumMapEntry { pitch: 50, name: "High Tom".into(), short_name: "HTom".into(), category: DrumCategory::Tom },
                DrumMapEntry { pitch: 51, name: "Ride Cymbal 1".into(), short_name: "Ride1".into(), category: DrumCategory::Cymbal },
                DrumMapEntry { pitch: 52, name: "Chinese Cymbal".into(), short_name: "China".into(), category: DrumCategory::Cymbal },
                DrumMapEntry { pitch: 53, name: "Ride Bell".into(), short_name: "RdBel".into(), category: DrumCategory::Cymbal },
                DrumMapEntry { pitch: 54, name: "Tambourine".into(), short_name: "Tamb".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 55, name: "Splash Cymbal".into(), short_name: "Splsh".into(), category: DrumCategory::Cymbal },
                DrumMapEntry { pitch: 56, name: "Cowbell".into(), short_name: "CwBel".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 57, name: "Crash Cymbal 2".into(), short_name: "Crsh2".into(), category: DrumCategory::Cymbal },
                DrumMapEntry { pitch: 58, name: "Vibraslap".into(), short_name: "VSlap".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 59, name: "Ride Cymbal 2".into(), short_name: "Ride2".into(), category: DrumCategory::Cymbal },
                DrumMapEntry { pitch: 60, name: "Hi Bongo".into(), short_name: "HBngo".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 61, name: "Low Bongo".into(), short_name: "LBngo".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 62, name: "Mute Hi Conga".into(), short_name: "MHCga".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 63, name: "Open Hi Conga".into(), short_name: "OHCga".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 64, name: "Low Conga".into(), short_name: "LCnga".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 65, name: "High Timbale".into(), short_name: "HTmbl".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 66, name: "Low Timbale".into(), short_name: "LTmbl".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 67, name: "High Agogo".into(), short_name: "HAggo".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 68, name: "Low Agogo".into(), short_name: "LAggo".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 69, name: "Cabasa".into(), short_name: "Cabsa".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 70, name: "Maracas".into(), short_name: "Marcs".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 71, name: "Short Whistle".into(), short_name: "SWstl".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 72, name: "Long Whistle".into(), short_name: "LWstl".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 73, name: "Short Guiro".into(), short_name: "SGuro".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 74, name: "Long Guiro".into(), short_name: "LGuro".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 75, name: "Claves".into(), short_name: "Clave".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 76, name: "Hi Wood Block".into(), short_name: "HWBlk".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 77, name: "Low Wood Block".into(), short_name: "LWBlk".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 78, name: "Mute Cuica".into(), short_name: "MCuic".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 79, name: "Open Cuica".into(), short_name: "OCuic".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 80, name: "Mute Triangle".into(), short_name: "MTri".into(), category: DrumCategory::Percussion },
                DrumMapEntry { pitch: 81, name: "Open Triangle".into(), short_name: "OTri".into(), category: DrumCategory::Percussion },
            ],
        }
    }

    pub fn row_count(&self) -> usize {
        self.entries.len()
    }

    pub fn pitch_to_row(&self, pitch: u8) -> Option<usize> {
        self.entries.iter().position(|e| e.pitch == pitch)
    }

    pub fn row_to_pitch(&self, row: usize) -> Option<u8> {
        self.entries.get(row).map(|e| e.pitch)
    }

    pub fn entry_for_pitch(&self, pitch: u8) -> Option<&DrumMapEntry> {
        self.entries.iter().find(|e| e.pitch == pitch)
    }

    pub fn entry_for_row(&self, row: usize) -> Option<&DrumMapEntry> {
        self.entries.get(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gm_default_has_47_entries() {
        let map = DrumMap::gm_default();
        assert_eq!(map.row_count(), 47);
    }

    #[test]
    fn pitch_to_row_roundtrip() {
        let map = DrumMap::gm_default();
        let pitch = 42; // Closed Hi-Hat
        let row = map.pitch_to_row(pitch).expect("Should find pitch 42");
        let roundtrip_pitch = map.row_to_pitch(row).expect("Should find row");
        assert_eq!(pitch, roundtrip_pitch);
    }

    #[test]
    fn bass_drum_is_kick_category() {
        let map = DrumMap::gm_default();
        let entry = map.entry_for_pitch(36).expect("Should find pitch 36");
        assert_eq!(entry.category, DrumCategory::Kick);
    }
}
