use std::sync::Mutex;

use klypse_domain::{CaptureTarget, KlypseError};
use klypse_platform::{
    PortalRecordingClient, PortalRecordingClientError, PortalRecordingSource,
    portal_screencast_options,
};

struct FakeClient {
    options: Mutex<Vec<klypse_platform::PortalScreencastOptions>>,
    result:
        Mutex<Option<Result<klypse_platform::PortalStreamDescriptor, PortalRecordingClientError>>>,
}

#[async_trait::async_trait]
impl PortalRecordingClient for FakeClient {
    async fn select_stream(
        &self,
        options: klypse_platform::PortalScreencastOptions,
        _identifier: Option<&ashpd::WindowIdentifier>,
    ) -> Result<klypse_platform::PortalStreamDescriptor, PortalRecordingClientError> {
        self.options.lock().unwrap().push(options);
        self.result.lock().unwrap().take().unwrap()
    }
}

#[test]
fn screen_request_selects_only_one_monitor_with_embedded_cursor() {
    let options = portal_screencast_options(CaptureTarget::Screen);

    assert!(options.monitors);
    assert!(!options.windows);
    assert!(!options.multiple);
    assert!(options.embedded_cursor);
}

#[test]
fn area_request_delegates_selection_to_the_compositor() {
    let options = portal_screencast_options(CaptureTarget::Area);

    assert!(options.monitors);
    assert!(options.windows);
    assert!(!options.multiple);
}

#[test]
fn portal_cancellation_maps_to_domain_cancellation() {
    let client = FakeClient {
        options: Mutex::new(Vec::new()),
        result: Mutex::new(Some(Err(PortalRecordingClientError::Cancelled))),
    };

    let result = futures_lite::future::block_on(PortalRecordingSource::select_with(
        &client,
        CaptureTarget::Window,
    ));

    assert!(matches!(result, Err(KlypseError::Cancelled)));
    assert_eq!(client.options.lock().unwrap().len(), 1);
}
