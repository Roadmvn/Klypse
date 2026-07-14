# Klypse Non-destructive Image Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a non-destructive screenshot editor with crop, shapes, arrows, lines, text, freehand drawing, redaction, undo/redo, and flattened PNG export.

**Architecture:** `klypse-image` owns a versioned annotation document and deterministic renderer independent from GTK. The app editor manipulates this document in image coordinates, stores its JSON in SQLite, and renders previews or exports through the same engine.

**Tech Stack:** Rust, serde, cairo-rs, Pango/PangoCairo, image, GTK 4 drawing area

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

- `crates/klypse-image/src/document.rs`: versioned annotation data and tool-layer types.
- `crates/klypse-image/src/geometry.rs`: coordinates, normalization, crop, and viewport transforms.
- `crates/klypse-image/src/history.rs`: bounded undo/redo command history.
- `crates/klypse-image/src/render.rs`: flattened Cairo/image rendering.
- `crates/klypse-image/src/redaction.rs`: pixelation and blur algorithms.
- `crates/klypse-app/src/editor/controller.rs`: active tool, selection, history, save, and export state.
- `crates/klypse-app/src/ui/editor.rs`: toolbar, canvas, text input, colors, widths, undo, and redo.

### Task 1: Annotation document, geometry, and history

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/klypse-image/Cargo.toml`
- Modify: `crates/klypse-image/src/lib.rs`
- Create: `crates/klypse-image/src/document.rs`
- Create: `crates/klypse-image/src/geometry.rs`
- Create: `crates/klypse-image/src/history.rs`
- Test: `crates/klypse-image/tests/document.rs`
- Test: `crates/klypse-image/tests/history.rs`

**Interfaces:**
- Produces: `AnnotationDocument`, `Layer`, `LayerKind`, `Rect`, `Point`, `Rgba`, `Stroke`, `ViewportTransform`, and `EditHistory`.

- [ ] **Step 1: Write failing document and history tests**

```rust
#[test]
fn document_round_trip_preserves_all_layer_kinds() {
    let document = AnnotationDocument::fixture_with_every_layer(1920, 1080);
    let json = serde_json::to_string(&document).unwrap();
    let decoded: AnnotationDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, document);
    assert_eq!(decoded.version, 1);
}

#[test]
fn undo_then_new_edit_discards_redo_branch() {
    let mut history = EditHistory::new(100);
    history.push(DocumentCommand::AddLayer(layer("a")));
    history.push(DocumentCommand::AddLayer(layer("b")));
    history.undo().unwrap();
    history.push(DocumentCommand::AddLayer(layer("c")));
    assert!(!history.can_redo());
}
```

- [ ] **Step 2: Verify editor-model tests fail**

Run: `cargo test -p klypse-image --test document --test history`

Expected: compilation fails because document and history types are absent.

- [ ] **Step 3: Implement a complete version-1 document**

Add `cairo-rs = "0.22.0"`, `pango = "0.22.8"`, and `pangocairo = "0.22.8"` to workspace dependencies.

Use these layer variants:

```rust
pub enum LayerKind {
    Rectangle { rect: Rect, stroke: Stroke, fill: Option<Rgba> },
    Ellipse { rect: Rect, stroke: Stroke, fill: Option<Rgba> },
    Line { start: Point, end: Point, stroke: Stroke },
    Arrow { start: Point, end: Point, stroke: Stroke, head_length: f64 },
    Text { origin: Point, text: String, font: String, size: f64, color: Rgba },
    Freehand { points: Vec<Point>, stroke: Stroke },
    Redaction { rect: Rect, mode: RedactionMode },
}
```

`AnnotationDocument` contains `version: u32`, original dimensions, optional crop rectangle, ordered layers, and rejects non-finite coordinates, empty freehand paths, empty text, invalid colors, negative sizes, and crops outside the original image.

`EditHistory` stores reversible `AddLayer`, `RemoveLayer`, `ReplaceLayer`, and `SetCrop` commands with a 100-command default limit. Viewport transforms preserve image-coordinate storage at every zoom level.

- [ ] **Step 4: Run model tests and clippy**

Run: `cargo test -p klypse-image --test document --test history && cargo clippy -p klypse-image --all-targets -- -D warnings`

Expected: all tests pass and clippy is clean.

- [ ] **Step 5: Commit editor models**

```bash
git add Cargo.toml crates/klypse-image
git commit -m "feat: define non-destructive annotation documents"
```

### Task 2: Deterministic flattening and redaction renderer

**Files:**
- Create: `crates/klypse-image/src/render.rs`
- Create: `crates/klypse-image/src/redaction.rs`
- Modify: `crates/klypse-image/src/lib.rs`
- Test: `crates/klypse-image/tests/render.rs`
- Test: `crates/klypse-image/tests/redaction.rs`

**Interfaces:**
- Consumes: validated `AnnotationDocument` and a source PNG.
- Produces: `Renderer::render_to_rgba`, `Renderer::render_to_png`, `pixelate_region`, and `blur_region`.

- [ ] **Step 1: Write failing pixel and dimension tests**

```rust
#[test]
fn crop_sets_exact_output_dimensions() {
    let source = TestImage::checkerboard(100, 80);
    let document = AnnotationDocument::new(100, 80).with_crop(Rect::new(10.0, 20.0, 30.0, 40.0).unwrap());
    let output = Renderer::default().render_to_rgba(source.bytes(), &document).unwrap();
    assert_eq!((output.width(), output.height()), (30, 40));
}

