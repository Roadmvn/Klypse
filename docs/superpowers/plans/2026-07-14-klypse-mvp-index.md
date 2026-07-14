# Klypse MVP Implementation Plan Index

The validated design is implemented in this strict order:

1. [`2026-07-14-klypse-01-foundation.md`](2026-07-14-klypse-01-foundation.md) — bootstrap, workspace, domain contracts, GTK shell, diagnostics, and localization foundation.
2. [`2026-07-14-klypse-02-gallery-storage.md`](2026-07-14-klypse-02-gallery-storage.md) — XDG paths, atomic persistence, SQLite, thumbnails, and paginated gallery.
3. [`2026-07-14-klypse-03-static-capture.md`](2026-07-14-klypse-03-static-capture.md) — automatic backend choice and X11/Wayland static capture.
4. [`2026-07-14-klypse-04-desktop-integration.md`](2026-07-14-klypse-04-desktop-integration.md) — preferences, clipboard, drag-and-drop, notifications, and global shortcuts.
5. [`2026-07-14-klypse-05-image-editor.md`](2026-07-14-klypse-05-image-editor.md) — non-destructive annotation, rendering, editor UI, and flattened export.
6. [`2026-07-14-klypse-06-recording.md`](2026-07-14-klypse-06-recording.md) — X11/Wayland WebM and GIF pipelines, finalization, and gallery integration.
7. [`2026-07-14-klypse-07-release.md`](2026-07-14-klypse-07-release.md) — recovery, accessibility, failure hardening, Debian/Flatpak packaging, CI, and release validation.

## Design Coverage

| Design requirement | Implementation plan |
|---|---|
| Product goals and Linux scope | 01, then enforced globally in 02–07 |
| Rust/GTK/GStreamer technology | 01 and 06 |
| Modular architecture and runtime detection | 01 |
| X11 capture | 03 |
| Wayland portal capture | 03 |
| Commands and single-instance flow | 01 and 04 |
| Gallery and XDG storage | 02 |
| Non-destructive editor | 05 |
| GTK UI and desktop integrations | 02, 03, 04, 05, and 06 |
| Localization and accessibility | 01, 04, and 07 |
| Error recovery and privacy | 03, 06, and 07 |
| Automated and compatibility testing | every plan, completed in 07 |
| Debian and Flatpak distribution | 07 |
| Acceptance criteria and release gate | 07 |

## Execution Choice

The user requested autonomous completion and delegated intermediate validation. Execution therefore uses the inline `superpowers:executing-plans` workflow, with each task's tests and commits serving as the review checkpoints. Work pauses only for an external blocker that cannot be resolved safely from the local environment.
