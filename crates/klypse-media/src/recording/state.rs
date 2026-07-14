use uuid::Uuid;

use crate::MediaError;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RecordingState {
    #[default]
    Idle,
    Selecting,
    Recording,
    Finalizing,
    Failed,
}

#[derive(Clone, Debug, Default)]
pub struct RecordingMachine {
    state: RecordingState,
    session_id: Option<Uuid>,
    failure: Option<String>,
}

impl RecordingMachine {
    pub fn idle() -> Self {
        Self::default()
    }

    pub const fn state(&self) -> RecordingState {
        self.state
    }

    pub const fn session_id(&self) -> Option<Uuid> {
        self.session_id
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn begin_selection(&mut self) -> Result<(), MediaError> {
        self.transition(RecordingState::Idle, RecordingState::Selecting)
    }

    pub fn cancel_selection(&mut self) -> Result<(), MediaError> {
        self.transition(RecordingState::Selecting, RecordingState::Idle)
    }

    pub fn start(&mut self, session_id: Uuid) -> Result<(), MediaError> {
        self.transition(RecordingState::Selecting, RecordingState::Recording)?;
        self.session_id = Some(session_id);
        Ok(())
    }

    pub fn begin_finalization(&mut self) -> Result<(), MediaError> {
        self.transition(RecordingState::Recording, RecordingState::Finalizing)
    }

    pub fn finish(&mut self) -> Result<(), MediaError> {
        self.transition(RecordingState::Finalizing, RecordingState::Idle)?;
        self.session_id = None;
        Ok(())
    }

    pub fn fail(&mut self, reason: impl Into<String>) -> Result<(), MediaError> {
        if !matches!(
            self.state,
            RecordingState::Selecting | RecordingState::Recording | RecordingState::Finalizing
        ) {
            return Err(invalid_transition(self.state, RecordingState::Failed));
        }
        self.state = RecordingState::Failed;
        self.failure = Some(reason.into());
        Ok(())
    }

    pub fn acknowledge_failure(&mut self) -> Result<(), MediaError> {
        self.transition(RecordingState::Failed, RecordingState::Idle)?;
        self.session_id = None;
        self.failure = None;
        Ok(())
    }

    fn transition(
        &mut self,
        expected: RecordingState,
        next: RecordingState,
    ) -> Result<(), MediaError> {
        if self.state != expected {
            return Err(invalid_transition(self.state, next));
        }
        self.state = next;
        Ok(())
    }
}

fn invalid_transition(from: RecordingState, to: RecordingState) -> MediaError {
    MediaError::InvalidRecording(format!(
        "recording cannot transition from {from:?} to {to:?}"
    ))
}
