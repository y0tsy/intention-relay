#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Provider contract fixtures use explicit failure messages for impossible pending local streams."
)]

mod support;

use intention_config::StartupProviderMaterial;
use intention_providers::GenericChatDriver;
use intention_providers::{ModelCancellationSignal, ModelExecutionDriver};
use support::{FAKE_CREDENTIAL, collect_ready, plain_request, startup_material};

fn material() -> StartupProviderMaterial {
    startup_material(
        &format!(
            "kind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\nendpoint = \"https://example.invalid/v1\"\ncredential = \"{FAKE_CREDENTIAL}\""
        ),
        "intention-relay-generic.toml",
    )
}

#[test]
fn generic_driver_declares_supported_subset() {
    let driver = GenericChatDriver::from_startup_material(material()).expect("driver builds");
    assert!(driver.capabilities().supports_text());
    assert!(driver.capabilities().supports_tool_calls());
    assert!(driver.capabilities().supports_reasoning());
    assert!(!driver.capabilities().supports_multimodal());
    assert!(!driver.capabilities().supports_vendor_extensions());
}

#[test]
fn generic_public_driver_does_not_expose_credential() {
    let driver = GenericChatDriver::from_startup_material(material()).expect("driver builds");
    let debug = format!("{driver:?}");
    assert!(!debug.contains(FAKE_CREDENTIAL));
}

#[test]
fn generic_execution_cancels_before_stream_creation_without_network_work() {
    let driver = GenericChatDriver::from_startup_material(material()).expect("driver builds");
    let cancellation = ModelCancellationSignal::new();
    cancellation.cancel();
    let events = collect_ready(driver.execute(plain_request(), cancellation));
    assert!(events.is_empty());
}

#[test]
fn generic_driver_rejects_wrong_kind_and_missing_endpoint_safely() {
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
