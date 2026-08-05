use klypse_app::editor::{EditorController, EditorTool};
use klypse_image::{LayerKind, Point, Rect};

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}

#[test]
fn rectangle_gesture_creates_one_normalized_layer() {
    let mut editor = EditorController::new(100, 100).unwrap();
    editor.set_tool(EditorTool::Rectangle);
    editor.pointer_down(point(80.0, 70.0)).unwrap();
    editor.pointer_move(point(20.0, 10.0)).unwrap();
    editor.pointer_up(point(20.0, 10.0)).unwrap();

    assert_eq!(editor.document().layers.len(), 1);
    assert_eq!(
        editor.document().layers[0].bounds(),
        Rect::new(20.0, 10.0, 60.0, 60.0).unwrap()
    );
    assert!(editor.is_dirty());
}

#[test]
fn escape_cancels_an_in_progress_gesture() {
    let mut editor = EditorController::new(100, 100).unwrap();
    editor.set_tool(EditorTool::Rectangle);
    editor.pointer_down(point(10.0, 10.0)).unwrap();
    editor.pointer_move(point(40.0, 40.0)).unwrap();
    assert!(editor.draft_layer().is_some());

    editor.cancel_current_action();

    assert!(editor.document().layers.is_empty());
    assert!(editor.draft_layer().is_none());
    assert!(!editor.is_dirty());
}

#[test]
fn undo_redo_and_zoom_preserve_image_coordinates() {
    let mut editor = EditorController::new(100, 100).unwrap();
    editor.set_zoom(2.0).unwrap();
    editor.set_tool(EditorTool::Line);
    editor.pointer_down(point(20.0, 40.0)).unwrap();
    editor.pointer_up(point(60.0, 80.0)).unwrap();

    let LayerKind::Line { start, end, .. } = editor.document().layers[0].kind else {
        panic!("expected a line layer");
    };
    assert_eq!(start, point(10.0, 20.0));
    assert_eq!(end, point(30.0, 40.0));
    assert!(editor.can_undo());

    editor.undo().unwrap();
    assert!(editor.document().layers.is_empty());
    assert!(editor.can_redo());
    editor.redo().unwrap();
    assert_eq!(editor.document().layers.len(), 1);
}

#[test]
fn deleting_the_last_annotation_can_be_undone_and_redone() {
    let mut editor = EditorController::new(100, 100).unwrap();
    editor.set_tool(EditorTool::Line);
    editor.pointer_down(point(10.0, 10.0)).unwrap();
    editor.pointer_up(point(30.0, 30.0)).unwrap();

    assert!(editor.can_delete_last_layer());
    assert!(editor.delete_last_layer().unwrap());
    assert!(!editor.can_delete_last_layer());

    editor.undo().unwrap();
    assert!(editor.can_delete_last_layer());
    assert_eq!(editor.document().layers.len(), 1);

    editor.redo().unwrap();
    assert!(!editor.can_delete_last_layer());
    assert!(editor.document().layers.is_empty());
}

#[test]
fn text_and_crop_commit_only_valid_actions() {
    let mut editor = EditorController::new(100, 100).unwrap();
    editor.set_tool(EditorTool::Text);
    editor.pointer_down(point(5.0, 8.0)).unwrap();
    editor.pointer_up(point(5.0, 8.0)).unwrap();
    assert!(!editor.set_text("   ").unwrap());
    assert!(editor.document().layers.is_empty());

    editor.pointer_down(point(5.0, 8.0)).unwrap();
    editor.pointer_up(point(5.0, 8.0)).unwrap();
    assert!(editor.set_text("Été 東京").unwrap());
    assert_eq!(editor.document().layers.len(), 1);

    editor.set_tool(EditorTool::Crop);
    editor.pointer_down(point(90.0, 80.0)).unwrap();
    editor.pointer_up(point(10.0, 20.0)).unwrap();
    assert_eq!(
        editor.document().crop,
        Some(Rect::new(10.0, 20.0, 80.0, 60.0).unwrap())
    );
}

#[test]
fn gestures_after_crop_remain_in_original_image_coordinates() {
    let mut editor = EditorController::new(100, 100).unwrap();
    editor.set_tool(EditorTool::Crop);
    editor.pointer_down(point(10.0, 20.0)).unwrap();
    editor.pointer_up(point(90.0, 80.0)).unwrap();

    editor.set_tool(EditorTool::Rectangle);
    editor.pointer_down(point(0.0, 0.0)).unwrap();
    editor.pointer_up(point(10.0, 10.0)).unwrap();

    assert_eq!(
        editor.document().layers[0].bounds(),
        Rect::new(10.0, 20.0, 10.0, 10.0).unwrap()
    );
}
