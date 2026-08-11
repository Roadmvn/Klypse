use std::{cell::RefCell, rc::Rc};

use async_channel::Sender;
use gtk::{Align, Orientation, glib, prelude::*};
use klypse_domain::AppCommand;
use klypse_platform::CapabilityReport;
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

    /// Confirms a successful action in the window.
    ///
    /// Without this the happy path ends in silence: the only feedback was a
    /// desktop notification, which the user can have turned off. Dropped rather
    /// than queued when no window is up - a stale "saved" toast minutes later
    /// would be worse than none.
    pub(crate) fn show_info(&self, message: impl Into<String>) {
        let overlay = self
            .state
            .borrow()
            .overlay
            .as_ref()
            .and_then(glib::WeakRef::upgrade);
        if let Some(overlay) = overlay {
            overlay.add_toast(
                adw::Toast::builder()
                    .title(message.into())
                    .use_markup(false)
                    .timeout(4)
                    .build(),
            );
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
    header.set_show_title(false);
    let title = gtk::Label::new(Some(&gettext("Klypse")));
    title.add_css_class("heading");
    title.set_margin_start(6);
    header.pack_start(&title);
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
    let about = gtk::Button::builder()
        .icon_name("help-about-symbolic")
        .tooltip_text(gettext("About Klypse"))
        .build();
    super::set_accessible_label(&about, &gettext("About Klypse"));
    about.connect_clicked({
        let window = window.clone();
        move |_| present_about(&window)
    });
    header.pack_end(&about);
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
    toolbar_view.set_content(Some(&content));
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&toolbar_view));
    notifier.register(&toast_overlay);
    window.set_content(Some(&toast_overlay));
    window.present();
    super::recovery::monitor(&recovery_host, gallery_event_sender, sender, recovery_scans);
}

/// Shows who made this, which version is running, and where to report a bug.
///
/// Everything comes from the crate metadata so the dialog cannot drift out of
/// sync with a release.
fn present_about(parent: &adw::ApplicationWindow) {
    let about = adw::AboutWindow::builder()
        .transient_for(parent)
        .modal(true)
        .application_name(gettext("Klypse"))
        .application_icon(APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .developer_name("Roadmvn")
        .license_type(gtk::License::Gpl30)
        .website(env!("CARGO_PKG_REPOSITORY"))
        .issue_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues"))
        .comments(gettext("Capture, record, and annotate your Linux desktop"))
        .build();
    about.present();
}
