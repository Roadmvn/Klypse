# Klypse X11 and Wayland Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record silent WebM videos and animated GIFs from X11 or Wayland sources, finalize them safely, and add successful recordings to the gallery.

**Architecture:** `klypse-platform` negotiates a display-specific frame source; `klypse-media` owns GStreamer and GIF pipelines. A single app recording controller enforces state transitions, duration guards, stop/finalization, persistence, recovery metadata, and UI feedback.

**Tech Stack:** Rust, GStreamer/gstreamer-rs 0.25, VP8/WebM, ashpd Screencast, PipeWire, ximagesrc, gif 0.14

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

- `crates/klypse-media/src/recording/state.rs`: validated recording state machine.
- `crates/klypse-media/src/recording/pipeline.rs`: common GStreamer construction and EOS finalization.
- `crates/klypse-media/src/recording/video.rs`: VP8/WebM encoding.
- `crates/klypse-media/src/recording/gif.rs`: streaming quantization and GIF encoding.
- `crates/klypse-platform/src/x11/recording.rs`: X11 source geometry and ximagesrc description.
- `crates/klypse-platform/src/portal/recording.rs`: screencast portal, PipeWire descriptor, and node selection.
- `crates/klypse-app/src/recording/controller.rs`: app orchestration, auto-stop, gallery, and error recovery.
- `crates/klypse-app/src/ui/recording.rs`: visible recording state and controls.

### Task 1: Recording state machine and test-source WebM pipeline

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/klypse-media/Cargo.toml`
- Modify: `crates/klypse-media/src/lib.rs`
- Create: `crates/klypse-media/src/recording/mod.rs`
- Create: `crates/klypse-media/src/recording/state.rs`
- Create: `crates/klypse-media/src/recording/pipeline.rs`
- Create: `crates/klypse-media/src/recording/video.rs`
- Test: `crates/klypse-media/tests/recording_state.rs`
- Test: `crates/klypse-media/tests/webm_pipeline.rs`

**Interfaces:**
- Produces: `RecordingState`, `RecordingMachine`, `VideoPipeline::{start,stop}`, `VideoPipelineConfig`, and `PipelineSource`.

- [ ] **Step 1: Write failing state and media-finalization tests**

```rust
#[test]
fn only_recording_can_transition_to_finalizing() {
    let mut machine = RecordingMachine::idle();
    assert!(machine.begin_finalization().is_err());
    machine.start(Uuid::new_v4()).unwrap();
    machine.begin_finalization().unwrap();
    assert_eq!(machine.state(), RecordingState::Finalizing);
}

