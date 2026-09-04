#![allow(
    clippy::expect_used,
    reason = "Slice 2 control-plane fixtures use expect to provide precise test failure messages."
)]

//! Slice 2 configuration control-plane candidate contract evidence.

use intention_config::control_plane::{
    CandidateIssueDto, ConfigCandidateDto, ConfigCandidateSourceDto, MAX_CANDIDATE_ISSUES,
    classify_changed_fields, parse_candidate, reject_catalog_affecting_edits,
    restore_credential_document, semantic_equivalence,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_types::{ConfigRevisionId, SchemaVersionDto, TimestampDto};

const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";
const SECRET_SHAPED_CREDENTIAL: &str = "sk-test-fake-credential-12345";

fn source() -> ConfigSourceDto {
    ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-relay-m5-control-plane.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    )
}

fn v1(
    kind: &str,
    model: &str,
    credential: &str,
    endpoint: Option<&str>,
    execution: &str,
) -> String {
    let endpoint = endpoint.map_or_else(String::new, |value| format!("endpoint = \"{value}\"\n"));
    format!(
        "schema_version = 1\n[provider]\nkind = \"{kind}\"\nmodel = \"{model}\"\ncredential = \"{credential}\"\n{endpoint}{execution}"
    )
}

fn resolved(text: &str) -> ResolvedConfigDto {
    ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(text.to_owned(), source()))
        .expect("fixture configuration resolves")
}

fn snapshot(text: &str, revision: &str) -> ConfigSnapshotDto {
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::parse(revision).expect("fixture revision is a canonical UUID"),
        TimestampDto::from_unix_seconds(1_700_000_000).expect("fixture timestamp is valid"),
        resolved(text),
    )
    .expect("fixture snapshot schema is compatible")
}

fn candidate(text: &str, previous: &ConfigSnapshotDto) -> ConfigCandidateDto {
    parse_candidate(RawConfigInputDto::new(text.to_owned(), source()), previous)
        .expect("fixture candidate parses")
}

#[test]
fn valid_candidate_produces_a_new_revision_distinct_from_previous() {
    let text = v1("openrouter", "fixture-model", FAKE_CREDENTIAL, None, "");
    let previous = snapshot(&text, "11111111-1111-4111-8111-111111111111");
    let parsed = candidate(&text, &previous);

    assert_ne!(
        parsed.candidate_revision_id(),
        previous.revision_id().to_string(),
        "candidate revision must be new"
    );
    assert_eq!(
        parsed.safe_snapshot().revision_id().to_string(),
        parsed.candidate_revision_id(),
        "the candidate revision is the snapshot revision"
    );
    assert!(
        parsed.validation().issues().is_empty(),
        "a valid candidate has no issues"
    );
    assert!(
        !parsed.validation().truncated(),
        "a valid candidate is not truncated"
    );
    assert_eq!(parsed.validation().total_issue_count(), 0);
    assert!(
        matches!(
            parsed.source(),
            ConfigCandidateSourceDto::RawToml { size_bytes } if size_bytes > 0
        ),
        "the raw source carries only its size"
    );
    assert_eq!(parsed.source().as_str(), "raw_toml");
    assert!(
        semantic_equivalence(parsed.safe_snapshot(), &previous),
        "an unchanged candidate is semantically equivalent"
    );

    let encoded = serde_json::to_string(&parsed).expect("candidate serializes");
    assert!(
        !encoded.contains(FAKE_CREDENTIAL),
        "the candidate wire never contains the credential"
    );
}

#[test]
fn whitespace_and_key_order_only_changes_are_semantically_equal() {
    let text_a = v1(
        "openrouter",
        "fixture-model",
        FAKE_CREDENTIAL,
        Some("https://example.invalid/v1"),
        "[provider.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n",
    );
    let text_b = "schema_version=1\n[provider.execution]\nmax_attempts=2\nattempt_timeout_seconds=45\n[provider]\nmodel=\"fixture-model\"\nendpoint=\"https://example.invalid/v1\"\nkind=\"openrouter\"\ncredential=\"fixture-credential-not-real-12345\"\n";
    let previous = snapshot(&text_a, "22222222-2222-4222-8222-222222222222");
    let parsed = candidate(text_b, &previous);

    assert!(
        semantic_equivalence(parsed.safe_snapshot(), &previous),
        "whitespace and key order changes are semantically equal"
    );
    let categories = classify_changed_fields(parsed.safe_snapshot(), &previous);
    assert!(
        categories.is_empty() || categories.iter().all(|category| category == "display"),
        "only display metadata may differ"
    );
}

