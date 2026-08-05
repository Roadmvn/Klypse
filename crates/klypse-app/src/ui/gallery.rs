use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};

use crate::i18n::gettext;
use chrono::Local;
use gtk::{gdk, gio, glib, prelude::*};
use klypse_domain::CaptureKind;
use klypse_image::Renderer;
use klypse_media::Thumbnailer;
use klypse_storage::{
    AppPaths, CaptureRecord, CaptureRepository, CaptureStore, DeleteMode, StorageError,
    open_database,
};
use libadwaita as adw;
use libadwaita::prelude::*;
use uuid::Uuid;

use crate::{
    desktop::{clipboard::copy_record, drag::install_drag_source},
    editor::EditorController,
    gallery::{GalleryController, GalleryEvent},
};

const PAGE_SIZE: usize = 50;
const THUMBNAIL_EDGE: u32 = 256;

struct CapturePreview {
    root: gtk::Box,
    back: gtk::Button,
    close: gtk::Button,
    picture: gtk::Picture,
    previous: gtk::Button,
    next: gtk::Button,
    counter: gtk::Label,
    current_id: RefCell<Option<Uuid>>,
}

struct GallerySelectionState {
    active: Cell<bool>,
    busy: Cell<bool>,
    ids: RefCell<HashSet<Uuid>>,
    card_toggles: RefCell<
        Vec<(
            glib::WeakRef<gtk::ListItem>,
            glib::WeakRef<gtk::CheckButton>,
        )>,
    >,
    select: gtk::Button,
    normal_controls: gtk::Box,
    controls: gtk::Box,
    select_all: gtk::Button,
    clear: gtk::Button,
    counter: gtk::Label,
    remove: gtk::Button,
    delete: gtk::Button,
    cancel: gtk::Button,
}

#[derive(Default)]
struct GalleryContextMenuRegistry {
    entries: RefCell<Vec<GalleryContextMenuEntry>>,
}

struct GalleryContextMenuEntry {
    list_item: glib::WeakRef<gtk::ListItem>,
    popover: glib::WeakRef<gtk::Popover>,
    record: std::rc::Weak<RefCell<Option<CaptureRecord>>>,
}

impl GalleryContextMenuRegistry {
    fn register(
        &self,
        list_item: &gtk::ListItem,
        popover: &gtk::Popover,
        record: &Rc<RefCell<Option<CaptureRecord>>>,
    ) {
        self.entries.borrow_mut().push(GalleryContextMenuEntry {
            list_item: list_item.downgrade(),
            popover: popover.downgrade(),
            record: Rc::downgrade(record),
        });
    }

    fn reset(&self, target: &gtk::ListItem, teardown: bool) {
        self.entries.borrow_mut().retain(|entry| {
            let Some(list_item) = entry.list_item.upgrade() else {
                return false;
            };
            if list_item != *target {
                return true;
            }
            if let Some(record) = entry.record.upgrade() {
                record.replace(None);
            }
            if let Some(popover) = entry.popover.upgrade() {
                popover.popdown();
                if teardown && popover.parent().is_some() {
                    popover.unparent();
                }
            }
            !teardown
        });
    }

    fn close_others(&self, target: &gtk::ListItem) {
        self.entries.borrow_mut().retain(|entry| {
            let Some(list_item) = entry.list_item.upgrade() else {
                return false;
            };
            if list_item != *target {
                if let Some(record) = entry.record.upgrade() {
                    record.replace(None);
                }
                if let Some(popover) = entry.popover.upgrade() {
                    popover.popdown();
                }
            }
            true
        });
    }
}

impl GallerySelectionState {
    fn new() -> Rc<Self> {
        let select = gtk::Button::with_label(&gettext("Select"));
        select.set_widget_name("gallery-select");
        let normal_controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .vexpand(true)
            .build();

        let title = gtk::Label::builder()
            .label(gettext("Selection"))
            .css_classes(["heading"])
            .xalign(0.0)
            .build();
        let counter = gtk::Label::builder()
            .css_classes(["dim-label"])
            .xalign(0.0)
            .build();
        counter.set_widget_name("gallery-selection-count");
        let select_all = gtk::Button::with_label(&gettext("Select All"));
        select_all.set_widget_name("gallery-select-all");
        let clear = gtk::Button::with_label(&gettext("Clear Selection"));
        clear.set_widget_name("gallery-clear-selection");
        let remove = gtk::Button::with_label(&gettext("Remove Selected from Gallery"));
        remove.set_widget_name("gallery-remove-selected");
        remove.set_tooltip_text(Some(&gettext("Keep the selected original files on disk")));
        let delete = gtk::Button::with_label(&gettext("Delete Selected from Disk"));
        delete.set_widget_name("gallery-delete-selected");
        delete.set_tooltip_text(Some(&gettext(
            "Permanently delete the selected capture files from disk",
        )));
        delete.add_css_class("destructive-action");
        let cancel = gtk::Button::with_label(&gettext("Cancel"));
        cancel.set_widget_name("gallery-cancel-selection");

        let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        spacer.set_vexpand(true);
        let controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .vexpand(true)
            .build();
        controls.set_widget_name("gallery-select-mode");
        controls.append(&title);
        controls.append(&counter);
        controls.append(&select_all);
        controls.append(&clear);
        controls.append(&spacer);
        controls.append(&remove);
        controls.append(&delete);
        controls.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        controls.append(&cancel);

        let state = Rc::new(Self {
            active: Cell::new(false),
            busy: Cell::new(false),
            ids: RefCell::new(HashSet::new()),
            card_toggles: RefCell::new(Vec::new()),
            select,
            normal_controls,
            controls,
            select_all,
            clear,
            counter,
            remove,
            delete,
            cancel,
        });
        state.sync();
        state
    }

    fn sync(&self) {
        let count = self.ids.borrow().len();
        let active = self.active.get();
        let available = active && !self.busy.get();
        self.normal_controls.set_visible(!active);
        self.controls.set_visible(active);
        self.counter
            .set_label(&format!("{}: {count}", gettext("Selected captures")));
        self.select_all.set_sensitive(available);
        self.clear.set_sensitive(available && count > 0);
        self.remove.set_sensitive(available && count > 0);
        self.delete.set_sensitive(available && count > 0);
        self.cancel.set_sensitive(available);
        self.sync_card_toggles();
    }

