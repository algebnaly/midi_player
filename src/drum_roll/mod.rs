//! Custom GTK4 drum-roll widget.

mod keyboard;
mod renderer;
pub mod types;
mod viewport;

use crate::midi::TrackMode;
use crate::roll::layout::DrumLayout;
use crate::roll::state::RollState;
use crate::roll::types::{KEY_WIDTH, default_theme};
use crate::roll::{RollView, renderer as shared_renderer};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene};
use gtk4 as gtk;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct DrumRollWidget {
        pub inner: RollState,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DrumRollWidget {
        const NAME: &'static str = "DrumRollWidget";
        type Type = super::DrumRollWidget;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for DrumRollWidget {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.set_size_request(800, 600);
            obj.set_focusable(true);
            *self.inner.scroll_y.borrow_mut() = 60.0 * 24.0 - 300.0;
            crate::roll::input::setup_controllers(&*obj);
        }
    }

    impl WidgetImpl for DrumRollWidget {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            let kw = KEY_WIDTH as f32;
            let vp = obj.build_viewport();
            let theme = default_theme();
            let active_track_idx = *self.inner.active_track.borrow();
            let drum_map = self.inner.data.borrow().as_ref().and_then(|midi| {
                midi.tracks
                    .get(active_track_idx)
                    .and_then(|track| match &track.mode {
                        TrackMode::Drum(dm) => Some(dm.clone()),
                        TrackMode::Melodic => None,
                    })
            });

            snapshot.append_color(
                &theme.background,
                &graphene::Rect::new(0.0, 0.0, width, height),
            );
            snapshot.push_clip(&graphene::Rect::new(kw, 0.0, width - kw, height));

            if let Some(ref dm) = drum_map {
                renderer::render_drum_grid(snapshot, &vp, dm, &theme);
                if let Some(midi) = &*self.inner.data.borrow() {
                    shared_renderer::render_beat_grid(snapshot, &vp, midi, &theme);
                    renderer::render_drum_notes(
                        snapshot,
                        &vp,
                        midi,
                        dm,
                        active_track_idx,
                        &*self.inner.selected_notes.borrow(),
                        &theme,
                    );
                }
                shared_renderer::render_playhead(
                    snapshot,
                    &vp,
                    *self.inner.playhead_time.borrow(),
                    &theme,
                );
                if let Some(sel) = &*self.inner.selection_rect.borrow() {
                    shared_renderer::render_selection_rect::<DrumLayout>(
                        snapshot,
                        &vp,
                        sel,
                        self.inner.data.borrow().as_ref(),
                        active_track_idx,
                        &theme,
                    );
                }
            }
            snapshot.pop();

            let pango_ctx = obj.pango_context();
            let active_pitches = shared_renderer::keyboard_active_pitches(
                *self.inner.preview_active_pitch.borrow(),
                self.inner
                    .typing_pressed_keys
                    .borrow()
                    .values()
                    .copied()
                    .chain(
                        self.inner
                            .external_pressed_notes
                            .borrow()
                            .iter()
                            .map(|(_, pitch)| *pitch),
                    )
                    .chain(self.inner.playback_active_pitches.borrow().iter().copied()),
            );
            if let Some(ref dm) = drum_map {
                keyboard::render_drum_sidebar(
                    snapshot,
                    &vp,
                    &pango_ctx,
                    dm,
                    &active_pitches,
                    &theme,
                );
            }
        }
    }
}

glib::wrapper! {
    pub struct DrumRollWidget(ObjectSubclass<imp::DrumRollWidget>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DrumRollWidget {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_active_track(&self, track_idx: usize) {
        <Self as RollView>::set_active_track(self, track_idx);
        if let Some(midi) = &*self.state().data.borrow() {
            if matches!(
                midi.tracks.get(track_idx).map(|track| &track.mode),
                Some(TrackMode::Drum(_))
            ) {
                *self.state().scroll_y.borrow_mut() = 0.0;
            }
        }
        gtk::prelude::WidgetExt::queue_draw(self);
    }
}

impl RollView for DrumRollWidget {
    type Layout = DrumLayout;

    fn state(&self) -> &RollState {
        &self.imp().inner
    }

    fn gtk_widget(&self) -> gtk::Widget {
        self.clone().upcast()
    }
}

impl Default for DrumRollWidget {
    fn default() -> Self {
        Self::new()
    }
}
