#![allow(
    clippy::expect_used,
    reason = "Catalog contract fixtures use expect to provide precise test failure messages."
)]

//! Slice 2 catalog document, endpoint, reload, edit, and private-material contracts.

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::FAKE_CREDENTIAL;

use intention_config::RawConfigInputDto;
use intention_config::catalog::{
    CatalogCandidate, CatalogDeclaredReasoningCapabilityDto, CatalogDocumentDto,
    CatalogReloadImpactDto, classify_catalog_reload, normalize_provider_endpoint,
};
use intention_proto::{
    ConfigurationEditDto, CredentialTransportModeDto, ErrorDto, LoopbackPolicyDto, ProviderKindId,
    ProviderProfileId, ReasoningEffortLevelDto, UserKindActivationPartDto, UserKindEffortPartDto,
    UserKindReasoningPartDto, UserKindStreamPartDto,
};

/// A distinct second fake credential so per-profile material extraction is observable.
const LOCAL_CREDENTIAL: &str = "fixture-local-credential-not-real-67890";

const GLOBAL_SECTION: &str = "schema_version = 1\n\n[provider]\ncontext_window_tokens = 180000\ndefault_profile = \"main\"\n";

const KINDS_SECTION: &str = "\n[providers.kinds.my-dialect]\nkind_id = \"my-dialect\"\nstream = \"chat_completions_sse\"\nreasoning = \"reasoning_content\"\nactivation = \"thinking_enabled\"\neffort = \"reasoning_effort\"\ncredential_transport = { header = \"X-Api-Key\" }\n";

fn main_profile() -> String {
    format!(
        "\n[providers.profiles.main]\nkind = \"openrouter\"\nmodel = \"gpt-5.6-terra\"\ncredential = \"{FAKE_CREDENTIAL}\"\ndisplay_name = \"Main\"\nenabled = true\nreasoning_effort = \"medium\"\n\n[providers.profiles.main.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n\n[providers.profiles.main.capabilities]\ntext_streaming = true\nreasoning = \"textual_reasoning_v1\"\nreasoning_efforts = [\"medium\", \"low\"]\nreasoning_summary = true\ntool_exchange = true\n\n[providers.profiles.main.pricing]\ninput_per_million_tokens = 3.0\noutput_per_million_tokens = 15.0\n"
    )
}

fn local_profile() -> String {
    format!(
        "\n[providers.profiles.local]\nkind = \"generic-chat-completion-api\"\nmodel = \"example-chat-model\"\ncredential = \"{LOCAL_CREDENTIAL}\"\nendpoint = \"http://127.0.0.1:8080/v1/\"\ndisplay_name = \"Local\"\nenabled = false\ncredential_transport = {{ header = \"X-Api-Key\" }}\n\n[providers.profiles.local.capabilities]\ntext_streaming = true\n"
    )
}

fn catalog_text() -> String {
    format!(
        "{GLOBAL_SECTION}{KINDS_SECTION}{}{}",
        main_profile(),
        local_profile()
    )
}

fn main_only_text() -> String {
    format!("{GLOBAL_SECTION}{KINDS_SECTION}{}", main_profile())
}

fn parse_candidate(text: &str) -> Result<CatalogCandidate, ErrorDto> {
    CatalogCandidate::parse(RawConfigInputDto::new(text))
}

fn parse_document(text: &str) -> Result<CatalogDocumentDto, ErrorDto> {
    CatalogDocumentDto::parse_credential_free_edit(text)
}

fn active_document(text: &str) -> CatalogDocumentDto {
    parse_candidate(text)
        .expect("fixture catalog resolves")
        .safe_document()
        .clone()
}

fn profile_id(value: &str) -> ProviderProfileId {
    ProviderProfileId::parse(value).expect("fixture profile id is a valid token")
}

fn profile_override(body: &str) -> String {
    format!(
        "schema_version = 1\n\n[providers.profiles.main]\nkind = \"openrouter\"\nmodel = \"fixture-model\"\ncredential = \"{FAKE_CREDENTIAL}\"\n{body}"
    )
}

