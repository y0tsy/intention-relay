#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first configuration parsing, migration, resolution, and redaction evidence.

use intention_config::{
    ConfigPathDto, ConfigPathResolver, ConfigSourceDto, ConfigSourceKindDto, RawConfigInputDto,
    ResolvedConfigDto,
};

const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";

fn fixture_path(filename: &str) -> String {
    std::env::temp_dir()
        .join(filename)
        .to_string_lossy()
        .into_owned()
}

fn explicit_source() -> ConfigSourceDto {
    ConfigSourceDto::Explicit(
        ConfigPathDto::parse(fixture_path("intention.toml"))
            .expect("fixture config path is absolute"),
    )
}

#[test]
fn valid_v1_toml_resolves_to_a_redacted_public_dto() {
    let raw = RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"gpt-5.6-terra\"\ncredential = \"fixture-credential-not-real-12345\"\n",
        explicit_source(),
    );

    let resolved = ResolvedConfigDto::parse_resolve(raw).expect("fixture config must resolve");
    let encoded = resolved.safe_debug_projection();

    assert_eq!(resolved.schema_version().major(), 1);
    assert_eq!(resolved.provider().kind().as_str(), "openrouter");
    assert!(!encoded.contains(FAKE_CREDENTIAL));
    assert!(!resolved.to_string().contains(FAKE_CREDENTIAL));
}

#[test]
fn malformed_or_unsupported_toml_returns_safe_typed_errors() {
    let malformed = RawConfigInputDto::new("schema_version = [", explicit_source());
    let future_version = RawConfigInputDto::new(
        "schema_version = 99\n[provider]\nkind = \"openrouter\"\nmodel = \"gpt-5.6-terra\"\ncredential = \"fixture-credential-not-real-12345\"\n",
        explicit_source(),
    );
    let wrong_schema_type = RawConfigInputDto::new(
        "schema_version = \"one\"\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential-not-real-12345\"\n",
        explicit_source(),
    );
    let undeclared_provider = RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openai\"\nmodel = \"gpt-5.6-terra\"\ncredential = \"fixture-credential-not-real-12345\"\n",
        explicit_source(),
    );

    for input in [
        malformed,
        future_version,
        wrong_schema_type,
        undeclared_provider,
    ] {
        let error = ResolvedConfigDto::parse_resolve(input).expect_err("fixture must fail");
        assert_eq!(error.category().as_str(), "validation");
        assert!(!error.to_string().contains(FAKE_CREDENTIAL));
    }
}

#[test]
fn v0_fixture_migrates_to_v1_without_disclosing_credentials() {
    let raw = RawConfigInputDto::new(
        "[model]\nprovider = \"openrouter\"\nname = \"gpt-5.6-terra\"\napi_key = \"fixture-credential-not-real-12345\"\n",
        explicit_source(),
    );

    let resolved = ResolvedConfigDto::parse_resolve(raw).expect("v0 fixture must migrate");

    assert_eq!(resolved.schema_version().major(), 1);
    assert!(!resolved.safe_debug_projection().contains(FAKE_CREDENTIAL));
}

#[test]
fn generic_chat_preserves_configured_model_identifier() {
    let raw = RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"example-chat-model\"\ncredential = \"fixture-credential-not-real-12345\"\n",
        explicit_source(),
    );

    let resolved = ResolvedConfigDto::parse_resolve(raw)
        .expect("generic provider must preserve the configured model identifier");

    assert_eq!(
        resolved.provider().kind().as_str(),
        "generic-chat-completion-api"
    );
    assert_eq!(resolved.provider().model(), "example-chat-model");
    assert!(!resolved.safe_debug_projection().contains(FAKE_CREDENTIAL));
}

#[test]
fn explicit_path_overrides_platform_default_resolution() {
    let fixture_path = fixture_path("override.toml");
    let explicit = ConfigPathDto::parse(fixture_path.as_str()).expect("fixture path is absolute");
    let source =
        ConfigPathResolver::resolve(Some(explicit.clone())).expect("explicit path is valid");

    assert_eq!(source, ConfigSourceDto::Explicit(explicit));
    assert_eq!(source.kind(), ConfigSourceKindDto::Explicit);
    assert_eq!(source.kind().as_str(), "explicit");
    assert_eq!(source.kind().to_string(), "explicit");

    let encoded = serde_json::to_string(&fixture_path).expect("fixture path serializes");
    let decoded: ConfigPathDto =
        serde_json::from_str(&encoded).expect("absolute wire path is valid");
    assert_eq!(decoded.as_str(), fixture_path);
    assert!(serde_json::from_str::<ConfigPathDto>(r#""relative.toml""#).is_err());
}
