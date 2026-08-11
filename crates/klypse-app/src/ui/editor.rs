use std::{
    cell::{Cell, RefCell},
    fs,
    rc::Rc,
    sync::Arc,
};

use crate::i18n::gettext;
use gtk::{gdk, glib, prelude::*};
use klypse_image::{AnnotationDocument, ImageError, Point, Renderer, Rgba};
use klypse_storage::{AppPaths, CaptureRecord, CaptureStore, StorageError};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::{
    desktop::clipboard::copy_flattened_image,
    editor::{EditorController, EditorError, EditorTool},
};

const ZOOM_STEP: f64 = 1.25;

type SaveAction = Rc<dyn Fn(&mut EditorController) -> Result<(), EditorError>>;
type ExportAction = Rc<dyn Fn(&EditorController, &[u8]) -> Result<(), EditorError>>;
pub type RecordChangedCallback = Rc<dyn Fn(CaptureRecord)>;

#[derive(Clone)]
struct EditorActions {
    save: SaveAction,
    export: Option<ExportAction>,
}

#[derive(Debug, thiserror::Error)]
pub enum EditorViewError {
    #[error(transparent)]
    Image(#[from] ImageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Editor(#[from] EditorError),
}

pub struct EditorView {
    root: gtk::Box,
    canvas: gtk::DrawingArea,
    controller: Rc<RefCell<EditorController>>,
    tool_button_count: usize,
    save_action: SaveAction,
}

impl EditorView {
    pub fn new(controller: EditorController, source: Vec<u8>) -> Result<Self, EditorViewError> {
        Self::new_with_actions(
            controller,
            Rc::new(source),
            EditorActions {
                save: Rc::new(|controller| {
                    controller.mark_saved();
                    Ok(())
                }),
                export: None,
            },
        )
    }

    fn new_with_actions(
        controller: EditorController,
        source: Rc<Vec<u8>>,
        actions: EditorActions,
    ) -> Result<Self, EditorViewError> {
        let controller = Rc::new(RefCell::new(controller));
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        root.set_widget_name("klypse-editor");
        let toolbar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_top(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        let undo = gtk::Button::builder()
            .icon_name("edit-undo-symbolic")
            .tooltip_text(gettext("Undo"))
            .build();
        let redo = gtk::Button::builder()
            .icon_name("edit-redo-symbolic")
            .tooltip_text(gettext("Redo"))
            .build();
        let delete = gtk::Button::builder()
            .icon_name("edit-delete-symbolic")
            .tooltip_text(gettext("Delete last annotation"))
            .build();
        undo.set_sensitive(false);
        redo.set_sensitive(false);
        delete.set_sensitive(false);
        let zoom_out = gtk::Button::builder()
            .icon_name("zoom-out-symbolic")
            .tooltip_text(gettext("Zoom out"))
            .build();
        let zoom_label = gtk::Label::new(Some("100%"));
        let zoom_in = gtk::Button::builder()
            .icon_name("zoom-in-symbolic")
            .tooltip_text(gettext("Zoom in"))
            .build();
        let save = gtk::Button::with_label(&gettext("Save"));
        save.add_css_class("suggested-action");
        let copy = gtk::Button::with_label(&gettext("Copy"));
        let export = gtk::Button::with_label(&gettext("Export"));
        export.set_sensitive(actions.export.is_some());
        for (button, label) in [
            (&undo, gettext("Undo")),
            (&redo, gettext("Redo")),
            (&delete, gettext("Delete last annotation")),
            (&zoom_out, gettext("Zoom out")),
            (&zoom_in, gettext("Zoom in")),
        ] {
            super::set_accessible_label(button, &label);
        }
        for widget in [
            undo.clone().upcast::<gtk::Widget>(),
            redo.clone().upcast(),
            delete.clone().upcast(),
            zoom_out.clone().upcast(),
            zoom_label.clone().upcast(),
            zoom_in.clone().upcast(),
        ] {
            toolbar.append(&widget);
        }
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        toolbar.append(&spacer);
        toolbar.append(&copy);
        toolbar.append(&export);
        toolbar.append(&save);
        root.append(&toolbar);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let tool_rail = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_start(6)
            .margin_bottom(6)
            .build();
        let tools = [
            (gettext("Rectangle"), EditorTool::Rectangle),
            (gettext("Ellipse"), EditorTool::Ellipse),
            (gettext("Line"), EditorTool::Line),
            (gettext("Arrow"), EditorTool::Arrow),
            (gettext("Text"), EditorTool::Text),
            (gettext("Freehand"), EditorTool::Freehand),
            (gettext("Pixelate"), EditorTool::Pixelate),
            (gettext("Blur"), EditorTool::Blur),
            (gettext("Crop"), EditorTool::Crop),
        ];
        let mut first_tool: Option<gtk::ToggleButton> = None;
        for (label, tool) in &tools {
            let button = gtk::ToggleButton::with_label(label);
            button.set_tooltip_text(Some(label));
            if let Some(first) = &first_tool {
                button.set_group(Some(first));
            } else {
                button.set_active(true);
                first_tool = Some(button.clone());
            }
            button.connect_toggled({
                let controller = Rc::clone(&controller);
                let tool = *tool;
                move |button| {
                    if button.is_active() {
                        controller.borrow_mut().set_tool(tool);
                    }
                }
            });
            tool_rail.append(&button);
        }
        body.append(&tool_rail);

        let picture = gtk::Picture::builder()
            .can_shrink(false)
            .content_fit(gtk::ContentFit::Fill)
            .build();
        let canvas = gtk::DrawingArea::builder()
            .can_focus(true)
            .focusable(true)
            .hexpand(true)
            .vexpand(true)
            .build();
        let canvas_label = gettext("Annotation canvas");
        canvas.set_tooltip_text(Some(&canvas_label));
        super::set_accessible_label(&canvas, &canvas_label);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&picture));
        overlay.add_overlay(&canvas);
        let scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&overlay)
            .build();
        body.append(&scroll);
        root.append(&body);