#[test]
fn credential_only_replacement_is_semantically_equal() {
    let previous = snapshot(
        &v1(
            "openrouter",
            "fixture-model",
            "first-credential-0001",
            None,
            "",
        ),
        "33333333-3333-4333-8333-333333333333",
    );
    let parsed = candidate(
        &v1(
            "openrouter",
            "fixture-model",
            "second-credential-0002",
            None,
            "",
        ),
        &previous,
    );

    assert!(
        semantic_equivalence(parsed.safe_snapshot(), &previous),
        "credential replacement does not change semantics"
    );
    assert!(
        classify_changed_fields(parsed.safe_snapshot(), &previous)
            .iter()
            .all(|category| category == "display"),
        "no semantic category changes"
    );
}

#[test]
fn model_endpoint_kind_and_policy_changes_are_semantic_with_correct_categories() {
    let base = v1(
        "openrouter",
        "fixture-model",
        FAKE_CREDENTIAL,
        Some("https://example.invalid/v1"),
        "[provider.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n",
    );
    let previous = snapshot(&base, "44444444-4444-4444-8444-444444444444");
    let fixtures = [
        (
            v1(
                "openrouter",
                "changed-model",
                FAKE_CREDENTIAL,
                Some("https://example.invalid/v1"),
                "[provider.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n",
            ),
            vec!["model", "display"],
        ),
        (
            v1(
                "openrouter",
                "fixture-model",
                FAKE_CREDENTIAL,
                Some("https://other.invalid/v1"),
                "[provider.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n",
            ),
            vec!["endpoint", "display"],
        ),
        (
            v1(
                "generic-chat-completion-api",
                "fixture-model",
                FAKE_CREDENTIAL,
                Some("https://example.invalid/v1"),
                "[provider.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n",
            ),
            vec!["provider_kind", "display"],
        ),
        (
            v1(
                "openrouter",
                "fixture-model",
                FAKE_CREDENTIAL,
                Some("https://example.invalid/v1"),
                "[provider.execution]\nattempt_timeout_seconds = 60\nmax_attempts = 2\n",
            ),
            vec!["execution_policy", "display"],
        ),
    ];
    for (text, expected_categories) in fixtures {
        let parsed = candidate(&text, &previous);
        assert!(
            !semantic_equivalence(parsed.safe_snapshot(), &previous),
            "changed semantics must not compare equal: {expected_categories:?}"
        );
        assert_eq!(
            classify_changed_fields(parsed.safe_snapshot(), &previous),
            expected_categories,
            "changed field categories"
        );
    }
}

#[test]
fn raw_content_over_512_kib_is_rejected() {
    let previous = snapshot(
        &v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        "55555555-5555-4555-8555-555555555555",
    );
    let mut text = v1("openrouter", "fixture", FAKE_CREDENTIAL, None, "");
    text.push_str(&"# padding comment line to exceed the reload size limit\n".repeat(20_000));
    assert!(
        text.len() > 512 * 1024,
        "fixture must exceed the 512 KiB limit"
    );

    let error = parse_candidate(RawConfigInputDto::new(text, source()), &previous)
        .expect_err("oversized candidate must fail");
    assert_eq!(error.code(), "candidate_too_large");
    assert_eq!(error.category().as_str(), "validation");
    assert!(
        !error.to_string().contains(FAKE_CREDENTIAL),
        "the error never echoes raw content"
    );
}

