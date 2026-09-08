use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_channel::{Receiver, Sender};
use gtk::{gio, glib, prelude::*};
use klypse_domain::{
    AppCommand, CaptureBackend, CaptureKind, CaptureRequest, CaptureSelection, CaptureTarget,
    HotkeyAction, KlypseError, PixelRect, RecordingRequest,
};
use klypse_media::Thumbnailer;
use klypse_platform::{
    BackendChoice, BackendSelector, CapabilityReport, HotkeyBinding, HotkeyManager, HotkeyMode,
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
    i18n::gettext,
    recording::{
        DesktopRecordingBackend, RecordingController, RecordingEffects, RecordingStage,
        RecordingUiState,
    },
    settings::AppSettings,
    ui,
    ui::recording::RecordingPresentation,
    ui::region_overlay::RegionOverlay,
};

/// How long a single command may run before the queue gives up on it.
///
/// Longer than the region selector's own deadline so that a stuck selection is
/// reported by the selector itself, with its own message, rather than by this
/// catch-all.
const COMMAND_DEADLINE: Duration = Duration::from_secs(330);

pub fn run() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let (sender, receiver) = async_channel::unbounded();
    let (gallery_event_sender, gallery_event_receiver) = async_channel::unbounded();
    let (hotkey_action_sender, hotkey_action_receiver) = async_channel::unbounded();
    let (recording_lifecycle_sender, recording_lifecycle_receiver) = async_channel::unbounded();
    let (recovery_scan_sender, recovery_scan_receiver) = async_channel::unbounded();
    let recording_presentation = Arc::new(Mutex::new(RecordingPresentation::default()));
    let ui_notifier = ui::window::UiNotifier::default();

    connect_activate(
        &application,
        sender.clone(),
        gallery_event_receiver,
        gallery_event_sender.clone(),
        Arc::clone(&recording_presentation),
        recovery_scan_receiver,
        ui_notifier.clone(),
    );
    connect_command_line(&application, sender.clone());
    connect_open_capture_action(&application, gallery_event_sender.clone());
    dispatch_hotkey_actions(hotkey_action_receiver, sender.clone());
    start_hotkeys(hotkey_action_sender);
    manage_recording_lifecycle(&application, recording_lifecycle_receiver);
    dispatch_commands(
        receiver,
        gallery_event_sender,
        recording_lifecycle_sender,
        recording_presentation,
        recovery_scan_sender,
        ui_notifier,
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
        if manager.mode() == HotkeyMode::DesktopCliFallback {
            tracing::info!(
                "the desktop already owns these keys; bind them to the klypse commands instead"
            );
        }
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
    gallery_event_sender: Sender<GalleryEvent>,
    recording: Arc<Mutex<RecordingPresentation>>,
    recovery_scans: Receiver<()>,
    notifier: ui::window::UiNotifier,
) {
    application.connect_activate(move |application| {
        ui::window::present(
            application,
            sender.clone(),
            gallery_events.clone(),
            gallery_event_sender.clone(),
            Arc::clone(&recording),
            recovery_scans.clone(),
            notifier.clone(),
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
                // Raising the gallery before taking pixels dismisses context
                // menus and changes the active window we intended to capture.
                let show_gallery = command == AppCommand::Open || application.windows().is_empty();
                if command != AppCommand::Open && sender.try_send(command).is_err() {
                    eprintln!("Klypse could not queue the requested action");
                    return 1.into();
                }
                if show_gallery {
                    application.activate();
                }
                0.into()
            }
            Err(error) => {
                let exit_code = u8::try_from(error.exit_code()).unwrap_or(2);
                let _ = error.print();
                exit_code.into()
            }
        }
    });
}

