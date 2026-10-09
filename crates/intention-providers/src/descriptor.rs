//! Code-owned first-party provider kind descriptors and the compatibility matrix.
//!
//! The descriptor revision, driver contract, capability envelope, endpoint
//! policy, and credential transport contract of each first-party provider kind
//! are code-owned facts. A catalog validates declared profiles and user-kind
//! compositions against them and never infers any of them from a model name,
//! endpoint, driver family, or configuration text.

use intention_proto::provider::{
    ContextPreservationCapabilityDto, CredentialTransportContractDto, CredentialTransportDto,
    CredentialTransportModeDto, ModelCapabilitySetV1, ModelCapabilityTaxonomyVersionDto,
    ModelInputKindDto, ProviderCapabilityAvailabilityDto, ProviderDriverContractRevisionDto,
    ProviderEndpointPolicyDto, ProviderKindDescriptorRevisionId, ProviderKindDescriptorRevisionV1,
    ProviderKindId, ReasoningCapabilityDto, ReasoningEffortLevelDto, ReasoningHistoryTransferDto,
    ToolExchangeCapabilityDto, UserKindActivationPartDto, UserKindCompositionDto,
    UserKindEffortPartDto, UserKindReasoningPartDto, UserKindStreamPartDto,
};
use intention_proto::{DtoResult, ErrorDto};

use crate::model::ModelCapabilitiesDto;

/// The reserved first-party OpenRouter kind identity.
pub const OPENROUTER_KIND_ID: &str = "openrouter";

/// The reserved first-party generic Chat Completions kind identity.
pub const GENERIC_CHAT_KIND_ID: &str = "generic-chat-completion-api";

/// The code-owned driver contract family of the OpenRouter adapter.
const OPENROUTER_DRIVER_FAMILY: &str = "openrouter-chat-completions";

/// The code-owned driver contract family of the generic Chat Completions adapter.
const GENERIC_CHAT_DRIVER_FAMILY: &str = "generic-chat-completions";

/// The one driver contract revision both first-party adapters implement.
const FIRST_PARTY_DRIVER_MAJOR: u16 = 1;

/// The compatible-on-change component of the first-party driver contract.
const FIRST_PARTY_DRIVER_MINOR: u16 = 0;

/// The code-owned descriptor revision identity of the OpenRouter kind.
const OPENROUTER_DESCRIPTOR_REVISION_ID: &str = "6d1b3a9c-0f4e-4c1a-9b2a-1f0a0c0e0001";

/// The code-owned descriptor revision identity of the generic Chat Completions kind.
const GENERIC_CHAT_DESCRIPTOR_REVISION_ID: &str = "6d1b3a9c-0f4e-4c1a-9b2a-1f0a0c0e0002";

/// The closed reasoning effort levels every first-party envelope can declare.
const CLOSED_REASONING_EFFORTS: [ReasoningEffortLevelDto; 7] = [
    ReasoningEffortLevelDto::None,
    ReasoningEffortLevelDto::Minimal,
    ReasoningEffortLevelDto::Low,
    ReasoningEffortLevelDto::Medium,
    ReasoningEffortLevelDto::High,
    ReasoningEffortLevelDto::XHigh,
    ReasoningEffortLevelDto::Max,
];

/// The code-owned model-tool-loop translation revision of both first-party kinds.
const MODEL_TOOL_LOOP_TRANSLATION_REVISION: &str = "chat-completions-tool-loop-v1";

/// The code-owned reasoning compatibility identity of the generic adapter.
const GENERIC_CHAT_REASONING_COMPATIBILITY_ID: &str = "generic-chat-reasoning-content-v1";

/// The code-owned descriptor family of every composed user kind.
const USER_DESCRIPTOR_FAMILY: &str = "user-kind-descriptor-v1";

/// The code-owned driver family every composed user kind declares.
///
/// No code-owned driver serves a user kind in this slice, so the descriptor
/// names a driverless family instead of borrowing a first-party one: dispatch
/// that resolved the family textually could otherwise reach the generic
/// adapter, which a user kind must never fall back to.
const USER_KIND_DRIVER_FAMILY: &str = "user-kind-no-code-owned-driver-v1";

/// The closed stream-framing part revision every user kind declares.
const USER_STREAM_PART_REVISION: &str = "chat-completions-sse-v1";

