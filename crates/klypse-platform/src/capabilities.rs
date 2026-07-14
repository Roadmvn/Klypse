use std::{env, path::Path, process::Command};

use klypse_domain::DisplayServer;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityStatus {
    pub available: bool,
    pub detail: String,
}

impl CapabilityStatus {
    fn available(detail: impl Into<String>) -> Self {
        Self {
            available: true,
            detail: detail.into(),
        }
    }

    fn unavailable(detail: impl Into<String>) -> Self {
        Self {
            available: false,
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityReport {
    pub display: DisplayServer,
    pub static_capture: CapabilityStatus,
    pub video_recording: CapabilityStatus,
    pub gif_recording: CapabilityStatus,
    pub global_shortcuts: CapabilityStatus,
}

impl CapabilityReport {
    pub fn detect() -> Self {
        Self::from_probe(DisplayProbe::detect())
    }

    pub fn from_probe(probe: DisplayProbe) -> Self {
        let display = if probe.wayland_available {
            DisplayServer::Wayland
        } else {
            DisplayServer::X11
        };

        let static_capture = match display {
            DisplayServer::Wayland if probe.portal_available => {
                CapabilityStatus::available("Desktop portal screenshot support is available")
            }
            DisplayServer::Wayland => {
                CapabilityStatus::unavailable("Desktop portal screenshot support is unavailable")
            }
            DisplayServer::X11 if probe.x11_available => {
                CapabilityStatus::available("X11 capture is available")
            }
            DisplayServer::X11 => {
                CapabilityStatus::unavailable("No graphical display was detected")
            }
        };

        let video_recording = match display {
            DisplayServer::Wayland
                if probe.portal_available
                    && probe.pipewire_available
                    && probe.pipewiresrc_available
                    && probe.vp8enc_available
                    && probe.webmmux_available =>
            {
                CapabilityStatus::available("Wayland WebM recording is available")
            }
            DisplayServer::Wayland => CapabilityStatus::unavailable(
                "Wayland recording requires the desktop portal, PipeWire, VP8, and WebM plugins",
            ),
            DisplayServer::X11
                if probe.x11_available
                    && probe.ximagesrc_available
                    && probe.vp8enc_available
                    && probe.webmmux_available =>
            {
                CapabilityStatus::available("X11 WebM recording is available")
            }
            DisplayServer::X11 => CapabilityStatus::unavailable(
                "X11 recording requires the XImage, VP8, and WebM plugins",
            ),
        };

        let gif_recording = match display {
            DisplayServer::Wayland
                if probe.portal_available
                    && probe.pipewire_available
                    && probe.pipewiresrc_available =>
            {
                CapabilityStatus::available("Wayland GIF recording is available")
            }
            DisplayServer::Wayland => CapabilityStatus::unavailable(
                "Wayland GIF recording requires the desktop portal and PipeWire",
            ),
            DisplayServer::X11 if probe.x11_available && probe.ximagesrc_available => {
                CapabilityStatus::available("X11 GIF recording is available")
            }
            DisplayServer::X11 => {
                CapabilityStatus::unavailable("X11 GIF recording requires the XImage plugin")
            }
        };

        let global_shortcuts = match display {
            DisplayServer::Wayland if probe.portal_available => {
                CapabilityStatus::available("Portal global shortcuts are available")
            }
            DisplayServer::Wayland => {
                CapabilityStatus::unavailable("Portal global shortcuts are unavailable")
            }
            DisplayServer::X11 if probe.x11_available => {
                CapabilityStatus::available("X11 global shortcuts are available")
            }
            DisplayServer::X11 => {
                CapabilityStatus::unavailable("No X11 display is available for global shortcuts")
            }
        };

        Self {
            display,
            static_capture,
            video_recording,
            gif_recording,
            global_shortcuts,
        }
    }

    pub const fn display_name(&self) -> &'static str {
        match self.display {
            DisplayServer::X11 => "X11",
            DisplayServer::Wayland => "Wayland",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayProbe {
    wayland_available: bool,
    x11_available: bool,
    portal_available: bool,
    pipewire_available: bool,
    vp8enc_available: bool,
    webmmux_available: bool,
    pipewiresrc_available: bool,
    ximagesrc_available: bool,
}

impl DisplayProbe {
    pub fn detect() -> Self {
        let runtime_dir = env::var_os("XDG_RUNTIME_DIR");
        let pipewire_available = runtime_dir
            .as_deref()
            .is_some_and(|directory| Path::new(directory).join("pipewire-0").exists());

        Self {
            wayland_available: env::var_os("WAYLAND_DISPLAY").is_some(),
            x11_available: env::var_os("DISPLAY").is_some(),
            portal_available: portal_has_owner(),
            pipewire_available,
            vp8enc_available: gst_element_exists("vp8enc"),
            webmmux_available: gst_element_exists("webmmux"),
            pipewiresrc_available: gst_element_exists("pipewiresrc"),
            ximagesrc_available: gst_element_exists("ximagesrc"),
        }
    }

    pub fn from_values(
        wayland_display: Option<String>,
        x11_display: Option<String>,
        portal_available: bool,
        pipewire_available: bool,
        gstreamer_plugins_available: bool,
    ) -> Self {
        Self {
            wayland_available: wayland_display.is_some(),
            x11_available: x11_display.is_some(),
            portal_available,
            pipewire_available,
            vp8enc_available: gstreamer_plugins_available,
            webmmux_available: gstreamer_plugins_available,
            pipewiresrc_available: gstreamer_plugins_available,
            ximagesrc_available: gstreamer_plugins_available,
        }
    }
}

fn portal_has_owner() -> bool {
    Command::new("timeout")
        .args([
            "1",
            "gdbus",
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            "org.freedesktop.portal.Desktop",
        ])
        .output()
        .is_ok_and(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("true")
        })
}

fn gst_element_exists(element: &str) -> bool {
    Command::new("gst-inspect-1.0")
        .args(["--exists", element])
        .status()
        .is_ok_and(|status| status.success())
}
