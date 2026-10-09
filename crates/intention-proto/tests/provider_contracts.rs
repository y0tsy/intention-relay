#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Provider identity, catalog, capability, reasoning, and control-plane contract evidence.

use intention_proto::provider::{
    CatalogRevisionId, ProviderDiscoveryAttemptId, ProviderKindDescriptorRevisionId,
    ProviderProfileRevisionId, ReasoningHistoryManifestId,
};
use intention_proto::{
    AcceptProviderCatalogRemovalCommandDto, ApplyConfigurationDocumentCommandDto,
    ApplyConfigurationEditsCommandDto, CheckProviderHealthCommandDto, ConfigurationEditAcceptedDto,
    ConfigurationEditDto, ConfigurationReloadAcceptedDto, ContextPreservationCapabilityDto,
    CredentialRotationAcceptedDto, CredentialTransportContractDto, CredentialTransportDto,
    CredentialTransportModeDto, DiscoverProviderModelsCommandDto, ErrorCategoryDto,
    GetSessionProviderProfileQueryDto, IdempotencyKey, ListProviderCatalogQueryDto,
    LoopbackPolicyDto, ModelCapabilitySetV1, ModelCapabilityTaxonomyVersionDto, ModelInputKindDto,
    ProviderCapabilityAvailabilityDto, ProviderCatalogActivationStateDto,
    ProviderCatalogCandidateHandleDto, ProviderCatalogCandidateRejectedDto,
    ProviderCatalogDegradedReasonDto, ProviderCatalogPageDto, ProviderCatalogRemovalAcceptedDto,
    ProviderCatalogStatusDto, ProviderCatalogValidationIssueDto, ProviderDiscoveryResultDto,
    ProviderDriverCapabilitiesDto, ProviderDriverContractRevisionDto, ProviderEndpointPolicyDto,
    ProviderExecutionPolicyDto, ProviderHealthEvidenceDto, ProviderHealthReasonDto,
    ProviderHealthStateDto, ProviderKindDescriptorRevisionV1, ProviderKindId,
    ProviderModelRecordDto, ProviderPricingPolicyDto, ProviderProfileEntryDto, ProviderProfileId,
    ProviderProfilePolicyDto, ProviderProfileReadinessDto, ProviderProfileRevisionV1,
    ProviderSelectionSourceDto, ProviderSelectionUnavailabilityDto, ReasoningCapabilityDto,
    ReasoningEffortLevelDto, ReasoningFragmentCategoryDto, ReasoningHistoryBoundDto,
    ReasoningHistoryManifestDto, ReasoningHistoryRecordReferenceDto,
    ReasoningHistorySourceEntryDto, ReasoningHistoryTransferDto,
    RejectProviderCatalogCandidateCommandDto, ReloadConfigurationCommandDto,
    ResolvedReasoningPolicyDto, ResolvedRunProviderSelectionDto,
    RotateProviderCredentialCommandDto, SessionId, SessionProviderProfileChangedDto,
    SessionProviderProfileProjectionDto, SetSessionProviderProfileAcceptedDto,
    SetSessionProviderProfileCommandDto, ToolExchangeCapabilityDto, UserKindActivationPartDto,
    UserKindCompositionDto, UserKindEffortPartDto, UserKindReasoningPartDto, UserKindStreamPartDto,
};

fn fixture_profile_id() -> ProviderProfileId {
    ProviderProfileId::parse("main").expect("fixture profile identity is valid")
}

fn fixture_kind_id() -> ProviderKindId {
    ProviderKindId::parse("openrouter").expect("fixture kind identity is valid")
}

fn fixture_execution_policy() -> ProviderExecutionPolicyDto {
    ProviderExecutionPolicyDto::new(30, 2).expect("fixture execution policy is valid")
}

fn fixture_transfer() -> ReasoningHistoryTransferDto {
    ReasoningHistoryTransferDto::textual_history_v1("fixture-compatibility-v1")
        .expect("fixture transfer contract is valid")
}

fn fixture_capability_subset(
    efforts: Vec<ReasoningEffortLevelDto>,
    summary_support: bool,
) -> ModelCapabilitySetV1 {
    ModelCapabilitySetV1::new(
        ModelCapabilityTaxonomyVersionDto::current(),
        ModelInputKindDto::TextOnly,
        ProviderCapabilityAvailabilityDto::Enabled,
        ProviderCapabilityAvailabilityDto::Disabled,
        ReasoningCapabilityDto::textual_reasoning_v1(efforts, summary_support)
            .expect("fixture reasoning capability is valid"),
        ToolExchangeCapabilityDto::model_tool_loop_v1("fixture-tool-loop-v1")
            .expect("fixture tool loop is valid"),
        ContextPreservationCapabilityDto::local_durable_history_v1(fixture_transfer()),
    )
    .expect("fixture capability subset is valid")
}

fn fixture_reasoning_policy() -> ResolvedReasoningPolicyDto {
    ResolvedReasoningPolicyDto::new(
        Some(ReasoningEffortLevelDto::Medium),
        true,
        fixture_transfer(),
        vec![
            ReasoningFragmentCategoryDto::Primary,
            ReasoningFragmentCategoryDto::Detail,
        ],
    )
    .expect("fixture reasoning policy is valid")
}

fn fixture_profile_revision() -> ProviderProfileRevisionV1 {
    ProviderProfileRevisionV1::new(
        fixture_profile_id(),
        ProviderProfileRevisionId::new(),
        fixture_kind_id(),
        ProviderKindDescriptorRevisionId::new(),
        "fixture-model",
        Some("https://provider.example/v1".to_owned()),
        CredentialTransportDto::bearer(),
        fixture_capability_subset(
            vec![
                ReasoningEffortLevelDto::Low,
                ReasoningEffortLevelDto::Medium,
                ReasoningEffortLevelDto::High,
            ],
            true,
        ),
        fixture_reasoning_policy(),
        fixture_execution_policy(),
        LoopbackPolicyDto::NotApplicable,
    )
    .expect("fixture profile revision is valid")
}

fn fixture_driver_contract() -> ProviderDriverContractRevisionDto {
    ProviderDriverContractRevisionDto::new("fixture-driver", 1, 0)
        .expect("fixture driver contract is valid")
}