fn dispatch_commands(
    receiver: Receiver<AppCommand>,
    gallery_events: Sender<GalleryEvent>,
    recording_lifecycle: Sender<RecordingLifecycleEvent>,
    recording_presentation: Arc<Mutex<RecordingPresentation>>,
    recovery_scans: Sender<()>,
    notifier: ui::window::UiNotifier,
) {
    glib::spawn_future_local(async move {
        let mut runtime: Option<ProductionCaptureRuntime> = None;
        loop {
            // Check between commands too: a busy queue must not starve stream
            // failures or the active session's duration limit.
            if let Some(runtime) = &mut runtime
                && runtime.recording.terminal_event().is_some()
            {
                show_outcome(
                    &notifier,
                    recording_outcome(runtime.recording.poll_terminal_event().await),
                );
            }
            let command = match glib::future_with_timeout(
                Duration::from_millis(200),
                receiver.recv(),
            )
            .await
            {
                Ok(Ok(command)) => command,
                Ok(Err(_)) => break,
                Err(_) => {
                    if let Some(runtime) = &mut runtime
                        && runtime.recording.terminal_event().is_some()
                    {
                        show_outcome(
                            &notifier,
                            recording_outcome(runtime.recording.poll_terminal_event().await),
                        );
                    }
                    continue;
                }
            };
            let is_capture = matches!(&command, AppCommand::Capture(_));
            tracing::info!(action = ?command, "received application command");
            if runtime.is_none() {
                match ProductionCaptureRuntime::new(
                    gallery_events.clone(),
                    recording_lifecycle.clone(),
                    Arc::clone(&recording_presentation),
                    recovery_scans.clone(),
                ) {
                    Ok(value) => runtime = Some(value),
                    Err(error) => {
                        tracing::error!(%error, "capture runtime is unavailable");
                        notifier.show_error(format!("{}: {error}", gettext("Action failed")));
                        if is_capture {
                            ui::window::restore_after_capture();
                        }
                        continue;
                    }
                }
            }
            // Finalization workers keep running if their future is dropped.
            // Always await them so the controller reconciles the saved file
            // and UI state. Captures apply their own deadline independently
            // from any recording finalization observed while selecting.
            let must_complete = is_capture || matches!(&command, AppCommand::StopRecording);
            let execution = runtime.as_mut().unwrap().execute(command, &notifier);
            let result = if must_complete {
                Ok(execution.await)
            } else {
                glib::future_with_timeout(COMMAND_DEADLINE, execution).await
            };
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(_) => {
                    tracing::error!("command timed out; releasing the queue");
                    notifier.show_error(gettext("Klypse stopped responding to that action"));
                    if is_capture {
                        ui::window::restore_after_capture();
                    }
                    continue;
                }
            };
            if is_capture {
                ui::window::restore_after_capture();
            }
            show_outcome(&notifier, outcome);
            tracing::debug!("command completed");
        }
    });
}

fn recording_outcome(result: Result<Option<CaptureRecord>, KlypseError>) -> CaptureOutcome {
    match result {
        Ok(Some(record)) => CaptureOutcome::Saved(record),
        Ok(None) => CaptureOutcome::Ignored,
        Err(error) => capture_error_outcome(error),
    }
}

fn show_outcome(notifier: &ui::window::UiNotifier, outcome: CaptureOutcome) {
    match outcome {
        CaptureOutcome::Saved(record) => {
            tracing::info!(capture_id = %record.id, "capture saved");
            // StopRecording also returns Saved, so the wording has to
            // follow what was actually produced.
            notifier.show_info(match record.kind {
                CaptureKind::Screenshot => gettext("Capture saved"),
                CaptureKind::Video | CaptureKind::Gif => gettext("Recording saved"),
            });
        }
        CaptureOutcome::Cancelled | CaptureOutcome::Ignored => {}
        CaptureOutcome::Failed(error) => {
            tracing::error!(%error, "capture failed");
            notifier.show_error(format!("{}: {error}", gettext("Action failed")));
        }
    }
}

