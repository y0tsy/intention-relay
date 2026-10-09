#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first configuration default resolution and redaction evidence.

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::FAKE_CREDENTIAL;

use intention_config::RawConfigInputDto;
use intention_config::catalog::CatalogCandidate;
use intention_proto::ProviderProfileId;

#[test]
fn catalog_policy_defaults_resolve_into_the_credential_free_document() {
    let candidate = CatalogCandidate::parse(RawConfigInputDto::new(format!(
        "schema_version = 1\n[provider]\ndefault_profile = \"main\"\n[providers.profiles.main]\nkind = \"openrouter\"\nmodel = \"gpt-5.6-terra\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"
    )))
    .expect("fixture catalog resolves");
    let document = candidate.safe_document();

    assert_eq!(
        document.context_window().window_tokens(),
        250_000,
        "an omitted global context window resolves to the current default"
    );
    let profile_id = ProviderProfileId::parse("main").expect("fixture profile identity is valid");
    let main = document
        .profile(&profile_id)
        .expect("the fixture profile is declared");
    assert_eq!(
        main.declaration()
            .effective_execution_policy()
            .attempt_timeout_seconds(),
        30,
        "an omitted execution table resolves to the default attempt timeout"
    );
    assert_eq!(
        main.declaration()
            .effective_execution_policy()
            .max_attempts(),
        2,
        "an omitted execution table resolves to the default attempt budget"
    );

    let encoded = serde_json::to_string(document).expect("the safe document serializes");
    assert!(!encoded.contains(FAKE_CREDENTIAL));
}