#[test]
fn valid_catalog_resolves_into_a_credential_free_candidate() {
    let candidate = parse_candidate(&catalog_text()).expect("valid catalog resolves");
    let document = candidate.safe_document();

    assert_eq!(document.context_window().window_tokens(), 180_000);
    assert_eq!(
        document.default_profile().map(ProviderProfileId::as_str),
        Some("main")
    );
    assert_eq!(document.user_kinds().len(), 1);
    let dialect = &document.user_kinds()[0];
    assert_eq!(dialect.kind_id().as_str(), "my-dialect");
    assert_eq!(dialect.composition().kind_id().as_str(), "my-dialect");
    assert_eq!(
        dialect.composition().stream(),
        UserKindStreamPartDto::ChatCompletionsSse
    );
    assert_eq!(
        dialect.composition().reasoning(),
        UserKindReasoningPartDto::ReasoningContent
    );
    assert_eq!(
        dialect.composition().activation(),
        UserKindActivationPartDto::ThinkingEnabled
    );
    assert_eq!(
        dialect.composition().effort(),
        UserKindEffortPartDto::ReasoningEffort
    );
    assert_eq!(
        dialect.composition().credential_transport().mode(),
        CredentialTransportModeDto::SafeHeader
    );
    assert_eq!(
        dialect
            .composition()
            .credential_transport()
            .safe_header_name(),
        Some("x-api-key")
    );

    let ordered: Vec<&str> = document
        .profiles()
        .iter()
        .map(|profile| profile.profile_id().as_str())
        .collect();
    assert_eq!(ordered, ["local", "main"]);

    let main = document
        .profile(&profile_id("main"))
        .expect("main profile is declared");
    assert_eq!(main.declaration().kind_id().as_str(), "openrouter");
    assert_eq!(main.declaration().model_id(), "gpt-5.6-terra");
    assert_eq!(main.declaration().normalized_effective_endpoint(), None);
    assert_eq!(
        main.declaration().credential_transport_mode(),
        CredentialTransportModeDto::Bearer
    );
    assert_eq!(
        main.declaration().credential_transport_safe_header_name(),
        None
    );
    assert_eq!(
        main.declaration().reasoning_effort(),
        Some(ReasoningEffortLevelDto::Medium)
    );
    assert_eq!(
        main.declaration()
            .effective_execution_policy()
            .attempt_timeout_seconds(),
        45
    );
    assert_eq!(
        main.declaration()
            .effective_execution_policy()
            .max_attempts(),
        2
    );
    assert_eq!(
        main.declaration().effective_loopback_policy(),
        LoopbackPolicyDto::NotApplicable
    );
    assert!(main.enabled());
    assert_eq!(main.display_name(), "Main");
    let pricing = main.pricing().expect("main profile declares pricing");
    assert_eq!(pricing.input_per_million_tokens(), Some(3.0));
    assert_eq!(pricing.output_per_million_tokens(), Some(15.0));
    assert!(main.declaration().declared_capabilities().text_streaming());
    assert!(main.declaration().declared_capabilities().tool_exchange());
    let reasoning = main.declaration().declared_capabilities().reasoning();
    let CatalogDeclaredReasoningCapabilityDto::TextualReasoningV1 {
        supported_efforts,
        summary_support,
    } = reasoning
    else {
        unreachable!("main profile declares textual reasoning");
    };
    assert_eq!(
        supported_efforts,
        &[
            ReasoningEffortLevelDto::Low,
            ReasoningEffortLevelDto::Medium
        ]
    );
    assert!(summary_support);

    let local = document
        .profile(&profile_id("local"))
        .expect("local profile is declared");
    assert_eq!(
        local.declaration().normalized_effective_endpoint(),
        Some("http://127.0.0.1:8080/v1")
    );
    assert_eq!(
        local.declaration().effective_loopback_policy(),
        LoopbackPolicyDto::ExplicitLoopback
    );
    assert_eq!(
        local.declaration().credential_transport_mode(),
        CredentialTransportModeDto::SafeHeader
    );
    assert_eq!(
        local.declaration().credential_transport_safe_header_name(),
        Some("x-api-key")
    );
    assert!(!local.enabled());
    assert!(local.pricing().is_none());
    assert!(matches!(
        local.declaration().declared_capabilities().reasoning(),
        CatalogDeclaredReasoningCapabilityDto::Disabled
    ));

    let encoded = serde_json::to_string(document).expect("credential-free document serializes");
    assert!(!encoded.contains(FAKE_CREDENTIAL));
    assert!(!encoded.contains(LOCAL_CREDENTIAL));
    assert!(!encoded.contains("\"credential\""));

    let main_id = profile_id("main");
    let local_id = profile_id("local");
    let unknown_id = profile_id("unknown");
    candidate.into_parts_for_catalog(|document, material| {
        let main_credential = material
            .with_profile_credential(&main_id, str::to_owned)
            .expect("main credential material is present");
        assert_eq!(main_credential, FAKE_CREDENTIAL);
        let local_credential = material
            .with_profile_credential(&local_id, str::to_owned)
            .expect("local credential material is present");
        assert_eq!(local_credential, LOCAL_CREDENTIAL);
        let missing = material
            .with_profile_credential(&unknown_id, str::to_owned)
            .expect_err("unknown profile has no material");
        assert_eq!(missing.code(), "missing_provider_credential");

        let rebound = CatalogCandidate::bind_retained_material(document.clone(), &material)
            .expect("every declared profile has retained material");
        assert_eq!(rebound.safe_document(), &document);
    });
}

