#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Slice 3 client fixtures use direct assertions and precise diagnostics."
)]

//! Slice 3 client facade contract tests.
//!
//! Owner: ADR 0044. These tests pin that the typed Slice 3 records are
//! reachable through the public client crate, that the Slice 3 selection
//! records round-trip losslessly through their canonical bytes on the client
//! path, that malformed or unsupported inputs fail with typed validation
//! errors instead of panicking, and that no credential-shaped or path-bearing
//! value crosses the client boundary. No socket, port, thread, sleep, or live
//! daemon is used.

use intention_client::IntentionClient;
use intention_client::slice3::{
    ContinualHarnessSelectionV1, DisabledOr, FixedRunLimits, GOAL_FAILURE_CODES,
    GoalCardReferenceV1, GoalDto, GoalGateDefinitionV1, GoalGateRevisionReferenceV1,
    GoalLifecycleStateDto, GoalReadinessStateDto, GoalRevisionReferenceV1, GoalRunKindV1,
    GoalRunSelectionBoundsV1, GoalRunSelectionV1, GoalScopeDto, GoalScopeLinkProvenanceV1,
    GoalUserDecisionStateDto, HARNESS_CLOSED_SAFE_FAILURES, HarnessClassResolutionV1,
    HarnessExecutionClassV1, HarnessIntervalScheduleV1, HarnessPresentationModeV1,
    HarnessRuleLifecycleStateV1, HarnessRuleOperationV1, HarnessRuleRevisionV1, HarnessRuleScopeV1,
    HarnessRuleSourceV1, HarnessRuleV1, HarnessSelectionBoundsV1, HarnessSourceKindV1,
    HarnessTaskModeV1, HarnessTriggerReasonV1, HarnessTriggerRecordV1, McpMethodCatalogSelectionV1,
    PROGRAMMATIC_POLICY_FAILURE_CODES, ProgrammaticAdmissionRuleV1,
    ProgrammaticCalendarPeriodKindV1, ProgrammaticCallerPolicyLifecycleStateV1,
    ProgrammaticCallerPolicyScopeV1, ProgrammaticCallerPolicySelectionV1,
    ProgrammaticCallerPolicyV1, ProgrammaticCallerRootOriginV1, ProgrammaticConfirmationBindingV1,
    ProgrammaticExactConfirmationV1, ProgrammaticMcpMethodReferenceV1, ProgrammaticRunLimitsV1,
    RunExecutionMeaningV4Record, VerificationAuditVerdictDto, VerificationGateDto,
    VerificationMandateAuthorityDto, VerifierContractReferenceV1, VerifierOperationV1,
    VerifierTargetSetReferenceV1, apply_harness_rule_operation, validate_harness_time_zone,
};
use intention_domain::canonical::Digest256;

const FAKE_SECRET: &str = "sk-live-slice3-client-fake-secret";
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

fn rule_revision() -> HarnessRuleRevisionV1 {
    HarnessRuleRevisionV1::new(
        hex16("11111111-1111-4111-8111-111111111111"),
        1,
        digest("slice3-client-harness-task"),
        HarnessExecutionClassV1::Medium,
        HarnessTaskModeV1::GoalDirected,
        vec![
            HarnessRuleSourceV1::ExplicitUserLaunch,
            HarnessRuleSourceV1::FixedInterval(
                HarnessIntervalScheduleV1::new(0, 60_000).expect("interval schedule is valid"),
            ),
        ],
        HarnessPresentationModeV1::JournalOnly,
        "Europe/Berlin".to_owned(),
    )
    .expect("the fixture rule revision is valid")
}

fn rule() -> HarnessRuleV1 {
    HarnessRuleV1::new(
        hex16("11111111-1111-4111-8111-111111111111"),
        HarnessRuleScopeV1::UserSession {
            project_id: hex16("22222222-2222-4222-8222-222222222222"),
            session_id: hex16("33333333-3333-4333-8333-333333333333"),
        },
        rule_revision(),
        hex16("44444444-4444-4444-8444-444444444444"),
    )
    .expect("the fixture rule is valid")
}

