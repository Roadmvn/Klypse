use std::{fs, path::Path};

use chrono::Utc;
use image::{Rgba, RgbaImage};
use klypse_app::editor::{EditorController, EditorTool};
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_image::Point;
use klypse_media::Thumbnailer;
use klypse_storage::{
    AppPaths, CaptureRecord, CaptureRepository, CaptureStore, NewCaptureRecord, open_database,
};
use uuid::Uuid;

struct Fixture {
    _directory: tempfile::TempDir,
    paths: AppPaths,
    repository: CaptureRepository,
    record: CaptureRecord,
    source: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("Pictures"),
        );
        paths.ensure().unwrap();
        let original = paths.captures.join("original.png");
        RgbaImage::from_pixel(40, 30, Rgba([255, 255, 255, 255]))
            .save(&original)
            .unwrap();
        let source = fs::read(&original).unwrap();
        let repository = CaptureRepository::new(open_database(&paths).unwrap());
        let record = repository
            .insert(NewCaptureRecord {
                id: Uuid::new_v4(),
                kind: CaptureKind::Screenshot,
                path: original,
                original_path: None,
                thumbnail_path: None,
                created_at: Utc::now(),
                width: 40,
                height: 30,
                duration: None,
                file_size: source.len() as u64,
                target: CaptureTarget::Area,
                backend: DisplayServer::X11,
                annotation_json: None,
            })
            .unwrap();
        Self {
            _directory: directory,
            paths,
            repository,
            record,
            source,
        }
    }

    fn edited_controller(&self) -> EditorController {
        let mut editor = EditorController::open(&self.record).unwrap();
        editor.set_tool(EditorTool::Rectangle);
        editor.pointer_down(Point::new(2.0, 2.0).unwrap()).unwrap();
        editor.pointer_up(Point::new(20.0, 15.0).unwrap()).unwrap();
        editor
    }
}

#[test]
fn saved_annotations_reopen_without_touching_original() {
    let fixture = Fixture::new();
    let original = fs::read(&fixture.record.path).unwrap();
    let mut editor = fixture.edited_controller();

    editor
        .save(&fixture.repository, &fixture.record.id)
        .unwrap();
    let stored = fixture.repository.get(&fixture.record.id).unwrap().unwrap();
    let reopened = EditorController::open(&stored).unwrap();

    assert_eq!(reopened.document().layers.len(), 1);
    assert!(!editor.is_dirty());
    assert_eq!(fs::read(&fixture.record.path).unwrap(), original);
}

#[test]
fn reopened_annotations_can_add_text_and_a_shape_then_save_again() {
    let fixture = Fixture::new();
    let mut editor = fixture.edited_controller();
    let first_layer = editor.document().layers[0].clone();
    let stored = editor
        .save_visible(
            &fixture.source,
            &fixture.paths,
            &fixture.repository,
            &fixture.record,
        )
        .unwrap();
    drop(editor);
    let mut reopened = EditorController::open(&stored).unwrap();

    reopened.set_tool(EditorTool::Text);
    reopened
        .pointer_down(Point::new(4.0, 4.0).unwrap())
        .unwrap();
    reopened.pointer_up(Point::new(4.0, 4.0).unwrap()).unwrap();
    assert!(reopened.set_text("Continued").unwrap());
    reopened.set_tool(EditorTool::Arrow);
    reopened
        .pointer_down(Point::new(5.0, 20.0).unwrap())
        .unwrap();
    reopened
        .pointer_up(Point::new(30.0, 20.0).unwrap())
        .unwrap();
    assert!(reopened.is_dirty());
    let expected = reopened.document().clone();
    reopened
        .save_visible(
            &fixture.source,
            &fixture.paths,
            &fixture.repository,
            &stored,
        )
        .unwrap();

    let saved = fixture.repository.get(&stored.id).unwrap().unwrap();
    let saved_editor = EditorController::open(&saved).unwrap();
    assert_eq!(saved_editor.document(), &expected);
    assert_eq!(saved_editor.document().layers.len(), 3);
    assert_eq!(saved_editor.document().layers[0], first_layer);
    assert!(!reopened.is_dirty());
    assert!(!saved_editor.is_dirty());
    assert_eq!(fs::read(&fixture.record.path).unwrap(), fixture.source);
}

