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

const CURRENT_SCHEMA_WIRE: &str = r#"{"major":1,"minor":0}"#;
const FOREIGN_SCHEMA_WIRE: &str = r#"{"major":2,"minor":0}"#;
const VALID_REVISION_ID: &str = "44444444-4444-4444-8444-444444444444";

/// Builds one snapshot wire from its schema version, revision identity, and
/// resolved projection, so each case changes exactly one fragment.
fn snapshot_wire(schema_version: &str, revision_id: &str, resolved: &str) -> String {
    format!(
        r#"{{"schema_version":{schema_version},"revision_id":"{revision_id}","captured_at":1700000000,"resolved":{resolved}}}"#
    )
}

/// Builds one resolved projection from its three policy fragments.
fn resolved_wire(provider: &str, provider_execution: &str, context_window: &str) -> String {
    format!(
        r#"{{"provider":{provider},"provider_execution":{provider_execution},"context_window":{context_window},"source_kind":"explicit"}}"#
    )
}

/// Builds one credential-free provider selection fragment.
fn provider_wire(kind: &str, model: &str, endpoint: &str) -> String {
    format!(
        r#"{{"kind":"{kind}","model":"{model}","endpoint":{endpoint},"credential_configured":true}}"#
    )
}

/// Builds one provider execution policy fragment.
fn execution_wire(attempt_timeout_seconds: u32, max_attempts: u32) -> String {
    format!(
        r#"{{"attempt_timeout_seconds":{attempt_timeout_seconds},"max_attempts":{max_attempts}}}"#
    )
}

/// Builds one context-window policy fragment.
fn context_window_wire(window_tokens: u64) -> String {
    format!(r#"{{"window_tokens":{window_tokens}}}"#)
}

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
    // The baseline must decode, otherwise every case below passes vacuously.
    assert!(
        serde_json::from_str::<ConfigSnapshotDto>(&snapshot_wire(
            CURRENT_SCHEMA_WIRE,
            VALID_REVISION_ID,
            VALID_RESOLVED,
        ))
        .is_ok()
    );

    let value_case = |provider: &str, execution: &str, window: &str| {
        snapshot_wire(
            CURRENT_SCHEMA_WIRE,
            VALID_REVISION_ID,
            &resolved_wire(provider, execution, window),
        )
    };
    let openrouter = provider_wire("openrouter", "fixture", "null");

    for (label, wire) in [
        (
            "foreign schema version",
            snapshot_wire(FOREIGN_SCHEMA_WIRE, VALID_REVISION_ID, VALID_RESOLVED),
        ),
        (
            "non-canonical revision identity",
            snapshot_wire(CURRENT_SCHEMA_WIRE, "not-an-id", VALID_RESOLVED),
        ),
        (
            "unknown snapshot field",
            format!(
                r#"{{"schema_version":{CURRENT_SCHEMA_WIRE},"revision_id":"{VALID_REVISION_ID}","captured_at":1700000000,"resolved":{VALID_RESOLVED},"unexpected":true}}"#
            ),
        ),
        (
            "empty resolved projection",
            snapshot_wire(CURRENT_SCHEMA_WIRE, VALID_REVISION_ID, "{}"),
        ),
        (
            "unsupported provider kind",
            value_case(
                &provider_wire("openai", "fixture", "null"),
                &execution_wire(30, 2),
                &context_window_wire(250_000),
            ),
        ),
        (
            "blank provider model",
            value_case(
                &provider_wire("openrouter", " ", "null"),
                &execution_wire(30, 2),
                &context_window_wire(250_000),
            ),
        ),
        (
            "blank provider endpoint",
            value_case(
                &provider_wire("openrouter", "fixture", "\" \""),
                &execution_wire(30, 2),
                &context_window_wire(250_000),
            ),
        ),
        (
            "zero attempt timeout",
            value_case(
                &openrouter,
                &execution_wire(0, 2),
                &context_window_wire(250_000),
            ),
        ),
        (
            "over-maximum attempt timeout",
            value_case(
                &openrouter,
                &execution_wire(61, 2),
                &context_window_wire(250_000),
            ),
        ),
        (
            "zero attempts",
            value_case(
                &openrouter,
                &execution_wire(30, 0),
                &context_window_wire(250_000),
            ),
        ),
        (
            "over-maximum attempts",
            value_case(
                &openrouter,
                &execution_wire(30, 3),
                &context_window_wire(250_000),
            ),
        ),
        (
            "zero context window",
            value_case(&openrouter, &execution_wire(30, 2), &context_window_wire(0)),
        ),
    ] {
        assert!(
            serde_json::from_str::<ConfigSnapshotDto>(&wire).is_err(),
            "snapshot wire must be rejected: {label}"
        );
    }
}

#[test]
fn decoded_resolved_config_rejects_value_violations() {
    let openrouter = provider_wire("openrouter", "fixture", "null");
    for (label, resolved, expected_code) in [
        (
            "blank provider model",
            resolved_wire(
                &provider_wire("openrouter", " ", "null"),
                &execution_wire(30, 2),
                &context_window_wire(250_000),
            ),
            "invalid_provider_model",
        ),
        (
            "blank provider endpoint",
            resolved_wire(
                &provider_wire("openrouter", "fixture", "\" \""),
                &execution_wire(30, 2),
                &context_window_wire(250_000),
            ),
            "invalid_provider_endpoint",
        ),
        (
            "zero attempt timeout",
            resolved_wire(
                &openrouter,
                &execution_wire(0, 2),
                &context_window_wire(250_000),
            ),
            "invalid_provider_attempt_timeout_seconds",
        ),
        (
            "over-maximum attempt timeout",
            resolved_wire(
                &openrouter,
                &execution_wire(61, 2),
                &context_window_wire(250_000),
            ),
            "invalid_provider_attempt_timeout_seconds",
        ),
        (
            "zero attempts",
            resolved_wire(
                &openrouter,
                &execution_wire(30, 0),
                &context_window_wire(250_000),
            ),
            "invalid_provider_max_attempts",
        ),
        (
            "over-maximum attempts",
            resolved_wire(
                &openrouter,
                &execution_wire(30, 3),
                &context_window_wire(250_000),
            ),
            "invalid_provider_max_attempts",
        ),
        (
            "zero context window",
            resolved_wire(&openrouter, &execution_wire(30, 2), &context_window_wire(0)),
            "invalid_provider_context_window_tokens",
        ),
    ] {
        let error = serde_json::from_str::<ResolvedConfigDto>(&resolved)
            .expect_err("a value violation must fail the decode");
        assert!(
            error.to_string().starts_with(&format!("{expected_code}: ")),
            "unexpected decode failure for {label}: {error}"
        );
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