fn policy() -> ProgrammaticCallerPolicyV1 {
    ProgrammaticCallerPolicyV1::new(
        hex16("55555555-5555-4555-8555-555555555555"),
        ProgrammaticCallerPolicyScopeV1::Session {
            project_id: hex16("22222222-2222-4222-8222-222222222222"),
            policy_owner_session_id: hex16("33333333-3333-4333-8333-333333333333"),
        },
        ProgrammaticCalendarPeriodKindV1::Week,
        2,
    )
    .expect("the fixture policy is valid")
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
        effective_policy_snapshot_reference: hex16("66666666-6666-4666-8666-666666666666"),
        policy_selection_digest: digest("slice3-client-policy-snapshot"),
        inherited_scope_provenance: vec![hex16("77777777-7777-4777-8777-777777777777")],
        fixed_run_limits: fixed_run_limits(),
    }
}

fn harness_selection() -> ContinualHarnessSelectionV1 {
    ContinualHarnessSelectionV1 {
        harness_id: hex16("11111111-1111-4111-8111-111111111111"),
        rule_revision: 1,
        trigger_reason: HarnessTriggerReasonV1 {
            reason_id: hex16("88888888-8888-4888-8888-888888888888"),
            source_kind: HarnessSourceKindV1::ExplicitUserLaunch,
            first_observed_at_ms: 1_700_000_000_000,
            last_observed_at_ms: 1_700_000_000_000,
            coalesced_count: 1,
        },
        class_resolution: HarnessClassResolutionV1 {
            class: HarnessExecutionClassV1::Light,
            narrowed_tool_ids: vec!["read".to_owned()],
        },
        dossier_digest: digest("slice3-client-dossier"),
        checkpoint_reference: None,
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
        leading_goal_id: hex16("99999999-9999-4999-8999-999999999999"),
        goal_revision: 1,
        scope_link_provenance: GoalScopeLinkProvenanceV1 {
            project_id: hex16("22222222-2222-4222-8222-222222222222"),
            session_id: Some(hex16("33333333-3333-4333-8333-333333333333")),
            link_id: Some(hex16("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")),
        },
        parent_revision_chain: vec![GoalRevisionReferenceV1 {
            goal_id: hex16("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
            revision: 1,
        }],
        obligatory_component_references: vec![hex16("cccccccc-cccc-4ccc-8ccc-cccccccccccc")],
        selected_gate_revisions: vec![GoalGateRevisionReferenceV1 {
            gate_reference: hex16("dddddddd-dddd-4ddd-8ddd-dddddddddddd"),
            revision: 1,
        }],
        valid_evidence_references: vec![hex16("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee")],
        selected_memory_cards: vec![GoalCardReferenceV1 {
            card_reference: hex16("ffffffff-ffff-4fff-8fff-ffffffffffff"),
            revision: 1,
        }],
        selected_skill_cards: Vec::new(),
        selected_role_cards: Vec::new(),
        revealed_full_record_references: Vec::new(),
        policy_snapshot_reference: hex16("66666666-6666-4666-8666-666666666666"),
        activity_selection_reference: hex16("12121212-1212-4212-8212-121212121212"),
        run_kind: GoalRunKindV1::VerificationOnly,
        target_snapshot_digest: digest("slice3-client-target-snapshot"),
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
        connection_reference: hex16("13131313-1313-4313-8313-131313131313"),
        server_revision_digest: digest("slice3-client-mcp-server"),
        method_reference: "tools/list".to_owned(),
        method_schema_revision: "schema-1".to_owned(),
        typed_input_constraint_family: Some("json-schema-2020-12".to_owned()),
    }
}

#[test]
fn slice3_surface_is_reachable_through_the_public_client_crate() {
    let goal_id = hex16("99999999-9999-4999-8999-999999999999");
    let goal = GoalDto::new(
        goal_id,
        GoalScopeDto::Project {
            project_id: hex16("22222222-2222-4222-8222-222222222222"),
        },
        1,
        GoalLifecycleStateDto::Active,
        GoalReadinessStateDto::NotReady,
        GoalUserDecisionStateDto::Unaccepted,
    )
    .expect("the fixture goal is valid");
    assert_eq!(goal.goal_id(), goal_id);
    assert_eq!(goal.active_revision(), 1);
    assert_eq!(goal.lifecycle_state(), GoalLifecycleStateDto::Active);
    assert_eq!(goal.readiness_state(), &GoalReadinessStateDto::NotReady);
    assert_eq!(
        goal.user_decision_state(),
        &GoalUserDecisionStateDto::Unaccepted
    );

    let rule = rule();
    assert!(rule.lifecycle_state().is_active());
    assert_eq!(rule.active_revision().source_count(), 2);
    let trigger = HarnessTriggerRecordV1::new(
        HarnessTriggerReasonV1 {
            reason_id: hex16("88888888-8888-4888-8888-888888888888"),
            source_kind: HarnessSourceKindV1::FixedInterval,
            first_observed_at_ms: 1_700_000_000_000,
            last_observed_at_ms: 1_700_000_060_000,
            coalesced_count: 2,
        },
        1,
        "Europe/Berlin".to_owned(),
        None,
    )
    .expect("the fixture trigger is valid");
    assert_eq!(trigger.reason().coalesced_count, 2);

    let policy = policy();
    assert_eq!(
        policy.lifecycle_state,
        ProgrammaticCallerPolicyLifecycleStateV1::Active
    );
    assert_eq!(policy.active_revision, 2);
    assert_eq!(
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired.restrictiveness(),
        2
    );
    let binding = ProgrammaticConfirmationBindingV1 {
        root_session_id: hex16("33333333-3333-4333-8333-333333333333"),
        root_run_id: hex16("14141414-1414-4414-8414-141414141414"),
        tool_call_id: hex16("15151515-1515-4515-8515-151515151515"),
        tool_id: "read".to_owned(),
        descriptor_revision: 1,
        mcp_method: Some(
            ProgrammaticMcpMethodReferenceV1::new(
                hex16("13131313-1313-4313-8313-131313131313"),
                "tools/list",
                "schema-1",
            )
            .expect("the fixture method reference is valid"),
        ),
        typed_input_digest: digest("slice3-client-typed-input"),
        policy_snapshot_digest: digest("slice3-client-policy-snapshot"),
        run_limits: ProgrammaticRunLimitsV1::maximum_v1(),
    };
    let confirmation = ProgrammaticExactConfirmationV1::new(
        hex16("16161616-1616-4616-8616-161616161616"),
        binding.clone(),
        1_700_000_000_000,
        1_700_000_060_000,
    )
    .expect("the fixture confirmation is valid");
    assert!(confirmation.matches_binding(&binding));

    let gate = GoalGateDefinitionV1 {
        gate_id: hex16("dddddddd-dddd-4ddd-8ddd-dddddddddddd"),
        gate: VerificationGateDto::ExecutableGate {
            template_id: hex16("17171717-1717-4717-8717-171717171717"),
            template_revision: 1,
        },
    };
    assert!(gate.gate.is_executable());
    gate.gate.validate().expect("the fixture gate is valid");

    let authority = VerificationMandateAuthorityDto {
        authority_id: hex16("18181818-1818-4818-8818-181818181818"),
        verifier_mandate_id: hex16("19191919-1919-4919-8919-191919191919"),
        authority_revision: 1,
        target_set_reference: VerifierTargetSetReferenceV1 {
            target_set_id: hex16("1a1a1a1a-1a1a-4a1a-8a1a-1a1a1a1a1a1a"),
            target_set_digest: digest("slice3-client-target-set"),
        },
        allowed_operations: VerifierOperationV1::ALL.to_vec(),
        audit_contract_reference: VerifierContractReferenceV1 {
            contract_id: hex16("1b1b1b1b-1b1b-4b1b-8b1b-1b1b1b1b1b1b"),
            contract_revision: 1,
            contract_digest: digest("slice3-client-audit-contract"),
        },
        canonical_authority_digest: digest("slice3-client-authority"),
    };
    assert_eq!(authority.allowed_operations, VerifierOperationV1::ALL);
    assert_eq!(VerificationAuditVerdictDto::Fail.code(), "fail");
    assert_eq!(VerificationAuditVerdictDto::ALL.len(), 7);

    // The shared client facade itself stays nameable and unchanged.
    let _client: Option<IntentionClient> = None;
    assert_eq!(HARNESS_CLOSED_SAFE_FAILURES.len(), 15);
    assert_eq!(PROGRAMMATIC_POLICY_FAILURE_CODES.len(), 22);
    assert!(GOAL_FAILURE_CODES.len() >= 15);
}

#[test]
fn slice3_selection_records_round_trip_losslessly_through_the_client_path() {
    let interactive = ProgrammaticCallerRootOriginV1::InteractiveUser {
        originating_turn_id: hex16("12121212-1212-4212-8212-121212121212"),
    };
    let harness_root = ProgrammaticCallerRootOriginV1::ContinualHarness {
        harness_id: hex16("11111111-1111-4111-8111-111111111111"),
        rule_revision: 1,
        trigger_reason_id: hex16("88888888-8888-4888-8888-888888888888"),
    };
    for origin in [interactive, harness_root] {
        let encoded = origin.encode().expect("the root origin encodes");
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&encoded).expect("the root origin decodes"),
            origin
        );
    }
    let selection = policy_selection(ProgrammaticCallerRootOriginV1::ContinualHarness {
        harness_id: hex16("11111111-1111-4111-8111-111111111111"),
        rule_revision: 1,
        trigger_reason_id: hex16("88888888-8888-4888-8888-888888888888"),
    });
    let encoded = selection.encode().expect("the policy selection encodes");
    assert_eq!(
        ProgrammaticCallerPolicySelectionV1::decode(&encoded)
            .expect("the policy selection decodes"),
        selection
    );
    let harness = harness_selection();
    let harness_bytes = harness.encode().expect("harness selection encodes");
    assert!(!harness_bytes.is_empty());
    assert_eq!(
        ContinualHarnessSelectionV1::decode(&harness_bytes).expect("harness selection decodes"),
        harness
    );
    let goal = goal_selection();
    let goal_bytes = goal.encode().expect("goal selection encodes");
    assert_eq!(
        GoalRunSelectionV1::decode(&goal_bytes).expect("goal selection decodes"),
        goal
    );
    let mcp = mcp_selection();
    let mcp_bytes = mcp.encode().expect("mcp selection encodes");
    assert_eq!(
        McpMethodCatalogSelectionV1::decode(&mcp_bytes).expect("mcp selection decodes"),
        mcp
    );
    let record = RunExecutionMeaningV4Record {
        fields: (0..6).map(|index| vec![index as u8; 3]).collect(),
        harness_selection: DisabledOr::Selected(harness_selection()),
        goal_selection: DisabledOr::Selected(goal_selection()),
        mcp_method_catalog_selection: DisabledOr::Selected(mcp_selection()),
        programmatic_caller_policy_selection: DisabledOr::Selected(policy_selection(
            ProgrammaticCallerRootOriginV1::InteractiveUser {
                originating_turn_id: hex16("12121212-1212-4212-8212-121212121212"),
            },
        )),
        agent_activity_selection: vec![0],
    };
    let encoded = record.encode().expect("the v4 record encodes");
    assert_eq!(
        RunExecutionMeaningV4Record::decode(&encoded).expect("the v4 record decodes"),
        record
    );
}

