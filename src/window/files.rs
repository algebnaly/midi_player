//! Open / save / export and command-line project loading.

use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

use crate::midi::{MidiData, TrackMode};
use crate::player::Player;
use crate::project::{PROJECT_EXTENSION, ProjectFile};

use super::helpers::TrackUi;

pub fn wire_file_actions(
    window: &gtk::ApplicationWindow,
    open_btn: &gtk::Button,
    save_btn: &gtk::Button,
    save_project_btn: &gtk::Button,
    export_wav_btn: &gtk::Button,
    export_mp3_btn: &gtk::Button,
    tracks: &TrackUi,
    bpm_spin: &gtk::SpinButton,
    current_midi_path: Rc<RefCell<Option<String>>>,
    player: Rc<RefCell<Option<Player>>>,
    def_sf2: String,
    def_drum_sf2: String,
) {
    let window_clone = window.clone();
    let current_midi_clone = current_midi_path;
    let tracks_open = tracks.clone();
    let bpm_spin_open = bpm_spin.clone();
    let player_open = player.clone();
    open_btn.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::new();
        let window = window_clone.clone();
        let midi_path = current_midi_clone.clone();
        let tracks = tracks_open.clone();
        let bpm_spin_inner = bpm_spin_open.clone();
        let def_sf2 = def_sf2.clone();
        let def_drum_sf2 = def_drum_sf2.clone();
        let p_open = player_open.clone();

        dialog.open(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
            if let Ok(file) = res {
                let path = file.path().unwrap();
                let path_str = path.to_string_lossy().to_string();
                *midi_path.borrow_mut() = Some(path_str.clone());

                let is_project = path
                    .extension()
                    .is_some_and(|extension| extension == PROJECT_EXTENSION);
                let loaded = if is_project {
                    ProjectFile::load(&path).map(|project| project.midi)
                } else {
                    MidiData::load(&path_str)
                };
                match loaded {
                    Ok(mut data) => {
                        if let Some(p) = &mut *p_open.borrow_mut() {
                            for track in &mut data.tracks {
                                let is_drum = matches!(&track.mode, TrackMode::Drum(_));
                                if !is_project {
                                    let initial_preset = match &track.synth_source {
                                        crate::midi::SynthSource::SoundFont { preset, .. } => *preset,
                                        _ => 0,
                                    };
                                    track.synth_source = if is_drum && !def_drum_sf2.is_empty() {
                                        crate::midi::SynthSource::SoundFont {
                                            path: def_drum_sf2.clone(),
                                            bank: 0,
                                            preset: 0,
                                        }
                                    } else {
                                        crate::midi::SynthSource::SoundFont {
                                            path: def_sf2.clone(),
                                            bank: 0,
                                            preset: initial_preset,
                                        }
                                    };
                                }

                                match p.add_or_get_synth(&track.synth_source) {
                                    Ok(idx) => track.synth_index = idx,
                                    Err(e) => eprintln!("Failed to load synth for track: {}", e),
                                }
                            }
                        }
                        let bpm = data.get_bpm();
                        let first_track = data.tracks[0].id;
                        bpm_spin_inner.set_value(bpm);
                        tracks.install(data, first_track, true);
                        tracks.roll.set_playhead(0.0);
                    }
                    Err(e) => eprintln!("Failed to load midi: {}", e),
                }
            }
        });
    });

    let tracks_save = tracks.clone();
    let window_save = window.clone();
    save_btn.connect_clicked(move |_| {
        if let Some(midi) = tracks_save.roll.get_data_clone() {
            let dialog = gtk::FileDialog::new();
            let window = window_save.clone();
            dialog.save(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
                if let Ok(file) = res
                    && let Some(path) = file.path()
                {
                    let path_str = path.to_string_lossy().to_string();
                    if let Err(e) = midi.export_to_file(&path_str) {
                        eprintln!("Failed to export: {}", e);
                    }
                }
            });
        }
    });

    let tracks_save_project = tracks.clone();
    let window_save_project = window.clone();
    save_project_btn.connect_clicked(move |_| {
        let Some(midi) = tracks_save_project.roll.get_data_clone() else {
            return;
        };
        let dialog = gtk::FileDialog::new();
        let window = window_save_project.clone();
        dialog.save(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
            if let Ok(file) = res
                && let Some(mut path) = file.path()
            {
                if path
                    .extension()
                    .is_none_or(|extension| extension != PROJECT_EXTENSION)
                {
                    path.set_extension(PROJECT_EXTENSION);
                }
                if let Err(err) = ProjectFile::new(midi).save(&path) {
                    eprintln!("Failed to save project: {err}");
                }
            }
        });
    });

    let tracks_export_wav = tracks.clone();
    let window_export_wav = window.clone();
    let player_export_wav = player.clone();
    export_wav_btn.connect_clicked(move |_| {
        let Some(midi) = tracks_export_wav.roll.get_data_clone() else {
            return;
        };
        let dialog = gtk::FileDialog::new();
        dialog.set_title("Export Audio (WAV)");
        dialog.set_initial_name(Some("output.wav"));
        let window = window_export_wav.clone();
        let p_cell = player_export_wav.clone();
        dialog.save(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
            if let Ok(file) = res
                && let Some(mut path) = file.path()
            {
                if path.extension().is_none_or(|ext| ext != "wav") {
                    path.set_extension("wav");
                }
                let mut p = p_cell.borrow_mut();
                if let Some(player) = p.as_mut() {
                    println!("Rendering WAV audio offline...");
                    match player.render_offline(&midi) {
                        Ok((left, right, sample_rate)) => {
                            if let Err(e) = crate::audio_export::export_wav(&path, &left, &right, sample_rate) {
                                eprintln!("Failed to export WAV: {}", e);
                            } else {
                                println!("✅ Successfully exported WAV: {}", path.display());
                            }
                        }
                        Err(e) => eprintln!("Failed to render audio: {}", e),
                    }
                }
            }
        });
    });

    let tracks_export_mp3 = tracks.clone();
    let window_export_mp3 = window.clone();
    let player_export_mp3 = player.clone();
    export_mp3_btn.connect_clicked(move |_| {
        let Some(midi) = tracks_export_mp3.roll.get_data_clone() else {
            return;
        };
        let dialog = gtk::FileDialog::new();
        dialog.set_title("Export Audio (MP3)");
        dialog.set_initial_name(Some("output.mp3"));
        let window = window_export_mp3.clone();
        let p_cell = player_export_mp3.clone();
        dialog.save(Some(&window), None::<&gtk::gio::Cancellable>, move |res| {
            if let Ok(file) = res
                && let Some(mut path) = file.path()
            {
                if path.extension().is_none_or(|ext| ext != "mp3") {
                    path.set_extension("mp3");
                }
                let mut p = p_cell.borrow_mut();
                if let Some(player) = p.as_mut() {
                    println!("Rendering MP3 audio offline...");
                    match player.render_offline(&midi) {
                        Ok((left, right, sample_rate)) => {
                            if let Err(e) = crate::audio_export::export_mp3(&path, &left, &right, sample_rate) {
                                eprintln!("Failed to export MP3: {}", e);
                            } else {
                                println!("✅ Successfully exported MP3: {}", path.display());
                            }
                        }
                        Err(e) => eprintln!("Failed to render audio: {}", e),
                    }
                }
            }
        });
    });
}

