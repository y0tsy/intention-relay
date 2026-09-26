#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Slice 3 wire-contract fixtures use direct assertions and precise diagnostics."
)]

//! Slice 3 typed wire-contract tests.
//!
//! Owner: ADR 0044. These tests pin the current protocol and DTO schema
//! versions, the ledger wiring of the Slice 3 tag families, the frozen public
//! wire families and their ADR 0036 Appendix A field tables, the reworked
//! `0x0201` field-1 closed root origin, and the typed `run-execution-meaning-v4`
//! slots 7-10. Every fixture here is hermetic: no socket, port, thread, sleep,
//! or live daemon is used.

use intention_domain::canonical::{
    CanonicalError, CanonicalRecordBuilder, CanonicalRecordReader, Digest256, TagRegistry,
    TagStatus, WireType, encode_u64,
};
use intention_protocol::contract_families::{
    ActiveToolDescriptorSelectionV1, BRIDGE_INVOCATION_V1, BridgeAttachmentResponseDto,
    BridgeInvocationAcceptedDto, BridgeInvocationCommandDto, BridgeOperationV1, BridgeRunGrantDto,
    CONTINUAL_HARNESS_SELECTION_V1, GOAL_RUN_SELECTION_V1, MCP_METHOD_CATALOG_SELECTION_V1,
    MODEL_TOOL_LOOP_V1, ModelToolExchangeDto, ModelToolLoopV1,
    PROGRAMMATIC_CALLER_POLICY_SELECTION_V1, PUBLIC_WIRE_CONTRACT_FAMILIES,
    TOOL_DESCRIPTOR_REVISION, TOOL_REGISTRY_REVISION, ToolDescriptorRevision, ToolRegistryRevision,
    ToolTerminalOutcome,
};
use intention_protocol::slice3::{
    ContinualHarnessSelectionV1, DisabledOr, FixedRunLimits, GOAL_FAILURE_CODES,
    GoalCardReferenceV1, GoalDto, GoalEvidenceKindV1, GoalGateDefinitionV1,
    GoalGateRevisionReferenceV1, GoalLifecycleStateDto, GoalReadinessStateDto,
    GoalRevisionReferenceV1, GoalRunKindV1, GoalRunSelectionBoundsV1, GoalRunSelectionV1,
    GoalScopeDto, GoalScopeLinkProvenanceV1, GoalUserDecisionStateDto,
    HARNESS_CLOSED_SAFE_FAILURES, HarnessClassResolutionV1, HarnessExecutionClassV1,
    HarnessIntervalScheduleV1, HarnessPresentationModeV1, HarnessRuleLifecycleStateV1,
    HarnessRuleOperationV1, HarnessRuleRevisionV1, HarnessRuleScopeV1, HarnessRuleSourceV1,
    HarnessRuleV1, HarnessSelectionBoundsV1, HarnessSourceKindV1, HarnessTaskModeV1,
    HarnessTriggerReasonV1, HarnessTriggerRecordV1, MAX_SLICE3_TEXT_CHARS,
    McpMethodCatalogSelectionV1, PROGRAMMATIC_POLICY_FAILURE_CODES, ProgrammaticAdmissionRuleV1,
    ProgrammaticCalendarPeriodKindV1, ProgrammaticCallerPolicyLifecycleStateV1,
    ProgrammaticCallerPolicyScopeV1, ProgrammaticCallerPolicySelectionV1,
    ProgrammaticCallerPolicyV1, ProgrammaticCallerRootOriginV1, ProgrammaticConfirmationBindingV1,
    ProgrammaticExactConfirmationV1, ProgrammaticMcpMethodReferenceV1, ProgrammaticRunLimitsV1,
    RunExecutionMeaningV4Record, UserLifecycleOperationV1, VerificationAuditVerdictDto,
    VerificationGateDto, VerificationMandateAuthorityDto, VerifierContractReferenceV1,
    VerifierOperationV1, VerifierTargetSetReferenceV1, apply_harness_rule_operation,
    validate_harness_rule_operation, validate_harness_time_zone,
};
use intention_protocol::{
    CURRENT_DTO_SCHEMA_VERSION, CURRENT_PROTOCOL_VERSION, ProtocolVersionDto,
};
use intention_types::SchemaVersionDto;

const FAKE_SECRET: &str = "sk-live-slice3-fake-secret";
const FAKE_PATH: &str = "C:\\Users\\relay\\.config\\plan.md";

fn hex16(spec: &str) -> [u8; 16] {
    let digits: String = spec.chars().filter(|character| *character != '-').collect();
    let mut bytes = [0u8; 16];
    for (index, chunk) in digits.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).expect("fixture uuid digits are ascii");
        bytes[index] = u8::from_str_radix(pair, 16).expect("fixture uuid digits are hex");
    }
    bytes
}

fn digest(seed: &str) -> Digest256 {
    Digest256::sha256(seed.as_bytes())
}

/// Asserts that every listed JSON key appears in the declared field order.
fn assert_field_order(wire: &str, keys: &[&str]) {
    let mut last = 0;
    for key in keys {
        let needle = format!("\"{key}\":");
        let position = wire
            .find(&needle)
            .unwrap_or_else(|| panic!("field {key} is missing from {wire}"));
        assert!(
            position >= last,
            "field {key} is out of its declared order in {wire}"
        );
        last = position;
    }
}

fn decode_error<T: serde::de::DeserializeOwned + std::fmt::Debug>(wire: &str) -> String {
    serde_json::from_str::<T>(wire)
        .expect_err("an invalid Slice 3 frame must not decode")
        .to_string()
}

fn tool_descriptor(tool_id: &str) -> ToolDescriptorRevision {
    ToolDescriptorRevision {
        tool_id: tool_id.to_owned(),
        descriptor_revision: "descriptor-rev-1".to_owned(),
        intended_owner: "workspace".to_owned(),
        input_schema_reference: "input-schema-1".to_owned(),
        result_schema_reference: "result-schema-1".to_owned(),
        required_capability_binding: "registry-read".to_owned(),
        mode_relation: "build".to_owned(),
        model_function_schema_revision: "function-schema-1".to_owned(),
        safe_result_projection_revision: "projection-1".to_owned(),
        observation_contract_revision: "observation-1".to_owned(),
        stream_shape: "json".to_owned(),
    }
}