    fn register_card_toggle(&self, list_item: &gtk::ListItem, toggle: &gtk::CheckButton) {
        self.card_toggles
            .borrow_mut()
            .push((list_item.downgrade(), toggle.downgrade()));
    }

    fn sync_card_toggles(&self) {
        let active = self.active.get();
        let selected_ids = self.ids.borrow().clone();
        self.card_toggles
            .borrow_mut()
            .retain(|(list_item, toggle)| {
                let (Some(list_item), Some(toggle)) = (list_item.upgrade(), toggle.upgrade())
                else {
                    return false;
                };
                let selected = list_item
                    .item()
                    .and_downcast::<glib::BoxedAnyObject>()
                    .is_some_and(|item| selected_ids.contains(&item.borrow::<CaptureRecord>().id));
                toggle.set_visible(active);
                toggle.set_active(selected);
                true
            });
    }

    fn set_active(&self, active: bool, model: &gio::ListStore) {
        self.active.set(active);
        self.busy.set(false);
        if !active {
            self.ids.borrow_mut().clear();
        }
        self.sync();
        refresh_gallery_cards(model);
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.sync();
    }

    fn toggle(&self, id: Uuid) {
        let mut ids = self.ids.borrow_mut();
        if !ids.insert(id) {
            ids.remove(&id);
        }
        drop(ids);
        self.sync();
    }

    fn set_selected(&self, id: Uuid, selected: bool) {
        let mut ids = self.ids.borrow_mut();
        let changed = if selected {
            ids.insert(id)
        } else {
            ids.remove(&id)
        };
        drop(ids);
        if changed {
            self.sync();
        }
    }

    fn replace_ids(&self, ids: impl IntoIterator<Item = Uuid>) {
        self.ids.replace(ids.into_iter().collect());
        self.sync();
    }

    fn selected_ids(&self) -> Vec<Uuid> {
        self.ids.borrow().iter().copied().collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreviewCommand {
    Back,
    Previous,
    Next,
}

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
    let stack = gtk::Stack::new();
    let gallery_selection = GallerySelectionState::new();

    let selection = gtk::SingleSelection::new(Some(model.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    selection.set_selected(gtk::INVALID_LIST_POSITION);
    let factory = gallery_factory(
        Rc::clone(&gallery_selection),
        &selection,
        &model,
        Rc::clone(&controller),
        &stack,
        &paths,
    );
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
    stack.add_named(&empty, Some("empty"));

    let detail = detail_pane(
        &selection,
        &model,
        Rc::clone(&controller),
        &stack,
        &paths,
        Rc::clone(&gallery_selection),
    );
    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&scroll)
        .end_child(&detail)
        .resize_end_child(false)
        .shrink_end_child(false)
        .build();
    stack.add_named(&paned, Some("gallery"));
    let preview = build_capture_preview(&stack, &selection, &model, Rc::clone(&controller), &paths);
    stack.add_named(&preview.root, Some("preview"));
    grid.connect_activate({
        let stack = stack.clone();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let paths = paths.clone();
        let preview = Rc::clone(&preview);
        let gallery_selection = Rc::clone(&gallery_selection);
        move |_, position| {
            if gallery_selection.active.get() {
                if let Some(record) = record_at(&model, position) {
                    gallery_selection.toggle(record.id);
                    selection.set_selected(gtk::INVALID_LIST_POSITION);
                    refresh_gallery_card(&model, position);
                }
                return;
            }
            if show_preview_at(position, &selection, &model, &controller, &paths, &preview) {
                stack.set_visible_child_name("preview");
                preview.back.grab_focus();
            }
        }
    });
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
        let selection = selection.clone();
        let preview = Rc::clone(&preview);
        let gallery_selection = Rc::clone(&gallery_selection);
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
                    refresh_active_preview(
                        &stack,
                        &selection,
                        &model,
                        &controller,
                        &paths,
                        &preview,
                    );
                    reconcile_gallery_selection(&controller, &gallery_selection, &model);
                    if let GalleryEvent::Select(id) = event {
                        select_record(&selection, &model, id);
                    }
                }
            }
        }
    });
    Ok(stack.upcast())
}

fn gallery_factory(
    selection_state: Rc<GallerySelectionState>,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: Rc<RefCell<GalleryController>>,
    stack: &gtk::Stack,
    paths: &AppPaths,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let context_menus = Rc::new(GalleryContextMenuRegistry::default());
    let setup_selection_state = Rc::clone(&selection_state);
    let setup_selection = selection.clone();
    let setup_model = model.clone();
    let setup_controller = Rc::clone(&controller);
    let setup_stack = stack.downgrade();
    let setup_paths = paths.clone();
    let setup_context_menus = Rc::clone(&context_menus);
    factory.connect_setup(move |_, list_item| {
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
        let selection_toggle = gtk::CheckButton::builder()
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_top(10)
            .margin_end(10)
            .build();
        selection_toggle.set_widget_name("gallery-card-selection-toggle");
        selection_toggle.add_css_class("osd");
        setup_selection_state.register_card_toggle(list_item, &selection_toggle);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&card));
        overlay.add_overlay(&selection_toggle);
        selection_toggle.connect_toggled({
            let list_item = list_item.downgrade();
            let selection_state = Rc::downgrade(&setup_selection_state);
            move |toggle| {
                let Some(selection_state) = selection_state.upgrade() else {
                    return;
                };
                if !selection_state.active.get() {
                    return;
                }
                let Some(item) = list_item
                    .upgrade()
                    .and_then(|list_item| list_item.item())
                    .and_downcast::<glib::BoxedAnyObject>()
                else {
                    return;
                };
                let id = item.borrow::<CaptureRecord>().id;
                selection_state.set_selected(id, toggle.is_active());
            }
        });
        install_drag_source(&card, {
            let list_item = list_item.downgrade();
            move || {
                list_item
                    .upgrade()
                    .and_then(|list_item| list_item.item())
                    .and_downcast::<glib::BoxedAnyObject>()
                    .map(|item| item.borrow::<CaptureRecord>().clone())
            }
        });
        install_gallery_context_menu(
            list_item,
            &overlay,
            &setup_selection,
            &setup_model,
            Rc::clone(&setup_controller),
            setup_stack.clone(),
            &setup_paths,
            Rc::clone(&setup_selection_state),
            Rc::clone(&setup_context_menus),
        );
        list_item.set_child(Some(&overlay));
    });
    factory.connect_bind(move |_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(item) = list_item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let Some(overlay) = list_item.child().and_downcast::<gtk::Overlay>() else {
            return;
        };
        let Some(card) = overlay.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(selection_toggle) = card.next_sibling().and_downcast::<gtk::CheckButton>() else {
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
        let selection_active = selection_state.active.get();
        selection_toggle.set_visible(selection_active);
        selection_toggle.set_active(selection_state.ids.borrow().contains(&record.id));
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
        super::set_accessible_label(
            &selection_toggle,
            &format!("{} — {}", gettext("Select capture"), timestamp.text()),
        );
    });
    factory.connect_unbind({
        let context_menus = Rc::clone(&context_menus);
        move |_, list_item| {
            if let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() {
                context_menus.reset(list_item, false);
            }
        }
    });
    factory.connect_teardown({
        let context_menus = Rc::clone(&context_menus);
        move |_, list_item| {
            if let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() {
                context_menus.reset(list_item, true);
            }
        }
    });
    factory
}

