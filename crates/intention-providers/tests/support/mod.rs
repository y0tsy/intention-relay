//! Shared fixture builders for the provider contract suites.
//!
//! Cargo does not treat this nested module as a test target: every suite that
//! needs a builder includes it with `mod support;`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "The shared provider fixtures use expect and panic for impossible local failures, and each suite uses its own subset of the builders."
)]

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, task::noop_waker_ref};
use intention_config::{
    ConfigPathDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto, StartupProviderMaterial,
};
use intention_proto::RunId;
use intention_providers::{
    ModelEventDto, ModelEventStream, ModelMessageDto, ModelRequestDto,
    ModelRequestedCapabilitiesDto, ModelRoleDto, ProviderErrorDto,
};

/// The fake credential every provider fixture carries instead of a real key.
pub const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";

/// Builds startup provider material from one `[provider]` table body.
pub fn startup_material(provider_body: &str, file_name: &str) -> StartupProviderMaterial {
    ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        format!("schema_version = 1\n[provider]\n{provider_body}"),
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join(file_name)
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        ),
    ))
    .expect("fixture config resolves")
}

/// Builds one driver request with the given requested capabilities and system context.
pub fn capability_request(capabilities: ModelRequestedCapabilitiesDto) -> ModelRequestDto {
    ModelRequestDto::new(
        RunId::new(),
        "fixture",
        vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
        Some("system".to_owned()),
        Some(capabilities),
    )
    .expect("request is valid")
}

/// Builds one plain request without requested capabilities or system context.
pub fn plain_request() -> ModelRequestDto {
    ModelRequestDto::new(
        RunId::new(),
        "fixture-model",
        vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
        None,
        None,
    )
    .expect("request is valid")
}

/// Polls a locally resolvable stream to completion.
pub fn collect_ready(mut stream: ModelEventStream) -> Vec<Result<ModelEventDto, ProviderErrorDto>> {
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
