# Klypse Gallery and Storage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist capture metadata and files atomically, generate thumbnails asynchronously, and present a paginated gallery that remains responsive with 1,000 records.

**Architecture:** `klypse-storage` owns XDG paths, SQLite migrations, and repository transactions. `klypse-media` creates thumbnails, while the app exposes a GTK-independent gallery controller that feeds a GTK grid in pages.

**Tech Stack:** Rust, rusqlite, SQLite, image, tempfile, GTK 4, GLib futures

## Global Constraints

- The MVP targets Linux and supports X11 and Wayland from its first release.
- The application ID is `io.github.roadmvn.Klypse`.
- Static captures use PNG; silent videos use WebM/VP8; GIF defaults are 12 FPS and 30 seconds maximum.
- Runtime data follows XDG directories and no MVP feature performs network requests.
- User cancellation is not an error and capture contents are never written to logs.
- Source strings are English and ship with a French Gettext catalog.
- Every task ends with focused tests and an intentional commit.

---

## File Map

- `crates/klypse-storage/src/paths.rs`: resolves data, cache, runtime, capture, and orphan locations.
- `crates/klypse-storage/src/migration.rs`: creates and upgrades the SQLite schema transactionally.
- `crates/klypse-storage/src/repository.rs`: inserts, lists, removes, and reconciles capture records.
- `crates/klypse-storage/src/atomic_file.rs`: persists temporary outputs without partial gallery entries.
- `crates/klypse-media/src/thumbnail.rs`: creates bounded thumbnails for images and media posters.
- `crates/klypse-app/src/gallery/controller.rs`: paginated gallery state and background thumbnail jobs.
- `crates/klypse-app/src/ui/gallery.rs`: GTK grid, empty state, and detail actions.

### Task 1: XDG paths and SQLite migration

**Files:**
- Modify: `crates/klypse-storage/Cargo.toml`
- Modify: `crates/klypse-storage/src/lib.rs`
- Create: `crates/klypse-storage/src/paths.rs`
- Create: `crates/klypse-storage/src/migration.rs`
- Test: `crates/klypse-storage/tests/paths_and_migration.rs`

**Interfaces:**
- Produces: `AppPaths::discover`, `AppPaths::ensure`, `open_database`, and schema version 1.

- [ ] **Step 1: Write failing path and migration tests**

```rust
#[test]
fn paths_use_injected_xdg_roots() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_roots(root.path().join("data"), root.path().join("cache"), root.path().join("run"), root.path().join("Pictures"));
    assert_eq!(paths.database, root.path().join("data/klypse/library.sqlite3"));
    assert_eq!(paths.thumbnails, root.path().join("cache/klypse/thumbnails"));
    assert_eq!(paths.captures, root.path().join("Pictures/Klypse"));
}

#[test]
fn migration_creates_schema_version_one() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    migrate(&mut connection).unwrap();
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
    assert_eq!(version, 1);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p klypse-storage --test paths_and_migration`

Expected: compilation fails because `AppPaths` and `migrate` are undefined.

- [ ] **Step 3: Implement paths and schema**

Use `directories::BaseDirs` for XDG-compatible roots, `$XDG_RUNTIME_DIR` when present, and a user-only cache fallback when absent. `ensure` creates `captures`, `thumbnails`, `temporary`, and `orphans` directories.

Migration 1 is exact:

```sql
CREATE TABLE captures (
  id TEXT PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('screenshot','video','gif')),
  path TEXT NOT NULL UNIQUE,
  original_path TEXT,
  thumbnail_path TEXT,
  created_at TEXT NOT NULL,
  width INTEGER NOT NULL CHECK(width >= 0),
  height INTEGER NOT NULL CHECK(height >= 0),
  duration_ms INTEGER,
  file_size INTEGER NOT NULL CHECK(file_size >= 0),
  target TEXT NOT NULL,
  backend TEXT NOT NULL CHECK(backend IN ('x11','wayland')),
  annotation_json TEXT
);
CREATE INDEX captures_created_at_idx ON captures(created_at DESC, id DESC);
PRAGMA user_version = 1;
```

Run migrations inside an immediate transaction. Reject a database with a newer schema using `StorageError::UnsupportedSchema`.

- [ ] **Step 4: Run storage checks**

