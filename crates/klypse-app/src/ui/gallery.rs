use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

use crate::i18n::gettext;
use chrono::Local;
use gtk::{gio, glib, prelude::*};
use klypse_domain::CaptureKind;
use klypse_media::Thumbnailer;
use klypse_storage::{
    AppPaths, CaptureRecord, CaptureRepository, CaptureStore, DeleteMode, StorageError,
    open_database,
};
use libadwaita as adw;
use uuid::Uuid;

use crate::{
    desktop::{clipboard::copy_record, drag::install_drag_source},
    gallery::{GalleryController, GalleryEvent},
};

const PAGE_SIZE: usize = 50;
const THUMBNAIL_EDGE: u32 = 256;
type PreviewWindowState = Rc<RefCell<Option<(adw::Window, gtk::Picture)>>>;

pub fn build(events: async_channel::Receiver<GalleryEvent>) -> Result<gtk::Widget, StorageError> {
    let paths = AppPaths::discover()?;
    build_with_paths(events, paths)
}

pub fn build_with_paths(
    events: async_channel::Receiver<GalleryEvent>,
    paths: AppPaths,
) -> Result<gtk::Widget, StorageError> {
    let repository = CaptureRepository::new(open_database(&paths)?);
    let mut controller = GalleryController::new(repository, PAGE_SIZE)?;
    controller.load_initial()?;
    let controller = Rc::new(RefCell::new(controller));
    let model = gio::ListStore::new::<glib::BoxedAnyObject>();
    append_records(&model, controller.borrow().items());

    let selection = gtk::SingleSelection::new(Some(model.clone()));
    let factory = gallery_factory();
    let grid = gtk::GridView::builder()
        .model(&selection)
        .factory(&factory)
        .max_columns(5)
        .min_columns(1)
        .single_click_activate(true)
        .build();
    let gallery_label = gettext("Capture gallery");
    grid.set_tooltip_text(Some(&gallery_label));
    super::set_accessible_label(&grid, &gallery_label);
    let preview_window = Rc::new(RefCell::new(None));
    grid.connect_activate({
        let model = model.clone();
        let preview_window = Rc::clone(&preview_window);
        move |grid, position| {
            let Some(item) = model.item(position).and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let record = item.borrow::<CaptureRecord>().clone();
            present_capture_preview(grid, &record, &preview_window);
        }
    });
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&grid)
        .build();
    let empty = gtk::Label::builder()
        .label(gettext("Your captures will appear here"))
        .css_classes(["title-2"])
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&empty, Some("empty"));

    let detail = detail_pane(&selection, &model, Rc::clone(&controller), &stack, &paths);
    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&scroll)
        .end_child(&detail)
        .resize_end_child(false)
        .shrink_end_child(false)
        .build();
    stack.add_named(&paned, Some("gallery"));
    update_empty_state(&stack, model.n_items());

    let adjustment = scroll.vadjustment();
    let loading = Rc::new(RefCell::new(false));
    adjustment.connect_value_changed({
        let controller = Rc::clone(&controller);
        let model = model.clone();
        let paths = paths.clone();
        let stack = stack.clone();
        let loading = Rc::clone(&loading);
        move |adjustment| {
            let threshold = (adjustment.upper() - adjustment.page_size()) * 0.8;
            if adjustment.value() < threshold
                || *loading.borrow()
                || !controller.borrow().has_more()
            {
                return;
            }
            *loading.borrow_mut() = true;
            let before = controller.borrow().items().len();
            let next_page = controller.borrow_mut().load_next();
            if let Ok(page) = next_page {
                let added = &page.items[before.min(page.items.len())..];
                append_records(&model, added);
                schedule_missing_thumbnails(added, &paths, controller.borrow().store(), &model);
                update_empty_state(&stack, model.n_items());
            }
            *loading.borrow_mut() = false;
        }
    });

    schedule_missing_thumbnails(
        controller.borrow().items(),
        &paths,
        controller.borrow().store(),
        &model,
    );
    glib::spawn_future_local({
        let controller = Rc::clone(&controller);
        let model = model.clone();
        let paths = paths.clone();
        let stack = stack.clone();
        async move {
            while let Ok(event) = events.recv().await {
                let refreshed = controller.borrow_mut().refresh();
                if let Ok(page) = refreshed {
                    replace_records(&model, &page.items);
                    schedule_missing_thumbnails(
                        &page.items,
                        &paths,
                        controller.borrow().store(),
                        &model,
                    );
                    update_empty_state(&stack, model.n_items());
                    if let GalleryEvent::Select(id) = event {
                        select_record(&selection, &model, id);
                    }
                }
            }
        }
    });
    Ok(stack.upcast())
}