/// The local reduction seed of the deterministic user-kind descriptor identity.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// The local reduction prime of the deterministic user-kind descriptor identity.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Returns the code-owned descriptor of every first-party provider kind.
///
/// # Errors
///
/// Returns a validation error when a code-owned descriptor token is not a safe
/// token; the constants are safe by construction, so this is a propagation
/// boundary rather than a live failure path.
pub fn first_party_kind_descriptors() -> DtoResult<Vec<ProviderKindDescriptorRevisionV1>> {
    Ok(vec![
        openrouter_kind_descriptor()?,
        generic_chat_kind_descriptor()?,
    ])
}

/// Returns the code-owned driver contract of `kind_id`, when one exists.
///
/// A user kind has no code-owned driver, so it has no driver contract: the
/// capability-subset check blocks it before any effect instead of falling back
/// to a first-party driver.
#[must_use]
pub fn driver_contract(kind_id: &ProviderKindId) -> Option<ProviderDriverContractRevisionDto> {
    let family = first_party_driver_family(kind_id)?;
    Some(first_party_driver_contract(family))
}

/// Returns the static capability declaration of the code-owned driver of `kind_id`.
///
/// A kind without a code-owned driver declares no capability at all, so a
/// caller that ignores the missing driver contract still never reads an
/// invented capability.
#[must_use]
pub fn driver_capabilities(kind_id: &ProviderKindId) -> ModelCapabilitiesDto {
    if driver_contract(kind_id).is_some() {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    } else {
        ModelCapabilitiesDto::new(false, false, false, false, false, false)
    }
}

/// Validates one declared capability subset against a kind descriptor.
///
/// The returned set is exactly `descriptor envelope ∩ declared exact-model
/// subset`: a component the envelope cannot represent blocks before effect,
/// and the code-owned driver contract of the descriptor's kind must declare
/// support for every enabled component.
///
/// # Errors
///
/// Returns a typed error when the taxonomy version differs, the declared
/// subset is outside the envelope, or no supported driver contract can serve
/// the resolved set.
pub fn validate_capability_subset(
    descriptor: &ProviderKindDescriptorRevisionV1,
    subset: &ModelCapabilitySetV1,
) -> DtoResult<ModelCapabilitySetV1> {
    let contract =
        driver_contract(descriptor.kind_id()).ok_or_else(driver_contract_incompatible)?;
    if descriptor.driver_contract_family() != contract.driver_family() {
        return Err(driver_contract_incompatible());
    }
    let intersection = capability_intersection(descriptor.model_capability_envelope(), subset)?;
    ensure_driver_covers(driver_capabilities(descriptor.kind_id()), &intersection)?;
    Ok(intersection)
}

/// Validates one closed user-kind composition through the compatibility matrix.
///
/// The composition is admitted as an immutable descriptor: closed parts only,
/// no raw template, header map, or secret interpolation. Admission does not
/// authorize execution: a user kind has no code-owned driver contract, so
/// [`validate_capability_subset`] blocks every run against its descriptor
/// before any effect.
///
/// # Errors
///
/// Returns a validation error when a code-owned descriptor token or envelope
/// is not representable; the closed parts are validated by their own
/// constructors before they arrive here.
pub fn validate_user_kind(
    parts: &UserKindCompositionDto,
) -> DtoResult<ProviderKindDescriptorRevisionV1> {
    ProviderKindDescriptorRevisionV1::new(
        parts.kind_id().clone(),
        user_descriptor_revision_id(parts)?,
        USER_DESCRIPTOR_FAMILY,
        user_protocol_part_revisions(parts),
        ProviderEndpointPolicyDto::new(true, true, true),
        user_credential_transport_contract(parts.credential_transport())?,
        user_capability_envelope(parts)?,
        USER_KIND_DRIVER_FAMILY,
    )
}

