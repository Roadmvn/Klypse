use std::{
    cell::Cell,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_channel::{Receiver, Sender};
use gettextrs::gettext;
use gtk::{gio, glib, prelude::*};
use klypse_domain::{
    AppCommand, CaptureBackend, CaptureKind, CaptureRequest, CaptureSelection, CaptureTarget,
    HotkeyAction, KlypseError, PixelRect, RecordingRequest,
};
use klypse_media::Thumbnailer;
use klypse_platform::{
    BackendChoice, BackendSelector, CapabilityReport, HotkeyBinding, HotkeyManager,
    PortalCaptureBackend, X11CaptureBackend,
};
use klypse_storage::{AppPaths, CaptureRecord, CaptureRepository, CaptureStore, open_database};
use libadwaita as adw;

use crate::{
    APP_ID,
    capture::{CaptureEffects, CaptureOutcome, CaptureService, CaptureStage},
    cli,
    desktop::{clipboard::copy_static_image, notification::notify_capture_saved},
    gallery::GalleryEvent,
    recording::{
        DesktopRecordingBackend, RecordingController, RecordingEffects, RecordingStage,
        RecordingUiState,
    },
    settings::AppSettings,
    ui,
    ui::recording::RecordingPresentation,
    ui::region_overlay::RegionOverlay,
};

pub fn run() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let (sender, receiver) = async_channel::unbounded();
    let (gallery_event_sender, gallery_event_receiver) = async_channel::unbounded();
    let (hotkey_action_sender, hotkey_action_receiver) = async_channel::unbounded();
    let (recording_lifecycle_sender, recording_lifecycle_receiver) = async_channel::unbounded();
    let recording_presentation = Arc::new(Mutex::new(RecordingPresentation::default()));

    connect_activate(
        &application,
        sender.clone(),
        gallery_event_receiver,
        Arc::clone(&recording_presentation),
    );
    connect_command_line(&application, sender.clone());
    connect_open_capture_action(&application, gallery_event_sender.clone());
    dispatch_hotkey_actions(hotkey_action_receiver, sender.clone());
    start_hotkeys(hotkey_action_sender);
    manage_recording_lifecycle(&application, recording_lifecycle_receiver, sender.clone());
    dispatch_commands(
        receiver,
        gallery_event_sender,
        recording_lifecycle_sender,
        recording_presentation,
    );

    application.run()
}

fn dispatch_hotkey_actions(actions: Receiver<HotkeyAction>, commands: Sender<AppCommand>) {
    glib::spawn_future_local(async move {
        while let Ok(action) = actions.recv().await {
            match command_for_hotkey(action) {
                Ok(command) => {
                    let _ = commands.try_send(command);
                }
                Err(error) => tracing::warn!(%error, action = action.id(), "hotkey was ignored"),
            }
        }
    });
}

fn start_hotkeys(actions: Sender<HotkeyAction>) {
    glib::spawn_future_local(async move {
        let settings = AppSettings::new().ok();
        let (rebind_sender, rebind_receiver) = async_channel::unbounded();
        let _handlers = settings.as_ref().map(|settings| {
            settings.connect_shortcuts_changed(move || {
                let _ = rebind_sender.try_send(());
            })
        });
        let report = CapabilityReport::detect();
        let mut manager = match HotkeyManager::start(
            &report,
            hotkey_bindings(settings.as_ref()),
            actions,
        )
        .await
        {
            Ok(manager) => manager,
            Err(error) => {
                tracing::warn!(%error, "global shortcuts are unavailable; CLI fallback is active");
                return;
            }
        };
        tracing::info!(mode = ?manager.mode(), "global shortcut mode selected");
        while rebind_receiver.recv().await.is_ok() {
            if let Err(error) = manager.rebind(hotkey_bindings(settings.as_ref())).await {
                tracing::warn!(%error, "global shortcut rebinding failed; previous bindings restored");
            }
        }
        let _ = manager.stop().await;
    });
}

fn hotkey_bindings(settings: Option<&AppSettings>) -> Vec<HotkeyBinding> {
    [
        (
            HotkeyAction::CaptureArea,
            "<Primary>Print",
            gettext("Capture area"),
        ),
        (
            HotkeyAction::CaptureScreen,
            "Print",
            gettext("Capture screen"),
        ),
        (
            HotkeyAction::CaptureWindow,
            "<Alt>Print",
            gettext("Capture window"),
        ),
        (
            HotkeyAction::RecordVideo,
            "<Shift>Print",
            gettext("Record video"),
        ),
        (
            HotkeyAction::RecordGif,
            "<Primary><Shift>Print",
            gettext("Record GIF"),
        ),
        (
            HotkeyAction::StopRecording,
            "<Primary><Shift>Escape",
            gettext("Stop recording"),
        ),
    ]
    .into_iter()
    .map(|(action, fallback, description)| {
        HotkeyBinding::new(
            action,
            settings
                .and_then(|settings| settings.shortcut(action))
                .unwrap_or_else(|| fallback.to_owned()),
        )
        .with_description(description)
    })
    .collect()
}

