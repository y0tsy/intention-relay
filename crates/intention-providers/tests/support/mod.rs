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
use intention_proto::provider::{
    ContextPreservationCapabilityDto, CredentialTransportDto, LoopbackPolicyDto,
    ModelCapabilitySetV1, ModelCapabilityTaxonomyVersionDto, ModelInputKindDto,
    ProviderCapabilityAvailabilityDto, ProviderExecutionPolicyDto,
    ProviderKindDescriptorRevisionV1, ProviderKindId, ProviderProfileId, ProviderProfileRevisionV1,
    ReasoningCapabilityDto, ReasoningEffortLevelDto, ReasoningHistoryTransferDto,
    ResolvedReasoningPolicyDto, ToolExchangeCapabilityDto,
};
use intention_proto::{ProviderProfileRevisionId, ReasoningFragmentCategoryDto, RunId};
use intention_providers::{
    ModelEventDto, ModelEventStream, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto, first_party_kind_descriptors,
};

/// The fake credential every provider fixture carries instead of a real key.
pub const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";

/// Returns the code-owned descriptor of one first-party kind.
pub fn first_party_descriptor(kind_id: &str) -> ProviderKindDescriptorRevisionV1 {
    first_party_kind_descriptors()
        .expect("code-owned first-party descriptors validate")
        .into_iter()
        .find(|descriptor| descriptor.kind_id().as_str() == kind_id)
        .expect("the requested first-party kind is declared")
}

/// Builds one exact profile revision for a first-party kind.
pub fn profile_revision(
    kind_id: &str,
    model_id: &str,
    endpoint: Option<&str>,
    credential_transport: CredentialTransportDto,
) -> ProviderProfileRevisionV1 {
    let transfer = first_party_descriptor(kind_id)
        .model_capability_envelope()
        .reasoning_input_contract()
        .clone();
    build_profile_revision(
        kind_id,
        model_id,
        endpoint,
        credential_transport,
        transfer,
        LoopbackPolicyDto::NotApplicable,
    )
}

/// Builds one exact profile revision with an explicit reasoning history transfer.
pub fn profile_revision_with_transfer(
    kind_id: &str,
    model_id: &str,
    endpoint: Option<&str>,
    credential_transport: CredentialTransportDto,
    transfer: ReasoningHistoryTransferDto,
) -> ProviderProfileRevisionV1 {
    build_profile_revision(
        kind_id,
        model_id,
        endpoint,
        credential_transport,
        transfer,
        LoopbackPolicyDto::NotApplicable,
    )
}

fn build_profile_revision(
    kind_id: &str,
    model_id: &str,
    endpoint: Option<&str>,
    credential_transport: CredentialTransportDto,
    transfer: ReasoningHistoryTransferDto,
    loopback_policy: LoopbackPolicyDto,
) -> ProviderProfileRevisionV1 {
    let descriptor = first_party_descriptor(kind_id);
    let envelope = descriptor.model_capability_envelope();
    let subset = ModelCapabilitySetV1::new(
        ModelCapabilityTaxonomyVersionDto::current(),
        ModelInputKindDto::TextOnly,
        ProviderCapabilityAvailabilityDto::Enabled,
        ProviderCapabilityAvailabilityDto::Disabled,
        ReasoningCapabilityDto::textual_reasoning_v1(vec![ReasoningEffortLevelDto::Medium], false)
            .expect("fixture reasoning subset is valid"),
        ToolExchangeCapabilityDto::model_tool_loop_v1(
            envelope
                .tool_exchange()
                .translation_revision()
                .expect("first-party envelopes declare the model tool loop"),
        )
        .expect("fixture tool-loop subset is valid"),
        ContextPreservationCapabilityDto::local_durable_history_v1(transfer.clone()),
    )
    .expect("fixture capability subset is valid");
    let resolved_reasoning_policy = ResolvedReasoningPolicyDto::new(
        Some(ReasoningEffortLevelDto::Medium),
        false,
        transfer,
        vec![ReasoningFragmentCategoryDto::Primary],
    )
    .expect("fixture reasoning policy is valid");
    ProviderProfileRevisionV1::new(
        ProviderProfileId::parse("fixture-profile").expect("fixture profile identity is valid"),
        ProviderProfileRevisionId::new(),
        ProviderKindId::parse(kind_id).expect("fixture kind identity is valid"),
        descriptor.descriptor_revision_id(),
        model_id,
        endpoint.map(str::to_owned),
        credential_transport,
        subset,
        resolved_reasoning_policy,
        ProviderExecutionPolicyDto::new(30, 2).expect("fixture execution policy is valid"),
        loopback_policy,
    )
    .expect("fixture profile revision is valid")
}

/// Builds one plain driver request without system context.
pub fn plain_request() -> ModelRequestDto {
    ModelRequestDto::new(
        RunId::new(),
        "fixture-model",
        vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
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