fn gallery_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        card.add_css_class("gallery-card");
        let picture = gtk::Picture::builder()
            .width_request(THUMBNAIL_EDGE as i32)
            .height_request(160)
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .build();
        let kind = gtk::Image::new();
        let timestamp = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        card.append(&picture);
        card.append(&kind);
        card.append(&timestamp);
        install_drag_source(&card, {
            let list_item = list_item.clone();
            move || {
                list_item
                    .item()
                    .and_downcast::<glib::BoxedAnyObject>()
                    .map(|item| item.borrow::<CaptureRecord>().clone())
            }
        });
        list_item.set_child(Some(&card));
    });
    factory.connect_bind(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(item) = list_item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let Some(card) = list_item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(picture) = card.first_child().and_downcast::<gtk::Picture>() else {
            return;
        };
        let Some(kind) = picture.next_sibling().and_downcast::<gtk::Image>() else {
            return;
        };
        let Some(timestamp) = kind.next_sibling().and_downcast::<gtk::Label>() else {
            return;
        };
        let record = item.borrow::<CaptureRecord>();
        picture.set_filename(record.thumbnail_path.as_ref().or(Some(&record.path)));
        kind.set_icon_name(Some(match record.kind {
            CaptureKind::Screenshot => "camera-photo-symbolic",
            CaptureKind::Video => "media-record-symbolic",
            CaptureKind::Gif => "image-x-generic-symbolic",
        }));
        timestamp.set_label(
            &record
                .created_at
                .with_timezone(&Local)
                .format("%x %X")
                .to_string(),
        );
        let accessible_label = format!(
            "{} — {}",
            match record.kind {
                CaptureKind::Screenshot => gettext("Screenshot"),
                CaptureKind::Video => gettext("Video"),
                CaptureKind::Gif => gettext("GIF"),
            },
            timestamp.text()
        );
        super::set_accessible_label(&card, &accessible_label);
    });
    factory
}

fn present_capture_preview(
    grid: &gtk::GridView,
    record: &CaptureRecord,
    state: &PreviewWindowState,
) {
    let preview_path = match record.kind {
        CaptureKind::Video => record.thumbnail_path.as_ref().or(Some(&record.path)),
        CaptureKind::Screenshot | CaptureKind::Gif => Some(&record.path),
    };
    if let Some((window, picture)) = state.borrow().as_ref().cloned() {
        picture.set_filename(preview_path);
        window.present();
        return;
    }

    let picture = gtk::Picture::builder()
        .can_shrink(true)
        .content_fit(gtk::ContentFit::Contain)
        .hexpand(true)
        .vexpand(true)
        .build();
    picture.set_filename(preview_path);
    let preview_label = gettext("Capture preview");
    super::set_accessible_label(&picture, &preview_label);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&picture));
    let window = adw::Window::builder()
        .title(preview_label)
        .default_width(1_000)
        .default_height(700)
        .content(&toolbar)
        .build();
    if let Some(parent) = grid.root().and_downcast::<gtk::Window>() {
        window.set_transient_for(Some(&parent));
    }
    window.connect_close_request({
        let state = Rc::downgrade(state);
        move |_| {
            if let Some(state) = state.upgrade() {
                state.borrow_mut().take();
            }
            glib::Propagation::Proceed
        }
    });
    *state.borrow_mut() = Some((window.clone(), picture));
    window.present();
}

fn detail_pane(
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: Rc<RefCell<GalleryController>>,
    stack: &gtk::Stack,
    paths: &AppPaths,
) -> gtk::Box {
    let pane = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .width_request(220)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let copy = gtk::Button::with_label(&gettext("Copy"));
    let edit = gtk::Button::with_label(&gettext("Edit"));
    let reveal = gtk::Button::with_label(&gettext("Reveal in Folder"));
    let remove = gtk::Button::with_label(&gettext("Remove from Gallery"));
    let delete = gtk::Button::with_label(&gettext("Delete File"));
    for button in [&copy, &edit, &reveal, &remove, &delete] {
        button.set_sensitive(false);
        pane.append(button);
    }

    sync_detail_actions(selection, &copy, &edit, &reveal, &remove, &delete);
    selection.connect_selected_item_notify({
        let copy = copy.clone();
        let edit = edit.clone();
        let reveal = reveal.clone();
        let remove = remove.clone();
        let delete = delete.clone();
        move |selection| {
            sync_detail_actions(selection, &copy, &edit, &reveal, &remove, &delete);
        }
    });
    copy.connect_clicked({
        let selection = selection.clone();
        move |_| {
            if let Some(record) = selected_record(&selection)
                && copy_record(&record).is_err()
            {
                tracing::warn!(capture_id = %record.id, "capture could not be copied");
            }
        }
    });
    edit.connect_clicked({
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let stack = stack.clone();
        let paths = paths.clone();
        move |_| {
            let Some(selected) = selected_record(&selection) else {
                return;
            };
            let store = controller.borrow().store();
            let record = match store.get(&selected.id) {
                Ok(Some(record)) => record,
                Ok(None) | Err(_) => selected,
            };
            let on_export = Rc::new({
                let selection = selection.clone();
                let model = model.clone();
                let controller = Rc::clone(&controller);
                let stack = stack.clone();
                let paths = paths.clone();
                move |exported: CaptureRecord| {
                    let refreshed = controller.borrow_mut().refresh();
                    if let Ok(page) = refreshed {
                        let store = controller.borrow().store();
                        replace_records(&model, &page.items);
                        schedule_missing_thumbnails(
                            &page.items,
                            &paths,
                            store,
                            &model,
                        );
                        update_empty_state(&stack, model.n_items());
                        select_record(&selection, &model, exported.id);
                    }
                }
            });
            if let Err(error) =
                super::editor::present(record.clone(), store, paths.clone(), on_export)
            {
                tracing::warn!(capture_id = %record.id, %error, "capture editor could not be opened");
            }
        }
    });
    reveal.connect_clicked({
        let selection = selection.clone();
        move |_| {
            let Some(record) = selected_record(&selection) else {
                return;
            };
            let directory = record.path.parent().unwrap_or(&record.path);
            let uri = gio::File::for_path(directory).uri();
            if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
                tracing::warn!(capture_id = %record.id, "capture folder could not be opened");
            }
        }
    });
    remove.connect_clicked({
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let stack = stack.clone();
        move |_| {
            let deleted = selected_record(&selection).is_some_and(|record| {
                controller
                    .borrow_mut()
                    .delete(&record.id, DeleteMode::GalleryOnly)
                    .is_ok()
            });
            if deleted {
                let items = controller.borrow().items().to_vec();
                replace_records(&model, &items);
                update_empty_state(&stack, model.n_items());
            }
        }
    });
    delete.connect_clicked({
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let stack = stack.clone();
        move |_| {
            let deleted = selected_record(&selection).is_some_and(|record| {
                controller
                    .borrow_mut()
                    .delete(&record.id, DeleteMode::GalleryAndFile)
                    .is_ok()
            });
            if deleted {
                let items = controller.borrow().items().to_vec();
                replace_records(&model, &items);
                update_empty_state(&stack, model.n_items());
            }
        }
    });
    pane
}

