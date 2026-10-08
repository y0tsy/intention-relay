#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Provider contract fixtures use explicit failure messages for impossible pending local streams."
)]

mod support;

use intention_config::StartupProviderMaterial;
use intention_providers::OpenRouterDriver;
use intention_providers::{ModelCancellationSignal, ModelExecutionDriver};
use support::{FAKE_CREDENTIAL, collect_ready, plain_request, startup_material};

fn material() -> StartupProviderMaterial {
    startup_material(
        &format!("kind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\""),
        "intention-relay-openrouter.toml",
    )
}

#[test]
fn openrouter_driver_declares_capabilities() {
    let driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
    assert!(driver.capabilities().supports_text());
    assert!(driver.capabilities().supports_reasoning());
    assert!(driver.capabilities().supports_tool_calls());
    assert!(!driver.capabilities().supports_multimodal());
    assert!(!driver.capabilities().supports_vendor_extensions());
}

#[test]
fn openrouter_public_driver_does_not_expose_credential() {
    let driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
    assert!(!format!("{driver:?}").contains(FAKE_CREDENTIAL));
}

#[test]
fn openrouter_execution_cancels_before_stream_creation_without_network_work() {
    let driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
    let cancellation = ModelCancellationSignal::new();
    cancellation.cancel();
    let events = collect_ready(driver.execute(plain_request(), cancellation));
    assert!(events.is_empty());
}
