use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use chrono::{TimeDelta, Utc};
use gtk::prelude::*;
use klypse_app::{gallery::GalleryController, ui::gallery::build_with_paths};
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_storage::{AppPaths, CaptureRepository, CaptureStore, NewCaptureRecord, open_database};
use uuid::Uuid;

#[test]
fn one_thousand_gallery_items_page_quickly_without_duplicates() {
    let fixture = Fixture::new(1_000);
    let repository = CaptureRepository::new(open_database(&fixture.paths).unwrap());
    let mut controller = GalleryController::new(repository, 50).unwrap();

    let started = Instant::now();
    let first = controller.load_initial().unwrap();
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(first.items.len(), 50);

    while controller.has_more() {
        controller.load_next().unwrap();
    }
    let unique = controller
        .items()
        .iter()
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    assert_eq!(controller.items().len(), 1_000);
    assert_eq!(unique.len(), 1_000);
}

#[test]
fn gallery_ui_is_virtualized_and_inline_preview_navigates() {
    if gtk::init().is_err() {
        return;
    }
    let context = gtk::glib::MainContext::default();

    {
        let fixture = Fixture::new(1_000);
        let (_sender, receiver) = async_channel::unbounded();
        let gallery = build_with_paths(receiver, fixture.paths.clone()).unwrap();
        let window = gtk::Window::builder()
            .default_width(1_000)
            .default_height(700)
            .child(&gallery)
            .build();
        window.present();
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
        }

        let realized = descendants(&gallery)
            .into_iter()
            .filter(|widget| widget.has_css_class("gallery-card"))
            .count();
        assert!(realized > 0);
        assert!(realized <= 150, "realized {realized} gallery cards");
        let copy = button_by_label(&gallery, "Copy");
        let edit = button_by_label(&gallery, "Edit");
        assert!(!copy.is_sensitive());
        assert!(!edit.is_sensitive());
        let grid = descendants(&gallery)
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::GridView>().ok())
            .unwrap();
        let stack = gallery.clone().downcast::<gtk::Stack>().unwrap();
        let select = widget_by_name::<gtk::Button>(&gallery, "gallery-select");
        let select_mode = widget_by_name::<gtk::Box>(&gallery, "gallery-select-mode");
        let select_all = widget_by_name::<gtk::Button>(&gallery, "gallery-select-all");
        let clear_selection = widget_by_name::<gtk::Button>(&gallery, "gallery-clear-selection");
        let cancel_selection = widget_by_name::<gtk::Button>(&gallery, "gallery-cancel-selection");
        let selection_counter = widget_by_name::<gtk::Label>(&gallery, "gallery-selection-count");
        let delete_selected = widget_by_name::<gtk::Button>(&gallery, "gallery-delete-selected");
        assert!(select.is_sensitive());
        assert!(!select_mode.is_visible());
        assert!(
            descendants(&gallery)
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .all(|button| {
                    !matches!(
                        button.label().as_deref(),
                        Some("Remove from Gallery" | "Delete from Disk")
                    )
                })
        );

        select.emit_clicked();
        assert!(select_mode.is_visible());
        assert_eq!(selection_counter.text(), "Selected captures: 0");
        assert!(!delete_selected.is_sensitive());
        grid.emit_by_name::<()>("activate", &[&0_u32]);
        while context.pending() {
            context.iteration(false);
        }
        assert_eq!(stack.visible_child_name().as_deref(), Some("gallery"));
        assert_eq!(selection_counter.text(), "Selected captures: 1");
        assert!(delete_selected.is_sensitive());
        let card_toggles = descendants(&gallery)
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk::CheckButton>().ok())
            .filter(|toggle| toggle.is_visible())
            .collect::<Vec<_>>();
        assert!(!card_toggles.is_empty());
        assert_eq!(
            card_toggles
                .iter()
                .filter(|toggle| toggle.is_active())
                .count(),
            1
        );
        select_all.emit_clicked();
        assert_eq!(selection_counter.text(), "Selected captures: 1000");
        clear_selection.emit_clicked();
        assert_eq!(selection_counter.text(), "Selected captures: 0");
        assert!(!delete_selected.is_sensitive());
        cancel_selection.emit_clicked();
        assert!(!select_mode.is_visible());

        let selection = grid.model().and_downcast::<gtk::SingleSelection>().unwrap();
        assert!(!selection.is_autoselect());
        assert!(selection.can_unselect());
        assert_eq!(selection.selected(), gtk::INVALID_LIST_POSITION);

        selection.set_selected(0);
        assert!(copy.is_sensitive());
        assert!(edit.is_sensitive());
        let model = selection
            .model()
            .and_downcast::<gtk::gio::ListStore>()
            .unwrap();
        let first = model.item(0).unwrap();
        model.remove_all();
        model.append(&first);
        assert_eq!(selection.selected(), gtk::INVALID_LIST_POSITION);
        assert!(!copy.is_sensitive());
        assert!(!edit.is_sensitive());
        window.close();
    }

    {
        let fixture = Fixture::new(0);
        let (_sender, receiver) = async_channel::unbounded();
        let gallery = build_with_paths(receiver, fixture.paths).unwrap();
        assert!(!widget_by_name::<gtk::Button>(&gallery, "gallery-select").is_sensitive());
    }

    {
        let fixture = Fixture::new(3);
        let (_sender, receiver) = async_channel::unbounded();
        let gallery = build_with_paths(receiver, fixture.paths.clone()).unwrap();
        let stack = gallery.clone().downcast::<gtk::Stack>().unwrap();
        let grid = descendants(&gallery)
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::GridView>().ok())
            .unwrap();

        grid.emit_by_name::<()>("activate", &[&0_u32]);

        assert_eq!(stack.visible_child_name().as_deref(), Some("preview"));
        assert_eq!(
            descendants(&gallery)
                .iter()
                .filter(|widget| widget.widget_name() == "klypse-capture-preview")
                .count(),
            1
        );
        let previous = widget_by_name::<gtk::Button>(&gallery, "preview-previous");
        let next = widget_by_name::<gtk::Button>(&gallery, "preview-next");
        let counter = widget_by_name::<gtk::Label>(&gallery, "preview-counter");
        let back = widget_by_name::<gtk::Button>(&gallery, "preview-back");
        let close = widget_by_name::<gtk::Button>(&gallery, "preview-close");
        let image = widget_by_name::<gtk::Button>(&gallery, "preview-image");
        assert!(!previous.is_sensitive());
        assert!(next.is_sensitive());
        assert_eq!(counter.text(), "1 / 3");

        next.emit_clicked();
        assert!(previous.is_sensitive());
        assert!(next.is_sensitive());
        assert_eq!(counter.text(), "2 / 3");
        next.emit_clicked();
        assert!(previous.is_sensitive());
        assert!(!next.is_sensitive());
        assert_eq!(counter.text(), "3 / 3");

        grid.emit_by_name::<()>("activate", &[&1_u32]);
        assert_eq!(counter.text(), "2 / 3");
        assert_eq!(
            descendants(&gallery)
                .iter()
                .filter(|widget| widget.widget_name() == "klypse-capture-preview")
                .count(),
            1
        );
        back.emit_clicked();
        assert_eq!(stack.visible_child_name().as_deref(), Some("gallery"));

        grid.emit_by_name::<()>("activate", &[&1_u32]);
        assert_eq!(stack.visible_child_name().as_deref(), Some("preview"));
        close.emit_clicked();
        assert_eq!(stack.visible_child_name().as_deref(), Some("gallery"));

        grid.emit_by_name::<()>("activate", &[&1_u32]);
        assert_eq!(stack.visible_child_name().as_deref(), Some("preview"));
        image.emit_clicked();
        assert_eq!(stack.visible_child_name().as_deref(), Some("gallery"));
    }

    {
        let fixture = Fixture::new(51);
        let (_sender, receiver) = async_channel::unbounded();
        let gallery = build_with_paths(receiver, fixture.paths).unwrap();
        let grid = descendants(&gallery)
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::GridView>().ok())
            .unwrap();

        grid.emit_by_name::<()>("activate", &[&49_u32]);
        let next = widget_by_name::<gtk::Button>(&gallery, "preview-next");
        let counter = widget_by_name::<gtk::Label>(&gallery, "preview-counter");
        assert_eq!(counter.text(), "50 / 50+");

        next.emit_clicked();

        assert_eq!(counter.text(), "51 / 51");
        assert!(!next.is_sensitive());
    }
}