#[test]
fn test_source_produces_discoverable_webm() {
    let fixture = VideoPipelineFixture::test_source(Duration::from_millis(500));
    let artifact = fixture.run_to_completion().unwrap();
    assert!(artifact.path.exists());
    assert!(artifact.duration.unwrap() >= Duration::from_millis(400));
    assert_eq!((artifact.width, artifact.height), (320, 180));
}
```

- [ ] **Step 2: Verify media tests fail**

Run: `cargo test -p klypse-media --test recording_state --test webm_pipeline`

Expected: compilation fails because recording modules are missing.

- [ ] **Step 3: Implement state and a reusable VP8 pipeline**

Add exact dependencies:

```toml
gstreamer = "0.25.3"
gstreamer-app = "0.25.3"
gstreamer-pbutils = "0.25.3"
gstreamer-video = "0.25.3"
gif = "0.14.2"
```

State transitions are exact:

```text
Idle -> Selecting -> Recording -> Finalizing -> Idle
Selecting -> Idle on cancellation
Selecting|Recording|Finalizing -> Failed on non-cancellation error
Failed -> Idle after the error is acknowledged
```

Build video pipelines from elements, not an interpolated parse string. The common suffix is `videoconvert`, `videorate`, `video/x-raw,framerate=30/1`, `queue`, `vp8enc deadline=1 cpu-used=8`, `webmmux`, and `filesink`. The test prefix is `videotestsrc is-live=true pattern=ball` plus `video/x-raw,width=320,height=180`.

`stop` sends EOS, waits up to 10 seconds for EOS or Error on the bus, sets the pipeline to Null, validates the result with `Discoverer`, and returns dimensions/duration. Timeout or bus error preserves the temporary path and returns `MediaError::Finalization`.

- [ ] **Step 4: Run GStreamer tests**

Run: `cargo test -p klypse-media --test recording_state --test webm_pipeline`

Expected: state tests and the discoverable WebM test pass.

- [ ] **Step 5: Commit the recording core**

```bash
git add Cargo.toml crates/klypse-media
git commit -m "feat: add the WebM recording pipeline"
```

### Task 2: X11 video recording source

**Files:**
- Create: `crates/klypse-platform/src/x11/recording.rs`
- Modify: `crates/klypse-platform/src/x11/mod.rs`
- Modify: `crates/klypse-platform/Cargo.toml`
- Test: `crates/klypse-platform/tests/x11_recording_source.rs`

**Interfaces:**
- Consumes: X11 target geometry and `PipelineSource`.
- Produces: `X11RecordingSource::for_target`, `X11RecordingSource::build_element`, and cursor inclusion.

- [ ] **Step 1: Write failing source-property tests**

```rust
#[test]
fn area_source_uses_inclusive_ximagesrc_bounds() {
    gstreamer::init().unwrap();
    let source = X11RecordingSource::for_rect(Rect { x: 10, y: 20, width: 100, height: 50 }, true).unwrap();
    let element = source.build_element().unwrap();
    assert_eq!(element.property::<u32>("startx"), 10);
    assert_eq!(element.property::<u32>("starty"), 20);
    assert_eq!(element.property::<u32>("endx"), 109);
    assert_eq!(element.property::<u32>("endy"), 69);
    assert!(element.property::<bool>("show-pointer"));
}
```

- [ ] **Step 2: Verify X11 recording test fails**

Run: `xvfb-run -a cargo test -p klypse-platform --test x11_recording_source`

Expected: compilation fails because `X11RecordingSource` is missing.

- [ ] **Step 3: Implement screen, window, and region sources**

Create `ximagesrc` through `gst::ElementFactory::make("ximagesrc")`. Set `use-damage=false`, `show-pointer=true`, and inclusive bounds for region/screen geometry. For a selected window, set `xid` when the installed plugin exposes the property; otherwise translate its geometry to root coordinates and capture those bounds.

Reuse the exact XRandR and active-window geometry code from static capture. Reject rectangles outside the current root after display changes, and return a localized unavailable-capability error when `ximagesrc` is absent.

- [ ] **Step 4: Run X11 source and short-recording tests**

Run: `xvfb-run -a cargo test -p klypse-platform --test x11_recording_source && xvfb-run -a cargo test -p klypse-media x11_test_recording`

Expected: properties are correct and a one-second Xvfb recording finalizes as WebM.

- [ ] **Step 5: Commit X11 recording**

```bash
git add crates/klypse-platform crates/klypse-media
git commit -m "feat: record X11 sources as WebM"
```

### Task 3: Wayland screencast and PipeWire source

**Files:**
- Create: `crates/klypse-platform/src/portal/recording.rs`
- Modify: `crates/klypse-platform/src/portal/mod.rs`
- Test: `crates/klypse-platform/tests/portal_recording_contract.rs`
- Test: `crates/klypse-platform/tests/pipewire_source.rs`

**Interfaces:**
- Consumes: `RecordingRequest`, optional GTK window identifier, ashpd Screencast, and `PipelineSource`.
- Produces: `PortalRecordingSource::select`, `PortalRecordingSource::build_element`, retained portal session, PipeWire FD, and selected node ID.

- [ ] **Step 1: Write failing source-selection tests**

```rust
#[test]
fn screen_request_selects_only_monitors() {
    let options = screencast_options(CaptureTarget::Screen);
    assert_eq!(options.sources, SourceType::Monitor);
    assert!(!options.multiple);
}

#[test]
fn portal_cancellation_maps_to_domain_cancellation() {
    let client = FakeScreencastClient::cancel_on_start();
    let result = block_on(PortalRecordingSource::select_with(client, CaptureTarget::Window));
    assert!(matches!(result, Err(KlypseError::Cancelled)));
}
```

- [ ] **Step 2: Verify portal recording tests fail**

Run: `cargo test -p klypse-platform --test portal_recording_contract`

Expected: compilation fails because portal recording types are absent.

- [ ] **Step 3: Implement Screencast session and PipeWire element**

Use this sequence and keep the session alive until finalization:

```rust
let proxy = Screencast::new().await?;
let session = proxy.create_session(Default::default()).await?;
proxy.select_sources(
    &session,
    SelectSourcesOptions::default()
        .set_cursor_mode(CursorMode::Embedded)
        .set_sources(source_type)
        .set_multiple(false)
        .set_persist_mode(PersistMode::Application),
).await?;
let streams = proxy.start(&session, window_identifier.as_ref(), Default::default()).await?.response()?;
let remote_fd = proxy.open_pipe_wire_remote(&session, Default::default()).await?;
```

Select exactly one stream. Build `pipewiresrc` with the retained duplicated file descriptor, selected node ID as its path, and `do-timestamp=true`. Keep both the ashpd session and FD in `PortalRecordingSource`; close the session only after the pipeline reaches Null.

Area recording on Wayland uses the compositor selection result and does not draw a Klypse overlay. Map cancellation and permission errors the same way as static portal capture.

- [ ] **Step 4: Run mocked portal and element tests**

Run: `cargo test -p klypse-platform --test portal_recording_contract --test pipewire_source`

Expected: sequence, source-type, cancellation, FD lifetime, and node-property tests pass; the element test skips only if `pipewiresrc` is not installed.

- [ ] **Step 5: Commit Wayland recording**

```bash
git add crates/klypse-platform
git commit -m "feat: record Wayland streams through PipeWire"
```

### Task 4: Streaming GIF encoder and duration guard

**Files:**
- Create: `crates/klypse-media/src/recording/gif.rs`
- Modify: `crates/klypse-media/src/recording/mod.rs`
- Test: `crates/klypse-media/tests/gif_pipeline.rs`

**Interfaces:**
- Consumes: a display-specific GStreamer source, configured FPS, maximum seconds, and temporary output.
- Produces: `GifPipeline::{start,stop}`, `GifPipelineConfig`, streaming frame conversion, and automatic stop signal.

- [ ] **Step 1: Write failing GIF metadata and guard tests**

```rust
#[test]
fn ten_frame_test_source_writes_ten_frame_gif() {
    let fixture = GifPipelineFixture::test_source(10, 10);
    let artifact = fixture.run_to_completion().unwrap();
    let decoded = fixture.decode(&artifact.path);
    assert_eq!(decoded.frame_count, 10);
    assert_eq!(decoded.delay_hundredths, vec![10; 10]);
}

