use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_channel::Sender;
use gettextrs::gettext;
use gtk::{Align, Orientation, glib, prelude::*};
use klypse_domain::{
    AppCommand, CaptureKind, CaptureRequest, CaptureTarget, GIF_MAX_DURATION, RecordingRequest,
};
use klypse_platform::CapabilityReport;

use crate::recording::RecordingUiState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingSnapshot {
    pub state: RecordingUiState,
    pub kind: Option<CaptureKind>,
    pub elapsed: Duration,
    pub remaining: Option<Duration>,
}

#[derive(Clone, Debug)]
pub struct RecordingPresentation {
    state: RecordingUiState,
    kind: Option<CaptureKind>,
    started_at: Option<Instant>,
    maximum_duration: Option<Duration>,
}

impl Default for RecordingPresentation {
    fn default() -> Self {
        Self {
            state: RecordingUiState::Idle,
            kind: None,
            started_at: None,
            maximum_duration: None,
        }
    }
}

impl RecordingPresentation {
    pub fn recording_started(&mut self, request: &RecordingRequest) {
        self.recording_started_at(request, Instant::now());
    }

    pub fn recording_started_at(&mut self, request: &RecordingRequest, started_at: Instant) {
        self.kind = Some(request.kind);
        self.started_at = Some(started_at);
        self.maximum_duration = request.max_duration;
    }

    pub fn state_changed(&mut self, state: RecordingUiState) {
        self.state = state;
        if state == RecordingUiState::Idle {
            self.kind = None;
            self.started_at = None;
            self.maximum_duration = None;
        }
    }

    pub fn snapshot(&self) -> RecordingSnapshot {
        self.snapshot_at(Instant::now())
    }

    pub fn snapshot_at(&self, now: Instant) -> RecordingSnapshot {
        let elapsed = self
            .started_at
            .map(|started_at| now.saturating_duration_since(started_at))
            .unwrap_or_default();
        RecordingSnapshot {
            state: self.state,
            kind: self.kind,
            elapsed,
            remaining: self
                .maximum_duration
                .map(|maximum| maximum.saturating_sub(elapsed)),
        }
    }
}

pub fn build(
    commands: Sender<AppCommand>,
    presentation: Arc<Mutex<RecordingPresentation>>,
    capabilities: &CapabilityReport,
) -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .build();
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    let idle = idle_actions(&commands, capabilities);
    stack.add_named(&idle, Some("idle"));

    let active = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .halign(Align::Center)
        .build();
    let status = gtk::Label::new(None);
    let timer = gtk::Label::new(None);
    timer.add_css_class("monospace");
    let stop = gtk::Button::with_label(&gettext("Stop recording"));
    stop.add_css_class("destructive-action");
    stop.connect_clicked(move |_| {
        let _ = commands.try_send(AppCommand::StopRecording);
    });
    active.append(&status);
    active.append(&timer);
    active.append(&stop);
    stack.add_named(&active, Some("active"));
    root.append(&stack);

    refresh(&stack, &status, &timer, &stop, &presentation);
    let weak_root = root.downgrade();
    glib::timeout_add_local(Duration::from_millis(250), move || {
        if weak_root.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        refresh(&stack, &status, &timer, &stop, &presentation);
        glib::ControlFlow::Continue
    });
    root
}

fn idle_actions(commands: &Sender<AppCommand>, capabilities: &CapabilityReport) -> gtk::Box {
    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .halign(Align::Center)
        .build();
    for (label, target) in [
        (gettext("Capture area"), CaptureTarget::Area),
        (gettext("Capture screen"), CaptureTarget::Screen),
        (gettext("Capture window"), CaptureTarget::Window),
    ] {
        let command = commands.clone();
        let button = gtk::Button::with_label(&label);
        button.connect_clicked(move |_| {
            let _ = command.try_send(AppCommand::Capture(CaptureRequest::new(target)));
        });
        actions.append(&button);
    }
    actions.append(&record_button(
        &gettext("Record area"),
        CaptureKind::Video,
        CaptureTarget::Area,
        None,
        capabilities.video_recording.available,
        commands,
    ));
    actions.append(&record_button(
        &gettext("Record screen"),
        CaptureKind::Video,
        CaptureTarget::Screen,
        None,
        capabilities.video_recording.available,
        commands,
    ));
    actions.append(&record_button(
        &gettext("Record GIF"),
        CaptureKind::Gif,
        CaptureTarget::Area,
        Some(GIF_MAX_DURATION),
        capabilities.gif_recording.available,
        commands,
    ));
    actions
}

fn record_button(
    label: &str,
    kind: CaptureKind,
    target: CaptureTarget,
    maximum_duration: Option<Duration>,
    available: bool,
    commands: &Sender<AppCommand>,
) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.set_sensitive(available);
    let commands = commands.clone();
    button.connect_clicked(move |_| {
        if let Ok(request) = RecordingRequest::new(kind, target, maximum_duration) {
            let _ = commands.try_send(AppCommand::Record(request));
        }
    });
    button
}

fn refresh(
    stack: &gtk::Stack,
    status: &gtk::Label,
    timer: &gtk::Label,
    stop: &gtk::Button,
    presentation: &Arc<Mutex<RecordingPresentation>>,
) {
    let Ok(presentation) = presentation.lock() else {
        return;
    };
    let snapshot = presentation.snapshot();
    if snapshot.state == RecordingUiState::Idle {
        stack.set_visible_child_name("idle");
        return;
    }
    stack.set_visible_child_name("active");
    let status_text = match snapshot.state {
        RecordingUiState::Idle => String::new(),
        RecordingUiState::Selecting => gettext("Select what to record"),
        RecordingUiState::Recording => match snapshot.kind {
            Some(CaptureKind::Gif) => gettext("Recording GIF"),
            _ => gettext("Recording video"),
        },
        RecordingUiState::Finalizing => gettext("Finalizing recording"),
        RecordingUiState::Failed => gettext("Recording failed"),
    };
    status.set_label(&status_text);
    timer.set_label(&time_label(snapshot));
    stop.set_sensitive(snapshot.state == RecordingUiState::Recording);
}

fn time_label(snapshot: RecordingSnapshot) -> String {
    match snapshot.remaining {
        Some(remaining) => format!(
            "{} / -{}",
            format_duration(snapshot.elapsed),
            format_duration(remaining)
        ),
        None => format_duration(snapshot.elapsed),
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