#[test]
fn visible_save_updates_the_same_record_thumbnail_without_touching_original() {
    let fixture = Fixture::new();
    let original = fs::read(&fixture.record.path).unwrap();
    let previous_thumbnail = fixture.paths.thumbnails.join("previous.png");
    Thumbnailer::new(256)
        .generate(&fixture.record.path, &previous_thumbnail)
        .unwrap();
    fixture
        .repository
        .set_thumbnail(&fixture.record.id, &previous_thumbnail)
        .unwrap();
    let mut record = fixture.record.clone();
    record.thumbnail_path = Some(previous_thumbnail.clone());
    let mut editor = fixture.edited_controller();

    let updated = editor
        .save_visible(
            &fixture.source,
            &fixture.paths,
            &fixture.repository,
            &record,
        )
        .unwrap();
    let stored = fixture.repository.get(&record.id).unwrap().unwrap();

    assert_eq!(updated.id, record.id);
    assert_eq!(stored, updated);
    assert_eq!(fixture.repository.list_page(0, 50).unwrap().len(), 1);
    assert_eq!(fs::read(&record.path).unwrap(), original);
    assert!(!editor.is_dirty());
    assert!(updated.annotation_json.is_some());
    let thumbnail = updated.thumbnail_path.as_ref().unwrap();
    assert_ne!(thumbnail, &previous_thumbnail);
    assert!(thumbnail.exists());
    assert!(!previous_thumbnail.exists());
    let rendered_thumbnail = image::open(thumbnail).unwrap().to_rgba8();
    assert!(
        rendered_thumbnail
            .pixels()
            .any(|pixel| pixel.0 != [255, 255, 255, 255])
    );

    editor.set_tool(EditorTool::Line);
    editor.pointer_down(Point::new(4.0, 20.0).unwrap()).unwrap();
    editor.pointer_up(Point::new(30.0, 20.0).unwrap()).unwrap();
    let saved_again = editor
        .save_visible(
            &fixture.source,
            &fixture.paths,
            &fixture.repository,
            &record,
        )
        .unwrap();
    let stored_again = fixture.repository.get(&record.id).unwrap().unwrap();

    assert_eq!(stored_again, saved_again);
    assert_ne!(saved_again.thumbnail_path, updated.thumbnail_path);
    assert!(!thumbnail.exists());
    assert_eq!(fixture.repository.list_page(0, 50).unwrap().len(), 1);
    assert_eq!(
        EditorController::open(&stored_again)
            .unwrap()
            .document()
            .layers
            .len(),
        2
    );
    assert_eq!(fs::read(&record.path).unwrap(), original);
    assert!(!editor.is_dirty());
}

#[test]
fn flattened_export_creates_a_new_gallery_png_and_thumbnail() {
    let fixture = Fixture::new();
    let editor = fixture.edited_controller();

    let exported = editor
        .export_flattened(
            &fixture.source,
            &fixture.paths,
            &fixture.repository,
            &fixture.record,
        )
        .unwrap();

    assert!(exported.path.exists());
    assert_ne!(exported.path, fixture.record.path);
    assert_eq!(exported.original_path, Some(fixture.record.path.clone()));
    assert!(
        exported
            .thumbnail_path
            .as_ref()
            .is_some_and(|path| Path::exists(path))
    );
    assert_eq!(fixture.repository.list_page(0, 50).unwrap().len(), 2);
    assert!(
        exported
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("original-edited-")
    );
}

#[test]
fn malformed_stored_annotation_is_reported_as_corrupt() {
    let fixture = Fixture::new();
    let mut record = fixture.record.clone();
    record.annotation_json = Some("{ definitely not valid json".into());

    let error = EditorController::open(&record).err().unwrap();

    assert!(error.to_string().contains("corrupt annotation"));
}
