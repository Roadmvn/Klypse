use std::{path::Path, sync::Arc};

use async_channel::Sender;
use gettextrs::gettext;
use gtk::{gio, glib, prelude::*};
use klypse_storage::{
    AppPaths, CaptureRepository, DeleteMode, InvalidRecoveryFile, Reconciler, RecoverableFile,
    RecoveryReport, StorageError, open_database,
};

use crate::gallery::GalleryEvent;

pub fn scan_and_mount(container: &gtk::Box, gallery_events: Sender<GalleryEvent>) {
    let container = container.clone();
    glib::idle_add_local_once(move || {
        let result = (|| {
            let paths = AppPaths::discover()?;
            let repository = Arc::new(CaptureRepository::new(open_database(&paths)?));
            let reconciler = Arc::new(Reconciler::new(paths, repository));
            let report = reconciler.scan()?;
            Ok::<_, StorageError>((report, reconciler))
        })();
        match result {
            Ok((report, reconciler)) => {
                if let Some(panel) = build(report, reconciler, gallery_events) {
                    container.prepend(&panel);
                }
            }
            Err(error) => tracing::warn!(%error, "startup recovery scan failed"),
        }
    });
}

pub fn build(
    report: RecoveryReport,
    reconciler: Arc<Reconciler>,
    gallery_events: Sender<GalleryEvent>,
) -> Option<gtk::Widget> {
    if report.recoverable.is_empty()
        && report.unrecoverable.is_empty()
        && report.missing_files.is_empty()
        && report.stale_thumbnails.is_empty()
    {
        return None;
    }
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    panel.add_css_class("card");
    let title = gtk::Label::builder()
        .label(gettext("Recovery needed"))
        .css_classes(["heading"])
        .xalign(0.0)
        .build();
    let status = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .build();
    panel.append(&title);
    panel.append(&status);

    for candidate in report.recoverable {
        panel.append(&recoverable_row(
            candidate,
            Arc::clone(&reconciler),
            gallery_events.clone(),
            &status,
        ));
    }
    for invalid in report.unrecoverable {
        panel.append(&invalid_row(invalid, Arc::clone(&reconciler), &status));
    }
    for missing in report.missing_files {
        panel.append(&missing_row(
            missing,
            Arc::clone(&reconciler),
            gallery_events.clone(),
            &status,
        ));
    }
    for thumbnail in report.stale_thumbnails {
        panel.append(&stale_thumbnail_row(
            thumbnail,
            Arc::clone(&reconciler),
            &status,
        ));
    }
    Some(panel.upcast())
}

fn recoverable_row(
    candidate: RecoverableFile,
    reconciler: Arc<Reconciler>,
    gallery_events: Sender<GalleryEvent>,
    status: &gtk::Label,
) -> gtk::Box {
    let row = action_row(&format!(
        "{}: {}",
        gettext("Interrupted capture"),
        safe_name(&candidate.path)
    ));
    let restore = gtk::Button::with_label(&gettext("Restore"));
    let discard = gtk::Button::with_label(&gettext("Discard"));
    row.append(&restore);
    row.append(&discard);
    restore.connect_clicked({
        let candidate = candidate.clone();
        let reconciler = Arc::clone(&reconciler);
        let gallery_events = gallery_events.clone();
        let row = row.clone();
        let status = status.clone();
        move |button| {
            button.set_sensitive(false);
            let reconciler = Arc::clone(&reconciler);
            let candidate = candidate.clone();
            let gallery_events = gallery_events.clone();
            let row = row.clone();
            let status = status.clone();
            let button = button.clone();
            glib::spawn_future_local(async move {
                match gio::spawn_blocking(move || reconciler.restore(&candidate)).await {
                    Ok(Ok(record)) => {
                        let _ = gallery_events.try_send(GalleryEvent::Select(record.id));
                        row.unparent();
                        show_status(&status, &gettext("Capture restored"), false);
                    }
                    Ok(Err(error)) => {
                        show_status(&status, &error.to_string(), true);
                        button.set_sensitive(true);
                    }
                    Err(_) => {
                        show_status(&status, &gettext("Recovery action failed"), true);
                        button.set_sensitive(true);
                    }
                }
            });
        }
    });
    discard.connect_clicked({
        let reconciler = Arc::clone(&reconciler);
        let row = row.clone();
        let status = status.clone();
        move |button| {
            button.set_sensitive(false);
            let reconciler = Arc::clone(&reconciler);
            let candidate = candidate.clone();
            let row = row.clone();
            let status = status.clone();
            let button = button.clone();
            glib::spawn_future_local(async move {
                match gio::spawn_blocking(move || reconciler.discard(&candidate)).await {
                    Ok(Ok(())) => row.unparent(),
                    Ok(Err(error)) => {
                        show_status(&status, &error.to_string(), true);
                        button.set_sensitive(true);
                    }
                    Err(_) => {
                        show_status(&status, &gettext("Recovery action failed"), true);
                        button.set_sensitive(true);
                    }
                }
            });
        }
    });
    row
}