/// Returns the descriptor of the first-party OpenRouter kind.
fn openrouter_kind_descriptor() -> DtoResult<ProviderKindDescriptorRevisionV1> {
    ProviderKindDescriptorRevisionV1::new(
        ProviderKindId::parse(OPENROUTER_KIND_ID)?,
        ProviderKindDescriptorRevisionId::parse(OPENROUTER_DESCRIPTOR_REVISION_ID)?,
        "openrouter-chat-completions-descriptor-v1",
        vec![
            "chat-completions-sse-v1".to_owned(),
            "reasoning-field-reasoning-v1".to_owned(),
        ],
        // OpenRouter has no first-scope endpoint override; the pinned SDK's own
        // default base URL stays the only endpoint.
        ProviderEndpointPolicyDto::new(false, false, false),
        CredentialTransportContractDto::new(vec![CredentialTransportModeDto::Bearer], None)?,
        ModelCapabilitySetV1::new(
            ModelCapabilityTaxonomyVersionDto::current(),
            ModelInputKindDto::TextOnly,
            ProviderCapabilityAvailabilityDto::Enabled,
            ProviderCapabilityAvailabilityDto::Disabled,
            ReasoningCapabilityDto::textual_reasoning_v1(CLOSED_REASONING_EFFORTS.to_vec(), false)?,
            ToolExchangeCapabilityDto::model_tool_loop_v1(MODEL_TOOL_LOOP_TRANSLATION_REVISION)?,
            // The pinned OpenRouter chat SDK has no assistant-message reasoning
            // echo field, so no cross-turn reasoning history can be accepted.
            ContextPreservationCapabilityDto::local_durable_history_v1(
                ReasoningHistoryTransferDto::Disabled,
            ),
        )?,
        OPENROUTER_DRIVER_FAMILY,
    )
}

/// Returns the descriptor of the first-party generic Chat Completions kind.
fn generic_chat_kind_descriptor() -> DtoResult<ProviderKindDescriptorRevisionV1> {
    ProviderKindDescriptorRevisionV1::new(
        ProviderKindId::parse(GENERIC_CHAT_KIND_ID)?,
        ProviderKindDescriptorRevisionId::parse(GENERIC_CHAT_DESCRIPTOR_REVISION_ID)?,
        "generic-chat-completions-descriptor-v1",
        vec![
            "chat-completions-sse-v1".to_owned(),
            "reasoning-field-reasoning-content-v1".to_owned(),
        ],
        ProviderEndpointPolicyDto::new(true, true, true),
        CredentialTransportContractDto::new(vec![CredentialTransportModeDto::Bearer], None)?,
        ModelCapabilitySetV1::new(
            ModelCapabilityTaxonomyVersionDto::current(),
            ModelInputKindDto::TextOnly,
            ProviderCapabilityAvailabilityDto::Enabled,
            ProviderCapabilityAvailabilityDto::Disabled,
            ReasoningCapabilityDto::textual_reasoning_v1(CLOSED_REASONING_EFFORTS.to_vec(), false)?,
            ToolExchangeCapabilityDto::model_tool_loop_v1(MODEL_TOOL_LOOP_TRANSLATION_REVISION)?,
            // The generic adapter is the live cross-turn translation path: it
            // consumes and preserves the typed `reasoning_content` field.
            ContextPreservationCapabilityDto::local_durable_history_v1(
                ReasoningHistoryTransferDto::textual_history_v1(
                    GENERIC_CHAT_REASONING_COMPATIBILITY_ID,
                )?,
            ),
        )?,
        GENERIC_CHAT_DRIVER_FAMILY,
    )
}

/// Returns the code-owned driver family of one first-party kind.
fn first_party_driver_family(kind_id: &ProviderKindId) -> Option<&'static str> {
    match kind_id.as_str() {
        OPENROUTER_KIND_ID => Some(OPENROUTER_DRIVER_FAMILY),
        GENERIC_CHAT_KIND_ID => Some(GENERIC_CHAT_DRIVER_FAMILY),
        _ => None,
    }
}

#[allow(
    clippy::expect_used,
    reason = "Code-owned driver contract families are safe tokens by construction, so validation cannot fail."
)]
fn first_party_driver_contract(family: &'static str) -> ProviderDriverContractRevisionDto {
    ProviderDriverContractRevisionDto::new(
        family,
        FIRST_PARTY_DRIVER_MAJOR,
        FIRST_PARTY_DRIVER_MINOR,
    )
    .expect("code-owned driver contract families are safe tokens")
}

/// Returns the intersection of one descriptor envelope and one declared subset.
///
/// # Errors
///
/// Returns a validation error when the subset declares a component the
/// envelope cannot represent; the intersection never silently narrows such a
/// declaration.
fn capability_intersection(
    envelope: &ModelCapabilitySetV1,
    subset: &ModelCapabilitySetV1,
) -> DtoResult<ModelCapabilitySetV1> {
    if subset.taxonomy_version() != envelope.taxonomy_version() {
        return Err(invalid_capability_subset(
            "the declared subset taxonomy version is not the descriptor taxonomy version",
        ));
    }
    let text_streaming =
        availability_intersection(envelope.text_streaming(), subset.text_streaming())?;
    let structured_output =
        availability_intersection(envelope.structured_output(), subset.structured_output())?;
    let reasoning = reasoning_intersection(envelope.reasoning(), subset.reasoning())?;
    let tool_exchange =
        tool_exchange_intersection(envelope.tool_exchange(), subset.tool_exchange())?;
    let context_preservation = context_preservation_intersection(
        envelope.context_preservation(),
        subset.context_preservation(),
    )?;
    ModelCapabilitySetV1::new(
        envelope.taxonomy_version(),
        subset.input(),
        text_streaming,
        structured_output,
        reasoning,
        tool_exchange,
        context_preservation,
    )
}

