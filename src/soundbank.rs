use crate::midi::SynthSource;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Soundbank {
    pub name: String,
    pub source: SynthSource,
}

pub struct SoundbankManager {
    pub banks: Vec<Soundbank>,
}

impl SoundbankManager {
    pub fn scan(dirs: &[String]) -> Self {
        let mut banks = Vec::new();
        for dir_path in dirs {
            let expanded_dir_path = if dir_path.starts_with("~/") {
                if let Some(home) = dirs::home_dir() {
                    home.join(&dir_path[2..]).to_string_lossy().to_string()
                } else {
                    dir_path.clone()
                }
            } else {
                dir_path.clone()
            };
            let path = Path::new(&expanded_dir_path);
            if path.exists() && path.is_dir() {
                Self::scan_dir_recursive(path, &mut banks);
            }
        }
        banks.sort_by(|a, b| a.name.cmp(&b.name));
        Self { banks }
    }

    fn scan_dir_recursive(dir: &Path, banks: &mut Vec<Soundbank>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::scan_dir_recursive(&path, banks);
                } else if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        let name = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let path_str = path.to_string_lossy().to_string();
                        match ext.to_lowercase().as_str() {
                            "sf2" => {
                                banks.push(Soundbank {
                                    name: format!("{} [SF2]", name),
                                    source: SynthSource::SoundFont { path: path_str },
                                });
                            }
                            "sfz" => {
                                banks.push(Soundbank {
                                    name: format!("{} [SFZ]", name),
                                    source: SynthSource::Sfz { path: path_str },
                                });
                            }
                            "clap" => {
                                banks.push(Soundbank {
                                    name: format!("{} [CLAP]", name),
                                    source: SynthSource::ClapPlugin { path: path_str },
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
