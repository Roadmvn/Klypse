use async_channel::Sender;
use gettextrs::gettext;
use gtk::{Align, Orientation, prelude::*};
use klypse_domain::{AppCommand, CaptureRequest, CaptureTarget};
use klypse_platform::{CapabilityReport, CapabilityStatus};
use libadwaita as adw;
use libadwaita::prelude::*;

pub fn present(application: &adw::Application, sender: Sender<AppCommand>) {
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
    toolbar_view.add_top_bar(&adw::HeaderBar::new());

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
    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .build();

    for (label, target) in [
        (gettext("Capture area"), CaptureTarget::Area),
        (gettext("Capture screen"), CaptureTarget::Screen),
        (gettext("Capture window"), CaptureTarget::Window),
    ] {
        let button = gtk::Button::with_label(&label);
        let action_sender = sender.clone();
        button.connect_clicked(move |_| {
            let _ = action_sender.try_send(AppCommand::Capture(CaptureRequest::new(target)));
        });
        actions.append(&button);
    }

    content.append(&actions);
    match super::gallery::build() {
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
    content.append(&diagnostics(&CapabilityReport::detect()));
    toolbar_view.set_content(Some(&content));
    window.set_content(Some(&toolbar_view));
    window.present();
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