#[allow(clippy::too_many_arguments)]
fn install_gallery_context_menu(
    list_item: &gtk::ListItem,
    anchor: &gtk::Overlay,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: Rc<RefCell<GalleryController>>,
    stack: glib::WeakRef<gtk::Stack>,
    paths: &AppPaths,
    selection_state: Rc<GallerySelectionState>,
    context_menus: Rc<GalleryContextMenuRegistry>,
) {
    let select_action = context_menu_button(&gettext("Select Capture"), "gallery-context-select");
    let copy_action = context_menu_button(&gettext("Copy"), "gallery-context-copy");
    let edit_action = context_menu_button(&gettext("Edit"), "gallery-context-edit");
    let reveal_action = context_menu_button(&gettext("Reveal in Folder"), "gallery-context-reveal");
    let remove_action = context_menu_button(
        &gettext("Remove This Capture from Gallery"),
        "gallery-context-remove",
    );
    let delete_action = context_menu_button(
        &gettext("Delete This Capture from Disk"),
        "gallery-context-delete",
    );
    delete_action.add_css_class("destructive-action");

    for (button, accessible_label) in [
        (&copy_action, gettext("Copy this capture")),
        (&edit_action, gettext("Edit this screenshot")),
        (&reveal_action, gettext("Reveal this capture in its folder")),
        (
            &remove_action,
            gettext("Remove this capture from the gallery"),
        ),
        (
            &delete_action,
            gettext("Permanently delete this capture from disk"),
        ),
    ] {
        super::set_accessible_label(button, &accessible_label);
    }

    let menu_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    for button in [&select_action, &copy_action, &edit_action, &reveal_action] {
        menu_content.append(button);
    }
    menu_content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    menu_content.append(&remove_action);
    menu_content.append(&delete_action);

    let popover = gtk::Popover::builder()
        .autohide(true)
        .has_arrow(true)
        .child(&menu_content)
        .build();
    popover.set_widget_name("gallery-context-menu");
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(anchor);
    super::set_accessible_label(&popover, &gettext("Capture actions"));

    // This record snapshot is replaced only when the user opens the menu. Every action reads the
    // snapshot rather than the list item's current position, which may change when GridView recycles
    // its virtualized rows.
    let context_record = Rc::new(RefCell::new(None::<CaptureRecord>));
    context_menus.register(list_item, &popover, &context_record);
    popover.connect_closed({
        let context_record = Rc::downgrade(&context_record);
        move |_| {
            if let Some(context_record) = context_record.upgrade() {
                context_record.replace(None);
            }
        }
    });
    let secondary_click = gtk::GestureClick::new();
    secondary_click.set_button(gdk::BUTTON_SECONDARY);
    secondary_click.set_propagation_phase(gtk::PropagationPhase::Capture);
    secondary_click.connect_pressed({
        let list_item = list_item.downgrade();
        let popover = popover.downgrade();
        let context_record = Rc::clone(&context_record);
        let selection_state = Rc::downgrade(&selection_state);
        let select_action = select_action.clone();
        let edit_action = edit_action.clone();
        let remove_action = remove_action.clone();
        let delete_action = delete_action.clone();
        let context_menus = Rc::downgrade(&context_menus);
        move |gesture, _, x, y| {
            let (Some(list_item), Some(popover), Some(selection_state), Some(context_menus)) = (
                list_item.upgrade(),
                popover.upgrade(),
                selection_state.upgrade(),
                context_menus.upgrade(),
            ) else {
                return;
            };
            let Some(item) = list_item.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let record = item.borrow::<CaptureRecord>().clone();
            let selected =
                selection_state.active.get() && selection_state.ids.borrow().contains(&record.id);
            let select_label = if selected {
                gettext("Deselect Capture")
            } else {
                gettext("Select Capture")
            };
            select_action.set_label(&select_label);
            super::set_accessible_label(&select_action, &select_label);
            select_action.set_sensitive(!selection_state.busy.get());
            edit_action.set_sensitive(record.kind == CaptureKind::Screenshot);
            remove_action.set_sensitive(!selection_state.busy.get());
            delete_action.set_sensitive(!selection_state.busy.get());
            context_menus.close_others(&list_item);
            context_record.replace(Some(record));
            popover.set_pointing_to(Some(&gdk::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                1,
                1,
            )));
            popover.popup();
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    anchor.add_controller(secondary_click);

    select_action.connect_clicked({
        let popover = popover.downgrade();
        let context_record = Rc::clone(&context_record);
        let selection_state = Rc::downgrade(&selection_state);
        let selection = selection.clone();
        let model = model.clone();
        move |_| {
            let Some(record) = context_record.borrow().clone() else {
                return;
            };
            let Some(selection_state) = selection_state.upgrade() else {
                return;
            };
            let was_selected =
                selection_state.active.get() && selection_state.ids.borrow().contains(&record.id);
            if !selection_state.active.get() {
                selection.set_selected(gtk::INVALID_LIST_POSITION);
                selection_state.set_active(true, &model);
            }
            selection_state.set_selected(record.id, !was_selected);
            if let Some(position) = record_position(&model, record.id) {
                refresh_gallery_card(&model, position);
            }
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        }
    });
    copy_action.connect_clicked({
        let popover = popover.downgrade();
        let context_record = Rc::clone(&context_record);
        move |_| {
            if let Some(record) = context_record.borrow().clone() {
                copy_capture(&record);
            }
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        }
    });
    edit_action.connect_clicked({
        let popover = popover.downgrade();
        let context_record = Rc::clone(&context_record);
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let stack = stack.clone();
        let paths = paths.clone();
        move |_| {
            if let (Some(record), Some(stack)) = (context_record.borrow().clone(), stack.upgrade())
                && record.kind == CaptureKind::Screenshot
            {
                open_capture_editor(record, &selection, &model, &controller, &stack, &paths);
            }
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        }
    });
    reveal_action.connect_clicked({
        let popover = popover.downgrade();
        let context_record = Rc::clone(&context_record);
        move |_| {
            if let Some(record) = context_record.borrow().clone() {
                reveal_capture(&record);
            }
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        }
    });
    remove_action.connect_clicked({
        let popover = popover.downgrade();
        let context_record = Rc::clone(&context_record);
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let stack = stack.clone();
        let selection_state = Rc::downgrade(&selection_state);
        move |button| {
            let Some(record) = context_record.borrow().clone() else {
                return;
            };
            let Some(selection_state) = selection_state.upgrade() else {
                return;
            };
            let Some(stack) = stack.upgrade() else {
                return;
            };
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            confirm_context_deletion(
                button,
                &selection,
                &model,
                &controller,
                &stack,
                &selection_state,
                record,
                DeleteMode::GalleryOnly,
            );
        }
    });
    delete_action.connect_clicked({
        let popover = popover.downgrade();
        let context_record = Rc::clone(&context_record);
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let stack = stack.clone();
        let selection_state = Rc::downgrade(&selection_state);
        move |button| {
            let Some(record) = context_record.borrow().clone() else {
                return;
            };
            let Some(selection_state) = selection_state.upgrade() else {
                return;
            };
            let Some(stack) = stack.upgrade() else {
                return;
            };
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            confirm_context_deletion(
                button,
                &selection,
                &model,
                &controller,
                &stack,
                &selection_state,
                record,
                DeleteMode::GalleryAndFile,
            );
        }
    });
}

fn context_menu_button(label: &str, widget_name: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.set_widget_name(widget_name);
    button.add_css_class("flat");
    button.set_hexpand(true);
    button
}

fn build_capture_preview(
    stack: &gtk::Stack,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: Rc<RefCell<GalleryController>>,
    paths: &AppPaths,
) -> Rc<CapturePreview> {
    let picture = gtk::Picture::builder()
        .can_shrink(true)
        .content_fit(gtk::ContentFit::Contain)
        .hexpand(true)
        .vexpand(true)
        .build();
    super::set_accessible_label(&picture, &gettext("Capture preview"));
    let image_exit_label = gettext("Capture preview — click to return to the gallery");
    let image_exit = gtk::Button::builder()
        .child(&picture)
        .tooltip_text(&image_exit_label)
        .hexpand(true)
        .vexpand(true)
        .build();
    image_exit.set_widget_name("preview-image");
    image_exit.add_css_class("flat");
    super::set_accessible_label(&image_exit, &image_exit_label);

    let previous = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text(gettext("Previous capture"))
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Center)
        .margin_start(12)
        .build();
    let next = gtk::Button::builder()
        .icon_name("go-next-symbolic")
        .tooltip_text(gettext("Next capture"))
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .margin_end(12)
        .build();
    previous.set_widget_name("preview-previous");
    next.set_widget_name("preview-next");
    for (button, label) in [
        (&previous, gettext("Previous capture")),
        (&next, gettext("Next capture")),
    ] {
        button.add_css_class("circular");
        button.add_css_class("osd");
        super::set_accessible_label(button, &label);
    }
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&image_exit));
    overlay.add_overlay(&previous);
    overlay.add_overlay(&next);

    let back_label = gettext("Back to Gallery");
    let back_content = adw::ButtonContent::builder()
        .icon_name("go-previous-symbolic")
        .label(&back_label)
        .build();
    let back = gtk::Button::builder().child(&back_content).build();
    back.set_tooltip_text(Some(&gettext("Close the capture preview")));
    back.set_widget_name("preview-back");
    super::set_accessible_label(&back, &back_label);
    let close_label = gettext("Close the capture preview");
    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text(&close_label)
        .build();
    close.set_widget_name("preview-close");
    close.add_css_class("flat");
    super::set_accessible_label(&close, &close_label);
    let title = gtk::Label::builder()
        .label(gettext("Capture preview"))
        .css_classes(["heading"])
        .build();
    let counter = gtk::Label::builder().css_classes(["dim-label"]).build();
    counter.set_widget_name("preview-counter");
    let title_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    title_box.append(&title);
    title_box.append(&counter);
    let top = gtk::CenterBox::builder()
        .orientation(gtk::Orientation::Horizontal)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    top.set_start_widget(Some(&back));
    top.set_center_widget(Some(&title_box));
    top.set_end_widget(Some(&close));
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    root.set_widget_name("klypse-capture-preview");
    root.append(&top);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&overlay);

    let preview = Rc::new(CapturePreview {
        root,
        back,
        close,
        picture,
        previous,
        next,
        counter,
        current_id: RefCell::new(None),
    });

    preview.back.connect_clicked({
        let preview = Rc::downgrade(&preview);
        let stack = stack.downgrade();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let paths = paths.clone();
        move |_| {
            if let (Some(preview), Some(stack)) = (preview.upgrade(), stack.upgrade()) {
                run_preview_command(
                    PreviewCommand::Back,
                    &stack,
                    &selection,
                    &model,
                    &controller,
                    &paths,
                    &preview,
                );
            }
        }
    });
    preview.close.connect_clicked({
        let preview = Rc::downgrade(&preview);
        let stack = stack.downgrade();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let paths = paths.clone();
        move |_| {
            if let (Some(preview), Some(stack)) = (preview.upgrade(), stack.upgrade()) {
                run_preview_command(
                    PreviewCommand::Back,
                    &stack,
                    &selection,
                    &model,
                    &controller,
                    &paths,
                    &preview,
                );
            }
        }
    });
    image_exit.connect_clicked({
        let preview = Rc::downgrade(&preview);
        let stack = stack.downgrade();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let paths = paths.clone();
        move |_| {
            if let (Some(preview), Some(stack)) = (preview.upgrade(), stack.upgrade()) {
                run_preview_command(
                    PreviewCommand::Back,
                    &stack,
                    &selection,
                    &model,
                    &controller,
                    &paths,
                    &preview,
                );
            }
        }
    });
    preview.previous.connect_clicked({
        let preview = Rc::downgrade(&preview);
        let stack = stack.downgrade();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let paths = paths.clone();
        move |_| {
            if let (Some(preview), Some(stack)) = (preview.upgrade(), stack.upgrade()) {
                run_preview_command(
                    PreviewCommand::Previous,
                    &stack,
                    &selection,
                    &model,
                    &controller,
                    &paths,
                    &preview,
                );
            }
        }
    });
    preview.next.connect_clicked({
        let preview = Rc::downgrade(&preview);
        let stack = stack.downgrade();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let paths = paths.clone();
        move |_| {
            if let (Some(preview), Some(stack)) = (preview.upgrade(), stack.upgrade()) {
                run_preview_command(
                    PreviewCommand::Next,
                    &stack,
                    &selection,
                    &model,
                    &controller,
                    &paths,
                    &preview,
                );
            }
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let preview = Rc::downgrade(&preview);
        let stack = stack.downgrade();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let paths = paths.clone();
        move |_, key, _, _| {
            let Some(command) = preview_command_for_key(key) else {
                return glib::Propagation::Proceed;
            };
            if let (Some(preview), Some(stack)) = (preview.upgrade(), stack.upgrade()) {
                run_preview_command(
                    command,
                    &stack,
                    &selection,
                    &model,
                    &controller,
                    &paths,
                    &preview,
                );
            }
            glib::Propagation::Stop
        }
    });
    preview.root.add_controller(keys);
    preview
}

