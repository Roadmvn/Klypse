use async_trait::async_trait;
use uuid::Uuid;

use crate::{CaptureArtifact, CaptureRequest, HotkeyAction, KlypseError, RecordingRequest};

#[async_trait]
pub trait CaptureBackend: Send + Sync {
    async fn capture(&self, request: &CaptureRequest) -> Result<CaptureArtifact, KlypseError>;
}

#[async_trait]
pub trait RecordingBackend: Send + Sync {
    async fn start(&self, request: &RecordingRequest) -> Result<Uuid, KlypseError>;
    async fn stop(&self, session_id: Uuid) -> Result<CaptureArtifact, KlypseError>;
}

#[async_trait]
pub trait HotkeyBackend: Send + Sync {
    async fn bind(&self, actions: &[HotkeyAction]) -> Result<(), KlypseError>;
}
