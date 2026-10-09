#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Provider contract fixtures use explicit failure messages for impossible pending local streams."
)]

mod support;

use intention_config::StartupProviderMaterial;
use intention_providers::{
    GenericChatDriver, ModelCancellationSignal, ModelExecutionDriver, OpenRouterDriver,
};
use support::{FAKE_CREDENTIAL, collect_ready, plain_request, startup_material};

fn generic_material() -> StartupProviderMaterial {
    startup_material(
        &format!(
            "kind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\nendpoint = \"https://example.invalid/v1\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
        "intention-relay-generic.toml",
    )
}

fn openrouter_material() -> StartupProviderMaterial {
    startup_material(
        &format!("kind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\""),
        "intention-relay-openrouter.toml",
    )
}

/// One configured adapter under the mirrored adapter contract cases.
struct Adapter {
    name: &'static str,
    debug: String,
    driver: Box<dyn ModelExecutionDriver>,
}

fn adapters() -> Vec<Adapter> {
    let generic =
        GenericChatDriver::from_startup_material(generic_material()).expect("driver builds");
    let openrouter =
        OpenRouterDriver::from_startup_material(openrouter_material()).expect("driver builds");
    vec![
        Adapter {
            name: "generic-chat",
            debug: format!("{generic:?}"),
            driver: Box::new(generic),
        },
        Adapter {
            name: "openrouter",
            debug: format!("{openrouter:?}"),
            driver: Box::new(openrouter),
        },
    ]
}

#[test]
fn adapters_declare_the_supported_subset() {
    for adapter in adapters() {
        let capabilities = adapter.driver.capabilities();
        assert!(capabilities.supports_text(), "{}", adapter.name);
        assert!(capabilities.supports_tool_calls(), "{}", adapter.name);
        assert!(capabilities.supports_reasoning(), "{}", adapter.name);
        assert!(!capabilities.supports_multimodal(), "{}", adapter.name);
        assert!(
            !capabilities.supports_vendor_extensions(),
            "{}",
            adapter.name
        );
    }
}

#[test]
fn adapters_do_not_expose_the_credential() {
    for adapter in adapters() {
        assert!(!adapter.debug.contains(FAKE_CREDENTIAL), "{}", adapter.name);
    }
}

#[test]
fn adapters_cancel_before_stream_creation_without_network_work() {
    for adapter in adapters() {
        let cancellation = ModelCancellationSignal::new();
        cancellation.cancel();
        let events = collect_ready(adapter.driver.execute(plain_request(), cancellation));
        assert!(events.is_empty(), "{}", adapter.name);
    }
}

#[test]
fn generic_chat_rejects_wrong_kind_and_missing_endpoint_safely() {
    let wrong_kind = startup_material(
        &format!("kind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\""),
        "intention-relay-generic-wrong-kind.toml",
    );
    assert_eq!(
        GenericChatDriver::from_startup_material(wrong_kind)
            .expect_err("wrong provider kind fails")
            .code(),
        "invalid_generic_chat_provider_config"
    );

    let missing_endpoint = startup_material(
        &format!(
            "kind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
        "intention-relay-generic-no-endpoint.toml",
    );
    assert_eq!(
        GenericChatDriver::from_startup_material(missing_endpoint)
            .expect_err("missing endpoint fails")
            .code(),
        "missing_generic_chat_endpoint"
    );
}