fn run_preview_command(
    command: PreviewCommand,
    stack: &gtk::Stack,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: &Rc<RefCell<GalleryController>>,
    paths: &AppPaths,
    preview: &CapturePreview,
) {
    match command {
        PreviewCommand::Back => {
            preview.current_id.replace(None);
            stack.set_visible_child_name(if model.n_items() == 0 {
                "empty"
            } else {
                "gallery"
            });
        }
        PreviewCommand::Previous => {
            navigate_preview(-1, selection, model, controller, paths, preview)
        }
        PreviewCommand::Next => navigate_preview(1, selection, model, controller, paths, preview),
    }
}

fn preview_command_for_key(key: gdk::Key) -> Option<PreviewCommand> {
    match key {
        gdk::Key::Escape => Some(PreviewCommand::Back),
        gdk::Key::Left | gdk::Key::KP_Left => Some(PreviewCommand::Previous),
        gdk::Key::Right | gdk::Key::KP_Right => Some(PreviewCommand::Next),
        _ => None,
    }
}

fn navigate_preview(
    direction: i32,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: &Rc<RefCell<GalleryController>>,
    paths: &AppPaths,
    preview: &CapturePreview,
) {
    let Some(id) = *preview.current_id.borrow() else {
        return;
    };
    let Some(current) = record_position(model, id) else {
        return;
    };
    let Some(target) = adjacent_preview_index(
        current,
        model.n_items(),
        direction,
        controller.borrow().has_more(),
    ) else {
        return;
    };
    show_preview_at(target, selection, model, controller, paths, preview);
}

