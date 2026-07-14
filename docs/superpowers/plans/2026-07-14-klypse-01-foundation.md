# Klypse Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce a reproducible Rust/GTK workspace with a runnable single-instance Klypse shell, typed domain interfaces, logging, localization, and runtime capability reporting.

**Architecture:** A Cargo workspace keeps domain contracts independent from GTK and system APIs. The GTK binary parses CLI actions and presents a small diagnostics-capable shell; later plans plug storage, platform, media, and editor implementations into the domain interfaces.

**Tech Stack:** Rust stable, Cargo, GTK 4, libadwaita, clap, serde, thiserror, tracing, Gettext

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

- `scripts/bootstrap-debian.sh`: installs Kali/Debian build and runtime dependencies and a user-local Rust toolchain.
- `scripts/check-workspace.sh`: verifies required workspace files and Cargo members.
- `Cargo.toml`: workspace members and shared dependency versions.
- `rust-toolchain.toml`: stable toolchain contract.
- `crates/klypse-domain/src/*`: display-independent models, commands, backend interfaces, and errors.
- `crates/klypse-platform/src/capabilities.rs`: non-invasive runtime capability probes.
- `crates/klypse-app/src/cli.rs`: command-line grammar.
- `crates/klypse-app/src/application.rs`: single-instance GTK application lifecycle.
- `crates/klypse-app/src/ui/window.rs`: initial libadwaita window and diagnostics presentation.
- `crates/klypse-app/resources/*`: GResource, GSettings, desktop metadata, and translations.

### Task 1: Reproducible Debian/Kali bootstrap and workspace

**Files:**
- Create: `scripts/bootstrap-debian.sh`
- Create: `scripts/check-workspace.sh`
- Create: `Cargo.toml`
- Create: `Cargo.lock`
- Create: `rust-toolchain.toml`
- Create: `crates/klypse-domain/Cargo.toml`
- Create: `crates/klypse-domain/src/lib.rs`
- Create: `crates/klypse-platform/Cargo.toml`
- Create: `crates/klypse-platform/src/lib.rs`
- Create: `crates/klypse-storage/Cargo.toml`
- Create: `crates/klypse-storage/src/lib.rs`
- Create: `crates/klypse-media/Cargo.toml`
- Create: `crates/klypse-media/src/lib.rs`
- Create: `crates/klypse-image/Cargo.toml`
- Create: `crates/klypse-image/src/lib.rs`
- Create: `crates/klypse-app/Cargo.toml`
- Create: `crates/klypse-app/src/main.rs`

**Interfaces:**
- Produces: a six-member Cargo workspace and a bootstrap script safe to rerun.

- [ ] **Step 1: Write the failing workspace check**

```bash
#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
required=(
  Cargo.toml
  rust-toolchain.toml
  crates/klypse-domain/Cargo.toml
  crates/klypse-platform/Cargo.toml
  crates/klypse-storage/Cargo.toml
  crates/klypse-media/Cargo.toml
  crates/klypse-image/Cargo.toml
  crates/klypse-app/Cargo.toml
)
for file in "${required[@]}"; do
  test -f "$root/$file" || { echo "missing $file" >&2; exit 1; }
done
```

- [ ] **Step 2: Run the check and verify it fails**

Run: `bash scripts/check-workspace.sh`

Expected: exit 1 with `missing Cargo.toml`.

- [ ] **Step 3: Add the bootstrap and workspace files**

`scripts/bootstrap-debian.sh` must install this exact package set after `apt-get update`:

```bash
packages=(
  build-essential curl pkg-config gettext desktop-file-utils libglib2.0-bin
  libgtk-4-dev libadwaita-1-dev libsqlite3-dev
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good
  gstreamer1.0-plugins-bad gstreamer1.0-libav
  libpipewire-0.3-dev libxcb1-dev libxcb-randr0-dev
  libxcb-composite0-dev libxcb-xfixes0-dev xvfb dbus-x11
  flatpak flatpak-builder dpkg-dev debhelper appstream lintian
)
sudo apt-get update
sudo apt-get install -y "${packages[@]}"
if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --profile minimal --default-toolchain stable
fi
source "$HOME/.cargo/env"
if ! command -v cargo-deny >/dev/null 2>&1; then
  cargo install cargo-deny --locked
fi
```

