mod capabilities;
mod hotkey;
pub mod portal;
mod selector;
pub mod x11;

pub use capabilities::{CapabilityReport, CapabilityStatus, DisplayProbe};
pub use hotkey::{
    HotkeyBinding, HotkeyManager, HotkeyMode, ParsedAccelerator, cli_fallback_commands,
    parse_accelerator,
};
pub use portal::{
    AvailableTargetSet, PortalCaptureBackend, PortalCaptureClient, PortalClientError,
    PortalHotkeyBackend, PortalHotkeyClientError, PortalSelection, PortalShortcutSpec,
    PortalTarget, map_portal_hotkey_error, map_portal_target, portal_shortcut_specs,
    validate_portal_bindings,
};
pub use selector::{BackendChoice, BackendSelector};
pub use x11::{Rect, X11CaptureBackend, X11HotkeyBackend, bgra_to_rgba, normalize_selection};

pub const CRATE_NAME: &str = "klypse-platform";
