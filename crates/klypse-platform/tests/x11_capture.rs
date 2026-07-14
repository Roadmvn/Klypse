use klypse_domain::{CaptureRequest, CaptureSelection, CaptureTarget, DisplayServer};
use klypse_platform::X11CaptureBackend;
use x11rb::{
    connection::Connection,
    protocol::xproto::{ConnectionExt, CreateWindowAux, WindowClass},
};

#[test]
fn captures_a_known_color_x11_window() {
    let Ok((connection, screen_number)) = x11rb::connect(None) else {
        return;
    };
    let screen = &connection.setup().roots[screen_number];
    let window = connection.generate_id().unwrap();
    connection
        .create_window(
            screen.root_depth,
            window,
            screen.root,
            10,
            10,
            64,
            48,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new().background_pixel(0x00ff_0000),
        )
        .unwrap();
    connection.map_window(window).unwrap();
    connection.flush().unwrap();
    connection.get_input_focus().unwrap().reply().unwrap();

    let output = tempfile::tempdir().unwrap();
    let backend = X11CaptureBackend::connect(output.path()).unwrap();
    let artifact = backend
        .capture_sync(&CaptureRequest {
            target: CaptureTarget::Window,
            copy_to_clipboard: false,
            selection: CaptureSelection::X11Window(window),
        })
        .unwrap();

    assert_eq!((artifact.width, artifact.height), (64, 48));
    assert_eq!(artifact.backend, DisplayServer::X11);
    let image = image::open(&artifact.path).unwrap().to_rgba8();
    let center = image.get_pixel(32, 24).0;
    assert!(center[0] > 200, "expected red pixel, got {center:?}");
    assert!(center[1] < 40 && center[2] < 40, "got {center:?}");
}
