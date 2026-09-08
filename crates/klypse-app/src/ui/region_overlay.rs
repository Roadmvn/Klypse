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
    hovered: Option<Rect>,
}

impl RegionSelectionState {
    pub fn hover(&mut self, rect: Option<Rect>) {
        if self.start.is_none() {
            self.hovered = rect;
        }
    }

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
        let (Some(start), Some(current)) = (self.start, self.current) else {
            return self.hovered;
        };
        let rect = normalize_selection(start, current);
        if rect.width < 4 && rect.height < 4 && self.hovered.is_some() {
            return self.hovered;
        }
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

pub fn window_at_point(frames: &[Rect], point: (i32, i32)) -> Option<Rect> {
    frames.iter().copied().find(|rect| {
        point.0 >= rect.x
            && point.1 >= rect.y
            && i64::from(point.0) < i64::from(rect.x) + i64::from(rect.width)
            && i64::from(point.1) < i64::from(rect.y) + i64::from(rect.height)
    })
}

pub fn from_root_coordinates(rect: Rect, placement: OverlayPlacement) -> Rect {
    let scale = placement.scale.max(1);
    Rect {
        x: rect.x / scale - placement.origin.0,
        y: rect.y / scale - placement.origin.1,
        width: rect.width / scale as u32,
        height: rect.height / scale as u32,
    }
}

/// How long the selector may stay on screen before it gives up on its own.
/// Generous on purpose: picking a region is a deliberate act and people do
/// pause mid-drag.
const OVERLAY_DEADLINE_SECONDS: u32 = 300;

/// Answers the pending capture exactly once, whatever happens to the overlay.
///
/// The selector used to reply only from a handful of GTK callbacks. Any path
/// they did not cover - a cancelled gesture, a click without a drag, a window
/// hidden by the session - left the capture waiting forever, and because the
/// senders outlive the wait the channel could never close by itself. Dropping
/// this guard is now enough to release the caller.
struct Answer(async_channel::Sender<Option<Rect>>);

impl Answer {
    fn send(&self, selection: Option<Rect>) {
        let _ = self.0.try_send(selection);
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        let _ = self.0.try_send(None);
    }
}

pub struct RegionOverlay;

impl RegionOverlay {
    pub async fn select(snapshot: impl AsRef<Path>) -> Result<Option<Rect>, KlypseError> {
        Self::select_windows(snapshot, Vec::new()).await
    }

