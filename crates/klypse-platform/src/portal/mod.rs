mod capture;
mod hotkeys;
mod recording;

pub use capture::{
    AvailableTargetSet, PortalCaptureBackend, PortalCaptureClient, PortalClientError,
    PortalSelection, PortalTarget, map_portal_target,
};
pub use hotkeys::{
    PortalHotkeyBackend, PortalHotkeyClientError, PortalShortcutSpec, map_portal_hotkey_error,
    portal_shortcut_specs, validate_portal_bindings,
};
pub use recording::{
    PortalRecordingClient, PortalRecordingClientError, PortalRecordingSource,
    PortalScreencastOptions, PortalStreamDescriptor, portal_screencast_options,
};
