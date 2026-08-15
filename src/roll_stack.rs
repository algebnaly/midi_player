use crate::drum_roll::DrumRollWidget;
use crate::midi::{MidiData, TrackId};
use crate::piano_roll::types::EditMode;
use crate::piano_roll::PianoRollWidget;
use crate::roll::RollView;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone)]
pub enum RollWidget {
    Melodic(PianoRollWidget),
    Drum(DrumRollWidget),
}

impl RollWidget {
    pub fn widget(&self) -> &gtk::Widget {
        match self {
            Self::Melodic(w) => w.upcast_ref(),
            Self::Drum(w) => w.upcast_ref(),
        }
    }
}

#[derive(Clone)]
pub struct RollStack {
    pub stack: gtk::Stack,
    widgets: Rc<RefCell<Vec<RollWidget>>>,
    active_idx: Rc<Cell<usize>>,

    cb_status: Rc<RefCell<Vec<Rc<dyn Fn(String)>>>>,
    cb_seek: Rc<RefCell<Vec<Rc<dyn Fn(f64)>>>>,
    cb_data_changed: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
    cb_note_on: Rc<RefCell<Vec<Rc<dyn Fn(usize, u8, u8, u8)>>>>,
    cb_note_off: Rc<RefCell<Vec<Rc<dyn Fn(usize, u8, u8)>>>>,
}

