use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_channel::Sender;
use gtk::{Align, Orientation, glib, prelude::*};
use klypse_domain::{
    AppCommand, CaptureKind, CaptureRequest, CaptureTarget, GIF_MAX_DURATION, RecordingRequest,
};
use klypse_platform::{CapabilityReport, CapabilityStatus};

use crate::{i18n::gettext, recording::RecordingUiState};

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
        if matches!(
            state,
            RecordingUiState::Idle | RecordingUiState::RecoveryRequired
        ) {
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
    let dismiss_commands = commands.clone();
    stop.connect_clicked(move |_| {
        let _ = commands.try_send(AppCommand::StopRecording);
    });
    let dismiss = gtk::Button::with_label(&gettext("Dismiss error"));
    dismiss.connect_clicked(move |_| {
        let _ = dismiss_commands.try_send(AppCommand::AcknowledgeRecordingFailure);
    });
    active.append(&status);
    active.append(&timer);
    active.append(&stop);
    active.append(&dismiss);
    stack.add_named(&active, Some("active"));
    root.append(&stack);

    refresh(&stack, &status, &timer, &stop, &dismiss, &presentation);
    let weak_root = root.downgrade();
    glib::timeout_add_local(Duration::from_millis(250), move || {
        if weak_root.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        refresh(&stack, &status, &timer, &stop, &dismiss, &presentation);
        glib::ControlFlow::Continue
    });
    root
}

fn idle_actions(commands: &Sender<AppCommand>, capabilities: &CapabilityReport) -> gtk::Box {
    let actions = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(14)
        .build();
    let capture_group = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(4)
        .column_spacing(8)
        .row_spacing(8)
        .build();
    let (shortcuts, _) = super::shortcuts::configured();
    for (label, hint, icon, target, delay, cli) in [
        (
            gettext("Capture area"),
            gettext("Drag to crop, or click a detected window."),
            "edit-cut-symbolic",
            CaptureTarget::Area,
            0,
            "klypse capture area",
        ),
        (
            gettext("Capture screen"),
            gettext("Capture the entire desktop in one click."),
            "video-display-symbolic",
            CaptureTarget::Screen,
            0,
            "klypse capture screen",
        ),
        (
            gettext("Capture window"),
            gettext("Hover to see the frame, then click."),
            "focus-windows-symbolic",
            CaptureTarget::Window,
            0,
            "klypse capture window",
        ),
        (
            gettext("Capture menu (5 seconds)"),
            gettext("Start, then open your context menu."),
            "alarm-symbolic",
            CaptureTarget::Screen,
            5,
            "klypse capture screen --delay 5",
        ),
    ] {
        let content = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .margin_top(10)
            .margin_bottom(10)
            .margin_start(8)
            .margin_end(8)
            .build();
        content.append(&gtk::Image::from_icon_name(icon));
        let title = gtk::Label::new(Some(&label));
        title.add_css_class("heading");
        title.set_wrap(true);
        content.append(&title);
        let description = gtk::Label::new(Some(&hint));
        description.set_wrap(true);
        description.set_max_width_chars(23);
        description.set_justify(gtk::Justification::Center);
        content.append(&description);
        if let Some(shortcut) = shortcuts.iter().find(|shortcut| shortcut.command == cli) {
            let keys = gtk::Label::new(Some(&shortcut.keys));
            keys.add_css_class("dim-label");
            keys.set_wrap(true);
            content.append(&keys);
        }
        let command = commands.clone();
        let button = gtk::Button::builder()
            .child(&content)
            .tooltip_text(&label)
            .build();
        super::set_accessible_label(&button, &label);
        apply_capability(&button, &capabilities.static_capture);
        if capabilities.static_capture.available {
            button.set_tooltip_text(Some(&label));
        }
        button.connect_clicked(move |button| {
            let mut request = CaptureRequest::new(target);
            request.delay = Duration::from_secs(delay);
            super::window::capture_from_button(button, &command, request);
        });
        capture_group.insert(&button, -1);
    }
    actions.append(&capture_group);

    let record_group = gtk::Box::builder()
        .halign(Align::Center)
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    record_group.append(&record_button(
        &gettext("Record area"),
        CaptureKind::Video,
        CaptureTarget::Area,
        None,
        &capabilities.video_recording,
        commands,
    ));
    record_group.append(&record_button(
        &gettext("Record screen"),
        CaptureKind::Video,
        CaptureTarget::Screen,
        None,
        &capabilities.video_recording,
        commands,
    ));
    record_group.append(&record_button(
        &gettext("Record GIF"),
        CaptureKind::Gif,
        CaptureTarget::Area,
        Some(GIF_MAX_DURATION),
        &capabilities.gif_recording,
        commands,
    ));
    actions.append(&record_group);
    actions
}

fn record_button(
    label: &str,
    kind: CaptureKind,
    target: CaptureTarget,
    maximum_duration: Option<Duration>,
    capability: &CapabilityStatus,
    commands: &Sender<AppCommand>,
) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    apply_capability(&button, capability);
    let commands = commands.clone();
    button.connect_clicked(move |_| {
        if let Ok(request) = RecordingRequest::new(kind, target, maximum_duration) {
            let _ = commands.try_send(AppCommand::Record(request));
        }
    });
    button
}

fn apply_capability(button: &gtk::Button, capability: &CapabilityStatus) {
    button.set_sensitive(capability.available);
    button.set_tooltip_text((!capability.available).then_some(capability.detail.as_str()));
}

fn refresh(
    stack: &gtk::Stack,
    status: &gtk::Label,
    timer: &gtk::Label,
    stop: &gtk::Button,
    dismiss: &gtk::Button,
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
        RecordingUiState::RecoveryRequired => gettext("Recovery needed"),
    };
    status.set_label(&status_text);
    timer.set_label(&time_label(snapshot));
    stop.set_sensitive(snapshot.state == RecordingUiState::Recording);
    stop.set_visible(!matches!(
        snapshot.state,
        RecordingUiState::Failed | RecordingUiState::RecoveryRequired
    ));
    dismiss.set_visible(snapshot.state == RecordingUiState::Failed);
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
