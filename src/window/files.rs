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
    let player_open = player;
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
                                    track.synth_source = if is_drum && !def_drum_sf2.is_empty() {
                                        crate::midi::SynthSource::SoundFont {
                                            path: def_drum_sf2.clone(),
                                        }
                                    } else {
                                        crate::midi::SynthSource::SoundFont {
                                            path: def_sf2.clone(),
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