fn sync_detail_actions(
    selection: &gtk::SingleSelection,
    copy: &gtk::Button,
    edit: &gtk::Button,
    reveal: &gtk::Button,
    remove: &gtk::Button,
    delete: &gtk::Button,
) {
    let record = selected_record(selection);
    let selected = record.is_some();
    copy.set_sensitive(selected);
    reveal.set_sensitive(selected);
    remove.set_sensitive(selected);
    delete.set_sensitive(selected);
    edit.set_sensitive(record.is_some_and(|record| record.kind == CaptureKind::Screenshot));
}

fn selected_record(selection: &gtk::SingleSelection) -> Option<CaptureRecord> {
    selection
        .selected_item()
        .and_downcast::<glib::BoxedAnyObject>()
        .map(|item| item.borrow::<CaptureRecord>().clone())
}

fn select_record(selection: &gtk::SingleSelection, model: &gio::ListStore, id: Uuid) {
    for index in 0..model.n_items() {
        let Some(item) = model.item(index).and_downcast::<glib::BoxedAnyObject>() else {
            continue;
        };
        if item.borrow::<CaptureRecord>().id == id {
            selection.set_selected(index);
            break;
        }
    }
}

fn append_records(model: &gio::ListStore, records: &[CaptureRecord]) {
    for record in records {
        model.append(&glib::BoxedAnyObject::new(record.clone()));
    }
}

fn replace_records(model: &gio::ListStore, records: &[CaptureRecord]) {
    model.remove_all();
    append_records(model, records);
}

fn update_empty_state(stack: &gtk::Stack, item_count: u32) {
    stack.set_visible_child_name(if item_count == 0 { "empty" } else { "gallery" });
}

fn schedule_missing_thumbnails(
    records: &[CaptureRecord],
    paths: &AppPaths,
    store: Arc<dyn CaptureStore>,
    model: &gio::ListStore,
) {
    let missing = records
        .iter()
        .filter(|record| record.thumbnail_path.is_none() && record.path.exists())
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return;
    }
    let (sender, receiver) = async_channel::unbounded::<(Uuid, PathBuf)>();
    for record in missing {
        let sender = sender.clone();
        let destination = paths.thumbnails.join(format!("{}.png", record.id));
        let store = Arc::clone(&store);
        std::thread::spawn(move || {
            if Thumbnailer::new(THUMBNAIL_EDGE)
                .generate(&record.path, &destination)
                .is_ok()
                && store.set_thumbnail(&record.id, &destination).is_ok()
            {
                let _ = sender.send_blocking((record.id, destination));
            }
        });
    }
    drop(sender);
    let model = model.clone();
    glib::spawn_future_local(async move {
        while let Ok((id, path)) = receiver.recv().await {
            update_thumbnail(&model, id, path);
        }
    });
}

fn update_thumbnail(model: &gio::ListStore, id: Uuid, path: PathBuf) {
    for index in 0..model.n_items() {
        let Some(item) = model.item(index).and_downcast::<glib::BoxedAnyObject>() else {
            continue;
        };
        if item.borrow::<CaptureRecord>().id == id {
            item.borrow_mut::<CaptureRecord>().thumbnail_path = Some(path);
            model.items_changed(index, 1, 1);
            break;
        }
    }
}
