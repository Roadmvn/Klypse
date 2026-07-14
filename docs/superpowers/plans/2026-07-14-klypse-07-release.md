# Klypse Recovery, Packaging, and Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden failure recovery, complete localization and accessibility, build Debian and Flatpak packages, and verify the Linux MVP against automated and manual release criteria.

**Architecture:** A startup reconciler repairs or reports incomplete storage/media state before the gallery loads. Packaging installs the same release binary and resource tree through Debian and Flatpak, and a release script runs one reproducible quality gate.

**Tech Stack:** Rust, GStreamer Discoverer, Gettext, Debian debhelper, Flatpak GNOME runtime, GitHub Actions, shell release tooling

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

- `crates/klypse-storage/src/recovery.rs`: reconciles rows, files, orphans, thumbnails, and session markers.
- `crates/klypse-app/src/ui/recovery.rs`: presents recoverable recordings without blocking unaffected gallery items.
- `crates/klypse-app/resources/po/fr.po`: complete French translations.
- `crates/klypse-app/resources/io.github.roadmvn.Klypse.desktop`: desktop integration.
- `crates/klypse-app/resources/io.github.roadmvn.Klypse.metainfo.xml`: AppStream metadata.
- `crates/klypse-app/resources/icons/*`: scalable application icon.
- `debian/*`: Debian source package metadata and installation rules.
- `packaging/flatpak/io.github.roadmvn.Klypse.yml`: Flatpak manifest.
- `scripts/verify-release.sh`: complete local quality gate.
- `.github/workflows/ci.yml`: clean Debian CI build and tests.
- `docs/testing/linux-compatibility-matrix.md`: reproducible manual validation record.

### Task 1: Startup reconciliation and interrupted-recording recovery

**Files:**
- Create: `crates/klypse-storage/src/recovery.rs`
- Modify: `crates/klypse-storage/src/lib.rs`
- Create: `crates/klypse-app/src/ui/recovery.rs`
- Modify: `crates/klypse-app/src/application.rs`
- Modify: `crates/klypse-app/src/ui/mod.rs`
- Test: `crates/klypse-storage/tests/recovery.rs`

**Interfaces:**
- Consumes: `AppPaths`, `CaptureStore`, recording markers, PNG/GIF decoders, and GStreamer Discoverer.
- Produces: `Reconciler::scan`, `RecoveryReport`, `RecoverableFile`, `Reconciler::restore`, and `Reconciler::discard`.

- [x] **Step 1: Write failing reconciliation tests**

```rust
#[test]
fn valid_orphan_is_offered_and_missing_row_is_not_deleted() {
    let fixture = RecoveryFixture::with_valid_orphan_png();
    let report = fixture.reconciler.scan().unwrap();
    assert_eq!(report.recoverable.len(), 1);
    assert!(report.unrecoverable.is_empty());
}

#[test]
fn missing_capture_file_is_reported_without_dropping_row() {
    let fixture = RecoveryFixture::with_missing_gallery_file();
    let report = fixture.reconciler.scan().unwrap();
    assert_eq!(report.missing_files.len(), 1);
    assert!(fixture.repository.get(&fixture.id).unwrap().is_some());
}
```

- [x] **Step 2: Verify reconciliation tests fail**

Run: `cargo test -p klypse-storage --test recovery`

Expected: compilation fails because recovery types are missing.

- [x] **Step 3: Implement conservative recovery rules**

Scan only Klypse-owned temporary, orphan, and marker directories. Validate PNG/GIF by fully decoding metadata and at least one frame. Validate WebM with GStreamer Discoverer. Match markers to their exact temporary paths after canonicalizing both under the expected runtime directory.

Return four lists: recoverable files, invalid temporary files, rows with missing files, and stale thumbnails. Never delete automatically. Restore moves a valid file into captures, creates a repository row, regenerates a thumbnail, and removes its marker transactionally. Discard requires an explicit UI action and removes only a path proven to be under a Klypse-owned temporary root.