struct ProductionCaptureRuntime {
    service: CaptureService,
    recording: RecordingController,
    capture_backend: Arc<dyn CaptureBackend>,
    repository: Arc<dyn CaptureStore>,
    recording_effects: GtkRecordingEffects,
    x11_backend: Option<Arc<X11CaptureBackend>>,
    copy_after_capture: bool,
    gif_maximum_duration: Duration,
    recovery_scans: Sender<()>,
}

impl ProductionCaptureRuntime {
    fn new(
        gallery_events: Sender<GalleryEvent>,
        recording_lifecycle: Sender<RecordingLifecycleEvent>,
        recording_presentation: Arc<Mutex<RecordingPresentation>>,
        recovery_scans: Sender<()>,
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
        let recording_effects = GtkRecordingEffects {
            gallery_events,
            lifecycle: recording_lifecycle,
            presentation: recording_presentation,
            notify_after_capture,
        };
        let recording = RecordingController::new(
            recording_backend.clone(),
            Arc::clone(&repository),
            paths.clone(),
            recording_backend.display(),
            Arc::new(recording_effects.clone()),
        );
        Ok(Self {
            service: CaptureService::new(backend.clone(), repository.clone(), paths, effects),
            recording,
            capture_backend: backend,
            repository,
            recording_effects,
            x11_backend,
            copy_after_capture,
            gif_maximum_duration,
            recovery_scans,
        })
    }

    fn refresh_capture_settings(&mut self) -> Result<(), KlypseError> {
        let preferences = AppSettings::new().ok();
        let paths = paths_for_capture(preferences.as_ref())?;
        self.copy_after_capture = preferences
            .as_ref()
            .is_none_or(AppSettings::copy_after_capture);
        self.service = CaptureService::new(
            self.capture_backend.clone(),
            self.repository.clone(),
            paths,
            Arc::new(GtkCaptureEffects {
                gallery_events: self.recording_effects.gallery_events.clone(),
                notify_after_capture: preferences
                    .as_ref()
                    .is_none_or(AppSettings::notify_after_capture),
            }),
        );
        Ok(())
    }

    fn refresh_recording_settings(&mut self) -> Result<(), KlypseError> {
        // An active recording and its recovery marker keep their original
        // destination and encoding settings until they have been finalized.
        if self.recording.state() != RecordingUiState::Idle
            || self.recording.active_session().is_some()
        {
            return Ok(());
        }
        let preferences = AppSettings::new().ok();
        let paths = paths_for_capture(preferences.as_ref())?;
        let fps = preferences.as_ref().map_or(12, AppSettings::gif_fps);
        self.gif_maximum_duration = Duration::from_secs(u64::from(
            preferences
                .as_ref()
                .map_or(30, AppSettings::gif_max_seconds),
        ));
        let backend = Arc::new(DesktopRecordingBackend::new(
            paths.clone(),
            fps,
            self.gif_maximum_duration,
        )?);
        let mut effects = self.recording_effects.clone();
        effects.notify_after_capture = preferences
            .as_ref()
            .is_none_or(AppSettings::notify_after_capture);
        self.recording = RecordingController::new(
            backend.clone(),
            self.repository.clone(),
            paths,
            backend.display(),
            Arc::new(effects),
        );
        Ok(())
    }