#[test]
fn malformed_slice3_inputs_fail_with_typed_errors_instead_of_panicking() {
    let cases: [(&str, String); 8] = [
        (
            "goal_not_ready",
            GoalReadinessStateDto::ready(Vec::new())
                .expect_err("an empty ready set fails closed")
                .code()
                .to_owned(),
        ),
        (
            "goal_not_active",
            GoalDto::new(
                [0; 16],
                GoalScopeDto::Session {
                    project_id: hex16("22222222-2222-4222-8222-222222222222"),
                    session_id: hex16("33333333-3333-4333-8333-333333333333"),
                },
                1,
                GoalLifecycleStateDto::Active,
                GoalReadinessStateDto::NotReady,
                GoalUserDecisionStateDto::Unaccepted,
            )
            .expect_err("a zero Goal identity fails closed")
            .code()
            .to_owned(),
        ),
        (
            "goal_gate_unavailable",
            VerificationGateDto::ExecutableGate {
                template_id: [0; 16],
                template_revision: 0,
            }
            .validate()
            .expect_err("an inexact executable gate fails closed")
            .code()
            .to_owned(),
        ),
        (
            "harness_revision_conflict",
            HarnessRuleRevisionV1::new(
                hex16("11111111-1111-4111-8111-111111111111"),
                0,
                digest("slice3-client-harness-task"),
                HarnessExecutionClassV1::Light,
                HarnessTaskModeV1::RepeatedTask,
                vec![HarnessRuleSourceV1::ExplicitUserLaunch],
                HarnessPresentationModeV1::JournalOnly,
                "UTC".to_owned(),
            )
            .expect_err("a zero rule revision fails closed")
            .code()
            .to_owned(),
        ),
        (
            "harness_source_limit_exceeded",
            HarnessRuleRevisionV1::new(
                hex16("11111111-1111-4111-8111-111111111111"),
                1,
                digest("slice3-client-harness-task"),
                HarnessExecutionClassV1::Light,
                HarnessTaskModeV1::RepeatedTask,
                (0..17)
                    .map(|_| HarnessRuleSourceV1::ExplicitUserLaunch)
                    .collect(),
                HarnessPresentationModeV1::JournalOnly,
                "UTC".to_owned(),
            )
            .expect_err("seventeen sources fail closed")
            .code()
            .to_owned(),
        ),
        (
            "harness_interval_too_short",
            HarnessIntervalScheduleV1::new(0, 59_999)
                .expect_err("an interval below one minute fails closed")
                .code()
                .to_owned(),
        ),
        (
            "programmatic_policy_revision_conflict",
            ProgrammaticCallerPolicyV1::new(
                hex16("55555555-5555-4555-8555-555555555555"),
                ProgrammaticCallerPolicyScopeV1::Project {
                    project_id: hex16("22222222-2222-4222-8222-222222222222"),
                },
                ProgrammaticCalendarPeriodKindV1::Day,
                0,
            )
            .expect_err("a zero active policy revision fails closed")
            .code()
            .to_owned(),
        ),
        (
            "programmatic_policy_confirmation_expired",
            ProgrammaticExactConfirmationV1::new(
                hex16("16161616-1616-4616-8616-161616161616"),
                ProgrammaticConfirmationBindingV1 {
                    root_session_id: hex16("33333333-3333-4333-8333-333333333333"),
                    root_run_id: hex16("14141414-1414-4414-8414-141414141414"),
                    tool_call_id: hex16("15151515-1515-4515-8515-151515151515"),
                    tool_id: "read".to_owned(),
                    descriptor_revision: 1,
                    mcp_method: None,
                    typed_input_digest: digest("slice3-client-typed-input"),
                    policy_snapshot_digest: digest("slice3-client-policy-snapshot"),
                    run_limits: ProgrammaticRunLimitsV1::maximum_v1(),
                },
                100,
                100,
            )
            .expect_err("a zero confirmation window fails closed")
            .code()
            .to_owned(),
        ),
    ];
    for (expected, code) in cases {
        assert_eq!(code, expected, "the typed error code is stable and safe");
    }

    // Unsupported lifecycle transitions fail closed through the same values.
    let archived = apply_harness_rule_operation(&rule(), false, HarnessRuleOperationV1::Archive)
        .expect("an idle rule archives");
    assert_eq!(
        archived.lifecycle_state(),
        HarnessRuleLifecycleStateV1::Archived
    );
    assert_eq!(
        apply_harness_rule_operation(&archived, false, HarnessRuleOperationV1::Resume)
            .expect_err("an archived rule rejects every operation")
            .code(),
        "harness_archived"
    );
    assert_eq!(
        apply_harness_rule_operation(&rule(), true, HarnessRuleOperationV1::Archive)
            .expect_err("a rule with active work cannot archive")
            .code(),
        "harness_not_active"
    );
    assert!(VerifierOperationV1::from_discriminant(5).is_none());
    assert!(VerificationAuditVerdictDto::from_discriminant(7).is_none());
}

