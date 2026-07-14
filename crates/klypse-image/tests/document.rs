use klypse_image::{
    AnnotationDocument, Layer, LayerKind, Point, Rect, RedactionMode, Rgba, Stroke,
    ViewportTransform,
};

fn stroke() -> Stroke {
    Stroke::new(Rgba::new(1.0, 0.2, 0.1, 1.0).unwrap(), 4.0).unwrap()
}

#[test]
fn document_round_trip_preserves_all_layer_kinds() {
    let rect = Rect::new(10.0, 20.0, 100.0, 80.0).unwrap();
    let start = Point::new(5.0, 6.0).unwrap();
    let end = Point::new(50.0, 60.0).unwrap();
    let layers = vec![
        Layer::new(
            "rectangle",
            LayerKind::Rectangle {
                rect,
                stroke: stroke(),
                fill: Some(Rgba::new(0.0, 0.0, 1.0, 0.4).unwrap()),
            },
        ),
        Layer::new(
            "ellipse",
            LayerKind::Ellipse {
                rect,
                stroke: stroke(),
                fill: None,
            },
        ),
        Layer::new(
            "line",
            LayerKind::Line {
                start,
                end,
                stroke: stroke(),
            },
        ),
        Layer::new(
            "arrow",
            LayerKind::Arrow {
                start,
                end,
                stroke: stroke(),
                head_length: 14.0,
            },
        ),
        Layer::new(
            "text",
            LayerKind::Text {
                origin: start,
                text: "Bonjour 👋".into(),
                font: "Sans".into(),
                size: 24.0,
                color: Rgba::new(0.0, 0.0, 0.0, 1.0).unwrap(),
            },
        ),
        Layer::new(
            "freehand",
            LayerKind::Freehand {
                points: vec![start, end],
                stroke: stroke(),
            },
        ),
        Layer::new(
            "pixelate",
            LayerKind::Redaction {
                rect,
                mode: RedactionMode::Pixelate { block_size: 12 },
            },
        ),
        Layer::new(
            "blur",
            LayerKind::Redaction {
                rect,
                mode: RedactionMode::Blur { radius: 8 },
            },
        ),
    ];
    let mut document = AnnotationDocument::new(1920, 1080).unwrap();
    document.layers = layers;
    document.validate().unwrap();

    let json = serde_json::to_string(&document).unwrap();
    let decoded: AnnotationDocument = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, document);
    assert_eq!(decoded.version, 1);
}

#[test]
fn validation_rejects_invalid_content_and_out_of_bounds_crop() {
    let mut document = AnnotationDocument::new(100, 80).unwrap();
    document.crop = Some(Rect::new(90.0, 70.0, 20.0, 20.0).unwrap());
    assert!(document.validate().is_err());

    document.crop = None;
    document.layers.push(Layer::new(
        "empty",
        LayerKind::Text {
            origin: Point::new(0.0, 0.0).unwrap(),
            text: String::new(),
            font: "Sans".into(),
            size: 12.0,
            color: Rgba::new(0.0, 0.0, 0.0, 1.0).unwrap(),
        },
    ));
    assert!(document.validate().is_err());
}

#[test]
fn viewport_round_trip_preserves_image_coordinates() {
    let transform = ViewportTransform::new(2.5, Point::new(30.0, 40.0).unwrap()).unwrap();
    let image = Point::new(12.0, 18.0).unwrap();

    let view = transform.image_to_view(image);
    let round_trip = transform.view_to_image(view);

    assert!((round_trip.x - image.x).abs() < 1e-9);
    assert!((round_trip.y - image.y).abs() < 1e-9);
}