impl RollStack {
    pub fn new() -> Self {
        Self {
            stack: gtk::Stack::new(),
            widgets: Rc::new(RefCell::new(Vec::new())),
            active_idx: Rc::new(Cell::new(0)),
            cb_status: Rc::new(RefCell::new(Vec::new())),
            cb_seek: Rc::new(RefCell::new(Vec::new())),
            cb_data_changed: Rc::new(RefCell::new(Vec::new())),
            cb_note_on: Rc::new(RefCell::new(Vec::new())),
            cb_note_off: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn set_default_note_beats(&self, beats: f64) {
        for w in self.widgets.borrow().iter() {
            match w {
                RollWidget::Melodic(mw) => mw.set_default_note_beats(beats),
                RollWidget::Drum(dw) => dw.set_default_note_beats(beats),
            }
        }
    }

    fn create_widget(&self, track_idx: usize, is_drum: bool) -> RollWidget {
        let widgets_clone = Rc::clone(&self.widgets);
        let active_idx_clone = Rc::clone(&self.active_idx);
        let cb_data_changed_ref = Rc::clone(&self.cb_data_changed);

        let on_data_changed = move || {
            let current_idx = active_idx_clone.get();

            let maybe_midi = if let Some(active_w) = widgets_clone.borrow().get(current_idx) {
                match active_w {
                    RollWidget::Melodic(mw) => mw.get_data_clone(),
                    RollWidget::Drum(dw) => dw.get_data_clone(),
                }
            } else {
                None
            };

            if let Some(midi) = maybe_midi {
                for (i, w) in widgets_clone.borrow().iter().enumerate() {
                    if i != current_idx {
                        match w {
                            RollWidget::Melodic(mw) => mw.update_data(midi.clone()),
                            RollWidget::Drum(dw) => dw.update_data(midi.clone()),
                        }
                    }
                }
            }

            for cb in cb_data_changed_ref.borrow().iter() {
                cb();
            }
        };

        if is_drum {
            let w = DrumRollWidget::new();
            w.set_active_track(track_idx);
            for cb in self.cb_status.borrow().iter() {
                let cb_clone = cb.clone();
                w.connect_status(move |msg| cb_clone(msg.to_string()));
            }
            for cb in self.cb_seek.borrow().iter() {
                let cb_clone = cb.clone();
                w.connect_seek(move |t| cb_clone(t));
            }
            w.connect_data_changed(on_data_changed);
            for cb in self.cb_note_on.borrow().iter() {
                let cb_clone = cb.clone();
                w.connect_preview_note_on(move |s, p, v, c| cb_clone(s, p, v, c));
            }
            for cb in self.cb_note_off.borrow().iter() {
                let cb_clone = cb.clone();
                w.connect_preview_note_off(move |s, p, c| cb_clone(s, p, c));
            }
            RollWidget::Drum(w)
        } else {
            let w = PianoRollWidget::new();
            w.set_active_track(track_idx);
            for cb in self.cb_status.borrow().iter() {
                let cb_clone = cb.clone();
                w.connect_status(move |msg| cb_clone(msg.to_string()));
            }
            for cb in self.cb_seek.borrow().iter() {
                let cb_clone = cb.clone();
                w.connect_seek(move |t| cb_clone(t));
            }
            w.connect_data_changed(on_data_changed);
            for cb in self.cb_note_on.borrow().iter() {
                let cb_clone = cb.clone();
                w.connect_preview_note_on(move |s, p, v, c| cb_clone(s, p, v, c));
            }
            for cb in self.cb_note_off.borrow().iter() {
                let cb_clone = cb.clone();
                w.connect_preview_note_off(move |s, p, c| cb_clone(s, p, c));
            }
            RollWidget::Melodic(w)
        }
    }

    pub fn set_data(&self, midi: MidiData) {
        let mut widgets = self.widgets.borrow_mut();

        for i in 0..widgets.len().min(midi.tracks.len()) {
            let is_drum = matches!(&midi.tracks[i].mode, crate::midi::TrackMode::Drum(_));
            let current_is_drum = matches!(&widgets[i], RollWidget::Drum(_));
            if is_drum != current_is_drum {
                self.stack.remove(widgets[i].widget());
                let new_widget = self.create_widget(i, is_drum);
                let name = format!("track_{}", i);
                self.stack.add_named(new_widget.widget(), Some(&name));
                widgets[i] = new_widget;
            }
        }

        while widgets.len() < midi.tracks.len() {
            let track_idx = widgets.len();
            let is_drum = matches!(&midi.tracks[track_idx].mode, crate::midi::TrackMode::Drum(_));
            let new_widget = self.create_widget(track_idx, is_drum);
            let name = format!("track_{}", track_idx);
            self.stack.add_named(new_widget.widget(), Some(&name));
            widgets.push(new_widget);
        }

        for (i, w) in widgets.iter().enumerate() {
            if i < midi.tracks.len() {
                match w {
                    RollWidget::Melodic(mw) => {
                        mw.set_midi(midi.clone());
                        mw.set_active_track(i);
                    }
                    RollWidget::Drum(dw) => {
                        dw.set_midi(midi.clone());
                        dw.set_active_track(i);
                    }
                }
            }
        }

        self.update_visible_child();
    }

    pub fn set_active_track(&self, idx: usize) {
        self.active_idx.set(idx);
        self.update_visible_child();
    }

    fn update_visible_child(&self) {
        let idx = self.active_idx.get();
        let name = format!("track_{}", idx);
        if let Some(child) = self.stack.child_by_name(&name) {
            self.stack.set_visible_child(&child);
        }
    }

    pub fn get_data_clone(&self) -> Option<MidiData> {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.first() {
            match w {
                RollWidget::Melodic(mw) => mw.get_data_clone(),
                RollWidget::Drum(dw) => dw.get_data_clone(),
            }
        } else {
            None
        }
    }

    pub fn notify_data_changed(&self) {
        for w in self.widgets.borrow().iter() {
            match w {
                RollWidget::Melodic(mw) => mw.notify_data_changed(),
                RollWidget::Drum(dw) => dw.notify_data_changed(),
            }
        }
    }

    pub fn active_track_index(&self) -> usize {
        self.active_idx.get()
    }

    pub fn track_synth_index(&self, track_idx: usize) -> usize {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(track_idx) {
            match w {
                RollWidget::Melodic(mw) => mw.track_synth_index(track_idx),
                RollWidget::Drum(dw) => dw.track_synth_index(track_idx),
            }
        } else {
            0
        }
    }

    pub fn set_playhead(&self, ticks: f64) {
        for w in self.widgets.borrow().iter() {
            match w {
                RollWidget::Melodic(mw) => mw.set_playhead(ticks),
                RollWidget::Drum(dw) => dw.set_playhead(ticks),
            }
        }
    }

    pub fn handle_note_deleted(&self, track: usize, note_idx: usize) {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(track) {
            match w {
                RollWidget::Melodic(mw) => mw.handle_note_deleted(track, note_idx),
                RollWidget::Drum(dw) => dw.handle_note_deleted(track, note_idx),
            }
        }
    }

    pub fn connect_status<F: Fn(String) + 'static>(&self, f: F) {
        let rc = Rc::new(f);
        self.cb_status.borrow_mut().push(rc);
    }

    pub fn connect_seek<F: Fn(f64) + 'static>(&self, f: F) {
        let rc = Rc::new(f);
        self.cb_seek.borrow_mut().push(rc);
    }