fn adjacent_preview_index(
    current: u32,
    item_count: u32,
    direction: i32,
    has_more: bool,
) -> Option<u32> {
    if direction < 0 {
        return current.checked_sub(1);
    }
    if direction == 0 {
        return None;
    }
    let next = current.checked_add(1)?;
    (next < item_count || has_more).then_some(next)
}

fn show_preview_at(
    position: u32,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: &Rc<RefCell<GalleryController>>,
    paths: &AppPaths,
    preview: &CapturePreview,
) -> bool {
    while position >= model.n_items() && controller.borrow().has_more() {
        if !load_next_preview_page(model, controller, paths) {
            break;
        }
    }
    let Some(item) = model.item(position).and_downcast::<glib::BoxedAnyObject>() else {
        return false;
    };
    let record = item.borrow::<CaptureRecord>().clone();
    set_preview_picture(&preview.picture, &record);
    preview.current_id.replace(Some(record.id));
    selection.set_selected(position);
    preview.previous.set_sensitive(position > 0);
    let has_more = controller.borrow().has_more();
    preview
        .next
        .set_sensitive(position.saturating_add(1) < model.n_items() || has_more);
    preview.counter.set_label(&format!(
        "{} / {}{}",
        position.saturating_add(1),
        model.n_items(),
        if has_more { "+" } else { "" }
    ));
    true
}

fn load_next_preview_page(
    model: &gio::ListStore,
    controller: &Rc<RefCell<GalleryController>>,
    paths: &AppPaths,
) -> bool {
    let before = controller.borrow().items().len();
    let next_page = controller.borrow_mut().load_next();
    match next_page {
        Ok(page) => {
            let added = &page.items[before.min(page.items.len())..];
            append_records(model, added);
            schedule_missing_thumbnails(added, paths, controller.borrow().store(), model);
            !added.is_empty()
        }
        Err(error) => {
            tracing::warn!(%error, "next gallery page could not be loaded for preview");
            false
        }
    }
}

