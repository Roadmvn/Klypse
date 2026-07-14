use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use klypse_image::{
    AnnotationDocument, Layer, LayerKind, Point, Rect, Renderer, Rgba as AnnotationRgba, Stroke,
};

fn png_bytes(image: RgbaImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn crop_sets_exact_output_dimensions_and_translates_layers() {
    let source = png_bytes(RgbaImage::from_pixel(100, 80, Rgba([255, 255, 255, 255])));
    let mut document = AnnotationDocument::new(100, 80)
        .unwrap()
        .with_crop(Rect::new(10.0, 20.0, 30.0, 40.0).unwrap())
        .unwrap();
    document.layers.push(Layer::new(
        "rectangle",
        LayerKind::Rectangle {
            rect: Rect::new(10.0, 20.0, 10.0, 10.0).unwrap(),
            stroke: Stroke::new(AnnotationRgba::new(1.0, 0.0, 0.0, 1.0).unwrap(), 2.0).unwrap(),
            fill: Some(AnnotationRgba::new(1.0, 0.0, 0.0, 1.0).unwrap()),
        },
    ));

    let output = Renderer::default()
        .render_to_rgba(&source, &document)
        .unwrap();

    assert_eq!(output.dimensions(), (30, 40));
    assert!(output.get_pixel(5, 5)[0] > 240);
    assert!(output.get_pixel(5, 5)[1] < 20);
    assert_eq!(output.get_pixel(20, 20), &Rgba([255, 255, 255, 255]));
}

#[test]
fn layers_render_in_document_order() {
    let source = png_bytes(RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255])));
    let mut document = AnnotationDocument::new(20, 20).unwrap();
    for (id, color) in [
        ("red", AnnotationRgba::new(1.0, 0.0, 0.0, 1.0).unwrap()),
        ("blue", AnnotationRgba::new(0.0, 0.0, 1.0, 1.0).unwrap()),
    ] {
        document.layers.push(Layer::new(
            id,
            LayerKind::Rectangle {
                rect: Rect::new(2.0, 2.0, 16.0, 16.0).unwrap(),
                stroke: Stroke::new(color, 1.0).unwrap(),
                fill: Some(color),
            },
        ));
    }

    let output = Renderer::default()
        .render_to_rgba(&source, &document)
        .unwrap();

    assert!(output.get_pixel(10, 10)[2] > 240);
    assert!(output.get_pixel(10, 10)[0] < 20);
}

#[test]
fn unicode_text_and_png_export_are_supported() {
    let source = png_bytes(RgbaImage::from_pixel(120, 60, Rgba([255, 255, 255, 255])));
    let mut document = AnnotationDocument::new(120, 60).unwrap();
    document.layers.push(Layer::new(
        "text",
        LayerKind::Text {
            origin: Point::new(4.0, 4.0).unwrap(),
            text: "Été 東京".into(),
            font: "Sans".into(),
            size: 20.0,
            color: AnnotationRgba::new(0.0, 0.0, 0.0, 1.0).unwrap(),
        },
    ));
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("edited.png");

    Renderer::default()
        .render_to_png(&source, &document, &output)
        .unwrap();

    let rendered = image::open(output).unwrap().to_rgba8();
    assert_eq!(rendered.dimensions(), (120, 60));
    assert!(rendered.pixels().any(|pixel| pixel[0] < 200));
}
