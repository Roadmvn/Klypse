use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gtk::prelude::*;
use klypse_app::{
    recording::RecordingUiState,
    ui::recording::{RecordingPresentation, build},
};
use klypse_domain::{AppCommand, CaptureKind, CaptureTarget, DisplayServer, RecordingRequest};
use klypse_platform::{CapabilityReport, CapabilityStatus};

#[test]
fn gif_presentation_exposes_elapsed_time_and_a_bounded_countdown() {
    let started_at = Instant::now();
    let request = RecordingRequest::new(
        CaptureKind::Gif,
        CaptureTarget::Area,
        Some(Duration::from_secs(30)),
    )
    .unwrap();
    let mut presentation = RecordingPresentation::default();

    presentation.recording_started_at(&request, started_at);
    presentation.state_changed(RecordingUiState::Recording);

    let snapshot = presentation.snapshot_at(started_at + Duration::from_secs(7));
    assert_eq!(snapshot.kind, Some(CaptureKind::Gif));
    assert_eq!(snapshot.elapsed, Duration::from_secs(7));
    assert_eq!(snapshot.remaining, Some(Duration::from_secs(23)));

    let expired = presentation.snapshot_at(started_at + Duration::from_secs(31));
    assert_eq!(expired.remaining, Some(Duration::ZERO));
}

#[test]
fn returning_to_idle_clears_the_previous_recording_clock() {
    let started_at = Instant::now();
    let request = RecordingRequest::new(CaptureKind::Video, CaptureTarget::Screen, None).unwrap();
    let mut presentation = RecordingPresentation::default();
    presentation.recording_started_at(&request, started_at);
    presentation.state_changed(RecordingUiState::Recording);

    presentation.state_changed(RecordingUiState::Idle);

    let snapshot = presentation.snapshot_at(started_at + Duration::from_secs(5));
    assert_eq!(snapshot.state, RecordingUiState::Idle);
    assert_eq!(snapshot.kind, None);
    assert_eq!(snapshot.elapsed, Duration::ZERO);
    assert_eq!(snapshot.remaining, None);
}

#[test]
fn recording_buttons_emit_the_shared_application_commands() {
    if gtk::init().is_err() {
        return;
    }
    let (sender, receiver) = async_channel::unbounded();
    let available = CapabilityStatus {
        available: true,
        detail: "test capability".into(),
    };
    let report = CapabilityReport {
        display: DisplayServer::X11,
        static_capture: available.clone(),
        video_recording: available.clone(),
        gif_recording: available.clone(),
        global_shortcuts: available,
    };
    let controls = build(
        sender,
        Arc::new(Mutex::new(RecordingPresentation::default())),
        &report,
    );

    find_button(&controls, "Record area").emit_clicked();
    let AppCommand::Record(request) = receiver.try_recv().unwrap() else {
        panic!("recording control did not emit AppCommand::Record");
    };
    assert_eq!(request.kind, CaptureKind::Video);
    assert_eq!(request.target, CaptureTarget::Area);

    find_button(&controls, "Record GIF").emit_clicked();
    let AppCommand::Record(request) = receiver.try_recv().unwrap() else {
        panic!("GIF control did not emit AppCommand::Record");
    };
    assert_eq!(request.kind, CaptureKind::Gif);
    assert_eq!(request.max_duration, Some(Duration::from_secs(30)));

    for (label, target) in [
        ("Capture screen", CaptureTarget::Screen),
        ("Capture window", CaptureTarget::Window),
    ] {
        find_button(&controls, label).emit_clicked();
        let context = gtk::glib::MainContext::default();
        let command = context.block_on(receiver.recv()).unwrap();
        assert!(matches!(command, AppCommand::Capture(request) if request.target == target));
    }
}

fn find_button(root: &impl IsA<gtk::Widget>, label: &str) -> gtk::Button {
    let mut pending = vec![root.as_ref().clone()];
    while let Some(widget) = pending.pop() {
        if let Ok(button) = widget.clone().downcast::<gtk::Button>()
            && (button.label().as_deref() == Some(label)
                || button.tooltip_text().as_deref() == Some(label))
        {
            return button;
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            pending.push(current);
        }
    }
    panic!("button {label:?} was not found");
}