pub fn command_for_hotkey(action: HotkeyAction) -> Result<AppCommand, KlypseError> {
    match action {
        HotkeyAction::CaptureArea => Ok(AppCommand::Capture(CaptureRequest::new(
            CaptureTarget::Area,
        ))),
        HotkeyAction::CaptureScreen => Ok(AppCommand::Capture(CaptureRequest::new(
            CaptureTarget::Screen,
        ))),
        HotkeyAction::CaptureWindow => Ok(AppCommand::Capture(CaptureRequest::new(
            CaptureTarget::ActiveWindow,
        ))),
        HotkeyAction::RecordVideo => {
            RecordingRequest::new(CaptureKind::Video, CaptureTarget::Screen, None)
                .map(AppCommand::Record)
        }
        HotkeyAction::RecordGif => RecordingRequest::new(
            CaptureKind::Gif,
            CaptureTarget::Area,
            Some(std::time::Duration::from_secs(30)),
        )
        .map(AppCommand::Record),
        HotkeyAction::StopRecording => Ok(AppCommand::StopRecording),
    }
}

fn connect_activate(
    application: &adw::Application,
    sender: Sender<AppCommand>,
    gallery_events: Receiver<GalleryEvent>,
    recording: Arc<Mutex<RecordingPresentation>>,
) {
    application.connect_activate(move |application| {
        ui::window::present(
            application,
            sender.clone(),
            gallery_events.clone(),
            Arc::clone(&recording),
        );
    });
}

fn connect_open_capture_action(
    application: &adw::Application,
    gallery_events: Sender<GalleryEvent>,
) {
    let action = gio::SimpleAction::new("open-capture", Some(glib::VariantTy::STRING));
    action.connect_activate({
        let application = application.clone();
        move |_, parameter| {
            let Some(id) = parameter
                .and_then(glib::Variant::str)
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
            else {
                return;
            };
            application.activate();
            let _ = gallery_events.try_send(GalleryEvent::Select(id));
        }
    });
    application.add_action(&action);
}

fn connect_command_line(application: &adw::Application, sender: Sender<AppCommand>) {
    application.connect_command_line(move |application, command_line| {
        match cli::parse_from(command_line.arguments()) {
            Ok(command) => {
                if command != AppCommand::Open && sender.try_send(command).is_err() {
                    eprintln!("Klypse could not queue the requested action");
                    return 1.into();
                }
                application.activate();
                0.into()
            }
            Err(error) => {
                eprint!("{error}");
                2.into()
            }
        }
    });
}

fn dispatch_commands(
    receiver: Receiver<AppCommand>,
    gallery_events: Sender<GalleryEvent>,
    recording_lifecycle: Sender<RecordingLifecycleEvent>,
    recording_presentation: Arc<Mutex<RecordingPresentation>>,
) {
    glib::spawn_future_local(async move {
        let mut runtime = None;
        while let Ok(command) = receiver.recv().await {
            tracing::info!(action = ?command, "received application command");
            if runtime.is_none() {
                match ProductionCaptureRuntime::new(
                    gallery_events.clone(),
                    recording_lifecycle.clone(),
                    Arc::clone(&recording_presentation),
                ) {
                    Ok(value) => runtime = Some(value),
                    Err(error) => {
                        tracing::error!(%error, "capture runtime is unavailable");
                        continue;
                    }
                }
            }
            let outcome = runtime.as_mut().unwrap().execute(command).await;
            match outcome {
                CaptureOutcome::Saved(record) => {
                    tracing::info!(capture_id = %record.id, "capture saved");
                }
                CaptureOutcome::Cancelled | CaptureOutcome::Ignored => {}
                CaptureOutcome::Failed(error) => {
                    tracing::error!(%error, "capture failed");
                }
            }
        }
    });
}

struct ProductionCaptureRuntime {
    service: CaptureService,
    recording: RecordingController,
    x11_backend: Option<Arc<X11CaptureBackend>>,
    copy_after_capture: bool,
    gif_maximum_duration: Duration,
}

