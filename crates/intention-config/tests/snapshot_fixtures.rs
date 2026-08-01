#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Versioned public configuration snapshot fixture evidence.

use intention_config::{ConfigSnapshotDto, ProviderKindDto, ResolvedConfigDto};
use intention_types::{ConfigRevisionId, SchemaVersionDto, TimestampDto};

const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";
const VALID_RESOLVED: &str = r#"{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"source_kind":"explicit"}"#;

#[test]
fn config_snapshot_fixture_decodes_and_round_trips_without_credential() {
    let fixture = include_str!("fixtures/config-snapshot-v1.json");
    let snapshot: ConfigSnapshotDto =
        serde_json::from_str(fixture).expect("config snapshot fixture must decode");

    assert_eq!(snapshot.schema_version().major(), 1);
    assert_eq!(
        snapshot.resolved().provider().kind(),
        ProviderKindDto::Openrouter
    );
    assert_eq!(snapshot.resolved().provider().model(), "example-chat-model");
    assert_eq!(snapshot.captured_at().unix_seconds(), 1_700_000_000);
    assert_eq!(snapshot.resolved().source_kind().as_str(), "explicit");

    let encoded = serde_json::to_string(&snapshot).expect("test serialization must succeed");
    let decoded: ConfigSnapshotDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");
    assert_eq!(decoded, snapshot);
    assert!(!encoded.contains(FAKE_CREDENTIAL));

    let constructed = ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        TimestampDto::from_unix_seconds(1_700_000_001).expect("fixture timestamp is valid"),
        snapshot.resolved().clone(),
    )
    .expect("compatible snapshot schema is valid");
    assert_eq!(constructed.schema_version(), SchemaVersionDto::new(1, 0));
    assert!(
        ConfigSnapshotDto::new(
            SchemaVersionDto::new(2, 0),
            ConfigRevisionId::new(),
            TimestampDto::from_unix_seconds(1_700_000_001).expect("fixture timestamp is valid"),
            snapshot.resolved().clone(),
        )
        .is_err()
    );
}

#[test]
fn malformed_config_snapshot_wire_shapes_are_rejected() {
    for wire in [
        r#"{"schema_version":{"major":1,"minor":0},"revision_id":"not-an-id","captured_at":1700000000,"resolved":{}}"#,
        r#"{"schema_version":{"major":2,"minor":0},"revision_id":"44444444-4444-4444-8444-444444444444","captured_at":1700000000,"resolved":{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"source_kind":"explicit"}}"#,
        r#"{"schema_version":{"major":1,"minor":0},"revision_id":"44444444-4444-4444-8444-444444444444","captured_at":1700000000}"#,
        r#"{"schema_version":{"major":1,"minor":0},"revision_id":"44444444-4444-4444-8444-444444444444","captured_at":1700000000,"resolved":{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openai","model":"fixture","endpoint":null,"credential_configured":true},"source_kind":"explicit"}}"#,
        r#"{"schema_version":{"major":1,"minor":0},"revision_id":"44444444-4444-4444-8444-444444444444","captured_at":1700000000,"resolved":{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openrouter","model":" ","endpoint":null,"credential_configured":true},"source_kind":"explicit"}}"#,
        r#"{"schema_version":{"major":1,"minor":0},"revision_id":"44444444-4444-4444-8444-444444444444","captured_at":1700000000,"resolved":{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openrouter","model":"fixture","endpoint":" ","credential_configured":true},"source_kind":"explicit"}}"#,
        r#"{"schema_version":{"major":1,"minor":0},"revision_id":"44444444-4444-4444-8444-444444444444","captured_at":1700000000,"resolved":{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"source_kind":"explicit"},"unexpected":true}"#,
    ] {
        assert!(serde_json::from_str::<ConfigSnapshotDto>(wire).is_err());
    }
}

#[test]
fn resolved_config_public_contract_is_credential_free_and_closed() {
    let resolved: ResolvedConfigDto =
        serde_json::from_str(VALID_RESOLVED).expect("public resolved config must decode");

    assert_eq!(resolved.provider().kind(), ProviderKindDto::Openrouter);
    assert!(
        !serde_json::to_string(&resolved)
            .expect("test serialization must succeed")
            .contains(FAKE_CREDENTIAL)
    );
    assert!(serde_json::from_str::<ResolvedConfigDto>(
        r#"{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"source_kind":"explicit","unexpected":true}"#
    )
    .is_err());
}