#[test]
fn catalog_document_boundary_failures_are_typed_and_safe() {
    let valid_profile = format!(
        "kind = \"openrouter\"\nmodel = \"fixture-model\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"
    );
    let cases: Vec<(&str, String)> = vec![
        ("invalid_config_toml", "schema_version = [".to_owned()),
        (
            "unsupported_config_schema_version",
            profile_override("").replace("schema_version = 1", "schema_version = 2"),
        ),
        (
            "invalid_config_schema_version",
            profile_override("")
                .replace("schema_version = 1", "schema_version = \"one\""),
        ),
        ("missing_provider_profile", "schema_version = 1\n".to_owned()),
        (
            "missing_provider_profile",
            "schema_version = 1\n\n[providers]\nprofiles = {}\n".to_owned(),
        ),
        (
            "missing_provider_profile",
            "schema_version = 1\n\n[providers]\n[providers.kinds]\n".to_owned(),
        ),
        (
            "invalid_config_schema",
            format!("schema_version = 1\nunknown_top_level = 1\n\n[providers.profiles.main]\n{valid_profile}"),
        ),
        (
            "invalid_config_schema",
            format!(
                "schema_version = 1\n\n[provider]\nkind = \"openrouter\"\n\n[providers.profiles.main]\n{valid_profile}"
            ),
        ),
        (
            "invalid_config_schema",
            profile_override("unknown_profile_key = 1\n"),
        ),
        (
            "invalid_config_schema",
            format!(
                "schema_version = 1\n\n[providers.profiles.main]\nkind = \"openrouter\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"
            ),
        ),
        (
            "invalid_provider_profile_id",
            format!(
                "schema_version = 1\n\n[providers.profiles.\"\"]\n{valid_profile}"
            ),
        ),
        (
            "invalid_provider_model",
            format!(
                "schema_version = 1\n\n[providers.profiles.main]\nkind = \"openrouter\"\nmodel = \" \"\ncredential = \"{FAKE_CREDENTIAL}\"\n"
            ),
        ),
        (
            "missing_provider_credential",
            "schema_version = 1\n\n[providers.profiles.main]\nkind = \"openrouter\"\nmodel = \"fixture\"\n".to_owned(),
        ),
        (
            "missing_provider_credential",
            "schema_version = 1\n\n[providers.profiles.main]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \" \"\n".to_owned(),
        ),
        (
            "invalid_provider_kind",
            format!("schema_version = 1\n\n[providers.profiles.main]\nkind = \"unknown-kind\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"),
        ),
        (
            "invalid_provider_kind",
            format!("schema_version = 1\n\n[providers.profiles.main]\nkind = \"\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"),
        ),
        (
            "invalid_provider_kind",
            format!("schema_version = 1\n\n[providers.profiles.main]\nkind = \"my kind\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"),
        ),
        (
            "invalid_provider_kind",
            format!("schema_version = 1\n\n[providers.profiles.main]\nkind = \"my-dialect\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\"\n"),
        ),
        (
            "invalid_provider_kind",
            "schema_version = 1\n\n[providers.kinds.openrouter]\nkind_id = \"openrouter\"\nstream = \"chat_completions_sse\"\nreasoning = \"absent\"\nactivation = \"absent\"\neffort = \"absent\"\ncredential_transport = \"bearer\"\n".to_owned(),
        ),
        (
            "invalid_provider_kind",
            "schema_version = 1\n\n[providers.kinds.my-dialect]\nkind_id = \"other-dialect\"\nstream = \"chat_completions_sse\"\nreasoning = \"absent\"\nactivation = \"absent\"\neffort = \"absent\"\ncredential_transport = \"bearer\"\n".to_owned(),
        ),
        (
            "invalid_config_schema",
            "schema_version = 1\n\n[providers.kinds.my-dialect]\nkind_id = \"my-dialect\"\nstream = \"chat_completions_sse\"\nreasoning = \"absent\"\nactivation = \"absent\"\neffort = \"absent\"\ncredential_transport = \"bearer\"\nunknown_part = 1\n".to_owned(),
        ),
        (
            "invalid_config_schema",
            "schema_version = 1\n\n[providers.kinds.my-dialect]\nkind_id = \"my-dialect\"\nstream = \"nope\"\nreasoning = \"absent\"\nactivation = \"absent\"\neffort = \"absent\"\ncredential_transport = \"bearer\"\n".to_owned(),
        ),
        (
            "invalid_provider_credential_transport",
            "schema_version = 1\n\n[providers.kinds.my-dialect]\nkind_id = \"my-dialect\"\nstream = \"chat_completions_sse\"\nreasoning = \"absent\"\nactivation = \"absent\"\neffort = \"absent\"\ncredential_transport = { mode = \"basic\" }\n".to_owned(),
        ),
        (
            "invalid_provider_credential_transport",
            "schema_version = 1\n\n[providers.kinds.my-dialect]\nkind_id = \"my-dialect\"\nstream = \"chat_completions_sse\"\nreasoning = \"absent\"\nactivation = \"absent\"\neffort = \"absent\"\ncredential_transport = { header = \"authorization\" }\n".to_owned(),
        ),
        (
            "invalid_provider_credential_transport",
            profile_override("credential_transport = { mode = \"bearer\", safe_header_name = \"x-api-key\" }\n"),
        ),
        (
            "invalid_provider_credential_transport",
            profile_override("credential_transport = { header = \"x-api-key\", mode = \"bearer\" }\n"),
        ),
        (
            "invalid_provider_default_profile",
            format!("schema_version = 1\n\n[provider]\ndefault_profile = \"absent-profile\"\n\n[providers.profiles.main]\n{valid_profile}"),
        ),
        (
            "invalid_provider_default_profile",
            format!("schema_version = 1\n\n[provider]\ndefault_profile = \"\"\n\n[providers.profiles.main]\n{valid_profile}"),
        ),
        (
            "invalid_provider_endpoint",
            format!("schema_version = 1\n\n[providers.profiles.main]\n{valid_profile}endpoint = \"http://example.invalid/v1\"\n"),
        ),
        (
            "invalid_provider_attempt_timeout_seconds",
            format!("schema_version = 1\n\n[providers.profiles.main]\n{valid_profile}\n[providers.profiles.main.execution]\nattempt_timeout_seconds = 61\n"),
        ),
        (
            "invalid_provider_max_attempts",
            format!("schema_version = 1\n\n[providers.profiles.main]\n{valid_profile}\n[providers.profiles.main.execution]\nmax_attempts = 3\n"),
        ),
        (
            "invalid_provider_context_window_tokens",
            format!("schema_version = 1\n\n[provider]\ncontext_window_tokens = 0\n\n[providers.profiles.main]\n{valid_profile}"),
        ),
        (
            "invalid_provider_reasoning_effort",
            profile_override("reasoning_effort = \"extreme\"\n"),
        ),
        (
            "invalid_provider_reasoning_effort",
            profile_override("reasoning_effort = \"high\"\n"),
        ),
        (
            "invalid_provider_reasoning_effort",
            profile_override(
                "reasoning_effort = \"high\"\n\n[providers.profiles.main.capabilities]\nreasoning = \"textual_reasoning_v1\"\nreasoning_efforts = [\"low\"]\n",
            ),
        ),
        (
            "invalid_provider_reasoning_effort",
            profile_override(
                "\n[providers.profiles.main.capabilities]\nreasoning = \"textual_reasoning_v1\"\nreasoning_efforts = [\"extreme\"]\n",
            ),
        ),
        (
            "invalid_provider_capabilities",
            profile_override(
                "\n[providers.profiles.main.capabilities]\nreasoning = \"disabled\"\nreasoning_efforts = [\"low\"]\n",
            ),
        ),
        (
            "invalid_provider_capabilities",
            profile_override("\n[providers.profiles.main.capabilities]\nreasoning = \"sometimes\"\n"),
        ),
        (
            "invalid_config_schema",
            profile_override("\n[providers.profiles.main.capabilities]\nunknown_capability = true\n"),
        ),
        (
            "invalid_provider_pricing_policy",
            profile_override("\n[providers.profiles.main.pricing]\ninput_per_million_tokens = -1.0\n"),
        ),
        (
            "invalid_provider_pricing_policy",
            profile_override("\n[providers.profiles.main.pricing]\ninput_per_million_tokens = nan\n"),
        ),
        (
            "invalid_provider_credential_transport",
            profile_override("credential_transport = \"apikey\"\n"),
        ),
        (
            "invalid_provider_credential_transport",
            profile_override("credential_transport = { header = \"x api key\" }\n"),
        ),
        (
            "invalid_provider_credential_transport",
            profile_override("credential_transport = { header = \"authorization\" }\n"),
        ),
        (
            "invalid_provider_credential_transport",
            profile_override("credential_transport = { mode = \"safe_header\" }\n"),
        ),
        (
            "invalid_provider_display_name",
            profile_override("display_name = \" \"\n"),
        ),
        (
            "invalid_provider_display_name",
            profile_override("display_name = \"bad\\u0007name\"\n"),
        ),
    ];

    for (expected_code, text) in cases {
        let failure_message = format!("invalid fixture must fail: {text}");
        let error = parse_candidate(&text)
            .map(|_candidate| ())
            .expect_err(&failure_message);
        assert_eq!(error.code(), expected_code, "fixture: {text}");
        assert!(
            !error.to_string().contains(FAKE_CREDENTIAL),
            "the error never echoes the credential: {error}"
        );
    }
}