Show recovery choices after the main window appears. Missing gallery files remain visible with a warning and actions to Locate File or Remove from Gallery.

- [x] **Step 4: Run recovery and path-traversal tests**

Run: `cargo test -p klypse-storage --test recovery && cargo test -p klypse-app recovery`

Expected: valid recovery, invalid media, missing files, stale thumbnails, marker mismatch, symlink escape, and cancellation tests pass.

- [x] **Step 5: Commit recovery support**

```bash
git add crates/klypse-storage crates/klypse-app
git commit -m "feat: recover interrupted capture sessions"
```

### Task 2: Localization, accessibility, scale, and full failure tests

**Files:**
- Modify: `crates/klypse-app/resources/po/fr.po`
- Create: `crates/klypse-app/resources/po/POTFILES.in`
- Create: `scripts/check-translations.sh`
- Create: `crates/klypse-app/tests/accessibility.rs`
- Create: `crates/klypse-app/tests/gallery_scale.rs`
- Create: `crates/klypse-app/tests/failure_matrix.rs`
- Modify: affected GTK UI files to add accessible labels and translated strings.

**Interfaces:**
- Consumes: every visible UI string, gallery controller, capture/recording services, and fake failure adapters.
- Produces: complete English/French UI catalogs and automated release-level behavior checks.

- [ ] **Step 1: Add failing catalog and accessibility checks**

```bash
#!/usr/bin/env bash
set -euo pipefail
xgettext --language=Rust --keyword=gettext --files-from=crates/klypse-app/resources/po/POTFILES.in --output=/tmp/klypse.pot
msgattrib --untranslated crates/klypse-app/resources/po/fr.po --output=/tmp/klypse-untranslated.po
test "$(grep -c '^msgid ' /tmp/klypse-untranslated.po)" -eq 1
```

```rust
#[test]
fn every_interactive_control_has_an_accessible_name() {
    gtk::init().unwrap();
    let window = TestWindow::build();
    let missing = collect_interactive_widgets(&window).into_iter().filter(|w| w.accessible_name().is_none()).collect::<Vec<_>>();
    assert!(missing.is_empty(), "missing accessible names: {missing:?}");
}
```

- [ ] **Step 2: Verify quality tests fail**

Run: `xvfb-run -a cargo test -p klypse-app --test accessibility --test gallery_scale --test failure_matrix`

Expected: tests fail with missing labels, untranslated strings, or absent scale/failure fixtures.

- [ ] **Step 3: Complete UI quality and failure coverage**

Translate every user-visible string and compile `fr.mo`. Add accessible names/descriptions to capture actions, gallery tiles, editor tools, recording state, diagnostics, recovery actions, and settings rows. Ensure keyboard traversal reaches all actions and no recording/error state relies on color alone.

The scale test inserts 1,000 records, loads the first page in under 250 ms on the test runner, keeps only at most 150 GTK item widgets realized, and retrieves all pages without duplicates.

The failure matrix injects permission denial, portal cancellation, missing GStreamer element, full-disk write, database insert failure, thumbnail failure, EOS timeout, malformed annotation JSON, and interrupted GIF. Assert the gallery remains consistent and the app returns to an actionable state.

- [ ] **Step 4: Run localization and quality gates**

Run: `bash scripts/check-translations.sh && xvfb-run -a cargo test -p klypse-app --test accessibility --test gallery_scale --test failure_matrix`

Expected: no untranslated catalog entries and all UI/failure tests pass.

- [ ] **Step 5: Commit quality hardening**

```bash
git add crates/klypse-app scripts/check-translations.sh
git commit -m "test: harden localization accessibility and failures"
```

### Task 3: Desktop metadata and Debian package