Run: `cargo test -p klypse-storage --test paths_and_migration && cargo clippy -p klypse-storage --all-targets -- -D warnings`

Expected: tests pass and clippy is clean.

- [ ] **Step 5: Commit path and migration support**

```bash
git add crates/klypse-storage
git commit -m "feat: add XDG storage paths and schema"
```

### Task 2: Atomic file persistence and capture repository

**Files:**
- Create: `crates/klypse-storage/src/model.rs`
- Create: `crates/klypse-storage/src/atomic_file.rs`
- Create: `crates/klypse-storage/src/repository.rs`
- Modify: `crates/klypse-storage/src/lib.rs`
- Test: `crates/klypse-storage/tests/repository.rs`

**Interfaces:**
- Consumes: `CaptureArtifact`, `CaptureKind`, `CaptureTarget`, and `DisplayServer`.
- Produces: `CaptureRecord`, `NewCaptureRecord`, `DeleteMode`, the `CaptureStore` trait, its `CaptureRepository` SQLite implementation, and `AtomicCaptureFile::commit`.

- [ ] **Step 1: Write failing repository tests**

```rust
#[test]
fn pages_are_newest_first_and_stable() {
    let fixture = RepositoryFixture::new();
    let older = uuid::Uuid::from_u128(1);
    let newer = uuid::Uuid::from_u128(2);
    fixture.insert_at(older, "2026-01-01T00:00:00Z");
    fixture.insert_at(newer, "2026-01-02T00:00:00Z");
    let page = fixture.repository.list_page(0, 50).unwrap();
    assert_eq!(page.iter().map(|r| r.id).collect::<Vec<_>>(), [newer, older]);
}

#[test]
fn gallery_only_delete_keeps_the_file() {
    let fixture = RepositoryFixture::with_png();
    let record = fixture.insert_png();
    fixture.repository.delete(&record.id, DeleteMode::GalleryOnly).unwrap();
    assert!(record.path.exists());
    assert!(fixture.repository.get(&record.id).unwrap().is_none());
}
```

- [ ] **Step 2: Verify repository tests fail**

Run: `cargo test -p klypse-storage --test repository`

Expected: compilation fails because repository types are missing.

- [ ] **Step 3: Implement typed mapping and transactions**

`CaptureRecord` uses typed enums from `klypse-domain`, `Uuid`, `DateTime<Utc>`, `PathBuf`, `Option<Duration>`, and `Option<String>` for annotation JSON. `CaptureStore` is a `Send + Sync` trait exposing `insert`, `list_page`, `get`, `delete`, `set_thumbnail`, and `set_annotation`; `CaptureRepository` is its SQLite implementation. `list_page(offset, limit)` enforces `1..=200` and orders by `created_at DESC, id DESC`.

`AtomicCaptureFile` writes in the temporary directory, calls `sync_all`, renames into the configured capture directory, syncs the destination directory, and only then allows the repository insert. If insertion fails, move the destination to `orphans/<id>.<ext>`.

`DeleteMode::GalleryAndFile` first removes the row transactionally, then removes the file and thumbnail. If file removal fails, restore the row from the captured record and return the I/O error.

- [ ] **Step 4: Run repository and corruption tests**

Run: `cargo test -p klypse-storage && cargo fmt --all --check && cargo clippy -p klypse-storage --all-targets -- -D warnings`

Expected: all storage tests pass.

- [ ] **Step 5: Commit the repository**

```bash
git add crates/klypse-storage
git commit -m "feat: persist capture records atomically"
```

### Task 3: Thumbnail generation

**Files:**
- Modify: `crates/klypse-media/Cargo.toml`
- Modify: `crates/klypse-media/src/lib.rs`
- Create: `crates/klypse-media/src/thumbnail.rs`
- Test: `crates/klypse-media/tests/thumbnail.rs`

**Interfaces:**
- Produces: `Thumbnailer::new(max_edge: u32)` and `Thumbnailer::generate(source, destination) -> Result<ThumbnailInfo, MediaError>`.

- [ ] **Step 1: Write a failing bounded-thumbnail test**

