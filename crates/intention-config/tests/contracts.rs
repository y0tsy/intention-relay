#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first configuration parsing, resolution, and redaction evidence.

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{FAKE_CREDENTIAL, explicit_source};

use intention_config::{RawConfigInputDto, ResolvedConfigDto};

#[test]
fn valid_v1_toml_resolves_to_a_redacted_public_dto() {
    let raw = RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"gpt-5.6-terra\"\ncredential = \"fixture-credential-not-real-12345\"\n",
        explicit_source(),
    );

    let resolved = ResolvedConfigDto::parse_resolve(raw).expect("fixture config must resolve");
    let encoded = serde_json::to_string(&resolved).expect("safe resolved config serializes");

    assert_eq!(resolved.provider().kind().as_str(), "openrouter");
    assert!(!encoded.contains(FAKE_CREDENTIAL));
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
