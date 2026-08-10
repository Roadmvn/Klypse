use std::{cell::RefCell, path::Path, rc::Rc};

use crate::i18n::gettext;
use gtk::{cairo, gdk, glib, prelude::*};
use klypse_domain::KlypseError;
use klypse_platform::{Rect, normalize_selection};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegionSelectionState {
    start: Option<(i32, i32)>,
    current: Option<(i32, i32)>,
    cancelled: bool,
}

impl RegionSelectionState {
    pub fn begin(&mut self, point: (i32, i32)) {
        self.start = Some(point);
        self.current = Some(point);
        self.cancelled = false;
    }

    pub fn update(&mut self, point: (i32, i32)) {
        if self.start.is_some() {
            self.current = Some(point);
        }
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn confirm(&self) -> Option<Rect> {
        if self.cancelled {
            return None;
        }
        let rect = normalize_selection(self.start?, self.current?);
        (rect.width > 0 && rect.height > 0).then_some(rect)
    }
}

/// Placement of the overlay window on the monitor that hosts it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayPlacement {
    /// Monitor origin in root coordinates, in logical units.
    pub origin: (i32, i32),
    /// Monitor scale factor, used to reach device pixels.
    pub scale: i32,
}

impl Default for OverlayPlacement {
    fn default() -> Self {
        Self {
            origin: (0, 0),
            scale: 1,
        }
    }
}

/// Converts a selection drawn in overlay coordinates into root coordinates.
///
/// The overlay only covers one monitor while the snapshot spans the whole root
/// window, so a selection has to be shifted by the monitor origin before the
/// capture backend can crop it.
pub fn to_root_coordinates(selection: Rect, placement: OverlayPlacement) -> Rect {
    let scale = placement.scale.max(1);
    Rect {
        x: (selection.x + placement.origin.0) * scale,
        y: (selection.y + placement.origin.1) * scale,
        width: selection.width * scale.unsigned_abs(),
        height: selection.height * scale.unsigned_abs(),
    }
}

pub struct RegionOverlay;

