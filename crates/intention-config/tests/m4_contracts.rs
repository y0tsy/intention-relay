#![allow(
    clippy::expect_used,
    reason = "M4 contract fixtures use expect to provide precise test failure messages."
)]

//! Resolved-value, startup-material, and public snapshot fixture contracts.

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{FAKE_CREDENTIAL, explicit_source};

use intention_config::{ConfigSnapshotDto, ProviderKindDto, RawConfigInputDto, ResolvedConfigDto};
use intention_proto::{ConfigRevisionId, SchemaVersionDto, TimestampDto};

const VALID_RESOLVED: &str = r#"{"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"provider_execution":{"attempt_timeout_seconds":30,"max_attempts":2},"context_window":{"window_tokens":250000},"source_kind":"explicit"}"#;

fn resolve(execution: &str) -> ResolvedConfigDto {
    ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n{execution}"
        ),
        explicit_source(),
    ))
    .expect("fixture configuration resolves")
}

#[test]
fn execution_policy_defaults_and_overrides_are_safe_snapshot_data() {
    let defaulted = resolve("");
    assert_eq!(defaulted.provider_execution().attempt_timeout_seconds(), 30);
    assert_eq!(defaulted.provider_execution().max_attempts(), 2);

    let overridden =
        resolve("[provider.execution]\nattempt_timeout_seconds = 60\nmax_attempts = 2\n");
    assert_eq!(
        overridden.provider_execution().attempt_timeout_seconds(),
        60
    );
    assert_eq!(overridden.provider_execution().max_attempts(), 2);

    let encoded = serde_json::to_string(&overridden).expect("safe projection serializes");
    assert!(!encoded.contains(FAKE_CREDENTIAL));
}

#[test]
fn provider_policy_rejects_out_of_range_values_without_redacting_errors() {
    for (text, code) in [
        (
            "[provider.execution]\nattempt_timeout_seconds = 0\n",
            "invalid_provider_attempt_timeout_seconds",
        ),
        (
            "[provider.execution]\nattempt_timeout_seconds = 61\n",
            "invalid_provider_attempt_timeout_seconds",
        ),
        (
            "[provider.execution]\nmax_attempts = 0\n",
            "invalid_provider_max_attempts",
        ),
        (
            "[provider.execution]\nmax_attempts = 3\n",
            "invalid_provider_max_attempts",
        ),
        (
            "context_window_tokens = 0\n",
            "invalid_provider_context_window_tokens",
        ),
    ] {
        let error = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            format!(
                "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n{text}"
            ),
            explicit_source(),
        ))
        .expect_err("out-of-range provider policy must fail");
        assert_eq!(error.code(), code);
        assert!(!error.to_string().contains(FAKE_CREDENTIAL));
    }
}

#[test]
fn context_window_policy_defaults_and_overrides_are_safe_snapshot_data() {
    let defaulted = resolve("");
    assert_eq!(defaulted.context_window().window_tokens(), 250_000);

    let overridden = resolve("context_window_tokens = 1000\n");
    assert_eq!(overridden.context_window().window_tokens(), 1_000);

    let encoded = serde_json::to_string(&overridden).expect("safe projection serializes");
    assert!(encoded.contains("\"context_window\""));
    assert!(!encoded.contains(FAKE_CREDENTIAL));
}

#[test]
fn startup_material_is_opaque_and_safe_projection_excludes_credential() {
    let material = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"
        ),
        explicit_source(),
    ))
    .expect("startup material resolves");
    let resolved = material.safe_resolved();
    let encoded = serde_json::to_string(resolved).expect("safe resolved config serializes");
    assert!(!encoded.contains(FAKE_CREDENTIAL));
    assert_eq!(resolved.provider().model(), "fixture");
}

#[test]
fn startup_material_preserves_current_selection_only_for_provider_construction() {
    let material = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"
        ),
        explicit_source(),
    ))
    .expect("current-shape startup material resolves");

    let (resolved, credential) =
        material.into_parts_for_provider(|resolved, credential| (resolved, credential));
    assert_eq!(
        resolved.provider().kind().as_str(),
        "generic-chat-completion-api"
    );
    assert_eq!(resolved.provider().model(), "fixture");
    assert_eq!(credential, FAKE_CREDENTIAL);
}

#[test]
fn startup_material_returns_safe_errors_before_provider_construction() {
    let result = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \" \"\n",
        explicit_source(),
    ));
    let error = result
        .err()
        .expect("invalid startup material must return an error");
    assert_eq!(error.code(), "missing_provider_credential");
    assert_eq!(error.category().as_str(), "validation");
}

#[test]
fn config_snapshot_fixture_decodes_and_rejects_a_foreign_schema() {
    let fixture = include_str!("fixtures/config-snapshot-v1.json");
    let snapshot: ConfigSnapshotDto =
        serde_json::from_str(fixture).expect("config snapshot fixture must decode");

    assert_eq!(
        snapshot.resolved().provider().kind(),
        ProviderKindDto::Openrouter
    );
    assert_eq!(snapshot.resolved().provider().model(), "example-chat-model");

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
        r#"{"schema_version":{"major":2,"minor":0},"revision_id":"44444444-4444-4444-8444-444444444444","captured_at":1700000000,"resolved":{"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"provider_execution":{"attempt_timeout_seconds":30,"max_attempts":2},"context_window":{"window_tokens":250000},"source_kind":"explicit"}}"#,
        r#"{"schema_version":{"major":1,"minor":0},"revision_id":"44444444-4444-4444-8444-444444444444","captured_at":1700000000,"resolved":{}}"#,
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
        serde_json::from_str::<ResolvedConfigDto>(
            r#"{"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"provider_execution":{"attempt_timeout_seconds":30,"max_attempts":2},"context_window":{"window_tokens":250000},"source_kind":"explicit","unexpected":true}"#
        )
        .is_err()
    );
}
