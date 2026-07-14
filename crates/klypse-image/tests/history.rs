use klypse_image::{
    AnnotationDocument, DocumentCommand, EditHistory, Layer, LayerKind, Rect, Rgba, Stroke,
};

fn layer(id: &str) -> Layer {
    Layer::new(
        id,
        LayerKind::Rectangle {
            rect: Rect::new(1.0, 2.0, 10.0, 20.0).unwrap(),
            stroke: Stroke::new(Rgba::new(1.0, 0.0, 0.0, 1.0).unwrap(), 2.0).unwrap(),
            fill: None,
        },
    )
}

#[test]
fn undo_then_new_edit_discards_redo_branch() {
    let mut document = AnnotationDocument::new(100, 100).unwrap();
    let mut history = EditHistory::new(100).unwrap();
    history
        .execute(&mut document, DocumentCommand::AddLayer(layer("a")))
        .unwrap();
    history
        .execute(&mut document, DocumentCommand::AddLayer(layer("b")))
        .unwrap();

    history.undo(&mut document).unwrap();
    history
        .execute(&mut document, DocumentCommand::AddLayer(layer("c")))
        .unwrap();

    assert!(!history.can_redo());
    assert_eq!(
        document
            .layers
            .iter()
            .map(|layer| layer.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "c"]
    );
}

#[test]
fn undo_and_redo_apply_reversible_commands() {
    let mut document = AnnotationDocument::new(100, 100).unwrap();
    let mut history = EditHistory::default();
    history
        .execute(&mut document, DocumentCommand::AddLayer(layer("a")))
        .unwrap();

    history.undo(&mut document).unwrap();
    assert!(document.layers.is_empty());
    history.redo(&mut document).unwrap();
    assert_eq!(document.layers, [layer("a")]);
}

#[test]
fn history_limit_discards_the_oldest_command() {
    let mut document = AnnotationDocument::new(100, 100).unwrap();
    let mut history = EditHistory::new(2).unwrap();
    for id in ["a", "b", "c"] {
        history
            .execute(&mut document, DocumentCommand::AddLayer(layer(id)))
            .unwrap();
    }

    history.undo(&mut document).unwrap();
    history.undo(&mut document).unwrap();

    assert!(!history.can_undo());
    assert_eq!(document.layers, [layer("a")]);
}
