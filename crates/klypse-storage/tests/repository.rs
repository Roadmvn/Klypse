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
fn gallery_only_delete_keeps_the_capture_file_and_cleans_its_thumbnail() {
    let fixture = RepositoryFixture::new();
    let mut record = fixture.insert_at(Uuid::new_v4(), "2026-01-01T00:00:00Z");
    let thumbnail = fixture.paths.thumbnails.join("gallery-only-thumb.png");
    fs::write(&thumbnail, b"thumbnail").unwrap();
    fixture
        .repository
        .set_thumbnail(&record.id, &thumbnail)
        .unwrap();
    record.thumbnail_path = Some(thumbnail.clone());

    fixture
        .repository
        .delete(&record.id, DeleteMode::GalleryOnly)
        .unwrap();

    assert!(record.path.exists());
    assert!(!thumbnail.exists());
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
fn delete_many_removes_only_the_requested_uuid_set() {
    let fixture = RepositoryFixture::new();
    for index in 0..80_u128 {
        fixture.insert_at(Uuid::from_u128(index + 1), "2026-01-01T00:00:00Z");
    }
    let records = fixture.repository.list_page(0, 80).unwrap();
    let selected = [records[1].id, records[49].id, records[74].id];
    let unselected = [records[0].id, records[2].id, records[48].id, records[75].id];

    let deleted = fixture
        .repository
        .delete_many(
            &[selected[0], selected[1], selected[2], selected[1]],
            DeleteMode::GalleryOnly,
        )
        .unwrap();

    assert_eq!(deleted, selected.len());
    for id in selected {
        assert!(fixture.repository.get(&id).unwrap().is_none());
    }
    for id in unselected {
        assert!(fixture.repository.get(&id).unwrap().is_some());
    }
}

#[test]
fn delete_many_validates_every_uuid_before_deleting_anything() {
    let fixture = RepositoryFixture::new();
    let first = fixture.insert_at(Uuid::from_u128(1), "2026-01-01T00:00:00Z");
    let second = fixture.insert_at(Uuid::from_u128(2), "2026-01-01T00:00:00Z");
    let missing = Uuid::from_u128(3);

    let error = fixture
        .repository
        .delete_many(&[first.id, missing, second.id], DeleteMode::GalleryAndFile)
        .unwrap_err();

    assert!(matches!(error, klypse_storage::StorageError::CaptureNotFound(id) if id == missing));
    assert!(fixture.repository.get(&first.id).unwrap().is_some());
    assert!(fixture.repository.get(&second.id).unwrap().is_some());
    assert!(first.path.exists());
    assert!(second.path.exists());
}

#[test]
fn failed_media_delete_restores_the_row_without_a_missing_thumbnail() {
    let fixture = RepositoryFixture::new();
    let id = Uuid::new_v4();
    let media_directory = fixture.paths.captures.join("undeletable-as-file");
    let thumbnail = fixture.paths.thumbnails.join("removed-before-failure.png");
    fs::create_dir_all(&media_directory).unwrap();
    fs::write(&thumbnail, b"thumbnail").unwrap();
    fixture
        .repository
        .insert(NewCaptureRecord {
            id,
            kind: CaptureKind::Screenshot,
            path: media_directory,
            original_path: None,
            thumbnail_path: Some(thumbnail.clone()),
            created_at: "2026-01-01T00:00:00Z".parse().unwrap(),
            width: 640,
            height: 480,
            duration: None,
            file_size: 0,
            target: CaptureTarget::Area,
            backend: DisplayServer::X11,
            annotation_json: None,
        })
        .unwrap();

    assert!(
        fixture
            .repository
            .delete(&id, DeleteMode::GalleryAndFile)
            .is_err()
    );

    let restored = fixture.repository.get(&id).unwrap().unwrap();
    assert!(restored.thumbnail_path.is_none());
    assert!(!thumbnail.exists());
}

#[test]
fn bulk_gallery_only_delete_keeps_every_file() {
    let fixture = RepositoryFixture::new();
    let records = (0..205)
        .map(|index| fixture.insert_at(Uuid::from_u128(index + 1), "2026-01-01T00:00:00Z"))
        .collect::<Vec<_>>();
    let thumbnail = fixture.paths.thumbnails.join("bulk-gallery-thumb.png");
    fs::write(&thumbnail, b"thumbnail").unwrap();
    fixture
        .repository
        .set_thumbnail(&records[0].id, &thumbnail)
        .unwrap();

    let deleted = fixture
        .repository
        .delete_all(DeleteMode::GalleryOnly)
        .unwrap();

    assert_eq!(deleted, records.len());
    assert!(fixture.repository.list_page(0, 200).unwrap().is_empty());
    assert!(records.iter().all(|record| record.path.exists()));
    assert!(!thumbnail.exists());
}

#[test]
fn bulk_file_delete_removes_media_and_thumbnail_but_preserves_original_path() {
    let fixture = RepositoryFixture::new();
    let id = Uuid::new_v4();
    let path = fixture.paths.captures.join(format!("{id}.png"));
    let original = fixture.paths.captures.join("source-original.png");
    let thumbnail = fixture.paths.thumbnails.join("bulk-thumb.png");
    fs::write(&path, b"capture").unwrap();
    fs::write(&original, b"original").unwrap();
    fs::write(&thumbnail, b"thumbnail").unwrap();
    let record = fixture
        .repository
        .insert(NewCaptureRecord {
            id,
            kind: CaptureKind::Screenshot,
            path,
            original_path: Some(original.clone()),
            thumbnail_path: Some(thumbnail.clone()),
            created_at: "2026-01-01T00:00:00Z".parse().unwrap(),
            width: 640,
            height: 480,
            duration: None,
            file_size: 7,
            target: CaptureTarget::Area,
            backend: DisplayServer::X11,
            annotation_json: None,
        })
        .unwrap();

    let deleted = fixture
        .repository
        .delete_all(DeleteMode::GalleryAndFile)
        .unwrap();

    assert_eq!(deleted, 1);
    assert!(!record.path.exists());
    assert!(!thumbnail.exists());
    assert!(original.exists());
    assert!(fixture.repository.list_page(0, 200).unwrap().is_empty());
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
