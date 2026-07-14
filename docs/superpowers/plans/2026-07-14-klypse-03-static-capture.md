# Klypse Static Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture regions, screens, windows, and active windows on X11 and Wayland, select the backend automatically, and persist every successful PNG into the gallery.

**Architecture:** `klypse-platform` implements the domain capture interface twice: direct X11/XCB and the Wayland screenshot portal. `CaptureService` selects a backend from the runtime report, treats portal cancellation as a normal outcome, and hands artifacts to atomic storage.

**Tech Stack:** Rust, x11rb, XRandR, XComposite, ashpd 0.13, XDG Screenshot portal, GTK 4 overlays, image

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

- `crates/klypse-platform/src/selector.rs`: chooses an available backend from the actual display and capabilities.
- `crates/klypse-platform/src/x11/capture.rs`: reads X11 pixels and window geometry.
- `crates/klypse-platform/src/portal/capture.rs`: requests compositor-approved screenshots.
- `crates/klypse-app/src/capture/service.rs`: orchestrates capture, persistence, thumbnails, and gallery refresh.
- `crates/klypse-app/src/ui/region_overlay.rs`: X11-only frozen-desktop region selector.

### Task 1: Backend selection and capture dependency wiring

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/klypse-platform/Cargo.toml`
- Modify: `crates/klypse-platform/src/lib.rs`
- Create: `crates/klypse-platform/src/selector.rs`
- Test: `crates/klypse-platform/tests/selector.rs`

**Interfaces:**
- Consumes: `CapabilityReport` and `DisplayServer`.
- Produces: `BackendChoice`, `BackendSelector::select_capture`, and platform dependency features.

- [ ] **Step 1: Write failing backend-selection tests**

```rust
#[test]
fn wayland_requires_the_screenshot_portal() {
    let report = CapabilityFixture::wayland().without_screenshot_portal();
    let error = BackendSelector::select_capture(&report).unwrap_err();
    assert!(matches!(error, KlypseError::UnavailableCapability(_)));
}