impl ProductionCaptureRuntime {
    fn new(
        gallery_events: Sender<GalleryEvent>,
        recording_lifecycle: Sender<RecordingLifecycleEvent>,
        recording_presentation: Arc<Mutex<RecordingPresentation>>,
    ) -> Result<Self, KlypseError> {
        let preferences = AppSettings::new().ok();
        let mut paths = AppPaths::discover().map_err(storage_error)?;
        if let Some(directory) = preferences
            .as_ref()
            .and_then(AppSettings::capture_directory)
        {
            paths.captures = directory;
        }
        let repository: Arc<dyn CaptureStore> = Arc::new(CaptureRepository::new(
            open_database(&paths).map_err(storage_error)?,
        ));
        let report = CapabilityReport::detect();
        let choice = BackendSelector::select_capture(&report)?;
        let (backend, x11_backend): (Arc<dyn CaptureBackend>, Option<Arc<X11CaptureBackend>>) =
            match choice {
                BackendChoice::X11 => {
                    let backend = Arc::new(X11CaptureBackend::connect(&paths.temporary)?);
                    (backend.clone(), Some(backend))
                }
                BackendChoice::Portal => {
                    (Arc::new(PortalCaptureBackend::new(&paths.temporary)?), None)
                }
            };
        let copy_after_capture = preferences
            .as_ref()
            .is_none_or(AppSettings::copy_after_capture);
        let notify_after_capture = preferences
            .as_ref()
            .is_none_or(AppSettings::notify_after_capture);
        let gif_fps = preferences.as_ref().map_or(12, AppSettings::gif_fps);
        let gif_maximum_duration = Duration::from_secs(u64::from(
            preferences
                .as_ref()
                .map_or(30, AppSettings::gif_max_seconds),
        ));
        let effects = Arc::new(GtkCaptureEffects {
            gallery_events: gallery_events.clone(),
            notify_after_capture,
        });
        let recording_backend = Arc::new(DesktopRecordingBackend::new(
            paths.clone(),
            gif_fps,
            gif_maximum_duration,
        )?);
        let recording_effects = Arc::new(GtkRecordingEffects {
            gallery_events,
            lifecycle: recording_lifecycle,
            presentation: recording_presentation,
            notify_after_capture,
        });
        let recording = RecordingController::new(
            recording_backend.clone(),
            Arc::clone(&repository),
            paths.clone(),
            recording_backend.display(),
            recording_effects,
        );
        Ok(Self {
            service: CaptureService::new(backend, repository, paths, effects),
            recording,
            x11_backend,
            copy_after_capture,
            gif_maximum_duration,
        })
    }

    async fn execute(&mut self, command: AppCommand) -> CaptureOutcome {
        match command {
            AppCommand::Open => CaptureOutcome::Ignored,
            AppCommand::Capture(mut request) => {
                request.copy_to_clipboard &= self.copy_after_capture;
                if let Err(error) = self
                    .select_x11_area(request.target, &mut request.selection)
                    .await
                {
                    return capture_error_outcome(error);
                }
                self.service.execute(AppCommand::Capture(request)).await
            }
            AppCommand::Record(mut request) => {
                if request.kind == CaptureKind::Gif {
                    request.max_duration = Some(
                        request
                            .max_duration
                            .unwrap_or(self.gif_maximum_duration)
                            .min(self.gif_maximum_duration),
                    );
                }
                if let Err(error) = self
                    .select_x11_area(request.target, &mut request.selection)
                    .await
                {
                    return capture_error_outcome(error);
                }
                match self.recording.start(request).await {
                    Ok(_) => CaptureOutcome::Ignored,
                    Err(error) => capture_error_outcome(error),
                }
            }
            AppCommand::StopRecording => match self.recording.stop().await {
                Ok(record) => CaptureOutcome::Saved(record),
                Err(error) => capture_error_outcome(error),
            },
        }
    }

    async fn select_x11_area(
        &self,
        target: CaptureTarget,
        selection: &mut CaptureSelection,
    ) -> Result<(), KlypseError> {
        let Some(backend) = &self.x11_backend else {
            return Ok(());
        };
        if target != CaptureTarget::Area || *selection != CaptureSelection::Automatic {
            return Ok(());
        }
        let snapshot_request = CaptureRequest::new(CaptureTarget::Screen);
        let snapshot = backend.capture_sync(&snapshot_request)?;
        let selected = RegionOverlay::select(&snapshot.path).await;
        let _ = std::fs::remove_file(&snapshot.path);
        match selected {
            Ok(Some(rect)) => {
                *selection = CaptureSelection::Region(PixelRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                });
                Ok(())
            }
            Ok(None) | Err(KlypseError::Cancelled) => Err(KlypseError::Cancelled),
            Err(error) => Err(error),
        }
    }
}