**Files:**
- Create: `crates/klypse-app/resources/io.github.roadmvn.Klypse.desktop`
- Create: `crates/klypse-app/resources/io.github.roadmvn.Klypse.metainfo.xml`
- Create: `crates/klypse-app/resources/icons/hicolor/scalable/apps/io.github.roadmvn.Klypse.svg`
- Create: `debian/changelog`
- Create: `debian/control`
- Create: `debian/copyright`
- Create: `debian/rules`
- Create: `debian/source/format`
- Test: `scripts/test-debian-package.sh`

**Interfaces:**
- Produces: installable `klypse_0.1.0-1_amd64.deb`, desktop entry, AppStream metadata, icons, schemas, catalogs, and runtime dependency declarations.

- [ ] **Step 1: Write a failing package-content test**

```bash
#!/usr/bin/env bash
set -euo pipefail
deb="${1:?deb path required}"
dpkg-deb --contents "$deb" > /tmp/klypse-deb-contents
grep -q './usr/bin/klypse$' /tmp/klypse-deb-contents
grep -q './usr/share/applications/io.github.roadmvn.Klypse.desktop$' /tmp/klypse-deb-contents
grep -q './usr/share/metainfo/io.github.roadmvn.Klypse.metainfo.xml$' /tmp/klypse-deb-contents
grep -q './usr/share/glib-2.0/schemas/io.github.roadmvn.Klypse.gschema.xml$' /tmp/klypse-deb-contents
grep -q './usr/share/locale/fr/LC_MESSAGES/klypse.mo$' /tmp/klypse-deb-contents
```

- [ ] **Step 2: Verify package build is absent**

Run: `bash scripts/test-debian-package.sh ../klypse_0.1.0-1_amd64.deb`

Expected: exit nonzero because the package does not exist.

- [ ] **Step 3: Implement Debian metadata and installation rules**

`debian/control` declares `debhelper-compat (= 13)`, Rust/Cargo, GTK/libadwaita, SQLite, GStreamer development packages, gettext, and pkg-config as build dependencies. Runtime dependencies use `${shlibs:Depends}`, `${misc:Depends}`, portal, PipeWire, and GStreamer base/good/bad/libav plugins.

`debian/rules` builds with `cargo build --release --locked`, installs `target/release/klypse`, the desktop file, metainfo, scalable SVG, schema XML, and compiled French catalog under `debian/klypse/usr`. Validate metadata with `desktop-file-validate` and `appstreamcli validate --no-net`.

The SVG is an original code-native icon: a dark rounded square containing a high-contrast crop-frame motif and a small violet capture spark. It includes no text or third-party logo.

- [ ] **Step 4: Build and test the Debian package**

Run: `dpkg-buildpackage -us -uc -b`

Expected: build exits 0 and creates `../klypse_0.1.0-1_amd64.deb`.

Run: `bash scripts/test-debian-package.sh ../klypse_0.1.0-1_amd64.deb && lintian ../klypse_0.1.0-1_amd64.deb`

Expected: content test passes and lintian reports no error-severity tag.

- [ ] **Step 5: Commit Debian packaging**

```bash
git add crates/klypse-app/resources debian scripts/test-debian-package.sh
git commit -m "build: add Debian packaging"
```

### Task 4: Flatpak package

**Files:**
- Create: `packaging/flatpak/io.github.roadmvn.Klypse.yml`
- Create: `packaging/flatpak/cargo-sources.json`
- Create: `scripts/build-flatpak.sh`
- Create: `scripts/test-flatpak.sh`

**Interfaces:**
- Produces: local Flatpak repository and installed `io.github.roadmvn.Klypse` test build using the GNOME 50 runtime.

- [ ] **Step 1: Write a failing Flatpak metadata test**

```bash
#!/usr/bin/env bash
set -euo pipefail
flatpak info io.github.roadmvn.Klypse >/tmp/klypse-flatpak-info
grep -q 'ID: io.github.roadmvn.Klypse' /tmp/klypse-flatpak-info
flatpak run --command=klypse io.github.roadmvn.Klypse --help | grep -q 'capture'
```

