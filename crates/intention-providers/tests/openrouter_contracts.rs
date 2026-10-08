#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Provider contract fixtures use explicit failure messages for impossible pending local streams."
)]

mod support;

use intention_config::StartupProviderMaterial;
use intention_providers::OpenRouterDriver;
use intention_providers::{
    ModelCancellationSignal, ModelDriver, ModelExecutionDriver, ModelRequestedCapabilitiesDto,
};
use support::{FAKE_CREDENTIAL, capability_request, collect_ready, startup_material};

fn material() -> StartupProviderMaterial {
    startup_material(
        &format!("kind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{FAKE_CREDENTIAL}\""),
        "intention-relay-openrouter.toml",
    )
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
            .preflight(&capability_request(ModelRequestedCapabilitiesDto::new(
                false, true, false, false,
            )))
            .expect_err("multimodal must fail before request translation")
            .code(),
        "unsupported_model_capability"
    );

    assert_eq!(
        driver
            .preflight(&capability_request(ModelRequestedCapabilitiesDto::new(
                false, false, false, true,
            )))
            .expect_err("vendor extensions must fail before request translation")
            .code(),
        "unsupported_model_capability"
    );
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
    let events = collect_ready(driver.execute(
        capability_request(ModelRequestedCapabilitiesDto::default()),
        cancellation,
    ));
    assert!(events.is_empty());
}

#[test]
fn openrouter_execution_rejects_preflight_before_stream_creation_without_network_work() {
    let driver = OpenRouterDriver::from_startup_material(material()).expect("driver builds");
    let events = collect_ready(driver.execute(
        capability_request(ModelRequestedCapabilitiesDto::new(
            false, true, false, false,
        )),
        ModelCancellationSignal::new(),
    ));
    assert!(matches!(
        events.as_slice(),
        [Err(error)] if error.code() == "openrouter_request_rejected"
    ));
}
