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
fn legacy_safe_snapshot_omitting_execution_policy_decodes_with_defaults() {
    let snapshot = serde_json::from_str::<intention_config::ConfigSnapshotDto>(include_str!(
        "fixtures/config-snapshot-v1.json"
    ))
    .expect("legacy M3 snapshot remains compatible");
    assert_eq!(
        snapshot
            .resolved()
            .provider_execution()
            .attempt_timeout_seconds(),
        30
    );
    assert_eq!(snapshot.resolved().provider_execution().max_attempts(), 2);
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
