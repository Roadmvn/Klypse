use std::sync::Mutex;

use klypse_domain::{
    CaptureBackend, CaptureRequest, CaptureSelection, CaptureTarget, DisplayServer, KlypseError,
};
use klypse_platform::{
    AvailableTargetSet, PortalCaptureBackend, PortalCaptureClient, PortalClientError,
    PortalSelection,
};
use url::Url;

struct FakePortalClient {
    version: u32,
    targets: AvailableTargetSet,
    result: Result<Url, PortalClientError>,
    selections: Mutex<Vec<PortalSelection>>,
}

#[async_trait::async_trait]
impl PortalCaptureClient for FakePortalClient {
    async fn version(&self) -> Result<u32, PortalClientError> {
        Ok(self.version)
    }

    async fn available_targets(&self) -> Result<AvailableTargetSet, PortalClientError> {
        Ok(self.targets)
    }

    async fn screenshot(
        &self,
        selection: PortalSelection,
        _identifier: Option<&ashpd::WindowIdentifier>,
    ) -> Result<Url, PortalClientError> {
        self.selections.lock().unwrap().push(selection);
        self.result.clone()
    }
}

fn request(target: CaptureTarget) -> CaptureRequest {
    CaptureRequest {
        target,
        delay: std::time::Duration::ZERO,
        copy_to_clipboard: false,
        selection: CaptureSelection::Automatic,
    }
}

#[test]
fn successful_portal_capture_is_copied_without_exposing_the_uri() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("portal-result.png");
    image::RgbaImage::new(32, 24).save(&source).unwrap();
    let client = FakePortalClient {
        version: 3,
        targets: AvailableTargetSet::AREA,
        result: Ok(Url::from_file_path(&source).unwrap()),
        selections: Mutex::new(Vec::new()),
    };
    let output = directory.path().join("output");
    let backend = PortalCaptureBackend::with_client(client, &output).unwrap();

    let artifact =
        futures_lite::future::block_on(backend.capture(&request(CaptureTarget::Area))).unwrap();

    assert_eq!((artifact.width, artifact.height), (32, 24));
    assert_eq!(artifact.backend, DisplayServer::Wayland);
    assert!(artifact.path.starts_with(&output));
    assert_ne!(artifact.path, source);
}

#[test]
fn portal_cancellation_and_permission_denial_are_typed() {
    for (client_error, expected_permission) in [
        (PortalClientError::Cancelled, false),
        (PortalClientError::PermissionDenied("denied".into()), true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let backend = PortalCaptureBackend::with_client(
            FakePortalClient {
                version: 1,
                targets: AvailableTargetSet::default(),
                result: Err(client_error),
                selections: Mutex::new(Vec::new()),
            },
            directory.path(),
        )
        .unwrap();

        let error =
            futures_lite::future::block_on(backend.capture(&request(CaptureTarget::ActiveWindow)))
                .unwrap_err();

        if expected_permission {
            assert!(matches!(error, KlypseError::PermissionDenied(_)));
        } else {
            assert!(matches!(error, KlypseError::Cancelled));
        }
        assert!(directory.path().read_dir().unwrap().next().is_none());
    }
}

#[test]
fn old_portal_versions_do_not_require_target_properties() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.png");
    image::RgbaImage::new(4, 4).save(&source).unwrap();
    let output = directory.path().join("output");
    let backend = PortalCaptureBackend::with_client(
        FakePortalClient {
            version: 2,
            targets: AvailableTargetSet::AREA,
            result: Ok(Url::from_file_path(&source).unwrap()),
            selections: Mutex::new(Vec::new()),
        },
        &output,
    )
    .unwrap();

    futures_lite::future::block_on(backend.capture(&request(CaptureTarget::Area))).unwrap();

    let copied = output
        .read_dir()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(copied.len(), 1);
}
