# Klypse Desktop Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete native desktop workflows: persisted preferences, configurable capture shortcuts, clipboard copy, file drag-and-drop, notifications, and reliable CLI fallback commands.

**Architecture:** GSettings owns user preferences. GTK/GDK content providers expose pixels and file URIs, GIO sends local notifications, and `klypse-platform` implements X11 and portal hotkey sessions behind the domain interface.

**Tech Stack:** Rust, GSettings, GTK 4/GDK, GIO notifications, global-hotkey, ashpd GlobalShortcuts

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

- `crates/klypse-app/resources/io.github.roadmvn.Klypse.gschema.xml`: all persisted user settings and defaults.
- `crates/klypse-app/src/settings.rs`: typed GSettings adapter.
- `crates/klypse-app/src/desktop/clipboard.rs`: image and URI clipboard providers.
- `crates/klypse-app/src/desktop/drag.rs`: file drag source.
- `crates/klypse-app/src/desktop/notification.rs`: post-capture notifications.
- `crates/klypse-platform/src/x11/hotkeys.rs`: X11 registrations and event forwarding.
- `crates/klypse-platform/src/portal/hotkeys.rs`: Wayland GlobalShortcuts session.
- `crates/klypse-app/src/ui/settings.rs`: preferences, shortcuts, and fallback command help.

### Task 1: Typed settings and settings window

**Files:**
- Create: `crates/klypse-app/resources/io.github.roadmvn.Klypse.gschema.xml`
- Create: `crates/klypse-app/src/settings.rs`
- Create: `crates/klypse-app/src/ui/settings.rs`
- Modify: `crates/klypse-app/src/ui/mod.rs`
- Modify: `crates/klypse-app/src/application.rs`
- Test: `crates/klypse-app/tests/settings.rs`

**Interfaces:**
- Produces: `AppSettings::{capture_directory,copy_after_capture,notify_after_capture,language,gif_fps,gif_max_seconds,shortcut}` and corresponding setters.

- [ ] **Step 1: Write failing settings-default tests**

```rust
#[test]
fn defaults_match_the_product_contract() {
    let fixture = MemorySettingsFixture::new();
    let settings = fixture.settings();
    assert!(settings.copy_after_capture());
    assert!(settings.notify_after_capture());
    assert_eq!(settings.gif_fps(), 12);
    assert_eq!(settings.gif_max_seconds(), 30);
    assert_eq!(settings.language(), "system");
}

#[test]
fn gif_settings_are_clamped() {
    let settings = MemorySettingsFixture::new().settings();
    assert!(settings.set_gif_fps(0).is_err());
    assert!(settings.set_gif_max_seconds(31).is_err());
}
```

- [ ] **Step 2: Verify settings tests fail**

Run: `cargo test -p klypse-app --test settings`

Expected: compilation fails because `AppSettings` is missing.

- [ ] **Step 3: Implement schema, adapter, and preferences UI**

The schema contains exact keys:

```xml
<key name="capture-directory" type="s"><default>''</default></key>
<key name="copy-after-capture" type="b"><default>true</default></key>
<key name="notify-after-capture" type="b"><default>true</default></key>
<key name="language" type="s"><default>'system'</default></key>
<key name="gif-fps" type="u"><default>12</default><range min="1" max="30"/></key>
<key name="gif-max-seconds" type="u"><default>30</default><range min="1" max="30"/></key>
<key name="shortcut-area" type="s"><default>'&lt;Primary&gt;Print'</default></key>
<key name="shortcut-screen" type="s"><default>'Print'</default></key>
<key name="shortcut-window" type="s"><default>'&lt;Alt&gt;Print'</default></key>
<key name="shortcut-video" type="s"><default>'&lt;Shift&gt;Print'</default></key>
<key name="shortcut-gif" type="s"><default>'&lt;Primary&gt;&lt;Shift&gt;Print'</default></key>
```

`AppSettings` validates values before writing. The settings window uses libadwaita preference rows, a native folder chooser, switches, spin rows, a System/English/Français language combo, shortcut rows, and the existing diagnostics report.

- [ ] **Step 4: Compile the schema and run tests**

Run: `glib-compile-schemas crates/klypse-app/resources && GSETTINGS_SCHEMA_DIR=crates/klypse-app/resources cargo test -p klypse-app --test settings`

Expected: schema compilation and all tests succeed.

- [ ] **Step 5: Commit settings support**

```bash
git add crates/klypse-app
git commit -m "feat: add typed application preferences"
```

### Task 2: Clipboard, drag-and-drop, and notifications

**Files:**
- Create: `crates/klypse-app/src/desktop/mod.rs`
- Create: `crates/klypse-app/src/desktop/clipboard.rs`
- Create: `crates/klypse-app/src/desktop/drag.rs`
- Create: `crates/klypse-app/src/desktop/notification.rs`
- Modify: `crates/klypse-app/src/ui/gallery.rs`
- Modify: `crates/klypse-app/src/capture/service.rs`
- Test: `crates/klypse-app/tests/desktop_content.rs`

**Interfaces:**
- Produces: `copy_static_image`, `copy_file_uri`, `capture_content_provider`, `install_drag_source`, and `notify_capture_saved`.