#[test]
fn config_rejects_more_than_thirty_seconds() {
    assert!(GifPipelineConfig::new(12, Duration::from_secs(31)).is_err());
}
```

- [ ] **Step 2: Verify GIF tests fail**

Run: `cargo test -p klypse-media --test gif_pipeline`

Expected: compilation fails because GIF pipeline types are missing.

- [ ] **Step 3: Implement bounded streaming GIF output**

The common GStreamer suffix is `videoconvert ! videoscale ! videorate ! video/x-raw,format=RGBA,framerate=<fps>/1 ! appsink`. Limit the longest output edge to 1280 while preserving aspect ratio. Pull one sample at a time, quantize through `gif::Frame::from_rgba_speed` with speed 10, write immediately to `gif::Encoder`, and release the sample before pulling the next one.

Set repeat to Infinite and frame delay to `round(100 / fps)` hundredths. A monotonic GLib timer sends an automatic stop signal at the configured limit, never later than 30 seconds. On stop, flush the encoder, validate the GIF by decoding its first frame, and return its dimensions and duration.

- [ ] **Step 4: Run GIF pipeline tests**

Run: `cargo test -p klypse-media --test gif_pipeline`

Expected: frame count, delay, scaling, guard, and truncated-output rejection tests pass.

- [ ] **Step 5: Commit GIF recording**

```bash
git add crates/klypse-media
git commit -m "feat: add bounded streaming GIF recording"
```

### Task 5: Recording controller, UI, persistence, and recovery markers

**Files:**
- Create: `crates/klypse-app/src/recording/mod.rs`
- Create: `crates/klypse-app/src/recording/controller.rs`
- Create: `crates/klypse-app/src/ui/recording.rs`
- Modify: `crates/klypse-app/src/application.rs`
- Modify: `crates/klypse-app/src/ui/window.rs`
- Modify: `crates/klypse-app/src/gallery/controller.rs`
- Test: `crates/klypse-app/tests/recording_controller.rs`

**Interfaces:**
- Consumes: `RecordingBackend`, media pipelines, settings, repository, thumbnailer, notifications, and `AppCommand::StopRecording`.
- Produces: `RecordingController::{start,stop,state}`, one active session limit, `.klypse-session.json` recovery markers, and recording UI state.

- [x] **Step 1: Write failing controller state tests**

```rust
#[test]
fn second_recording_is_rejected_while_one_is_active() {
    let fixture = RecordingControllerFixture::started_video();
    let error = fixture.controller.start(fixture.gif_request()).unwrap_err();
    assert!(matches!(error, KlypseError::UnavailableCapability(_)));
}

#[test]
fn successful_stop_persists_before_returning_to_idle() {
    let fixture = RecordingControllerFixture::started_video();
    fixture.controller.stop().unwrap();
    assert_eq!(fixture.events(), ["finalize", "move", "database", "thumbnail", "gallery", "idle"]);
}
```

- [x] **Step 2: Verify controller tests fail**

Run: `cargo test -p klypse-app --test recording_controller`

Expected: compilation fails because recording controller types are missing.

- [x] **Step 3: Implement orchestration and visible recording state**

Allow one recording session. Before pipeline start, atomically write a JSON marker containing session ID, kind, backend, temporary path, requested target, and start time. On successful finalization and repository insert, remove the marker. On failure, retain it for startup recovery.

The UI replaces capture actions with a prominent Stop button while recording, displays elapsed time, and shows a GIF countdown. Closing the window leaves recording active; quitting the application requests stop and waits for finalization. The global stop shortcut and `klypse stop` invoke the same method.

After finalization, atomically move the file, insert metadata, generate the thumbnail/poster, refresh and select the gallery record, and notify. Recordings are not automatically placed on the clipboard.

- [x] **Step 4: Run recording controller and workspace tests**

Run: `cargo test -p klypse-app --test recording_controller && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`

Expected: all state, persistence-order, cancellation, auto-stop, and error tests pass.

- [x] **Step 5: Commit complete recording workflows**

```bash
git add crates/klypse-app
git commit -m "feat: connect video and GIF recording workflows"
```
