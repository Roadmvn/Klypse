# Klypse

[![Review](https://github.com/Roadmvn/Klypse/actions/workflows/review.yml/badge.svg)](https://github.com/Roadmvn/Klypse/actions/workflows/review.yml)

Klypse is a native Linux screenshot and screen-recording workspace for X11 and Wayland. It creates PNG screenshots, silent WebM/VP8 recordings, and animated GIFs, then keeps them in a persistent local gallery.

The 0.1.0 MVP includes region, screen, window, and active-window capture; video and GIF recording; clipboard copy and drag; a non-destructive annotation editor; configurable global shortcuts; recovery after interrupted writes; and English/French UI. Klypse does not upload captures or make application-level network requests.

## Download and install

Download the current packages from the [latest Klypse release](https://github.com/Roadmvn/Klypse/releases/latest). Packages are currently provided for 64-bit x86 Linux (`amd64`/`x86_64`). Verify the downloaded files with:

```bash
sha256sum -c SHA256SUMS
```

### Debian, Ubuntu, and Kali

The `.deb` targets Debian 13, Ubuntu 24.04, Kali Rolling, and newer compatible systems. After downloading it:

```bash
sudo apt install ./klypse_*_amd64.deb
klypse open
```

### Flatpak

Use the Flatpak bundle on other recent distributions. It supports Wayland and fallback X11:

```bash
flatpak remote-add --user --if-not-exists flathub \
  https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./Klypse.flatpak
flatpak run io.github.roadmvn.Klypse -- open
```

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

On X11, Klypse uses its direct capture backend and selection overlay. On Wayland, it uses the desktop Screenshot, ScreenCast/PipeWire, and GlobalShortcuts portals; the compositor owns the secure picker. For area recording, current Wayland portals may offer a monitor or window rather than a free-form rectangle. If the Wayland shortcut portal is unavailable, configure the desktop environment to invoke the CLI commands above.

The editor supports rectangle, ellipse, line, arrow, text, freehand, crop, pixelation, and blur tools, plus undo/redo, zoom, non-destructive save, flattened export, and clipboard copy.

For a Flatpak installation, replace `klypse` in the examples with `flatpak run io.github.roadmvn.Klypse --`.

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

## Development transparency

Klypse has been developed with assistance from automated coding tools, including OpenAI Codex. Tool-assisted work can include implementation, refactoring, tests, documentation, and review support. The project maintainer remains responsible for reviewing, accepting, and releasing every change. Automated checks improve confidence but do not replace independent security review or real-desktop testing.

## Develop and verify

On Debian-compatible systems, `./scripts/bootstrap-debian.sh` installs prerequisites when passwordless sudo is available, or prepares the development container when Docker is available.

Build packages locally:

```bash
./scripts/build-debian-package.sh
./scripts/dev-container.sh bash scripts/test-debian-package.sh \
  build/debian/klypse_*_amd64.deb
bash scripts/build-flatpak.sh
bash scripts/test-flatpak.sh
```

Run the complete release gate:

```bash
./scripts/dev-container.sh bash scripts/verify-release.sh
```

It checks formatting, translations, Desktop/AppStream metadata, Clippy with warnings denied, workspace coverage split into bounded runs, targeted X11/GTK smoke tests under Xvfb, dependency advisories, licences, and source policy. The parallel Debian and Flatpak jobs provide the release-build checks. Pass a `.deb` path as the first argument to include its content smoke test. Set `KLYPSE_VERIFY_FLATPAK=1` when the Flatpak is installed in the current environment.

See [the Linux compatibility matrix](docs/testing/linux-compatibility-matrix.md) for the distinction between automated coverage and manual desktop-session validation.

Klypse is licensed under GPL-3.0-or-later.
