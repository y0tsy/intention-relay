#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Provider contract fixtures use explicit failure messages for impossible pending local streams."
)]

mod support;

use intention_proto::provider::{
    ContextPreservationCapabilityDto, CredentialTransportDto, CredentialTransportModeDto,
    ModelCapabilitySetV1, ModelCapabilityTaxonomyVersionDto, ModelInputKindDto,
    ProviderCapabilityAvailabilityDto, ProviderEndpointPolicyDto, ProviderKindDescriptorRevisionV1,
    ProviderKindId, ReasoningCapabilityDto, ReasoningEffortLevelDto, ReasoningHistoryTransferDto,
    ToolExchangeCapabilityDto, UserKindActivationPartDto, UserKindCompositionDto,
    UserKindEffortPartDto, UserKindReasoningPartDto, UserKindStreamPartDto,
};
use intention_providers::{
    AssistantReasoningHistoryDto, AuthenticationHeaderPolicyV1, GenericChatDriver,
    GenericChatDriverOptions, ModelCancellationSignal, ModelCapabilitiesDto, ModelExecutionDriver,
    ModelMessageDto, ModelRequestDto, ModelRoleDto, OpenRouterDriver, OpenRouterDriverOptions,
    driver_capabilities, driver_contract, first_party_kind_descriptors, validate_capability_subset,
    validate_user_kind,
};
use support::{
    FAKE_CREDENTIAL, collect_ready, first_party_descriptor, plain_request, profile_revision,
    profile_revision_with_transfer,
};

/// One configured adapter under the mirrored adapter contract cases.
struct Adapter {
    name: &'static str,
    debug: String,
    driver: Box<dyn ModelExecutionDriver>,
}