/// Covers the shape that binding a card can take while a selection is live:
/// reading the selected set while `toggled` writes it back re-enters the same
/// `RefCell`, which panics inside a GTK trampoline and aborts the process. The
/// app refreshes the gallery after every capture, so this path runs constantly.
///
/// Honest limitation: this exercises the refresh, not the re-entrant bind. It
/// passes with and without the guard, because GTK reuses the list items here
/// instead of rebinding them. It is kept as a smoke test of refresh-with-live-
/// selection; the guard itself is justified by the code path, not by this test.
#[test]
fn refreshing_the_list_with_a_live_selection_does_not_abort() {
    if gtk::init().is_err() {
        return;
    }
    let context = gtk::glib::MainContext::default();

    {
        let fixture = Fixture::new(60);
        let (_sender, receiver) = async_channel::unbounded();
        let gallery = build_with_paths(receiver, fixture.paths.clone()).unwrap();
        let window = gtk::Window::builder()
            .default_width(900)
            .default_height(600)
            .child(&gallery)
            .build();
        window.present();
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
        }

        let grid = descendants(&gallery)
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::GridView>().ok())
            .unwrap();
        let select = widget_by_name::<gtk::Button>(&gallery, "gallery-select");
        let selection_counter = widget_by_name::<gtk::Label>(&gallery, "gallery-selection-count");

        select.emit_clicked();
        grid.emit_by_name::<()>("activate", &[&0_u32]);
        while context.pending() {
            context.iteration(false);
        }
        assert_eq!(selection_counter.text(), "Selected captures: 1");

        // What GalleryEvent::Refresh does, with the selection deliberately left
        // active: remove every item then append them again.
        let selection = grid.model().and_downcast::<gtk::SingleSelection>().unwrap();
        let model = selection
            .model()
            .and_downcast::<gtk::gio::ListStore>()
            .unwrap();
        let items = (0..model.n_items())
            .filter_map(|index| model.item(index))
            .collect::<Vec<_>>();
        model.remove_all();
        for item in &items {
            model.append(item);
        }
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
        }

        // Reaching this line at all is the assertion: the bug aborted the
        // process rather than failing the test.
        assert!(widget_by_name::<gtk::Box>(&gallery, "gallery-select-mode").is_visible());
        window.close();
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    paths: AppPaths,
}

