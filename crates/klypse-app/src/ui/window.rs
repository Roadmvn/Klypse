use async_channel::Sender;
use gtk::{Align, Orientation, prelude::*};
use klypse_domain::{AppCommand, CaptureRequest, CaptureTarget};
use libadwaita as adw;
use libadwaita::prelude::*;

pub fn present(application: &adw::Application, sender: Sender<AppCommand>) {
    if let Some(window) = application.active_window() {
        window.present();
        return;
    }

    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Klypse")
        .default_width(1000)
        .default_height(700)
        .build();
    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&adw::HeaderBar::new());

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .halign(Align::Center)
        .valign(Align::Center)
        .build();
    let title = gtk::Label::builder()
        .label("Your captures will appear here")
        .css_classes(["title-2"])
        .build();
    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .build();

    for (label, target) in [
        ("Capture area", CaptureTarget::Area),
        ("Capture screen", CaptureTarget::Screen),
        ("Capture window", CaptureTarget::Window),
    ] {
        let button = gtk::Button::with_label(label);
        let action_sender = sender.clone();
        button.connect_clicked(move |_| {
            let _ = action_sender.try_send(AppCommand::Capture(CaptureRequest::new(target)));
        });
        actions.append(&button);
    }

    content.append(&title);
    content.append(&actions);
    toolbar_view.set_content(Some(&content));
    window.set_content(Some(&toolbar_view));
    window.present();
}