fn adapters() -> Vec<Adapter> {
    let generic = GenericChatDriver::from_profile_revision(
        &profile_revision(
            "generic-chat-completion-api",
            "fixture",
            Some("https://example.invalid/v1"),
            CredentialTransportDto::bearer(),
        ),
        FAKE_CREDENTIAL.to_owned(),
    )
    .expect("driver builds");
    let openrouter = OpenRouterDriver::from_profile_revision(
        &profile_revision(
            "openrouter",
            "fixture",
            None,
            CredentialTransportDto::bearer(),
        ),
        FAKE_CREDENTIAL.to_owned(),
    )
    .expect("driver builds");
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
fn authentication_header_policy_validates_the_closed_transport_declaration() {
    let bearer = AuthenticationHeaderPolicyV1::bearer();
    assert_eq!(bearer.transport(), CredentialTransportModeDto::Bearer);
    assert!(!bearer.selects_safe_header());
    assert_eq!(bearer.safe_header_name(), None);

    let safe = AuthenticationHeaderPolicyV1::new(
        CredentialTransportModeDto::SafeHeader,
        Some("x-api-key".to_owned()),
    )
    .expect("safe-header policy is valid");
    assert_eq!(safe.transport(), CredentialTransportModeDto::SafeHeader);
    assert!(safe.selects_safe_header());
    assert_eq!(safe.safe_header_name(), Some("x-api-key"));
    let declared = CredentialTransportDto::safe_header("x-api-key")
        .expect("safe-header transport declaration is valid");
    assert_eq!(
        AuthenticationHeaderPolicyV1::from_credential_transport(&declared),
        safe
    );

    assert!(
        AuthenticationHeaderPolicyV1::new(CredentialTransportModeDto::SafeHeader, None).is_err(),
        "a safe-header policy without a header name is incoherent"
    );
    assert!(
        AuthenticationHeaderPolicyV1::new(
            CredentialTransportModeDto::SafeHeader,
            Some("not a header".to_owned()),
        )
        .is_err(),
        "a safe-header policy requires a valid header-name token"
    );
    assert!(
        AuthenticationHeaderPolicyV1::new(
            CredentialTransportModeDto::Bearer,
            Some("x-api-key".to_owned()),
        )
        .is_err(),
        "a bearer policy carries no header name"
    );
}

#[test]
fn driver_options_carry_the_exact_profile_credential_transport() {
    let bearer = CredentialTransportDto::bearer();
    let openrouter_profile = profile_revision("openrouter", "fixture-model", None, bearer.clone());
    assert_eq!(
        OpenRouterDriverOptions::from_profile_revision(&openrouter_profile).credential_transport(),
        &bearer
    );
    let generic_profile = profile_revision(
        "generic-chat-completion-api",
        "fixture-model",
        Some("https://example.invalid/v1"),
        bearer.clone(),
    );
    assert_eq!(
        GenericChatDriverOptions::from_profile_revision(&generic_profile).credential_transport(),
        &bearer
    );
}

#[test]
fn profile_revisions_apply_at_construction_and_keep_the_credential_private() {
    let openrouter_revision = profile_revision(
        "openrouter",
        "fixture-model",
        None,
        CredentialTransportDto::bearer(),
    );
    let openrouter =
        OpenRouterDriver::from_profile_revision(&openrouter_revision, FAKE_CREDENTIAL.to_owned())
            .expect("bearer profile revision builds the driver");
    assert_openrouter_driver_contract(&openrouter);

    let generic_revision = profile_revision(
        "generic-chat-completion-api",
        "fixture-model",
        Some("https://example.invalid/v1"),
        CredentialTransportDto::bearer(),
    );
    let generic =
        GenericChatDriver::from_profile_revision(&generic_revision, FAKE_CREDENTIAL.to_owned())
            .expect("bearer profile revision builds the driver");
    assert_generic_driver_contract(&generic);
}

#[test]
fn safe_header_transport_fails_closed_at_adapter_construction() {
    let safe = CredentialTransportDto::safe_header("x-api-key")
        .expect("safe-header transport declaration is valid");
    for descriptor in first_party_kind_descriptors().expect("code-owned descriptors validate") {
        assert_eq!(
            descriptor.credential_transport_contract().supported(),
            &[CredentialTransportModeDto::Bearer],
            "{}",
            descriptor.kind_id()
        );
        assert_eq!(
            descriptor
                .credential_transport_contract()
                .safe_header_name(),
            None,
            "{}",
            descriptor.kind_id()
        );
    }

    let openrouter_revision = profile_revision("openrouter", "fixture-model", None, safe.clone());
    assert_eq!(
        OpenRouterDriver::from_profile_revision(&openrouter_revision, FAKE_CREDENTIAL.to_owned())
            .expect_err("the pinned OpenRouter SDK cannot apply a safe header")
            .code(),
        "openrouter_credential_transport_unsupported"
    );

    let generic_revision = profile_revision(
        "generic-chat-completion-api",
        "fixture-model",
        Some("https://example.invalid/v1"),
        safe,
    );
    assert_eq!(
        GenericChatDriver::from_profile_revision(&generic_revision, FAKE_CREDENTIAL.to_owned())
            .expect_err("the pinned chat SDK cannot apply a safe header")
            .code(),
        "generic_chat_credential_transport_unsupported"
    );
}

#[test]
fn profile_revisions_bind_the_exact_kind_and_endpoint_policy() {
    let credential = FAKE_CREDENTIAL.to_owned();
    let openrouter_revision = profile_revision(
        "openrouter",
        "fixture-model",
        None,
        CredentialTransportDto::bearer(),
    );
    let generic_revision = profile_revision(
        "generic-chat-completion-api",
        "fixture-model",
        Some("https://example.invalid/v1"),
        CredentialTransportDto::bearer(),
    );

    assert_eq!(
        OpenRouterDriver::from_profile_revision(&generic_revision, credential.clone())
            .expect_err("a generic profile is not an OpenRouter profile")
            .code(),
        "invalid_openrouter_provider_config"
    );
    assert_eq!(
        GenericChatDriver::from_profile_revision(&openrouter_revision, credential.clone())
            .expect_err("an OpenRouter profile is not a generic chat profile")
            .code(),
        "invalid_generic_chat_provider_config"
    );

    let openrouter_override = profile_revision(
        "openrouter",
        "fixture-model",
        Some("https://example.invalid/v1"),
        CredentialTransportDto::bearer(),
    );
    assert_eq!(
        OpenRouterDriver::from_profile_revision(&openrouter_override, credential.clone())
            .expect_err("OpenRouter declares no endpoint override")
            .code(),
        "invalid_openrouter_provider_config"
    );

    let generic_without_endpoint = profile_revision(
        "generic-chat-completion-api",
        "fixture-model",
        None,
        CredentialTransportDto::bearer(),
    );
    assert_eq!(
        GenericChatDriver::from_profile_revision(&generic_without_endpoint, credential)
            .expect_err("generic chat requires an explicit endpoint")
            .code(),
        "missing_generic_chat_endpoint"
    );
}

#[test]
fn options_that_do_not_bind_the_exact_revision_transport_fail_closed() {
    let credential = FAKE_CREDENTIAL.to_owned();
    let openrouter_revision = profile_revision(
        "openrouter",
        "fixture-model",
        None,
        CredentialTransportDto::bearer(),
    );
    let generic_revision = profile_revision(
        "generic-chat-completion-api",
        "fixture-model",
        Some("https://example.invalid/v1"),
        CredentialTransportDto::bearer(),
    );
    let stale_openrouter_options =
        OpenRouterDriverOptions::from_profile_revision(&profile_revision(
            "openrouter",
            "fixture-model",
            None,
            CredentialTransportDto::safe_header("x-api-key")
                .expect("safe-header transport declaration is valid"),
        ));
    assert_eq!(
        OpenRouterDriver::from_profile_revision_with_options(
            &openrouter_revision,
            credential.clone(),
            stale_openrouter_options,
        )
        .expect_err("options from another revision never apply to this one")
        .code(),
        "openrouter_credential_transport_mismatch"
    );

    let stale_generic_options = GenericChatDriverOptions::from_profile_revision(&profile_revision(
        "generic-chat-completion-api",
        "fixture-model",
        Some("https://example.invalid/v1"),
        CredentialTransportDto::safe_header("x-api-key")
            .expect("safe-header transport declaration is valid"),
    ));
    assert_eq!(
        GenericChatDriver::from_profile_revision_with_options(
            &generic_revision,
            credential,
            stale_generic_options,
        )
        .expect_err("options from another revision never apply to this one")
        .code(),
        "generic_chat_credential_transport_mismatch"
    );
}

#[test]
fn first_party_descriptors_declare_the_closed_first_party_contract() {
    let descriptors = first_party_kind_descriptors().expect("code-owned descriptors validate");
    let kind_ids = descriptors
        .iter()
        .map(|descriptor| descriptor.kind_id().as_str().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        kind_ids,
        [
            "openrouter".to_owned(),
            "generic-chat-completion-api".to_owned()
        ]
    );

    for descriptor in &descriptors {
        assert!(descriptor.kind_id().is_first_party());
        assert_eq!(
            descriptor.model_capability_envelope().text_streaming(),
            ProviderCapabilityAvailabilityDto::Enabled
        );
        assert_eq!(
            descriptor.model_capability_envelope().structured_output(),
            ProviderCapabilityAvailabilityDto::Disabled
        );
        assert!(matches!(
            descriptor.model_capability_envelope().tool_exchange(),
            ToolExchangeCapabilityDto::ModelToolLoopV1 { .. }
        ));
        assert!(
            !descriptor.model_capability_envelope().supports_summary(),
            "no slice-2 adapter emits reasoning summaries"
        );
        let contract = driver_contract(descriptor.kind_id())
            .expect("every first-party kind has a code-owned driver contract");
        assert_eq!(
            contract.driver_family(),
            descriptor.driver_contract_family()
        );
        assert_eq!(contract.major(), 1);
        assert_eq!(
            driver_capabilities(descriptor.kind_id()),
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        );
        assert_eq!(
            descriptor.descriptor_revision_id(),
            first_party_descriptor(descriptor.kind_id().as_str()).descriptor_revision_id(),
            "a descriptor revision identity is stable across calls"
        );
    }

    let openrouter = first_party_descriptor("openrouter");
    assert_eq!(
        openrouter.endpoint_policy(),
        ProviderEndpointPolicyDto::new(false, false, false)
    );
    assert_eq!(
        openrouter
            .model_capability_envelope()
            .reasoning_input_contract(),
        &ReasoningHistoryTransferDto::Disabled,
        "the pinned OpenRouter chat SDK accepts no reasoning history echo"
    );

    let generic = first_party_descriptor("generic-chat-completion-api");
    assert_eq!(
        generic.endpoint_policy(),
        ProviderEndpointPolicyDto::new(true, true, true)
    );
    assert!(
        matches!(
            generic
                .model_capability_envelope()
                .reasoning_input_contract(),
            ReasoningHistoryTransferDto::TextualHistoryV1 { .. }
        ),
        "the generic adapter is the live cross-turn translation path"
    );

    let user_kind = ProviderKindId::parse("my-kind").expect("fixture identity is valid");
    assert!(driver_contract(&user_kind).is_none());
    assert_eq!(
        driver_capabilities(&user_kind),
        ModelCapabilitiesDto::new(false, false, false, false, false, false),
        "a kind without a code-owned driver declares no capability"
    );
}

fn subset(
    reasoning: ReasoningCapabilityDto,
    tool_exchange: ToolExchangeCapabilityDto,
    transfer: ReasoningHistoryTransferDto,
) -> ModelCapabilitySetV1 {
    ModelCapabilitySetV1::new(
        ModelCapabilityTaxonomyVersionDto::current(),
        ModelInputKindDto::TextOnly,
        ProviderCapabilityAvailabilityDto::Enabled,
        ProviderCapabilityAvailabilityDto::Disabled,
        reasoning,
        tool_exchange,
        ContextPreservationCapabilityDto::local_durable_history_v1(transfer),
    )
    .expect("fixture capability subset is valid")
}

fn closed_efforts() -> Vec<ReasoningEffortLevelDto> {
    vec![
        ReasoningEffortLevelDto::None,
        ReasoningEffortLevelDto::Minimal,
        ReasoningEffortLevelDto::Low,
        ReasoningEffortLevelDto::Medium,
        ReasoningEffortLevelDto::High,
        ReasoningEffortLevelDto::XHigh,
        ReasoningEffortLevelDto::Max,
    ]
}

fn textual_reasoning(
    efforts: Vec<ReasoningEffortLevelDto>,
    summary_support: bool,
) -> ReasoningCapabilityDto {
    ReasoningCapabilityDto::textual_reasoning_v1(efforts, summary_support)
        .expect("fixture reasoning capability is valid")
}

#[test]
fn capability_subset_validation_intersects_the_envelope_with_the_declared_subset() {
    let descriptor = first_party_descriptor("openrouter");
    let translation_revision = descriptor
        .model_capability_envelope()
        .tool_exchange()
        .translation_revision()
        .expect("first-party envelopes declare the model tool loop")
        .to_owned();
    let valid = subset(
        textual_reasoning(vec![ReasoningEffortLevelDto::Low], false),
        ToolExchangeCapabilityDto::model_tool_loop_v1(translation_revision)
            .expect("fixture tool loop is valid"),
        ReasoningHistoryTransferDto::Disabled,
    );
    assert_eq!(
        validate_capability_subset(&descriptor, &valid)
            .expect("a subset inside the envelope validates"),
        valid,
        "the intersection of an admitted subset is the subset itself"
    );

    let summary_beyond_envelope = subset(
        textual_reasoning(vec![ReasoningEffortLevelDto::Low], true),
        ToolExchangeCapabilityDto::Disabled,
        ReasoningHistoryTransferDto::Disabled,
    );
    assert_eq!(
        validate_capability_subset(&descriptor, &summary_beyond_envelope)
            .expect_err("summary support outside the envelope blocks")
            .code(),
        "invalid_model_capability_set"
    );

    let unknown_translation = subset(
        textual_reasoning(vec![ReasoningEffortLevelDto::Low], false),
        ToolExchangeCapabilityDto::model_tool_loop_v1("unknown-translation-v9")
            .expect("fixture translation token is valid"),
        ReasoningHistoryTransferDto::Disabled,
    );
    assert_eq!(
        validate_capability_subset(&descriptor, &unknown_translation)
            .expect_err("an undeclared tool-loop translation blocks")
            .code(),
        "invalid_model_capability_set"
    );

    let unknown_driver_descriptor = ProviderKindDescriptorRevisionV1::new(
        descriptor.kind_id().clone(),
        descriptor.descriptor_revision_id(),
        descriptor.descriptor_family(),
        descriptor.ordered_protocol_part_revisions().to_vec(),
        descriptor.endpoint_policy(),
        descriptor.credential_transport_contract().clone(),
        descriptor.model_capability_envelope().clone(),
        "unknown-driver-family",
    )
    .expect("fixture descriptor is valid");
    assert_eq!(
        validate_capability_subset(&unknown_driver_descriptor, &valid)
            .expect_err("an unknown driver family blocks")
            .code(),
        "provider_driver_contract_incompatible"
    );
}

#[test]
fn capability_subset_validation_blocks_declarations_the_envelope_cannot_represent() {
    let base = first_party_descriptor("openrouter");
    let narrowed = ProviderKindDescriptorRevisionV1::new(
        base.kind_id().clone(),
        base.descriptor_revision_id(),
        base.descriptor_family(),
        base.ordered_protocol_part_revisions().to_vec(),
        base.endpoint_policy(),
        base.credential_transport_contract().clone(),
        ModelCapabilitySetV1::new(
            ModelCapabilityTaxonomyVersionDto::current(),
            ModelInputKindDto::TextOnly,
            ProviderCapabilityAvailabilityDto::Disabled,
            ProviderCapabilityAvailabilityDto::Disabled,
            textual_reasoning(vec![ReasoningEffortLevelDto::Low], false),
            ToolExchangeCapabilityDto::Disabled,
            ContextPreservationCapabilityDto::local_durable_history_v1(
                ReasoningHistoryTransferDto::Disabled,
            ),
        )
        .expect("narrowed fixture envelope is valid"),
        base.driver_contract_family(),
    )
    .expect("narrowed fixture descriptor is valid");

    let streaming_beyond_envelope = subset(
        ReasoningCapabilityDto::Disabled,
        ToolExchangeCapabilityDto::Disabled,
        ReasoningHistoryTransferDto::Disabled,
    );
    assert_eq!(
        validate_capability_subset(&narrowed, &streaming_beyond_envelope)
            .expect_err("text streaming enabled in the subset blocks when the envelope disables it")
            .code(),
        "invalid_model_capability_set"
    );

    let effort_beyond_envelope = subset(
        textual_reasoning(vec![ReasoningEffortLevelDto::High], false),
        ToolExchangeCapabilityDto::Disabled,
        ReasoningHistoryTransferDto::Disabled,
    );
    assert_eq!(
        validate_capability_subset(&narrowed, &effort_beyond_envelope)
            .expect_err("an undeclared effort blocks")
            .code(),
        "invalid_model_capability_set"
    );

    let tool_loop_beyond_envelope = subset(
        ReasoningCapabilityDto::Disabled,
        ToolExchangeCapabilityDto::model_tool_loop_v1("chat-completions-tool-loop-v1")
            .expect("fixture tool loop is valid"),
        ReasoningHistoryTransferDto::Disabled,
    );
    assert_eq!(
        validate_capability_subset(&narrowed, &tool_loop_beyond_envelope)
            .expect_err("a tool loop outside the envelope blocks")
            .code(),
        "invalid_model_capability_set"
    );
}

fn user_kind(
    reasoning: UserKindReasoningPartDto,
    activation: UserKindActivationPartDto,
    effort: UserKindEffortPartDto,
    credential_transport: CredentialTransportDto,
) -> UserKindCompositionDto {
    UserKindCompositionDto::new(
        ProviderKindId::parse("my-kind").expect("fixture identity is valid"),
        UserKindStreamPartDto::ChatCompletionsSse,
        reasoning,
        activation,
        effort,
        credential_transport,
    )
    .expect("fixture user kind composition is valid")
}

#[test]
fn user_kind_compositions_map_to_deterministic_admission_only_descriptors() {
    let composition = user_kind(
        UserKindReasoningPartDto::ReasoningContent,
        UserKindActivationPartDto::ThinkingEnabled,
        UserKindEffortPartDto::ReasoningEffort,
        CredentialTransportDto::bearer(),
    );
    let descriptor = validate_user_kind(&composition).expect("closed parts are admitted");
    assert_eq!(descriptor.kind_id().as_str(), "my-kind");
    assert_eq!(
        descriptor.endpoint_policy(),
        ProviderEndpointPolicyDto::new(true, true, true)
    );
    assert_eq!(
        descriptor.credential_transport_contract().supported(),
        &[CredentialTransportModeDto::Bearer]
    );
    assert!(
        first_party_kind_descriptors()
            .expect("code-owned descriptors are valid")
            .iter()
            .all(|first_party| first_party.driver_contract_family()
                != descriptor.driver_contract_family()),
        "a user kind never borrows a first-party driver family"
    );
    assert_eq!(
        descriptor.model_capability_envelope().reasoning(),
        &textual_reasoning(closed_efforts(), false),
        "a declared closed effort field carries the closed effort set"
    );
    assert_eq!(
        descriptor,
        validate_user_kind(&composition).expect("closed parts are admitted"),
        "an unchanged composition keeps one immutable descriptor revision"
    );

    let edited = user_kind(
        UserKindReasoningPartDto::MessageThinking,
        UserKindActivationPartDto::ThinkingEnabled,
        UserKindEffortPartDto::ReasoningEffort,
        CredentialTransportDto::bearer(),
    );
    let edited_descriptor = validate_user_kind(&edited).expect("closed parts are admitted");
    assert_ne!(
        descriptor.descriptor_revision_id(),
        edited_descriptor.descriptor_revision_id(),
        "an edited composition is a different revision, never an edit under the old identity"
    );

    let safe_header = user_kind(
        UserKindReasoningPartDto::ReasoningDetailsText,
        UserKindActivationPartDto::ThinkEffort,
        UserKindEffortPartDto::Absent,
        CredentialTransportDto::safe_header("x-api-key")
            .expect("safe-header transport declaration is valid"),
    );
    let safe_header_descriptor =
        validate_user_kind(&safe_header).expect("closed parts are admitted");
    assert_eq!(
        safe_header_descriptor
            .credential_transport_contract()
            .supported(),
        &[CredentialTransportModeDto::SafeHeader]
    );
    assert_eq!(
        safe_header_descriptor
            .credential_transport_contract()
            .safe_header_name(),
        Some("x-api-key")
    );

    // Admission is not execution: a user kind has no code-owned driver
    // contract, so every capability subset is blocked before any effect.
    let subset = subset(
        ReasoningCapabilityDto::Disabled,
        ToolExchangeCapabilityDto::Disabled,
        ReasoningHistoryTransferDto::Disabled,
    );
    assert_eq!(
        validate_capability_subset(&descriptor, &subset)
            .expect_err("a user kind has no code-owned driver contract")
            .code(),
        "provider_driver_contract_incompatible"
    );
}

fn assert_openrouter_driver_contract(driver: &OpenRouterDriver) {
    assert_eq!(
        driver.capabilities(),
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    );
    assert!(!format!("{driver:?}").contains(FAKE_CREDENTIAL));
    let cancellation = ModelCancellationSignal::new();
    cancellation.cancel();
    assert!(collect_ready(driver.execute(plain_request(), cancellation)).is_empty());
}

fn assert_generic_driver_contract(driver: &GenericChatDriver) {
    assert_eq!(
        driver.capabilities(),
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    );
    assert!(!format!("{driver:?}").contains(FAKE_CREDENTIAL));
    let cancellation = ModelCancellationSignal::new();
    cancellation.cancel();
    assert!(collect_ready(driver.execute(plain_request(), cancellation)).is_empty());
}

#[test]
fn openrouter_profiles_selecting_textual_history_fail_closed() {
    let revision = profile_revision_with_transfer(
        "openrouter",
        "fixture-model",
        None,
        CredentialTransportDto::bearer(),
        ReasoningHistoryTransferDto::textual_history_v1("generic-chat-reasoning-content-v1")
            .expect("fixture transfer is valid"),
    );
    assert_eq!(
        OpenRouterDriver::from_profile_revision(&revision, FAKE_CREDENTIAL.to_owned())
            .expect_err("the pinned OpenRouter SDK cannot echo prior reasoning")
            .code(),
        "openrouter_reasoning_history_unsupported"
    );

    // The same transfer is a representable generic-chat profile revision: the
    // generic adapter is the live cross-turn translation path.
    let generic = profile_revision_with_transfer(
        "generic-chat-completion-api",
        "fixture-model",
        Some("https://example.invalid/v1"),
        CredentialTransportDto::bearer(),
        ReasoningHistoryTransferDto::textual_history_v1("generic-chat-reasoning-content-v1")
            .expect("fixture transfer is valid"),
    );
    assert!(GenericChatDriver::from_profile_revision(&generic, FAKE_CREDENTIAL.to_owned()).is_ok());
}

#[test]
fn openrouter_requests_carrying_history_fail_before_provider_work() {
    let openrouter = OpenRouterDriver::from_profile_revision(
        &profile_revision(
            "openrouter",
            "fixture-model",
            None,
            CredentialTransportDto::bearer(),
        ),
        FAKE_CREDENTIAL.to_owned(),
    )
    .expect("driver builds");
    let history = AssistantReasoningHistoryDto::new(
        "generic-chat-reasoning-content-v1",
        vec![(
            intention_providers::ReasoningFragmentCategoryDto::Primary,
            "prior reasoning".to_owned(),
        )],
        Vec::new(),
    )
    .expect("fixture history is valid");
    let message = ModelMessageDto::assistant_with_reasoning_history("the answer", history)
        .expect("assistant history message is valid");
    let request = ModelRequestDto::new(
        intention_proto::RunId::new(),
        "fixture-model",
        vec![
            message,
            ModelMessageDto::new(ModelRoleDto::User, "next").expect("message is valid"),
        ],
        None,
    )
    .expect("request is valid");

    // The typed rejection resolves locally, so the idle loopback-free path never
    // reaches the provider and never drops the history silently.
    let events = collect_ready(openrouter.execute(request, ModelCancellationSignal::new()));
    assert_eq!(events.len(), 1, "one typed failure, no provider event");
    assert_eq!(
        events[0]
            .as_ref()
            .expect_err("a history-bearing request fails closed")
            .code(),
        "openrouter_request_rejected"
    );
    assert!(
        !format!("{events:?}").contains("prior reasoning"),
        "the rejected reasoning text never enters the provider error surface"
    );
}

#[test]
fn probe_defaults_fail_closed_for_drivers_without_a_probe_path() {
    /// A driver that declares only the execution surface keeps both defaults.
    struct MinimalDriver;

    impl ModelExecutionDriver for MinimalDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }

        fn execute(
            &self,
            _request: ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> intention_providers::ModelEventStream {
            Box::pin(futures_util::stream::empty())
        }
    }

    let driver = MinimalDriver;
    assert_eq!(
        futures_executor::block_on(driver.health_probe())
            .expect_err("the default declares no health probe path")
            .code(),
        "provider_probe_unsupported"
    );
    assert_eq!(
        futures_executor::block_on(driver.list_models())
            .expect_err("the default declares no model listing path")
            .code(),
        "provider_probe_unsupported"
    );
}
