# Klypse Linux Capture Workspace Design

**Date:** 2026-07-14
**Status:** Validated in conversation
**Target:** Linux MVP with first-class X11 and Wayland support

## 1. Product Summary

Klypse is a Linux-first screenshot and screen-recording workspace. It keeps captures in a persistent visual gallery so users can find, edit, copy, drag, and reuse them without searching through folders.

The MVP supports X11 and Wayland from its first release. It provides static screenshots, silent screen recordings, GIF recordings, global capture shortcuts, non-destructive image annotations, English and French localization, and Linux-native distribution.

## 2. Goals

The MVP must:

- Capture a selected region, an entire screen, or a window.
- Record a screen or window as a silent WebM video.
- Record a selected source as an animated GIF.
- Work on both X11 and Wayland without requiring the user to choose a backend.
- Keep all successful captures in a persistent gallery.
- Let users crop and annotate screenshots.
- Copy images to the clipboard and drag capture files into other applications.
- Expose configurable global shortcuts, with a desktop-shortcut fallback where a Wayland compositor does not implement the global-shortcuts portal.
- Start in the detected system language and allow switching between English and French.
- Ship as a Debian package for Kali/Debian and as a Flatpak for portable installation.

## 3. Explicit Non-goals

The first MVP does not include:

- Cloud uploads, URL shortening, or third-party hosting integrations.
- OCR, scrolling capture, pin-to-screen, or automated post-capture workflows.
- Audio recording. Video recordings are silent in this release.
- Windows or macOS support.
- A plugin SDK.
- An AppImage package.
- A mandatory system-tray icon. Klypse can remain active as a single-instance background application without relying on inconsistent tray implementations.

These omissions keep the release focused while preserving architectural seams for later additions.

## 4. Technology Decisions

- **Language:** Rust, using the stable toolchain.
- **Desktop UI:** GTK 4 with libadwaita through gtk-rs.
- **Media pipelines:** GStreamer through the official Rust bindings.
- **Wayland integration:** XDG Desktop Portal over D-Bus and PipeWire streams.
- **X11 integration:** XCB-compatible Rust libraries, with XRandR/XComposite support where needed.
- **Metadata storage:** SQLite.
- **Preferences:** GSettings.
- **Localization:** Gettext catalogs for English and French.
- **Static image format:** PNG by default.
- **Video format:** WebM with VP8 by default for broad Linux availability.
- **GIF encoding:** A streaming Rust GIF encoder fed by sampled frames, defaulting to 12 frames per second and a 30-second maximum duration.

Rust keeps the system-facing code memory-safe while retaining native performance. GTK, GStreamer, PipeWire, and the portal APIs all fit the GLib event model, which reduces impedance between the UI and multimedia components.

## 5. Architecture

Klypse is a Cargo workspace divided into focused crates:

```text
klypse-app       GTK application, CLI entry points, application orchestration
klypse-domain    Capture models, commands, backend traits, domain errors
klypse-platform  Session detection, X11 backend, Wayland/portal backend, hotkeys
klypse-media     GStreamer recording pipelines, GIF frame pipeline, thumbnails
klypse-storage   SQLite repository, filesystem layout, atomic persistence
klypse-image     Annotation document, rendering, crop and export operations
```

Dependencies point inward toward `klypse-domain`. Platform, storage, image, and media crates implement domain interfaces. The GTK application depends on those implementations but domain logic never depends on GTK.

The core interfaces are:

```text
CaptureBackend
  capture(CaptureRequest) -> CaptureArtifact

RecordingBackend
  start(RecordingRequest) -> RecordingSession
  stop(RecordingSession) -> CaptureArtifact

HotkeyBackend
  bind(ActionSet) -> HotkeySession

CaptureRepository
  add(CaptureArtifact) -> CaptureRecord
  list(GalleryQuery) -> CapturePage
  delete(CaptureId, DeleteMode)
```

Backend selection happens at runtime. The rest of the application works only with these interfaces and does not branch on X11 versus Wayland.

## 6. Runtime Capability Detection

At startup Klypse inspects the actual display connection instead of trusting only environment variables:

1. Detect an active Wayland or X11 GTK display.
2. Query portal interface versions and advertised screenshot targets.
3. Check PipeWire and required GStreamer elements.
4. Select capture, recording, and hotkey implementations independently.
5. Publish a capability report to the settings screen and diagnostic logs.

