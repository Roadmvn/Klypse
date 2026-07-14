use std::{path::Path, sync::Arc};

use async_channel::{Receiver, Sender};
use gettextrs::gettext;
use gtk::{gio, glib, prelude::*};
use klypse_domain::{
    AppCommand, CaptureBackend, CaptureKind, CaptureRequest, CaptureSelection, CaptureTarget,
    HotkeyAction, KlypseError, PixelRect, RecordingRequest,
};
use klypse_platform::{
    BackendChoice, BackendSelector, CapabilityReport, HotkeyBinding, HotkeyManager,
    PortalCaptureBackend, X11CaptureBackend,
};
use klypse_storage::{AppPaths, CaptureRecord, CaptureRepository, open_database};
use libadwaita as adw;

use crate::{
    APP_ID,
    capture::{CaptureEffects, CaptureOutcome, CaptureService, CaptureStage},
    cli,
    desktop::{clipboard::copy_static_image, notification::notify_capture_saved},
    gallery::GalleryEvent,
    settings::AppSettings,
    ui,
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

    connect_activate(&application, sender.clone(), gallery_event_receiver);
    connect_command_line(&application, sender.clone());
    connect_open_capture_action(&application, gallery_event_sender.clone());
    dispatch_hotkey_actions(hotkey_action_receiver, sender);
    start_hotkeys(hotkey_action_sender);
    dispatch_commands(receiver, gallery_event_sender);

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
) {
    application.connect_activate(move |application| {
        ui::window::present(application, sender.clone(), gallery_events.clone());
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

fn dispatch_commands(receiver: Receiver<AppCommand>, gallery_events: Sender<GalleryEvent>) {
    glib::spawn_future_local(async move {
        let mut runtime = None;
        while let Ok(command) = receiver.recv().await {
            tracing::info!(action = ?command, "received application command");
            if runtime.is_none() {
                match ProductionCaptureRuntime::new(gallery_events.clone()) {
                    Ok(value) => runtime = Some(value),
                    Err(error) => {
                        tracing::error!(%error, "capture runtime is unavailable");
                        continue;
                    }
                }
            }
            let outcome = runtime.as_ref().unwrap().execute(command).await;
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
    x11_backend: Option<Arc<X11CaptureBackend>>,
    copy_after_capture: bool,
}

impl ProductionCaptureRuntime {
    fn new(gallery_events: Sender<GalleryEvent>) -> Result<Self, KlypseError> {
        let preferences = AppSettings::new().ok();
        let mut paths = AppPaths::discover().map_err(storage_error)?;
        if let Some(directory) = preferences
            .as_ref()
            .and_then(AppSettings::capture_directory)
        {
            paths.captures = directory;
        }
        let repository = CaptureRepository::new(open_database(&paths).map_err(storage_error)?);
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
        let effects = Arc::new(GtkCaptureEffects {
            gallery_events,
            notify_after_capture: preferences
                .as_ref()
                .is_none_or(AppSettings::notify_after_capture),
        });
        Ok(Self {
            service: CaptureService::new(backend, Arc::new(repository), paths, effects),
            x11_backend,
            copy_after_capture,
        })
    }

    async fn execute(&self, mut command: AppCommand) -> CaptureOutcome {
        if let AppCommand::Capture(request) = &mut command {
            request.copy_to_clipboard &= self.copy_after_capture;
        }
        if let (Some(backend), AppCommand::Capture(request)) = (&self.x11_backend, &mut command)
            && request.target == CaptureTarget::Area
            && request.selection == CaptureSelection::Automatic
        {
            let snapshot_request = CaptureRequest::new(CaptureTarget::Screen);
            let snapshot = match backend.capture_sync(&snapshot_request) {
                Ok(snapshot) => snapshot,
                Err(error) => return CaptureOutcome::Failed(error),
            };
            let selection = RegionOverlay::select(&snapshot.path).await;
            let _ = std::fs::remove_file(&snapshot.path);
            match selection {
                Ok(Some(rect)) => {
                    request.selection = CaptureSelection::Region(PixelRect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    });
                }
                Ok(None) | Err(KlypseError::Cancelled) => return CaptureOutcome::Cancelled,
                Err(error) => return CaptureOutcome::Failed(error),
            }
        }
        self.service.execute(command).await
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

fn storage_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Storage(error.to_string())
}
