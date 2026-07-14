use std::{path::PathBuf, sync::Arc};

use chrono::Utc;
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_storage::{
    AppPaths, CaptureRepository, CaptureStore, NewCaptureRecord, Reconciler, open_database,
};
use uuid::Uuid;

struct Fixture {
    _directory: tempfile::TempDir,
    paths: AppPaths,
    repository: Arc<CaptureRepository>,
    reconciler: Reconciler,
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
        let repository = Arc::new(CaptureRepository::new(open_database(&paths).unwrap()));
        let reconciler = Reconciler::new(paths.clone(), repository.clone());
        Self {
            _directory: directory,
            paths,
            repository,
            reconciler,
        }
    }

    fn png(&self, directory: &std::path::Path, id: Uuid) -> PathBuf {
        let path = directory.join(format!("{id}.png"));
        image::RgbaImage::new(24, 16).save(&path).unwrap();
        path
    }

    fn insert_missing_row(&self) -> Uuid {
        let id = Uuid::new_v4();
        self.repository
            .insert(NewCaptureRecord {
                id,
                kind: CaptureKind::Screenshot,
                path: self.paths.captures.join(format!("{id}.png")),
                original_path: None,
                thumbnail_path: None,
                created_at: Utc::now(),
                width: 24,
                height: 16,
                duration: None,
                file_size: 0,
                target: CaptureTarget::Area,
                backend: DisplayServer::X11,
                annotation_json: None,
            })
            .unwrap();
        id
    }
}

#[test]
fn valid_orphan_is_offered_and_missing_row_is_not_deleted() {
    let fixture = Fixture::new();
    let orphan = fixture.png(&fixture.paths.orphans, Uuid::new_v4());
    let missing_id = fixture.insert_missing_row();

    let report = fixture.reconciler.scan().unwrap();

    assert_eq!(report.recoverable.len(), 1);
    assert_eq!(report.recoverable[0].path, orphan);
    assert!(report.unrecoverable.is_empty());
    assert_eq!(report.missing_files.len(), 1);
    assert_eq!(report.missing_files[0].id, missing_id);
    assert!(fixture.repository.get(&missing_id).unwrap().is_some());
}

#[test]
fn restore_moves_valid_media_inserts_a_row_and_generates_a_thumbnail() {
    let fixture = Fixture::new();
    let orphan = fixture.png(&fixture.paths.orphans, Uuid::new_v4());
    let candidate = fixture.reconciler.scan().unwrap().recoverable.remove(0);

    let record = fixture.reconciler.restore(&candidate).unwrap();

    assert!(!orphan.exists());
    assert!(record.path.exists());
    assert!(
        record
            .thumbnail_path
            .as_ref()
            .is_some_and(|path| path.exists())
    );
    assert_eq!(record.kind, CaptureKind::Screenshot);
    assert_eq!((record.width, record.height), (24, 16));
    assert!(fixture.repository.get(&record.id).unwrap().is_some());
}

#[test]
fn invalid_temporary_media_and_marker_escape_are_never_offered() {
    let fixture = Fixture::new();
    let invalid = fixture.paths.temporary.join("broken.gif");
    std::fs::write(&invalid, b"GIF89a").unwrap();
    let outside = fixture._directory.path().join("outside.png");
    image::RgbaImage::new(4, 4).save(&outside).unwrap();
    let marker = serde_json::json!({
        "session_id": Uuid::new_v4(),
        "kind": "screenshot",
        "backend": "x11",
        "temporary_path": outside,
        "target": "area",
        "started_at": Utc::now(),
    });
    std::fs::write(
        fixture.paths.temporary.join(".klypse-session.json"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();

    let report = fixture.reconciler.scan().unwrap();

    assert!(report.recoverable.is_empty());
    assert_eq!(report.unrecoverable.len(), 2);
    assert!(outside.exists());
}

#[cfg(unix)]
#[test]
fn discard_rejects_a_symlink_escape_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let outside = fixture._directory.path().join("outside.png");
    image::RgbaImage::new(4, 4).save(&outside).unwrap();
    let link = fixture.paths.temporary.join("linked.png");
    symlink(&outside, &link).unwrap();

    assert!(fixture.reconciler.discard_path(&link).is_err());
    assert!(outside.exists());
    assert!(link.exists());
}

#[test]
fn stale_thumbnail_is_reported_without_automatic_deletion() {
    let fixture = Fixture::new();
    let stale = fixture.paths.thumbnails.join("stale.png");
    image::RgbaImage::new(4, 4).save(&stale).unwrap();

    let report = fixture.reconciler.scan().unwrap();

    assert_eq!(report.stale_thumbnails, vec![stale.clone()]);
    assert!(stale.exists());
}

#[test]
fn restoring_a_marked_interrupted_gif_removes_its_exact_marker() {
    let fixture = Fixture::new();
    let id = Uuid::new_v4();
    let temporary = fixture.paths.temporary.join(format!("{id}.gif"));
    image::RgbaImage::new(8, 6).save(&temporary).unwrap();
    let marker_path = fixture.paths.temporary.join(".klypse-session.json");
    let marker = serde_json::json!({
        "session_id": id,
        "kind": "gif",
        "backend": "x11",
        "temporary_path": temporary,
        "target": "area",
        "started_at": Utc::now(),
    });
    std::fs::write(&marker_path, serde_json::to_vec(&marker).unwrap()).unwrap();
    let candidate = fixture.reconciler.scan().unwrap().recoverable.remove(0);

    let record = fixture.reconciler.restore(&candidate).unwrap();

    assert_eq!(record.id, id);
    assert!(!marker_path.exists());
    assert!(!temporary.exists());
}

#[test]
fn locating_a_missing_capture_validates_and_updates_the_gallery_row() {
    let fixture = Fixture::new();
    let id = fixture.insert_missing_row();
    let replacement = fixture._directory.path().join("replacement.png");
    image::RgbaImage::new(32, 20).save(&replacement).unwrap();

    let record = fixture
        .reconciler
        .relocate_missing(&id, &replacement)
        .unwrap();

    assert_eq!(record.path, replacement.canonicalize().unwrap());
    assert_eq!((record.width, record.height), (32, 20));
    assert!(
        record
            .thumbnail_path
            .as_ref()
            .is_some_and(|path| path.exists())
    );
    assert_eq!(
        fixture.repository.get(&id).unwrap().unwrap().path,
        replacement.canonicalize().unwrap()
    );
}