fn active_descriptor(tool_id: &str) -> ActiveToolDescriptorSelectionV1 {
    ActiveToolDescriptorSelectionV1 {
        tool_id: tool_id.to_owned(),
        intended_owner: "workspace".to_owned(),
        descriptor_revision: "descriptor-rev-1".to_owned(),
        input_schema_reference: "input-schema-1".to_owned(),
        result_schema_reference: "result-schema-1".to_owned(),
        required_capability_binding: "registry-read".to_owned(),
        mode_relation: "build".to_owned(),
        model_function_schema_revision: "function-schema-1".to_owned(),
        safe_result_projection_revision: "projection-1".to_owned(),
        observation_contract_revision: "observation-1".to_owned(),
        stream_shape: "json".to_owned(),
    }
}

fn registry(descriptors: Vec<ToolDescriptorRevision>) -> ToolRegistryRevision {
    ToolRegistryRevision {
        registry_revision_id: "registry-rev-1".to_owned(),
        descriptors,
        admission_engine_revision: "admission-1".to_owned(),
        hook_pipeline_revision: "hooks-1".to_owned(),
    }
}

fn tool_loop(descriptors: Vec<ActiveToolDescriptorSelectionV1>) -> ModelToolLoopV1 {
    ModelToolLoopV1 {
        tool_registry_revision_id: "registry-rev-1".to_owned(),
        admission_engine_revision: "admission-1".to_owned(),
        hook_pipeline_revision: "hooks-1".to_owned(),
        active_descriptors: descriptors,
        model_tool_loop_required: true,
        translation_revision: "translation-1".to_owned(),
        stream_shape: "json".to_owned(),
    }
}

fn bridge_grant() -> BridgeRunGrantDto {
    BridgeRunGrantDto {
        opaque_grant_identity: "grant-1".to_owned(),
        issued_protocol_revision: "1.1".to_owned(),
    }
}

fn bridge_operation() -> BridgeOperationV1 {
    BridgeOperationV1 {
        bridge_operation_id: "operation-1".to_owned(),
        run_id: "run-1".to_owned(),
        mandate_id: "mandate-1".to_owned(),
        mandate_revision: "2".to_owned(),
        model_step_id: "step-1".to_owned(),
        tool_id: "read".to_owned(),
        descriptor_revision: "descriptor-rev-1".to_owned(),
        typed_input_digest: "0".repeat(64),
        tool_call_id: "call-1".to_owned(),
        admission_outcome: "admitted".to_owned(),
        attempt_reference: Some("attempt-1".to_owned()),
    }
}

#[test]
fn slice3_wire_pins_the_current_protocol_and_dto_schema_versions() {
    assert_eq!(CURRENT_PROTOCOL_VERSION, ProtocolVersionDto::new(1, 1));
    assert_eq!(CURRENT_DTO_SCHEMA_VERSION, SchemaVersionDto::new(1, 1));
    assert_eq!(CURRENT_PROTOCOL_VERSION.major(), 1);
    assert_eq!(CURRENT_PROTOCOL_VERSION.minor(), 1);
    assert_eq!(CURRENT_DTO_SCHEMA_VERSION.major(), 1);
    assert_eq!(CURRENT_DTO_SCHEMA_VERSION.minor(), 1);
    // Slice 3 adds no negotiated capability, no envelope, and no schema bump;
    // the hello fixture of the current version keeps decoding unchanged.
    let hello: intention_protocol::ProtocolHelloDto = serde_json::from_str(include_str!(
        "fixtures/goldens/hello-current-version-v1.json"
    ))
    .expect("the current-version hello golden decodes");
    assert_eq!(hello.version(), CURRENT_PROTOCOL_VERSION);
    // The four frozen Slice 3 wire families are version one records.
    for descriptor in [
        TOOL_DESCRIPTOR_REVISION,
        TOOL_REGISTRY_REVISION,
        MODEL_TOOL_LOOP_V1,
        BRIDGE_INVOCATION_V1,
    ] {
        assert_eq!(
            descriptor.version, 1,
            "{} is a version-one family",
            descriptor.name
        );
    }
}

#[test]
fn ledger_wiring_covers_every_wired_slice3_tag_exactly_once_by_name() {
    assert_eq!(TagRegistry::LEDGER.len(), 24, "the ledger holds 24 tags");
    assert_eq!(
        TagRegistry::LEDGER
            .iter()
            .filter(|entry| entry.status == TagStatus::Wired)
            .count(),
        16,
        "16 tags are wired after Slice 3"
    );
    assert_eq!(
        TagRegistry::LEDGER
            .iter()
            .filter(|entry| entry.status == TagStatus::ReservedForSlice4)
            .count(),
        8,
        "8 tags stay reserved for Slice 4"
    );
    // The `TagStatus` enum has exactly two variants today, so the exhaustive
    // match below is also the "no ReservedForSlice3 remains" assertion: the
    // former Slice 3 reservation variant no longer exists.
    for entry in TagRegistry::LEDGER {
        match entry.status {
            TagStatus::Wired | TagStatus::ReservedForSlice4 => {}
        }
    }
    // Every wire-ledger tag is covered by its family descriptor; only the
    // version-aliased fork snapshot and preview families appear twice (one
    // descriptor per version sharing the single ledger tag).
    let mut covered_tags: Vec<u32> = PUBLIC_WIRE_CONTRACT_FAMILIES
        .iter()
        .map(|descriptor| descriptor.tag)
        .collect();
    covered_tags.sort_unstable();
    covered_tags.dedup();
    assert_eq!(
        covered_tags.len(),
        TagRegistry::LEDGER.len() - 1,
        "every wire-ledger tag except run-execution-meaning has a public family"
    );
    for descriptor in PUBLIC_WIRE_CONTRACT_FAMILIES {
        let tag = descriptor.tag;
        let name = descriptor.name;
        let ledger = TagRegistry::LEDGER
            .iter()
            .find(|entry| {
                entry.value == tag
                    && (entry.name == name
                        || (entry.name.contains('/')
                            && entry
                                .name
                                .contains(name.trim_end_matches("-v1").trim_end_matches("-v2"))))
            })
            .unwrap_or_else(|| panic!("{name} is not a ledger tag"));
        assert!(
            descriptor.version == 1 || descriptor.version == 2,
            "{} carries a supported family version",
            descriptor.name
        );
        if ledger.name.contains('/') {
            assert_eq!(
                PUBLIC_WIRE_CONTRACT_FAMILIES
                    .iter()
                    .filter(|candidate| candidate.tag == descriptor.tag)
                    .count(),
                2,
                "a version-aliased family carries exactly one descriptor per version"
            );
        }
    }
    // The four frozen public families use the domain-owned constants directly;
    // there is no protocol-side numeric mirror.
    assert_eq!(
        TOOL_DESCRIPTOR_REVISION.tag,
        TagRegistry::TOOL_DESCRIPTOR_REVISION
    );
    assert_eq!(
        TOOL_REGISTRY_REVISION.tag,
        TagRegistry::TOOL_REGISTRY_REVISION
    );
    assert_eq!(MODEL_TOOL_LOOP_V1.tag, TagRegistry::MODEL_TOOL_LOOP_V1);
    assert_eq!(BRIDGE_INVOCATION_V1.tag, TagRegistry::BRIDGE_INVOCATION_V1);
    // The Slice 3 canonical selections are wired and named in the ledger.
    assert_eq!(
        PROGRAMMATIC_CALLER_POLICY_SELECTION_V1.tag,
        TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1
    );
    assert_eq!(
        GOAL_RUN_SELECTION_V1.tag,
        TagRegistry::GOAL_RUN_SELECTION_V1
    );
    assert_eq!(
        CONTINUAL_HARNESS_SELECTION_V1.tag,
        TagRegistry::CONTINUAL_HARNESS_SELECTION_V1
    );
    assert_eq!(
        MCP_METHOD_CATALOG_SELECTION_V1.tag,
        TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1
    );
}