fn record_position(model: &gio::ListStore, id: Uuid) -> Option<u32> {
    (0..model.n_items()).find(|&position| {
        model
            .item(position)
            .and_downcast::<glib::BoxedAnyObject>()
            .is_some_and(|item| item.borrow::<CaptureRecord>().id == id)
    })
}

fn refresh_active_preview(
    stack: &gtk::Stack,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: &Rc<RefCell<GalleryController>>,
    paths: &AppPaths,
    preview: &CapturePreview,
) {
    if stack.visible_child_name().as_deref() != Some("preview") {
        return;
    }
    let Some(id) = *preview.current_id.borrow() else {
        return;
    };
    loop {
        if let Some(position) = record_position(model, id) {
            show_preview_at(position, selection, model, controller, paths, preview);
            return;
        }
        if !controller.borrow().has_more() || !load_next_preview_page(model, controller, paths) {
            run_preview_command(
                PreviewCommand::Back,
                stack,
                selection,
                model,
                controller,
                paths,
                preview,
            );
            return;
        }
    }
}

fn set_preview_picture(picture: &gtk::Picture, record: &CaptureRecord) {
    if record.kind == CaptureKind::Screenshot && record.annotation_json.is_some() {
        match render_saved_preview(record) {
            Ok(texture) => {
                picture.set_paintable(Some(&texture));
                return;
            }
            Err(error) => {
                tracing::warn!(
                    capture_id = %record.id,
                    %error,
                    "saved annotations could not be rendered in the gallery preview"
                );
            }
        }
    }

    let preview_path = match record.kind {
        CaptureKind::Video => record.thumbnail_path.as_ref().or(Some(&record.path)),
        CaptureKind::Screenshot | CaptureKind::Gif => Some(&record.path),
    };
    picture.set_filename(preview_path);
}

fn render_saved_preview(record: &CaptureRecord) -> Result<gdk::MemoryTexture, String> {
    let source = fs::read(&record.path).map_err(|error| error.to_string())?;
    let controller = EditorController::open(record).map_err(|error| error.to_string())?;
    let rendered = Renderer::default()
        .render_to_rgba(&source, controller.document())
        .map_err(|error| error.to_string())?;
    let width = rendered.width();
    let height = rendered.height();
    let bytes = glib::Bytes::from_owned(rendered.into_raw());
    Ok(gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        width as usize * 4,
    ))
}

fn detail_pane(
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: Rc<RefCell<GalleryController>>,
    stack: &gtk::Stack,
    paths: &AppPaths,
    gallery_selection: Rc<GallerySelectionState>,
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
    for button in [&copy, &edit, &reveal] {
        button.set_sensitive(false);
        gallery_selection.normal_controls.append(button);
    }

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    gallery_selection.normal_controls.append(&spacer);
    gallery_selection
        .normal_controls
        .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    gallery_selection
        .normal_controls
        .append(&gallery_selection.select);
    gallery_selection.select.set_sensitive(model.n_items() > 0);
    pane.append(&gallery_selection.normal_controls);
    pane.append(&gallery_selection.controls);
    model.connect_items_changed({
        let select = gallery_selection.select.clone();
        move |model, _, _, _| select.set_sensitive(model.n_items() > 0)
    });

    sync_detail_actions(selection, &copy, &edit, &reveal);
    selection.connect_selected_item_notify({
        let copy = copy.clone();
        let edit = edit.clone();
        let reveal = reveal.clone();
        move |selection| {
            sync_detail_actions(selection, &copy, &edit, &reveal);
        }
    });
    copy.connect_clicked({
        let selection = selection.clone();
        move |_| {
            if let Some(record) = selected_record(&selection) {
                copy_capture(&record);
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
            if let Some(record) = selected_record(&selection) {
                open_capture_editor(record, &selection, &model, &controller, &stack, &paths);
            }
        }
    });
    reveal.connect_clicked({
        let selection = selection.clone();
        move |_| {
            if let Some(record) = selected_record(&selection) {
                reveal_capture(&record);
            }
        }
    });
    gallery_selection.select.connect_clicked({
        let selection = selection.clone();
        let model = model.clone();
        let gallery_selection = Rc::downgrade(&gallery_selection);
        move |_| {
            let Some(gallery_selection) = gallery_selection.upgrade() else {
                return;
            };
            selection.set_selected(gtk::INVALID_LIST_POSITION);
            gallery_selection.set_active(true, &model);
        }
    });
    gallery_selection.select_all.connect_clicked({
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let gallery_selection = Rc::downgrade(&gallery_selection);
        move |button| {
            let Some(gallery_selection) = gallery_selection.upgrade() else {
                return;
            };
            let parent = button.root().and_downcast::<gtk::Window>();
            match controller.borrow().all_ids() {
                Ok(ids) => {
                    gallery_selection.replace_ids(ids);
                    refresh_gallery_cards(&model);
                }
                Err(error) => present_gallery_error(parent.as_ref(), &error.to_string()),
            }
        }
    });
    gallery_selection.clear.connect_clicked({
        let model = model.clone();
        let gallery_selection = Rc::downgrade(&gallery_selection);
        move |_| {
            let Some(gallery_selection) = gallery_selection.upgrade() else {
                return;
            };
            gallery_selection.replace_ids([]);
            refresh_gallery_cards(&model);
        }
    });
    gallery_selection.cancel.connect_clicked({
        let selection = selection.clone();
        let model = model.clone();
        let gallery_selection = Rc::downgrade(&gallery_selection);
        move |_| {
            let Some(gallery_selection) = gallery_selection.upgrade() else {
                return;
            };
            selection.set_selected(gtk::INVALID_LIST_POSITION);
            gallery_selection.set_active(false, &model);
        }
    });
    gallery_selection.remove.connect_clicked({
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let stack = stack.clone();
        let gallery_selection = Rc::downgrade(&gallery_selection);
        move |button| {
            let Some(gallery_selection) = gallery_selection.upgrade() else {
                return;
            };
            confirm_selected_deletion(
                button,
                &selection,
                &model,
                &controller,
                &stack,
                &gallery_selection,
                DeleteMode::GalleryOnly,
            );
        }
    });
    gallery_selection.delete.connect_clicked({
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(&controller);
        let stack = stack.clone();
        let gallery_selection = Rc::downgrade(&gallery_selection);
        move |button| {
            let Some(gallery_selection) = gallery_selection.upgrade() else {
                return;
            };
            confirm_selected_deletion(
                button,
                &selection,
                &model,
                &controller,
                &stack,
                &gallery_selection,
                DeleteMode::GalleryAndFile,
            );
        }
    });
    pane
}

fn copy_capture(record: &CaptureRecord) {
    if copy_record(record).is_err() {
        tracing::warn!(capture_id = %record.id, "capture could not be copied");
    }
}

#[allow(clippy::too_many_arguments)]
fn open_capture_editor(
    requested: CaptureRecord,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: &Rc<RefCell<GalleryController>>,
    stack: &gtk::Stack,
    paths: &AppPaths,
) {
    if requested.kind != CaptureKind::Screenshot {
        return;
    }
    let store = controller.borrow().store();
    let record = match store.get(&requested.id) {
        Ok(Some(record)) if record.kind == CaptureKind::Screenshot => record,
        Ok(Some(_)) => return,
        Ok(None) => {
            tracing::warn!(capture_id = %requested.id, "capture no longer exists");
            return;
        }
        Err(error) => {
            tracing::warn!(capture_id = %requested.id, %error, "capture could not be loaded for editing");
            return;
        }
    };
    let on_export = Rc::new({
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(controller);
        let stack = stack.clone();
        let paths = paths.clone();
        move |exported: CaptureRecord| {
            let refreshed = controller.borrow_mut().refresh();
            if let Ok(page) = refreshed {
                let store = controller.borrow().store();
                replace_records(&model, &page.items);
                schedule_missing_thumbnails(&page.items, &paths, store, &model);
                update_empty_state(&stack, model.n_items());
                select_record(&selection, &model, exported.id);
            }
        }
    });
    if let Err(error) = super::editor::present(record.clone(), store, paths.clone(), on_export) {
        tracing::warn!(capture_id = %record.id, %error, "capture editor could not be opened");
    }
}

fn reveal_capture(record: &CaptureRecord) {
    let directory = record.path.parent().unwrap_or(&record.path);
    let uri = gio::File::for_path(directory).uri();
    if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
        tracing::warn!(capture_id = %record.id, "capture folder could not be opened");
    }
}

