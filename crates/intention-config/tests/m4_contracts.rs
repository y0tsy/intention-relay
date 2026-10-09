#![allow(
    clippy::expect_used,
    reason = "M4 contract fixtures use expect to provide precise test failure messages."
)]

//! Configuration snapshot wire contracts: the schema gate, the closed shape,
//! and the committed context-window policy.

use intention_config::{ConfigSnapshotDto, ContextWindowPolicyDto};
use intention_proto::{ConfigRevisionId, SchemaVersionDto, TimestampDto};

const CURRENT_SCHEMA_WIRE: &str = r#"{"major":1,"minor":0}"#;
const FOREIGN_SCHEMA_WIRE: &str = r#"{"major":2,"minor":0}"#;
const VALID_REVISION_ID: &str = "44444444-4444-4444-8444-444444444444";
const VALID_WINDOW_WIRE: &str = r#"{"window_tokens":250000}"#;

/// Builds one snapshot wire from its schema version, revision identity, and
/// window policy, so each case changes exactly one fragment.
fn snapshot_wire(schema_version: &str, revision_id: &str, window: &str) -> String {
    format!(
        r#"{{"schema_version":{schema_version},"revision_id":"{revision_id}","captured_at":1700000000,"context_window":{window}}}"#
    )
}

fn fixture_timestamp() -> TimestampDto {
    TimestampDto::from_unix_seconds(1_700_000_001).expect("fixture timestamp is valid")
}

#[test]
fn config_snapshot_fixture_decodes_and_rejects_a_foreign_schema() {
    let fixture = include_str!("fixtures/config-snapshot-v1.json");
    let snapshot: ConfigSnapshotDto =
        serde_json::from_str(fixture).expect("config snapshot fixture must decode");
    assert_eq!(
        snapshot.schema_version(),
        SchemaVersionDto::new(1, 0),
        "the fixture carries the current snapshot schema"
    );
    assert_eq!(
        snapshot.context_window().window_tokens(),
        250_000,
        "the fixture carries its committed window policy"
    );

    let constructed = ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        fixture_timestamp(),
        *snapshot.context_window(),
    )
    .expect("compatible snapshot schema is valid");
    assert_eq!(constructed.schema_version(), SchemaVersionDto::new(1, 0));
    assert!(
        ConfigSnapshotDto::new(
            SchemaVersionDto::new(2, 0),
            ConfigRevisionId::new(),
            fixture_timestamp(),
            *snapshot.context_window(),
        )
        .is_err()
    );
}

#[test]
fn malformed_config_snapshot_wire_shapes_are_rejected() {
    // The baseline must decode, otherwise every case below passes vacuously.
    assert!(
        serde_json::from_str::<ConfigSnapshotDto>(&snapshot_wire(
            CURRENT_SCHEMA_WIRE,
            VALID_REVISION_ID,
            VALID_WINDOW_WIRE,
        ))
        .is_ok()
    );

    for (label, wire) in [
        (
            "foreign schema version",
            snapshot_wire(FOREIGN_SCHEMA_WIRE, VALID_REVISION_ID, VALID_WINDOW_WIRE),
        ),
        (
            "non-canonical revision identity",
            snapshot_wire(CURRENT_SCHEMA_WIRE, "not-an-id", VALID_WINDOW_WIRE),
        ),
        (
            "unknown snapshot field",
            format!(
                r#"{{"schema_version":{CURRENT_SCHEMA_WIRE},"revision_id":"{VALID_REVISION_ID}","captured_at":1700000000,"context_window":{VALID_WINDOW_WIRE},"unexpected":true}}"#
            ),
        ),
        (
            "missing window policy",
            snapshot_wire(CURRENT_SCHEMA_WIRE, VALID_REVISION_ID, "{}"),
        ),
        (
            "unknown window field",
            snapshot_wire(
                CURRENT_SCHEMA_WIRE,
                VALID_REVISION_ID,
                r#"{"window_tokens":250000,"unexpected":true}"#,
            ),
        ),
        (
            "zero context window",
            snapshot_wire(
                CURRENT_SCHEMA_WIRE,
                VALID_REVISION_ID,
                r#"{"window_tokens":0}"#,
            ),
        ),
    ] {
        assert!(
            serde_json::from_str::<ConfigSnapshotDto>(&wire).is_err(),
            "snapshot wire must be rejected: {label}"
        );
    }
}

#[test]
fn context_window_policy_validates_through_the_snapshot_constructor() {
    let window = ContextWindowPolicyDto::new(180_000).expect("a positive window is valid");
    let snapshot = ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        fixture_timestamp(),
        window,
    )
    .expect("a validated window commits into a snapshot");
    assert_eq!(snapshot.context_window().window_tokens(), 180_000);
    assert_eq!(
        ContextWindowPolicyDto::default_policy().window_tokens(),
        250_000
    );
    assert_eq!(
        ContextWindowPolicyDto::new(0)
            .expect_err("a zero window rejects")
            .code(),
        "invalid_provider_context_window_tokens"
    );
}
