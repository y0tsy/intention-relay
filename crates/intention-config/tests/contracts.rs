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
use intention_proto::ThemeDto;

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
fn tui_theme_defaults_and_spellings_are_resolved_and_validated() {
    let document = |tui: &str| {
        RawConfigInputDto::new(
            format!(
                "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n{tui}"
            ),
            explicit_source(),
        )
    };

    // An absent [tui] section resolves to the light default.
    let defaulted =
        ResolvedConfigDto::parse_resolve(document("")).expect("absent section resolves");
    assert_eq!(defaulted.tui().theme(), ThemeDto::Light);

    // Both declared spellings resolve, and an empty section keeps the default.
    for (spelling, theme) in [("light", ThemeDto::Light), ("dark", ThemeDto::Dark)] {
        let resolved =
            ResolvedConfigDto::parse_resolve(document(&format!("[tui]\ntheme = \"{spelling}\"\n")))
                .expect("declared spelling resolves");
        assert_eq!(resolved.tui().theme(), theme);
    }
    let empty =
        ResolvedConfigDto::parse_resolve(document("[tui]\n")).expect("empty section resolves");
    assert_eq!(empty.tui().theme(), ThemeDto::Light);

    // An unknown spelling fails typed and names the accepted values.
    let error = ResolvedConfigDto::parse_resolve(document("[tui]\ntheme = \"midnight\"\n"))
        .expect_err("an unknown theme spelling fails");
    assert_eq!(error.code(), "invalid_tui_theme");
    assert_eq!(error.category().as_str(), "validation");
    let message = error.to_string();
    assert!(
        message.contains("\"light\"") && message.contains("\"dark\""),
        "the error names the accepted spellings: {message}"
    );

    // An unknown sibling key inside the section still fails the schema.
    assert_eq!(
        ResolvedConfigDto::parse_resolve(document("[tui]\ntheme = \"dark\"\nmode = \"plain\"\n"))
            .expect_err("an unknown [tui] key fails")
            .code(),
        "invalid_config_schema"
    );

    // The resolved projection round-trips its effective theme.
    let wire = serde_json::to_string(&defaulted).expect("resolved config serializes");
    assert!(wire.contains(r#""theme":"light""#));
    assert_eq!(
        serde_json::from_str::<ResolvedConfigDto>(&wire).expect("resolved config decodes"),
        defaulted
    );
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
