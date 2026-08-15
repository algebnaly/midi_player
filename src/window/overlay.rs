//! Overlay helpers for in-window floating panels.

use gtk::glib::object::IsA;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::Cell;
use std::rc::Rc;

/// Drag `panel` by `handle`, clamped to the overlay's visible allocation.
pub fn make_floating_panel_draggable(
    panel: &impl IsA<gtk::Widget>,
    handle: &impl IsA<gtk::Widget>,
    overlay: &gtk::Overlay,
) {
    let panel = panel.clone().upcast::<gtk::Widget>();
    let drag = gtk::GestureDrag::new();
    let origin = Rc::new(Cell::new(None::<(i32, i32, f64, f64)>));

    let panel_begin = panel.clone();
    let origin_begin = origin.clone();
    drag.connect_drag_begin(move |gesture, _, _| {
        origin_begin.set(gesture.current_event().and_then(|event| {
            event.position().map(|(pointer_x, pointer_y)| {
                (
                    panel_begin.margin_start(),
                    panel_begin.margin_top(),
                    pointer_x,
                    pointer_y,
                )
            })
        }));
    });

    let panel_update = panel;
    let overlay_update = overlay.clone();
    drag.connect_drag_update(move |gesture, _, _| {
        let Some((origin_x, origin_y, pointer_origin_x, pointer_origin_y)) = origin.get() else {
            return;
        };
        let Some((pointer_x, pointer_y)) = gesture.current_event().and_then(|event| event.position())
        else {
            return;
        };
        let max_x = (overlay_update.width() - panel_update.width()).max(0);
        let max_y = (overlay_update.height() - panel_update.height()).max(0);
        let target_x = origin_x + (pointer_x - pointer_origin_x).round() as i32;
        let target_y = origin_y + (pointer_y - pointer_origin_y).round() as i32;
        panel_update.set_margin_start(target_x.clamp(0, max_x));
        panel_update.set_margin_top(target_y.clamp(0, max_y));
    });
    handle.add_controller(drag);
}