Create a root workspace with resolver 2 and these dependency floors:

```toml
[workspace]
resolver = "2"
members = [
  "crates/klypse-domain",
  "crates/klypse-platform",
  "crates/klypse-storage",
  "crates/klypse-media",
  "crates/klypse-image",
  "crates/klypse-app",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "GPL-3.0-or-later"
repository = "https://github.com/Roadmvn/Klypse"

[workspace.dependencies]
anyhow = "1.0.103"
async-channel = "2.5.0"
async-trait = "0.1"
chrono = { version = "0.4.45", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
directories = "6.0.0"
gettext-rs = { version = "0.7.7", features = ["gettext-system"] }
gstreamer = "0.25.3"
gstreamer-app = "0.25.3"
gstreamer-pbutils = "0.25.3"
gstreamer-video = "0.25.3"
gtk = { package = "gtk4", version = "0.11.4", features = ["v4_10"] }
image = "0.25.10"
libadwaita = { version = "0.9.2", features = ["v1_4"] }
rusqlite = { version = "0.40.1", features = ["chrono"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1"
tempfile = "3.27.0"
thiserror = "2.0.18"
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
uuid = { version = "1.23.5", features = ["v4", "serde"] }
```

Each initial library exports its crate name through a public constant, and `klypse-app` initially prints `Klypse workspace ready`.

- [ ] **Step 4: Bootstrap, format, and verify the workspace**

Run: `bash scripts/bootstrap-debian.sh`

Expected: all packages and the stable Rust toolchain install successfully.

Run: `source "$HOME/.cargo/env" && bash scripts/check-workspace.sh && cargo fmt --all --check && cargo check --workspace`

Expected: all commands exit 0.

- [ ] **Step 5: Commit the workspace foundation**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml scripts crates
git commit -m "build: scaffold the Klypse Rust workspace"
```

### Task 2: Domain models and backend contracts

**Files:**
- Modify: `crates/klypse-domain/Cargo.toml`
- Modify: `crates/klypse-domain/src/lib.rs`
- Create: `crates/klypse-domain/src/capture.rs`
- Create: `crates/klypse-domain/src/command.rs`
- Create: `crates/klypse-domain/src/backend.rs`
- Create: `crates/klypse-domain/src/error.rs`
- Test: `crates/klypse-domain/tests/domain_contract.rs`

**Interfaces:**
- Produces: `CaptureKind`, `CaptureTarget`, `DisplayServer`, `PixelRect`, `CaptureSelection`, `CaptureRequest`, `RecordingRequest`, `CaptureArtifact`, `HotkeyAction`, `AppCommand`, `CaptureBackend`, `RecordingBackend`, `HotkeyBackend`, and `KlypseError`.

- [ ] **Step 1: Write failing serialization and validation tests**

```rust
use std::time::Duration;
use klypse_domain::{CaptureKind, CaptureTarget, RecordingRequest};

#[test]
fn gif_request_rejects_more_than_thirty_seconds() {
    let request = RecordingRequest::new(
        CaptureKind::Gif,
        CaptureTarget::Area,
        Some(Duration::from_secs(31)),
    );
    assert!(request.is_err());
}