    async fn execute(
        &mut self,
        command: AppCommand,
        notifier: &ui::window::UiNotifier,
    ) -> CaptureOutcome {
        match command {
            AppCommand::Open => CaptureOutcome::Ignored,
            AppCommand::Capture(mut request) => {
                if let Err(error) = self.refresh_capture_settings() {
                    return capture_error_outcome(error);
                }
                request.copy_to_clipboard &= self.copy_after_capture;
                let mut capture = Box::pin(glib::future_with_timeout(
                    COMMAND_DEADLINE,
                    capture_request(&self.service, self.x11_backend.as_deref(), request),
                ));
                loop {
                    match glib::future_with_timeout(Duration::from_millis(200), capture.as_mut())
                        .await
                    {
                        Ok(Ok(outcome)) => return outcome,
                        Ok(Err(_)) => {
                            tracing::error!("capture timed out; releasing the queue");
                            notifier
                                .show_error(gettext("Klypse stopped responding to that action"));
                            return CaptureOutcome::Ignored;
                        }
                        Err(_) => {
                            if self.recording.terminal_event().is_some() {
                                show_outcome(
                                    notifier,
                                    recording_outcome(self.recording.poll_terminal_event().await),
                                );
                            }
                        }
                    }
                }
            }
            AppCommand::Record(mut request) => {
                if self.recording.state() != RecordingUiState::Idle {
                    return match self.recording.start(request).await {
                        Ok(_) => CaptureOutcome::Ignored,
                        Err(error) => capture_error_outcome(error),
                    };
                }
                if let Err(error) = self.refresh_recording_settings() {
                    return capture_error_outcome(error);
                }
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
            // An automatic EOS may have finalized just before a queued click.
            AppCommand::StopRecording if self.recording.state() == RecordingUiState::Idle => {
                CaptureOutcome::Ignored
            }
            AppCommand::StopRecording => match self.recording.stop().await {
                Ok(record) => CaptureOutcome::Saved(record),
                Err(error) => capture_error_outcome(error),
            },
            AppCommand::AcknowledgeRecordingFailure => match self.recording.acknowledge_failure() {
                Ok(()) => {
                    let _ = self.recovery_scans.try_send(());
                    CaptureOutcome::Ignored
                }
                Err(error) => CaptureOutcome::Failed(error),
            },
            AppCommand::CompleteRecordingRecovery => match self.recording.complete_recovery() {
                Ok(_) => CaptureOutcome::Ignored,
                Err(error) => CaptureOutcome::Failed(error),
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

async fn capture_request(
    service: &CaptureService,
    backend: Option<&X11CaptureBackend>,
    mut request: CaptureRequest,
) -> CaptureOutcome {
    if !request.delay.is_zero() {
        glib::timeout_future(request.delay).await;
        request.delay = Duration::ZERO;
    }
    if matches!(request.target, CaptureTarget::Area | CaptureTarget::Window)
        && request.selection == CaptureSelection::Automatic
        && let Some(backend) = backend
    {
        let result = async {
            let mut snapshot = backend.capture_sync(&CaptureRequest::new(CaptureTarget::Screen))?;
            // Also remove the temporary screenshot on cancellation or timeout.
            let _cleanup = tempfile::TempPath::try_from_path(&snapshot.path)?;
            let frames = backend.window_frames()?;
            let rect = RegionOverlay::select_windows(&snapshot.path, frames)
                .await?
                .ok_or(KlypseError::Cancelled)?;
            crate::capture::crop_snapshot(
                &mut snapshot,
                PixelRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                },
            )?;
            service.save_artifact(request, snapshot)
        }
        .await;
        return match result {
            Ok(record) => CaptureOutcome::Saved(record),
            Err(error) => capture_error_outcome(error),
        };
    }
    service.execute(AppCommand::Capture(request)).await
}

fn paths_for_capture(preferences: Option<&AppSettings>) -> Result<AppPaths, KlypseError> {
    let mut paths = AppPaths::discover().map_err(storage_error)?;
    if let Some(directory) = preferences.and_then(AppSettings::capture_directory) {
        paths.captures = directory;
    }
    paths.ensure().map_err(storage_error)?;
    Ok(paths)
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
}

fn manage_recording_lifecycle(
    application: &adw::Application,
    events: Receiver<RecordingLifecycleEvent>,
) {
    let application = application.clone();
    glib::spawn_future_local(async move {
        let mut hold_guard = None;
        while let Ok(event) = events.recv().await {
            match event {
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
                }
            }
        }
        drop(hold_guard);
    });
}

#[derive(Clone)]
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
