//! Custom GTK4 piano-roll widget (melodic View).

mod keyboard;
mod renderer;
pub mod types;
mod viewport;

use crate::roll::layout::MelodicLayout;
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
    pub struct PianoRollWidget {
        pub inner: RollState,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PianoRollWidget {
        const NAME: &'static str = "PianoRollWidget";
        type Type = super::PianoRollWidget;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for PianoRollWidget {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.set_size_request(800, 600);
            obj.set_focusable(true);
            *self.inner.scroll_y.borrow_mut() = 60.0 * 32.0 - 300.0;
            crate::roll::input::setup_controllers(&*obj);
        }
    }

    impl WidgetImpl for PianoRollWidget {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            let kw = KEY_WIDTH as f32;
            let vp = obj.build_viewport();
            let theme = default_theme();
            let active_track_idx = *self.inner.active_track.borrow();

            snapshot.append_color(
                &theme.background,
                &graphene::Rect::new(0.0, 0.0, width, height),
            );
            snapshot.push_clip(&graphene::Rect::new(kw, 0.0, width - kw, height));

            renderer::render_pitch_lines(snapshot, &vp, &theme);
            if let Some(midi) = &*self.inner.data.borrow() {
                shared_renderer::render_beat_grid(snapshot, &vp, midi, &theme);
                renderer::render_notes(
                    snapshot,
                    &vp,
                    midi,
                    active_track_idx,
                    &*self.inner.selected_notes.borrow(),
                    &theme,
                );
            }
            shared_renderer::render_playhead(snapshot, &vp, *self.inner.playhead_time.borrow(), &theme);
            if let Some(sel) = &*self.inner.selection_rect.borrow() {
                shared_renderer::render_selection_rect::<MelodicLayout>(
                    snapshot,
                    &vp,
                    sel,
                    self.inner.data.borrow().as_ref(),
                    active_track_idx,
                    &theme,
                );
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
            keyboard::render_keyboard(snapshot, &vp, &pango_ctx, &active_pitches, &theme);
        }
    }
}

glib::wrapper! {
    pub struct PianoRollWidget(ObjectSubclass<imp::PianoRollWidget>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PianoRollWidget {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }
}

impl RollView for PianoRollWidget {
    type Layout = MelodicLayout;

    fn state(&self) -> &RollState {
        &self.imp().inner
    }

    fn gtk_widget(&self) -> gtk::Widget {
        self.clone().upcast()
    }
}

impl Default for PianoRollWidget {
    fn default() -> Self {
        Self::new()
    }
}
