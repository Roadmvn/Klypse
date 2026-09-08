use crate::i18n::gettext;
use gtk::prelude::*;
use libadwaita::{self as adw, prelude::*};

pub struct Shortcut {
    pub title: String,
    pub command: &'static str,
    pub keys: String,
}

pub fn parse_xfce_bindings(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("/commands/custom/")?;
            let (key, command) = line.split_once(char::is_whitespace)?;
            let mut words = command.split_whitespace();
            let executable = words.next()?;
            if std::path::Path::new(executable).file_name()?.to_str()? != "klypse" {
                return None;
            }
            Some((
                key.to_owned(),
                format!("klypse {}", words.collect::<Vec<_>>().join(" ")),
            ))
        })
        .collect()
}

pub fn configured() -> (Vec<Shortcut>, bool) {
    let bindings = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_uppercase()
        .contains("XFCE")
        .then(|| {
            std::process::Command::new("xfconf-query")
                .args(["-c", "xfce4-keyboard-shortcuts", "-lv"])
                .output()
                .ok()
        })
        .flatten()
        .filter(|output| output.status.success())
        .map(|output| parse_xfce_bindings(&String::from_utf8_lossy(&output.stdout)));
    let verified = bindings.is_some();
    let bindings = bindings.unwrap_or_default();
    let rows = [
        (gettext("Capture area"), "klypse capture area"),
        (gettext("Capture screen"), "klypse capture screen"),
        (gettext("Capture window"), "klypse capture window"),
        (
            gettext("Capture active window"),
            "klypse capture active-window",
        ),
        (
            gettext("Capture menu (5 seconds)"),
            "klypse capture screen --delay 5",
        ),
        (gettext("Record area"), "klypse record video area"),
        (gettext("Record screen"), "klypse record video screen"),
        (gettext("Record GIF"), "klypse record gif area"),
        (gettext("Stop recording"), "klypse stop"),
    ]
    .into_iter()
    .map(|(title, command)| {
        let keys = bindings
            .iter()
            .filter(|(_, value)| value == command)
            .map(|(key, _)| {
                gtk::accelerator_parse(key)
                    .map(|(key, modifiers)| gtk::accelerator_get_label(key, modifiers).to_string())
                    .unwrap_or_else(|| key.clone())
            })
            .collect::<Vec<_>>();
        Shortcut {
            title,
            command,
            keys: if keys.is_empty() {
                if verified {
                    gettext("Not assigned")
                } else {
                    gettext("Check desktop settings")
                }
            } else {
                keys.join(" / ")
            },
        }
    })
    .collect();
    (rows, verified)
}

pub fn group() -> adw::PreferencesGroup {
    let (rows, verified) = configured();
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Your keyboard shortcuts"))
        .description(if verified {
            gettext("Shortcuts currently configured in your desktop.")
        } else {
            gettext("Desktop shortcuts cannot be read here. Check your desktop keyboard settings.")
        })
        .build();
    for shortcut in rows {
        let row = adw::ActionRow::builder()
            .title(&shortcut.title)
            .subtitle(&shortcut.keys)
            .build();
        group.add(&row);
    }
    group
}

pub fn present(parent: &adw::ApplicationWindow) {
    let window = adw::PreferencesWindow::builder()
        .title(gettext("Keyboard shortcuts"))
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .default_height(660)
        .build();
    let page = adw::PreferencesPage::new();
    page.add(&group());
    window.add(&page);
    window.present();
}
