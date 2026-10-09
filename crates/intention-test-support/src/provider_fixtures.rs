//! Deterministic provider catalog, selection, and typed-history fixtures.
//!
//! The fixtures build credential-free Slice 2 values from the code-owned
//! first-party descriptors, so engine and daemon suites share one definition of
//! a valid exact selection and of a typed cross-turn reasoning history.

use intention_proto::provider::{
    CredentialTransportDto, LoopbackPolicyDto, ModelCapabilitySetV1,
    ProviderDriverContractRevisionDto, ProviderExecutionPolicyDto,
    ProviderKindDescriptorRevisionV1, ProviderProfileId, ProviderProfileRevisionId,
    ProviderProfileRevisionV1, ProviderSelectionSourceDto, ReasoningCapabilityDto,
    ReasoningEffortLevelDto, ReasoningFragmentCategoryDto, ReasoningHistoryTransferDto,
    ResolvedReasoningPolicyDto, ResolvedRunProviderSelectionDto,
};
use intention_proto::{DtoResult, ErrorDto};
use intention_providers::{
    AssistantReasoningHistoryDto, driver_contract, first_party_kind_descriptors,
    validate_capability_subset,
};

/// Returns the code-owned descriptor of one first-party kind.
///
/// # Errors
///
/// Returns the descriptor validation failure of the code-owned declaration,
/// which the constants already satisfy.
fn first_party_descriptor(kind_id: &str) -> DtoResult<ProviderKindDescriptorRevisionV1> {
    first_party_kind_descriptors()?
        .into_iter()
        .find(|descriptor| descriptor.kind_id().as_str() == kind_id)
        .ok_or_else(|| {
            ErrorDto::validation(
                "invalid_provider_kind",
                "the fixture kind is not a first-party kind",
            )
        })
}

/// Returns the declared exact-model capability subset of one fixture profile.
///
/// # Errors
///
/// Returns the typed validation failure of the fixture declaration.
fn fixture_capabilities(
    descriptor: &ProviderKindDescriptorRevisionV1,
) -> DtoResult<ModelCapabilitySetV1> {
    let envelope = descriptor.model_capability_envelope();
    let reasoning = match envelope.reasoning() {
        ReasoningCapabilityDto::Disabled => ReasoningCapabilityDto::Disabled,
        ReasoningCapabilityDto::TextualReasoningV1 {
            summary_support, ..
        } => ReasoningCapabilityDto::textual_reasoning_v1(
            vec![ReasoningEffortLevelDto::Medium],
            *summary_support,
        )?,
    };
    validate_capability_subset(
        descriptor,
        &ModelCapabilitySetV1::new(
            envelope.taxonomy_version(),
            envelope.input(),
            envelope.text_streaming(),
            envelope.structured_output(),
            reasoning,
            envelope.tool_exchange().clone(),
            envelope.context_preservation().clone(),
        )?,
    )
}

/// Returns one deterministic first-party fixture profile revision.
///
/// The revision binds the profile to the code-owned descriptor of its kind, so
/// it is exactly the shape the catalog path produces.
///
/// # Errors
///
/// Returns the typed validation failure of the fixture declaration.
pub fn fixture_profile_revision(
    profile_id: &str,
    kind_id: &str,
    model: &str,
) -> DtoResult<ProviderProfileRevisionV1> {
    let descriptor = first_party_descriptor(kind_id)?;
    let capabilities = fixture_capabilities(&descriptor)?;
    let transfer = capabilities.reasoning_input_contract().clone();
    let supported_categories = if matches!(transfer, ReasoningHistoryTransferDto::Disabled) {
        vec![ReasoningFragmentCategoryDto::Primary]
    } else {
        vec![
            ReasoningFragmentCategoryDto::Primary,
            ReasoningFragmentCategoryDto::Detail,
        ]
    };
    ProviderProfileRevisionV1::new(
        ProviderProfileId::parse(profile_id)?,
        ProviderProfileRevisionId::new(),
        descriptor.kind_id().clone(),
        descriptor.descriptor_revision_id(),
        model,
        None,
        CredentialTransportDto::bearer(),
        capabilities.clone(),
        ResolvedReasoningPolicyDto::new(
            Some(ReasoningEffortLevelDto::Medium),
            capabilities.supports_summary(),
            transfer,
            supported_categories,
        )?,
        ProviderExecutionPolicyDto::new(30, 2)?,
        LoopbackPolicyDto::NotApplicable,
    )
}

/// Returns the exact fixture provider selection of one first-party profile.
///
/// # Errors
///
/// Returns the typed validation failure of the fixture declaration.
pub fn fixture_selection(
    profile_id: &str,
    kind_id: &str,
    model: &str,
) -> DtoResult<ResolvedRunProviderSelectionDto> {
    let revision = fixture_profile_revision(profile_id, kind_id, model)?;
    let contract = driver_contract(revision.kind_id()).ok_or_else(|| {
        ErrorDto::validation(
            "provider_driver_contract_incompatible",
            "the fixture kind has no code-owned driver contract",
        )
    })?;
    Ok(ResolvedRunProviderSelectionDto::from_profile_revision(
        &revision,
        contract,
        ProviderSelectionSourceDto::GlobalDefault,
    ))
}

/// Returns the code-owned driver contract revision of one fixture kind.
///
/// # Errors
///
/// Returns a typed validation failure when the fixture kind is not first-party.
pub fn fixture_driver_contract(kind_id: &str) -> DtoResult<ProviderDriverContractRevisionDto> {
    let descriptor = first_party_descriptor(kind_id)?;
    driver_contract(descriptor.kind_id()).ok_or_else(|| {
        ErrorDto::validation(
            "provider_driver_contract_incompatible",
            "the fixture kind has no code-owned driver contract",
        )
    })
}

/// Returns one typed fixture cross-turn reasoning history.
///
/// The compatibility identity must be the descriptor transfer identity of the
/// consuming capability contract, exactly like a real verified transfer.
///
/// # Errors
///
/// Returns the typed validation failure of the history declaration.
pub fn fixture_reasoning_history(
    compatibility_id: &str,
    fragments: &[(ReasoningFragmentCategoryDto, &str)],
    summaries: &[&str],
) -> DtoResult<AssistantReasoningHistoryDto> {
    AssistantReasoningHistoryDto::new(
        compatibility_id,
        fragments
            .iter()
            .map(|(category, fragment)| (*category, (*fragment).to_owned()))
            .collect(),
        summaries
            .iter()
            .map(|summary| (*summary).to_owned())
            .collect(),
    )
}
