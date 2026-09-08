use gtk::{gdk, prelude::*};
use klypse_storage::CaptureRecord;

use super::clipboard::capture_content_provider;

pub fn install_drag_source<W, F>(widget: &W, record: F)
where
    W: IsA<gtk::Widget>,
    F: Fn() -> Option<CaptureRecord> + 'static,
{
    let source = gtk::DragSource::builder()
        .actions(gdk::DragAction::COPY)
        .build();
    source.connect_prepare(move |source, _, _| {
        let record = record()?;
        let provider = match capture_content_provider(&record) {
            Ok(provider) => provider,
            Err(error) => {
                tracing::warn!(
                    capture_id = %record.id,
                    %error,
                    "capture could not be prepared for drag and drop"
                );
                if let Some(widget) = source.widget() {
                    crate::ui::window::show_action_error(
                        &widget,
                        &crate::i18n::gettext("Unable to share capture"),
                        &error.to_string(),
                    );
                }
                return None;
            }
        };
        let icon_path = record.thumbnail_path.as_ref().unwrap_or(&record.path);
        if let Ok(texture) = gdk::Texture::from_filename(icon_path) {
            source.set_icon(Some(&texture), 0, 0);
        }
        Some(provider)
    });
    widget.add_controller(source);
}