#[allow(clippy::too_many_arguments)]
fn confirm_context_deletion(
    button: &gtk::Button,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: &Rc<RefCell<GalleryController>>,
    stack: &gtk::Stack,
    gallery_selection: &Rc<GallerySelectionState>,
    record: CaptureRecord,
    mode: DeleteMode,
) {
    let parent = button.root().and_downcast::<gtk::Window>();
    let ids = vec![record.id];
    let filename = record
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| record.path.to_str().unwrap_or("?"));
    let timestamp = record
        .created_at
        .with_timezone(&Local)
        .format("%x %X")
        .to_string();
    let identity = format!("{}\n{timestamp} — {filename}", gettext("Selected capture:"));
    let (title, detail, action) = match mode {
        DeleteMode::GalleryOnly => (
            gettext("Remove this capture?"),
            gettext(
                "This capture will be removed from Klypse, but the original file will stay on disk.",
            ),
            gettext("Remove This Capture from Gallery"),
        ),
        DeleteMode::GalleryAndFile => (
            gettext("Delete this file?"),
            gettext("This permanently deletes the capture file from disk."),
            gettext("Delete This Capture from Disk"),
        ),
    };
    let confirmation = format!("{identity}\n\n{detail}");
    let dialog = adw::MessageDialog::new(parent.as_ref(), Some(&title), Some(&confirmation));
    dialog.add_responses(&[("cancel", &gettext("Cancel")), ("confirm", &action)]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("cancel"));
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.choose(gtk::gio::Cancellable::NONE, {
        let parent = parent.clone();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(controller);
        let stack = stack.clone();
        let gallery_selection = Rc::clone(gallery_selection);
        let ids = ids.clone();
        move |response| {
            if response == "confirm" {
                spawn_gallery_deletion(
                    parent.clone(),
                    selection.clone(),
                    model.clone(),
                    Rc::clone(&controller),
                    stack.clone(),
                    Rc::clone(&gallery_selection),
                    ids.clone(),
                    mode,
                    false,
                );
            }
        }
    });
}

