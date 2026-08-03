#![allow(
    clippy::expect_used,
    reason = "Provider contract fixtures use expect to provide precise test failure messages."
)]

use intention_config::{
    ConfigPathDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto, StartupProviderMaterial,
};
use intention_model::{
    FinishReasonDto, ModelDriver, ModelMessageDto, ModelRequestDto, ModelRequestedCapabilitiesDto,
    ModelRoleDto,
};
use intention_provider_generic_chat::GenericChatDriver;
use intention_types::RunId;

const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";

fn material() -> StartupProviderMaterial {
    ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\nendpoint = \"https://example.invalid/v1\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-relay-generic.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        ),
    ))
    .expect("generic config resolves")
}

fn request(capabilities: ModelRequestedCapabilitiesDto) -> ModelRequestDto {
    ModelRequestDto::new(
        RunId::new(),
        "fixture",
        vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
        Some("system".to_owned()),
        Some(capabilities),
    )
    .expect("request is valid")
}

#[test]
fn generic_driver_declares_supported_subset_and_rejects_unsupported_preflight() {
    let driver = GenericChatDriver::from_startup_material(material()).expect("driver builds");
    assert!(driver.capabilities().supports_text());
    assert!(driver.capabilities().supports_tool_calls());
    assert!(!driver.capabilities().supports_reasoning());
    assert!(!driver.capabilities().supports_multimodal());
    assert!(!driver.capabilities().supports_vendor_extensions());
    assert!(
        driver
            .preflight(&request(ModelRequestedCapabilitiesDto::new(
                false, false, true, false,
            )))
            .is_ok()
    );
    assert_eq!(
        driver
            .preflight(&request(ModelRequestedCapabilitiesDto::new(
                true, false, false, false,
            )))
            .expect_err("reasoning must reject before outbound work")
            .code(),
        "unsupported_model_capability"
    );
    assert_eq!(driver.prepared_request_count(), 0);

    for unsupported in [
        ModelRequestedCapabilitiesDto::new(false, true, false, false),
        ModelRequestedCapabilitiesDto::new(false, false, false, true),
    ] {
        assert_eq!(
            driver
                .preflight(&request(unsupported))
                .expect_err("unsupported generic capability rejects before preparation")
                .code(),
            "unsupported_model_capability"
        );
        assert_eq!(driver.prepared_request_count(), 0);
    }
}

#[test]
fn generic_mapping_normalizes_text_usage_finish_error_and_tool_call() {
    let text = GenericChatDriver::map_fixture_text("hello").expect("text maps");
    assert_eq!(
        serde_json::to_string(&text).expect("serializes"),
        r#"{"kind":"text_delta","content":"hello"}"#
    );
    let usage = GenericChatDriver::map_fixture_usage(2, 3, 5).expect("usage maps");
    assert!(
        serde_json::to_string(&usage)
            .expect("serializes")
            .contains("reported")
    );
    assert_eq!(
        GenericChatDriver::map_fixture_finish("tool_calls"),
        FinishReasonDto::ToolCalls
    );
    assert_eq!(
        GenericChatDriver::map_fixture_finish("other"),
        FinishReasonDto::Unknown
    );
    let tool =
        GenericChatDriver::map_fixture_tool_call("call-1", "inspect", "{}").expect("tool maps");
    assert_eq!(tool.name(), "inspect");
    let error = GenericChatDriver::map_fixture_error(429, "provider text must not leak")
        .expect("error maps");
    assert_eq!(error.code(), "generic_chat_provider_unavailable");
    assert!(
        !serde_json::to_string(&error)
            .expect("serializes")
            .contains("provider text")
    );
}

#[test]
fn generic_public_driver_does_not_expose_credential() {
    let driver = GenericChatDriver::from_startup_material(material()).expect("driver builds");
    let debug = format!("{driver:?}");
    assert!(!debug.contains(FAKE_CREDENTIAL));
}

#[test]
fn generic_driver_translates_all_text_roles_without_network_work() {
    let mut driver = GenericChatDriver::from_startup_material(material()).expect("driver builds");
    let request = ModelRequestDto::new(
        RunId::new(),
        "fixture",
        vec![
            ModelMessageDto::new(ModelRoleDto::System, "system message").expect("message is valid"),
            ModelMessageDto::new(ModelRoleDto::User, "user message").expect("message is valid"),
            ModelMessageDto::new(ModelRoleDto::Assistant, "assistant message")
                .expect("message is valid"),
        ],
        Some("separate system context".to_owned()),
        None,
    )
    .expect("request is valid");

    driver.prepare_request(&request).expect("request prepares");
    assert_eq!(driver.prepared_request_count(), 1);
}

#[test]
fn generic_driver_rejects_wrong_kind_and_missing_endpoint_safely() {
    let wrong_kind = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-relay-generic-wrong-kind.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        ),
    ))
    .expect("material resolves");
    assert_eq!(
        GenericChatDriver::from_startup_material(wrong_kind)
            .expect_err("wrong provider kind fails")
            .code(),
        "invalid_generic_chat_provider_config"
    );

    let missing_endpoint = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-relay-generic-no-endpoint.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        ),
    ))
    .expect("material resolves");
    assert_eq!(
        GenericChatDriver::from_startup_material(missing_endpoint)
            .expect_err("missing endpoint fails")
            .code(),
        "missing_generic_chat_endpoint"
    );
}

#[test]
fn generic_mapping_covers_known_finish_status_and_invalid_fixture_values() {
    for (native, expected) in [
        ("stop", FinishReasonDto::Stop),
        ("length", FinishReasonDto::Length),
        ("tool_calls", FinishReasonDto::ToolCalls),
        ("function_call", FinishReasonDto::ToolCalls),
        ("content_filter", FinishReasonDto::ContentFilter),
        ("error", FinishReasonDto::Error),
        ("unknown", FinishReasonDto::Unknown),
    ] {
        assert_eq!(GenericChatDriver::map_fixture_finish(native), expected);
    }
    for status in [400_u16, 429, 500] {
        let error = GenericChatDriver::map_fixture_error(status, FAKE_CREDENTIAL)
            .expect("error mapping is valid");
        assert!(
            !serde_json::to_string(&error)
                .expect("error serializes")
                .contains(FAKE_CREDENTIAL)
        );
    }
    assert!(GenericChatDriver::map_fixture_text("").is_err());
    assert!(GenericChatDriver::map_fixture_usage(1, 1, 1).is_err());
    assert!(GenericChatDriver::map_fixture_tool_call("call", "", "{}").is_err());
    assert!(GenericChatDriver::map_fixture_tool_call("call", "inspect", "[]").is_err());
}
