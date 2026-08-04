#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Provider contract fixtures use explicit failure messages for impossible pending local streams."
)]

use intention_config::{
    ConfigPathDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto, StartupProviderMaterial,
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, task::noop_waker_ref};
use intention_model::{
    FinishReasonDto, ModelCancellationSignal, ModelDriver, ModelExecutionDriver, ModelMessageDto,
    ModelRequestDto, ModelRequestedCapabilitiesDto, ModelRoleDto,
};
use intention_provider_openrouter::OpenRouterDriver;
use intention_types::RunId;

const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";

fn material() -> StartupProviderMaterial {
    ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-relay-openrouter.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        ),
    ))
    .expect("OpenRouter config resolves")
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
fn openrouter_driver_declares_capabilities_and_preflights_before_outbound_work() {
    let driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
    assert!(driver.capabilities().supports_text());
    assert!(driver.capabilities().supports_reasoning());
    assert!(driver.capabilities().supports_tool_calls());
    assert!(!driver.capabilities().supports_multimodal());
    assert!(!driver.capabilities().supports_vendor_extensions());
    assert_eq!(
        driver
            .preflight(&request(ModelRequestedCapabilitiesDto::new(
                false, true, false, false,
            )))
            .expect_err("multimodal must fail before request translation")
            .code(),
        "unsupported_model_capability"
    );
    assert_eq!(driver.prepared_request_count(), 0);

    assert_eq!(
        driver
            .preflight(&request(ModelRequestedCapabilitiesDto::new(
                false, false, false, true,
            )))
            .expect_err("vendor extensions must fail before request translation")
            .code(),
        "unsupported_model_capability"
    );
    assert_eq!(driver.prepared_request_count(), 0);
}

#[test]
fn openrouter_mapping_normalizes_text_reasoning_usage_finish_error_and_tool_call() {
    let text = OpenRouterDriver::map_fixture_text("hello").expect("text maps");
    assert_eq!(
        serde_json::to_string(&text).expect("serializes"),
        r#"{"kind":"text_delta","content":"hello"}"#
    );
    let reasoning =
        OpenRouterDriver::map_fixture_reasoning("considering context").expect("reasoning maps");
    assert_eq!(
        serde_json::to_string(&reasoning).expect("serializes"),
        r#"{"kind":"reasoning_delta","content":"considering context"}"#
    );
    let usage = OpenRouterDriver::map_fixture_usage(2, 3, 5).expect("usage maps");
    assert!(
        serde_json::to_string(&usage)
            .expect("serializes")
            .contains("reported")
    );
    assert_eq!(
        OpenRouterDriver::map_fixture_finish("tool_calls"),
        FinishReasonDto::ToolCalls
    );
    assert_eq!(
        OpenRouterDriver::map_fixture_finish("unknown"),
        FinishReasonDto::Unknown
    );
    let tool =
        OpenRouterDriver::map_fixture_tool_call("call-1", "inspect", "{}").expect("tool maps");
    assert_eq!(tool.name(), "inspect");
    let error = OpenRouterDriver::map_fixture_error(503, "provider text must not leak")
        .expect("error maps");
    assert_eq!(error.code(), "openrouter_provider_unavailable");
    assert!(
        !serde_json::to_string(&error)
            .expect("serializes")
            .contains("provider text")
    );
}

#[test]
fn openrouter_public_driver_does_not_expose_credential() {
    let driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
    assert!(!format!("{driver:?}").contains(FAKE_CREDENTIAL));
}

#[test]
fn openrouter_driver_translates_all_text_roles_without_network_work() {
    let mut driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
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
        Some(ModelRequestedCapabilitiesDto::new(true, false, true, false)),
    )
    .expect("request is valid");

    driver.prepare_request(&request).expect("request prepares");
    assert_eq!(driver.prepared_request_count(), 1);
}

fn collect_ready(
    mut stream: intention_model::ModelEventStream,
) -> Vec<Result<intention_model::ModelEventDto, intention_model::ProviderErrorDto>> {
    let waker = noop_waker_ref();
    let mut context = Context::from_waker(waker);
    let mut events = Vec::new();
    loop {
        match Pin::new(&mut stream).poll_next(&mut context) {
            Poll::Ready(Some(event)) => events.push(event),
            Poll::Ready(None) => return events,
            Poll::Pending => panic!("fixture stream must resolve without a network request"),
        }
    }
}

#[test]
fn openrouter_execution_cancels_before_stream_creation_without_network_work() {
    let driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
    let cancellation = ModelCancellationSignal::new();
    cancellation.cancel();
    let events = collect_ready(driver.execute(
        request(ModelRequestedCapabilitiesDto::default()),
        cancellation,
    ));
    assert!(events.is_empty());
}

#[test]
fn openrouter_execution_rejects_preflight_before_stream_creation_without_network_work() {
    let driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
    let events = collect_ready(driver.execute(
        request(ModelRequestedCapabilitiesDto::new(
            false, true, false, false,
        )),
        ModelCancellationSignal::new(),
    ));
    assert!(matches!(
        events.as_slice(),
        [Err(error)] if error.code() == "openrouter_request_rejected"
    ));
}

#[test]
fn openrouter_driver_rejects_wrong_kind_and_maps_all_finish_and_error_classes() {
    let wrong_kind = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\nendpoint = \"https://example.invalid/v1\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-relay-openrouter-wrong-kind.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        ),
    ))
    .expect("material resolves");
    assert_eq!(
        OpenRouterDriver::from_startup_material(wrong_kind)
            .expect_err("wrong provider kind fails")
            .code(),
        "invalid_openrouter_provider_config"
    );

    for (native, expected) in [
        ("stop", FinishReasonDto::Stop),
        ("length", FinishReasonDto::Length),
        ("tool_calls", FinishReasonDto::ToolCalls),
        ("function_call", FinishReasonDto::ToolCalls),
        ("content_filter", FinishReasonDto::ContentFilter),
        ("error", FinishReasonDto::Error),
        ("unknown", FinishReasonDto::Unknown),
    ] {
        assert_eq!(OpenRouterDriver::map_fixture_finish(native), expected);
    }
    for status in [400_u16, 429, 500] {
        let error = OpenRouterDriver::map_fixture_error(status, FAKE_CREDENTIAL)
            .expect("error mapping is valid");
        assert!(
            !serde_json::to_string(&error)
                .expect("error serializes")
                .contains(FAKE_CREDENTIAL)
        );
    }
    assert!(OpenRouterDriver::map_fixture_text("").is_err());
    assert!(OpenRouterDriver::map_fixture_reasoning("").is_err());
    assert!(OpenRouterDriver::map_fixture_usage(1, 1, 1).is_err());
    assert!(OpenRouterDriver::map_fixture_tool_call("call", "", "{}").is_err());
    assert!(OpenRouterDriver::map_fixture_tool_call("call", "inspect", "[]").is_err());
}