#[test]
fn capture_target_round_trips_as_kebab_case_json() {
    let json = serde_json::to_string(&CaptureTarget::ActiveWindow).unwrap();
    assert_eq!(json, "\"active-window\"");
}
```

- [ ] **Step 2: Run the focused tests and verify failure**

Run: `cargo test -p klypse-domain --test domain_contract`

Expected: compilation fails because the domain types do not exist.

- [ ] **Step 3: Implement the complete domain surface**

Use these exact public shapes:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureKind { Screenshot, Video, Gif }

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureTarget { Area, Screen, Window, ActiveWindow }

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayServer { X11, Wayland }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureSelection {
    Automatic,
    Region(PixelRect),
    X11Window(u32),
}

pub struct CaptureRequest {
    pub target: CaptureTarget,
    pub copy_to_clipboard: bool,
    pub selection: CaptureSelection,
}

pub struct RecordingRequest {
    pub kind: CaptureKind,
    pub target: CaptureTarget,
    pub selection: CaptureSelection,
    pub max_duration: Option<std::time::Duration>,
}

pub struct CaptureArtifact {
    pub id: uuid::Uuid,
    pub kind: CaptureKind,
    pub path: std::path::PathBuf,
    pub width: u32,
    pub height: u32,
    pub duration: Option<std::time::Duration>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub backend: DisplayServer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    CaptureArea,
    CaptureScreen,
    CaptureWindow,
    RecordVideo,
    RecordGif,
    StopRecording,
}

impl HotkeyAction {
    pub const fn id(self) -> &'static str {
        match self {
            Self::CaptureArea => "capture-area",
            Self::CaptureScreen => "capture-screen",
            Self::CaptureWindow => "capture-window",
            Self::RecordVideo => "record-video",
            Self::RecordGif => "record-gif",
            Self::StopRecording => "stop-recording",
        }
    }
}
```

`RecordingRequest::new` rejects screenshot kinds, GIF durations over 30 seconds, and zero durations. Define `AppCommand` variants `Open`, `Capture(CaptureRequest)`, `Record(RecordingRequest)`, and `StopRecording`.

Define object-safe async traits with `async_trait`:

```rust
#[async_trait::async_trait]
pub trait CaptureBackend: Send + Sync {
    async fn capture(&self, request: &CaptureRequest) -> Result<CaptureArtifact, KlypseError>;
}

#[async_trait::async_trait]
pub trait RecordingBackend: Send + Sync {
    async fn start(&self, request: &RecordingRequest) -> Result<uuid::Uuid, KlypseError>;
    async fn stop(&self, session_id: uuid::Uuid) -> Result<CaptureArtifact, KlypseError>;
}

#[async_trait::async_trait]
pub trait HotkeyBackend: Send + Sync {
    async fn bind(&self, actions: &[HotkeyAction]) -> Result<(), KlypseError>;
}
```

`KlypseError` has explicit variants for `Cancelled`, `UnavailableCapability(String)`, `PermissionDenied(String)`, `MissingDependency(String)`, `Io(std::io::Error)`, `Storage(String)`, and `Media(String)`.

- [ ] **Step 4: Run tests and workspace checks**

Run: `cargo test -p klypse-domain && cargo fmt --all --check && cargo clippy -p klypse-domain --all-targets -- -D warnings`

Expected: all tests pass and clippy emits no warnings.

- [ ] **Step 5: Commit the domain contracts**

```bash
git add crates/klypse-domain
git commit -m "feat: define capture domain contracts"
```

### Task 3: CLI and single-instance GTK shell

**Files:**
- Modify: `crates/klypse-app/Cargo.toml`
- Modify: `crates/klypse-app/src/main.rs`
- Create: `crates/klypse-app/src/cli.rs`
- Create: `crates/klypse-app/src/application.rs`
- Create: `crates/klypse-app/src/ui/mod.rs`
- Create: `crates/klypse-app/src/ui/window.rs`
- Test: `crates/klypse-app/tests/cli.rs`

**Interfaces:**
- Consumes: `klypse_domain::AppCommand` and request types.
- Produces: `cli::parse_from`, `application::run`, and a single-instance GTK application with ID `io.github.roadmvn.Klypse`.

- [ ] **Step 1: Write failing CLI tests**

```rust
use klypse_app::cli::parse_from;
use klypse_domain::{AppCommand, CaptureTarget};

#[test]
fn parses_area_capture() {
    let command = parse_from(["klypse", "capture", "area"]).unwrap();
    assert!(matches!(command, AppCommand::Capture(request) if request.target == CaptureTarget::Area));
}

#[test]
fn defaults_to_open() {
    assert!(matches!(parse_from(["klypse"]).unwrap(), AppCommand::Open));
}
```

- [ ] **Step 2: Verify the CLI tests fail**

Run: `cargo test -p klypse-app --test cli`

Expected: compilation fails because `klypse_app::cli` does not exist.

- [ ] **Step 3: Implement CLI parsing and the window**

The CLI grammar is exact:

