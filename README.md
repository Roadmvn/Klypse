# Klypse

[![Review](https://github.com/Roadmvn/Klypse/actions/workflows/review.yml/badge.svg)](https://github.com/Roadmvn/Klypse/actions/workflows/review.yml)

Klypse is a native Linux screenshot and screen-recording workspace for X11 and Wayland. It creates PNG screenshots, silent WebM/VP8 recordings, and animated GIFs, then keeps them in a persistent local gallery.

The 0.1.0 MVP includes region, screen, window, and active-window capture; video and GIF recording; clipboard copy and drag; a non-destructive annotation editor; configurable global shortcuts; recovery after interrupted writes; and English/French UI. Klypse does not upload captures or make application-level network requests.

## Install

### Debian package

Build and inspect the package in the reproducible Debian container:

```bash
./scripts/build-debian-package.sh
./scripts/dev-container.sh bash scripts/test-debian-package.sh \
  build/debian/klypse_0.1.0-1_amd64.deb
sudo apt install ./build/debian/klypse_0.1.0-1_amd64.deb
```

The Review workflow also publishes the `.deb` as a CI artifact after every successful `main` build.

### Flatpak

Install `flatpak`, `flatpak-builder`, `elfutils` (for `eu-strip`), and the SVG
loader package (`librsvg2-common` on Debian/Ubuntu), then run:

```bash
bash scripts/build-flatpak.sh
bash scripts/test-flatpak.sh
```

The script installs the GNOME 50 SDK/runtime from Flathub when needed, builds Cargo dependencies offline from the committed lockfile sources, exports `build/flatpak-repo`, and installs `io.github.roadmvn.Klypse` for the current user.

The sandbox grants Wayland, fallback X11, GPU acceleration, and `xdg-pictures`. It does not grant network, full home-directory, or raw host D-Bus access.

## Use

Launch Klypse from the application menu or with `klypse open`. The command-line interface uses the same application command bus as the buttons and shortcuts:

```bash
klypse capture area
klypse capture screen
klypse capture window
klypse capture active-window
klypse record video area
klypse record video screen
klypse record gif area
klypse stop
```

Video and GIF recording are silent in 0.1.0. GIF defaults to 12 FPS and stops after at most 30 seconds; both values are configurable within their supported range.

Default shortcuts:

| Action | Shortcut |
|---|---|
| Capture area | `Ctrl+Print` |
| Capture screen | `Print` |
| Capture window | `Alt+Print` |
| Record video | `Shift+Print` |
| Record GIF | `Ctrl+Shift+Print` |
| Stop recording | `Ctrl+Shift+Escape` |

On X11, Klypse uses its direct capture backend and selection overlay. On Wayland, it uses the desktop Screenshot, ScreenCast/PipeWire, and GlobalShortcuts portals; the compositor owns the secure picker. If the Wayland shortcut portal is unavailable, configure the desktop environment to invoke the CLI commands above.

The editor supports rectangle, ellipse, line, arrow, text, freehand, crop, pixelation, and blur tools, plus undo/redo, zoom, non-destructive save, flattened export, and clipboard copy.

## Local data and privacy

Native package defaults:

| Data | Default location |
|---|---|
| Captures | `$XDG_PICTURES_DIR/Klypse` |
| Gallery database and annotations | `$XDG_DATA_HOME/klypse/library.sqlite3` |
| Recovery orphans | `$XDG_DATA_HOME/klypse/orphans` |
| Thumbnails | `$XDG_CACHE_HOME/klypse/thumbnails` |
| In-progress files | `$XDG_RUNTIME_DIR/klypse/tmp` |

When an XDG variable is unset, the usual `~/.local/share`, `~/.cache`, and `~/Pictures` fallbacks apply. Flatpak stores database/cache state below `~/.var/app/io.github.roadmvn.Klypse/` while captures remain in the permitted Pictures directory.

Klypse never logs capture contents. Interrupted and orphaned files are reported for an explicit restore/discard decision and are not deleted automatically.

## Uninstall

```bash
sudo apt remove klypse
# or
flatpak uninstall --user io.github.roadmvn.Klypse
```

Package removal intentionally preserves captures and native user data. `flatpak uninstall --delete-data` removes Flatpak-private database/cache state, but files saved in `~/Pictures/Klypse` remain user-owned and must be removed manually if desired.

## Develop and verify

On Debian-compatible systems, `./scripts/bootstrap-debian.sh` installs prerequisites when passwordless sudo is available, or prepares the development container when Docker is available.

Run the complete release gate:

```bash
./scripts/dev-container.sh bash scripts/verify-release.sh
```

It checks formatting, translations, Desktop/AppStream metadata, Clippy with warnings denied, the full workspace test suite both normally and under Xvfb, the release build, dependency advisories, licences, and source policy. Pass a `.deb` path as the first argument to include its content smoke test. Set `KLYPSE_VERIFY_FLATPAK=1` when the Flatpak is installed in the current environment.

See [the Linux compatibility matrix](docs/testing/linux-compatibility-matrix.md) for the distinction between automated coverage and manual desktop-session validation.

Klypse is licensed under GPL-3.0-or-later.
