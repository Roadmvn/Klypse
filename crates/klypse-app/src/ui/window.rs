use std::{cell::RefCell, rc::Rc};

use async_channel::Sender;
use gtk::{Align, Orientation, glib, prelude::*};
use klypse_domain::AppCommand;
use klypse_platform::{CapabilityReport, CapabilityStatus};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::{APP_ID, i18n::gettext};

#[derive(Default)]
struct UiNotifierState {
    overlay: Option<glib::WeakRef<adw::ToastOverlay>>,
    pending_error: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct UiNotifier {
    state: Rc<RefCell<UiNotifierState>>,
}

impl UiNotifier {
    fn register(&self, overlay: &adw::ToastOverlay) {
        let pending_error = {
            let mut state = self.state.borrow_mut();
            state.overlay = Some(overlay.downgrade());
            state.pending_error.take()
        };
        if let Some(message) = pending_error {
            add_error_toast(overlay, &message);
        }
    }

    pub(crate) fn show_error(&self, message: impl Into<String>) {
        let message = message.into();
        let overlay = self
            .state
            .borrow()
            .overlay
            .as_ref()
            .and_then(glib::WeakRef::upgrade);
        if let Some(overlay) = overlay {
            add_error_toast(&overlay, &message);
        } else {
            self.state.borrow_mut().pending_error = Some(message);
        }
    }
}

fn add_error_toast(overlay: &adw::ToastOverlay, message: &str) {
    overlay.add_toast(
        adw::Toast::builder()
            .title(message)
            .use_markup(false)
            .priority(adw::ToastPriority::High)
            .timeout(8)
            .build(),
    );
}

pub(crate) fn present(
    application: &adw::Application,
    sender: Sender<AppCommand>,
    gallery_events: async_channel::Receiver<crate::gallery::GalleryEvent>,
    gallery_event_sender: async_channel::Sender<crate::gallery::GalleryEvent>,
    recording: std::sync::Arc<std::sync::Mutex<super::recording::RecordingPresentation>>,
    recovery_scans: async_channel::Receiver<()>,
    notifier: UiNotifier,
) {
    if let Some(window) = application.active_window() {
        window.present();
        return;
    }

    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title(gettext("Klypse"))
        .default_width(1000)
        .default_height(700)
        .build();
    let toolbar_view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let app_icon = gtk::Image::builder()
        .icon_name(APP_ID)
        .pixel_size(24)
        .tooltip_text(gettext("Klypse"))
        .margin_start(4)
        .margin_end(4)
        .build();
    header.pack_start(&app_icon);
    let preferences = gtk::Button::builder()
        .icon_name("preferences-system-symbolic")
        .tooltip_text(gettext("Preferences"))
        .build();
    super::set_accessible_label(&preferences, &gettext("Preferences"));
    preferences.connect_clicked({
        let window = window.clone();
        move |_| super::settings::present(&window)
    });
    header.pack_end(&preferences);
    toolbar_view.add_top_bar(&header);

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .halign(Align::Fill)
        .valign(Align::Fill)
        .hexpand(true)
        .vexpand(true)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let capabilities = CapabilityReport::detect();
    let recovery_host = gtk::Box::new(Orientation::Vertical, 0);
    content.append(&recovery_host);
    content.append(&super::recording::build(
        sender.clone(),
        recording,
        &capabilities,
    ));
    match super::gallery::build(gallery_events) {
        Ok(gallery) => content.append(&gallery),
        Err(error) => {
            let failure = gtk::Label::new(Some(&format!(
                "{}: {error}",
                gettext("Unable to open the capture library")
            )));
            failure.add_css_class("error");
            content.append(&failure);
        }
    }
    content.append(&diagnostics(&capabilities));
    toolbar_view.set_content(Some(&content));
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&toolbar_view));
    notifier.register(&toast_overlay);
    window.set_content(Some(&toast_overlay));
    window.present();
    super::recovery::monitor(&recovery_host, gallery_event_sender, sender, recovery_scans);
}

fn diagnostics(report: &CapabilityReport) -> gtk::Expander {
    let list = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    let display = gtk::Label::new(Some(&format!(
        "{}: {}",
        gettext("Display server"),
        report.display_name()
    )));
    display.set_xalign(0.0);
    list.append(&display);
    list.append(&capability_row(
        &gettext("Static capture"),
        &report.static_capture,
    ));
    list.append(&capability_row(
        &gettext("Video recording"),
        &report.video_recording,
    ));
    list.append(&capability_row(
        &gettext("GIF recording"),
        &report.gif_recording,
    ));
    list.append(&capability_row(
        &gettext("Global shortcuts"),
        &report.global_shortcuts,
    ));

    gtk::Expander::builder()
        .label(gettext("Diagnostics"))
        .child(&list)
        .build()
}

fn capability_row(label: &str, status: &CapabilityStatus) -> gtk::Label {
    let state = if status.available {
        gettext("Available")
    } else {
        gettext("Unavailable")
    };
    let row = gtk::Label::new(Some(&format!("{label}: {state}")));
    row.set_xalign(0.0);
    row.set_tooltip_text(Some(&status.detail));
    row
}