#[test]
fn no_credential_shaped_or_path_bearing_value_crosses_the_client_boundary() {
    // Credential-shaped text fails closed with the shared codec code.
    assert_eq!(
        ProgrammaticMcpMethodReferenceV1::new(
            hex16("13131313-1313-4313-8313-131313131313"),
            FAKE_SECRET,
            "schema-1",
        )
        .expect_err("a credential-shaped method reference fails closed")
        .code(),
        "credentials_forbidden"
    );
    assert_eq!(
        validate_harness_time_zone(FAKE_SECRET)
            .expect_err("a credential-shaped time zone fails closed")
            .code(),
        "credentials_forbidden"
    );
    // Path-bearing text in an identifier position fails closed too.
    assert_eq!(
        validate_harness_time_zone(FAKE_PATH)
            .expect_err("a path-bearing time zone fails closed")
            .code(),
        "harness_schedule_invalid"
    );

    // The typed projections that do cross the boundary never embed a secret or
    // a filesystem path; their fields are identities, revisions, digests, and
    // bounds.
    let projections = [
        format!("{:?}", rule()),
        format!("{:?}", policy()),
        format!("{:?}", harness_selection()),
        format!("{:?}", goal_selection()),
        format!("{:?}", mcp_selection()),
        format!(
            "{:?}",
            policy_selection(ProgrammaticCallerRootOriginV1::InteractiveUser {
                originating_turn_id: hex16("12121212-1212-4212-8212-121212121212"),
            })
        ),
        format!("{:?}", VerificationAuditVerdictDto::Pass),
        format!("{:?}", VerifierOperationV1::MarkComplete),
    ];
    for projection in projections {
        assert!(
            !projection.contains(FAKE_SECRET),
            "a client projection must never carry a credential"
        );
        assert!(
            !projection.contains(FAKE_PATH),
            "a client projection must never carry a filesystem path"
        );
    }
}

#[test]
fn slice3_client_surface_adds_no_negotiated_capability() {
    // Slice 3 rides the existing daemon facade and the single live
    // run-execution-meaning v4 record; no capability token is introduced.
    assert_eq!(intention_protocol::POST_M5_CAPABILITIES.len(), 7);
    let tokens = format!("{:?}", intention_protocol::POST_M5_CAPABILITIES);
    for slice3_surface in ["Harness", "Goal", "Policy", "Verification"] {
        assert!(
            !tokens.contains(slice3_surface),
            "no Slice 3 capability token may be negotiated"
        );
    }
    assert_eq!(
        intention_protocol::CURRENT_PROTOCOL_VERSION,
        intention_protocol::ProtocolVersionDto::new(1, 1)
    );
    assert_eq!(
        intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
        intention_types::SchemaVersionDto::new(1, 1)
    );
}
