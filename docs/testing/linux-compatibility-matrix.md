# Linux compatibility matrix

Last updated: 2026-07-14

This record separates automated contract/backend tests from manual end-to-end desktop testing. `Pass (automated)` is not presented as a manual compositor result. `Not run` means no claim is made.

| Environment | Region / screen / window capture | WebM / GIF start-stop | Cancellation | Clipboard / drag / editor | Persistence / language / recovery | Result |
|---|---|---|---|---|---|---|
| Kali GNU/Linux Rolling, GNOME Wayland, Linux 6.18.5 | Portal selection, target fallback, permission, and PipeWire contracts pass automatically. Interactive compositor picker not run. | WebM/GIF pipelines and recording state pass automatically. Real Wayland ScreenCast session not run. | Portal cancellation and denial mappings pass automatically. | GTK content-provider, drag, editor, accessibility, and export tests pass under Xvfb; Wayland cross-app transfer not run. | SQLite paging, EN/FR catalog, settings, interrupted media, missing-file, orphan, and symlink-escape recovery tests pass automatically. | Partial: environment detected; interactive workflow not run by automation. |
| Kali Xfce X11 | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run. |
| KDE Plasma Wayland | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run. |
| GNOME or KDE X11 session | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run: environment unavailable. | Not run. |
| GitHub Actions Ubuntu 24.04 + Xvfb | Direct X11 known-window pixel capture, region geometry, target selection, and global hotkey registration pass automatically. | X11 `ximagesrc` WebM, VP8/WebM discovery, bounded GIF encoding, EOS, and thumbnail tests pass automatically. | Zero-area selection and service cancellation tests pass automatically. | Clipboard/content-provider, drag, editor, accessibility, and 1,000-item virtualized gallery tests pass automatically. | SQLite, settings, translations, failure matrix, and conservative recovery tests pass automatically. | Pass (automated X11 backend and headless UI); not a full manual desktop session. |

## Manual release checklist

For each environment marked `Not run` or `Partial`, a release tester must perform all of the following before changing the row to manual pass:

1. Capture a region, full screen, window, and active window; cancel each picker once.
2. Start and stop silent WebM and GIF recordings, including the 30-second GIF guard.
3. Exercise every default shortcut and the CLI fallback.
4. Copy and drag PNG, WebM, and GIF captures into another desktop application.
5. Use every editor tool, undo/redo, zoom, save, flattened export, and reopen.
6. Restart Klypse and confirm gallery ordering, thumbnails, settings, and EN/FR language selection.
7. Simulate an interrupted recording and confirm explicit restore/discard behavior without automatic deletion.