#[test]
fn endpoint_rules_accept_canonical_urls_and_reject_aliases() {
    for (input, expected) in [
        ("https://example.invalid/v1", "https://example.invalid/v1"),
        ("https://example.invalid/v1/", "https://example.invalid/v1"),
        (
            "https://example.invalid:443/v1",
            "https://example.invalid/v1",
        ),
        (
            "https://example.invalid:8443/v1",
            "https://example.invalid:8443/v1",
        ),
        (
            "https://example.invalid/%7bembed%7d",
            "https://example.invalid/%7Bembed%7D",
        ),
        ("http://localhost:8080/v1", "http://localhost:8080/v1"),
        ("http://127.0.0.1/v1", "http://127.0.0.1/v1"),
        ("http://[::1]:80/v1", "http://[::1]/v1"),
        ("https://127.0.0.1/v1", "https://127.0.0.1/v1"),
        (
            "https://api.example.com/v1/chat",
            "https://api.example.com/v1/chat",
        ),
    ] {
        assert_eq!(
            normalize_provider_endpoint(input).expect("canonical endpoint is accepted"),
            expected
        );
    }

    for input in [
        "",
        "https://example.invalid",
        "https://example.invalid/",
        "https://user:secret@example.invalid/v1",
        "https://example.invalid/v1?query=1",
        "https://example.invalid/v1#fragment",
        "http://example.invalid/v1",
        "http://192.168.1.10/v1",
        "HTTP://example.invalid/v1",
        "https://EXAMPLE.invalid/v1",
        "https://example.invalid:0/v1",
        "https://example.invalid:65536/v1",
        "https://example.invalid:0443/v1",
        "https://example.invalid:port/v1",
        "https://127.1/v1",
        "https://0x7f.0.0.1/v1",
        "https://127.0.0.01/v1",
        "https://[0:0:0:0:0:0:0:1]/v1",
        "https://[2001:db8::1]/v1",
        "https://example.invalid/v1//chat",
        "https://example.invalid/v1/./chat",
        "https://example.invalid/v1/../chat",
        "https://example.invalid/v1%2fchat",
        "https://example.invalid/%zz",
        "https://example.invalid/%0a",
        "https://example.invalid/%23fragment",
        "https://local host/v1",
        "https://example.invalid/v1\n",
        "https://example.invalid\\v1",
        "https://localhost./v1",
        "https://-example.invalid/v1",
        "https://example.invalid-/v1",
    ] {
        let error = normalize_provider_endpoint(input).expect_err("non-canonical endpoint fails");
        assert_eq!(error.code(), "invalid_provider_endpoint");
        assert!(input.is_empty() || !error.to_string().contains(input));
    }
}