fn invalid_row(
    invalid: InvalidRecoveryFile,
    reconciler: Arc<Reconciler>,
    status: &gtk::Label,
) -> gtk::Box {
    let row = action_row(&format!(
        "{}: {}",
        gettext("Unusable temporary file"),
        safe_name(&invalid.path)
    ));
    row.set_tooltip_text(Some(&invalid.reason));
    let discard = gtk::Button::with_label(&gettext("Discard"));
    row.append(&discard);
    discard.connect_clicked({
        let row = row.clone();
        let status = status.clone();
        move |button| {
            button.set_sensitive(false);
            let reconciler = Arc::clone(&reconciler);
            let path = invalid.path.clone();
            let row = row.clone();
            let status = status.clone();
            let button = button.clone();
            glib::spawn_future_local(async move {
                match gio::spawn_blocking(move || reconciler.discard_path(&path)).await {
                    Ok(Ok(())) => row.unparent(),
                    Ok(Err(error)) => {
                        show_status(&status, &error.to_string(), true);
                        button.set_sensitive(true);
                    }
                    Err(_) => {
                        show_status(&status, &gettext("Recovery action failed"), true);
                        button.set_sensitive(true);
                    }
                }
            });
        }
    });
    row
}

fn missing_row(
    missing: klypse_storage::CaptureRecord,
    reconciler: Arc<Reconciler>,
    gallery_events: Sender<GalleryEvent>,
    status: &gtk::Label,
) -> gtk::Box {
    let row = action_row(&format!(
        "{}: {}",
        gettext("Missing gallery file"),
        safe_name(&missing.path)
    ));
    let locate = gtk::Button::with_label(&gettext("Locate File"));
    let remove = gtk::Button::with_label(&gettext("Remove from Gallery"));
    row.append(&locate);
    row.append(&remove);
    locate.connect_clicked({
        let row = row.clone();
        let status = status.clone();
        let reconciler = Arc::clone(&reconciler);
        let gallery_events = gallery_events.clone();
        move |button| {
            let parent = row.root().and_downcast::<gtk::Window>();
            let dialog = gtk::FileDialog::new();
            let row = row.clone();
            let status = status.clone();
            let reconciler = Arc::clone(&reconciler);
            let gallery_events = gallery_events.clone();
            let button = button.clone();
            let id = missing.id;
            glib::spawn_future_local(async move {
                let Ok(file) = dialog.open_future(parent.as_ref()).await else {
                    return;
                };
                let Some(path) = file.path() else {
                    show_status(&status, &gettext("Selected file is not local"), true);
                    return;
                };
                button.set_sensitive(false);
                match gio::spawn_blocking(move || reconciler.relocate_missing(&id, &path)).await {
                    Ok(Ok(record)) => {
                        let _ = gallery_events.try_send(GalleryEvent::Select(record.id));
                        row.unparent();
                        show_status(&status, &gettext("Capture location updated"), false);
                    }
                    Ok(Err(error)) => {
                        show_status(&status, &error.to_string(), true);
                        button.set_sensitive(true);
                    }
                    Err(_) => {
                        show_status(&status, &gettext("Recovery action failed"), true);
                        button.set_sensitive(true);
                    }
                }
            });
        }
    });
    remove.connect_clicked({
        let row = row.clone();
        let status = status.clone();
        move |button| {
            button.set_sensitive(false);
            let store = reconciler.store();
            let gallery_events = gallery_events.clone();
            let row = row.clone();
            let status = status.clone();
            let button = button.clone();
            let id = missing.id;
            glib::spawn_future_local(async move {
                match gio::spawn_blocking(move || store.delete(&id, DeleteMode::GalleryOnly)).await
                {
                    Ok(Ok(())) => {
                        let _ = gallery_events.try_send(GalleryEvent::Refresh);
                        row.unparent();
                    }
                    Ok(Err(error)) => {
                        show_status(&status, &error.to_string(), true);
                        button.set_sensitive(true);
                    }
                    Err(_) => {
                        show_status(&status, &gettext("Recovery action failed"), true);
                        button.set_sensitive(true);
                    }
                }
            });
        }
    });
    row
}

fn stale_thumbnail_row(
    thumbnail: std::path::PathBuf,
    reconciler: Arc<Reconciler>,
    status: &gtk::Label,
) -> gtk::Box {
    let row = action_row(&format!(
        "{}: {}",
        gettext("Stale thumbnail"),
        safe_name(&thumbnail)
    ));
    let discard = gtk::Button::with_label(&gettext("Discard"));
    row.append(&discard);
    discard.connect_clicked({
        let row = row.clone();
        let status = status.clone();
        move |button| {
            button.set_sensitive(false);
            let reconciler = Arc::clone(&reconciler);
            let thumbnail = thumbnail.clone();
            let row = row.clone();
            let status = status.clone();
            let button = button.clone();
            glib::spawn_future_local(async move {
                match gio::spawn_blocking(move || reconciler.discard_stale_thumbnail(&thumbnail))
                    .await
                {
                    Ok(Ok(())) => row.unparent(),
                    Ok(Err(error)) => {
                        show_status(&status, &error.to_string(), true);
                        button.set_sensitive(true);
                    }
                    Err(_) => {
                        show_status(&status, &gettext("Recovery action failed"), true);
                        button.set_sensitive(true);
                    }
                }
            });
        }
    });
    row
}

fn action_row(label: &str) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let label = gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    row.append(&label);
    row
}

fn safe_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| gettext("Unknown file"))
}

fn show_status(status: &gtk::Label, text: &str, error: bool) {
    status.set_label(text);
    status.set_visible(true);
    if error {
        status.add_css_class("error");
    } else {
        status.remove_css_class("error");
    }
}
