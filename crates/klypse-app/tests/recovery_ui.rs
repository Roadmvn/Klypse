use std::{sync::Arc, time::Duration};

use gtk::prelude::*;
use klypse_app::ui::recovery::build;
use klypse_storage::{AppPaths, CaptureRepository, Reconciler, open_database};

#[test]
fn recovery_panel_is_hidden_when_nothing_requires_attention() {
    let fixture = Fixture::new();
    let (sender, _receiver) = async_channel::unbounded();

    assert!(
        build(
            fixture.reconciler.scan().unwrap(),
            fixture.reconciler,
            sender
        )
        .is_none()
    );
}

#[test]
fn recovery_panel_exposes_explicit_actions_for_invalid_files() {
    if gtk::init().is_err() {
        return;
    }
    let fixture = Fixture::new();
    std::fs::write(fixture.paths.temporary.join("broken.gif"), b"GIF89a").unwrap();
    let (sender, _receiver) = async_channel::unbounded();

    let panel = build(
        fixture.reconciler.scan().unwrap(),
        fixture.reconciler,
        sender,
    )
    .unwrap();
    let labels = descendants::<gtk::Label>(&panel)
        .into_iter()
        .map(|label| label.text().to_string())
        .collect::<Vec<_>>();
    let buttons = descendants::<gtk::Button>(&panel);
    let button_labels = buttons
        .iter()
        .filter_map(|button| button.label().map(|label| label.to_string()))
        .collect::<Vec<_>>();

    assert!(labels.iter().any(|label| label == "Recovery needed"));
    assert!(labels.iter().any(|label| label.contains("broken.gif")));
    assert!(button_labels.iter().any(|label| label == "Discard"));

    let host = gtk::Box::new(gtk::Orientation::Vertical, 0);
    host.append(&panel);
    buttons
        .into_iter()
        .find(|button| button.label().as_deref() == Some("Discard"))
        .unwrap()
        .emit_clicked();
    let context = gtk::glib::MainContext::default();
    for _ in 0..100 {
        while context.pending() {
            context.iteration(false);
        }
        if host.first_child().is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(host.first_child().is_none());
}

struct Fixture {
    _directory: tempfile::TempDir,
    paths: AppPaths,
    reconciler: Arc<Reconciler>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("Pictures"),
        );
        paths.ensure().unwrap();
        let repository = Arc::new(CaptureRepository::new(open_database(&paths).unwrap()));
        let reconciler = Arc::new(Reconciler::new(paths.clone(), repository));
        Self {
            _directory: directory,
            paths,
            reconciler,
        }
    }
}

fn descendants<T>(root: &impl IsA<gtk::Widget>) -> Vec<T>
where
    T: IsA<gtk::Widget> + gtk::glib::object::Cast,
{
    let mut matches = Vec::new();
    let mut pending = vec![root.as_ref().clone()];
    while let Some(widget) = pending.pop() {
        if let Ok(widget) = widget.clone().downcast::<T>() {
            matches.push(widget);
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            pending.push(current);
        }
    }
    matches
}