fn capture_error_outcome(error: KlypseError) -> CaptureOutcome {
    if matches!(error, KlypseError::Cancelled) {
        CaptureOutcome::Cancelled
    } else {
        CaptureOutcome::Failed(error)
    }
}

struct GtkCaptureEffects {
    gallery_events: Sender<GalleryEvent>,
    notify_after_capture: bool,
}

impl CaptureEffects for GtkCaptureEffects {
    fn stage(&self, stage: CaptureStage) {
        tracing::debug!(?stage, "capture stage completed");
    }

    fn refresh_gallery(&self) -> Result<(), KlypseError> {
        self.gallery_events
            .try_send(GalleryEvent::Refresh)
            .map_err(|_| {
                KlypseError::UnavailableCapability("gallery refresh channel is closed".into())
            })
    }

    fn copy_to_clipboard(&self, path: &Path) -> Result<(), KlypseError> {
        copy_static_image(path)
    }

    fn notify_saved(&self, record: &CaptureRecord) {
        if self.notify_after_capture && notify_capture_saved(record).is_err() {
            tracing::warn!(capture_id = %record.id, "capture notification could not be sent");
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RecordingLifecycleEvent {
    StateChanged(RecordingUiState),
    Started(Option<Duration>),
}

fn manage_recording_lifecycle(
    application: &adw::Application,
    events: Receiver<RecordingLifecycleEvent>,
    commands: Sender<AppCommand>,
) {
    let application = application.clone();
    glib::spawn_future_local(async move {
        let generation = Rc::new(Cell::new(0_u64));
        let mut hold_guard = None;
        while let Ok(event) = events.recv().await {
            match event {
                RecordingLifecycleEvent::Started(Some(maximum_duration)) => {
                    let current = generation.get().wrapping_add(1);
                    generation.set(current);
                    let generation = Rc::clone(&generation);
                    let commands = commands.clone();
                    glib::spawn_future_local(async move {
                        glib::timeout_future(maximum_duration).await;
                        if generation.get() == current {
                            let _ = commands.try_send(AppCommand::StopRecording);
                        }
                    });
                }
                RecordingLifecycleEvent::Started(None) => {
                    generation.set(generation.get().wrapping_add(1));
                }
                RecordingLifecycleEvent::StateChanged(state) => {
                    let active = matches!(
                        state,
                        RecordingUiState::Selecting
                            | RecordingUiState::Recording
                            | RecordingUiState::Finalizing
                    );
                    if active && hold_guard.is_none() {
                        hold_guard = Some(application.hold());
                    } else if !active {
                        hold_guard = None;
                    }
                    if matches!(state, RecordingUiState::Idle | RecordingUiState::Failed) {
                        generation.set(generation.get().wrapping_add(1));
                    }
                }
            }
        }
        drop(hold_guard);
    });
}

struct GtkRecordingEffects {
    gallery_events: Sender<GalleryEvent>,
    lifecycle: Sender<RecordingLifecycleEvent>,
    presentation: Arc<Mutex<RecordingPresentation>>,
    notify_after_capture: bool,
}

impl RecordingEffects for GtkRecordingEffects {
    fn stage(&self, stage: RecordingStage) {
        tracing::debug!(?stage, "recording stage completed");
    }

    fn gallery_saved(&self, record: &CaptureRecord) {
        let _ = self
            .gallery_events
            .try_send(GalleryEvent::Select(record.id));
        if self.notify_after_capture && notify_capture_saved(record).is_err() {
            tracing::warn!(capture_id = %record.id, "recording notification could not be sent");
        }
    }

    fn state_changed(&self, state: RecordingUiState) {
        if let Ok(mut presentation) = self.presentation.lock() {
            presentation.state_changed(state);
        }
        let _ = self
            .lifecycle
            .try_send(RecordingLifecycleEvent::StateChanged(state));
    }

    fn recording_started(&self, request: &RecordingRequest) {
        if let Ok(mut presentation) = self.presentation.lock() {
            presentation.recording_started(request);
        }
        let _ = self
            .lifecycle
            .try_send(RecordingLifecycleEvent::Started(request.max_duration));
    }

    fn generate_thumbnail(
        &self,
        record: &CaptureRecord,
        destination: &Path,
    ) -> Result<bool, KlypseError> {
        Thumbnailer::new(256)
            .generate(&record.path, destination)
            .map(|_| true)
            .map_err(|error| KlypseError::Media(error.to_string()))
    }
}

fn storage_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Storage(error.to_string())
}