#[test]
fn pixelation_replaces_each_block_with_one_color() {
    let mut image = TestImage::gradient(16, 16).into_rgba();
    pixelate_region(&mut image, PixelRect::new(0, 0, 16, 16), 8).unwrap();
    assert_eq!(image.get_pixel(0, 0), image.get_pixel(7, 7));
    assert_ne!(image.get_pixel(0, 0), image.get_pixel(8, 8));
}
```

- [ ] **Step 2: Verify renderer tests fail**

Run: `cargo test -p klypse-image --test render --test redaction`

Expected: compilation fails because renderer functions are missing.

- [ ] **Step 3: Implement the renderer in stable layer order**

Decode the source through `image`, apply crop before drawing, and create an ARGB32 Cairo image surface. Draw visible layers in vector order with `Context::save`/`restore` around each layer. Use round line caps for freehand strokes, an explicit arrowhead polygon, and PangoCairo for UTF-8 text.

Redaction reads only the current flattened pixels inside its rectangle. Pixelation uses 12-pixel blocks by default and writes the average premultiplied RGBA color per block. Blur uses a separable box blur with radius 8 and clamps reads to the redaction rectangle so adjacent pixels are not leaked inward.

Export converts the Cairo buffer back to straight RGBA and writes a PNG to a caller-provided temporary path. Never overwrite the original source.

- [ ] **Step 4: Run deterministic renderer tests**

Run: `cargo test -p klypse-image --test render --test redaction`

Expected: dimensions, shape pixels, crop translation, layer order, pixelation, blur bounds, and Unicode text smoke tests pass.

- [ ] **Step 5: Commit rendering support**

```bash
git add crates/klypse-image
git commit -m "feat: render and redact annotated screenshots"
```

### Task 3: GTK editor controller and canvas

**Files:**
- Create: `crates/klypse-app/src/editor/mod.rs`
- Create: `crates/klypse-app/src/editor/controller.rs`
- Create: `crates/klypse-app/src/ui/editor.rs`
- Modify: `crates/klypse-app/src/ui/mod.rs`
- Modify: `crates/klypse-app/src/ui/window.rs`
- Modify: `crates/klypse-app/src/ui/gallery.rs`
- Test: `crates/klypse-app/tests/editor_controller.rs`

**Interfaces:**
- Consumes: `AnnotationDocument`, `EditHistory`, `ViewportTransform`, and a screenshot `CaptureRecord`.
- Produces: `EditorController::{open,set_tool,pointer_down,pointer_move,pointer_up,set_text,undo,redo,save,export}` and `EditorTool`.

- [ ] **Step 1: Write failing controller gesture tests**

```rust
#[test]
fn rectangle_gesture_creates_one_normalized_layer() {
    let mut editor = EditorFixture::open_100x100();
    editor.set_tool(EditorTool::Rectangle);
    editor.pointer_down(ViewPoint::new(80.0, 70.0));
    editor.pointer_move(ViewPoint::new(20.0, 10.0));
    editor.pointer_up(ViewPoint::new(20.0, 10.0));
    assert_eq!(editor.document().layers.len(), 1);
    assert_eq!(editor.document().layers[0].bounds(), Rect::new(20.0, 10.0, 60.0, 60.0).unwrap());
}