#[test]
fn catalog_reload_classification_separates_restart_and_fresh_run_changes() {
    let active = active_document(&catalog_text());

    let credential_only =
        active_document(&catalog_text().replace(FAKE_CREDENTIAL, "another-credential"));
    assert_eq!(
        classify_catalog_reload(&active, &credential_only)
            .expect("credential-only change is equal"),
        CatalogReloadImpactDto::Unchanged
    );

    for change in [
        catalog_text().replace("display_name = \"Main\"", "display_name = \"Primary\""),
        catalog_text().replace("enabled = true", "enabled = false"),
        catalog_text().replace(
            "input_per_million_tokens = 3.0",
            "input_per_million_tokens = 4.0",
        ),
        catalog_text().replace("default_profile = \"main\"", "default_profile = \"local\""),
        catalog_text().replace(
            "context_window_tokens = 180000",
            "context_window_tokens = 200000",
        ),
    ] {
        let candidate = active_document(&change);
        assert_eq!(
            classify_catalog_reload(&active, &candidate).expect("fresh-run change is reloadable"),
            CatalogReloadImpactDto::FreshRunsOnly
        );
    }

    for change in [
        catalog_text().replace("gpt-5.6-terra", "gpt-5.7-terra"),
        catalog_text().replace("http://127.0.0.1:8080/v1/", "http://127.0.0.1:9090/v1/"),
        catalog_text().replace(
            "kind = \"openrouter\"",
            "kind = \"generic-chat-completion-api\"",
        ),
        catalog_text().replace(
            "reasoning_effort = \"medium\"",
            "reasoning_effort = \"low\"",
        ),
        catalog_text().replace("reasoning_summary = true", "reasoning_summary = false"),
        catalog_text().replace(
            "activation = \"thinking_enabled\"",
            "activation = \"absent\"",
        ),
        catalog_text().replace(
            "attempt_timeout_seconds = 45",
            "attempt_timeout_seconds = 30",
        ),
        format!(
            "{}\n[providers.profiles.extra]\nkind = \"openrouter\"\nmodel = \"extra\"\ncredential = \"{FAKE_CREDENTIAL}\"\n",
            catalog_text()
        ),
        main_only_text(),
    ] {
        let candidate = active_document(&change);
        let error = classify_catalog_reload(&active, &candidate)
            .expect_err("catalog-affecting change rejects a live reload");
        assert_eq!(error.code(), "catalog_change_requires_restart");
    }

    let kinds_removed = catalog_text().replace(KINDS_SECTION, "");
    let candidate = active_document(&kinds_removed);
    assert_eq!(
        classify_catalog_reload(&active, &candidate)
            .expect_err("kind removal is catalog-affecting")
            .code(),
        "catalog_change_requires_restart"
    );
}