#[test]
fn tool_descriptor_revision_matches_the_frozen_field_table() {
    let descriptor = tool_descriptor("read");
    let wire = serde_json::to_string(&descriptor).expect("descriptor encodes");
    assert_field_order(
        &wire,
        &[
            "tool_id",
            "descriptor_revision",
            "intended_owner",
            "input_schema_reference",
            "result_schema_reference",
            "required_capability_binding",
            "mode_relation",
            "model_function_schema_revision",
            "safe_result_projection_revision",
            "observation_contract_revision",
            "stream_shape",
        ],
    );
    let decoded: ToolDescriptorRevision = serde_json::from_str(&wire).expect("descriptor decodes");
    assert_eq!(decoded, descriptor);
    descriptor
        .validate()
        .expect("the fixture descriptor is valid");

    let blank = ToolDescriptorRevision {
        tool_id: "   ".to_owned(),
        ..descriptor.clone()
    };
    assert_eq!(
        blank
            .validate()
            .expect_err("a blank tool id is rejected")
            .code(),
        "tool_descriptor_revision_invalid"
    );
    let over_long = ToolDescriptorRevision {
        tool_id: "a".repeat(MAX_SLICE3_TEXT_CHARS + 1),
        ..descriptor.clone()
    };
    assert_eq!(
        over_long
            .validate()
            .expect_err("an over-long tool id is rejected")
            .code(),
        "tool_descriptor_revision_invalid"
    );
    let credential_shaped = ToolDescriptorRevision {
        intended_owner: FAKE_SECRET.to_owned(),
        ..descriptor
    };
    assert_eq!(
        credential_shaped
            .validate()
            .expect_err("a credential-shaped descriptor field is rejected")
            .code(),
        "credentials_forbidden"
    );
}

#[test]
fn tool_registry_revision_matches_the_frozen_field_table_and_bounds() {
    let registry_value = registry(vec![tool_descriptor("read"), tool_descriptor("glob")]);
    let wire = serde_json::to_string(&registry_value).expect("registry encodes");
    assert_field_order(
        &wire,
        &[
            "registry_revision_id",
            "descriptors",
            "admission_engine_revision",
            "hook_pipeline_revision",
        ],
    );
    let decoded: ToolRegistryRevision = serde_json::from_str(&wire).expect("registry decodes");
    assert_eq!(decoded, registry_value);
    registry_value
        .validate()
        .expect("the fixture registry is valid");

    let duplicate = registry(vec![tool_descriptor("read"), tool_descriptor("read")]);
    assert_eq!(
        duplicate
            .validate()
            .expect_err("a duplicate tool descriptor is rejected")
            .code(),
        "duplicate_tool_descriptor"
    );

    let over_count = registry(
        (0..257)
            .map(|index| tool_descriptor(&format!("tool-{index}")))
            .collect(),
    );
    assert_eq!(
        over_count
            .validate()
            .expect_err("a 257-descriptor registry is rejected")
            .code(),
        "tool_registry_revision_too_large"
    );

    let wide_field = "f".repeat(200);
    let over_bytes = registry(
        (0..256)
            .map(|index| ToolDescriptorRevision {
                tool_id: format!("{wide_field}{index}"),
                descriptor_revision: wide_field.clone(),
                intended_owner: wide_field.clone(),
                input_schema_reference: wide_field.clone(),
                result_schema_reference: wide_field.clone(),
                required_capability_binding: wide_field.clone(),
                mode_relation: wide_field.clone(),
                model_function_schema_revision: wide_field.clone(),
                safe_result_projection_revision: wide_field.clone(),
                observation_contract_revision: wide_field.clone(),
                stream_shape: wide_field.clone(),
            })
            .collect(),
    );
    assert_eq!(
        over_bytes
            .validate()
            .expect_err("a registry over the 512 KiB aggregate is rejected")
            .code(),
        "tool_registry_revision_too_large"
    );

    let credential_shaped = ToolRegistryRevision {
        registry_revision_id: FAKE_SECRET.to_owned(),
        ..registry_value
    };
    assert_eq!(
        credential_shaped
            .validate()
            .expect_err("a credential-shaped registry identity is rejected")
            .code(),
        "credentials_forbidden"
    );
}