#[test]
fn escape_cancels_an_in_progress_gesture() {
    let mut editor = EditorFixture::open_100x100();
    editor.pointer_down(ViewPoint::new(10.0, 10.0));
    editor.cancel_current_action();
    assert!(editor.document().layers.is_empty());
}
```

- [ ] **Step 2: Verify controller tests fail**

Run: `cargo test -p klypse-app --test editor_controller`

Expected: compilation fails because editor controller types are missing.

- [ ] **Step 3: Implement tools and GTK editor surface**

The controller owns the document, history, active tool, draft layer, zoom, and selected color/stroke width. Pointer events are converted through `ViewportTransform`; only pointer-up commits a history command. Text commits on Enter or focus loss and cancels when empty.

The GTK view has a left tool rail, central `gtk::DrawingArea`, top undo/redo/zoom controls, and contextual bottom controls. The drawing callback renders the source texture and vector layers at the current zoom; redaction previews use the shared renderer. Keyboard actions are Ctrl+Z, Ctrl+Shift+Z, Escape, Delete, plus/minus zoom, and Ctrl+S.

Closing with unsaved document changes shows Save, Discard, and Cancel choices. Videos and GIFs never open this editor.

- [ ] **Step 4: Run controller and headless editor tests**

Run: `cargo test -p klypse-app --test editor_controller && xvfb-run -a cargo test -p klypse-app editor_view`

Expected: gesture, history, keyboard-action, and view-construction tests pass.

- [ ] **Step 5: Commit the editor UI**

```bash
git add crates/klypse-app
git commit -m "feat: add the screenshot annotation editor"
```

### Task 4: Annotation persistence and flattened export

**Files:**
- Modify: `crates/klypse-storage/src/repository.rs`
- Modify: `crates/klypse-app/src/editor/controller.rs`
- Modify: `crates/klypse-app/src/desktop/clipboard.rs`
- Modify: `crates/klypse-app/src/ui/gallery.rs`
- Test: `crates/klypse-app/tests/editor_persistence.rs`

**Interfaces:**
- Consumes: `CaptureStore::set_annotation`, `Renderer`, and atomic file persistence.
- Produces: persistent annotation JSON, `EditorController::export_flattened`, and flattened clipboard pixels.

- [ ] **Step 1: Write failing save/reopen/export tests**

```rust
#[test]
fn saved_annotations_reopen_without_touching_original() {
    let fixture = EditorPersistenceFixture::new();
    let original_hash = fixture.original_hash();
    fixture.add_rectangle_and_save();
    let reopened = fixture.reopen();
    assert_eq!(reopened.document().layers.len(), 1);
    assert_eq!(fixture.original_hash(), original_hash);
}

#[test]
fn flattened_export_creates_a_new_png() {
    let fixture = EditorPersistenceFixture::new();
    let output = fixture.export_flattened().unwrap();
    assert!(output.exists());
    assert_ne!(output, fixture.original_path());
}
```

- [ ] **Step 2: Verify persistence tests fail**

Run: `cargo test -p klypse-app --test editor_persistence`

Expected: tests fail because save and export are not connected.

- [ ] **Step 3: Connect repository, export, gallery, and clipboard**

Serialize validated annotation JSON and store it with `set_annotation` in one SQLite transaction. On open, treat malformed JSON as `StorageError::CorruptAnnotation` and offer to reset annotations without deleting the original.

Flattened export writes `<original-stem>-edited-<UTC timestamp>.png` through `AtomicCaptureFile`, creates a new screenshot gallery row referencing the same original path, generates its thumbnail, and selects it. Copy from the editor renders flattened pixels directly to a GDK texture without replacing either file.

- [ ] **Step 4: Run complete editor and workspace tests**

Run: `cargo test -p klypse-image && cargo test -p klypse-app --test editor_persistence && cargo test --workspace`

Expected: all tests pass and original-file hashes remain unchanged.

- [ ] **Step 5: Commit editor persistence**

```bash
git add crates/klypse-storage crates/klypse-app
git commit -m "feat: persist and export screenshot annotations"
```
