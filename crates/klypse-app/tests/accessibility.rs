use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use gtk::prelude::*;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use klypse_app::{
    editor::EditorController,
    ui::{
        editor::EditorView,
        recording::{RecordingPresentation, build as build_recording},
    },
};
use klypse_domain::DisplayServer;
use klypse_platform::{CapabilityReport, CapabilityStatus};

#[test]
fn every_interactive_editor_and_recording_control_has_a_readable_name() {
    if gtk::init().is_err() {
        return;
    }
    let editor = EditorView::new(EditorController::new(80, 60).unwrap(), source_png()).unwrap();
    assert_named_interactive_controls(editor.root());

    let available = CapabilityStatus {
        available: true,
        detail: "test".into(),
    };
    let report = CapabilityReport {
        display: DisplayServer::X11,
        static_capture: available.clone(),
        video_recording: available.clone(),
        gif_recording: available.clone(),
        global_shortcuts: available,
    };
    let (sender, _receiver) = async_channel::unbounded();
    let recording = build_recording(
        sender,
        Arc::new(Mutex::new(RecordingPresentation::default())),
        &report,
    );
    assert_named_interactive_controls(&recording);
}

fn assert_named_interactive_controls(root: &impl IsA<gtk::Widget>) {
    let unnamed = descendants(root)
        .into_iter()
        .filter(is_interactive)
        .filter(|widget| accessible_name(widget).is_none())
        .map(|widget| widget.type_().name().to_string())
        .collect::<Vec<_>>();
    assert!(
        unnamed.is_empty(),
        "unnamed interactive controls: {unnamed:?}"
    );
}

fn is_interactive(widget: &gtk::Widget) -> bool {
    widget.is::<gtk::Button>()
        || widget.is::<gtk::ToggleButton>()
        || widget.is::<gtk::Entry>()
        || widget.is::<gtk::SpinButton>()
        || widget.is::<gtk::ColorDialogButton>()
        || widget.is::<gtk::DrawingArea>()
}

fn accessible_name(widget: &gtk::Widget) -> Option<String> {
    if let Ok(button) = widget.clone().downcast::<gtk::Button>()
        && let Some(label) = button.label().filter(|label| !label.is_empty())
    {
        return Some(label.to_string());
    }
    if let Ok(button) = widget.clone().downcast::<gtk::ToggleButton>()
        && let Some(label) = button.label().filter(|label| !label.is_empty())
    {
        return Some(label.to_string());
    }
    if let Ok(entry) = widget.clone().downcast::<gtk::Entry>()
        && let Some(placeholder) = entry.placeholder_text().filter(|text| !text.is_empty())
    {
        return Some(placeholder.to_string());
    }
    widget.tooltip_text().map(|text| text.to_string())
}

fn descendants(root: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut descendants = Vec::new();
    let mut pending = vec![root.as_ref().clone()];
    while let Some(widget) = pending.pop() {
        if !is_interactive(&widget) {
            let mut child = widget.first_child();
            while let Some(current) = child {
                child = current.next_sibling();
                pending.push(current);
            }
        }
        descendants.push(widget);
    }
    descendants
}

fn source_png() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(80, 60, Rgba([240, 240, 240, 255])))
        .write_to(&mut cursor, ImageFormat::Png)
        .unwrap();
    cursor.into_inner()
}