#[test]
fn edit_rendering_round_trips_and_rejects_credentials() {
    let candidate = parse_candidate(&catalog_text()).expect("valid catalog resolves");
    let edits = [
        ConfigurationEditDto::set_default_profile(profile_id("local")),
        ConfigurationEditDto::set_profile_enabled(profile_id("main"), false),
        ConfigurationEditDto::set_profile_display_name(profile_id("main"), "Primary")
            .expect("edited display name is valid"),
    ];

    let rendered = candidate
        .render_edited_document(&edits)
        .expect("typed edits render");
    assert_eq!(
        candidate
            .render_edited_document(&edits)
            .expect("rendering is deterministic"),
        rendered
    );
    assert!(rendered.contains("default_profile = \"local\""));
    assert!(rendered.contains("display_name = \"Primary\""));
    assert!(rendered.contains("\nenabled = false\n"));
    assert!(!rendered.contains("credential ="));
    assert!(!rendered.contains(FAKE_CREDENTIAL));
    assert!(!rendered.contains(LOCAL_CREDENTIAL));

    let expected = candidate
        .safe_document()
        .apply_edits(&edits)
        .expect("typed edits apply to the document");
    let reparsed =
        parse_document(&rendered).expect("edited document validates through the same path");
    assert_eq!(reparsed, expected);

    let injected = rendered.replace(
        "model = \"gpt-5.6-terra\"",
        &format!("model = \"gpt-5.6-terra\"\ncredential = \"{FAKE_CREDENTIAL}\""),
    );
    assert_ne!(
        injected, rendered,
        "profile injection lands in the document"
    );
    let error = parse_document(&injected).expect_err("credential-bearing edit fails");
    assert_eq!(error.code(), "configuration_edit_contains_credential");
    assert!(!error.to_string().contains(FAKE_CREDENTIAL));

    // A credential nested inside a subtable is rejected by the same document scan.
    let nested_injection = rendered.replace(
        "[providers.kinds.my-dialect.credential_transport]\nheader = \"x-api-key\"",
        &format!(
            "[providers.kinds.my-dialect.credential_transport]\nheader = \"x-api-key\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
    );
    assert_ne!(
        nested_injection, rendered,
        "nested injection lands in the document"
    );
    let error =
        parse_document(&nested_injection).expect_err("nested credential-bearing edit fails");
    assert_eq!(error.code(), "configuration_edit_contains_credential");
    assert!(!error.to_string().contains(FAKE_CREDENTIAL));

    let unknown = candidate
        .safe_document()
        .apply_edits(&[ConfigurationEditDto::set_profile_enabled(
            profile_id("absent"),
            true,
        )])
        .expect_err("unknown profile edit fails");
    assert_eq!(unknown.code(), "invalid_configuration_edit");

    let blank = candidate
        .safe_document()
        .apply_edits(&[ConfigurationEditDto::SetProfileDisplayName {
            profile_id: profile_id("main"),
            display_name: " ".to_owned(),
        }])
        .expect_err("blank display name edit fails");
    assert_eq!(blank.code(), "invalid_configuration_edit");

    let (_, material) = candidate.into_parts_for_catalog(|document, material| (document, material));
    let rebound = CatalogCandidate::bind_retained_material(reparsed, &material)
        .expect("retained material restores the edited document");
    rebound.into_parts_for_catalog(|document, material| {
        assert_eq!(document.context_window().window_tokens(), 180_000);
        assert_eq!(
            material
                .with_profile_credential(&profile_id("main"), str::to_owned)
                .expect("main material is retained"),
            FAKE_CREDENTIAL
        );
    });
}

#[test]
fn user_kind_declarations_are_closed_and_cannot_replace_first_party_ids() {
    let candidate = parse_candidate(&catalog_text()).expect("user kind resolves");
    let kind = &candidate.safe_document().user_kinds()[0];
    assert!(
        !ProviderKindId::parse("my-dialect")
            .expect("token parses")
            .is_first_party()
    );
    assert_eq!(kind.kind_id().as_str(), "my-dialect");
    assert_eq!(
        kind.composition().credential_transport().mode(),
        CredentialTransportModeDto::SafeHeader
    );
    assert_eq!(
        kind.composition().credential_transport().safe_header_name(),
        Some("x-api-key")
    );

    let reserved = format!(
        "schema_version = 1\n\n[providers.kinds.generic-chat-completion-api]\nkind_id = \"generic-chat-completion-api\"\nstream = \"chat_completions_sse\"\nreasoning = \"absent\"\nactivation = \"absent\"\neffort = \"absent\"\ncredential_transport = \"bearer\"\n{}{}",
        main_profile(),
        local_profile()
    );
    let error = parse_candidate(&reserved)
        .map(|_candidate| ())
        .expect_err("reserved kind fails");
    assert_eq!(error.code(), "invalid_provider_kind");

    let rendered = candidate
        .safe_document()
        .render_with_edits(&[])
        .expect("credential-free rendering succeeds");
    let reparsed = parse_document(&rendered).expect("rendered document round-trips");
    assert_eq!(reparsed, *candidate.safe_document());

    // Kind sections also accept the shared `CredentialTransportDto` part shape and
    // normalize it to the same validated declaration.
    let shared_shape_kinds = KINDS_SECTION.replace(
        "credential_transport = { header = \"X-Api-Key\" }",
        "credential_transport = { mode = \"safe_header\", safe_header_name = \"x-api-key\" }",
    );
    assert_ne!(shared_shape_kinds, KINDS_SECTION);
    let shared_shape = parse_candidate(&format!(
        "{GLOBAL_SECTION}{shared_shape_kinds}{}{}",
        main_profile(),
        local_profile()
    ))
    .expect("kind sections accept the shared credential-transport shape");
    assert_eq!(shared_shape.safe_document(), candidate.safe_document());
}

#[test]
fn startup_material_is_required_and_bound_to_declared_profiles() {
    let candidate = parse_candidate(&catalog_text()).expect("valid catalog resolves");
    let (document, material) =
        candidate.into_parts_for_catalog(|document, material| (document, material));

    let extra_material = CatalogCandidate::bind_retained_material(document.clone(), &material)
        .expect("full material binds");

    let edited = document
        .apply_edits(&[ConfigurationEditDto::set_default_profile(profile_id(
            "local",
        ))])
        .expect("default profile edit applies");
    CatalogCandidate::bind_retained_material(edited, &material)
        .expect("edited document binds to retained material");

    extra_material.into_parts_for_catalog(|document, material| {
        assert_eq!(document.profiles().len(), 2);
        assert!(
            material
                .with_profile_credential(&profile_id("main"), |_| ())
                .is_ok()
        );
    });
}
