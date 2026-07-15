use gtk::{gio, prelude::*};
use klypse_domain::HotkeyAction;
use klypse_platform::{CapabilityReport, CapabilityStatus, cli_fallback_commands};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::{i18n::gettext, settings::AppSettings};

pub fn present(parent: &adw::ApplicationWindow) {
    let settings = match AppSettings::new() {
        Ok(settings) => settings,
        Err(error) => {
            tracing::error!(%error, "preferences are unavailable");
            let alert = gtk::AlertDialog::builder()
                .message(gettext("Unable to open preferences"))
                .detail(error.to_string())
                .modal(true)
                .build();
            alert.show(Some(parent));
            return;
        }
    };

    let dialog = adw::PreferencesWindow::builder()
        .title(gettext("Preferences"))
        .default_width(680)
        .default_height(720)
        .transient_for(parent)
        .modal(true)
        .build();
    let page = adw::PreferencesPage::builder()
        .title(gettext("Preferences"))
        .icon_name("preferences-system-symbolic")
        .build();
    page.add(&general_group(parent, &settings));
    page.add(&gif_group(&settings));
    page.add(&shortcut_group(&settings));
    page.add(&fallback_group());
    page.add(&diagnostics_group(&CapabilityReport::detect()));
    dialog.add(&page);
    dialog.present();
}

fn general_group(parent: &adw::ApplicationWindow, settings: &AppSettings) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("General"))
        .build();

    let directory = adw::EntryRow::builder()
        .title(gettext("Capture folder"))
        .text(
            settings
                .capture_directory()
                .and_then(|path| path.to_str().map(ToOwned::to_owned))
                .unwrap_or_default(),
        )
        .build();
    let browse = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text(gettext("Choose a capture folder"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    super::set_accessible_label(&browse, &gettext("Choose a capture folder"));
    directory.add_suffix(&browse);
    directory.connect_changed({
        let settings = settings.clone();
        move |row| {
            let text = row.text();
            let value = (!text.is_empty()).then(|| std::path::Path::new(text.as_str()));
            if let Err(error) = settings.set_capture_directory(value) {
                tracing::warn!(%error, "capture directory preference was rejected");
            }
        }
    });
    browse.connect_clicked({
        let parent = parent.clone();
        let directory = directory.clone();
        move |_| {
            let chooser = gtk::FileDialog::builder()
                .title(gettext("Choose a capture folder"))
                .modal(true)
                .build();
            chooser.select_folder(Some(&parent), gio::Cancellable::NONE, {
                let directory = directory.clone();
                move |result| {
                    if let Ok(file) = result
                        && let Some(path) = file.path()
                        && let Some(path) = path.to_str()
                    {
                        directory.set_text(path);
                    }
                }
            });
        }
    });
    group.add(&directory);

    let copy = adw::SwitchRow::builder()
        .title(gettext("Copy screenshots after capture"))
        .active(settings.copy_after_capture())
        .build();
    copy.connect_active_notify({
        let settings = settings.clone();
        move |row| {
            if let Err(error) = settings.set_copy_after_capture(row.is_active()) {
                tracing::warn!(%error, "copy preference could not be saved");
            }
        }
    });
    group.add(&copy);

    let notify = adw::SwitchRow::builder()
        .title(gettext("Notify after capture"))
        .active(settings.notify_after_capture())
        .build();
    notify.connect_active_notify({
        let settings = settings.clone();
        move |row| {
            if let Err(error) = settings.set_notify_after_capture(row.is_active()) {
                tracing::warn!(%error, "notification preference could not be saved");
            }
        }
    });
    group.add(&notify);

    let languages =
        gtk::StringList::new(&[&gettext("System"), &gettext("English"), &gettext("French")]);
    let language = adw::ComboRow::builder()
        .title(gettext("Language"))
        .subtitle(gettext("Restart Klypse to apply language changes"))
        .model(&languages)
        .selected(match settings.language().as_str() {
            "en" => 1,
            "fr" => 2,
            _ => 0,
        })
        .build();
    language.connect_selected_notify({
        let settings = settings.clone();
        move |row| {
            let value = match row.selected() {
                1 => "en",
                2 => "fr",
                _ => "system",
            };
            if let Err(error) = settings.set_language(value) {
                tracing::warn!(%error, "language preference could not be saved");
            }
        }
    });
    group.add(&language);
    group
}

fn gif_group(settings: &AppSettings) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("GIF recording"))
        .build();
    let fps = adw::SpinRow::with_range(1.0, 30.0, 1.0);
    fps.set_title(&gettext("Frames per second"));
    fps.set_value(settings.gif_fps().into());
    fps.connect_value_notify({
        let settings = settings.clone();
        move |row| {
            if let Err(error) = settings.set_gif_fps(row.value().round() as u32) {
                tracing::warn!(%error, "GIF frame rate preference could not be saved");
            }
        }
    });
    group.add(&fps);

    let duration = adw::SpinRow::with_range(1.0, 30.0, 1.0);
    duration.set_title(&gettext("Maximum duration (seconds)"));
    duration.set_value(settings.gif_max_seconds().into());
    duration.connect_value_notify({
        let settings = settings.clone();
        move |row| {
            if let Err(error) = settings.set_gif_max_seconds(row.value().round() as u32) {
                tracing::warn!(%error, "GIF duration preference could not be saved");
            }
        }
    });
    group.add(&duration);
    group
}

