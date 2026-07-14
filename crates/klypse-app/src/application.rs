use async_channel::{Receiver, Sender};
use gtk::{gio, glib, prelude::*};
use klypse_domain::AppCommand;
use libadwaita as adw;

use crate::{APP_ID, cli, ui};

pub fn run() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let (sender, receiver) = async_channel::unbounded();

    connect_activate(&application, sender.clone());
    connect_command_line(&application, sender);
    dispatch_commands(receiver);

    application.run()
}

fn connect_activate(application: &adw::Application, sender: Sender<AppCommand>) {
    application.connect_activate(move |application| {
        ui::window::present(application, sender.clone());
    });
}

fn connect_command_line(application: &adw::Application, sender: Sender<AppCommand>) {
    application.connect_command_line(move |application, command_line| {
        match cli::parse_from(command_line.arguments()) {
            Ok(command) => {
                if command != AppCommand::Open && sender.try_send(command).is_err() {
                    eprintln!("Klypse could not queue the requested action");
                    return 1.into();
                }
                application.activate();
                0.into()
            }
            Err(error) => {
                eprint!("{error}");
                2.into()
            }
        }
    });
}

fn dispatch_commands(receiver: Receiver<AppCommand>) {
    glib::spawn_future_local(async move {
        while let Ok(command) = receiver.recv().await {
            tracing::info!(action = ?command, "received application command");
        }
    });
}
