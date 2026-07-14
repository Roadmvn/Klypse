use std::os::fd::{AsRawFd, OwnedFd};

use gstreamer::prelude::*;
use klypse_platform::{PortalRecordingSource, PortalStreamDescriptor};

#[test]
fn pipewire_element_keeps_the_remote_fd_and_node_path() {
    if !gstreamer_element_exists("pipewiresrc") {
        eprintln!("pipewiresrc is unavailable");
        return;
    }
    gstreamer::init().unwrap();
    let (remote, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let fd: OwnedFd = remote.into();
    let raw_fd = fd.as_raw_fd();
    let source =
        PortalRecordingSource::from_stream(PortalStreamDescriptor::new(42, 1280, 720, fd)).unwrap();

    let element = source.build_element().unwrap();

    assert_eq!(element.property::<String>("path"), "42");
    assert_eq!(element.property::<i32>("fd"), raw_fd);
    assert!(element.property::<bool>("do-timestamp"));
    assert_eq!(source.dimensions(), (1280, 720));
}

fn gstreamer_element_exists(element: &str) -> bool {
    std::process::Command::new("gst-inspect-1.0")
        .args(["--exists", element])
        .status()
        .is_ok_and(|status| status.success())
}
