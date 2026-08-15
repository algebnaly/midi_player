//! Floating MIDI velocity-curve editor.

use gtk::prelude::*;
use gtk::{Box, Button, Label, ToggleButton};
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::velocity_curve::{VelocityCurve, VelocityPoint, default_velocity_points};

use super::overlay::make_floating_panel_draggable;

pub fn attach_velocity_panel(
    overlay: &gtk::Overlay,
    toggle_btn: &ToggleButton,
    velocity_curve: VelocityCurve,
) {
    let velocity_panel = Box::new(gtk::Orientation::Vertical, 6);
    velocity_panel.set_size_request(430, 360);
    velocity_panel.set_halign(gtk::Align::Start);
    velocity_panel.set_valign(gtk::Align::Start);
    velocity_panel.set_margin_top(48);
    velocity_panel.set_margin_start(48);
    velocity_panel.add_css_class("floating-panel");
    velocity_panel.set_visible(false);

    let velocity_panel_title = Label::new(Some("MIDI Velocity Curve"));
    velocity_panel_title.set_hexpand(true);
    velocity_panel_title.set_xalign(0.0);
    velocity_panel_title.add_css_class("panel-title");
    let reset_velocity_btn = Button::with_label("Reset");
    reset_velocity_btn.set_tooltip_text(Some("Restore the default linear curve"));
    let close_toggle_btn = Button::with_label("×");
    close_toggle_btn.add_css_class("flat");
    close_toggle_btn.add_css_class("panel-close-button");
    let velocity_panel_header = Box::new(gtk::Orientation::Horizontal, 6);
    velocity_panel_header.append(&velocity_panel_title);
    velocity_panel_header.append(&reset_velocity_btn);
    velocity_panel_header.append(&close_toggle_btn);
    velocity_panel.append(&velocity_panel_header);

    let velocity_hint = Label::new(Some(
        "Drag points to shape the response · Double-click to add a point",
    ));
    velocity_hint.set_xalign(0.0);
    velocity_hint.add_css_class("panel-hint");
    velocity_panel.append(&velocity_hint);

    let velocity_area = gtk::DrawingArea::new();
    velocity_area.set_content_width(400);
    velocity_area.set_content_height(280);
    velocity_area.set_hexpand(true);
    velocity_area.set_vexpand(true);
    velocity_area.set_cursor_from_name(Some("crosshair"));
    velocity_panel.append(&velocity_area);
    overlay.add_overlay(&velocity_panel);

    let velocity_points = Rc::new(RefCell::new(default_velocity_points()));
    let points_draw = velocity_points.clone();
    velocity_area.set_draw_func(move |_, context, width, height| {
        let padding = 28.0;
        let graph_width = (width as f64 - padding * 2.0).max(1.0);
        let graph_height = (height as f64 - padding * 2.0).max(1.0);

        context.set_source_rgb(0.09, 0.09, 0.11);
        let _ = context.paint();
        context.set_line_width(1.0);
        context.set_source_rgb(0.25, 0.25, 0.28);
        for step in 0..=4 {
            let amount = step as f64 / 4.0;
            let x = padding + graph_width * amount;
            let y = padding + graph_height * amount;
            context.move_to(x, padding);
            context.line_to(x, padding + graph_height);
            context.move_to(padding, y);
            context.line_to(padding + graph_width, y);
        }
        let _ = context.stroke();

        let points = points_draw.borrow();
        context.set_source_rgb(0.30, 0.72, 1.0);
        context.set_line_width(2.5);
        for (index, point) in points.iter().enumerate() {
            let x = padding + point.input * graph_width;
            let y = padding + (1.0 - point.output) * graph_height;
            if index == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        let _ = context.stroke();

        for point in points.iter() {
            let x = padding + point.input * graph_width;
            let y = padding + (1.0 - point.output) * graph_height;
            context.arc(x, y, 6.0, 0.0, std::f64::consts::TAU);
            context.set_source_rgb(0.93, 0.96, 1.0);
            let _ = context.fill_preserve();
            context.set_source_rgb(0.12, 0.50, 0.82);
            context.set_line_width(2.0);
            let _ = context.stroke();
        }

        context.set_source_rgb(0.75, 0.75, 0.78);
        context.set_font_size(12.0);
        context.move_to(padding, height as f64 - 7.0);
        let _ = context.show_text("Input velocity  0");
        context.move_to(width as f64 - 48.0, height as f64 - 7.0);
        let _ = context.show_text("127");
        context.move_to(4.0, padding + 4.0);
        let _ = context.show_text("127");
        context.move_to(12.0, height as f64 - padding);
        let _ = context.show_text("0");
    });

    let active_velocity_point = Rc::new(Cell::new(None::<usize>));
    let velocity_drag = gtk::GestureDrag::new();
    let points_drag_begin = velocity_points.clone();
    let active_drag_begin = active_velocity_point.clone();
    velocity_drag.connect_drag_begin(move |gesture, x, y| {
        let Some(widget) = gesture.widget() else {
            return;
        };
        let padding = 28.0;
        let graph_width = (widget.width() as f64 - padding * 2.0).max(1.0);
        let graph_height = (widget.height() as f64 - padding * 2.0).max(1.0);
        let selected = points_drag_begin
            .borrow()
            .iter()
            .enumerate()
            .map(|(index, point)| {
                let point_x = padding + point.input * graph_width;
                let point_y = padding + (1.0 - point.output) * graph_height;
                (index, (point_x - x).hypot(point_y - y))
            })
            .filter(|(_, distance)| *distance <= 14.0)
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index);
        active_drag_begin.set(selected);
    });
    let points_drag_update = velocity_points.clone();
    let active_drag_update = active_velocity_point.clone();
    let area_drag_update = velocity_area.clone();
    let curve_drag_update = velocity_curve.clone();
    velocity_drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        let Some(index) = active_drag_update.get() else {
            return;
        };
        let Some((start_x, start_y)) = gesture.start_point() else {
            return;
        };
        let padding = 28.0;
        let graph_width = (area_drag_update.width() as f64 - padding * 2.0).max(1.0);
        let graph_height = (area_drag_update.height() as f64 - padding * 2.0).max(1.0);
        let input = ((start_x + offset_x - padding) / graph_width).clamp(0.0, 1.0);
        let output = (1.0 - (start_y + offset_y - padding) / graph_height).clamp(0.0, 1.0);
        let mut points = points_drag_update.borrow_mut();
        let last = points.len() - 1;
        let min_input = index
            .checked_sub(1)
            .map(|previous| points[previous].input + 0.005)
            .unwrap_or(0.0);
        let max_input = points
            .get(index + 1)
            .map(|next| next.input - 0.005)
            .unwrap_or(1.0);
        points[index].input = if index == 0 {
            0.0
        } else if index == last {
            1.0
        } else {
            input.clamp(min_input, max_input)
        };
        points[index].output = output;
        curve_drag_update.set_points(&points);
        drop(points);
        area_drag_update.queue_draw();
    });
    velocity_area.add_controller(velocity_drag);

    let velocity_click = gtk::GestureClick::new();
    let points_double_click = velocity_points.clone();
    let area_double_click = velocity_area.clone();
    let curve_double_click = velocity_curve.clone();
    velocity_click.connect_pressed(move |_, press_count, x, y| {
        if press_count != 2 {
            return;
        }
        let padding = 28.0;
        let graph_width = (area_double_click.width() as f64 - padding * 2.0).max(1.0);
        let graph_height = (area_double_click.height() as f64 - padding * 2.0).max(1.0);
        let input = ((x - padding) / graph_width).clamp(0.0, 1.0);
        let output = (1.0 - (y - padding) / graph_height).clamp(0.0, 1.0);
        if input <= 0.005 || input >= 0.995 {
            return;
        }
        let mut points = points_double_click.borrow_mut();
        if points
            .iter()
            .any(|point| (point.input - input).abs() < 0.01)
        {
            return;
        }
        points.push(VelocityPoint::new(input, output));
        points.sort_by(|left, right| left.input.total_cmp(&right.input));
        curve_double_click.set_points(&points);
        drop(points);
        area_double_click.queue_draw();
    });
    velocity_area.add_controller(velocity_click);

    let points_reset = velocity_points.clone();
    let area_reset = velocity_area.clone();
    let curve_reset = velocity_curve.clone();
    reset_velocity_btn.connect_clicked(move |_| {
        *points_reset.borrow_mut() = default_velocity_points();
        curve_reset.set_points(&points_reset.borrow());
        area_reset.queue_draw();
    });

    let velocity_panel_toggle = velocity_panel.clone();
    toggle_btn.connect_toggled(move |button| {
        velocity_panel_toggle.set_visible(button.is_active());
    });
    let velocity_panel_close_toggle = toggle_btn.clone();
    close_toggle_btn.connect_clicked(move |_| {
        velocity_panel_close_toggle.set_active(false);
    });

    make_floating_panel_draggable(&velocity_panel, &velocity_panel_header, overlay);
}