#[test]
fn credential_shaped_raw_content_is_rejected_without_echoing_content() {
    let previous = snapshot(
        &v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        "66666666-6666-4666-8666-666666666666",
    );
    let fixtures = [
        // A bearer token smuggled into an unknown provider field.
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\ntoken = \"Bearer secret-token-123\"\n",
        // An api_key duplicate in a v1 document.
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\napi_key = \"sk-test-123\"\n",
        // A password assignment in an unversioned document.
        "[model]\nprovider = \"openrouter\"\nname = \"fixture\"\napi_key = \"fixture-credential-not-real-12345\"\npassword = \"hunter2\"\n",
        // A credential-shaped value hidden inside a model identifier.
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"sk-test-model\"\ncredential = \"fixture-credential-not-real-12345\"\n",
    ];
    for text in fixtures {
        let error = parse_candidate(RawConfigInputDto::new(text.to_owned(), source()), &previous)
            .expect_err("credential-shaped content must fail");
        assert_eq!(
            error.code(),
            "credentials_forbidden",
            "typed rejection code"
        );
        assert_eq!(error.category().as_str(), "policy");
        for secret in ["sk-test", "Bearer", "hunter2"] {
            assert!(
                !error.to_string().contains(secret),
                "the error never echoes the offending content"
            );
        }
    }
}

#[test]
fn validation_issues_are_bounded_at_32_with_total_and_truncation() {
    let previous = snapshot(
        &v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        "77777777-7777-4777-8777-777777777777",
    );
    let mut text = String::from(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n",
    );
    for index in 0..MAX_CANDIDATE_ISSUES + 1 {
        text.push_str(&format!("unknown_field_{index} = 1\n"));
    }

    let parsed = candidate(&text, &previous);

    assert_eq!(
        parsed.validation().issues().len(),
        MAX_CANDIDATE_ISSUES,
        "issues are bounded at 32"
    );
    assert!(
        parsed.validation().truncated(),
        "overflow is flagged as truncated"
    );
    assert_eq!(
        parsed.validation().total_issue_count(),
        MAX_CANDIDATE_ISSUES as u32 + 1,
        "the total issue count is preserved"
    );
    assert_eq!(
        parsed.safe_snapshot(),
        &previous,
        "failed validation leaves the active snapshot unchanged"
    );
    for issue in parsed.validation().issues() {
        assert_eq!(issue.code(), "unknown_config_field");
        assert!(issue.field().is_some(), "unknown fields carry a path");
        assert!(
            !issue.message().contains(FAKE_CREDENTIAL),
            "issue messages never echo content"
        );
    }
}

#[test]
fn catalog_affecting_edits_are_rejected_and_benign_edits_pass() {
    let base = v1(
        "openrouter",
        "fixture-model",
        FAKE_CREDENTIAL,
        Some("https://example.invalid/v1"),
        "",
    );
    let previous = snapshot(&base, "88888888-8888-4888-8888-888888888888");

    let unchanged = candidate(&base, &previous);
    assert!(
        reject_catalog_affecting_edits(&unchanged, &previous).is_ok(),
        "an unchanged candidate is benign"
    );

    // Execution-policy-only changes remain reloadable.
    let policy_only = candidate(
        &v1(
            "openrouter",
            "fixture-model",
            FAKE_CREDENTIAL,
            Some("https://example.invalid/v1"),
            "[provider.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n",
        ),
        &previous,
    );
    assert!(
        reject_catalog_affecting_edits(&policy_only, &previous).is_ok(),
        "execution-policy changes are reloadable"
    );

    // Model and endpoint changes are catalog-affecting because both fields
    // hash into the profile revision and the resolved selection of the active
    // catalog profile; they require a restart until catalog replacement can
    // atomically advance both authorities (PR24-008).
    let model_change = candidate(
        &v1(
            "openrouter",
            "other-model",
            FAKE_CREDENTIAL,
            Some("https://example.invalid/v1"),
            "",
        ),
        &previous,
    );
    let model_error = reject_catalog_affecting_edits(&model_change, &previous)
        .expect_err("model changes are catalog-affecting");
    assert_eq!(model_error.code(), "catalog_change_requires_restart");
    assert_eq!(model_error.category().as_str(), "policy");

    let endpoint_change = candidate(
        &v1(
            "openrouter",
            "fixture-model",
            FAKE_CREDENTIAL,
            Some("https://other.invalid/v1"),
            "",
        ),
        &previous,
    );
    let endpoint_error = reject_catalog_affecting_edits(&endpoint_change, &previous)
        .expect_err("endpoint changes are catalog-affecting");
    assert_eq!(endpoint_error.code(), "catalog_change_requires_restart");
    assert_eq!(endpoint_error.category().as_str(), "policy");

    let kind_change = candidate(
        &v1(
            "generic-chat-completion-api",
            "fixture-model",
            FAKE_CREDENTIAL,
            Some("https://example.invalid/v1"),
            "",
        ),
        &previous,
    );
    let error = reject_catalog_affecting_edits(&kind_change, &previous)
        .expect_err("provider kind changes are catalog-affecting");
    assert_eq!(error.code(), "catalog_change_requires_restart");
    assert_eq!(error.category().as_str(), "policy");
}