- [ ] **Step 2: Verify the Flatpak test fails before installation**

Run: `bash scripts/test-flatpak.sh`

Expected: `flatpak info` exits nonzero because Klypse is not installed.

- [ ] **Step 3: Implement manifest and build script**

The manifest uses:

```yaml
app-id: io.github.roadmvn.Klypse
runtime: org.gnome.Platform
runtime-version: '50'
sdk: org.gnome.Sdk
command: klypse
finish-args:
  - --share=ipc
  - --socket=fallback-x11
  - --socket=wayland
  - --device=dri
  - --filesystem=xdg-pictures
```

Build with Cargo offline sources generated from the committed lockfile. Install the same binary, desktop file, metainfo, icon, schema, and translation paths as the Debian package. Do not grant network, home-directory, raw X11, or broad D-Bus permissions; portal access uses Flatpak's standard portal route.

`scripts/build-flatpak.sh` installs GNOME 50 SDK/runtime from Flathub when missing, builds into `build/flatpak`, exports `build/flatpak-repo`, and installs the local build with `--user --reinstall`.

- [ ] **Step 4: Build and smoke-test Flatpak**

Run: `bash scripts/build-flatpak.sh && bash scripts/test-flatpak.sh`

Expected: package builds, metadata is present, and the CLI help smoke test succeeds.

- [ ] **Step 5: Commit Flatpak packaging**

```bash
git add packaging/flatpak scripts/build-flatpak.sh scripts/test-flatpak.sh
git commit -m "build: add Flatpak packaging"
```

### Task 5: CI, release verification, and compatibility record

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `deny.toml`
- Create: `scripts/verify-release.sh`
- Create: `docs/testing/linux-compatibility-matrix.md`
- Modify: `README.md`

**Interfaces:**
- Produces: one local release command, clean CI, installation instructions, and an auditable manual X11/Wayland test record.

- [ ] **Step 1: Write the release verification script with strict failure**

```bash
#!/usr/bin/env bash
set -euo pipefail
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
xvfb-run -a cargo test --workspace
bash scripts/check-translations.sh
desktop-file-validate crates/klypse-app/resources/io.github.roadmvn.Klypse.desktop
appstreamcli validate --no-net crates/klypse-app/resources/io.github.roadmvn.Klypse.metainfo.xml
cargo deny check
```

- [ ] **Step 2: Run the release script and capture failures**

Run: `bash scripts/verify-release.sh`

Expected: any still-unmet formatter, lint, test, metadata, or dependency-policy requirement fails the script.

- [ ] **Step 3: Complete CI, documentation, and manual matrix**

CI runs the bootstrap package set on Debian stable, caches Cargo downloads, and invokes `scripts/verify-release.sh`. Add `cargo-deny` configuration allowing only licenses compatible with GPL-3.0-or-later and denying unknown git dependencies.

README documents `.deb` and Flatpak installation, all CLI commands, X11 versus Wayland picker behavior, global-shortcut fallback, silent-video limitation, data locations, and uninstall behavior.

The compatibility matrix has dated rows for Kali Xfce X11, GNOME Wayland, KDE Plasma Wayland, and one GNOME/KDE X11 session. Each row records region/screen/window capture, WebM/GIF start-stop, cancellation, clipboard, drag, editor, persistence, language, and recovery. Mark environments not available on the development host as `Not run: environment unavailable`; never mark them passing by inference.

- [ ] **Step 4: Run final verification and package smoke tests**

Run: `bash scripts/verify-release.sh`

Expected: exit 0.

Run: `bash scripts/test-debian-package.sh ../klypse_0.1.0-1_amd64.deb && bash scripts/test-flatpak.sh`

Expected: both package smoke tests pass.

- [ ] **Step 5: Commit the release gate**

```bash
git add .github README.md docs/testing scripts Cargo.toml deny.toml
git commit -m "ci: add the Klypse release verification gate"
```
