use crate::i18n::gettext;
use gtk::{gio, prelude::*};
use klypse_domain::KlypseError;
use klypse_storage::CaptureRecord;

pub fn notification_body(record: &CaptureRecord) -> String {
    record
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| gettext("Capture"))
}

pub fn notify_capture_saved(record: &CaptureRecord) -> Result<(), KlypseError> {
    let application = gio::Application::default().ok_or_else(|| {
        KlypseError::UnavailableCapability("application notifications are unavailable".into())
    })?;
    if application.dbus_connection().is_none() {
        return Err(KlypseError::UnavailableCapability(
            "the session notification bus is unavailable".into(),
        ));
    }
    let notification = gio::Notification::new(&gettext("Capture saved"));
    notification.set_body(Some(&notification_body(record)));
    notification.set_default_action_and_target_value(
        "app.open-capture",
        Some(&record.id.to_string().to_variant()),
    );
    application.send_notification(
        Some(record.id.as_hyphenated().to_string().as_str()),
        &notification,
    );
    Ok(())
}