/// Returns the intersection of one availability pair.
fn availability_intersection(
    envelope: ProviderCapabilityAvailabilityDto,
    subset: ProviderCapabilityAvailabilityDto,
) -> DtoResult<ProviderCapabilityAvailabilityDto> {
    if subset == ProviderCapabilityAvailabilityDto::Enabled
        && envelope == ProviderCapabilityAvailabilityDto::Disabled
    {
        return Err(invalid_capability_subset(
            "the declared subset enables a capability the descriptor envelope does not represent",
        ));
    }
    Ok(subset)
}

/// Returns the intersection of one reasoning declaration pair.
fn reasoning_intersection(
    envelope: &ReasoningCapabilityDto,
    subset: &ReasoningCapabilityDto,
) -> DtoResult<ReasoningCapabilityDto> {
    match (envelope, subset) {
        (_, ReasoningCapabilityDto::Disabled) => Ok(ReasoningCapabilityDto::Disabled),
        (ReasoningCapabilityDto::Disabled, ReasoningCapabilityDto::TextualReasoningV1 { .. }) => {
            Err(invalid_capability_subset(
                "the declared subset selects textual reasoning the descriptor envelope does not represent",
            ))
        }
        (
            ReasoningCapabilityDto::TextualReasoningV1 {
                supported_efforts: envelope_efforts,
                summary_support: envelope_summary,
            },
            ReasoningCapabilityDto::TextualReasoningV1 {
                supported_efforts,
                summary_support,
            },
        ) => {
            if !supported_efforts
                .iter()
                .all(|effort| envelope_efforts.contains(effort))
            {
                return Err(invalid_capability_subset(
                    "the declared subset selects a reasoning effort the descriptor envelope does not represent",
                ));
            }
            if *summary_support && !envelope_summary {
                return Err(invalid_capability_subset(
                    "the declared subset enables reasoning summaries the descriptor envelope does not represent",
                ));
            }
            Ok(ReasoningCapabilityDto::textual_reasoning_v1(
                supported_efforts.clone(),
                *summary_support,
            )?)
        }
    }
}

/// Returns the intersection of one tool-exchange declaration pair.
fn tool_exchange_intersection(
    envelope: &ToolExchangeCapabilityDto,
    subset: &ToolExchangeCapabilityDto,
) -> DtoResult<ToolExchangeCapabilityDto> {
    match (envelope, subset) {
        (_, ToolExchangeCapabilityDto::Disabled) => Ok(ToolExchangeCapabilityDto::Disabled),
        (
            ToolExchangeCapabilityDto::Disabled,
            ToolExchangeCapabilityDto::ModelToolLoopV1 { .. },
        ) => Err(invalid_capability_subset(
            "the declared subset selects a model tool loop the descriptor envelope does not represent",
        )),
        (
            ToolExchangeCapabilityDto::ModelToolLoopV1 {
                translation_revision: envelope_revision,
            },
            ToolExchangeCapabilityDto::ModelToolLoopV1 {
                translation_revision,
            },
        ) => {
            if envelope_revision != translation_revision {
                return Err(invalid_capability_subset(
                    "the declared subset selects a tool-loop translation the descriptor envelope does not represent",
                ));
            }
            Ok(ToolExchangeCapabilityDto::model_tool_loop_v1(
                translation_revision.clone(),
            )?)
        }
    }
}

/// Returns the intersection of one context-preservation declaration pair.
fn context_preservation_intersection(
    envelope: &ContextPreservationCapabilityDto,
    subset: &ContextPreservationCapabilityDto,
) -> DtoResult<ContextPreservationCapabilityDto> {
    if envelope.reasoning_input_contract() != subset.reasoning_input_contract() {
        return Err(invalid_capability_subset(
            "the declared subset selects a reasoning input contract the descriptor envelope does not represent",
        ));
    }
    Ok(ContextPreservationCapabilityDto::local_durable_history_v1(
        subset.reasoning_input_contract().clone(),
    ))
}

