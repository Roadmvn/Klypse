use gtk::gio;
use klypse_app::settings::AppSettings;
use klypse_domain::HotkeyAction;

fn memory_settings() -> AppSettings {
    AppSettings::with_backend(gio::memory_settings_backend_new()).unwrap()
}

#[test]
fn defaults_match_the_product_contract() {
    let settings = memory_settings();

    assert_eq!(settings.capture_directory(), None);
    assert!(settings.copy_after_capture());
    assert!(settings.notify_after_capture());
    assert_eq!(settings.gif_fps(), 12);
    assert_eq!(settings.gif_max_seconds(), 30);
    assert_eq!(settings.language(), "system");
    assert_eq!(
        settings.shortcut(HotkeyAction::CaptureArea).as_deref(),
        Some("<Primary>Print")
    );
    assert_eq!(
        settings.shortcut(HotkeyAction::CaptureScreen).as_deref(),
        Some("Print")
    );
    assert_eq!(
        settings.shortcut(HotkeyAction::StopRecording).as_deref(),
        Some("<Primary><Shift>Escape")
    );
}

#[test]
fn typed_settings_round_trip_and_reject_invalid_values() {
    let settings = memory_settings();
    let directory = std::path::Path::new("/tmp/Klypse captures");

    settings.set_capture_directory(Some(directory)).unwrap();
    settings.set_copy_after_capture(false).unwrap();
    settings.set_notify_after_capture(false).unwrap();
    settings.set_language("fr").unwrap();
    settings.set_gif_fps(24).unwrap();
    settings.set_gif_max_seconds(15).unwrap();
    settings
        .set_shortcut(HotkeyAction::RecordGif, "<Super>G")
        .unwrap();
    settings
        .set_shortcut(HotkeyAction::StopRecording, "<Super>Escape")
        .unwrap();

    assert_eq!(settings.capture_directory().as_deref(), Some(directory));
    assert!(!settings.copy_after_capture());
    assert!(!settings.notify_after_capture());
    assert_eq!(settings.language(), "fr");
    assert_eq!(settings.gif_fps(), 24);
    assert_eq!(settings.gif_max_seconds(), 15);
    assert_eq!(
        settings.shortcut(HotkeyAction::RecordGif).as_deref(),
        Some("<Super>G")
    );
    assert_eq!(
        settings.shortcut(HotkeyAction::StopRecording).as_deref(),
        Some("<Super>Escape")
    );

    assert!(settings.set_gif_fps(0).is_err());
    assert!(settings.set_gif_fps(31).is_err());
    assert!(settings.set_gif_max_seconds(0).is_err());
    assert!(settings.set_gif_max_seconds(31).is_err());
    assert!(settings.set_language("de").is_err());
    assert!(
        settings
            .set_shortcut(HotkeyAction::StopRecording, "")
            .is_err()
    );
}