    pub async fn select_windows(
        snapshot: impl AsRef<Path>,
        frames: Vec<Rect>,
    ) -> Result<Option<Rect>, KlypseError> {
        if gdk::Display::default().is_none() {
            return Err(KlypseError::UnavailableCapability(
                "no display is available for the X11 region selector".into(),
            ));
        }
        let state = Rc::new(RefCell::new(RegionSelectionState::default()));
        let placement = Rc::new(RefCell::new(OverlayPlacement::default()));
        let (sender, receiver) = async_channel::bounded(1);
        let answer = Rc::new(Answer(sender));
        let window = gtk::Window::builder()
            .title(gettext("Klypse region selector"))
            .decorated(false)
            .modal(true)
            .build();
        let overlay = gtk::Overlay::new();
        let picture = gtk::Picture::for_filename(snapshot);
        let screen_size = picture
            .paintable()
            .map(|image| (image.intrinsic_width(), image.intrinsic_height()))
            .ok_or_else(|| KlypseError::Media("snapshot could not be loaded".into()))?;
        // The snapshot covers every monitor. Holding it in a fixed container
        // lets us slide the hosting monitor under the window; its size is set
        // from the monitor scale on map so one image pixel stays one screen
        // pixel.
        let stage = gtk::Fixed::new();
        stage.put(&picture, 0.0, 0.0);
        overlay.set_child(Some(&stage));
        let drawing = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .can_focus(true)
            .build();
        let selector_label = if frames.is_empty() {
            gettext("Drag to select a region. Release to confirm, Esc cancels.")
        } else {
            gettext("Click a window or drag a region. Enter: entire screen. Esc: cancel.")
        };
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
                // Keep the frame outside the selection so the frozen pixels
                // that will be saved remain fully visible.
                let x = f64::from(rect.x);
                let y = f64::from(rect.y);
                let w = f64::from(rect.width);
                let h = f64::from(rect.height);
                context.set_source_rgb(1.0, 1.0, 1.0);
                context.set_line_width(2.0);
                context.rectangle(x - 2.0, y - 2.0, w + 4.0, h + 4.0);
                let _ = context.stroke();

                let size = format!("{} × {}", rect.width, rect.height);
                context.set_font_size(16.0);
                let Ok(extents) = context.text_extents(&size) else {
                    return;
                };
                // Above the selection when there is room, below it otherwise.
                let baseline = if y > 28.0 { y - 10.0 } else { y + h + 24.0 };
                context.move_to(x - extents.x_bearing(), baseline);
                let _ = context.show_text(&size);
            }
        });
        overlay.add_overlay(&drawing);
        let screen_button = gtk::Button::with_label(&gettext("Capture screen"));
        screen_button.set_halign(gtk::Align::End);
        screen_button.set_valign(gtk::Align::Start);
        screen_button.set_margin_top(24);
        screen_button.set_margin_end(24);
        screen_button.add_css_class("suggested-action");
        screen_button.connect_clicked({
            let answer = Rc::clone(&answer);
            let window = window.clone();
            let placement = Rc::clone(&placement);
            move |_| {
                answer.send(Some(from_root_coordinates(
                    Rect {
                        x: 0,
                        y: 0,
                        width: screen_size.0 as u32,
                        height: screen_size.1 as u32,
                    },
                    *placement.borrow(),
                )));
                window.close();
            }
        });
        overlay.add_overlay(&screen_button);
        window.set_child(Some(&overlay));
        window.set_cursor_from_name(Some("crosshair"));

        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin({
            let state = Rc::clone(&state);
            let frames = frames.clone();
            let placement = Rc::clone(&placement);
            move |gesture, x, y| {
                let placement = *placement.borrow();
                let point = to_root_coordinates(
                    Rect {
                        x: x as i32,
                        y: y as i32,
                        width: 1,
                        height: 1,
                    },
                    placement,
                );
                state.borrow_mut().hover(
                    window_at_point(&frames, (point.x, point.y))
                        .map(|rect| from_root_coordinates(rect, placement)),
                );
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
            let answer = Rc::clone(&answer);
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
                    answer.send(Some(rect));
                    window.close();
                }
            }
        });
        drawing.add_controller(drag);

        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion({
            let state = Rc::clone(&state);
            let placement = Rc::clone(&placement);
            let drawing = drawing.clone();
            move |_, x, y| {
                let placement = *placement.borrow();
                let point = to_root_coordinates(
                    Rect {
                        x: x as i32,
                        y: y as i32,
                        width: 1,
                        height: 1,
                    },
                    placement,
                );
                let hovered = window_at_point(&frames, (point.x, point.y))
                    .map(|rect| from_root_coordinates(rect, placement));
                state.borrow_mut().hover(hovered);
                drawing.queue_draw();
            }
        });
        drawing.add_controller(motion);

        let keys = gtk::EventControllerKey::new();
        // Capture phase on the window: Esc has to work even when focus slipped
        // away from the drawing area, otherwise the overlay traps the session.
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed({
            let state = Rc::clone(&state);
            let answer = Rc::clone(&answer);
            let window = window.clone();
            let placement = Rc::clone(&placement);
            move |_, key, _, _| match key {
                gdk::Key::Escape => {
                    state.borrow_mut().cancel();
                    answer.send(None);
                    window.close();
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    // The borrow has to end before close(): tearing the window
                    // down makes GTK reset its controllers, which fires
                    // drag-end, which borrows the very same cell.
                    answer.send(Some(from_root_coordinates(
                        Rect {
                            x: 0,
                            y: 0,
                            width: screen_size.0 as u32,
                            height: screen_size.1 as u32,
                        },
                        *placement.borrow(),
                    )));
                    window.close();
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
                let scale = monitor.scale_factor().max(1);
                *placement.borrow_mut() = OverlayPlacement {
                    origin: (geometry.x(), geometry.y()),
                    scale,
                };
                // The snapshot holds device pixels while the widget tree works
                // in logical units. Without this the picture is drawn `scale`
                // times too large and the selection lands somewhere else.
                if let Some(paintable) = picture.paintable() {
                    picture.set_size_request(
                        paintable.intrinsic_width() / scale,
                        paintable.intrinsic_height() / scale,
                    );
                }
                stage.move_(&picture, -f64::from(geometry.x()), -f64::from(geometry.y()));
                drawing.queue_draw();
            }
        });
        window.connect_close_request({
            let answer = Rc::clone(&answer);
            move |_| {
                answer.send(None);
                glib::Propagation::Proceed
            }
        });
        window.connect_unmap({
            let answer = Rc::clone(&answer);
            move |_| {
                // The overlay can leave the screen without being closed, for
                // instance when the session locks or the workspace changes.
                answer.send(None);
            }
        });
        // Commands run one at a time, so an overlay that stopped answering
        // would freeze every later capture. The exits above cover the cases we
        // know of; this is the backstop for the ones we do not.
        //
        // It answers FIRST and unconditionally. Relying on close() was not
        // enough: an overlay that never got a surface ignores it, and the
        // window keeps its controllers alive through a reference cycle GTK
        // never collects, so the guard alone would never run either.
        glib::timeout_add_seconds_local_once(OVERLAY_DEADLINE_SECONDS, {
            let answer = Rc::clone(&answer);
            let window = window.clone();
            move || {
                tracing::warn!("region selection timed out; releasing the capture");
                answer.send(None);
                if window.surface().is_some() && window.is_visible() {
                    window.close();
                }
            }
        });
        // Only the widget callbacks and the backstop may keep the answer alive
        // from here on: once GTK drops them the guard replies on its own.
        drop(answer);
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
/// Recordings use the live root window after selection. Let the compositor
/// repaint before they start; screenshots instead crop the frozen snapshot.
async fn withdraw(window: &gtk::Window) {
    // The surface can already be gone, for instance when something outside the
    // application destroyed it. Touching the window then raises BadDrawable,
    // and GTK turns X errors into an abort.
    let Some(surface) = window.surface() else {
        return;
    };
    window.set_visible(false);
    surface.display().flush();
    glib::timeout_future(std::time::Duration::from_millis(80)).await;
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