pub fn load_initial_project(
    path_str: String,
    current_midi_path: &Rc<RefCell<Option<String>>>,
    player: &Rc<RefCell<Option<Player>>>,
    bpm_spin: &gtk::SpinButton,
    tracks: &TrackUi,
) {
    let path = std::path::Path::new(&path_str);
    if !path.extension().is_some_and(|ext| ext == PROJECT_EXTENSION) {
        eprintln!("Command line loading only supports project files (.midiproj)");
        return;
    }

    *current_midi_path.borrow_mut() = Some(path_str.clone());
    match ProjectFile::load(path) {
        Ok(project) => {
            let mut data = project.midi;
            if let Some(p) = &mut *player.borrow_mut() {
                for track in &mut data.tracks {
                    match p.add_or_get_synth(&track.synth_source) {
                        Ok(idx) => track.synth_index = idx,
                        Err(e) => eprintln!("Failed to load synth for track: {}", e),
                    }
                }
            }
            let bpm = data.get_bpm();
            let first_track = data
                .tracks
                .first()
                .map(|t| t.id)
                .unwrap_or_else(|| crate::midi::TrackId(1));
            bpm_spin.set_value(bpm);
            tracks.install(data, first_track, true);
            tracks.roll.set_playhead(0.0);
        }
        Err(e) => eprintln!("Failed to load initial file {}: {}", path_str, e),
    }
}