- [ ] **Step 1: Write failing content-provider tests**

```rust
#[test]
fn static_image_provider_offers_pixels_and_uri() {
    gtk::init().unwrap();
    let fixture = PngFixture::new();
    let provider = capture_content_provider(&fixture.record).unwrap();
    let formats = provider.formats();
    assert!(formats.contain_mime_type("image/png"));
    assert!(formats.contain_mime_type("text/uri-list"));
}

#[test]
fn video_provider_offers_only_a_file_uri() {
    gtk::init().unwrap();
    let fixture = VideoFixture::new();
    let provider = capture_content_provider(&fixture.record).unwrap();
    assert!(provider.formats().contain_mime_type("text/uri-list"));
    assert!(!provider.formats().contain_mime_type("image/png"));
}
```

- [ ] **Step 2: Verify desktop-content tests fail**

Run: `xvfb-run -a cargo test -p klypse-app --test desktop_content`

Expected: compilation fails because content-provider functions do not exist.

- [ ] **Step 3: Implement local content and notification actions**

For screenshots, build a union provider containing a `gdk::Texture` and `gdk::FileList` for the capture path. For video and GIF gallery drag, expose a `gdk::FileList`; GIF clipboard also exposes the decoded static texture plus the file URI. Install `gtk::DragSource` on gallery tiles with Copy action and a drag icon generated from the thumbnail.

Notifications use `gio::Notification`, title `Capture saved`, body equal to the safe file name only, and a default action that opens the capture detail view. Do not put the full path or pixels in notification logs.

Connect post-capture settings: screenshots copy automatically when enabled, all successful captures notify when enabled, and no failed or cancelled action sends a notification.

- [ ] **Step 4: Run desktop integration tests**

Run: `xvfb-run -a cargo test -p klypse-app --test desktop_content && cargo test --workspace`

Expected: content-format and post-capture tests pass.

- [ ] **Step 5: Commit desktop content workflows**

```bash
git add crates/klypse-app
git commit -m "feat: add clipboard drag and notification workflows"
```

### Task 3: X11 and Wayland global shortcuts

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/klypse-platform/Cargo.toml`
- Create: `crates/klypse-platform/src/hotkey.rs`
- Create: `crates/klypse-platform/src/x11/hotkeys.rs`
- Create: `crates/klypse-platform/src/portal/hotkeys.rs`
- Modify: `crates/klypse-platform/src/x11/mod.rs`
- Modify: `crates/klypse-platform/src/portal/mod.rs`
- Test: `crates/klypse-platform/tests/hotkey_mapping.rs`
- Test: `crates/klypse-platform/tests/portal_hotkey_contract.rs`

**Interfaces:**
- Consumes: `HotkeyBackend`, `HotkeyAction`, `BackendSelector`, and shortcut strings from `AppSettings`.
- Produces: `HotkeyManager::{start,rebind,stop}`, `X11HotkeyBackend`, and `PortalHotkeyBackend`.

- [ ] **Step 1: Write failing shortcut mapping and fallback tests**

```rust
#[test]
fn default_actions_have_stable_ids() {
    assert_eq!(HotkeyAction::CaptureArea.id(), "capture-area");
    assert_eq!(HotkeyAction::RecordGif.id(), "record-gif");
}

#[test]
fn unavailable_wayland_portal_returns_cli_fallback() {
    let result = HotkeyManager::select(CapabilityFixture::wayland_without_shortcuts());
    assert!(matches!(result, HotkeyMode::DesktopCliFallback));
}
```

- [ ] **Step 2: Verify hotkey tests fail**

Run: `cargo test -p klypse-platform --test hotkey_mapping`

Expected: compilation fails because hotkey manager types are missing.

- [ ] **Step 3: Implement both hotkey sessions and fallback mode**

Add `global-hotkey = "0.8.0"` for the X11 implementation. Parse GTK accelerator strings into modifiers and keys, register all actions, consume the global event receiver on a dedicated thread, and forward stable action IDs to the GLib main context. Rebinding unregisters the old set before registering the new set; failure restores the previous set.

For Wayland, create an `ashpd::desktop::global_shortcuts::GlobalShortcuts` proxy and session, bind `NewShortcut` entries using stable IDs, localized descriptions, and preferred triggers, then listen for Activated signals. If the portal is absent or returns no shortcuts, select `DesktopCliFallback`.

Fallback mode shows these exact commands next to copy buttons:

```text
klypse capture area
klypse capture screen
klypse capture active-window
klypse record video screen
klypse record gif area
```

- [ ] **Step 4: Run hotkey tests and X11 smoke check**

Run: `cargo test -p klypse-platform --test hotkey_mapping --test portal_hotkey_contract`

Expected: mapping, portal success, portal cancellation, and fallback tests pass.

Run: `xvfb-run -a cargo test -p klypse-platform x11_hotkey`

Expected: registering and unregistering the default accelerators succeeds.

- [ ] **Step 5: Commit global shortcuts**

```bash
git add Cargo.toml crates/klypse-platform crates/klypse-app
git commit -m "feat: add global capture shortcuts"
```