impl RegionOverlay {
    pub async fn select(snapshot: impl AsRef<Path>) -> Result<Option<Rect>, KlypseError> {
        if gdk::Display::default().is_none() {
            return Err(KlypseError::UnavailableCapability(
                "no display is available for the X11 region selector".into(),
            ));
        }
        let state = Rc::new(RefCell::new(RegionSelectionState::default()));
        let placement = Rc::new(RefCell::new(OverlayPlacement::default()));
        let (sender, receiver) = async_channel::bounded(1);
        let window = gtk::Window::builder()
            .title(gettext("Klypse region selector"))
            .decorated(false)
            .modal(true)
            .build();
        let overlay = gtk::Overlay::new();
        let picture = gtk::Picture::for_filename(snapshot);
        picture.set_can_shrink(false);
        // The snapshot covers every monitor. Holding it in a fixed container
        // keeps it at its natural size so one screen pixel stays one image
        // pixel, and lets us slide the hosting monitor under the window.
        let stage = gtk::Fixed::new();
        stage.put(&picture, 0.0, 0.0);
        overlay.set_child(Some(&stage));
        let drawing = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .can_focus(true)
            .build();
        let selector_label = gettext("Drag to select a region. Enter confirms, Esc cancels.");
        drawing.set_tooltip_text(Some(&selector_label));
        super::set_accessible_label(&drawing, &selector_label);
        drawing.set_draw_func({
            let state = Rc::clone(&state);
            let selector_label = selector_label.clone();
            move |_, context, width, height| {
                let width = f64::from(width);
                let height = f64::from(height);
                let selection = state.borrow().confirm();
                // Dimming everything but the selection is what tells the user
                // the desktop is still alive: an untouched snapshot is
                // indistinguishable from a frozen screen.
                context.set_source_rgba(0.0, 0.0, 0.0, 0.45);
                context.rectangle(0.0, 0.0, width, height);
                if let Some(rect) = selection {
                    context.rectangle(
                        f64::from(rect.x),
                        f64::from(rect.y),
                        f64::from(rect.width),
                        f64::from(rect.height),
                    );
                    context.set_fill_rule(cairo::FillRule::EvenOdd);
                }
                let _ = context.fill();
                context.set_fill_rule(cairo::FillRule::Winding);

                let Some(rect) = selection else {
                    draw_hint(context, width, height, &selector_label);
                    return;
                };
                context.set_source_rgb(1.0, 1.0, 1.0);
                context.set_line_width(2.0);
                context.rectangle(
                    f64::from(rect.x),
                    f64::from(rect.y),
                    f64::from(rect.width),
                    f64::from(rect.height),
                );
                let _ = context.stroke();
                context.move_to(f64::from(rect.x + 8), f64::from(rect.y + 22));
                context.set_font_size(16.0);
                let _ = context.show_text(&format!("{} × {}", rect.width, rect.height));
            }
        });
        overlay.add_overlay(&drawing);
        window.set_child(Some(&overlay));
        window.set_cursor_from_name(Some("crosshair"));

        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin({
            let state = Rc::clone(&state);
            move |gesture, x, y| {
                state
                    .borrow_mut()
                    .begin((x.round() as i32, y.round() as i32));
                if let Some(widget) = gesture.widget() {
                    widget.queue_draw();
                }
            }
        });
        drag.connect_drag_update({
            let state = Rc::clone(&state);
            move |gesture, offset_x, offset_y| {
                if let Some((start_x, start_y)) = gesture.start_point() {
                    state.borrow_mut().update((
                        (start_x + offset_x).round() as i32,
                        (start_y + offset_y).round() as i32,
                    ));
                }
                if let Some(widget) = gesture.widget() {
                    widget.queue_draw();
                }
            }
        });
        drag.connect_drag_end({
            let state = Rc::clone(&state);
            let sender = sender.clone();
            let window = window.clone();
            move |gesture, offset_x, offset_y| {
                // Releasing the pointer confirms the selection. Waiting for a
                // key press left people staring at a still image with no way of
                // knowing the overlay was waiting for them.
                let confirmed = {
                    let mut state = state.borrow_mut();
                    if let Some((start_x, start_y)) = gesture.start_point() {
                        state.update((
                            (start_x + offset_x).round() as i32,
                            (start_y + offset_y).round() as i32,
                        ));
                    }
                    state.confirm()
                };
                if let Some(rect) = confirmed {
                    let _ = sender.try_send(Some(rect));
                    window.close();
                }
            }
        });
        drawing.add_controller(drag);

        let keys = gtk::EventControllerKey::new();
        // Capture phase on the window: Esc has to work even when focus slipped
        // away from the drawing area, otherwise the overlay traps the session.
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed({
            let state = Rc::clone(&state);
            let sender = sender.clone();
            let window = window.clone();
            move |_, key, _, _| match key {
                gdk::Key::Escape => {
                    state.borrow_mut().cancel();
                    let _ = sender.try_send(None);
                    window.close();
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if let Some(rect) = state.borrow().confirm() {
                        let _ = sender.try_send(Some(rect));
                        window.close();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        window.add_controller(keys);

        window.connect_map({
            let placement = Rc::clone(&placement);
            let stage = stage.clone();
            let picture = picture.clone();
            let drawing = drawing.clone();
            move |window| {
                let (Some(surface), Some(display)) = (window.surface(), gdk::Display::default())
                else {
                    return;
                };
                let Some(monitor) = display.monitor_at_surface(&surface) else {
                    return;
                };
                let geometry = monitor.geometry();
                *placement.borrow_mut() = OverlayPlacement {
                    origin: (geometry.x(), geometry.y()),
                    scale: monitor.scale_factor(),
                };
                stage.move_(&picture, -f64::from(geometry.x()), -f64::from(geometry.y()));
                drawing.queue_draw();
            }
        });
        window.connect_close_request({
            let sender = sender.clone();
            move |_| {
                let _ = sender.try_send(None);
                glib::Propagation::Proceed
            }
        });
        window.connect_destroy({
            let sender = sender.clone();
            move |_| {
                // A surface torn down from the outside never emits a close
                // request. Without this the capture would wait forever and the
                // whole application would sit there holding a modal grab.
                let _ = sender.try_send(None);
            }
        });
        drop(sender);
        window.fullscreen();
        window.present();
        drawing.grab_focus();

        let selection = receiver.recv().await.unwrap_or(None);
        withdraw(&window).await;
        let placement = *placement.borrow();
        Ok(selection.map(|rect| to_root_coordinates(rect, placement)))
    }
}

/// Takes the overlay off screen and waits for the server to drop it.
///
/// The caller grabs the selected pixels straight from the root window, so
/// returning while the overlay is still mapped bakes the selection frame and
/// its size label into the screenshot.
async fn withdraw(window: &gtk::Window) {
    window.set_visible(false);
    for _ in 0..40 {
        if !window.is_mapped() {
            break;
        }
        glib::timeout_future(std::time::Duration::from_millis(5)).await;
    }
    // Unmapping only queues the repaint of whatever sat underneath.
    glib::timeout_future(std::time::Duration::from_millis(60)).await;
}

fn draw_hint(context: &cairo::Context, width: f64, height: f64, hint: &str) {
    context.set_font_size(20.0);
    let Ok(extents) = context.text_extents(hint) else {
        return;
    };
    let padding = 24.0;
    let box_width = extents.width() + padding * 2.0;
    let box_height = extents.height() + padding * 2.0;
    let x = ((width - box_width) / 2.0).max(0.0);
    let y = ((height - box_height) / 2.0).max(0.0);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.78);
    context.rectangle(x, y, box_width, box_height);
    let _ = context.fill();
    context.set_source_rgb(1.0, 1.0, 1.0);
    context.move_to(
        x + padding - extents.x_bearing(),
        y + padding - extents.y_bearing(),
    );
    let _ = context.show_text(hint);
}
