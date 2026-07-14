use std::{fs, path::Path};

use chrono::Utc;
use image::{Rgba, RgbaImage};
use klypse_app::editor::{EditorController, EditorTool};
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_image::Point;
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