/// Verifies that one code-owned driver declaration covers the resolved set.
fn ensure_driver_covers(
    driver: ModelCapabilitiesDto,
    resolved: &ModelCapabilitySetV1,
) -> DtoResult<()> {
    let text_streaming = resolved.text_streaming() != ProviderCapabilityAvailabilityDto::Disabled;
    if text_streaming && !(driver.supports_text() && driver.supports_streaming()) {
        return Err(driver_contract_incompatible());
    }
    if matches!(
        resolved.reasoning(),
        ReasoningCapabilityDto::TextualReasoningV1 { .. }
    ) && !driver.supports_reasoning()
    {
        return Err(driver_contract_incompatible());
    }
    if resolved.supports_summary() && !driver.supports_reasoning_summary() {
        return Err(driver_contract_incompatible());
    }
    if matches!(
        resolved.tool_exchange(),
        ToolExchangeCapabilityDto::ModelToolLoopV1 { .. }
    ) && !driver.supports_tool_calls()
    {
        return Err(driver_contract_incompatible());
    }
    Ok(())
}

/// Returns the credential transport contract declared by a user-kind composition.
fn user_credential_transport_contract(
    transport: &CredentialTransportDto,
) -> DtoResult<CredentialTransportContractDto> {
    CredentialTransportContractDto::new(
        vec![transport.mode()],
        transport.safe_header_name().map(str::to_owned),
    )
}

/// Returns the ordered protocol part revisions of a user-kind composition.
fn user_protocol_part_revisions(parts: &UserKindCompositionDto) -> Vec<String> {
    let mut revisions = vec![user_stream_part_revision(parts.stream()).to_owned()];
    if let Some(revision) = user_reasoning_part_revision(parts.reasoning()) {
        revisions.push(revision.to_owned());
    }
    if let Some(revision) = user_activation_part_revision(parts.activation()) {
        revisions.push(revision.to_owned());
    }
    if let Some(revision) = user_effort_part_revision(parts.effort()) {
        revisions.push(revision.to_owned());
    }
    revisions
}

/// Returns the code-owned part revision of one stream-framing part.
const fn user_stream_part_revision(part: UserKindStreamPartDto) -> &'static str {
    match part {
        UserKindStreamPartDto::ChatCompletionsSse => USER_STREAM_PART_REVISION,
    }
}

/// Returns the code-owned part revision of one textual reasoning field part.
const fn user_reasoning_part_revision(part: UserKindReasoningPartDto) -> Option<&'static str> {
    match part {
        UserKindReasoningPartDto::Absent => None,
        UserKindReasoningPartDto::ReasoningContent => Some("reasoning-field-reasoning-content-v1"),
        UserKindReasoningPartDto::Reasoning => Some("reasoning-field-reasoning-v1"),
        UserKindReasoningPartDto::ReasoningDetailsText => {
            Some("reasoning-field-reasoning-details-text-v1")
        }
        UserKindReasoningPartDto::MessageThinking => Some("reasoning-field-message-thinking-v1"),
    }
}

/// Returns the code-owned part revision of one thinking activation part.
const fn user_activation_part_revision(part: UserKindActivationPartDto) -> Option<&'static str> {
    match part {
        UserKindActivationPartDto::Absent => None,
        UserKindActivationPartDto::ThinkingEnabled => Some("activation-thinking-enabled-v1"),
        UserKindActivationPartDto::ThinkingAdaptive => Some("activation-thinking-adaptive-v1"),
        UserKindActivationPartDto::EnableThinking => Some("activation-enable-thinking-v1"),
        UserKindActivationPartDto::ThinkBoolean => Some("activation-think-boolean-v1"),
        UserKindActivationPartDto::ThinkEffort => Some("activation-think-effort-v1"),
    }
}

/// Returns the code-owned part revision of one effort field part.
const fn user_effort_part_revision(part: UserKindEffortPartDto) -> Option<&'static str> {
    match part {
        UserKindEffortPartDto::Absent => None,
        UserKindEffortPartDto::ReasoningEffort => Some("effort-field-reasoning-effort-v1"),
        UserKindEffortPartDto::ThinkingBudget => Some("effort-field-thinking-budget-v1"),
        UserKindEffortPartDto::ThinkingTokenBudget => Some("effort-field-thinking-token-budget-v1"),
    }
}