#[test]
fn model_tool_loop_and_exchange_pin_the_closed_sixteen_call_group() {
    let loop_value = tool_loop(vec![active_descriptor("read")]);
    let decoded: ModelToolLoopV1 =
        serde_json::from_str(&serde_json::to_string(&loop_value).expect("loop encodes"))
            .expect("loop decodes");
    assert_eq!(decoded, loop_value);
    loop_value.validate().expect("the fixture loop is valid");
    // The top-level field table is checked on a loop without nested
    // descriptors, so the nested `stream_shape` key cannot mask the order.
    assert_field_order(
        &serde_json::to_string(&tool_loop(Vec::new())).expect("loop encodes"),
        &[
            "tool_registry_revision_id",
            "admission_engine_revision",
            "hook_pipeline_revision",
            "active_descriptors",
            "model_tool_loop_required",
            "translation_revision",
            "stream_shape",
        ],
    );

    let sixteen = tool_loop(
        (0..16)
            .map(|index| active_descriptor(&format!("tool-{index}")))
            .collect(),
    );
    sixteen
        .validate()
        .expect("sixteen active descriptors are valid");
    let seventeen = tool_loop(
        (0..17)
            .map(|index| active_descriptor(&format!("tool-{index}")))
            .collect(),
    );
    assert_eq!(
        seventeen
            .validate()
            .expect_err("seventeen active descriptors are rejected")
            .code(),
        "model_tool_loop_invalid"
    );

    let exchange = ModelToolExchangeDto {
        assistant_ordered_calls: (0..16).map(|index| format!("call-{index}")).collect(),
        canonical_call_identities: vec!["identity-1".to_owned()],
        completed_result_records: Vec::new(),
        safe_model_visible_projections: Vec::new(),
    };
    exchange.validate().expect("sixteen calls are valid");
    let over_group = ModelToolExchangeDto {
        assistant_ordered_calls: (0..17).map(|index| format!("call-{index}")).collect(),
        ..exchange
    };
    assert_eq!(
        over_group
            .validate()
            .expect_err("seventeen ordered calls are rejected")
            .code(),
        "provider_tool_group_invalid"
    );

    // The terminal outcome set is closed to exactly eight wire values.
    let wire_values = [
        "succeeded",
        "denied_before_execution",
        "failed_before_external_effect",
        "cancelled_before_start",
        "interrupted_before_start",
        "output_limit_exceeded",
        "execution_unavailable",
        "external_effect_unknown",
    ];
    for value in wire_values {
        let encoded = format!("\"{value}\"");
        let outcome: ToolTerminalOutcome =
            serde_json::from_str(&encoded).expect("a closed terminal outcome decodes");
        assert_eq!(
            serde_json::to_string(&outcome).expect("outcome encodes"),
            encoded
        );
    }
    assert!(
        serde_json::from_str::<ToolTerminalOutcome>("\"unknown_outcome\"").is_err(),
        "an unknown terminal outcome must fail closed"
    );
}

#[test]
fn bridge_invocation_family_matches_the_frozen_field_table() {
    let operation = bridge_operation();
    let wire = serde_json::to_string(&operation).expect("bridge operation encodes");
    assert_field_order(
        &wire,
        &[
            "bridge_operation_id",
            "run_id",
            "mandate_id",
            "mandate_revision",
            "model_step_id",
            "tool_id",
            "descriptor_revision",
            "typed_input_digest",
            "tool_call_id",
            "admission_outcome",
            "attempt_reference",
        ],
    );
    let decoded: BridgeOperationV1 = serde_json::from_str(&wire).expect("bridge operation decodes");
    assert_eq!(decoded, operation);
    operation
        .validate()
        .expect("the fixture operation is valid");

    let uppercase_digest = BridgeOperationV1 {
        typed_input_digest: "A".repeat(64),
        ..operation.clone()
    };
    assert_eq!(
        uppercase_digest
            .validate()
            .expect_err("an uppercase digest is rejected")
            .code(),
        "invalid_digest"
    );
    let short_digest = BridgeOperationV1 {
        typed_input_digest: "0".repeat(63),
        ..operation.clone()
    };
    assert_eq!(
        short_digest
            .validate()
            .expect_err("a short digest is rejected")
            .code(),
        "invalid_digest"
    );
    let blank_field = BridgeOperationV1 {
        model_step_id: "   ".to_owned(),
        ..operation.clone()
    };
    assert_eq!(
        blank_field
            .validate()
            .expect_err("a blank bridge field is rejected")
            .code(),
        "bridge_invocation_invalid"
    );
    let credential_shaped = BridgeOperationV1 {
        admission_outcome: FAKE_SECRET.to_owned(),
        ..operation
    };
    assert_eq!(
        credential_shaped
            .validate()
            .expect_err("a credential-shaped bridge field is rejected")
            .code(),
        "credentials_forbidden"
    );

    let command = BridgeInvocationCommandDto {
        bridge_run_grant: bridge_grant(),
        bridge_operation_id: "operation-1".to_owned(),
        typed_tool_invocation: "typed-invocation".to_owned(),
    };
    command.validate().expect("the fixture command is valid");
    assert_eq!(
        serde_json::from_str::<BridgeInvocationCommandDto>(
            &serde_json::to_string(&command).expect("command encodes")
        )
        .expect("command decodes"),
        command
    );
    let accepted = BridgeInvocationAcceptedDto {
        bridge_operation_id: "operation-1".to_owned(),
        tool_call_id: "call-1".to_owned(),
        admission_state: "admitted".to_owned(),
    };
    accepted
        .validate()
        .expect("the fixture acceptance is valid");
    assert_eq!(
        serde_json::from_str::<BridgeInvocationAcceptedDto>(
            &serde_json::to_string(&accepted).expect("acceptance encodes")
        )
        .expect("acceptance decodes"),
        accepted
    );
    let attachment = BridgeAttachmentResponseDto {
        bridge_run_grant: bridge_grant(),
        negotiated_capabilities: vec![
            intention_protocol::ProtocolCapabilityDto::CorrelatedRequests,
        ],
        initial_run_cursor: 0,
    };
    assert_eq!(
        serde_json::from_str::<BridgeAttachmentResponseDto>(
            &serde_json::to_string(&attachment).expect("attachment encodes")
        )
        .expect("attachment decodes"),
        attachment
    );
}

#[test]
fn frozen_families_reject_unknown_variants_wrong_types_and_trailing_bytes() {
    let registry = registry(vec![tool_descriptor("read")]);
    let registry_wire = serde_json::to_string(&registry).expect("registry encodes");
    assert!(
        decode_error::<ToolRegistryRevision>(&format!("{registry_wire}{registry_wire}"))
            .contains("trailing characters"),
        "trailing bytes must fail closed"
    );
    assert!(
        !decode_error::<ToolRegistryRevision>(&format!("{registry_wire} 0")).is_empty(),
        "a trailing scalar must fail closed"
    );
    let wrong_type = registry_wire.replace("\"descriptors\":[", "\"descriptors\":\"[");
    assert!(
        !decode_error::<ToolRegistryRevision>(&wrong_type).is_empty(),
        "a wrong-typed known field must fail closed"
    );
    // Unknown additive object fields stay decodable: the frozen field table is
    // closed for variants, wire types, and versions, while field-level
    // additions follow the established forward-compatibility policy.
    let mut additive: serde_json::Value =
        serde_json::from_str(&registry_wire).expect("registry parses as a JSON value");
    additive
        .as_object_mut()
        .expect("the registry frame is an object")
        .insert("future_additive".to_owned(), serde_json::Value::Bool(true));
    let decoded: ToolRegistryRevision =
        serde_json::from_str(&additive.to_string()).expect("an additive field stays decodable");
    assert_eq!(decoded, registry);

    let operation_wire = serde_json::to_string(&bridge_operation()).expect("operation encodes");
    assert!(
        !decode_error::<BridgeOperationV1>(&format!("{operation_wire}null")).is_empty(),
        "bridge trailing bytes must fail closed"
    );
    let unknown_variant = operation_wire.replace("\"admitted\"", "\"unknown_state\"");
    // `admission_outcome` is bounded text rather than a closed enum, so the
    // closed-variant assertion uses the genuinely closed terminal outcome set.
    assert!(
        serde_json::from_str::<BridgeOperationV1>(&unknown_variant).is_ok(),
        "bounded text stays open by design"
    );
    assert!(
        serde_json::from_str::<ToolTerminalOutcome>("\"not_a_terminal_outcome\"").is_err(),
        "an unknown closed variant must fail closed"
    );
}