    pub fn connect_data_changed<F: Fn() + 'static>(&self, f: F) {
        let rc = Rc::new(f);
        self.cb_data_changed.borrow_mut().push(rc);
    }

    pub fn connect_preview_note_on<F: Fn(usize, u8, u8, u8) + 'static>(&self, f: F) {
        let rc = Rc::new(f);
        self.cb_note_on.borrow_mut().push(rc.clone());
        for w in self.widgets.borrow().iter() {
            let cb = rc.clone();
            match w {
                RollWidget::Melodic(mw) => {
                    mw.connect_preview_note_on(move |s, p, v, c| cb(s, p, v, c))
                }
                RollWidget::Drum(dw) => {
                    dw.connect_preview_note_on(move |s, p, v, c| cb(s, p, v, c))
                }
            }
        }
    }

    pub fn connect_preview_note_off<F: Fn(usize, u8, u8) + 'static>(&self, f: F) {
        let rc = Rc::new(f);
        self.cb_note_off.borrow_mut().push(rc.clone());
        for w in self.widgets.borrow().iter() {
            let cb = rc.clone();
            match w {
                RollWidget::Melodic(mw) => mw.connect_preview_note_off(move |s, p, c| cb(s, p, c)),
                RollWidget::Drum(dw) => dw.connect_preview_note_off(move |s, p, c| cb(s, p, c)),
            }
        }
    }

    pub fn get_edit_mode(&self) -> EditMode {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.get_edit_mode(),
                RollWidget::Drum(dw) => dw.get_edit_mode(),
            }
        } else {
            EditMode::Draw
        }
    }

    pub fn is_typing_keyboard_enabled(&self) -> bool {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.is_typing_keyboard_enabled(),
                RollWidget::Drum(dw) => dw.is_typing_keyboard_enabled(),
            }
        } else {
            false
        }
    }

    pub fn update_data(&self, midi: MidiData) {
        self.set_data(midi);
    }

    pub fn update_data_and_notify(&self, midi: MidiData) {
        self.set_data(midi);
        self.notify_data_changed();
    }

    pub fn toggle_put_length_quantization(&self) -> bool {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.toggle_put_length_quantization(),
                RollWidget::Drum(dw) => dw.toggle_put_length_quantization(),
            }
        } else {
            false
        }
    }

    pub fn is_normal_mode(&self) -> bool {
        self.get_edit_mode() == EditMode::Draw
    }

    pub fn enter_put_mode(&self) {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.enter_put_mode(),
                RollWidget::Drum(dw) => dw.enter_put_mode(),
            }
        }
    }

    pub fn active_track_id(&self) -> Option<TrackId> {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.active_track_id(),
                RollWidget::Drum(dw) => dw.active_track_id(),
            }
        } else {
            None
        }
    }

    pub fn set_playback_active_pitches(&self, pitches: impl IntoIterator<Item = u8> + Clone) {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.set_playback_active_pitches(pitches),
                RollWidget::Drum(dw) => dw.set_playback_active_pitches(pitches),
            }
        }
    }

    pub fn grab_focus(&self) {
        self.stack.grab_focus();
    }

    pub fn put_midi_note_on(
        &self,
        channel: u8,
        pitch: u8,
        velocity: u8,
        occurred_at: std::time::Instant,
    ) -> bool {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.put_midi_note_on(channel, pitch, velocity, occurred_at),
                RollWidget::Drum(dw) => dw.put_midi_note_on(channel, pitch, velocity, occurred_at),
            }
        } else {
            false
        }
    }

    pub fn put_midi_note_off(&self, channel: u8, pitch: u8, occurred_at: std::time::Instant) -> bool {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.put_midi_note_off(channel, pitch, occurred_at),
                RollWidget::Drum(dw) => dw.put_midi_note_off(channel, pitch, occurred_at),
            }
        } else {
            false
        }
    }

    pub fn enter_normal_mode(&self) {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.enter_normal_mode(),
                RollWidget::Drum(dw) => dw.enter_normal_mode(),
            }
        }
    }

    pub fn enter_select_mode(&self) {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.enter_select_mode(),
                RollWidget::Drum(dw) => dw.enter_select_mode(),
            }
        }
    }

    pub fn enter_typing_keyboard_mode(&self) {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.enter_typing_keyboard_mode(),
                RollWidget::Drum(dw) => dw.enter_typing_keyboard_mode(),
            }
        }
    }

    pub fn clear_external_notes(&self) {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.clear_external_notes(),
                RollWidget::Drum(dw) => dw.clear_external_notes(),
            }
        }
    }

    pub fn set_external_note_active(&self, channel: u8, pitch: u8, active: bool) {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.set_external_note_active(channel, pitch, active),
                RollWidget::Drum(dw) => dw.set_external_note_active(channel, pitch, active),
            }
        }
    }

    pub fn get_playhead_tick(&self) -> f64 {
        let widgets = self.widgets.borrow();
        if let Some(w) = widgets.get(self.active_idx.get()) {
            match w {
                RollWidget::Melodic(mw) => mw.get_playhead_tick(),
                RollWidget::Drum(dw) => dw.get_playhead_tick(),
            }
        } else {
            0.0
        }
    }

    pub fn set_playhead_tick(&self, tick: f64) {
        for w in self.widgets.borrow().iter() {
            match w {
                RollWidget::Melodic(mw) => mw.set_playhead_tick(tick),
                RollWidget::Drum(dw) => dw.set_playhead_tick(tick),
            }
        }
    }
}
