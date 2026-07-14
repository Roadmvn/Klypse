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
fn virtualized_gallery_realizes_at_most_three_pages_of_cards() {
    if gtk::init().is_err() {
        return;
    }
    let fixture = Fixture::new(1_000);
    let (_sender, receiver) = async_channel::unbounded();
    let gallery = build_with_paths(receiver, fixture.paths.clone()).unwrap();
    let window = gtk::Window::builder()
        .default_width(1_000)
        .default_height(700)
        .child(&gallery)
        .build();
    window.present();
    let context = gtk::glib::MainContext::default();
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
    window.close();
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