A mixed environment may use different implementations for different features. For example, a Wayland session can use the screenshot and screencast portals while global shortcuts fall back to desktop-configured CLI commands.

## 7. X11 Backend

The X11 backend provides:

- Direct full-screen and per-monitor capture.
- Active-window discovery and capture.
- A custom transparent selection overlay for region capture.
- Multi-monitor geometry through XRandR.
- Video frames through a GStreamer X11 source.
- Global shortcuts through X11 key grabs.

The overlay freezes or snapshots the desktop before selection so the selected pixels match what the user saw when capture began. Coordinate conversion accounts for monitor origins and GTK scale factors.

If the compositor or X11 extensions needed for a specialized operation are missing, the backend falls back to a full-screen capture and performs a crop only when that produces correct pixels. Otherwise it returns a clear unsupported-capability error.

## 8. Wayland Backend

The Wayland backend obeys compositor security boundaries:

- Static captures use `org.freedesktop.portal.Screenshot`.
- Video capture uses `org.freedesktop.portal.ScreenCast` and consumes the returned PipeWire node through GStreamer.
- Global shortcuts use `org.freedesktop.portal.GlobalShortcuts` when available.
- The compositor-provided picker is used when security rules prevent a custom cross-screen overlay.

Klypse requests screen, window, active-window, or area targets only when the installed portal advertises them. On older portal backends, it opens the general interactive picker rather than pretending that a specific target is guaranteed.

User cancellation of any portal dialog is a normal result, not an application error. Portal permission state is never bypassed.

## 9. Commands and Data Flow

The GTK interface, global shortcuts, and CLI all dispatch the same application commands:

```text
klypse open
klypse capture area
klypse capture screen
klypse capture window
klypse record video
klypse record gif
klypse stop
```

The binary is single-instance. Secondary CLI invocations forward their command to the running application over the GLib application bus.

Static capture flow:

```text
UI, hotkey, or CLI
  -> application command
  -> selected CaptureBackend
  -> temporary PNG
  -> atomic persistence and SQLite transaction
  -> thumbnail generation
  -> gallery update
  -> optional clipboard copy and notification
```

Recording flow:

```text
UI, hotkey, or CLI
  -> selected RecordingBackend
  -> source selection
  -> PipeWire or X11 frames
  -> GStreamer pipeline or GIF encoder
  -> temporary output
  -> finalize and atomically move file
  -> gallery update
```

A failed capture never creates a gallery row. If the file succeeds but the database transaction fails, the file is moved to a recoverable orphan area and reconciled at the next startup.

## 10. Gallery and Storage

Klypse follows XDG paths:

- Database: `$XDG_DATA_HOME/klypse/library.sqlite3`.
- Thumbnails: `$XDG_CACHE_HOME/klypse/thumbnails/`.
- Temporary recordings: `$XDG_RUNTIME_DIR/klypse/` with a safe fallback under the cache directory.
- User captures: the configured capture folder, defaulting to the localized XDG Pictures directory plus `Klypse`.

Each gallery record stores:

- Stable capture ID.
- Kind: screenshot, video, or GIF.
- Original and current file paths.
- Creation timestamp in UTC.
- Width, height, duration, and file size when applicable.
- Capture target and backend.
- Thumbnail path and generation state.
- Annotation document reference for edited images.

Deleting a gallery item offers two explicit modes: remove it from the gallery only, or remove both the record and local files. The application never deletes a local file implicitly.

The gallery loads records in pages and generates thumbnails asynchronously so a large history does not block the UI.

## 11. Image Editor

Editing is non-destructive. Klypse preserves the original PNG and stores an annotation document in image coordinates. The MVP tools are:

- Crop.
- Rectangle and ellipse.
- Arrow and straight line.
- Text.
- Freehand drawing.
- Pixelation or blur for redaction.
- Undo and redo.

Saving updates the annotation document. Exporting or copying renders a flattened PNG. This separates editing intent from output pixels and allows later adjustment without degrading the original.

Annotation rendering is implemented in `klypse-image` with no dependency on GTK widgets, enabling deterministic tests.

## 12. User Interface

The primary window contains:

- A compact action bar for screenshot, video, and GIF capture.
- A paginated thumbnail gallery ordered newest first.
- A detail view with copy, drag, edit, reveal-in-folder, and delete actions.
- A dedicated image editor view.
- Settings for capture folder, post-capture behavior, language, GIF limits, and shortcuts.
- A diagnostics section showing X11/Wayland, portals, PipeWire, GStreamer elements, and fallback state.

