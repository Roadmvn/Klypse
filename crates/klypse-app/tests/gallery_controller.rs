use chrono::Utc;
use klypse_app::gallery::GalleryController;
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_storage::{
    AppPaths, CaptureRepository, CaptureStore, DeleteMode, NewCaptureRecord, open_database,
};
use uuid::Uuid;

struct GalleryFixture {
    _directory: tempfile::TempDir,
    repository: CaptureRepository,
}

impl GalleryFixture {
    fn with_records(count: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_roots(
            directory.path().join("data"),
            directory.path().join("cache"),
            directory.path().join("run"),
            directory.path().join("Pictures"),
        );
        let repository = CaptureRepository::new(open_database(&paths).unwrap());
        for index in 0..count {
            let id = Uuid::from_u128(index as u128 + 1);
            repository
                .insert(NewCaptureRecord {
                    id,
                    kind: CaptureKind::Screenshot,
                    path: paths.captures.join(format!("{id}.png")),
                    original_path: None,
                    thumbnail_path: None,
                    created_at: Utc::now(),
                    width: 1280,
                    height: 720,
                    duration: None,
                    file_size: 0,
                    target: CaptureTarget::Area,
                    backend: DisplayServer::X11,
                    annotation_json: None,
                })
                .unwrap();
        }
        Self {
            _directory: directory,
            repository,
        }
    }
}

#[test]
fn controller_loads_fifty_items_per_page_without_duplicates() {
    let fixture = GalleryFixture::with_records(101);
    let mut controller = GalleryController::new(fixture.repository, 50).unwrap();

    controller.load_initial().unwrap();
    assert_eq!(controller.items().len(), 50);
    assert!(controller.has_more());
    controller.load_next().unwrap();
    controller.load_next().unwrap();

    assert_eq!(controller.items().len(), 101);
    assert!(!controller.has_more());
    let mut ids = controller
        .items()
        .iter()
        .map(|record| record.id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 101);
}

#[test]
fn refresh_and_delete_keep_controller_state_consistent() {
    let fixture = GalleryFixture::with_records(2);
    let mut controller = GalleryController::new(fixture.repository, 50).unwrap();
    controller.load_initial().unwrap();
    let removed = controller.items()[0].id;

    controller
        .delete(&removed, DeleteMode::GalleryOnly)
        .unwrap();
    controller.refresh().unwrap();

    assert_eq!(controller.items().len(), 1);
    assert_ne!(controller.items()[0].id, removed);
    assert!(!controller.has_more());
}

#[test]
fn controller_rejects_invalid_page_sizes() {
    let fixture = GalleryFixture::with_records(0);

    assert!(GalleryController::new(fixture.repository, 0).is_err());
}
