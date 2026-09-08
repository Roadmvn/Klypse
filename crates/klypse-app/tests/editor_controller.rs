use klypse_app::editor::{EditorController, EditorTool};
use klypse_image::{AnnotationDocument, LayerKind, Point, Rect};

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

#[test]
fn reopened_layers_with_sparse_and_custom_ids_survive_further_edits() {
    let mut initial = EditorController::new(100, 100).unwrap();
    initial.pointer_down(point(10.0, 10.0)).unwrap();
    initial.pointer_up(point(30.0, 30.0)).unwrap();
    let template = initial.document().layers[0].clone();
    let mut document = AnnotationDocument::new(100, 100).unwrap();
    for id in [
        "layer-1",
        "layer-3",
        "custom-annotation",
        "draft",
        "layer-18446744073709551615",
        "layer-18446744073709551616",
    ] {
        let mut layer = template.clone();
        layer.id = id.into();
        document.layers.push(layer);
    }
    let previous = document.layers.clone();
    let mut editor = EditorController::from_document(document).unwrap();

    for _ in 0..3 {
        editor.pointer_down(point(40.0, 40.0)).unwrap();
        editor.pointer_move(point(50.0, 50.0)).unwrap();
        let draft = editor.draft_layer().unwrap().clone();
        let mut preview = editor.document().clone();
        preview.layers.push(draft.clone());
        preview.validate().unwrap();
        editor.pointer_up(point(50.0, 50.0)).unwrap();
        assert_eq!(editor.document().layers.last(), Some(&draft));
    }

    editor.document().validate().unwrap();
    assert_eq!(editor.document().layers.len(), previous.len() + 3);
    assert_eq!(&editor.document().layers[..previous.len()], &previous);
}

#[test]
fn reopened_layer_ids_remain_reserved_across_deletion_and_undo_redo() {
    let mut initial = EditorController::new(100, 100).unwrap();
    initial.pointer_down(point(10.0, 10.0)).unwrap();
    initial.pointer_up(point(30.0, 30.0)).unwrap();
    let previous = initial.document().layers[0].clone();
    let mut editor = EditorController::from_document(initial.document().clone()).unwrap();

    editor.delete_last_layer().unwrap();
    editor.pointer_down(point(40.0, 40.0)).unwrap();
    editor.pointer_up(point(50.0, 50.0)).unwrap();
    let added = editor.document().layers[0].clone();
    assert_ne!(added.id, previous.id);

    editor.undo().unwrap();
    editor.undo().unwrap();
    assert_eq!(editor.document().layers, vec![previous]);
    editor.redo().unwrap();
    editor.redo().unwrap();
    assert_eq!(editor.document().layers, vec![added.clone()]);

    editor.undo().unwrap();
    editor.pointer_down(point(60.0, 60.0)).unwrap();
    editor.pointer_up(point(70.0, 70.0)).unwrap();
    assert_ne!(editor.document().layers[0].id, added.id);
    assert!(!editor.can_redo());
    editor.document().validate().unwrap();
}