```text
klypse
klypse open
klypse capture area|screen|window|active-window
klypse record video|gif [area|screen|window]
klypse stop
```

Use `clap` derives, map parsed commands to `AppCommand`, and expose `parse_from<I, T>(args: I) -> Result<AppCommand, clap::Error>`.

`application::run` creates `adw::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE).build()`. Its activate handler creates one `adw::ApplicationWindow` with title `Klypse`, default size 1000×700, a header bar, capture buttons, and an empty-state label `Your captures will appear here`.

The command-line handler parses arguments, activates the app, and queues non-Open commands through a GLib channel so future plans can attach the command dispatcher without changing the CLI.

- [ ] **Step 4: Run CLI and headless GTK verification**

Run: `cargo test -p klypse-app --test cli`

Expected: all CLI tests pass.

Run: `xvfb-run -a timeout 3 cargo run -p klypse-app -- open`

Expected: the process starts a GTK window and exits only because `timeout` returns 124; stderr contains no panic.

- [ ] **Step 5: Commit the application shell**

```bash
git add crates/klypse-app
git commit -m "feat: add the Klypse GTK application shell"
```

### Task 4: Logging, localization, and capability report

**Files:**
- Modify: `crates/klypse-platform/Cargo.toml`
- Modify: `crates/klypse-platform/src/lib.rs`
- Create: `crates/klypse-platform/src/capabilities.rs`
- Modify: `crates/klypse-app/src/main.rs`
- Modify: `crates/klypse-app/src/ui/window.rs`
- Create: `crates/klypse-app/src/i18n.rs`
- Create: `crates/klypse-app/resources/po/fr.po`
- Create: `crates/klypse-app/resources/po/LINGUAS`
- Test: `crates/klypse-platform/tests/capabilities.rs`

**Interfaces:**
- Produces: `CapabilityReport::detect()`, `CapabilityStatus`, `init_logging()`, and `i18n::init()`.

- [ ] **Step 1: Write failing capability tests**

```rust
use klypse_platform::{CapabilityReport, DisplayProbe};

#[test]
fn report_does_not_claim_wayland_without_a_socket() {
    let probe = DisplayProbe::from_values(None, Some(":99".into()), false, false, false);
    let report = CapabilityReport::from_probe(probe);
    assert_eq!(report.display_name(), "X11");
    assert!(!report.wayland_screenshot.available);
}
```

- [ ] **Step 2: Verify the capability test fails**

Run: `cargo test -p klypse-platform --test capabilities`

Expected: compilation fails because the capability types are missing.

- [ ] **Step 3: Implement probes, diagnostics UI, and localization setup**

`DisplayProbe` records `WAYLAND_DISPLAY`, `DISPLAY`, portal ownership on the session bus, PipeWire availability, and whether `gst-inspect-1.0` finds `vp8enc`, `webmmux`, `pipewiresrc`, and `ximagesrc`.

`CapabilityReport` exposes these fields:

```rust
pub struct CapabilityStatus {
    pub available: bool,
    pub detail: String,
}

pub struct CapabilityReport {
    pub display: klypse_domain::DisplayServer,
    pub static_capture: CapabilityStatus,
    pub video_recording: CapabilityStatus,
    pub gif_recording: CapabilityStatus,
    pub global_shortcuts: CapabilityStatus,
}
```

The initial window adds a diagnostics expander showing the report without file paths or environment values that may contain sensitive data.

Initialize logging with `tracing_subscriber::EnvFilter`, default `klypse=info`, and no ANSI when stderr is not a terminal. Initialize Gettext domain `klypse`; `fr.po` translates the initial title, empty state, capture labels, and diagnostic labels.

- [ ] **Step 4: Run all foundation checks**

Run: `cargo test --workspace && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

Expected: all tests pass and no warnings remain.

Run: `LANG=fr_FR.UTF-8 xvfb-run -a timeout 3 cargo run -p klypse-app`

Expected: the app starts without panic and translated labels are loaded when the locale is installed.

- [ ] **Step 5: Commit the completed foundation**

```bash
git add crates/klypse-app crates/klypse-platform
git commit -m "feat: add diagnostics and localization foundation"
```