fn confirm_selected_deletion(
    button: &gtk::Button,
    selection: &gtk::SingleSelection,
    model: &gio::ListStore,
    controller: &Rc<RefCell<GalleryController>>,
    stack: &gtk::Stack,
    gallery_selection: &Rc<GallerySelectionState>,
    mode: DeleteMode,
) {
    let ids = gallery_selection.selected_ids();
    if ids.is_empty() {
        return;
    }
    let parent = button.root().and_downcast::<gtk::Window>();
    let count = format!("{}: {}", gettext("Selected captures"), ids.len());
    let (title, detail, action) = match mode {
        DeleteMode::GalleryOnly => (
            gettext("Remove selected captures?"),
            gettext(
                "The selected captures will be removed from Klypse, but the original files will stay on disk.",
            ),
            gettext("Remove Selected from Gallery"),
        ),
        DeleteMode::GalleryAndFile => (
            gettext("Delete selected files?"),
            gettext("The selected capture files will be permanently deleted from disk."),
            gettext("Delete Selected from Disk"),
        ),
    };
    let confirmation = format!("{count}\n\n{detail}");
    let dialog = adw::MessageDialog::new(parent.as_ref(), Some(&title), Some(&confirmation));
    dialog.add_responses(&[("cancel", &gettext("Cancel")), ("confirm", &action)]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("cancel"));
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.choose(gtk::gio::Cancellable::NONE, {
        let parent = parent.clone();
        let selection = selection.clone();
        let model = model.clone();
        let controller = Rc::clone(controller);
        let stack = stack.clone();
        let gallery_selection = Rc::clone(gallery_selection);
        move |response| {
            if response != "confirm" {
                return;
            }
            spawn_gallery_deletion(
                parent.clone(),
                selection.clone(),
                model.clone(),
                Rc::clone(&controller),
                stack.clone(),
                Rc::clone(&gallery_selection),
                ids.clone(),
                mode,
                true,
            );
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn spawn_gallery_deletion(
    parent: Option<gtk::Window>,
    selection: gtk::SingleSelection,
    model: gio::ListStore,
    controller: Rc<RefCell<GalleryController>>,
    stack: gtk::Stack,
    gallery_selection: Rc<GallerySelectionState>,
    ids: Vec<Uuid>,
    mode: DeleteMode,
    exit_selection_mode: bool,
) {
    if ids.is_empty() || gallery_selection.busy.get() {
        return;
    }
    gallery_selection.set_busy(true);
    let store = controller.borrow().store();
    glib::spawn_future_local(async move {
        let deletion_ids = ids.clone();
        let deletion = gio::spawn_blocking(move || store.delete_many(&deletion_ids, mode)).await;
        let refreshed = controller.borrow_mut().refresh();
        match refreshed {
            Ok(page) => {
                replace_records(&model, &page.items);
                update_empty_state(&stack, model.n_items());
            }
            Err(error) => present_gallery_error(parent.as_ref(), &error.to_string()),
        }

        match deletion {
            Ok(Ok(_)) => {
                selection.set_selected(gtk::INVALID_LIST_POSITION);
                if exit_selection_mode {
                    gallery_selection.set_active(false, &model);
                } else {
                    gallery_selection
                        .ids
                        .borrow_mut()
                        .retain(|id| !ids.contains(id));
                    reconcile_gallery_selection(&controller, &gallery_selection, &model);
                    gallery_selection.set_busy(false);
                }
            }
            Ok(Err(error)) => {
                present_gallery_error(parent.as_ref(), &error.to_string());
                reconcile_gallery_selection(&controller, &gallery_selection, &model);
                gallery_selection.set_busy(false);
            }
            Err(_) => {
                present_gallery_error(
                    parent.as_ref(),
                    &gettext("The gallery operation could not be completed"),
                );
                reconcile_gallery_selection(&controller, &gallery_selection, &model);
                gallery_selection.set_busy(false);
            }
        }
    });
}

fn present_gallery_error(parent: Option<&gtk::Window>, detail: &str) {
    let dialog = adw::MessageDialog::new(
        parent,
        Some(&gettext("Gallery could not be updated")),
        Some(detail),
    );
    dialog.add_response("close", &gettext("Close"));
    dialog.set_close_response("close");
    dialog.present();
}

fn sync_detail_actions(
    selection: &gtk::SingleSelection,
    copy: &gtk::Button,
    edit: &gtk::Button,
    reveal: &gtk::Button,
) {
    let record = selected_record(selection);
    let selected = record.is_some();
    copy.set_sensitive(selected);
    reveal.set_sensitive(selected);
    edit.set_sensitive(record.is_some_and(|record| record.kind == CaptureKind::Screenshot));
}

fn selected_record(selection: &gtk::SingleSelection) -> Option<CaptureRecord> {
    selection
        .selected_item()
        .and_downcast::<glib::BoxedAnyObject>()
        .map(|item| item.borrow::<CaptureRecord>().clone())
}

fn record_at(model: &gio::ListStore, position: u32) -> Option<CaptureRecord> {
    model
        .item(position)
        .and_downcast::<glib::BoxedAnyObject>()
        .map(|item| item.borrow::<CaptureRecord>().clone())
}

fn refresh_gallery_card(model: &gio::ListStore, position: u32) {
    if position < model.n_items() {
        model.items_changed(position, 1, 1);
    }
}

fn refresh_gallery_cards(model: &gio::ListStore) {
    let count = model.n_items();
    if count > 0 {
        model.items_changed(0, count, count);
    }
}

fn reconcile_gallery_selection(
    controller: &Rc<RefCell<GalleryController>>,
    gallery_selection: &GallerySelectionState,
    model: &gio::ListStore,
) {
    if !gallery_selection.active.get() {
        return;
    }
    match controller.borrow().all_ids() {
        Ok(ids) => {
            let existing = ids.into_iter().collect::<HashSet<_>>();
            gallery_selection
                .ids
                .borrow_mut()
                .retain(|id| existing.contains(id));
            gallery_selection.sync();
            refresh_gallery_cards(model);
        }
        Err(error) => {
            tracing::warn!(%error, "gallery selection could not be reconciled");
        }
    }
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
    if item_count > 0 && stack.visible_child_name().as_deref() == Some("preview") {
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::{PreviewCommand, adjacent_preview_index, preview_command_for_key};
    use gtk::gdk;

    #[test]
    fn preview_navigation_stops_at_loaded_boundaries() {
        assert_eq!(adjacent_preview_index(0, 0, -1, false), None);
        assert_eq!(adjacent_preview_index(0, 1, -1, false), None);
        assert_eq!(adjacent_preview_index(0, 1, 1, false), None);
        assert_eq!(adjacent_preview_index(1, 3, -1, false), Some(0));
        assert_eq!(adjacent_preview_index(1, 3, 1, false), Some(2));
        assert_eq!(adjacent_preview_index(2, 3, 1, false), None);
        assert_eq!(adjacent_preview_index(2, 3, 1, true), Some(3));
        assert_eq!(adjacent_preview_index(1, 3, 0, true), None);
    }

    #[test]
    fn preview_keyboard_commands_are_explicit() {
        assert_eq!(
            preview_command_for_key(gdk::Key::Escape),
            Some(PreviewCommand::Back)
        );
        assert_eq!(
            preview_command_for_key(gdk::Key::Left),
            Some(PreviewCommand::Previous)
        );
        assert_eq!(
            preview_command_for_key(gdk::Key::Right),
            Some(PreviewCommand::Next)
        );
        assert_eq!(preview_command_for_key(gdk::Key::space), None);
    }
}