#[test]
fn reworked_policy_selection_field_one_is_the_closed_typed_root_origin() {
    let interactive = ProgrammaticCallerRootOriginV1::InteractiveUser {
        originating_turn_id: hex16("11111111-1111-4111-8111-111111111111"),
    };
    let harness = ProgrammaticCallerRootOriginV1::ContinualHarness {
        harness_id: hex16("22222222-2222-4222-8222-222222222222"),
        rule_revision: 3,
        trigger_reason_id: hex16("33333333-3333-4333-8333-333333333333"),
    };
    for origin in [interactive, harness] {
        let encoded = origin.encode().expect("root origin encodes");
        let reader = CanonicalRecordReader::new(&encoded, 3).expect("root origin frames");
        assert_eq!(reader.tag, 0, "the nested root origin is anonymous");
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&encoded).expect("root origin decodes"),
            origin
        );
    }
    assert_eq!(
        CanonicalRecordReader::new(
            &ProgrammaticCallerRootOriginV1::InteractiveUser {
                originating_turn_id: hex16("11111111-1111-4111-8111-111111111111"),
            }
            .encode()
            .expect("interactive origin encodes"),
            3
        )
        .expect("interactive origin frames")
        .version,
        1,
        "InteractiveUser is nested variant version one"
    );
    assert_eq!(
        CanonicalRecordReader::new(
            &ProgrammaticCallerRootOriginV1::ContinualHarness {
                harness_id: hex16("22222222-2222-4222-8222-222222222222"),
                rule_revision: 3,
                trigger_reason_id: hex16("33333333-3333-4333-8333-333333333333"),
            }
            .encode()
            .expect("harness origin encodes"),
            3
        )
        .expect("harness origin frames")
        .version,
        2,
        "ContinualHarness is nested variant version two"
    );

    // An unknown nested variant version fails closed.
    let unknown_version = CanonicalRecordBuilder::new(0, 3)
        .field(1, WireType::U64, encode_u64(0))
        .expect("unknown-version record builds")
        .finish()
        .expect("unknown-version record frames");
    assert_eq!(
        ProgrammaticCallerRootOriginV1::decode(&unknown_version),
        Err(CanonicalError::InvalidTag)
    );

    // The removed Slice 1 ExecutionKind U64 shape of field 1 fails closed.
    let removed_shape =
        CanonicalRecordBuilder::new(TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1, 1)
            .field(1, WireType::U64, encode_u64(0))
            .expect("removed-shape field 1 builds")
            .field(2, WireType::Uuid, vec![7; 16])
            .expect("removed-shape field 2 builds")
            .field(3, WireType::Digest, vec![8; 32])
            .expect("removed-shape field 3 builds")
            .field(4, WireType::List, vec![0, 0, 0, 0])
            .expect("removed-shape field 4 builds")
            .field(
                5,
                WireType::Record,
                fixed_run_limits().encode().expect("limits encode"),
            )
            .expect("removed-shape field 5 builds")
            .finish()
            .expect("removed-shape record frames");
    assert_eq!(
        ProgrammaticCallerPolicySelectionV1::decode(&removed_shape),
        Err(CanonicalError::InvalidField)
    );

    // A valid typed selection round-trips through its canonical bytes.
    let selection = policy_selection(ProgrammaticCallerRootOriginV1::ContinualHarness {
        harness_id: hex16("22222222-2222-4222-8222-222222222222"),
        rule_revision: 3,
        trigger_reason_id: hex16("33333333-3333-4333-8333-333333333333"),
    });
    let encoded = selection.encode().expect("policy selection encodes");
    assert_eq!(
        ProgrammaticCallerPolicySelectionV1::decode(&encoded).expect("selection decodes"),
        selection
    );
    let wrong_tag = CanonicalRecordBuilder::new(TagRegistry::AGENT_ACTIVITY_SELECTION_V1, 1)
        .finish()
        .expect("wrong-tag record frames");
    assert_eq!(
        ProgrammaticCallerPolicySelectionV1::decode(&wrong_tag),
        Err(CanonicalError::InvalidTag)
    );
}