#[test]
fn x11_uses_the_direct_backend() {
    let report = CapabilityFixture::x11();
    assert_eq!(BackendSelector::select_capture(&report).unwrap(), BackendChoice::X11);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p klypse-platform --test selector`

Expected: compilation fails because selector types are undefined.

- [ ] **Step 3: Add dependencies and implement deterministic selection**

Add these workspace dependencies:

```toml
ashpd = { version = "0.13.12", default-features = false, features = ["async-io", "gtk4", "screenshot", "screencast", "global_shortcuts"] }
x11rb = { version = "0.13.2", features = ["randr", "composite", "xfixes"] }
url = "2"
```

`BackendSelector::select_capture` returns X11 only for `DisplayServer::X11` with an openable X connection. It returns Portal only for `DisplayServer::Wayland` with screenshot portal version at least 1. It never selects an X11 fallback from inside a native Wayland session.

- [ ] **Step 4: Run selector checks**

Run: `cargo test -p klypse-platform --test selector && cargo clippy -p klypse-platform --all-targets -- -D warnings`

Expected: selector tests pass and clippy is clean.

- [ ] **Step 5: Commit backend selection**

```bash
git add Cargo.toml crates/klypse-platform
git commit -m "feat: select capture backends at runtime"
```

### Task 2: Direct X11 screenshot backend

**Files:**
- Create: `crates/klypse-platform/src/x11/mod.rs`
- Create: `crates/klypse-platform/src/x11/geometry.rs`
- Create: `crates/klypse-platform/src/x11/capture.rs`
- Modify: `crates/klypse-platform/src/lib.rs`
- Test: `crates/klypse-platform/tests/x11_geometry.rs`
- Test: `crates/klypse-platform/tests/x11_capture.rs`

**Interfaces:**
- Consumes: `CaptureBackend`, `CaptureRequest`, `CaptureArtifact`, and a temporary-output directory.
- Produces: `X11CaptureBackend::connect`, `X11CaptureBackend::with_connection`, `Rect`, `normalize_selection`, and `capture_rect`.

- [ ] **Step 1: Write failing geometry and pixel-conversion tests**

```rust
#[test]
fn reversed_drag_is_normalized() {
    assert_eq!(normalize_selection((100, 80), (20, 30)), Rect { x: 20, y: 30, width: 80, height: 50 });
}

#[test]
fn bgra_pixels_become_rgba() {
    let rgba = bgra_to_rgba(&[0x10, 0x20, 0x30, 0xff]).unwrap();
    assert_eq!(rgba, [0x30, 0x20, 0x10, 0xff]);
}
```

- [ ] **Step 2: Verify tests fail**

Run: `cargo test -p klypse-platform --test x11_geometry`

Expected: compilation fails because geometry and conversion functions do not exist.

- [ ] **Step 3: Implement X11 geometry, target discovery, and PNG output**

Use `x11rb::rust_connection::RustConnection`. Full-screen capture reads the root window geometry. Per-screen geometry uses XRandR monitors. Active window comes from the root `_NET_ACTIVE_WINDOW` property; explicit window capture accepts the window selected by the X11 overlay. Translate child-window coordinates to the root before reading pixels.

Read pixels with `get_image(ImageFormat::Z_PIXMAP, drawable, x, y, width, height, u32::MAX)`. Convert the server pixmap format to RGBA using the setup pixmap formats and image byte order rather than assuming BGRA. Encode PNG through the `image` crate into a user-only temporary file under the output directory injected into `X11CaptureBackend`; the application storage service owns the later atomic commit.

Reject zero-area and out-of-root rectangles with `KlypseError::UnavailableCapability`. Map connection failures to `UnavailableCapability("X11 connection unavailable")` and I/O failures to the typed I/O variant.

- [ ] **Step 4: Run pure tests and Xvfb integration**

Run: `cargo test -p klypse-platform --test x11_geometry`

Expected: all pure tests pass.

Run: `xvfb-run -a cargo test -p klypse-platform --test x11_capture`

Expected: a known-color X11 window is captured with the expected dimensions and center pixel.

- [ ] **Step 5: Commit X11 capture**

```bash
git add crates/klypse-platform
git commit -m "feat: capture screenshots directly on X11"
```

### Task 3: Wayland screenshot portal backend

**Files:**
- Create: `crates/klypse-platform/src/portal/mod.rs`
- Create: `crates/klypse-platform/src/portal/capture.rs`
- Modify: `crates/klypse-platform/src/lib.rs`
- Test: `crates/klypse-platform/tests/portal_target.rs`
- Test: `crates/klypse-platform/tests/portal_capture_contract.rs`

**Interfaces:**
- Consumes: `CaptureBackend`, `CaptureRequest`, optional `ashpd::WindowIdentifier`, and the temporary-output directory.
- Produces: `PortalCaptureBackend::new`, `map_portal_target`, and `PortalCaptureClient` for mocking D-Bus.

- [ ] **Step 1: Write failing target-mapping tests**

```rust
#[test]
fn unsupported_active_window_uses_interactive_picker() {
    let supported = AvailableTargetSet::SCREEN | AvailableTargetSet::WINDOW;
    let selection = map_portal_target(CaptureTarget::ActiveWindow, supported);
    assert_eq!(selection, PortalSelection::InteractiveWithoutTarget);
}

#[test]
fn supported_area_requests_area_directly() {
    let selection = map_portal_target(CaptureTarget::Area, AvailableTargetSet::AREA);
    assert_eq!(selection, PortalSelection::Target(PortalTarget::Area));
}
```

- [ ] **Step 2: Verify portal tests fail**

Run: `cargo test -p klypse-platform --test portal_target`

Expected: compilation fails because portal mapping types are absent.

- [ ] **Step 3: Implement the portal client and response copying**

The production client performs this request shape:

```rust
let request = ashpd::desktop::screenshot::Screenshot::request()
    .identifier(window_identifier)
    .interactive(interactive)
    .modal(true)
    .target(target)
    .send()
    .await?;
let response = request.response()?;
let source = gio::File::for_uri(response.uri().as_str());
```

Query `ScreenshotProxy::available_targets` only when portal version 3 or newer is advertised. Map `Screen`, `Window`, `Area`, and `ActiveWindow`; use a general interactive request for an unavailable target. Copy the returned URI into a user-only temporary PNG, decode it to obtain dimensions, and construct a Wayland `CaptureArtifact`.

Map ashpd cancellation responses to `KlypseError::Cancelled`, access errors to `PermissionDenied`, and other D-Bus failures to `UnavailableCapability` without including the screenshot URI in logs.

- [ ] **Step 4: Run target and mocked D-Bus tests**

Run: `cargo test -p klypse-platform --test portal_target --test portal_capture_contract`

Expected: all tests pass, including success, cancellation, permission denial, and missing target properties.

- [ ] **Step 5: Commit Wayland portal capture**

```bash
git add crates/klypse-platform
git commit -m "feat: capture screenshots through Wayland portals"
```

### Task 4: X11 region overlay and capture orchestration

**Files:**
- Create: `crates/klypse-app/src/capture/mod.rs`
- Create: `crates/klypse-app/src/capture/service.rs`
- Create: `crates/klypse-app/src/ui/region_overlay.rs`
- Modify: `crates/klypse-app/src/application.rs`
- Modify: `crates/klypse-app/src/ui/window.rs`
- Test: `crates/klypse-app/tests/capture_service.rs`

**Interfaces:**
- Consumes: both capture backends, `CaptureStore`, `AtomicCaptureFile`, `Thumbnailer`, and `GalleryController::refresh`.
- Produces: `CaptureService::execute(AppCommand) -> CaptureOutcome` and `RegionOverlay::select(snapshot) -> Option<Rect>`.

- [ ] **Step 1: Write failing orchestration tests**

```rust
#[test]
fn successful_capture_is_persisted_before_gallery_refresh() {
    let fixture = CaptureServiceFixture::successful();
    let outcome = fixture.execute_area();
    assert!(matches!(outcome, CaptureOutcome::Saved(_)));
    assert_eq!(fixture.events(), ["backend", "file", "database", "thumbnail", "gallery"]);
}

#[test]
fn cancellation_creates_no_file_or_row() {
    let fixture = CaptureServiceFixture::cancelled();
    assert_eq!(fixture.execute_area(), CaptureOutcome::Cancelled);
    assert!(fixture.repository.is_empty());
}
```

- [ ] **Step 2: Verify service tests fail**

Run: `cargo test -p klypse-app --test capture_service`

Expected: compilation fails because the service is missing.

- [ ] **Step 3: Implement the overlay, service, and UI actions**

On X11 area capture, first capture the root pixels, then show one undecorated fullscreen GTK window per monitor with the frozen image. Pointer press starts a selection, motion updates a high-contrast rectangle and dimension label, Enter confirms, and Escape cancels. Convert the selected logical rectangle to root pixel coordinates before cropping.

On Wayland, never show this overlay; launch the portal picker.

`CaptureService` selects the backend, resolves X11 overlays into `CaptureSelection::Region` or `CaptureSelection::X11Window`, waits for an artifact, atomically commits it, inserts the row, generates the thumbnail, refreshes the gallery, copies the screenshot when requested, and issues a notification hook. Wayland requests keep `CaptureSelection::Automatic`. A cancellation exits before persistence. Capture buttons and CLI commands invoke the same service.

- [ ] **Step 4: Run service, X11 overlay, and workspace tests**

Run: `cargo test -p klypse-app --test capture_service && xvfb-run -a cargo test -p klypse-app region_overlay && cargo test --workspace`

Expected: all tests pass.

- [ ] **Step 5: Commit complete static capture**

```bash
git add crates/klypse-app
git commit -m "feat: connect static capture to the gallery"
```