fn shortcut_group(settings: &AppSettings) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Keyboard shortcuts"))
        .description(gettext("GTK accelerator syntax is supported"))
        .build();
    for (title, action) in [
        (gettext("Capture area"), HotkeyAction::CaptureArea),
        (gettext("Capture screen"), HotkeyAction::CaptureScreen),
        (gettext("Capture window"), HotkeyAction::CaptureWindow),
        (gettext("Record video"), HotkeyAction::RecordVideo),
        (gettext("Record GIF"), HotkeyAction::RecordGif),
        (gettext("Stop recording"), HotkeyAction::StopRecording),
    ] {
        let row = adw::EntryRow::builder()
            .title(title)
            .text(settings.shortcut(action).unwrap_or_default())
            .build();
        row.connect_changed({
            let settings = settings.clone();
            move |row| {
                if let Err(error) = settings.set_shortcut(action, row.text().as_str()) {
                    tracing::warn!(%error, action = action.id(), "shortcut preference was rejected");
                }
            }
        });
        group.add(&row);
    }
    group
}

fn fallback_group() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Desktop shortcut fallback"))
        .description(gettext(
            "Use these commands in your desktop shortcut settings if global registration is unavailable",
        ))
        .build();
    for (action, command) in cli_fallback_commands() {
        let title = match action {
            HotkeyAction::CaptureArea => gettext("Capture area"),
            HotkeyAction::CaptureScreen => gettext("Capture screen"),
            HotkeyAction::CaptureWindow => gettext("Capture window"),
            HotkeyAction::RecordVideo => gettext("Record video"),
            HotkeyAction::RecordGif => gettext("Record GIF"),
            HotkeyAction::StopRecording => gettext("Stop recording"),
        };
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(command)
            .build();
        let copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text(gettext("Copy command"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        super::set_accessible_label(&copy, &gettext("Copy command"));
        copy.connect_clicked(move |_| {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(command);
            }
        });
        row.add_suffix(&copy);
        group.add(&row);
    }
    group
}

fn diagnostics_group(report: &CapabilityReport) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Diagnostics"))
        .description(format!(
            "{}: {}",
            gettext("Display server"),
            report.display_name()
        ))
        .build();
    for (title, status) in [
        (gettext("Static capture"), &report.static_capture),
        (gettext("Video recording"), &report.video_recording),
        (gettext("GIF recording"), &report.gif_recording),
        (gettext("Global shortcuts"), &report.global_shortcuts),
    ] {
        group.add(&capability_row(&title, status));
    }
    group
}

fn capability_row(title: &str, status: &CapabilityStatus) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(if status.available {
            gettext("Available")
        } else {
            format!("{} — {}", gettext("Unavailable"), status.detail)
        })
        .build()
}