/// Returns the capability envelope of one user-kind composition.
fn user_capability_envelope(parts: &UserKindCompositionDto) -> DtoResult<ModelCapabilitySetV1> {
    let efforts = user_supported_efforts(parts);
    let reasoning = if efforts.is_empty() {
        ReasoningCapabilityDto::Disabled
    } else {
        // A user kind composes no summary field, so its envelope never
        // declares summary support.
        ReasoningCapabilityDto::textual_reasoning_v1(efforts, false)?
    };
    ModelCapabilitySetV1::new(
        ModelCapabilityTaxonomyVersionDto::current(),
        ModelInputKindDto::TextOnly,
        ProviderCapabilityAvailabilityDto::Enabled,
        ProviderCapabilityAvailabilityDto::Disabled,
        reasoning,
        // The composition declares no tool-loop translation part.
        ToolExchangeCapabilityDto::Disabled,
        // The composition declares no cross-turn reasoning transfer part.
        ContextPreservationCapabilityDto::local_durable_history_v1(
            ReasoningHistoryTransferDto::Disabled,
        ),
    )
}

/// Returns the closed effort levels one user-kind composition can express.
fn user_supported_efforts(parts: &UserKindCompositionDto) -> Vec<ReasoningEffortLevelDto> {
    let closed_efforts = || CLOSED_REASONING_EFFORTS.to_vec();
    match (parts.activation(), parts.effort()) {
        (UserKindActivationPartDto::ThinkEffort, _)
        | (_, UserKindEffortPartDto::ReasoningEffort) => closed_efforts(),
        // A numeric thinking budget carries tokens, not a closed effort level.
        (
            _,
            UserKindEffortPartDto::Absent
            | UserKindEffortPartDto::ThinkingBudget
            | UserKindEffortPartDto::ThinkingTokenBudget,
        ) => Vec::new(),
    }
}

/// Returns the deterministic descriptor revision identity of a user-kind composition.
///
/// The identity is derived from the kind identity and the closed composition
/// parts, so repeated validation of the same declaration yields the same
/// revision and a semantically equal catalog writes no new revision. A changed
/// composition is a different revision rather than an edit under an old
/// identity; the catalog rejects it as a kind-immutability mismatch before any
/// effect. The reduction is a local identity mechanism, never a security
/// boundary.
///
/// # Errors
///
/// Returns a validation error when the derived identity is not a canonical UUID.
fn user_descriptor_revision_id(
    parts: &UserKindCompositionDto,
) -> DtoResult<ProviderKindDescriptorRevisionId> {
    let canonical = format!(
        "user-kind-v1|{}|{}|{}|{}|{}|{}",
        parts.kind_id().as_str(),
        user_stream_part_revision(parts.stream()),
        user_reasoning_part_revision(parts.reasoning()).unwrap_or_default(),
        user_activation_part_revision(parts.activation()).unwrap_or_default(),
        user_effort_part_revision(parts.effort()).unwrap_or_default(),
        user_credential_transport_token(parts.credential_transport()),
    );
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&fnv1a_64(&canonical).to_be_bytes());
    bytes[8..].copy_from_slice(&fnv1a_64(&format!("revision-v1|{canonical}")).to_be_bytes());
    // The identity stays a canonical version-4 UUID even though it is code-derived.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    ProviderKindDescriptorRevisionId::parse(&canonical_uuid_text(&bytes))
}

/// Returns the canonical identity token of one credential transport part.
fn user_credential_transport_token(transport: &CredentialTransportDto) -> String {
    match transport.mode() {
        CredentialTransportModeDto::Bearer => "credential-transport-bearer".to_owned(),
        CredentialTransportModeDto::SafeHeader => format!(
            "credential-transport-safe-header:{}",
            transport.safe_header_name().unwrap_or_default()
        ),
    }
}

/// Reduces one identity string to sixty-four bits (FNV-1a).
fn fnv1a_64(value: &str) -> u64 {
    value.bytes().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

/// Formats sixteen bytes as the canonical lowercase UUID representation.
fn canonical_uuid_text(bytes: &[u8; 16]) -> String {
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The validation failure for a subset outside its descriptor envelope.
fn invalid_capability_subset(message: &'static str) -> ErrorDto {
    ErrorDto::validation("invalid_model_capability_set", message)
}

/// The validation failure for an unsupported or mismatched driver contract.
fn driver_contract_incompatible() -> ErrorDto {
    ErrorDto::validation(
        "provider_driver_contract_incompatible",
        "the descriptor driver contract cannot serve the declared capability set",
    )
}