Default post-capture behavior is: persist the file, add it to the gallery, copy static screenshots to the clipboard, and show a desktop notification. Recording results are added to the gallery without copying their full contents to the clipboard.

Drag-and-drop exports a `text/uri-list` for the underlying file. Static image copy also exposes image pixel data through GTK/GDK content providers.

## 13. Localization and Accessibility

English source strings are translated to French through Gettext. On first launch Klypse follows the system locale. A manual language setting overrides detection after restart.

All actions have accessible names, keyboard navigation, visible focus states, and text alternatives. Color alone never communicates recording state or errors.

## 14. Error Handling and Recovery

Errors are categorized into user cancellation, unavailable capability, denied permission, missing dependency, I/O failure, database failure, and media-pipeline failure.

- Cancellations return silently to the previous state.
- Missing dependencies show the exact missing capability and a distribution-appropriate package hint where known.
- Pipeline errors stop recording, preserve any recoverable temporary output, and leave the application responsive.
- Database migrations run transactionally and keep a backup before destructive schema changes.
- Incomplete temporary files are scanned at startup and offered for recovery when they contain valid media.
- Logs contain technical context but never image pixels or capture file contents.

## 15. Privacy and Security

Klypse performs no network requests in the MVP. Capture files remain local. Portal permissions are requested only when an action requires them. File permissions follow the user's umask, and temporary files are created with user-only access where possible.

The app does not run with elevated privileges. It does not inject input events or attempt to bypass Wayland security policies.

## 16. Testing Strategy

Automated tests include:

- Unit tests for domain commands, state transitions, coordinate mapping, annotation serialization, and rendering.
- Repository tests against temporary SQLite databases and filesystems.
- Media tests using GStreamer test sources to verify finalization, duration, and error propagation.
- X11 integration tests under Xvfb for screen geometry, region crop, and active-window capture.
- D-Bus contract tests with mocked screenshot, screencast, and global-shortcut portals.
- GTK view-model tests that do not require rendering a real desktop.

Release validation uses this manual matrix:

- Kali Linux with Xfce on X11.
- GNOME on Wayland.
- KDE Plasma on Wayland.
- At least one GNOME or KDE X11 session.

For every environment, the release checklist covers all capture targets, recording start/stop, cancellation, clipboard, drag-and-drop, editing, gallery persistence, localization, and restart recovery.

## 17. Packaging

The primary package is a `.deb` for current Kali and Debian releases. A Flatpak manifest provides broader distribution support and exercises the intended portal paths.

Both packages install:

- The Klypse binary and desktop entry.
- Icons and AppStream metadata.
- GSettings schemas.
- Gettext catalogs.
- Required runtime dependency declarations.

GStreamer plugin availability is checked both during packaging tests and at runtime. Klypse does not silently substitute a codec that changes the file extension or produces an unreadable gallery item.

## 18. Delivery Sequence

Implementation proceeds in reviewable vertical slices:

1. Cargo workspace, domain types, GTK shell, logging, localization, and capability report.
2. SQLite/filesystem repository and gallery with imported test captures.
3. X11 and Wayland static capture backends with automatic selection.
4. Clipboard, drag-and-drop, notifications, CLI actions, and global shortcuts.
5. Non-destructive editor and flattened export.
6. X11 and Wayland WebM recording.
7. Streaming GIF recording and duration guard.
8. Recovery, diagnostics, packaging, full automated tests, and manual compatibility validation.

Each slice must leave the workspace compiling and its relevant tests passing.

## 19. MVP Acceptance Criteria

The MVP is complete only when:

- Region, screen, and window screenshots succeed on the validated X11 and Wayland environments, using the desktop picker where Wayland requires it.
- WebM and GIF recording can start, stop, persist, and appear in the gallery on both display systems.
- Global shortcuts work through the native backend or the documented desktop CLI fallback.
- The gallery survives restarts and remains responsive with at least 1,000 records.
- All image editing tools produce a correct flattened PNG while preserving the original.
- Static images can be copied and all capture types can be dragged into a file-accepting application.
- French and English interfaces work and the initial locale is detected correctly.
- Permission denial, portal cancellation, missing codecs, full disks, and interrupted recordings fail without corrupting the gallery.
- The `.deb` installs and runs on the target Kali/Debian system.
- The Flatpak installs and runs on the validated Wayland and X11 desktops.
- Automated tests pass and the manual compatibility matrix is completed without unresolved release-blocking defects.