```rust
#[test]
fn thumbnail_preserves_aspect_ratio_and_bounds() {
    let fixture = ImageFixture::png(1200, 600);
    let destination = fixture.directory.path().join("thumb.png");
    let info = Thumbnailer::new(256).generate(&fixture.source, &destination).unwrap();
    assert_eq!((info.width, info.height), (256, 128));
    assert!(destination.exists());
}
```

- [ ] **Step 2: Verify the thumbnail test fails**

Run: `cargo test -p klypse-media --test thumbnail`

Expected: compilation fails because `Thumbnailer` is missing.

- [ ] **Step 3: Implement image and media poster thumbnails**

Enable the workspace `image`, `gstreamer`, `gstreamer-app`, and `gstreamer-video` dependencies in `klypse-media`. Use `image 0.25.10` for PNG/GIF input, Lanczos3 resizing, and PNG output. Refuse a zero maximum edge. Correct EXIF orientation when metadata is available. For WebM, request one RGB frame at timestamp zero from a GStreamer `uridecodebin ! videoconvert ! appsink` pipeline and pass it through the same resize function.

All destination writes use a sibling temporary file followed by rename. Return width, height, and destination path without logging source pixels.

- [ ] **Step 4: Run thumbnail tests**

Run: `cargo test -p klypse-media --test thumbnail && cargo clippy -p klypse-media --all-targets -- -D warnings`

Expected: image tests pass; the WebM test skips only when the test pipeline reports a missing plugin.

- [ ] **Step 5: Commit thumbnail support**

```bash
git add crates/klypse-media
git commit -m "feat: generate gallery thumbnails"
```

### Task 4: Paginated gallery controller and GTK grid

**Files:**
- Create: `crates/klypse-app/src/gallery/mod.rs`
- Create: `crates/klypse-app/src/gallery/controller.rs`
- Create: `crates/klypse-app/src/ui/gallery.rs`
- Modify: `crates/klypse-app/src/ui/mod.rs`
- Modify: `crates/klypse-app/src/ui/window.rs`
- Modify: `crates/klypse-app/src/application.rs`
- Test: `crates/klypse-app/tests/gallery_controller.rs`

**Interfaces:**
- Consumes: `CaptureStore::list_page`, `CaptureStore::set_thumbnail`, and `Thumbnailer::generate`.
- Produces: `GalleryController::{load_initial,load_next,refresh,delete}` and `GalleryPage { items, has_more }`.

- [ ] **Step 1: Write failing controller pagination tests**

```rust
#[test]
fn controller_loads_fifty_items_per_page_without_duplicates() {
    let repository = FakeRepository::with_records(101);
    let mut controller = GalleryController::new(repository, 50);
    controller.load_initial().unwrap();
    controller.load_next().unwrap();
    controller.load_next().unwrap();
    assert_eq!(controller.items().len(), 101);
    assert!(!controller.has_more());
}
```

- [ ] **Step 2: Verify controller tests fail**

Run: `cargo test -p klypse-app --test gallery_controller`

Expected: compilation fails because the gallery controller is absent.

- [ ] **Step 3: Implement controller and GTK gallery**

Keep pagination and selection in a GTK-independent controller using a repository trait. The UI uses `gtk::GridView`, `gio::ListStore`, `gtk::SignalListItemFactory`, and `gtk::SingleSelection`. Each item shows a 256-pixel thumbnail, media-kind icon, and localized timestamp.

Load the first 50 records during activation. Trigger `load_next` when the vertical adjustment reaches 80% of its range. Generate missing thumbnails on `std::thread::spawn` workers and return results through an `async_channel` receiver consumed by `glib::MainContext::spawn_local`. Update GTK models only on the GLib main thread. The empty state is visible only when the first page is empty.

The detail pane exposes Copy, Edit, Reveal in Folder, Remove from Gallery, and Delete File actions, disabling Edit for video and GIF records.

- [ ] **Step 4: Run gallery tests and headless smoke test**

Run: `cargo test -p klypse-app --test gallery_controller && cargo test --workspace`

Expected: controller and workspace tests pass.

Run: `xvfb-run -a timeout 3 cargo run -p klypse-app`

Expected: gallery shell starts without panic and timeout exits 124.

- [ ] **Step 5: Commit the gallery slice**

```bash
git add crates/klypse-app
git commit -m "feat: add the persistent capture gallery"
```
