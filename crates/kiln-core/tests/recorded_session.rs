use kiln_core::{EventEnvelope, TaskProjection};

const RECORDING: &str = include_str!("../../../fixtures/sessions/complete-task-v1.json");
const EXPECTED_PROJECTION: &str =
    include_str!("../../../fixtures/sessions/complete-task-v1.expected.json");

#[test]
fn recorded_session_rebuilds_to_the_versioned_projection_snapshot() {
    let events: Vec<EventEnvelope> =
        serde_json::from_str(RECORDING).expect("recording should deserialize");
    let projection = TaskProjection::rebuild(&events).expect("recording should project");
    let actual = serde_json::to_value(&projection).expect("projection should serialize");
    let expected: serde_json::Value =
        serde_json::from_str(EXPECTED_PROJECTION).expect("expected projection should deserialize");

    assert_eq!(actual, expected);
    assert_eq!(projection.last_sequence, events.len() as u64);
    assert_eq!(projection.tools.len(), 2);
    assert_eq!(projection.artifacts.len(), 4);
}

#[test]
fn repeated_recorded_session_replay_is_byte_stable() {
    let events: Vec<EventEnvelope> =
        serde_json::from_str(RECORDING).expect("recording should deserialize");
    let first = TaskProjection::rebuild(&events).expect("first replay");
    let second = TaskProjection::rebuild(&events).expect("second replay");

    assert_eq!(
        serde_json::to_vec(&first).expect("first projection"),
        serde_json::to_vec(&second).expect("second projection"),
    );
}