#[test]
fn v4_slots_seven_through_ten_are_typed_closed_markers() {
    let selected = v4_record(true);
    let encoded = selected.encode().expect("selected v4 record encodes");
    let reader = CanonicalRecordReader::new(&encoded, 11).expect("v4 record frames");
    assert_eq!(reader.tag, TagRegistry::RUN_EXECUTION_MEANING);
    assert_eq!(reader.version, 4);
    let decoded = RunExecutionMeaningV4Record::decode(&encoded).expect("v4 record decodes");
    assert_eq!(decoded, selected);
    assert!(matches!(decoded.harness_selection, DisabledOr::Selected(_)));
    assert!(matches!(decoded.goal_selection, DisabledOr::Selected(_)));
    assert!(matches!(
        decoded.mcp_method_catalog_selection,
        DisabledOr::Selected(_)
    ));
    assert!(matches!(
        decoded.programmatic_caller_policy_selection,
        DisabledOr::Selected(_)
    ));

    let disabled = v4_record(false);
    let disabled_bytes = disabled.encode().expect("disabled v4 record encodes");
    let decoded_disabled =
        RunExecutionMeaningV4Record::decode(&disabled_bytes).expect("disabled v4 record decodes");
    assert_eq!(decoded_disabled, disabled);
    assert_eq!(decoded_disabled.harness_selection, DisabledOr::Disabled);
    assert_eq!(decoded_disabled.goal_selection, DisabledOr::Disabled);
    assert_eq!(
        decoded_disabled.mcp_method_catalog_selection,
        DisabledOr::Disabled
    );
    assert_eq!(
        decoded_disabled.programmatic_caller_policy_selection,
        DisabledOr::Disabled
    );
    assert_eq!(
        decoded_disabled.fields, disabled.fields,
        "slots 1-6 keep their recorded bytes in order"
    );
    assert_eq!(
        decoded_disabled.agent_activity_selection, disabled.agent_activity_selection,
        "slot 11 keeps its recorded bytes"
    );

    // The one-byte marker is closed: any other marker or a trailing value after
    // the Disabled marker fails closed, per typed slot.
    assert!(DisabledOr::<ContinualHarnessSelectionV1>::decode(&[2]).is_err());
    assert!(DisabledOr::<ContinualHarnessSelectionV1>::decode(&[0, 0]).is_err());
    assert!(DisabledOr::<GoalRunSelectionV1>::decode(&[2]).is_err());
    assert!(DisabledOr::<GoalRunSelectionV1>::decode(&[0, 0]).is_err());
    assert!(DisabledOr::<McpMethodCatalogSelectionV1>::decode(&[2]).is_err());
    assert!(DisabledOr::<McpMethodCatalogSelectionV1>::decode(&[0, 0]).is_err());
    assert!(DisabledOr::<ProgrammaticCallerPolicySelectionV1>::decode(&[2]).is_err());
    assert!(DisabledOr::<ProgrammaticCallerPolicySelectionV1>::decode(&[0, 0]).is_err());
    assert!(DisabledOr::<ProgrammaticCallerPolicySelectionV1>::decode(&[1]).is_err());
    assert!(
        DisabledOr::<ProgrammaticCallerPolicySelectionV1>::decode(&[]).is_err(),
        "an absent presence marker fails closed"
    );

    // A malformed marker inside one v4 slot fails the whole record decode.
    let malformed = v4_record_with_slot(MarkerSlot::Harness, vec![2]);
    assert_eq!(
        RunExecutionMeaningV4Record::decode(&malformed)
            .expect_err("an unknown harness presence marker fails the record decode"),
        CanonicalError::InvalidOptional
    );
    let trailing_disabled = v4_record_with_slot(MarkerSlot::Goal, vec![0, 0]);
    assert_eq!(
        RunExecutionMeaningV4Record::decode(&trailing_disabled)
            .expect_err("a trailing disabled byte fails the record decode"),
        CanonicalError::InvalidOptional
    );
    let malformed_policy = v4_record_with_slot(MarkerSlot::Policy, vec![1, 0xff]);
    assert!(RunExecutionMeaningV4Record::decode(&malformed_policy).is_err());
}

#[test]
fn slice3_closed_safe_failure_families_are_complete_and_unique() {
    assert_eq!(HARNESS_CLOSED_SAFE_FAILURES.len(), 15);
    assert_eq!(PROGRAMMATIC_POLICY_FAILURE_CODES.len(), 22);
    assert!(GOAL_FAILURE_CODES.len() >= 15);
    let mut all: Vec<&str> = Vec::new();
    for &code in HARNESS_CLOSED_SAFE_FAILURES
        .iter()
        .chain(PROGRAMMATIC_POLICY_FAILURE_CODES.iter())
        .chain(GOAL_FAILURE_CODES.iter())
    {
        assert!(!code.is_empty(), "a closed safe failure has a code");
        assert!(
            code.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
            "{code} is a snake_case machine code"
        );
        assert!(!all.contains(&code), "{code} is listed once");
        assert!(
            code != "credentials_forbidden",
            "credential rejection is the shared codec code, not a family code"
        );
        all.push(code);
    }
    assert_eq!(
        HARNESS_CLOSED_SAFE_FAILURES
            .iter()
            .filter(|code| code.starts_with("harness_"))
            .count(),
        HARNESS_CLOSED_SAFE_FAILURES.len(),
        "every harness failure code is harness-namespaced"
    );
    assert_eq!(
        PROGRAMMATIC_POLICY_FAILURE_CODES
            .iter()
            .filter(|code| code.starts_with("programmatic_policy_"))
            .count(),
        PROGRAMMATIC_POLICY_FAILURE_CODES.len(),
        "every policy failure code is policy-namespaced"
    );

    // The delegated verifier operation set is exactly five and excludes the
    // user-lifecycle Pause and Resume operations.
    assert_eq!(VerifierOperationV1::ALL.len(), 5);
    assert!(VerifierOperationV1::from_discriminant(5).is_none());
    let operation_codes: Vec<&str> = VerifierOperationV1::ALL
        .iter()
        .map(|operation| operation.code())
        .collect();
    for user_operation in [
        UserLifecycleOperationV1::Pause,
        UserLifecycleOperationV1::Resume,
    ] {
        assert!(
            !operation_codes.contains(&user_operation.code()),
            "{} is a user-lifecycle operation, never verifier authority",
            user_operation.code()
        );
    }
    assert_eq!(VerificationAuditVerdictDto::ALL.len(), 7);
    assert!(VerificationAuditVerdictDto::from_discriminant(7).is_none());
}

