use std::io::Cursor;

use gtk::prelude::*;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use klypse_app::{editor::EditorController, ui::editor::EditorView};

fn source_png() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(80, 60, Rgba([240, 240, 240, 255])))
        .write_to(&mut cursor, ImageFormat::Png)
        .unwrap();
    cursor.into_inner()
}

#[test]
fn editor_view_constructs_the_complete_tool_surface() {
    if gtk::init().is_err() {
        eprintln!("GTK display unavailable; the Xvfb review job runs this test with a display");
        return;
    }
    let controller = EditorController::new(80, 60).unwrap();
    let view = EditorView::new(controller, source_png()).unwrap();

    assert_eq!(view.root().widget_name(), "klypse-editor");
    assert_eq!(view.tool_button_count(), 9);
    assert_eq!(view.controller().borrow().document().layers.len(), 0);
    assert!(view.canvas().can_focus());
}
