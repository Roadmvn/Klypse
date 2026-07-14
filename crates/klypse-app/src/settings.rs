use std::path::{Path, PathBuf};

use gtk::gio::{self, prelude::*};
use klypse_domain::HotkeyAction;
use thiserror::Error;

use crate::APP_ID;

const CAPTURE_DIRECTORY: &str = "capture-directory";
const COPY_AFTER_CAPTURE: &str = "copy-after-capture";
const NOTIFY_AFTER_CAPTURE: &str = "notify-after-capture";
const LANGUAGE: &str = "language";
const GIF_FPS: &str = "gif-fps";
const GIF_MAX_SECONDS: &str = "gif-max-seconds";

#[derive(Clone)]
pub struct AppSettings {
    inner: gio::Settings,
}

impl AppSettings {
    pub fn new() -> Result<Self, SettingsError> {
        Self::from_backend(None)
    }

    pub fn with_backend(backend: gio::SettingsBackend) -> Result<Self, SettingsError> {
        Self::from_backend(Some(&backend))
    }

    fn from_backend(backend: Option<&gio::SettingsBackend>) -> Result<Self, SettingsError> {
        let source = gio::SettingsSchemaSource::default().ok_or(SettingsError::MissingSchema)?;
        let schema = source
            .lookup(APP_ID, true)
            .ok_or(SettingsError::MissingSchema)?;
        Ok(Self {
            inner: gio::Settings::new_full(&schema, backend, None),
        })
    }

    pub fn capture_directory(&self) -> Option<PathBuf> {
        let value = self.inner.string(CAPTURE_DIRECTORY);
        (!value.is_empty()).then(|| PathBuf::from(value.as_str()))
    }

    pub fn set_capture_directory(&self, value: Option<&Path>) -> Result<(), SettingsError> {
        let value = match value {
            Some(path) => path.to_str().ok_or(SettingsError::NonUtf8Path)?,
            None => "",
        };
        self.set_string(CAPTURE_DIRECTORY, value)
    }

    pub fn copy_after_capture(&self) -> bool {
        self.inner.boolean(COPY_AFTER_CAPTURE)
    }

    pub fn set_copy_after_capture(&self, value: bool) -> Result<(), SettingsError> {
        self.inner
            .set_boolean(COPY_AFTER_CAPTURE, value)
            .map_err(SettingsError::write)
    }

    pub fn notify_after_capture(&self) -> bool {
        self.inner.boolean(NOTIFY_AFTER_CAPTURE)
    }

    pub fn set_notify_after_capture(&self, value: bool) -> Result<(), SettingsError> {
        self.inner
            .set_boolean(NOTIFY_AFTER_CAPTURE, value)
            .map_err(SettingsError::write)
    }

    pub fn language(&self) -> String {
        self.inner.string(LANGUAGE).to_string()
    }

    pub fn set_language(&self, value: &str) -> Result<(), SettingsError> {
        if !matches!(value, "system" | "en" | "fr") {
            return Err(SettingsError::InvalidValue {
                key: LANGUAGE,
                value: value.to_owned(),
            });
        }
        self.set_string(LANGUAGE, value)
    }

    pub fn gif_fps(&self) -> u32 {
        self.inner.uint(GIF_FPS)
    }

    pub fn set_gif_fps(&self, value: u32) -> Result<(), SettingsError> {
        self.set_bounded_uint(GIF_FPS, value)
    }

    pub fn gif_max_seconds(&self) -> u32 {
        self.inner.uint(GIF_MAX_SECONDS)
    }

    pub fn set_gif_max_seconds(&self, value: u32) -> Result<(), SettingsError> {
        self.set_bounded_uint(GIF_MAX_SECONDS, value)
    }

    pub fn shortcut(&self, action: HotkeyAction) -> Option<String> {
        shortcut_key(action).map(|key| self.inner.string(key).to_string())
    }

    pub fn set_shortcut(&self, action: HotkeyAction, value: &str) -> Result<(), SettingsError> {
        let key = shortcut_key(action).ok_or_else(|| SettingsError::InvalidValue {
            key: "shortcut",
            value: action.id().to_owned(),
        })?;
        if value.trim().is_empty() {
            return Err(SettingsError::InvalidValue {
                key,
                value: value.to_owned(),
            });
        }
        self.set_string(key, value)
    }

    fn set_bounded_uint(&self, key: &'static str, value: u32) -> Result<(), SettingsError> {
        if !(1..=30).contains(&value) {
            return Err(SettingsError::InvalidValue {
                key,
                value: value.to_string(),
            });
        }
        self.inner
            .set_uint(key, value)
            .map_err(SettingsError::write)
    }

    fn set_string(&self, key: &'static str, value: &str) -> Result<(), SettingsError> {
        self.inner
            .set_string(key, value)
            .map_err(SettingsError::write)
    }
}

fn shortcut_key(action: HotkeyAction) -> Option<&'static str> {
    match action {
        HotkeyAction::CaptureArea => Some("shortcut-area"),
        HotkeyAction::CaptureScreen => Some("shortcut-screen"),
        HotkeyAction::CaptureWindow => Some("shortcut-window"),
        HotkeyAction::RecordVideo => Some("shortcut-video"),
        HotkeyAction::RecordGif => Some("shortcut-gif"),
        HotkeyAction::StopRecording => None,
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("the {APP_ID} GSettings schema is not installed")]
    MissingSchema,
    #[error("the selected capture directory is not valid UTF-8")]
    NonUtf8Path,
    #[error("invalid value {value:?} for setting {key}")]
    InvalidValue { key: &'static str, value: String },
    #[error("unable to save application setting: {0}")]
    Write(String),
}

impl SettingsError {
    fn write(error: impl std::fmt::Display) -> Self {
        Self::Write(error.to_string())
    }
}
