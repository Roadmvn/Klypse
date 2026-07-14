use std::{cell::RefCell, path::Path, rc::Rc};

use gettextrs::gettext;
use gtk::{gdk, glib, prelude::*};
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

pub struct RegionOverlay;

impl RegionOverlay {
    pub async fn select(snapshot: impl AsRef<Path>) -> Result<Option<Rect>, KlypseError> {
        if gdk::Display::default().is_none() {
            return Err(KlypseError::UnavailableCapability(
                "no display is available for the X11 region selector".into(),
            ));
        }
        let state = Rc::new(RefCell::new(RegionSelectionState::default()));
        let (sender, receiver) = async_channel::bounded(1);
        let window = gtk::Window::builder()
            .title(gettext("Klypse region selector"))
            .decorated(false)
            .modal(true)
            .build();
        let overlay = gtk::Overlay::new();
        let picture = gtk::Picture::for_filename(snapshot);
        picture.set_content_fit(gtk::ContentFit::Fill);
        picture.set_can_shrink(true);
        overlay.set_child(Some(&picture));
        let drawing = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .can_focus(true)
            .build();
        let selector_label = gettext("Drag to select a capture region, then press Enter");
        drawing.set_tooltip_text(Some(&selector_label));
        super::set_accessible_label(&drawing, &selector_label);
        drawing.set_draw_func({
            let state = Rc::clone(&state);
            move |_, context, _, _| {
                let Some(rect) = state.borrow().confirm() else {
                    return;
                };
                context.set_source_rgba(0.1, 0.6, 1.0, 0.2);
                context.rectangle(
                    f64::from(rect.x),
                    f64::from(rect.y),
                    f64::from(rect.width),
                    f64::from(rect.height),
                );
                let _ = context.fill_preserve();
                context.set_source_rgb(1.0, 1.0, 1.0);
                context.set_line_width(2.0);
                let _ = context.stroke();
                context.move_to(f64::from(rect.x + 8), f64::from(rect.y + 22));
                context.set_font_size(16.0);
                let _ = context.show_text(&format!("{} × {}", rect.width, rect.height));
            }
        });
        overlay.add_overlay(&drawing);
        window.set_child(Some(&overlay));

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
        drawing.add_controller(drag);

        let keys = gtk::EventControllerKey::new();
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
        drawing.add_controller(keys);
        window.connect_close_request({
            let sender = sender.clone();
            move |_| {
                let _ = sender.try_send(None);
                glib::Propagation::Proceed
            }
        });
        drop(sender);
        window.fullscreen();
        window.present();
        drawing.grab_focus();

        Ok(receiver.recv().await.unwrap_or(None))
    }
}