#[test]
fn provider_token_identities_validate_their_character_set() {
    let kind = ProviderKindId::parse("first-party-kind-1").expect("fixture kind identity is valid");
    assert_eq!(kind.as_str(), "first-party-kind-1");
    assert_eq!(kind.to_string(), "first-party-kind-1");
    assert!(
        !kind.is_first_party(),
        "a declared kind identity is not a reserved first-party identity"
    );
    assert_eq!(
        serde_json::from_str::<ProviderKindId>(
            &serde_json::to_string(&kind).expect("kind serializes")
        )
        .expect("kind decodes"),
        kind
    );

    let profile = fixture_profile_id();
    assert_eq!(profile.as_str(), "main");
    assert_eq!(profile.to_string(), "main");
    assert_eq!(
        serde_json::from_str::<ProviderProfileId>(
            &serde_json::to_string(&profile).expect("profile id serializes")
        )
        .expect("profile id decodes"),
        profile
    );

    for invalid in [
        "",
        " ",
        "with space",
        "tab\t",
        "control\u{7}",
        "non-ascii-\u{e9}",
    ] {
        let kind_error =
            ProviderKindId::parse(invalid).expect_err("invalid kind identity must fail");
        assert_eq!(kind_error.code(), "invalid_provider_kind_id");
        assert_eq!(kind_error.category(), ErrorCategoryDto::Validation);
        let profile_error =
            ProviderProfileId::parse(invalid).expect_err("invalid profile identity must fail");
        assert_eq!(profile_error.code(), "invalid_provider_profile_id");
        assert_eq!(profile_error.category(), ErrorCategoryDto::Validation);
    }
    assert!(serde_json::from_str::<ProviderKindId>(r#""""#).is_err());
    assert!(serde_json::from_str::<ProviderKindId>(r#""bad value""#).is_err());
}

#[test]
fn first_party_kind_identities_are_reserved() {
    for reserved in ProviderKindId::FIRST_PARTY_IDS {
        let kind = ProviderKindId::parse(reserved).expect("reserved identity is a valid token");
        assert!(
            kind.is_first_party(),
            "{reserved} is a reserved first-party kind"
        );
    }
    assert_eq!(
        ProviderKindId::FIRST_PARTY_IDS,
        ["openrouter", "generic-chat-completion-api"]
    );
}

#[test]
fn uuid_identities_round_trip_as_canonical_strings() {
    let ids = [
        CatalogRevisionId::new().to_string(),
        ProviderProfileRevisionId::new().to_string(),
        ProviderKindDescriptorRevisionId::new().to_string(),
        ProviderDiscoveryAttemptId::new().to_string(),
        ReasoningHistoryManifestId::new().to_string(),
    ];
    for id in ids {
        assert_eq!(
            id.len(),
            36,
            "a generated identity uses the canonical UUID form"
        );
        assert_eq!(
            CatalogRevisionId::parse(&id)
                .expect("canonical identity parses")
                .to_string(),
            id
        );
    }
    for invalid in ["not-an-id", "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"] {
        assert_eq!(
            CatalogRevisionId::parse(invalid)
                .expect_err("non-canonical identity fails")
                .code(),
            "invalid_id"
        );
    }
    let revision = CatalogRevisionId::new();
    assert_eq!(
        serde_json::from_str::<CatalogRevisionId>(
            &serde_json::to_string(&revision).expect("identity serializes")
        )
        .expect("identity decodes"),
        revision
    );
}

#[test]
fn credential_transport_rules_are_enforced() {
    let bearer = CredentialTransportDto::bearer();
    assert_eq!(bearer.mode(), CredentialTransportModeDto::Bearer);
    assert_eq!(bearer.safe_header_name(), None);
    assert_eq!(
        serde_json::to_value(bearer).expect("bearer transport serializes"),
        serde_json::json!({"mode": "bearer"})
    );

    let header = CredentialTransportDto::safe_header("x-api-key").expect("valid header transport");
    assert_eq!(header.mode(), CredentialTransportModeDto::SafeHeader);
    assert_eq!(header.safe_header_name(), Some("x-api-key"));
    assert_eq!(
        serde_json::from_str::<CredentialTransportDto>(
            &serde_json::to_string(&header).expect("header transport serializes")
        )
        .expect("header transport decodes"),
        header
    );

    assert_eq!(
        CredentialTransportDto::new(
            CredentialTransportModeDto::Bearer,
            Some("x-api-key".to_owned())
        )
        .expect_err("bearer transport forbids a header name")
        .code(),
        "invalid_credential_transport"
    );
    assert_eq!(
        CredentialTransportDto::new(CredentialTransportModeDto::SafeHeader, None)
            .expect_err("safe-header transport requires a header name")
            .code(),
        "invalid_credential_transport"
    );
    for invalid in ["", "bad header", "bad\nheader", "bad:header", "(bad)"] {
        assert_eq!(
            CredentialTransportDto::safe_header(invalid)
                .expect_err("invalid header name fails")
                .code(),
            "invalid_credential_transport"
        );
    }
    assert!(CredentialTransportDto::safe_header("X_Foo.Bar~1").is_ok());
    assert!(
        serde_json::from_str::<CredentialTransportDto>("{\"mode\":\"safe_header\"}").is_err(),
        "a safe-header transport without its header name fails closed"
    );
    assert!(
        serde_json::from_str::<CredentialTransportDto>(
            "{\"mode\":\"bearer\",\"safe_header_name\":\"x-api-key\"}"
        )
        .is_err(),
        "a bearer transport with a header name fails closed"
    );
}

#[test]
fn credential_transport_contracts_require_coherent_declarations() {
    let contract = CredentialTransportContractDto::new(
        vec![
            CredentialTransportModeDto::Bearer,
            CredentialTransportModeDto::SafeHeader,
        ],
        Some("x-api-key".to_owned()),
    )
    .expect("coherent contract is valid");
    assert!(contract.supports(CredentialTransportModeDto::Bearer));
    assert!(contract.supports(CredentialTransportModeDto::SafeHeader));
    assert_eq!(contract.safe_header_name(), Some("x-api-key"));

    assert_eq!(
        CredentialTransportContractDto::new(Vec::new(), None)
            .expect_err("an empty contract fails")
            .code(),
        "invalid_credential_transport_contract"
    );
    assert_eq!(
        CredentialTransportContractDto::new(
            vec![
                CredentialTransportModeDto::Bearer,
                CredentialTransportModeDto::Bearer
            ],
            None,
        )
        .expect_err("a repeated transport fails")
        .code(),
        "invalid_credential_transport_contract"
    );
    assert_eq!(
        CredentialTransportContractDto::new(vec![CredentialTransportModeDto::SafeHeader], None)
            .expect_err("safe-header support requires its declared header")
            .code(),
        "invalid_credential_transport_contract"
    );
    assert_eq!(
        CredentialTransportContractDto::new(
            vec![CredentialTransportModeDto::Bearer],
            Some("x-api-key".to_owned())
        )
        .expect_err("a header without safe-header support fails")
        .code(),
        "invalid_credential_transport_contract"
    );
}

#[test]
fn closed_provider_enums_keep_their_wire_spellings() {
    assert_eq!(
        serde_json::to_value(CredentialTransportModeDto::Bearer).expect("mode serializes"),
        serde_json::json!("bearer")
    );
    assert_eq!(
        serde_json::to_value(CredentialTransportModeDto::SafeHeader).expect("mode serializes"),
        serde_json::json!("safe_header")
    );
    for mode in [
        CredentialTransportModeDto::Bearer,
        CredentialTransportModeDto::SafeHeader,
    ] {
        assert_eq!(
            CredentialTransportModeDto::parse(mode.as_str()).ok(),
            Some(mode)
        );
        assert_eq!(
            serde_json::from_str::<CredentialTransportModeDto>(
                &serde_json::to_string(&mode).expect("mode serializes")
            )
            .expect("mode decodes"),
            mode
        );
    }
    assert_eq!(
        CredentialTransportModeDto::parse("Bearer")
            .expect_err("an unknown mode fails")
            .code(),
        "invalid_credential_transport_mode"
    );

    let efforts = [
        (ReasoningEffortLevelDto::None, "none"),
        (ReasoningEffortLevelDto::Minimal, "minimal"),
        (ReasoningEffortLevelDto::Low, "low"),
        (ReasoningEffortLevelDto::Medium, "medium"),
        (ReasoningEffortLevelDto::High, "high"),
        (ReasoningEffortLevelDto::XHigh, "xhigh"),
        (ReasoningEffortLevelDto::Max, "max"),
    ];
    for (effort, spelling) in efforts {
        assert_eq!(effort.as_str(), spelling);
        assert_eq!(ReasoningEffortLevelDto::parse(spelling).ok(), Some(effort));
        assert_eq!(
            serde_json::to_value(effort).expect("effort serializes"),
            serde_json::json!(spelling)
        );
    }
    assert_eq!(
        ReasoningEffortLevelDto::parse("ultra")
            .expect_err("an unknown effort fails")
            .code(),
        "invalid_reasoning_effort"
    );

    for (category, spelling) in [
        (ReasoningFragmentCategoryDto::Primary, "primary"),
        (ReasoningFragmentCategoryDto::Detail, "detail"),
    ] {
        assert_eq!(
            serde_json::to_value(category).expect("category serializes"),
            serde_json::json!(spelling)
        );
    }
    assert_eq!(
        serde_json::to_value(ReasoningHistoryTransferDto::Disabled).expect("transfer serializes"),
        serde_json::json!({"mode": "disabled"})
    );
    assert_eq!(
        serde_json::to_value(fixture_transfer()).expect("transfer serializes"),
        serde_json::json!({"mode": "textual_history_v1", "compatibility_id": "fixture-compatibility-v1"})
    );
    assert!(
        serde_json::from_str::<ReasoningHistoryTransferDto>("{\"mode\":\"textual_history_v1\"}")
            .is_err(),
        "a textual transfer without its compatibility identity fails closed"
    );
    assert!(
        serde_json::from_str::<ReasoningHistoryTransferDto>("{\"mode\":\"unknown\"}").is_err(),
        "a closed transfer table rejects unknown modes"
    );
    assert!(
        ReasoningHistoryTransferDto::textual_history_v1(" ")
            .expect_err("a blank compatibility identity fails")
            .code()
            == "invalid_reasoning_history_transfer"
    );

    assert_eq!(
        serde_json::to_value(ModelCapabilityTaxonomyVersionDto::current())
            .expect("taxonomy version serializes"),
        serde_json::json!("model-capability-taxonomy-v1")
    );
    assert_eq!(
        serde_json::to_value(ModelInputKindDto::TextOnly).expect("input kind serializes"),
        serde_json::json!("text_only")
    );
    assert_eq!(
        serde_json::to_value(ProviderCapabilityAvailabilityDto::Enabled)
            .expect("availability serializes"),
        serde_json::json!("enabled")
    );
    assert_eq!(
        serde_json::to_value(ProviderCapabilityAvailabilityDto::Disabled)
            .expect("availability serializes"),
        serde_json::json!("disabled")
    );
    assert_eq!(
        serde_json::to_value(ReasoningCapabilityDto::Disabled).expect("reasoning serializes"),
        serde_json::json!({"kind": "disabled"})
    );
    assert_eq!(
        serde_json::to_value(
            ReasoningCapabilityDto::textual_reasoning_v1(vec![ReasoningEffortLevelDto::High], true)
                .expect("reasoning declaration is valid")
        )
        .expect("reasoning serializes"),
        serde_json::json!({"kind": "textual_reasoning_v1", "supported_efforts": ["high"], "summary_support": true})
    );
    assert!(
        serde_json::from_str::<ReasoningCapabilityDto>("{\"kind\":\"unknown\"}").is_err(),
        "a closed reasoning table rejects unknown kinds"
    );
    assert_eq!(
        serde_json::to_value(ToolExchangeCapabilityDto::Disabled)
            .expect("tool exchange serializes"),
        serde_json::json!({"kind": "disabled"})
    );
    assert_eq!(
        serde_json::to_value(
            ToolExchangeCapabilityDto::model_tool_loop_v1("tool-loop-v1")
                .expect("tool loop is valid")
        )
        .expect("tool exchange serializes"),
        serde_json::json!({"kind": "model_tool_loop_v1", "translation_revision": "tool-loop-v1"})
    );
    assert_eq!(
        serde_json::to_value(ContextPreservationCapabilityDto::local_durable_history_v1(
            ReasoningHistoryTransferDto::Disabled
        ))
        .expect("context preservation serializes"),
        serde_json::json!({"kind": "local_durable_history_v1", "reasoning_input_contract": {"mode": "disabled"}})
    );

    for (policy, spelling) in [
        (LoopbackPolicyDto::NotApplicable, "not_applicable"),
        (LoopbackPolicyDto::ExplicitLoopback, "explicit_loopback"),
    ] {
        assert_eq!(
            serde_json::to_value(policy).expect("loopback serializes"),
            serde_json::json!(spelling)
        );
    }
    for (source, spelling) in [
        (ProviderSelectionSourceDto::GlobalDefault, "global_default"),
        (
            ProviderSelectionSourceDto::SessionDefault,
            "session_default",
        ),
        (ProviderSelectionSourceDto::TurnOverride, "turn_override"),
    ] {
        assert_eq!(
            serde_json::to_value(source).expect("source serializes"),
            serde_json::json!(spelling)
        );
    }
    for (unavailability, spelling) in [
        (ProviderSelectionUnavailabilityDto::Missing, "missing"),
        (ProviderSelectionUnavailabilityDto::Disabled, "disabled"),
        (
            ProviderSelectionUnavailabilityDto::RuntimeUnavailable,
            "runtime_unavailable",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(unavailability).expect("unavailability serializes"),
            serde_json::json!(spelling)
        );
    }
    for (readiness, spelling) in [
        (ProviderProfileReadinessDto::Ready, "ready"),
        (ProviderProfileReadinessDto::Disabled, "disabled"),
        (ProviderProfileReadinessDto::Unavailable, "unavailable"),
    ] {
        assert_eq!(
            serde_json::to_value(readiness).expect("readiness serializes"),
            serde_json::json!(spelling)
        );
    }
    for (state, spelling) in [
        (ProviderCatalogActivationStateDto::Preparing, "preparing"),
        (ProviderCatalogActivationStateDto::Active, "active"),
        (
            ProviderCatalogActivationStateDto::PendingRemoval,
            "pending_removal",
        ),
        (
            ProviderCatalogActivationStateDto::ActivationRecoveryRequired,
            "activation_recovery_required",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(state).expect("activation state serializes"),
            serde_json::json!(spelling)
        );
    }
    for (reason, spelling) in [
        (
            ProviderCatalogDegradedReasonDto::RemovalCandidatePending,
            "removal_candidate_pending",
        ),
        (
            ProviderCatalogDegradedReasonDto::RemovalCandidateRejected,
            "removal_candidate_rejected",
        ),
        (
            ProviderCatalogDegradedReasonDto::ActivationRecoveryRequired,
            "activation_recovery_required",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(reason).expect("degraded reason serializes"),
            serde_json::json!(spelling)
        );
    }
    for (state, spelling) in [
        (ProviderHealthStateDto::Available, "available"),
        (ProviderHealthStateDto::Unavailable, "unavailable"),
    ] {
        assert_eq!(
            serde_json::to_value(state).expect("health state serializes"),
            serde_json::json!(spelling)
        );
    }
    for (reason, spelling) in [
        (
            ProviderHealthReasonDto::CredentialNotConfigured,
            "credential_not_configured",
        ),
        (
            ProviderHealthReasonDto::EndpointUnreachable,
            "endpoint_unreachable",
        ),
        (
            ProviderHealthReasonDto::ProviderRejected,
            "provider_rejected",
        ),
        (ProviderHealthReasonDto::TimedOut, "timed_out"),
    ] {
        assert_eq!(
            serde_json::to_value(reason).expect("health reason serializes"),
            serde_json::json!(spelling)
        );
    }
    assert!(
        serde_json::from_str::<ProviderProfileReadinessDto>("\"unknown\"").is_err(),
        "a closed readiness table rejects unknown values"
    );
}

#[test]
fn provider_execution_policy_ranges_and_defaults() {
    let policy = fixture_execution_policy();
    assert_eq!(policy.attempt_timeout_seconds(), 30);
    assert_eq!(policy.max_attempts(), 2);
    assert_eq!(
        ProviderExecutionPolicyDto::new(0, 2)
            .expect_err("a zero timeout fails")
            .code(),
        "invalid_provider_attempt_timeout_seconds"
    );
    assert_eq!(
        ProviderExecutionPolicyDto::new(61, 2)
            .expect_err("an over-bound timeout fails")
            .code(),
        "invalid_provider_attempt_timeout_seconds"
    );
    assert_eq!(
        ProviderExecutionPolicyDto::new(30, 0)
            .expect_err("a zero attempt budget fails")
            .code(),
        "invalid_provider_max_attempts"
    );
    assert_eq!(
        ProviderExecutionPolicyDto::new(30, 3)
            .expect_err("an over-bound attempt budget fails")
            .code(),
        "invalid_provider_max_attempts"
    );

    let decoded: ProviderExecutionPolicyDto =
        serde_json::from_str("{}").expect("an empty policy decodes with defaults");
    assert_eq!(decoded, fixture_execution_policy());
    let decoded: ProviderExecutionPolicyDto =
        serde_json::from_str("{\"attempt_timeout_seconds\":5,\"max_attempts\":1}")
            .expect("a declared policy decodes");
    assert_eq!(decoded.attempt_timeout_seconds(), 5);
    assert_eq!(decoded.max_attempts(), 1);
    assert!(serde_json::from_str::<ProviderExecutionPolicyDto>("{\"max_attempts\":9}").is_err());
    assert!(
        serde_json::from_str::<ProviderExecutionPolicyDto>(
            "{\"attempt_timeout_seconds\":30,\"max_attempts\":2,\"future\":true}"
        )
        .is_err()
    );
}

#[test]
fn capability_sets_validate_closed_taxonomy_boundaries() {
    let subset = fixture_capability_subset(
        vec![ReasoningEffortLevelDto::Low, ReasoningEffortLevelDto::High],
        false,
    );
    assert!(subset.supports_effort(ReasoningEffortLevelDto::Low));
    assert!(!subset.supports_effort(ReasoningEffortLevelDto::Medium));
    assert!(!subset.supports_summary());
    assert_eq!(
        subset.taxonomy_version().as_str(),
        "model-capability-taxonomy-v1"
    );
    assert_eq!(subset.input(), ModelInputKindDto::TextOnly);
    assert_eq!(
        serde_json::from_str::<ModelCapabilitySetV1>(
            &serde_json::to_string(&subset).expect("subset serializes")
        )
        .expect("subset decodes"),
        subset
    );

    assert_eq!(
        ModelCapabilitySetV1::new(
            ModelCapabilityTaxonomyVersionDto::current(),
            ModelInputKindDto::TextOnly,
            ProviderCapabilityAvailabilityDto::Enabled,
            ProviderCapabilityAvailabilityDto::Enabled,
            ReasoningCapabilityDto::Disabled,
            ToolExchangeCapabilityDto::Disabled,
            ContextPreservationCapabilityDto::local_durable_history_v1(
                ReasoningHistoryTransferDto::Disabled
            ),
        )
        .expect_err("structured output requires a new taxonomy version")
        .code(),
        "invalid_model_capability_set"
    );
    assert_eq!(
        ReasoningCapabilityDto::textual_reasoning_v1(
            vec![ReasoningEffortLevelDto::High, ReasoningEffortLevelDto::High],
            false
        )
        .expect_err("a repeated effort fails")
        .code(),
        "invalid_model_capability_set"
    );
    assert_eq!(
        ToolExchangeCapabilityDto::model_tool_loop_v1(" ")
            .expect_err("a blank translation revision fails")
            .code(),
        "invalid_model_capability_set"
    );
    assert!(
        serde_json::from_str::<ModelCapabilitySetV1>(
            r#"{"taxonomy_version":"model-capability-taxonomy-v2","input":"text_only","text_streaming":"enabled","structured_output":"disabled","reasoning":{"kind":"disabled"},"tool_exchange":{"kind":"disabled"},"context_preservation":{"kind":"local_durable_history_v1","reasoning_input_contract":{"mode":"disabled"}}}"#
        )
        .is_err(),
        "an unknown taxonomy version fails closed"
    );
}

#[test]
fn reasoning_policies_require_transferable_material_without_repeats() {
    let policy = fixture_reasoning_policy();
    assert_eq!(policy.effort(), Some(ReasoningEffortLevelDto::Medium));
    assert!(policy.summary_support());
    assert_eq!(
        policy.transfer().compatibility_id(),
        Some("fixture-compatibility-v1")
    );
    assert_eq!(policy.supported_categories().len(), 2);
    assert_eq!(
        serde_json::from_str::<ResolvedReasoningPolicyDto>(
            &serde_json::to_string(&policy).expect("policy serializes")
        )
        .expect("policy decodes"),
        policy
    );
    assert_eq!(
        ResolvedReasoningPolicyDto::new(
            None,
            false,
            fixture_transfer(),
            vec![
                ReasoningFragmentCategoryDto::Primary,
                ReasoningFragmentCategoryDto::Primary
            ],
        )
        .expect_err("a repeated category fails")
        .code(),
        "invalid_resolved_reasoning_policy"
    );
    assert_eq!(
        ResolvedReasoningPolicyDto::new(None, false, fixture_transfer(), Vec::new())
            .expect_err("a textual transfer without material fails")
            .code(),
        "invalid_resolved_reasoning_policy"
    );
    assert!(
        ResolvedReasoningPolicyDto::new(
            None,
            false,
            ReasoningHistoryTransferDto::Disabled,
            Vec::new()
        )
        .is_ok(),
        "a disabled transfer with no material is a coherent policy"
    );
}

#[test]
fn profile_revisions_validate_endpoint_loopback_and_capability_coherence() {
    let revision = fixture_profile_revision();
    assert_eq!(revision.profile_id(), &fixture_profile_id());
    assert_eq!(revision.model_id(), "fixture-model");
    assert_eq!(
        revision.normalized_effective_endpoint(),
        Some("https://provider.example/v1")
    );
    assert_eq!(
        revision.effective_loopback_policy(),
        LoopbackPolicyDto::NotApplicable
    );
    assert_eq!(
        serde_json::from_str::<ProviderProfileRevisionV1>(
            &serde_json::to_string(&revision).expect("revision serializes")
        )
        .expect("revision decodes"),
        revision
    );

    let capabilities = fixture_capability_subset(vec![ReasoningEffortLevelDto::Low], false);
    let build = |endpoint: Option<&str>,
                 loopback: LoopbackPolicyDto,
                 policy: ResolvedReasoningPolicyDto,
                 subset: ModelCapabilitySetV1| {
        ProviderProfileRevisionV1::new(
            fixture_profile_id(),
            ProviderProfileRevisionId::new(),
            fixture_kind_id(),
            ProviderKindDescriptorRevisionId::new(),
            "fixture-model",
            endpoint.map(str::to_owned),
            CredentialTransportDto::bearer(),
            subset,
            policy,
            fixture_execution_policy(),
            loopback,
        )
    };
    let unsupported_effort = ResolvedReasoningPolicyDto::new(
        Some(ReasoningEffortLevelDto::High),
        false,
        fixture_transfer(),
        vec![ReasoningFragmentCategoryDto::Primary],
    )
    .expect("fixture policy is valid");
    assert_eq!(
        build(
            Some("https://provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            unsupported_effort,
            capabilities.clone(),
        )
        .expect_err("an undeclared effort fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    let unsupported_summary = ResolvedReasoningPolicyDto::new(
        None,
        true,
        fixture_transfer(),
        vec![ReasoningFragmentCategoryDto::Primary],
    )
    .expect("fixture policy is valid");
    assert_eq!(
        build(
            Some("https://provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            unsupported_summary,
            capabilities.clone(),
        )
        .expect_err("an undeclared summary fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    let mismatched_transfer = ResolvedReasoningPolicyDto::new(
        None,
        false,
        ReasoningHistoryTransferDto::Disabled,
        vec![ReasoningFragmentCategoryDto::Primary],
    )
    .expect("fixture policy is valid");
    assert_eq!(
        build(
            Some("https://provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            mismatched_transfer,
            capabilities.clone(),
        )
        .expect_err("a transfer the subset does not declare fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    assert_eq!(
        build(
            Some("https://user@provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            fixture_reasoning_policy(),
            capabilities.clone()
        )
        .expect_err("userinfo fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    assert_eq!(
        build(
            Some("https://provider.example/v1?query=1"),
            LoopbackPolicyDto::NotApplicable,
            fixture_reasoning_policy(),
            capabilities.clone()
        )
        .expect_err("a query fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    assert_eq!(
        build(
            Some("https://provider.example/v1#fragment"),
            LoopbackPolicyDto::NotApplicable,
            fixture_reasoning_policy(),
            capabilities.clone()
        )
        .expect_err("a fragment fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    assert_eq!(
        build(
            Some("provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            fixture_reasoning_policy(),
            capabilities.clone()
        )
        .expect_err("a relative endpoint fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    assert_eq!(
        build(
            Some("http://localhost:8080/v1"),
            LoopbackPolicyDto::NotApplicable,
            fixture_reasoning_policy(),
            capabilities.clone()
        )
        .expect_err("HTTP without an explicit loopback policy fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    assert_eq!(
        build(
            Some("http://192.168.1.4/v1"),
            LoopbackPolicyDto::ExplicitLoopback,
            fixture_reasoning_policy(),
            capabilities
        )
        .expect_err("HTTP outside loopback fails")
        .code(),
        "invalid_provider_profile_revision"
    );
    for loopback_endpoint in [
        "http://localhost/v1",
        "http://127.0.0.1:1234/v1",
        "http://[::1]:1234/v1",
    ] {
        assert!(
            build(
                Some(loopback_endpoint),
                LoopbackPolicyDto::ExplicitLoopback,
                fixture_reasoning_policy(),
                fixture_capability_subset(
                    vec![
                        ReasoningEffortLevelDto::Low,
                        ReasoningEffortLevelDto::Medium,
                        ReasoningEffortLevelDto::High,
                    ],
                    true,
                ),
            )
            .is_ok(),
            "{loopback_endpoint} is an explicit loopback endpoint"
        );
    }
    assert_eq!(
        ProviderProfileRevisionV1::new(
            fixture_profile_id(),
            ProviderProfileRevisionId::new(),
            fixture_kind_id(),
            ProviderKindDescriptorRevisionId::new(),
            " ",
            None,
            CredentialTransportDto::bearer(),
            fixture_capability_subset(vec![ReasoningEffortLevelDto::Medium], true),
            fixture_reasoning_policy(),
            fixture_execution_policy(),
            LoopbackPolicyDto::NotApplicable,
        )
        .expect_err("a blank model identity fails")
        .code(),
        "invalid_provider_profile_revision"
    );
}

#[test]
fn resolved_run_selections_derive_from_profile_revisions() {
    let revision = fixture_profile_revision();
    let driver = fixture_driver_contract();
    let selection = ResolvedRunProviderSelectionDto::from_profile_revision(
        &revision,
        driver.clone(),
        ProviderSelectionSourceDto::SessionDefault,
    );
    assert_eq!(
        selection.selection_contract_revision(),
        ResolvedRunProviderSelectionDto::SELECTION_CONTRACT_REVISION
    );
    assert_eq!(selection.profile_id(), revision.profile_id());
    assert_eq!(
        selection.provider_profile_revision_id(),
        revision.revision_id()
    );
    assert_eq!(selection.kind_id(), revision.kind_id());
    assert_eq!(
        selection.kind_descriptor_revision_id(),
        revision.kind_descriptor_revision_id()
    );
    assert_eq!(selection.model_id(), "fixture-model");
    assert_eq!(
        selection.normalized_effective_endpoint(),
        Some("https://provider.example/v1")
    );
    assert_eq!(
        selection.credential_transport_mode(),
        CredentialTransportModeDto::Bearer
    );
    assert_eq!(selection.credential_transport_safe_header_name(), None);
    assert_eq!(
        selection.effective_execution_policy(),
        fixture_execution_policy()
    );
    assert_eq!(
        selection.effective_loopback_policy(),
        LoopbackPolicyDto::NotApplicable
    );
    assert_eq!(selection.provider_driver_contract_revision(), &driver);
    assert_eq!(
        selection.selection_source(),
        ProviderSelectionSourceDto::SessionDefault
    );
    assert_eq!(
        serde_json::from_str::<ResolvedRunProviderSelectionDto>(
            &serde_json::to_string(&selection).expect("selection serializes")
        )
        .expect("selection decodes"),
        selection
    );

    let build = |endpoint: Option<&str>,
                 loopback: LoopbackPolicyDto,
                 mode: CredentialTransportModeDto,
                 header: Option<&str>| {
        ResolvedRunProviderSelectionDto::new(
            ResolvedRunProviderSelectionDto::SELECTION_CONTRACT_REVISION,
            fixture_profile_id(),
            ProviderProfileRevisionId::new(),
            fixture_kind_id(),
            ProviderKindDescriptorRevisionId::new(),
            "fixture-model",
            endpoint.map(str::to_owned),
            mode,
            header.map(str::to_owned),
            fixture_capability_subset(vec![ReasoningEffortLevelDto::Medium], true),
            fixture_reasoning_policy(),
            fixture_execution_policy(),
            loopback,
            fixture_driver_contract(),
            ProviderSelectionSourceDto::TurnOverride,
        )
    };
    assert!(
        build(
            Some("https://provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            CredentialTransportModeDto::Bearer,
            None,
        )
        .is_ok()
    );
    assert_eq!(
        build(
            Some("https://provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            CredentialTransportModeDto::Bearer,
            Some("x-api-key"),
        )
        .expect_err("a bearer selection with a header fails")
        .code(),
        "invalid_resolved_run_provider_selection"
    );
    assert_eq!(
        build(
            Some("https://provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            CredentialTransportModeDto::SafeHeader,
            None,
        )
        .expect_err("a safe-header selection without a header fails")
        .code(),
        "invalid_resolved_run_provider_selection"
    );
    assert_eq!(
        build(
            Some("http://127.0.0.1/v1"),
            LoopbackPolicyDto::NotApplicable,
            CredentialTransportModeDto::Bearer,
            None,
        )
        .expect_err("HTTP without an explicit loopback policy fails")
        .code(),
        "invalid_resolved_run_provider_selection"
    );
    assert_eq!(
        build(
            Some("https://provider.example/v1"),
            LoopbackPolicyDto::NotApplicable,
            CredentialTransportModeDto::Bearer,
            None,
        )
        .expect("valid selection")
        .resolved_reasoning_policy()
        .supported_categories()
        .len(),
        2
    );
}

#[test]
fn reasoning_history_manifests_and_bounds_validate_their_contracts() {
    let manifest_id = ReasoningHistoryManifestId::new();
    let source = ReasoningHistorySourceEntryDto::new(
        SessionId::new(),
        intention_proto::RunId::new(),
        Some(7),
        vec![
            ReasoningHistoryRecordReferenceDto::new(ReasoningFragmentCategoryDto::Primary, 1024)
                .expect("record reference is valid"),
            ReasoningHistoryRecordReferenceDto::new(ReasoningFragmentCategoryDto::Detail, 512)
                .expect("record reference is valid"),
        ],
    );
    assert_eq!(source.records().len(), 2);
    assert_eq!(source.final_assistant_message_id(), Some(7));
    assert_eq!(
        serde_json::from_str::<ReasoningHistorySourceEntryDto>(
            &serde_json::to_string(&source).expect("source serializes")
        )
        .expect("source decodes"),
        source
    );
    assert_eq!(
        ReasoningHistoryRecordReferenceDto::new(ReasoningFragmentCategoryDto::Primary, 0)
            .expect_err("a zero-size record reference fails")
            .code(),
        "invalid_reasoning_history_manifest"
    );

    let manifest = ReasoningHistoryManifestDto::new(
        manifest_id,
        fixture_transfer(),
        Some("fixture-compatibility-v1".to_owned()),
        vec![source],
        1536,
    )
    .expect("coherent manifest is valid");
    assert_eq!(manifest.manifest_id(), manifest_id);
    assert_eq!(manifest.aggregate_size_bytes(), 1536);
    assert_eq!(
        serde_json::from_str::<ReasoningHistoryManifestDto>(
            &serde_json::to_string(&manifest).expect("manifest serializes")
        )
        .expect("manifest decodes"),
        manifest
    );
    assert_eq!(
        ReasoningHistoryManifestDto::new(
            manifest_id,
            fixture_transfer(),
            Some("other-compatibility".to_owned()),
            Vec::new(),
            0,
        )
        .expect_err("a mismatched compatibility identity fails")
        .code(),
        "invalid_reasoning_history_manifest"
    );
    assert_eq!(
        ReasoningHistoryManifestDto::new(
            manifest_id,
            ReasoningHistoryTransferDto::Disabled,
            Some("fixture-compatibility-v1".to_owned()),
            Vec::new(),
            0,
        )
        .expect_err("a disabled transfer carries no compatibility identity")
        .code(),
        "invalid_reasoning_history_manifest"
    );
    assert!(
        ReasoningHistoryManifestDto::new(
            manifest_id,
            ReasoningHistoryTransferDto::Disabled,
            None,
            Vec::new(),
            0,
        )
        .is_ok()
    );

    let bound = ReasoningHistoryBoundDto::new(
        manifest_id,
        fixture_transfer(),
        Some("fixture-compatibility-v1".to_owned()),
        1,
        1536,
    )
    .expect("coherent bound is valid");
    assert_eq!(bound.source_entry_count(), 1);
    assert_eq!(bound.aggregate_size_bytes(), 1536);
    assert!(
        !serde_json::to_string(&bound)
            .expect("bound serializes")
            .contains("reasoning text"),
        "a bound carries no reasoning text"
    );
    assert_eq!(
        ReasoningHistoryBoundDto::new(manifest_id, fixture_transfer(), None, 0, 0)
            .expect_err("a mismatched compatibility identity fails")
            .code(),
        "invalid_reasoning_history_manifest"
    );
}

#[test]
fn user_kind_compositions_use_closed_parts_only() {
    let composition = UserKindCompositionDto::new(
        ProviderKindId::parse("my-kind").expect("fixture user kind identity is valid"),
        UserKindStreamPartDto::ChatCompletionsSse,
        UserKindReasoningPartDto::ReasoningContent,
        UserKindActivationPartDto::ThinkingEnabled,
        UserKindEffortPartDto::ReasoningEffort,
        CredentialTransportDto::bearer(),
    )
    .expect("coherent user kind composition is valid");
    assert_eq!(composition.kind_id().as_str(), "my-kind");
    assert_eq!(
        composition.stream(),
        UserKindStreamPartDto::ChatCompletionsSse
    );
    assert_eq!(
        composition.reasoning(),
        UserKindReasoningPartDto::ReasoningContent
    );
    assert_eq!(
        composition.activation(),
        UserKindActivationPartDto::ThinkingEnabled
    );
    assert_eq!(composition.effort(), UserKindEffortPartDto::ReasoningEffort);
    assert_eq!(
        serde_json::from_str::<UserKindCompositionDto>(
            &serde_json::to_string(&composition).expect("composition serializes")
        )
        .expect("composition decodes"),
        composition
    );

    for reserved in ProviderKindId::FIRST_PARTY_IDS {
        let reserved_kind =
            ProviderKindId::parse(reserved).expect("reserved identity is a valid token");
        assert_eq!(
            UserKindCompositionDto::new(
                reserved_kind,
                UserKindStreamPartDto::ChatCompletionsSse,
                UserKindReasoningPartDto::Absent,
                UserKindActivationPartDto::Absent,
                UserKindEffortPartDto::Absent,
                CredentialTransportDto::bearer(),
            )
            .expect_err("a reserved first-party identity fails")
            .code(),
            "invalid_user_kind_composition"
        );
    }

    assert_eq!(
        serde_json::to_value(UserKindStreamPartDto::ChatCompletionsSse).expect("part serializes"),
        serde_json::json!("chat_completions_sse")
    );
    for (part, spelling) in [
        (UserKindReasoningPartDto::Absent, "absent"),
        (
            UserKindReasoningPartDto::ReasoningContent,
            "reasoning_content",
        ),
        (UserKindReasoningPartDto::Reasoning, "reasoning"),
        (
            UserKindReasoningPartDto::ReasoningDetailsText,
            "reasoning_details_text",
        ),
        (
            UserKindReasoningPartDto::MessageThinking,
            "message_thinking",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(part).expect("part serializes"),
            serde_json::json!(spelling)
        );
    }
    for (part, spelling) in [
        (UserKindActivationPartDto::Absent, "absent"),
        (
            UserKindActivationPartDto::ThinkingEnabled,
            "thinking_enabled",
        ),
        (
            UserKindActivationPartDto::ThinkingAdaptive,
            "thinking_adaptive",
        ),
        (UserKindActivationPartDto::EnableThinking, "enable_thinking"),
        (UserKindActivationPartDto::ThinkBoolean, "think_boolean"),
        (UserKindActivationPartDto::ThinkEffort, "think_effort"),
    ] {
        assert_eq!(
            serde_json::to_value(part).expect("part serializes"),
            serde_json::json!(spelling)
        );
    }
    for (part, spelling) in [
        (UserKindEffortPartDto::Absent, "absent"),
        (UserKindEffortPartDto::ReasoningEffort, "reasoning_effort"),
        (UserKindEffortPartDto::ThinkingBudget, "thinking_budget"),
        (
            UserKindEffortPartDto::ThinkingTokenBudget,
            "thinking_token_budget",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(part).expect("part serializes"),
            serde_json::json!(spelling)
        );
    }
    assert!(
        serde_json::from_str::<UserKindReasoningPartDto>("\"raw_json_template\"").is_err(),
        "a user kind part table is closed"
    );
    assert!(
        serde_json::from_str::<UserKindCompositionDto>(
            r#"{"kind_id":"my-kind","stream":"ollama_native","reasoning":"absent","activation":"absent","effort":"absent","credential_transport":{"mode":"bearer"}}"#
        )
        .is_err(),
        "an unmodeled stream framing fails closed"
    );
}

#[test]
fn catalog_pages_and_status_validate_their_coherence() {
    let entry = provider_entry();
    assert!(ProviderCatalogPageDto::new(None, None, Vec::new(), None, false).is_ok());
    assert_eq!(
        ProviderCatalogPageDto::new(None, None, Vec::new(), Some("next-page".to_owned()), false,)
            .expect_err("a token without more entries fails")
            .code(),
        "invalid_provider_catalog_page"
    );
    assert_eq!(
        ProviderCatalogPageDto::new(None, None, Vec::new(), None, true,)
            .expect_err("more entries without a token fails")
            .code(),
        "invalid_provider_catalog_page"
    );
    assert_eq!(
        ProviderCatalogPageDto::new(None, None, vec![entry.clone(), entry.clone()], None, false,)
            .expect_err("unordered entries fail")
            .code(),
        "invalid_provider_catalog_page"
    );
    let ordered = ProviderCatalogPageDto::new(
        Some(CatalogRevisionId::new()),
        Some(fixture_profile_id()),
        vec![entry],
        Some("next-page".to_owned()),
        true,
    )
    .expect("ordered page is valid");
    assert!(ordered.has_more());
    assert_eq!(ordered.next_page_token(), Some("next-page"));
    assert_eq!(
        serde_json::from_str::<ProviderCatalogPageDto>(
            &serde_json::to_string(&ordered).expect("page serializes")
        )
        .expect("page decodes"),
        ordered
    );

    assert_eq!(
        ListProviderCatalogQueryDto::new(Some(" ".to_owned()))
            .expect_err("a malformed page token fails")
            .code(),
        "provider_catalog_page_token_invalid"
    );
    assert_eq!(
        ProviderCatalogPageDto::new(None, None, Vec::new(), Some("bad token".to_owned()), true)
            .expect_err("a malformed page token fails")
            .code(),
        "provider_catalog_page_token_invalid"
    );
    assert!(ListProviderCatalogQueryDto::new(None).is_ok());

    let candidate =
        ProviderCatalogCandidateHandleDto::new(CatalogRevisionId::new(), CatalogRevisionId::new())
            .expect("candidate handle is valid");
    assert_ne!(
        candidate.candidate_revision_id(),
        candidate.expected_active_revision_id()
    );
    let same = CatalogRevisionId::new();
    assert_eq!(
        ProviderCatalogCandidateHandleDto::new(same, same)
            .expect_err("a candidate equal to the active revision fails")
            .code(),
        "invalid_provider_catalog_candidate_handle"
    );

    assert_eq!(
        ProviderCatalogStatusDto::new(
            ProviderCatalogActivationStateDto::PendingRemoval,
            Some(ProviderCatalogDegradedReasonDto::RemovalCandidatePending),
            Some(CatalogRevisionId::new()),
            None,
            None,
            Vec::new(),
        )
        .expect_err("a pending removal without its candidate fails")
        .code(),
        "invalid_provider_catalog_status"
    );
    let status = ProviderCatalogStatusDto::new(
        ProviderCatalogActivationStateDto::PendingRemoval,
        Some(ProviderCatalogDegradedReasonDto::RemovalCandidatePending),
        Some(CatalogRevisionId::new()),
        Some(candidate),
        Some(fixture_profile_id()),
        vec![
            ProviderCatalogValidationIssueDto::new(
                "profile_model_missing",
                Some(fixture_profile_id()),
                None,
            )
            .expect("validation issue is valid"),
        ],
    )
    .expect("coherent status is valid");
    assert_eq!(
        status.activation_state(),
        ProviderCatalogActivationStateDto::PendingRemoval
    );
    assert_eq!(status.validation_issues().len(), 1);
    assert_eq!(
        status.validation_issues()[0].code(),
        "profile_model_missing"
    );
    assert_eq!(
        ProviderCatalogValidationIssueDto::new("ProfileMissing", None, None)
            .expect_err("a non-snake_case code fails")
            .code(),
        "invalid_provider_catalog_validation_issue"
    );
    assert_eq!(
        serde_json::from_str::<ProviderCatalogStatusDto>(
            &serde_json::to_string(&status).expect("status serializes")
        )
        .expect("status decodes"),
        status
    );
}

#[test]
fn session_provider_profile_payloads_validate_their_projection() {
    let profile_id = fixture_profile_id();
    let command = SetSessionProviderProfileCommandDto::new(
        SessionId::new(),
        profile_id.clone(),
        4,
        IdempotencyKey::new(),
    );
    assert_eq!(command.profile_id(), &profile_id);
    assert_eq!(command.expected_session_projection_revision(), 4);
    assert_eq!(
        serde_json::from_str::<SetSessionProviderProfileCommandDto>(
            &serde_json::to_string(&command).expect("command serializes")
        )
        .expect("command decodes"),
        command
    );
    let accepted = SetSessionProviderProfileAcceptedDto::new(command.session_id(), false, 4);
    assert!(!accepted.changed());
    assert_eq!(accepted.session_projection_revision(), 4);
    let _ = GetSessionProviderProfileQueryDto::new(command.session_id());

    let entry = provider_entry();
    let projection = SessionProviderProfileProjectionDto::new(
        command.session_id(),
        Some(profile_id.clone()),
        Some(entry.clone()),
        None,
        4,
        Some(profile_id.clone()),
    )
    .expect("resolved projection is valid");
    assert_eq!(projection.durable_profile_id(), Some(&profile_id));
    assert!(projection.resolved_entry().is_some());
    assert_eq!(projection.unavailability(), None);
    assert_eq!(
        serde_json::from_str::<SessionProviderProfileProjectionDto>(
            &serde_json::to_string(&projection).expect("projection serializes")
        )
        .expect("projection decodes"),
        projection
    );
    assert_eq!(
        SessionProviderProfileProjectionDto::new(
            command.session_id(),
            Some(profile_id.clone()),
            Some(entry),
            Some(ProviderSelectionUnavailabilityDto::Missing),
            4,
            None,
        )
        .expect_err("a resolved and unavailable projection fails")
        .code(),
        "invalid_session_provider_profile_projection"
    );
    let unavailable = SessionProviderProfileProjectionDto::new(
        command.session_id(),
        Some(profile_id.clone()),
        None,
        Some(ProviderSelectionUnavailabilityDto::RuntimeUnavailable),
        5,
        None,
    )
    .expect("unavailable projection is valid");
    assert!(unavailable.resolved_entry().is_none());

    let changed = SessionProviderProfileChangedDto::new(command.session_id(), profile_id, 5);
    assert_eq!(changed.session_projection_revision(), 5);
    assert_eq!(
        serde_json::from_str::<SessionProviderProfileChangedDto>(
            &serde_json::to_string(&changed).expect("change serializes")
        )
        .expect("change decodes"),
        changed
    );
}

#[test]
fn health_evidence_and_discovery_results_are_non_authorizing() {
    let profile_id = fixture_profile_id();
    let available =
        ProviderHealthEvidenceDto::new(profile_id.clone(), ProviderHealthStateDto::Available, None)
            .expect("available evidence is valid");
    assert_eq!(available.provider_id(), &profile_id);
    assert_eq!(available.state(), ProviderHealthStateDto::Available);
    assert_eq!(available.reason(), None);
    assert!(
        !serde_json::to_string(&available)
            .expect("evidence serializes")
            .contains("revision"),
        "health evidence reports no profile revision"
    );
    let unavailable = ProviderHealthEvidenceDto::new(
        profile_id.clone(),
        ProviderHealthStateDto::Unavailable,
        Some(ProviderHealthReasonDto::CredentialNotConfigured),
    )
    .expect("unavailable evidence is valid");
    assert_eq!(
        serde_json::from_str::<ProviderHealthEvidenceDto>(
            &serde_json::to_string(&unavailable).expect("evidence serializes")
        )
        .expect("evidence decodes"),
        unavailable
    );
    assert_eq!(
        ProviderHealthEvidenceDto::new(
            profile_id.clone(),
            ProviderHealthStateDto::Unavailable,
            None,
        )
        .expect_err("an unavailable provider retains a reason")
        .code(),
        "invalid_provider_health_evidence"
    );
    assert_eq!(
        ProviderHealthEvidenceDto::new(
            profile_id,
            ProviderHealthStateDto::Available,
            Some(ProviderHealthReasonDto::TimedOut),
        )
        .expect_err("an available provider carries no reason")
        .code(),
        "invalid_provider_health_evidence"
    );

    let record = ProviderModelRecordDto::new("fixture-model", Some("Fixture".to_owned()))
        .expect("model record is valid");
    assert_eq!(record.model_id(), "fixture-model");
    let result = ProviderDiscoveryResultDto::new(ProviderDiscoveryAttemptId::new(), vec![record])
        .expect("discovery result is valid");
    assert_eq!(
        serde_json::from_str::<ProviderDiscoveryResultDto>(
            &serde_json::to_string(&result).expect("result serializes")
        )
        .expect("result decodes"),
        result
    );
    assert_eq!(
        ProviderDiscoveryResultDto::new(
            ProviderDiscoveryAttemptId::new(),
            vec![
                ProviderModelRecordDto::new("fixture-model", None).expect("record is valid"),
                ProviderModelRecordDto::new("fixture-model", None).expect("record is valid"),
            ],
        )
        .expect_err("a repeated discovered model fails")
        .code(),
        "invalid_provider_discovery_result"
    );
    assert_eq!(
        ProviderModelRecordDto::new("  ", None)
            .expect_err("a blank model identity fails")
            .code(),
        "invalid_provider_model_record"
    );
}

#[test]
fn configuration_commands_and_edits_are_closed_and_credential_free() {
    let profile_id = fixture_profile_id();
    let reload = ReloadConfigurationCommandDto::new(IdempotencyKey::new());
    assert_eq!(
        serde_json::from_str::<ReloadConfigurationCommandDto>(
            &serde_json::to_string(&reload).expect("reload serializes")
        )
        .expect("reload decodes"),
        reload
    );
    let accepted = ConfigurationReloadAcceptedDto::new(
        intention_proto::ConfigRevisionId::new(),
        Some(CatalogRevisionId::new()),
    );
    assert!(accepted.catalog_revision_id().is_some());
    let accepted =
        ConfigurationEditAcceptedDto::new(intention_proto::ConfigRevisionId::new(), true);
    assert!(accepted.requires_restart());
    assert_eq!(
        serde_json::from_str::<ConfigurationEditAcceptedDto>(
            &serde_json::to_string(&accepted).expect("edit acceptance serializes")
        )
        .expect("edit acceptance decodes"),
        accepted
    );

    for edit in [
        ConfigurationEditDto::set_default_profile(profile_id.clone()),
        ConfigurationEditDto::set_profile_enabled(profile_id.clone(), false),
        ConfigurationEditDto::set_profile_display_name(profile_id.clone(), "Main profile")
            .expect("display-name edit is valid"),
    ] {
        assert_eq!(edit.profile_id(), &profile_id);
        assert_eq!(
            serde_json::from_str::<ConfigurationEditDto>(
                &serde_json::to_string(&edit).expect("edit serializes")
            )
            .expect("edit decodes"),
            edit
        );
    }
    assert_eq!(
        ConfigurationEditDto::set_profile_display_name(profile_id, "   ")
            .expect_err("a blank display name fails")
            .code(),
        "invalid_configuration_edit"
    );
    assert!(
        serde_json::from_str::<ConfigurationEditDto>(
            r#"{"kind":"set_profile_credential","profile_id":"main","credential":"fixture"}"#
        )
        .is_err(),
        "the closed edit table rejects unknown edits"
    );
    assert!(
        serde_json::from_str::<ConfigurationEditDto>(
            r#"{"kind":"set_profile_display_name","profile_id":"main","display_name":" "}"#
        )
        .is_err(),
        "a blank display name fails on decode"
    );

    assert_eq!(
        ApplyConfigurationDocumentCommandDto::new("   ", IdempotencyKey::new())
            .expect_err("an empty document fails")
            .code(),
        "invalid_configuration_edit"
    );
    assert_eq!(
        ApplyConfigurationEditsCommandDto::new(Vec::new(), IdempotencyKey::new())
            .expect_err("an empty edit list fails")
            .code(),
        "invalid_configuration_edit"
    );
    let document =
        ApplyConfigurationDocumentCommandDto::new("schema_version = 1\n", IdempotencyKey::new())
            .expect("document command is valid");
    assert_eq!(document.document(), "schema_version = 1\n");
    assert!(
        !serde_json::to_string(&document)
            .expect("document serializes")
            .contains("credential"),
        "the applied document carries no credential field"
    );
}

#[test]
fn rotation_and_catalog_removal_commands_carry_their_identity() {
    let profile_id = fixture_profile_id();
    let rotation =
        RotateProviderCredentialCommandDto::new(profile_id.clone(), IdempotencyKey::new());
    assert_eq!(rotation.profile_id(), &profile_id);
    assert!(
        !serde_json::to_string(&rotation)
            .expect("rotation serializes")
            .contains("credential\""),
        "a rotation command carries no credential material"
    );
    assert_eq!(
        CredentialRotationAcceptedDto::new(profile_id.clone()).profile_id(),
        &profile_id
    );
    let health = CheckProviderHealthCommandDto::new(profile_id.clone());
    assert_eq!(health.profile_id(), &profile_id);
    let discovery = DiscoverProviderModelsCommandDto::new(profile_id);
    assert_eq!(
        serde_json::from_str::<DiscoverProviderModelsCommandDto>(
            &serde_json::to_string(&discovery).expect("discovery serializes")
        )
        .expect("discovery decodes"),
        discovery
    );

    let handle =
        ProviderCatalogCandidateHandleDto::new(CatalogRevisionId::new(), CatalogRevisionId::new())
            .expect("candidate handle is valid");
    let accept = AcceptProviderCatalogRemovalCommandDto::new(handle, IdempotencyKey::new());
    assert_eq!(accept.candidate(), handle);
    let reject = RejectProviderCatalogCandidateCommandDto::new(handle, IdempotencyKey::new());
    assert_eq!(reject.candidate(), handle);
    let accepted = ProviderCatalogRemovalAcceptedDto::new(CatalogRevisionId::new());
    assert_eq!(
        serde_json::from_str::<ProviderCatalogRemovalAcceptedDto>(
            &serde_json::to_string(&accepted).expect("acceptance serializes")
        )
        .expect("acceptance decodes"),
        accepted
    );
    let rejected = ProviderCatalogCandidateRejectedDto::new(Some(CatalogRevisionId::new()));
    assert!(rejected.active_catalog_revision_id().is_some());
}

#[test]
fn pricing_and_profile_policy_validate_their_boundaries() {
    let pricing = ProviderPricingPolicyDto::new(Some(3.0), Some(15.0)).expect("pricing is valid");
    assert_eq!(pricing.input_per_million_tokens(), Some(3.0));
    assert_eq!(pricing.output_per_million_tokens(), Some(15.0));
    assert_eq!(
        serde_json::from_str::<ProviderPricingPolicyDto>(
            &serde_json::to_string(&pricing).expect("pricing serializes")
        )
        .expect("pricing decodes"),
        pricing
    );
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        assert_eq!(
            ProviderPricingPolicyDto::new(Some(invalid), None)
                .expect_err("an invalid price fails")
                .code(),
            "invalid_provider_pricing_policy"
        );
    }
    assert!(ProviderPricingPolicyDto::new(None, None).is_ok());

    let policy = ProviderProfilePolicyDto::new("Main", true, Some(pricing))
        .expect("profile policy is valid");
    assert_eq!(policy.display_name(), "Main");
    assert!(policy.enabled());
    assert_eq!(policy.pricing(), Some(&pricing));
    assert_eq!(
        serde_json::from_str::<ProviderProfilePolicyDto>(
            &serde_json::to_string(&policy).expect("profile policy serializes")
        )
        .expect("profile policy decodes"),
        policy
    );
    assert_eq!(
        ProviderProfilePolicyDto::new("  ", true, None)
            .expect_err("a blank display name fails")
            .code(),
        "invalid_provider_profile_policy"
    );
    assert!(
        ProviderProfilePolicyDto::new("Main", true, None)
            .expect("policy without pricing is valid")
            .pricing()
            .is_none()
    );
    assert!(
        serde_json::from_str::<ProviderProfilePolicyDto>(
            r#"{"display_name":"Main","enabled":true,"pricing":{"input_per_million_tokens":-1.0}}"#
        )
        .is_err(),
        "a negative price fails on decode"
    );
}

#[test]
fn provider_endpoint_policies_and_driver_capabilities_are_explicit() {
    let policy = ProviderEndpointPolicyDto::new(true, false, true);
    assert!(policy.requires_explicit_endpoint());
    assert!(!policy.permits_override());
    assert!(policy.permits_loopback_http());
    assert_eq!(
        serde_json::from_str::<ProviderEndpointPolicyDto>(
            &serde_json::to_string(&policy).expect("endpoint policy serializes")
        )
        .expect("endpoint policy decodes"),
        policy
    );
    let capabilities = ProviderDriverCapabilitiesDto::new(true, true, false);
    assert!(capabilities.text_streaming());
    assert!(capabilities.reasoning());
    assert!(!capabilities.tool_calls());
    assert_eq!(
        serde_json::from_str::<ProviderDriverCapabilitiesDto>(
            &serde_json::to_string(&capabilities).expect("capabilities serialize")
        )
        .expect("capabilities decode"),
        capabilities
    );

    let driver = fixture_driver_contract();
    assert_eq!(driver.driver_family(), "fixture-driver");
    assert_eq!(driver.major(), 1);
    assert_eq!(driver.minor(), 0);
    assert_eq!(
        ProviderDriverContractRevisionDto::new(" ", 1, 0)
            .expect_err("a blank driver family fails")
            .code(),
        "invalid_provider_driver_contract_revision"
    );

    let descriptor = ProviderKindDescriptorRevisionV1::new(
        fixture_kind_id(),
        ProviderKindDescriptorRevisionId::new(),
        "fixture-descriptor",
        vec!["protocol-part-v1".to_owned()],
        ProviderEndpointPolicyDto::new(true, false, false),
        CredentialTransportContractDto::new(vec![CredentialTransportModeDto::Bearer], None)
            .expect("fixture contract is valid"),
        fixture_capability_subset(vec![ReasoningEffortLevelDto::Medium], true),
        "fixture-driver",
    )
    .expect("fixture descriptor is valid");
    assert_eq!(descriptor.descriptor_family(), "fixture-descriptor");
    assert_eq!(descriptor.ordered_protocol_part_revisions().len(), 1);
    assert_eq!(
        serde_json::from_str::<ProviderKindDescriptorRevisionV1>(
            &serde_json::to_string(&descriptor).expect("descriptor serializes")
        )
        .expect("descriptor decodes"),
        descriptor
    );
    assert_eq!(
        ProviderKindDescriptorRevisionV1::new(
            fixture_kind_id(),
            ProviderKindDescriptorRevisionId::new(),
            "fixture-descriptor",
            vec!["protocol-part-v1".to_owned(), "protocol-part-v1".to_owned()],
            ProviderEndpointPolicyDto::new(true, false, false),
            CredentialTransportContractDto::new(vec![CredentialTransportModeDto::Bearer], None)
                .expect("fixture contract is valid"),
            fixture_capability_subset(vec![ReasoningEffortLevelDto::Medium], true),
            "fixture-driver",
        )
        .expect_err("a repeated protocol part fails")
        .code(),
        "invalid_provider_kind_descriptor_revision"
    );
}

fn provider_entry() -> ProviderProfileEntryDto {
    ProviderProfileEntryDto::new(
        fixture_profile_id(),
        "Main",
        true,
        fixture_kind_id(),
        ProviderKindDescriptorRevisionId::new(),
        "fixture-model",
        Some("https://provider.example/v1".to_owned()),
        fixture_execution_policy(),
        fixture_capability_subset(vec![ReasoningEffortLevelDto::Medium], true),
        CredentialTransportDto::safe_header("x-api-key").expect("fixture transport is valid"),
        true,
        ProviderDriverCapabilitiesDto::new(true, true, true),
        ProviderProfileReadinessDto::Ready,
        Some(ProviderPricingPolicyDto::new(Some(3.0), Some(15.0)).expect("fixture pricing")),
    )
    .expect("fixture provider entry is valid")
}

#[test]
fn profile_entries_validate_their_display_and_endpoint() {
    let entry = provider_entry();
    assert_eq!(entry.display_name(), "Main");
    assert_eq!(entry.model_id(), "fixture-model");
    assert_eq!(
        entry.normalized_endpoint(),
        Some("https://provider.example/v1")
    );
    assert!(entry.credential_configured());
    assert_eq!(entry.readiness(), ProviderProfileReadinessDto::Ready);
    assert_eq!(
        entry.credential_transport().safe_header_name(),
        Some("x-api-key")
    );
    assert_eq!(
        serde_json::from_str::<ProviderProfileEntryDto>(
            &serde_json::to_string(&entry).expect("entry serializes")
        )
        .expect("entry decodes"),
        entry
    );
    assert_eq!(
        ProviderProfileEntryDto::new(
            fixture_profile_id(),
            " ",
            true,
            fixture_kind_id(),
            ProviderKindDescriptorRevisionId::new(),
            "fixture-model",
            None,
            fixture_execution_policy(),
            fixture_capability_subset(vec![ReasoningEffortLevelDto::Medium], true),
            CredentialTransportDto::bearer(),
            false,
            ProviderDriverCapabilitiesDto::new(true, true, true),
            ProviderProfileReadinessDto::Disabled,
            None,
        )
        .expect_err("a blank display name fails")
        .code(),
        "invalid_provider_profile_entry"
    );
    assert_eq!(
        ProviderProfileEntryDto::new(
            fixture_profile_id(),
            "Main",
            true,
            fixture_kind_id(),
            ProviderKindDescriptorRevisionId::new(),
            "fixture-model",
            Some("not-an-endpoint".to_owned()),
            fixture_execution_policy(),
            fixture_capability_subset(vec![ReasoningEffortLevelDto::Medium], true),
            CredentialTransportDto::bearer(),
            false,
            ProviderDriverCapabilitiesDto::new(true, true, true),
            ProviderProfileReadinessDto::Unavailable,
            None,
        )
        .expect_err("a malformed endpoint fails")
        .code(),
        "invalid_provider_profile_entry"
    );
}
