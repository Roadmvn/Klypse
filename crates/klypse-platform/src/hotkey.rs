use async_channel::Sender;
use klypse_domain::{DisplayServer, HotkeyAction, KlypseError};

use crate::{CapabilityReport, portal::PortalHotkeyBackend, x11::X11HotkeyBackend};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HotkeyBinding {
    pub action: HotkeyAction,
    pub accelerator: String,
    pub description: String,
}

impl HotkeyBinding {
    pub fn new(action: HotkeyAction, accelerator: impl Into<String>) -> Self {
        Self {
            action,
            accelerator: accelerator.into(),
            description: default_description(action).to_owned(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyMode {
    X11,
    Portal,
    DesktopCliFallback,
}

enum HotkeySession {
    X11(X11HotkeyBackend),
    Portal(PortalHotkeyBackend),
    Fallback,
}

pub struct HotkeyManager {
    preferred_mode: HotkeyMode,
    mode: HotkeyMode,
    bindings: Vec<HotkeyBinding>,
    actions: Sender<HotkeyAction>,
    session: Option<HotkeySession>,
}

impl HotkeyManager {
    pub fn select(report: &CapabilityReport) -> HotkeyMode {
        match report.display {
            DisplayServer::X11 if report.global_shortcuts.available => HotkeyMode::X11,
            DisplayServer::Wayland if report.global_shortcuts.available => HotkeyMode::Portal,
            _ => HotkeyMode::DesktopCliFallback,
        }
    }

    pub async fn start(
        report: &CapabilityReport,
        bindings: Vec<HotkeyBinding>,
        actions: Sender<HotkeyAction>,
    ) -> Result<Self, KlypseError> {
        let preferred_mode = Self::select(report);
        let (mode, session) = start_session(preferred_mode, &bindings, actions.clone()).await?;
        Ok(Self {
            preferred_mode,
            mode,
            bindings,
            actions,
            session: Some(session),
        })
    }

    pub const fn mode(&self) -> HotkeyMode {
        self.mode
    }

    pub async fn rebind(&mut self, bindings: Vec<HotkeyBinding>) -> Result<(), KlypseError> {
        match self.session.as_mut() {
            Some(HotkeySession::X11(backend)) => {
                backend.rebind(&bindings)?;
                self.bindings = bindings;
                Ok(())
            }
            Some(HotkeySession::Portal(_)) => self.rebind_portal(bindings).await,
            Some(HotkeySession::Fallback) | None => {
                let (mode, session) =
                    start_session(self.preferred_mode, &bindings, self.actions.clone()).await?;
                self.mode = mode;
                self.session = Some(session);
                self.bindings = bindings;
                Ok(())
            }
        }
    }

    async fn rebind_portal(&mut self, bindings: Vec<HotkeyBinding>) -> Result<(), KlypseError> {
        if let Some(HotkeySession::Portal(backend)) = self.session.as_mut() {
            backend.stop().await;
        }
        let previous = self.bindings.clone();
        match PortalHotkeyBackend::start(&bindings, self.actions.clone()).await {
            Ok(backend) => {
                self.session = Some(HotkeySession::Portal(backend));
                self.mode = HotkeyMode::Portal;
                self.bindings = bindings;
                Ok(())
            }
            Err(error) => {
                match PortalHotkeyBackend::start(&previous, self.actions.clone()).await {
                    Ok(backend) => {
                        self.session = Some(HotkeySession::Portal(backend));
                        self.mode = HotkeyMode::Portal;
                    }
                    Err(_) => {
                        self.session = Some(HotkeySession::Fallback);
                        self.mode = HotkeyMode::DesktopCliFallback;
                    }
                }
                Err(error)
            }
        }
    }

    pub async fn stop(&mut self) -> Result<(), KlypseError> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        match session {
            HotkeySession::X11(backend) => backend.stop(),
            HotkeySession::Portal(backend) => {
                backend.stop().await;
                Ok(())
            }
            HotkeySession::Fallback => Ok(()),
        }
    }
}

async fn start_session(
    preferred_mode: HotkeyMode,
    bindings: &[HotkeyBinding],
    actions: Sender<HotkeyAction>,
) -> Result<(HotkeyMode, HotkeySession), KlypseError> {
    match preferred_mode {
        // A desktop that already owns these keys - which is the norm on GNOME,
        // KDE and Xfce for the Print family - makes registration fail. Degrade
        // to the CLI fallback like the portal arm does instead of propagating:
        // the caller treats an error as fatal and stops listening for rebinds,
        // which used to leave the shortcut preferences dead for the whole
        // session.
        HotkeyMode::X11 => match X11HotkeyBackend::start(bindings, actions) {
            Ok(backend) => Ok((HotkeyMode::X11, HotkeySession::X11(backend))),
            Err(_) => Ok((HotkeyMode::DesktopCliFallback, HotkeySession::Fallback)),
        },
        HotkeyMode::Portal => match PortalHotkeyBackend::start(bindings, actions).await {
            Ok(backend) => Ok((HotkeyMode::Portal, HotkeySession::Portal(backend))),
            Err(_) => Ok((HotkeyMode::DesktopCliFallback, HotkeySession::Fallback)),
        },
        HotkeyMode::DesktopCliFallback => Ok((preferred_mode, HotkeySession::Fallback)),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedAccelerator {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
    pub key: String,
}

pub fn parse_accelerator(value: &str) -> Result<ParsedAccelerator, KlypseError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid_accelerator());
    }
    let mut parsed = ParsedAccelerator {
        control: false,
        shift: false,
        alt: false,
        super_key: false,
        key: String::new(),
    };
    let mut rest = value;
    let mut used_gtk_syntax = false;
    while let Some(modifier) = rest.strip_prefix('<') {
        let Some(end) = modifier.find('>') else {
            return Err(invalid_accelerator());
        };
        apply_modifier(&mut parsed, &modifier[..end])?;
        rest = modifier[end + 1..].trim();
        used_gtk_syntax = true;
    }
    if used_gtk_syntax {
        parsed.key = rest.to_owned();
    } else {
        let mut parts = rest.split('+').map(str::trim).collect::<Vec<_>>();
        parsed.key = parts.pop().unwrap_or_default().to_owned();
        for modifier in parts {
            apply_modifier(&mut parsed, modifier)?;
        }
    }
    if parsed.key.is_empty() {
        return Err(invalid_accelerator());
    }
    Ok(parsed)
}

fn apply_modifier(accelerator: &mut ParsedAccelerator, modifier: &str) -> Result<(), KlypseError> {
    match modifier.to_ascii_lowercase().as_str() {
        "primary" | "control" | "ctrl" => accelerator.control = true,
        "shift" => accelerator.shift = true,
        "alt" | "mod1" => accelerator.alt = true,
        "super" | "meta" | "mod4" => accelerator.super_key = true,
        _ => return Err(invalid_accelerator()),
    }
    Ok(())
}

fn invalid_accelerator() -> KlypseError {
    KlypseError::InvalidRequest("invalid keyboard accelerator".into())
}

pub const fn cli_fallback_commands() -> [(HotkeyAction, &'static str); 6] {
    [
        (HotkeyAction::CaptureArea, "klypse capture area"),
        (HotkeyAction::CaptureScreen, "klypse capture screen"),
        (HotkeyAction::CaptureWindow, "klypse capture active-window"),
        (HotkeyAction::RecordVideo, "klypse record video screen"),
        (HotkeyAction::RecordGif, "klypse record gif area"),
        (HotkeyAction::StopRecording, "klypse stop"),
    ]
}

pub const fn default_description(action: HotkeyAction) -> &'static str {
    match action {
        HotkeyAction::CaptureArea => "Capture area",
        HotkeyAction::CaptureScreen => "Capture screen",
        HotkeyAction::CaptureWindow => "Capture window",
        HotkeyAction::RecordVideo => "Record video",
        HotkeyAction::RecordGif => "Record GIF",
        HotkeyAction::StopRecording => "Stop recording",
    }
}

pub(crate) fn action_from_id(id: &str) -> Option<HotkeyAction> {
    [
        HotkeyAction::CaptureArea,
        HotkeyAction::CaptureScreen,
        HotkeyAction::CaptureWindow,
        HotkeyAction::RecordVideo,
        HotkeyAction::RecordGif,
        HotkeyAction::StopRecording,
    ]
    .into_iter()
    .find(|action| action.id() == id)
}