#[test]
fn slice3_projection_surface_is_reachable_through_the_protocol_crate() {
    // Harness rules and triggers.
    let harness_id = hex16("44444444-4444-4444-8444-444444444444");
    let revision = HarnessRuleRevisionV1::new(
        harness_id,
        1,
        digest("slice3-harness-task"),
        HarnessExecutionClassV1::Medium,
        HarnessTaskModeV1::GoalDirected,
        vec![
            HarnessRuleSourceV1::ExplicitUserLaunch,
            HarnessRuleSourceV1::FixedInterval(
                HarnessIntervalScheduleV1::new(0, 60_000).expect("interval schedule is valid"),
            ),
        ],
        HarnessPresentationModeV1::JournalAndActivityEntry,
        "Europe/Berlin".to_owned(),
    )
    .expect("the fixture rule revision is valid");
    let rule = HarnessRuleV1::new(
        harness_id,
        HarnessRuleScopeV1::Project {
            project_id: hex16("55555555-5555-4555-8555-555555555555"),
        },
        revision,
        hex16("66666666-6666-4666-8666-666666666666"),
    )
    .expect("the fixture rule is valid");
    assert_eq!(rule.lifecycle_state(), HarnessRuleLifecycleStateV1::Active);
    let paused = apply_harness_rule_operation(&rule, false, HarnessRuleOperationV1::Pause)
        .expect("an active rule pauses");
    assert!(!paused.lifecycle_state().is_active());
    assert!(!paused.lifecycle_state().permits_automatic_launch());
    assert!(
        validate_harness_rule_operation(
            paused.lifecycle_state(),
            false,
            HarnessRuleOperationV1::Resume
        )
        .is_ok()
    );
    let trigger = HarnessTriggerRecordV1::new(
        HarnessTriggerReasonV1 {
            reason_id: hex16("77777777-7777-4777-8777-777777777777"),
            source_kind: HarnessSourceKindV1::FixedInterval,
            first_observed_at_ms: 1_700_000_000_000,
            last_observed_at_ms: 1_700_000_060_000,
            coalesced_count: 2,
        },
        1,
        "Europe/Berlin".to_owned(),
        Some(hex16("88888888-8888-4888-8888-888888888888")),
    )
    .expect("the fixture trigger record is valid");
    let reason_bytes = trigger.reason().encode().expect("trigger reason encodes");
    assert_eq!(
        HarnessTriggerReasonV1::decode(&reason_bytes).expect("trigger reason decodes"),
        *trigger.reason()
    );

    // Policy admission decisions and confirmations.
    assert_eq!(
        ProgrammaticAdmissionRuleV1::Prohibited
            .most_restrictive(ProgrammaticAdmissionRuleV1::DirectLocalRead),
        ProgrammaticAdmissionRuleV1::Prohibited
    );
    let policy = ProgrammaticCallerPolicyV1::new(
        hex16("99999999-9999-4999-8999-999999999999"),
        ProgrammaticCallerPolicyScopeV1::Project {
            project_id: hex16("55555555-5555-4555-8555-555555555555"),
        },
        ProgrammaticCalendarPeriodKindV1::Day,
        1,
    )
    .expect("the fixture policy is valid");
    assert_eq!(
        policy.lifecycle_state,
        ProgrammaticCallerPolicyLifecycleStateV1::Active
    );
    let mcp_method = ProgrammaticMcpMethodReferenceV1::new(
        hex16("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
        "tools/list",
        "schema-1",
    )
    .expect("the fixture method reference is valid");
    let binding = ProgrammaticConfirmationBindingV1 {
        root_session_id: hex16("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
        root_run_id: hex16("cccccccc-cccc-4ccc-8ccc-cccccccccccc"),
        tool_call_id: hex16("dddddddd-dddd-4ddd-8ddd-dddddddddddd"),
        tool_id: "mcp".to_owned(),
        descriptor_revision: 1,
        mcp_method: Some(mcp_method),
        typed_input_digest: digest("slice3-typed-input"),
        policy_snapshot_digest: digest("slice3-policy-snapshot"),
        run_limits: ProgrammaticRunLimitsV1::maximum_v1(),
    };
    let confirmation = ProgrammaticExactConfirmationV1::new(
        hex16("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee"),
        binding.clone(),
        1_700_000_000_000,
        1_700_000_060_000,
    )
    .expect("the fixture confirmation is valid");
    assert!(confirmation.matches_binding(&binding));

    // Goal identity, readiness, and gates.
    let goal = GoalDto::new(
        hex16("12121212-1212-4212-8212-121212121212"),
        GoalScopeDto::Project {
            project_id: hex16("55555555-5555-4555-8555-555555555555"),
        },
        1,
        GoalLifecycleStateDto::Active,
        GoalReadinessStateDto::NotReady,
        GoalUserDecisionStateDto::Unaccepted,
    )
    .expect("the fixture goal is valid");
    let gate = GoalGateDefinitionV1 {
        gate_id: hex16("13131313-1313-4313-8313-131313131313"),
        gate: VerificationGateDto::ReferenceGate {
            evidence_contract_revision: 1,
            accepted_reference_kinds: vec![GoalEvidenceKindV1::TerminalChildResult],
        },
    };
    gate.gate.validate().expect("the fixture gate is valid");
    assert!(!gate.gate.is_executable());
    assert_eq!(goal.readiness_state(), &GoalReadinessStateDto::NotReady);

    // Verification authority and verdicts.
    let authority = VerificationMandateAuthorityDto {
        authority_id: hex16("14141414-1414-4414-8414-141414141414"),
        verifier_mandate_id: hex16("15151515-1515-4515-8515-151515151515"),
        authority_revision: 1,
        target_set_reference: VerifierTargetSetReferenceV1 {
            target_set_id: hex16("16161616-1616-4616-8616-161616161616"),
            target_set_digest: digest("slice3-verifier-target-set"),
        },
        allowed_operations: VerifierOperationV1::ALL.to_vec(),
        audit_contract_reference: VerifierContractReferenceV1 {
            contract_id: hex16("17171717-1717-4717-8717-171717171717"),
            contract_revision: 1,
            contract_digest: digest("slice3-verifier-audit-contract"),
        },
        canonical_authority_digest: digest("slice3-verifier-authority"),
    };
    assert_eq!(authority.allowed_operations.len(), 5);
    assert!(VerificationAuditVerdictDto::ALL.contains(&VerificationAuditVerdictDto::Pass));
    assert_eq!(VerificationAuditVerdictDto::Pass.code(), "pass");
}

#[test]
fn slice3_projection_validation_never_carries_secrets_or_paths() {
    assert_eq!(
        validate_harness_time_zone(FAKE_PATH)
            .expect_err("a path-bearing time zone fails closed")
            .code(),
        "harness_schedule_invalid"
    );
    assert_eq!(
        validate_harness_time_zone(FAKE_SECRET)
            .expect_err("a credential-shaped time zone fails closed")
            .code(),
        "credentials_forbidden"
    );
    assert!(
        validate_harness_time_zone("Europe/Berlin").is_ok(),
        "a valid IANA zone stays valid"
    );
    let harness_id = hex16("44444444-4444-4444-8444-444444444444");
    let error = HarnessRuleRevisionV1::new(
        harness_id,
        0,
        digest("slice3-harness-task"),
        HarnessExecutionClassV1::Light,
        HarnessTaskModeV1::RepeatedTask,
        vec![HarnessRuleSourceV1::ExplicitUserLaunch],
        HarnessPresentationModeV1::JournalOnly,
        "UTC".to_owned(),
    )
    .expect_err("a zero rule revision fails closed");
    assert_eq!(error.code(), "harness_revision_conflict");
    assert!(!format!("{error:?}").contains(FAKE_SECRET));
    assert!(
        ProgrammaticCallerRootOriginV1::InteractiveUser {
            originating_turn_id: hex16("11111111-1111-4111-8111-111111111111"),
        }
        .encode()
        .is_ok(),
        "the closed root origin encodes without any path or credential field"
    );
}

const fn fixed_run_limits() -> FixedRunLimits {
    FixedRunLimits {
        max_attempts: 1,
        max_total_seconds: 3600,
        max_actions: 256,
        max_concurrent_actions: 4,
        max_retained_bytes: 1_048_576,
        max_clarification_seconds: 3600,
    }
}

fn policy_selection(
    root_origin: ProgrammaticCallerRootOriginV1,
) -> ProgrammaticCallerPolicySelectionV1 {
    ProgrammaticCallerPolicySelectionV1 {
        root_origin,
        effective_policy_snapshot_reference: hex16("18181818-1818-4818-8818-181818181818"),
        policy_selection_digest: digest("slice3-policy-snapshot"),
        inherited_scope_provenance: vec![hex16("19191919-1919-4919-8919-191919191919")],
        fixed_run_limits: fixed_run_limits(),
    }
}

fn harness_selection() -> ContinualHarnessSelectionV1 {
    ContinualHarnessSelectionV1 {
        harness_id: hex16("44444444-4444-4444-8444-444444444444"),
        rule_revision: 3,
        trigger_reason: HarnessTriggerReasonV1 {
            reason_id: hex16("77777777-7777-4777-8777-777777777777"),
            source_kind: HarnessSourceKindV1::CalendarTime,
            first_observed_at_ms: 1_700_000_000_000,
            last_observed_at_ms: 1_700_000_060_000,
            coalesced_count: 2,
        },
        class_resolution: HarnessClassResolutionV1 {
            class: HarnessExecutionClassV1::Medium,
            narrowed_tool_ids: vec!["read".to_owned(), "glob".to_owned()],
        },
        dossier_digest: digest("slice3-dossier"),
        checkpoint_reference: Some(hex16("1a1a1a1a-1a1a-4a1a-8a1a-1a1a1a1a1a1a")),
        time_zone_application: "Europe/Berlin".to_owned(),
        bounds: HarnessSelectionBoundsV1 {
            max_cause_depth: 8,
            max_concurrent: 16,
            max_total_launches: 256,
            dossier_bytes: 512 * 1024,
            checkpoint_bytes: 512 * 1024,
            conclusion_bytes: 512 * 1024,
        },
    }
}

fn goal_selection() -> GoalRunSelectionV1 {
    GoalRunSelectionV1 {
        leading_goal_id: hex16("12121212-1212-4212-8212-121212121212"),
        goal_revision: 1,
        scope_link_provenance: GoalScopeLinkProvenanceV1 {
            project_id: hex16("55555555-5555-4555-8555-555555555555"),
            session_id: None,
            link_id: None,
        },
        parent_revision_chain: vec![GoalRevisionReferenceV1 {
            goal_id: hex16("1b1b1b1b-1b1b-4b1b-8b1b-1b1b1b1b1b1b"),
            revision: 1,
        }],
        obligatory_component_references: vec![hex16("1c1c1c1c-1c1c-4c1c-8c1c-1c1c1c1c1c1c")],
        selected_gate_revisions: vec![GoalGateRevisionReferenceV1 {
            gate_reference: hex16("13131313-1313-4313-8313-131313131313"),
            revision: 1,
        }],
        valid_evidence_references: vec![hex16("1d1d1d1d-1d1d-4d1d-8d1d-1d1d1d1d1d1d")],
        selected_memory_cards: vec![GoalCardReferenceV1 {
            card_reference: hex16("1e1e1e1e-1e1e-4e1e-8e1e-1e1e1e1e1e1e"),
            revision: 1,
        }],
        selected_skill_cards: Vec::new(),
        selected_role_cards: Vec::new(),
        revealed_full_record_references: Vec::new(),
        policy_snapshot_reference: hex16("18181818-1818-4818-8818-181818181818"),
        activity_selection_reference: hex16("1f1f1f1f-1f1f-4f1f-8f1f-1f1f1f1f1f1f"),
        run_kind: GoalRunKindV1::GoalDirectedOrdinary,
        target_snapshot_digest: digest("slice3-target-snapshot"),
        bounds: GoalRunSelectionBoundsV1 {
            max_memory_cards: 128,
            max_skill_role_cards: 32,
            target_snapshot_bytes: 1024 * 1024,
            context_bytes: 4 * 1024 * 1024,
        },
    }
}

fn mcp_selection() -> McpMethodCatalogSelectionV1 {
    McpMethodCatalogSelectionV1 {
        method_catalog_revision: "mcp-catalog-1".to_owned(),
        connection_reference: hex16("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
        server_revision_digest: digest("slice3-mcp-server"),
        method_reference: "tools/list".to_owned(),
        method_schema_revision: "schema-1".to_owned(),
        typed_input_constraint_family: Some("json-schema-2020-12".to_owned()),
    }
}

fn v4_record(all_selected: bool) -> RunExecutionMeaningV4Record {
    RunExecutionMeaningV4Record {
        fields: (0..6).map(|index| vec![index as u8; 3]).collect(),
        harness_selection: if all_selected {
            DisabledOr::Selected(harness_selection())
        } else {
            DisabledOr::Disabled
        },
        goal_selection: if all_selected {
            DisabledOr::Selected(goal_selection())
        } else {
            DisabledOr::Disabled
        },
        mcp_method_catalog_selection: if all_selected {
            DisabledOr::Selected(mcp_selection())
        } else {
            DisabledOr::Disabled
        },
        programmatic_caller_policy_selection: if all_selected {
            DisabledOr::Selected(policy_selection(
                ProgrammaticCallerRootOriginV1::InteractiveUser {
                    originating_turn_id: hex16("20202020-2020-4020-8020-202020202020"),
                },
            ))
        } else {
            DisabledOr::Disabled
        },
        agent_activity_selection: vec![0],
    }
}

#[derive(Clone, Copy)]
enum MarkerSlot {
    Harness,
    Goal,
    Policy,
}

fn v4_record_with_slot(slot: MarkerSlot, marker: Vec<u8>) -> Vec<u8> {
    let mut builder = CanonicalRecordBuilder::new(TagRegistry::RUN_EXECUTION_MEANING, 4);
    for number in 1..=6 {
        builder = builder
            .field(number, WireType::Record, vec![number as u8; 3])
            .expect("a v4 base field builds");
    }
    // The closed `Disabled` marker is the single one-byte `0` presence marker.
    let disabled = vec![0u8];
    let harness = if matches!(slot, MarkerSlot::Harness) {
        marker.clone()
    } else {
        disabled.clone()
    };
    let goal = if matches!(slot, MarkerSlot::Goal) {
        marker.clone()
    } else {
        disabled.clone()
    };
    let policy = if matches!(slot, MarkerSlot::Policy) {
        marker
    } else {
        disabled.clone()
    };
    for (number, value) in [
        (7, harness),
        (8, goal),
        (9, disabled),
        (10, policy),
        (11, vec![0]),
    ] {
        builder = builder
            .field(number, WireType::Record, value)
            .expect("a v4 slot builds");
    }
    builder.finish().expect("the v4 record frames")
}