        let contextual = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(6)
            .margin_end(6)
            .margin_bottom(6)
            .build();
        let color = gtk::ColorDialogButton::new(None::<gtk::ColorDialog>);
        color.set_rgba(&gdk::RGBA::new(0.93, 0.12, 0.18, 1.0));
        color.set_tooltip_text(Some(&gettext("Color")));
        super::set_accessible_label(&color, &gettext("Color"));
        let stroke = gtk::SpinButton::with_range(1.0, 32.0, 1.0);
        stroke.set_value(3.0);
        stroke.set_tooltip_text(Some(&gettext("Stroke width")));
        super::set_accessible_label(&stroke, &gettext("Stroke width"));
        let text = gtk::Entry::builder()
            .placeholder_text(gettext("Type text and press Enter"))
            .hexpand(true)
            .build();
        super::set_accessible_label(&text, &gettext("Annotation text"));
        contextual.append(&color);
        contextual.append(&stroke);
        contextual.append(&text);
        root.append(&contextual);

        let refresh = Rc::new({
            let controller = Rc::clone(&controller);
            let source = Rc::clone(&source);
            let picture = picture.clone();
            let canvas = canvas.clone();
            let zoom_label = zoom_label.clone();
            let undo = undo.clone();
            let redo = redo.clone();
            let delete = delete.clone();
            move || {
                let controller = controller.borrow();
                if let Err(error) =
                    refresh_preview(&controller, source.as_slice(), &picture, &canvas)
                {
                    tracing::warn!(%error, "editor preview could not be refreshed");
                }
                zoom_label.set_label(&format!("{:.0}%", controller.zoom() * 100.0));
                undo.set_sensitive(controller.can_undo());
                redo.set_sensitive(controller.can_redo());
                delete.set_sensitive(controller.can_delete_last_layer());
            }
        });

        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |_, x, y| {
                let updated = controller.borrow_mut().pointer_down(Point { x, y }).is_ok();
                if updated {
                    refresh();
                }
            }
        });
        drag.connect_drag_update({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |gesture, offset_x, offset_y| {
                if let Some((start_x, start_y)) = gesture.start_point() {
                    let updated = controller
                        .borrow_mut()
                        .pointer_move(Point {
                            x: start_x + offset_x,
                            y: start_y + offset_y,
                        })
                        .is_ok();
                    if updated {
                        refresh();
                    }
                }
            }
        });
        drag.connect_drag_end({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            let text = text.clone();
            move |gesture, offset_x, offset_y| {
                if let Some((start_x, start_y)) = gesture.start_point() {
                    let updated = controller
                        .borrow_mut()
                        .pointer_up(Point {
                            x: start_x + offset_x,
                            y: start_y + offset_y,
                        })
                        .is_ok();
                    if updated {
                        if controller.borrow().active_tool() == EditorTool::Text {
                            text.grab_focus();
                        }
                        refresh();
                    }
                }
            }
        });
        canvas.add_controller(drag);

        text.connect_activate({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |entry| {
                let updated = controller
                    .borrow_mut()
                    .set_text(entry.text().as_str())
                    .is_ok();
                if updated {
                    entry.set_text("");
                    refresh();
                }
            }
        });
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            let text = text.clone();
            move |_| {
                let updated = controller
                    .borrow_mut()
                    .set_text(text.text().as_str())
                    .is_ok();
                if updated {
                    text.set_text("");
                    refresh();
                }
            }
        });
        text.add_controller(focus);

        color.connect_rgba_notify({
            let controller = Rc::clone(&controller);
            move |button| {
                let value = button.rgba();
                if let Ok(color) = Rgba::new(
                    f64::from(value.red()),
                    f64::from(value.green()),
                    f64::from(value.blue()),
                    f64::from(value.alpha()),
                ) {
                    let _ = controller.borrow_mut().set_color(color);
                }
            }
        });
        stroke.connect_value_changed({
            let controller = Rc::clone(&controller);
            move |spin| {
                let _ = controller.borrow_mut().set_stroke_width(spin.value());
            }
        });

        undo.connect_clicked({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |_| {
                let updated = controller.borrow_mut().undo().is_ok();
                if updated {
                    refresh();
                }
            }
        });
        redo.connect_clicked({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |_| {
                let updated = controller.borrow_mut().redo().is_ok();
                if updated {
                    refresh();
                }
            }
        });
        delete.connect_clicked({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |_| {
                let updated = controller.borrow_mut().delete_last_layer().unwrap_or(false);
                if updated {
                    refresh();
                }
            }
        });
        zoom_out.connect_clicked({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |_| {
                let updated = controller.borrow_mut().zoom_by(1.0 / ZOOM_STEP).is_ok();
                if updated {
                    refresh();
                }
            }
        });
        zoom_in.connect_clicked({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |_| {
                let updated = controller.borrow_mut().zoom_by(ZOOM_STEP).is_ok();
                if updated {
                    refresh();
                }
            }
        });
        save.connect_clicked({
            let controller = Rc::clone(&controller);
            let save = Rc::clone(&actions.save);
            move |_| {
                if let Err(error) = save(&mut controller.borrow_mut()) {
                    tracing::warn!(%error, "annotations could not be saved");
                }
            }
        });
        copy.connect_clicked({
            let controller = Rc::clone(&controller);
            let source = Rc::clone(&source);
            move |_| {
                if let Err(error) =
                    copy_flattened_image(source.as_slice(), controller.borrow().document())
                {
                    tracing::warn!(%error, "flattened editor pixels could not be copied");
                }
            }
        });
        if let Some(export_action) = actions.export.clone() {
            export.connect_clicked({
                let controller = Rc::clone(&controller);
                let source = Rc::clone(&source);
                move |_| {
                    if let Err(error) = export_action(&controller.borrow(), source.as_slice()) {
                        tracing::warn!(%error, "flattened screenshot could not be exported");
                    }
                }
            });
        }

        let keys = gtk::EventControllerKey::new();
        let keyboard_save = Rc::clone(&actions.save);
        keys.connect_key_pressed({
            let controller = Rc::clone(&controller);
            let refresh = Rc::clone(&refresh);
            move |_, key, _, modifiers| {
                let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
                let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
                let handled = match key {
                    gdk::Key::Escape => {
                        controller.borrow_mut().cancel_current_action();
                        true
                    }
                    gdk::Key::Delete => {
                        controller.borrow_mut().delete_last_layer().unwrap_or(false)
                    }
                    gdk::Key::plus | gdk::Key::KP_Add => {
                        controller.borrow_mut().zoom_by(ZOOM_STEP).is_ok()
                    }
                    gdk::Key::minus | gdk::Key::KP_Subtract => {
                        controller.borrow_mut().zoom_by(1.0 / ZOOM_STEP).is_ok()
                    }
                    gdk::Key::z if control && shift => controller.borrow_mut().redo().is_ok(),
                    gdk::Key::z if control => controller.borrow_mut().undo().is_ok(),
                    gdk::Key::s if control => keyboard_save(&mut controller.borrow_mut()).is_ok(),
                    _ => false,
                };
                if handled {
                    refresh();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        });
        root.add_controller(keys);
        refresh();

        Ok(Self {
            root,
            canvas,
            controller,
            tool_button_count: tools.len(),
            save_action: actions.save,
        })
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn canvas(&self) -> &gtk::DrawingArea {
        &self.canvas
    }

    pub fn controller(&self) -> Rc<RefCell<EditorController>> {
        Rc::clone(&self.controller)
    }

    pub const fn tool_button_count(&self) -> usize {
        self.tool_button_count
    }
}

pub fn present(
    record: CaptureRecord,
    store: Arc<dyn CaptureStore>,
    paths: AppPaths,
    on_changed: RecordChangedCallback,
) -> Result<(), EditorViewError> {
    let source = Rc::new(fs::read(&record.path)?);
    let controller = match EditorController::open(&record) {
        Ok(controller) => controller,
        Err(EditorError::Storage(StorageError::CorruptAnnotation { reason, .. })) => {
            present_corrupt_annotation(record, store, paths, on_changed, reason);
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let notifications = adw::ToastOverlay::new();
    let actions = EditorActions {
        save: {
            let store = Arc::clone(&store);
            let paths = paths.clone();
            let record = record.clone();
            let source = Rc::clone(&source);
            let on_changed = Rc::clone(&on_changed);
            let notifications = notifications.downgrade();
            Rc::new(move |controller| {
                let saved = match controller.save_visible(
                    source.as_slice(),
                    &paths,
                    store.as_ref(),
                    &record,
                ) {
                    Ok(saved) => saved,
                    Err(error) => {
                        if let Some(notifications) = notifications.upgrade() {
                            notifications
                                .add_toast(adw::Toast::new(&gettext("Changes could not be saved")));
                        }
                        return Err(error);
                    }
                };
                on_changed(saved);
                if let Some(notifications) = notifications.upgrade() {
                    notifications.add_toast(adw::Toast::new(&gettext("Changes saved")));
                }
                Ok(())
            })
        },
        export: {
            let store = Arc::clone(&store);
            let paths = paths.clone();
            let record = record.clone();
            let on_changed = Rc::clone(&on_changed);
            Some(Rc::new(
                move |controller: &EditorController, source: &[u8]| {
                    let exported =
                        controller.export_flattened(source, &paths, store.as_ref(), &record)?;
                    on_changed(exported);
                    Ok(())
                },
            ))
        },
    };
    let view = EditorView::new_with_actions(controller, source, actions)?;
    notifications.set_child(Some(view.root()));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&notifications));
    let window = adw::Window::builder()
        .title(gettext("Edit screenshot"))
        .default_width(1100)
        .default_height(760)
        .content(&toolbar)
        .build();
    let controller = view.controller();
    let save_action = Rc::clone(&view.save_action);
    let closing = Rc::new(Cell::new(false));
    window.connect_close_request({
        let window = window.clone();
        let controller = Rc::clone(&controller);
        let closing = Rc::clone(&closing);
        move |_| {
            if closing.get() || !controller.borrow().is_dirty() {
                return glib::Propagation::Proceed;
            }
            let dialog = adw::MessageDialog::new(
                Some(&window),
                Some(&gettext("Save your changes?")),
                Some(&gettext("The original screenshot will remain unchanged.")),
            );
            dialog.add_responses(&[
                ("cancel", &gettext("Cancel")),
                ("discard", &gettext("Discard Changes")),
                ("save", &gettext("Save")),
            ]);
            dialog.set_close_response("cancel");
            dialog.set_default_response(Some("save"));
            dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
            dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
            dialog.choose(gtk::gio::Cancellable::NONE, {
                let window = window.clone();
                let controller = Rc::clone(&controller);
                let closing = Rc::clone(&closing);
                let save_action = Rc::clone(&save_action);
                move |response| match response.as_str() {
                    "save" => match save_action(&mut controller.borrow_mut()) {
                        Ok(()) => {
                            closing.set(true);
                            window.close();
                        }
                        Err(error) => {
                            tracing::warn!(%error, "annotations could not be saved before closing");
                        }
                    },
                    "discard" => {
                        closing.set(true);
                        window.close();
                    }
                    _ => {}
                }
            });
            glib::Propagation::Stop
        }
    });
    window.present();
    Ok(())
}

fn present_corrupt_annotation(
    mut record: CaptureRecord,
    store: Arc<dyn CaptureStore>,
    paths: AppPaths,
    on_changed: RecordChangedCallback,
    reason: String,
) {
    let dialog = adw::MessageDialog::new(
        None::<&gtk::Window>,
        Some(&gettext("Annotations cannot be opened")),
        Some(&format!(
            "{}\n\n{reason}",
            gettext("Reset the annotations to edit the original screenshot again?")
        )),
    );
    dialog.add_responses(&[
        ("cancel", &gettext("Cancel")),
        ("reset", &gettext("Reset annotations")),
    ]);
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
    dialog.choose(gtk::gio::Cancellable::NONE, move |response| {
        if response == "reset" {
            match store.set_annotation(&record.id, None) {
                Ok(()) => {
                    record.annotation_json = None;
                    if let Err(error) = present(record, store, paths, on_changed) {
                        tracing::warn!(%error, "capture editor could not be reopened");
                    }
                }
                Err(error) => tracing::warn!(%error, "corrupt annotations could not be reset"),
            }
        }
    });
}

fn refresh_preview(
    controller: &EditorController,
    source: &[u8],
    picture: &gtk::Picture,
    canvas: &gtk::DrawingArea,
) -> Result<(), ImageError> {
    let mut document: AnnotationDocument = controller.document().clone();
    if let Some(draft) = controller.draft_layer() {
        document.layers.push(draft.clone());
    }
    let rendered = Renderer::default().render_to_rgba(source, &document)?;
    let width = rendered.width();
    let height = rendered.height();
    let bytes = glib::Bytes::from_owned(rendered.into_raw());
    let texture = gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        width as usize * 4,
    );
    picture.set_paintable(Some(&texture));
    let display_width = (f64::from(width) * controller.zoom()).round() as i32;
    let display_height = (f64::from(height) * controller.zoom()).round() as i32;
    for widget in [
        picture.clone().upcast::<gtk::Widget>(),
        canvas.clone().upcast(),
    ] {
        widget.set_size_request(display_width.max(1), display_height.max(1));
    }
    Ok(())
}
