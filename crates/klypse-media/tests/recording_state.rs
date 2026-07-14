use klypse_media::{RecordingMachine, RecordingState};
use uuid::Uuid;

#[test]
fn only_recording_can_transition_to_finalizing() {
    let mut machine = RecordingMachine::idle();
    assert!(machine.begin_finalization().is_err());
    machine.begin_selection().unwrap();
    machine.start(Uuid::new_v4()).unwrap();
    machine.begin_finalization().unwrap();
    assert_eq!(machine.state(), RecordingState::Finalizing);
    machine.finish().unwrap();
    assert_eq!(machine.state(), RecordingState::Idle);
}

#[test]
fn cancellation_and_failures_follow_explicit_recovery_paths() {
    let mut machine = RecordingMachine::idle();
    machine.begin_selection().unwrap();
    machine.cancel_selection().unwrap();
    assert_eq!(machine.state(), RecordingState::Idle);

    machine.begin_selection().unwrap();
    machine.fail("portal denied the request").unwrap();
    assert_eq!(machine.state(), RecordingState::Failed);
    assert_eq!(machine.failure(), Some("portal denied the request"));
    machine.acknowledge_failure().unwrap();
    assert_eq!(machine.state(), RecordingState::Idle);
}

#[test]
fn illegal_transitions_never_mutate_the_current_state() {
    let mut machine = RecordingMachine::idle();
    assert!(machine.start(Uuid::new_v4()).is_err());
    assert!(machine.cancel_selection().is_err());
    assert!(machine.finish().is_err());
    assert_eq!(machine.state(), RecordingState::Idle);
}
