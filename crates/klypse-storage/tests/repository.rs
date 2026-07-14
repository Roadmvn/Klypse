use std::{fs, path::PathBuf, time::Duration};

use chrono::{DateTime, Utc};
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_storage::{
    AppPaths, AtomicCaptureFile, CaptureRepository, CaptureStore, DeleteMode, NewCaptureRecord,
    open_database,
};
use tempfile::TempDir;
use uuid::Uuid;

struct RepositoryFixture {
    _directory: TempDir,
    paths: AppPaths,
    repository: CaptureRepository,
}

impl RepositoryFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("Pictures"),
        );
        let connection = open_database(&paths).unwrap();
        let repository = CaptureRepository::new(connection);
        Self {
            _directory: directory,
            paths,
            repository,
        }
    }

    fn insert_at(&self, id: Uuid, created_at: &str) -> klypse_storage::CaptureRecord {
        let path = self.paths.captures.join(format!("{id}.png"));
        fs::write(&path, b"png fixture").unwrap();
        self.repository
            .insert(NewCaptureRecord {
                id,
                kind: CaptureKind::Screenshot,
                path,
                original_path: None,
                thumbnail_path: None,
                created_at: created_at.parse::<DateTime<Utc>>().unwrap(),
                width: 640,
                height: 480,
                duration: None,
                file_size: 11,
                target: CaptureTarget::Area,
                backend: DisplayServer::X11,
                annotation_json: None,
            })
            .unwrap()
    }
}

#[test]
fn pages_are_newest_first_and_stable() {
    let fixture = RepositoryFixture::new();
    let older = Uuid::from_u128(1);
    let newer = Uuid::from_u128(2);
    fixture.insert_at(older, "2026-01-01T00:00:00Z");
    fixture.insert_at(newer, "2026-01-02T00:00:00Z");

    let page = fixture.repository.list_page(0, 50).unwrap();

    assert_eq!(
        page.iter().map(|record| record.id).collect::<Vec<_>>(),
        [newer, older]
    );
}

#[test]
fn gallery_only_delete_keeps_the_file() {
    let fixture = RepositoryFixture::new();
    let record = fixture.insert_at(Uuid::new_v4(), "2026-01-01T00:00:00Z");

    fixture
        .repository
        .delete(&record.id, DeleteMode::GalleryOnly)
        .unwrap();

    assert!(record.path.exists());
    assert!(fixture.repository.get(&record.id).unwrap().is_none());
}

#[test]
fn gallery_and_file_delete_removes_both_files() {
    let fixture = RepositoryFixture::new();
    let mut record = fixture.insert_at(Uuid::new_v4(), "2026-01-01T00:00:00Z");
    let thumbnail = fixture.paths.thumbnails.join("thumb.png");
    fs::write(&thumbnail, b"thumbnail").unwrap();
    fixture
        .repository
        .set_thumbnail(&record.id, &thumbnail)
        .unwrap();
    record.thumbnail_path = Some(thumbnail.clone());

    fixture
        .repository
        .delete(&record.id, DeleteMode::GalleryAndFile)
        .unwrap();

    assert!(!record.path.exists());
    assert!(!thumbnail.exists());
    assert!(fixture.repository.get(&record.id).unwrap().is_none());
}

#[test]
fn list_page_rejects_unbounded_limits() {
    let fixture = RepositoryFixture::new();

    assert!(fixture.repository.list_page(0, 0).is_err());
    assert!(fixture.repository.list_page(0, 201).is_err());
}

#[test]
fn atomic_capture_file_is_invisible_until_commit() {
    let fixture = RepositoryFixture::new();
    let id = Uuid::new_v4();
    let mut output = AtomicCaptureFile::new(&fixture.paths, id, "png").unwrap();
    std::io::Write::write_all(&mut output, b"complete png").unwrap();
    let final_path = fixture.paths.captures.join(format!("{id}.png"));

    assert!(!final_path.exists());
    let committed = output.commit().unwrap();

    assert_eq!(committed, final_path);
    assert_eq!(fs::read(committed).unwrap(), b"complete png");
}

#[test]
fn repository_round_trips_duration_and_annotation() {
    let fixture = RepositoryFixture::new();
    let id = Uuid::new_v4();
    let path = fixture.paths.captures.join(format!("{id}.webm"));
    fs::write(&path, b"webm").unwrap();
    fixture
        .repository
        .insert(NewCaptureRecord {
            id,
            kind: CaptureKind::Video,
            path: PathBuf::from(&path),
            original_path: None,
            thumbnail_path: None,
            created_at: "2026-01-01T00:00:00Z".parse().unwrap(),
            width: 1920,
            height: 1080,
            duration: Some(Duration::from_millis(1_234)),
            file_size: 4,
            target: CaptureTarget::Screen,
            backend: DisplayServer::Wayland,
            annotation_json: Some("{\"version\":1}".into()),
        })
        .unwrap();

    let record = fixture.repository.get(&id).unwrap().unwrap();

    assert_eq!(record.duration, Some(Duration::from_millis(1_234)));
    assert_eq!(record.annotation_json.as_deref(), Some("{\"version\":1}"));
    assert_eq!(record.backend, DisplayServer::Wayland);
}