#[test]
fn removed_digest_and_acceptance_outcome_surfaces_have_no_live_consumers() {
    // PR24-037/038: the private SHA-256 module and the dead
    // CandidateAcceptanceOutcomeDto/redacted_safe_digest surfaces were removed
    // because no production surface consumes them. Semantic comparison and
    // classification functions that production does consume remain available.
    let text = v1("openrouter", "fixture", FAKE_CREDENTIAL, None, "");
    let previous = snapshot(&text, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaab");
    let parsed = candidate(&text, &previous);
    assert!(
        semantic_equivalence(parsed.safe_snapshot(), &previous),
        "semantic comparison remains available"
    );
    assert!(
        classify_changed_fields(parsed.safe_snapshot(), &previous)
            .iter()
            .all(|category| category == "display"),
        "field classification remains available"
    );
}

#[test]
fn fake_secret_sweep_covers_candidate_serialization_and_validation() {
    const FAKE_SECRETS: [&str; 5] = ["sk-test", "Bearer secret", "api_key", "token=", "password="];
    let text = v1(
        "openrouter",
        "fixture-model",
        SECRET_SHAPED_CREDENTIAL,
        None,
        "",
    );
    let previous = snapshot(&text, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let parsed = candidate(&text, &previous);

    let payloads = [
        serde_json::to_string(&parsed).expect("candidate serializes"),
        serde_json::to_string(parsed.validation()).expect("validation summary serializes"),
        format!("{parsed:?}"),
    ];
    for payload in payloads {
        assert!(
            !payload.contains(SECRET_SHAPED_CREDENTIAL),
            "the credential value never leaks"
        );
        for secret in FAKE_SECRETS {
            assert!(
                !payload.contains(secret),
                "the credential-shaped marker {secret:?} never leaks"
            );
        }
    }
}

#[test]
fn candidate_wire_shapes_round_trip_without_unknown_fields() {
    let text = v1("openrouter", "fixture-model", FAKE_CREDENTIAL, None, "");
    let previous = snapshot(&text, "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    let parsed = candidate(&text, &previous);

    let encoded = serde_json::to_string(&parsed).expect("candidate serializes");
    let decoded: ConfigCandidateDto = serde_json::from_str(&encoded).expect("candidate decodes");
    assert_eq!(decoded, parsed);

    let issue: CandidateIssueDto =
        serde_json::from_str(r#"{"code":"unknown_config_field","field":"provider.extra","message":"unrecognized configuration field"}"#)
            .expect("issue decodes");
    assert_eq!(issue.code(), "unknown_config_field");
    assert_eq!(issue.field(), Some("provider.extra"));
    assert!(
        serde_json::from_str::<ConfigCandidateDto>(
            r#"{"candidate_revision_id":"","source":{"raw_toml":{"size_bytes":0}},"safe_snapshot":{"schema_version":{"major":1,"minor":0},"revision_id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","captured_at":1700000000,"resolved":{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"provider_execution":{"attempt_timeout_seconds":30,"max_attempts":2},"source_kind":"explicit"}},"validation":{"issues":[],"truncated":false,"total_issue_count":0}}"#
        )
        .is_err(),
        "an empty candidate revision id is rejected"
    );
    assert!(serde_json::from_str::<ConfigCandidateDto>(
        r#"{"candidate_revision_id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","source":{"raw_toml":{"size_bytes":0}},"safe_snapshot":{"schema_version":{"major":1,"minor":0},"revision_id":"cccccccc-cccc-4ccc-8ccc-cccccccccccc","captured_at":1700000000,"resolved":{"schema_version":{"major":1,"minor":0},"provider":{"kind":"openrouter","model":"fixture","endpoint":null,"credential_configured":true},"provider_execution":{"attempt_timeout_seconds":30,"max_attempts":2},"source_kind":"explicit"}},"validation":{"issues":[],"truncated":false,"total_issue_count":0},"unexpected":true}"#
    )
    .is_err());
}

#[test]
fn malformed_candidate_toml_collects_a_typed_issue_without_advancing() {
    let previous = snapshot(
        &v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbc",
    );
    let parsed = candidate("schema_version = [", &previous);

    assert!(
        parsed
            .validation()
            .issues()
            .iter()
            .any(|issue| issue.code() == "invalid_config_toml"),
        "malformed TOML reports a parse issue"
    );
    assert_eq!(
        parsed.safe_snapshot(),
        &previous,
        "a malformed candidate never advances the active snapshot"
    );
}

#[test]
fn source_kind_changes_classify_as_other_and_compare_different() {
    let explicit = snapshot(
        &v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbd",
    );
    let platform_resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        ConfigSourceDto::PlatformDefault(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-relay-m5-platform.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        ),
    ))
    .expect("fixture configuration resolves");
    let platform = ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::parse("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbe")
            .expect("fixture revision is a canonical UUID"),
        TimestampDto::from_unix_seconds(1_700_000_000).expect("fixture timestamp is valid"),
        platform_resolved,
    )
    .expect("fixture snapshot schema is compatible");

    assert!(
        !semantic_equivalence(&platform, &explicit),
        "a source kind difference is semantic"
    );
    assert_eq!(
        classify_changed_fields(&platform, &explicit),
        vec!["other".to_owned()],
        "source kind changes classify as other"
    );
}

#[test]
fn unversioned_candidate_fails_closed_without_disclosing_credentials() {
    let previous = snapshot(
        &v1("openrouter", "fixture-model", FAKE_CREDENTIAL, None, ""),
        "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
    );
    // An unversioned document without credential-shaped content surfaces the
    // missing schema version as a validation issue and never advances the
    // active snapshot.
    let unversioned = "[model]\nprovider = \"openrouter\"\nname = \"fixture-model\"\n";
    let parsed = candidate(unversioned, &previous);

    assert!(
        parsed
            .validation()
            .issues()
            .iter()
            .any(|issue| issue.code() == "invalid_config_schema"
                && issue.field() == Some("schema_version")),
        "an unversioned candidate reports the missing schema version"
    );
    assert_eq!(
        parsed.safe_snapshot(),
        &previous,
        "an invalid candidate never advances the active snapshot"
    );
    let encoded = serde_json::to_string(&parsed).expect("candidate serializes");
    assert!(
        !encoded.contains(FAKE_CREDENTIAL),
        "the rejected candidate never leaks credential material"
    );

    // An unversioned document carrying a credential-shaped `model.api_key` key
    // is rejected outright without echoing content.
    let with_api_key = "[model]\nprovider = \"openrouter\"\nname = \"fixture-model\"\napi_key = \"fixture-credential-not-real-12345\"\n";
    let error = parse_candidate(
        RawConfigInputDto::new(with_api_key.to_owned(), source()),
        &previous,
    )
    .expect_err("a credential-shaped key outside provider.credential must fail");
    assert_eq!(error.code(), "credentials_forbidden");
    assert_eq!(error.category().as_str(), "policy");
    assert!(
        !error.to_string().contains(FAKE_CREDENTIAL),
        "the error never echoes the offending content"
    );
}

#[test]
fn validation_collects_typed_issues_for_malformed_v1_documents() {
    let previous = snapshot(
        &v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
    );
    let cases = [
        (
            "schema_version = 2\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n",
            "unsupported_config_schema_version",
            Some("schema_version"),
        ),
        (
            "schema_version = \"one\"\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n",
            "invalid_config_schema_version",
            Some("schema_version"),
        ),
        (
            "schema_version = 1\n",
            "invalid_config_schema",
            Some("provider"),
        ),
        (
            "schema_version = 1\nprovider = \"openrouter\"\n",
            "invalid_config_schema",
            Some("provider"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"unknown-kind\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n",
            "invalid_provider_kind",
            Some("provider.kind"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"\"\ncredential = \"fixture-credential-not-real-12345\"\n",
            "invalid_provider_model",
            Some("provider.model"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\n",
            "missing_provider_credential",
            Some("provider.credential"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nendpoint = \"\"\n",
            "invalid_provider_endpoint",
            Some("provider.endpoint"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nexecution = \"fast\"\n",
            "invalid_config_schema",
            Some("provider.execution"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n[provider.execution]\nattempt_timeout_seconds = 999\n",
            "invalid_provider_attempt_timeout_seconds",
            Some("provider.execution.attempt_timeout_seconds"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n[provider.execution]\nmax_attempts = 99\n",
            "invalid_provider_max_attempts",
            Some("provider.execution.max_attempts"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n[provider.execution]\nretries = 3\n",
            "unknown_config_field",
            Some("provider.execution.retries"),
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nextra = 1\n",
            "unknown_config_field",
            Some("provider.extra"),
        ),
        (
            "schema_version = 1\nother = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n",
            "unknown_config_field",
            Some("other"),
        ),
    ];
    for (text, expected_code, expected_field) in cases {
        let parsed = candidate(text, &previous);
        assert!(
            parsed
                .validation()
                .issues()
                .iter()
                .any(|issue| issue.code() == expected_code && issue.field() == expected_field),
            "expected issue {expected_code:?} at {expected_field:?} for {text:?}"
        );
        assert_eq!(
            parsed.safe_snapshot(),
            &previous,
            "invalid candidates never advance the active snapshot"
        );
        let encoded = serde_json::to_string(&parsed).expect("candidate serializes");
        assert!(
            !encoded.contains(FAKE_CREDENTIAL),
            "validation issues never leak credential material"
        );
    }
}

#[test]
fn validation_collects_typed_issues_for_unversioned_documents() {
    let previous = snapshot(
        &v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        "ffffffff-ffff-4fff-8fff-ffffffffffff",
    );
    let cases = [
        "",
        "model = \"openrouter\"\n",
        "[model]\nprovider = \"unknown\"\nname = \"fixture\"\n",
        "[model]\nprovider = \"openrouter\"\nname = \"fixture\"\nextra = 1\n",
        "other = 1\n[model]\nprovider = \"openrouter\"\nname = \"fixture\"\n",
    ];
    for text in cases {
        let parsed = candidate(text, &previous);
        assert!(
            parsed
                .validation()
                .issues()
                .iter()
                .any(|issue| issue.code() == "invalid_config_schema"
                    && issue.field() == Some("schema_version")),
            "expected an invalid_config_schema issue for {text:?}"
        );
        assert_eq!(
            parsed.safe_snapshot(),
            &previous,
            "invalid unversioned candidates never advance the active snapshot"
        );
    }
}

#[test]
fn credential_shaped_keys_values_and_arrays_are_rejected() {
    let previous = snapshot(
        &v1("openrouter", "fixture", FAKE_CREDENTIAL, None, ""),
        "11111111-1111-4111-8111-111111111112",
    );
    let cases = [
        // A credential-shaped value hidden inside an array.
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nlabels = [\"ok\", \"sk-test-array\"]\n",
        // Credential-shaped key names beyond the legitimate credential key.
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\ntoken = \"abc\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nbearer = \"abc\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nauthorization = \"abc\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\naccess_token = \"abc\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nauth_token = \"abc\"\n",
        // Credential-shaped values in arbitrary fields.
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nheaders = \"Bearer abc\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nmeta = \"password123\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nmeta = \"key=secret\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nmeta = \"auth=secret\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nmeta = \"apikey123\"\n",
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\nmeta = \"mysecret\"\n",
        // Credential-shaped key names in an unversioned document are also
        // forbidden; `api_key` is legitimate only as `provider.credential`.
        "[model]\nprovider = \"openrouter\"\nname = \"fixture\"\napi_key = \"fixture-credential-not-real-12345\"\n",
    ];
    for text in cases {
        let error = parse_candidate(
            RawConfigInputDto::new(text.to_string(), source()),
            &previous,
        )
        .expect_err("credential-shaped content must fail");
        assert_eq!(error.code(), "credentials_forbidden");
        assert_eq!(error.category().as_str(), "policy");
        for secret in ["sk-test", "Bearer", "password", "secret", "apikey"] {
            assert!(
                !error.to_string().contains(secret),
                "the error never echoes the offending content"
            );
        }
    }
}

#[test]
fn candidate_source_accessors_are_exposed() {
    assert_eq!(
        ConfigCandidateSourceDto::StructuredEdits.as_str(),
        "structured_edits"
    );
    assert_eq!(
        ConfigCandidateSourceDto::StartupFile.as_str(),
        "startup_file"
    );

    let text = v1("openrouter", "fixture", FAKE_CREDENTIAL, None, "");
    let previous = snapshot(&text, "22222222-2222-4222-8222-222222222223");
    let parsed = candidate(&text, &previous);
    assert!(
        matches!(parsed.source(), ConfigCandidateSourceDto::RawToml { size_bytes } if size_bytes > 0),
        "the candidate source classification is exposed"
    );
    assert_eq!(
        parsed.candidate_revision_id(),
        parsed.safe_snapshot().revision_id().to_string(),
        "the candidate revision accessor matches the snapshot"
    );
}

#[test]
fn restore_credential_document_reinserts_the_credential_into_typed_edit_candidates() {
    let previous = snapshot(
        &v1("openrouter", "fixture-model", FAKE_CREDENTIAL, None, ""),
        "33333333-3333-4333-8333-333333333331",
    );
    // The typed-edit renderer emits a credential-free candidate document that
    // omits `provider.credential`; the private channel restores it.
    let edited = "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture-model\"\n[provider.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n";
    let restored = restore_credential_document(edited, FAKE_CREDENTIAL);
    assert!(
        !restored.eq(edited),
        "the restored document differs from the credential-free candidate"
    );

    let parsed = candidate(&restored, &previous);
    assert!(
        parsed.validation().issues().is_empty(),
        "restoring the credential makes the typed-edit candidate valid"
    );
    assert_eq!(parsed.validation().total_issue_count(), 0);
    let encoded = serde_json::to_string(&parsed).expect("candidate serializes");
    assert!(
        !encoded.contains(FAKE_CREDENTIAL),
        "the restored credential never crosses the candidate wire"
    );

    // The restored document resolves with the exact private value: the
    // composition-owned startup material carries the restored credential.
    let material =
        ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(restored, source()))
            .expect("the restored document parses as startup material");
    let credential = material.into_parts_for_provider(|_resolved, credential| credential);
    assert_eq!(credential, FAKE_CREDENTIAL);
}

#[test]
fn restore_credential_document_escapes_values_and_passes_through_unsuitable_documents() {
    // A credential carrying TOML-significant characters round-trips exactly:
    // the value is inserted as a TOML value, so escaping is serializer-owned.
    let tricky = "sk-\"quoted\"-and\\backslash";
    let edited = "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture-model\"\ncredential = \"previous-value\"\n[provider.execution]\nmax_attempts = 2\n";
    let restored = restore_credential_document(edited, tricky);
    let material =
        ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(restored, source()))
            .expect("the escaped document parses as startup material");
    let credential = material.into_parts_for_provider(|_resolved, credential| credential);
    assert_eq!(
        credential, tricky,
        "the restored value is the exact private credential, not a mangled text"
    );

    // Documents that cannot carry a provider credential table are returned
    // unchanged so downstream validation fails closed exactly as it does for
    // a document that legitimately omits the credential.
    for unsuitable in [
        "schema_version = 1\n",
        "not a toml document [[",
        "provider = \"openrouter\"",
    ] {
        assert_eq!(
            restore_credential_document(unsuitable, FAKE_CREDENTIAL),
            unsuitable,
            "unsuitable documents pass through unchanged"
        );
    }
}