impl Fixture {
    fn new(count: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("Pictures"),
        );
        paths.ensure().unwrap();
        let repository = CaptureRepository::new(open_database(&paths).unwrap());
        for index in 0..count {
            let id = Uuid::new_v4();
            repository
                .insert(NewCaptureRecord {
                    id,
                    kind: CaptureKind::Screenshot,
                    path: paths.captures.join(format!("{id}.png")),
                    original_path: None,
                    thumbnail_path: None,
                    created_at: Utc::now() + TimeDelta::milliseconds(index as i64),
                    width: 1_920,
                    height: 1_080,
                    duration: None,
                    file_size: 0,
                    target: CaptureTarget::Screen,
                    backend: DisplayServer::X11,
                    annotation_json: None,
                })
                .unwrap();
        }
        Self {
            _directory: directory,
            paths,
        }
    }
}

fn descendants(root: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut descendants = Vec::new();
    let mut pending = vec![root.as_ref().clone()];
    while let Some(widget) = pending.pop() {
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            pending.push(current);
        }
        descendants.push(widget);
    }
    descendants
}

fn button_by_label(root: &impl IsA<gtk::Widget>, label: &str) -> gtk::Button {
    descendants(root)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
        .find(|button| button.label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("button {label:?} was not found"))
}

fn widget_by_name<T>(root: &impl IsA<gtk::Widget>, name: &str) -> T
where
    T: IsA<gtk::Widget> + gtk::glib::object::Cast,
{
    descendants(root)
        .into_iter()
        .find(|widget| widget.widget_name() == name)
        .and_then(|widget| widget.downcast::<T>().ok())
        .unwrap_or_else(|| panic!("widget {name:?} was not found"))
}
