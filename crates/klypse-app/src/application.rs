use std::{path::Path, sync::Arc};

use async_channel::{Receiver, Sender};
use gtk::{gdk, gio, glib, prelude::*};
use klypse_domain::{
    AppCommand, CaptureBackend, CaptureRequest, CaptureSelection, CaptureTarget, KlypseError,
    PixelRect,
};
use klypse_platform::{
    BackendChoice, BackendSelector, CapabilityReport, PortalCaptureBackend, X11CaptureBackend,
};
use klypse_storage::{AppPaths, CaptureRecord, CaptureRepository, open_database};
use libadwaita as adw;

use crate::{
    APP_ID,
    capture::{CaptureEffects, CaptureOutcome, CaptureService, CaptureStage},
    cli, ui,
    ui::region_overlay::RegionOverlay,
};

pub fn run() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let (sender, receiver) = async_channel::unbounded();
    let (gallery_refresh_sender, gallery_refresh_receiver) = async_channel::unbounded();

    connect_activate(&application, sender.clone(), gallery_refresh_receiver);
    connect_command_line(&application, sender);
    dispatch_commands(receiver, gallery_refresh_sender);

    application.run()
}

fn connect_activate(
    application: &adw::Application,
    sender: Sender<AppCommand>,
    gallery_refreshes: Receiver<()>,
) {
    application.connect_activate(move |application| {
        ui::window::present(application, sender.clone(), gallery_refreshes.clone());
    });
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

fn dispatch_commands(receiver: Receiver<AppCommand>, gallery_refreshes: Sender<()>) {
    glib::spawn_future_local(async move {
        let mut runtime = None;
        while let Ok(command) = receiver.recv().await {
            tracing::info!(action = ?command, "received application command");
            if runtime.is_none() {
                match ProductionCaptureRuntime::new(gallery_refreshes.clone()) {
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
}

impl ProductionCaptureRuntime {
    fn new(gallery_refreshes: Sender<()>) -> Result<Self, KlypseError> {
        let paths = AppPaths::discover().map_err(storage_error)?;
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
        let effects = Arc::new(GtkCaptureEffects { gallery_refreshes });
        Ok(Self {
            service: CaptureService::new(backend, Arc::new(repository), paths, effects),
            x11_backend,
        })
    }

    async fn execute(&self, mut command: AppCommand) -> CaptureOutcome {
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
    gallery_refreshes: Sender<()>,
}

impl CaptureEffects for GtkCaptureEffects {
    fn stage(&self, stage: CaptureStage) {
        tracing::debug!(?stage, "capture stage completed");
    }

    fn refresh_gallery(&self) -> Result<(), KlypseError> {
        self.gallery_refreshes.try_send(()).map_err(|_| {
            KlypseError::UnavailableCapability("gallery refresh channel is closed".into())
        })
    }

    fn copy_to_clipboard(&self, path: &Path) -> Result<(), KlypseError> {
        let display = gdk::Display::default().ok_or_else(|| {
            KlypseError::UnavailableCapability("clipboard display is unavailable".into())
        })?;
        let texture = gdk::Texture::from_filename(path)
            .map_err(|error| KlypseError::Media(error.to_string()))?;
        display.clipboard().set_texture(&texture);
        Ok(())
    }

    fn notify_saved(&self, record: &CaptureRecord) {
        tracing::debug!(capture_id = %record.id, "capture notification hook invoked");
    }
}

fn storage_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::Storage(error.to_string())
}
