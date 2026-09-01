#![allow(
    clippy::expect_used,
    reason = "M4 contract fixtures use expect to provide precise test failure messages."
)]

use intention_config::{ConfigPathDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto};

const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";

fn source() -> ConfigSourceDto {
    ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-relay-m4-config.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    )
}

fn resolve(execution: &str) -> ResolvedConfigDto {
    ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n{execution}"
        ),
        source(),
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
fn execution_policy_rejects_out_of_range_values_without_redacting_errors() {
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
    ] {
        let error = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            format!(
                "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n{text}"
            ),
            source(),
        ))
        .expect_err("out-of-range execution policy must fail");
        assert_eq!(error.code(), code);
        assert!(!error.to_string().contains(FAKE_CREDENTIAL));
    }
}

#[test]
fn startup_material_is_opaque_and_safe_projection_excludes_credential() {
    let material = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"
        ),
        source(),
    ))
    .expect("startup material resolves");
    let resolved = material.safe_resolved();
    assert!(!resolved.safe_debug_projection().contains(FAKE_CREDENTIAL));
    assert_eq!(resolved.provider().model(), "fixture");
}

#[test]
fn startup_material_preserves_current_selection_only_for_provider_construction() {
    let material = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"
        ),
        source(),
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
    for (text, code) in [
        ("not = [valid", "invalid_config_toml"),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \" \"\n",
            "missing_provider_credential",
        ),
        (
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\n",
            "missing_provider_credential",
        ),
    ] {
        let result =
            ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(text, source()));
        assert!(result.is_err());
        let error = result
            .err()
            .expect("invalid startup material must return an error");
        assert_eq!(error.code(), code);
        assert!(!error.to_string().contains(FAKE_CREDENTIAL));
    }
}
