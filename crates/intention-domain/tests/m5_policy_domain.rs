#![allow(
    clippy::expect_used,
    reason = "M5+ policy domain fixtures use expect for precise test diagnostics."
)]

//! Slice 3 programmatic-caller policy domain contract tests.
//!
//! Owner: architecture 27 with ADR 0044. Every test states the architecture 27
//! required-evidence row it satisfies.

use std::collections::BTreeSet;

use intention_domain::canonical::Digest256;
use intention_domain::programmatic_policy::{
    DescriptorInputConstraintSelectionV1, HARNESS_DELEGATION_TOOL_ID,
    INTERACTIVE_LOCAL_READ_BASELINE_ACTIONS, INTERACTIVE_LOCAL_READ_BASELINE_CONCURRENT_ACTIONS,
    InteractiveLocalReadBaselineV1, MAX_AWAITING_CONFIRMATIONS_PER_ROOT_TREE,
    MAX_EFFECTIVE_POLICY_SNAPSHOT_BYTES, MAX_POLICIES_PER_GOAL, MAX_POLICIES_PER_PROJECT,
    MAX_POLICY_ACTIONS_PER_RUN, MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN, MAX_POLICY_RECORD_BYTES,
    MAX_POLICY_REFERENCES_PER_SESSION, MAX_RULES_PER_POLICY_REVISION,
    MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE, MAX_UNFINISHED_CORRIDORS_PER_ROOT_TREE,
    PROGRAMMATIC_POLICY_CALENDAR_LIMIT_EXCEEDED, PROGRAMMATIC_POLICY_CONFIRMATION_EXPIRED,
    PROGRAMMATIC_POLICY_CONFIRMATION_REQUIRED, PROGRAMMATIC_POLICY_CORRIDOR_EXHAUSTED,
    PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE, PROGRAMMATIC_POLICY_COUNTER_UNAVAILABLE,
    PROGRAMMATIC_POLICY_DRAFT_CONFLICT, PROGRAMMATIC_POLICY_DRAFT_TOO_LARGE,
    PROGRAMMATIC_POLICY_FAILURE_CODES, PROGRAMMATIC_POLICY_HARNESS_DELEGATION_FORBIDDEN,
    PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
    PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH, PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
    PROGRAMMATIC_POLICY_NOT_APPLICABLE, PROGRAMMATIC_POLICY_ORIGIN_INVALID,
    PROGRAMMATIC_POLICY_RESERVATION_CONFLICT, PROGRAMMATIC_POLICY_REVISION_CONFLICT,
    PROGRAMMATIC_POLICY_REVOKED, PROGRAMMATIC_POLICY_ROOT_ONLY_INTERACTION,
    PROGRAMMATIC_POLICY_RUN_LIMIT_EXCEEDED, PROGRAMMATIC_POLICY_SNAPSHOT_TOO_LARGE,
    PROGRAMMATIC_POLICY_SNAPSHOT_UNAVAILABLE, PROGRAMMATIC_POLICY_SUSPENDED,
    ProgrammaticAdmissionCallV1, ProgrammaticAdmissionRuleEntryV1, ProgrammaticAdmissionRuleV1,
    ProgrammaticAuthorizationCorridorV1, ProgrammaticCalendarLimitV1,
    ProgrammaticCalendarPeriodKindV1, ProgrammaticCallContextV1,
    ProgrammaticCallerApplicablePolicyV1, ProgrammaticCallerPathV1,
    ProgrammaticCallerPolicyDraftV1, ProgrammaticCallerPolicyLifecycleStateV1,
    ProgrammaticCallerPolicyRevisionReferenceV1, ProgrammaticCallerPolicyRevisionV1,
    ProgrammaticCallerPolicyScopeV1, ProgrammaticCallerPolicyV1, ProgrammaticConfirmationBindingV1,
    ProgrammaticCorridorRootV1, ProgrammaticCorridorSelectorsV1, ProgrammaticCorridorUsageV1,
    ProgrammaticDraftProposalOutcomeV1, ProgrammaticExactConfirmationV1, ProgrammaticLimitStateV1,
    ProgrammaticMcpMethodReferenceV1, ProgrammaticPolicyDraftContentV1,
    ProgrammaticPolicyDraftDecisionV1, ProgrammaticPolicyDraftOutcomeV1,
    ProgrammaticProvenanceLinksV1, ProgrammaticRecoveryDispositionV1,
    ProgrammaticReservationBindingV1, ProgrammaticRootOriginKindV1, ProgrammaticRootOriginRuleV1,
    ProgrammaticRuleSelectorV1, ProgrammaticRunLimitsV1, ProgrammaticToolEffectFlagV1,
    USER_INTERACTION_TOOL_ID, apply_policy_draft_decision, apply_recovery_disposition,
    commit_reservation_started, effective_calendar_action_limit, effective_policy_snapshot_digest,
    finish_started_action, new_provenance, path_may_root, policy_identity_digest,
    policy_revision_digest, propose_policy_draft, provenance_digest, recovery_disposition_for,
    release_unstarted_reservation, reserve_limit_action, resolve_effective_policy_snapshot,
    resolve_interactive_local_read_baseline, resolve_snapshot_decision, tighten_admission_rule,
    validate_awaiting_confirmations_per_root_tree, validate_bounded_corridor_admission,
    validate_calendar_revision, validate_corridor_active, validate_corridor_child_selectors,
    validate_corridor_extension, validate_corridor_use, validate_counter_available,
    validate_direct_local_read_tool, validate_draft_size, validate_effective_policy_snapshot_size,
    validate_fork_policy_inheritance, validate_harness_delegation, validate_interaction_admission,
    validate_live_policy_state, validate_policies_attached_to_goal, validate_policies_in_project,
    validate_policy_lifecycle_transition, validate_policy_record_size,
    validate_provenance_continuity, validate_revision_narrowing, validate_scope_applicability,
    validate_selector_narrowing, validate_session_policy_references,
    validate_unfinished_corridors_per_root_tree,
};
use intention_domain::slice3_selections::ProgrammaticCallerRootOriginV1;
use intention_types::DtoResult;

const PROJECT: [u8; 16] = [7; 16];
const ROOT_SESSION: [u8; 16] = [4; 16];
const ROOT_RUN: [u8; 16] = [10; 16];
const DESCENDANT_SESSION: [u8; 16] = [6; 16];
const DESCENDANT_RUN: [u8; 16] = [11; 16];
const HARNESS_SESSION: [u8; 16] = [8; 16];
const HARNESS_RUN: [u8; 16] = [12; 16];
const HARNESS_DESCENDANT_SESSION: [u8; 16] = [9; 16];
const HARNESS_DESCENDANT_RUN: [u8; 16] = [13; 16];
const POLICY_A: [u8; 16] = [31; 16];
const POLICY_B: [u8; 16] = [32; 16];

fn rejection_code<T: std::fmt::Debug>(result: DtoResult<T>) -> String {
    result
        .expect_err("the operation rejects before an external effect")
        .code()
        .to_owned()
}

const fn interactive_origin() -> ProgrammaticCallerRootOriginV1 {
    ProgrammaticCallerRootOriginV1::InteractiveUser {
        originating_turn_id: [1; 16],
    }
}

const fn harness_origin() -> ProgrammaticCallerRootOriginV1 {
    ProgrammaticCallerRootOriginV1::ContinualHarness {
        harness_id: [2; 16],
        rule_revision: 7,
        trigger_reason_id: [3; 16],
    }
}

const fn links(
    parents: Vec<[u8; 16]>,
    bridge: Option<[u8; 16]>,
    goal: Option<[u8; 16]>,
) -> ProgrammaticProvenanceLinksV1 {
    ProgrammaticProvenanceLinksV1 {
        parent_link_references: parents,
        bridge_operation_reference: bridge,
        leading_goal_reference: goal,
    }
}

const fn context(
    kind: ProgrammaticRootOriginKindV1,
    root_session: [u8; 16],
    root_run: [u8; 16],
    current_session: [u8; 16],
    current_run: [u8; 16],
    tool_call: [u8; 16],
) -> ProgrammaticCallContextV1 {
    ProgrammaticCallContextV1 {
        root_origin: kind,
        root_session_id: root_session,
        root_run_id: root_run,
        current_session_id: current_session,
        current_run_id: current_run,
        tool_call_id: tool_call,
    }
}

fn read_call_with_constraints(
    constraints: Vec<DescriptorInputConstraintSelectionV1>,
) -> ProgrammaticAdmissionCallV1 {
    ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ROOT_SESSION,
            ROOT_RUN,
            ROOT_SESSION,
            ROOT_RUN,
            [5; 16],
        ),
        "read",
        4,
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        None,
        constraints,
        Digest256::sha256(b"read-input"),
    )
    .expect("fixture read call is valid")
}

fn read_call() -> ProgrammaticAdmissionCallV1 {
    read_call_with_constraints(Vec::new())
}

fn descendant_call() -> ProgrammaticAdmissionCallV1 {
    ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ROOT_SESSION,
            ROOT_RUN,
            DESCENDANT_SESSION,
            DESCENDANT_RUN,
            [6; 16],
        ),
        "read",
        4,
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        None,
        Vec::new(),
        Digest256::sha256(b"descendant-input"),
    )
    .expect("fixture descendant call is valid")
}

fn harness_call() -> ProgrammaticAdmissionCallV1 {
    ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            HARNESS_SESSION,
            HARNESS_RUN,
            HARNESS_SESSION,
            HARNESS_RUN,
            [7; 16],
        ),
        "read",
        4,
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        None,
        Vec::new(),
        Digest256::sha256(b"harness-input"),
    )
    .expect("fixture harness call is valid")
}

fn harness_descendant_call() -> ProgrammaticAdmissionCallV1 {
    ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            HARNESS_SESSION,
            HARNESS_RUN,
            HARNESS_DESCENDANT_SESSION,
            HARNESS_DESCENDANT_RUN,
            [8; 16],
        ),
        "read",
        4,
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        None,
        Vec::new(),
        Digest256::sha256(b"harness-descendant-input"),
    )
    .expect("fixture harness descendant call is valid")
}

fn run_limits() -> ProgrammaticRunLimitsV1 {
    ProgrammaticRunLimitsV1::new(64, 4).expect("fixture run limits are valid")
}

fn calendar() -> ProgrammaticCalendarLimitV1 {
    calendar_with(128)
}

fn calendar_with(max_actions: u64) -> ProgrammaticCalendarLimitV1 {
    ProgrammaticCalendarLimitV1::new(ProgrammaticCalendarPeriodKindV1::Day, max_actions)
        .expect("fixture calendar limit is valid")
}

fn read_selector() -> ProgrammaticRuleSelectorV1 {
    ProgrammaticRuleSelectorV1::exact_tool("read", 4).expect("fixture read selector is valid")
}

fn entry(
    decision: ProgrammaticAdmissionRuleV1,
    selectors: Vec<ProgrammaticRuleSelectorV1>,
) -> ProgrammaticAdmissionRuleEntryV1 {
    ProgrammaticAdmissionRuleEntryV1::new(decision, selectors, Vec::new())
        .expect("fixture rule entry is valid")
}

fn read_rule(decision: ProgrammaticAdmissionRuleV1) -> ProgrammaticAdmissionRuleEntryV1 {
    entry(decision, vec![read_selector()])
}

fn applicable_with(
    kind: ProgrammaticRootOriginKindV1,
    policy_id: [u8; 16],
    ceiling: ProgrammaticAdmissionRuleV1,
    rules: Vec<ProgrammaticAdmissionRuleEntryV1>,
    limits: ProgrammaticRunLimitsV1,
    calendar_limit: ProgrammaticCalendarLimitV1,
) -> ProgrammaticCallerApplicablePolicyV1 {
    let revision = ProgrammaticCallerPolicyRevisionV1::new(
        policy_id,
        1,
        vec![ProgrammaticRootOriginRuleV1::new(kind, ceiling)],
        rules,
        limits,
        calendar_limit,
        Vec::new(),
    )
    .expect("fixture revision is valid");
    ProgrammaticCallerApplicablePolicyV1::new(
        policy_id,
        1,
        ProgrammaticCallerPolicyScopeV1::Project {
            project_id: PROJECT,
        },
        revision,
    )
    .expect("fixture applicable policy is coherent")
}

fn applicable(
    kind: ProgrammaticRootOriginKindV1,
    policy_id: [u8; 16],
    ceiling: ProgrammaticAdmissionRuleV1,
    rules: Vec<ProgrammaticAdmissionRuleEntryV1>,
) -> ProgrammaticCallerApplicablePolicyV1 {
    applicable_with(kind, policy_id, ceiling, rules, run_limits(), calendar())
}

fn baseline_snapshot()
-> intention_domain::programmatic_policy::EffectiveProgrammaticCallerPolicySnapshotV1 {
    resolve_effective_policy_snapshot(ProgrammaticRootOriginKindV1::InteractiveUser, &[])
        .expect("policy-less interactive snapshot resolves")
}

fn corridor_with(
    snapshot_digest: Digest256,
    selectors: ProgrammaticCorridorSelectorsV1,
) -> ProgrammaticAuthorizationCorridorV1 {
    ProgrammaticAuthorizationCorridorV1::new(
        ProgrammaticCorridorRootV1 {
            root_session_id: ROOT_SESSION,
            root_run_id: ROOT_RUN,
            root_origin_kind: ProgrammaticRootOriginKindV1::InteractiveUser,
            effective_policy_snapshot_digest: snapshot_digest,
        },
        selectors,
        8,
        2,
        [40; 16],
    )
    .expect("fixture corridor is valid")
}

fn corridor_for(snapshot_digest: Digest256) -> ProgrammaticAuthorizationCorridorV1 {
    let selectors = ProgrammaticCorridorSelectorsV1::new(
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        vec![read_selector()],
        Vec::new(),
    )
    .expect("fixture corridor selectors are valid");
    corridor_with(snapshot_digest, selectors)
}

const fn reserve_binding(
    tool_call_id: [u8; 16],
    typed_input_digest: &Digest256,
) -> ProgrammaticReservationBindingV1 {
    ProgrammaticReservationBindingV1 {
        policy_id: POLICY_A,
        policy_revision: 1,
        root_run_id: ROOT_RUN,
        tool_call_id,
        typed_input_digest: *typed_input_digest,
    }
}

fn draft_content_with(ceiling: ProgrammaticAdmissionRuleV1) -> ProgrammaticPolicyDraftContentV1 {
    ProgrammaticPolicyDraftContentV1 {
        root_origin_rules: vec![ProgrammaticRootOriginRuleV1::new(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ceiling,
        )],
        admission_rules: vec![read_rule(ceiling)],
        run_limits: run_limits(),
        calendar_limit: calendar(),
    }
}

fn draft_content() -> ProgrammaticPolicyDraftContentV1 {
    draft_content_with(ProgrammaticAdmissionRuleV1::DirectLocalRead)
}

#[test]
fn closed_failure_set_declares_each_policy_code_exactly_once() {
    let mut seen = BTreeSet::new();
    for code in PROGRAMMATIC_POLICY_FAILURE_CODES {
        assert!(
            code.starts_with("programmatic_policy_"),
            "code {code} keeps the closed prefix"
        );
        assert!(seen.insert(code), "code {code} is declared exactly once");
    }
    assert_eq!(seen.len(), 22);
    assert!(seen.contains(PROGRAMMATIC_POLICY_DRAFT_TOO_LARGE));
}

#[test]
fn root_origin_closed_set_has_exactly_two_roots_and_no_third_path() {
    let paths = [
        ProgrammaticCallerPathV1::InteractiveUserTurn,
        ProgrammaticCallerPathV1::ContinualHarnessLaunch,
        ProgrammaticCallerPathV1::ProtocolPeer,
        ProgrammaticCallerPathV1::DetachedPythonTask,
        ProgrammaticCallerPathV1::ChildAgent,
        ProgrammaticCallerPathV1::McpService,
        ProgrammaticCallerPathV1::Provider,
        ProgrammaticCallerPathV1::BridgeChannel,
        ProgrammaticCallerPathV1::QueuedItem,
        ProgrammaticCallerPathV1::Replay,
        ProgrammaticCallerPathV1::DaemonRecovery,
    ];
    let roots: Vec<ProgrammaticCallerPathV1> = paths
        .into_iter()
        .filter(|path| path_may_root(*path))
        .collect();
    assert_eq!(
        roots,
        vec![
            ProgrammaticCallerPathV1::InteractiveUserTurn,
            ProgrammaticCallerPathV1::ContinualHarnessLaunch,
        ]
    );

    assert_eq!(
        ProgrammaticRootOriginKindV1::of(&interactive_origin()),
        ProgrammaticRootOriginKindV1::InteractiveUser
    );
    assert_eq!(
        ProgrammaticRootOriginKindV1::of(&harness_origin()),
        ProgrammaticRootOriginKindV1::ContinualHarness
    );
    assert!(ProgrammaticRootOriginKindV1::InteractiveUser.is_interactive_user());
    assert!(!ProgrammaticRootOriginKindV1::ContinualHarness.is_interactive_user());
}

#[test]
fn provenance_requires_daemon_assigned_root_shape_and_never_re_roots() {
    let root_provenance = new_provenance(
        interactive_origin(),
        &read_call(),
        links(Vec::new(), None, None),
        [20; 16],
        [21; 16],
        vec![[22; 16]],
    )
    .expect("root-run provenance is valid");
    assert_eq!(root_provenance.root_session_id, ROOT_SESSION);
    assert_eq!(root_provenance.current_run_id, ROOT_RUN);
    assert!(root_provenance.parent_link_references.is_empty());
    assert_eq!(root_provenance.tool_call_id, [5; 16]);
    assert_eq!(root_provenance.selected_tool_id, "read");

    let descendant = new_provenance(
        interactive_origin(),
        &descendant_call(),
        links(vec![[30; 16]], None, None),
        [20; 16],
        [21; 16],
        Vec::new(),
    )
    .expect("descendant provenance is valid");
    validate_provenance_continuity(&root_provenance, &descendant)
        .expect("the descendant continues the one root tree");

    let bad_root = new_provenance(
        interactive_origin(),
        &read_call(),
        links(vec![[30; 16]], None, None),
        [20; 16],
        [21; 16],
        Vec::new(),
    );
    assert_eq!(rejection_code(bad_root), PROGRAMMATIC_POLICY_ORIGIN_INVALID);

    let bad_descendant = new_provenance(
        interactive_origin(),
        &descendant_call(),
        links(Vec::new(), None, None),
        [20; 16],
        [21; 16],
        Vec::new(),
    );
    assert_eq!(
        rejection_code(bad_descendant),
        PROGRAMMATIC_POLICY_ORIGIN_INVALID
    );

    let mismatched_root = new_provenance(
        harness_origin(),
        &descendant_call(),
        links(vec![[30; 16]], None, None),
        [20; 16],
        [21; 16],
        Vec::new(),
    );
    assert_eq!(
        rejection_code(mismatched_root),
        PROGRAMMATIC_POLICY_ORIGIN_INVALID
    );

    let harness_descendant = new_provenance(
        harness_origin(),
        &harness_descendant_call(),
        links(vec![[30; 16]], None, None),
        [20; 16],
        [21; 16],
        Vec::new(),
    )
    .expect("harness descendant provenance is shaped like a descendant");
    assert_eq!(
        rejection_code(validate_provenance_continuity(
            &root_provenance,
            &harness_descendant
        )),
        PROGRAMMATIC_POLICY_ORIGIN_INVALID
    );
}

#[test]
fn provenance_digest_is_deterministic_and_derived_from_safe_fields_only() {
    let first = new_provenance(
        interactive_origin(),
        &read_call(),
        links(Vec::new(), None, None),
        [20; 16],
        [21; 16],
        Vec::new(),
    )
    .expect("fixture provenance is valid");
    let second = new_provenance(
        interactive_origin(),
        &read_call(),
        links(Vec::new(), None, None),
        [20; 16],
        [21; 16],
        Vec::new(),
    )
    .expect("fixture provenance is valid");
    assert_eq!(first, second);
    assert_eq!(provenance_digest(&first), first.canonical_provenance_digest);

    let changed = new_provenance(
        interactive_origin(),
        &read_call(),
        links(Vec::new(), None, None),
        [20; 16],
        [23; 16],
        Vec::new(),
    )
    .expect("fixture provenance is valid");
    assert_ne!(
        changed.canonical_provenance_digest,
        first.canonical_provenance_digest
    );
}

#[test]
fn policy_scope_applies_only_inside_its_project_and_selected_goal_or_session() {
    let project = ProgrammaticCallerPolicyScopeV1::Project {
        project_id: PROJECT,
    };
    validate_scope_applicability(&project, PROJECT, DESCENDANT_SESSION, &[], false)
        .expect("a project policy applies to every session in its project");
    assert_eq!(
        rejection_code(validate_scope_applicability(
            &project,
            [70; 16],
            DESCENDANT_SESSION,
            &[],
            false
        )),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );

    let goal = ProgrammaticCallerPolicyScopeV1::Goal {
        project_id: PROJECT,
        goal_id: [9; 16],
    };
    validate_scope_applicability(&goal, PROJECT, ROOT_SESSION, &[[9; 16]], false)
        .expect("a goal policy applies through its leading goal");
    validate_scope_applicability(&goal, PROJECT, ROOT_SESSION, &[[99; 16], [9; 16]], false)
        .expect("a goal policy applies through an ancestor chain");
    assert_eq!(
        rejection_code(validate_scope_applicability(
            &goal,
            PROJECT,
            ROOT_SESSION,
            &[[99; 16]],
            false
        )),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );

    let session = ProgrammaticCallerPolicyScopeV1::Session {
        project_id: PROJECT,
        policy_owner_session_id: ROOT_SESSION,
    };
    validate_scope_applicability(&session, PROJECT, ROOT_SESSION, &[], false)
        .expect("a session policy applies to its owner session");
    validate_scope_applicability(&session, PROJECT, DESCENDANT_SESSION, &[], true)
        .expect("a session policy applies to a fork only through its inherited reference");
    assert_eq!(
        rejection_code(validate_scope_applicability(
            &session,
            PROJECT,
            DESCENDANT_SESSION,
            &[],
            false
        )),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );
    assert_eq!(
        session.project_id(),
        PROJECT,
        "scope identity must expose its project"
    );
}

#[test]
fn policy_revision_identity_is_immutable_bounded_and_digest_stable() {
    let revision = ProgrammaticCallerPolicyRevisionV1::new(
        POLICY_A,
        1,
        vec![ProgrammaticRootOriginRuleV1::new(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ProgrammaticAdmissionRuleV1::DirectLocalRead,
        )],
        vec![read_rule(ProgrammaticAdmissionRuleV1::DirectLocalRead)],
        run_limits(),
        calendar(),
        Vec::new(),
    )
    .expect("fixture revision is valid");
    assert_eq!(revision.policy_id, POLICY_A);
    assert_eq!(revision.revision, 1);
    assert_eq!(
        policy_revision_digest(&revision),
        revision.canonical_revision_digest
    );

    assert_eq!(
        rejection_code(ProgrammaticCallerPolicyRevisionV1::new(
            POLICY_A,
            0,
            vec![ProgrammaticRootOriginRuleV1::new(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                ProgrammaticAdmissionRuleV1::DirectLocalRead,
            )],
            Vec::new(),
            run_limits(),
            calendar(),
            Vec::new(),
        )),
        PROGRAMMATIC_POLICY_REVISION_CONFLICT
    );

    let over_limit_rules = (0..=MAX_RULES_PER_POLICY_REVISION)
        .map(|_| read_rule(ProgrammaticAdmissionRuleV1::DirectLocalRead))
        .collect();
    assert_eq!(
        rejection_code(ProgrammaticCallerPolicyRevisionV1::new(
            POLICY_A,
            1,
            vec![ProgrammaticRootOriginRuleV1::new(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                ProgrammaticAdmissionRuleV1::DirectLocalRead,
            )],
            over_limit_rules,
            run_limits(),
            calendar(),
            Vec::new(),
        )),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );

    assert_eq!(
        rejection_code(ProgrammaticCallerPolicyRevisionV1::new(
            POLICY_A,
            1,
            vec![
                ProgrammaticRootOriginRuleV1::new(
                    ProgrammaticRootOriginKindV1::InteractiveUser,
                    ProgrammaticAdmissionRuleV1::DirectLocalRead,
                ),
                ProgrammaticRootOriginRuleV1::new(
                    ProgrammaticRootOriginKindV1::InteractiveUser,
                    ProgrammaticAdmissionRuleV1::Prohibited,
                ),
            ],
            Vec::new(),
            run_limits(),
            calendar(),
            Vec::new(),
        )),
        PROGRAMMATIC_POLICY_ORIGIN_INVALID
    );

    assert_eq!(
        rejection_code(validate_policy_record_size(MAX_POLICY_RECORD_BYTES + 1)),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );
}

#[test]
fn narrowing_only_child_revisions_are_accepted() {
    let parent_limits = ProgrammaticRunLimitsV1::new(128, 8).expect("parent limits are valid");
    let child_limits = ProgrammaticRunLimitsV1::new(64, 4).expect("child limits are valid");
    validate_revision_narrowing(
        parent_limits,
        &calendar(),
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        child_limits,
        &calendar_with(64),
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
    )
    .expect("equal and smaller child bounds narrow the parent");

    let larger_actions = ProgrammaticRunLimitsV1::new(129, 4).expect("limit is valid");
    assert_eq!(
        rejection_code(validate_revision_narrowing(
            parent_limits,
            &calendar(),
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
            larger_actions,
            &calendar(),
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        )),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );

    let weekly = ProgrammaticCalendarLimitV1::new(ProgrammaticCalendarPeriodKindV1::Week, 100)
        .expect("weekly calendar limit is valid");
    assert_eq!(
        rejection_code(validate_revision_narrowing(
            parent_limits,
            &calendar(),
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
            child_limits,
            &weekly,
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        )),
        PROGRAMMATIC_POLICY_REVISION_CONFLICT
    );

    assert_eq!(
        rejection_code(validate_revision_narrowing(
            parent_limits,
            &calendar(),
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
            child_limits,
            &calendar_with(129),
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        )),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );

    assert_eq!(
        rejection_code(validate_revision_narrowing(
            parent_limits,
            &calendar(),
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
            child_limits,
            &calendar(),
            ProgrammaticAdmissionRuleV1::DirectLocalRead,
        )),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );

    assert_eq!(
        rejection_code(validate_calendar_revision(
            &calendar(),
            &ProgrammaticCalendarLimitV1::new(ProgrammaticCalendarPeriodKindV1::Month, 1)
                .expect("monthly calendar limit is valid")
        )),
        PROGRAMMATIC_POLICY_REVISION_CONFLICT
    );

    let parent_effect =
        ProgrammaticRuleSelectorV1::effect_profile(vec![ProgrammaticToolEffectFlagV1::LocalRead])
            .expect("effect selector is valid");
    let child_effect = ProgrammaticRuleSelectorV1::effect_profile(vec![
        ProgrammaticToolEffectFlagV1::LocalRead,
        ProgrammaticToolEffectFlagV1::LocalWrite,
    ])
    .expect("effect selector is valid");
    validate_selector_narrowing(&parent_effect, &child_effect)
        .expect("requiring more declared effects narrows the selector");
    assert_eq!(
        rejection_code(validate_selector_narrowing(&child_effect, &parent_effect)),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );
    validate_selector_narrowing(&read_selector(), &read_selector())
        .expect("an equal exact selector stays inside the parent selection");
    let write_selector =
        ProgrammaticRuleSelectorV1::exact_tool("write", 1).expect("write selector is valid");
    assert_eq!(
        rejection_code(validate_selector_narrowing(
            &read_selector(),
            &write_selector
        )),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );
    assert_eq!(
        rejection_code(validate_selector_narrowing(
            &read_selector(),
            &ProgrammaticRuleSelectorV1::exact_tool("read", 5).expect("selector is valid")
        )),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );
}

#[test]
fn effective_snapshot_intersects_policies_with_deterministic_digests() {
    let permissive = applicable_with(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        POLICY_A,
        ProgrammaticAdmissionRuleV1::DirectLocalRead,
        vec![read_rule(ProgrammaticAdmissionRuleV1::DirectLocalRead)],
        ProgrammaticRunLimitsV1::new(200, 10).expect("limits are valid"),
        calendar_with(300),
    );
    let strict = applicable_with(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        POLICY_B,
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        vec![read_rule(
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        )],
        ProgrammaticRunLimitsV1::new(50, 4).expect("limits are valid"),
        calendar_with(100),
    );

    let snapshot = resolve_effective_policy_snapshot(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        &[permissive.clone(), strict.clone()],
    )
    .expect("applicable policies intersect");
    assert_eq!(
        snapshot.decision_ceiling,
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired
    );
    assert_eq!(
        snapshot.run_limits,
        ProgrammaticRunLimitsV1::new(50, 4).expect("narrowed limits are valid")
    );
    assert_eq!(effective_calendar_action_limit(&snapshot), Some(100));
    assert_eq!(
        snapshot.policy_references,
        vec![
            ProgrammaticCallerPolicyRevisionReferenceV1 {
                policy_id: POLICY_A,
                revision: 1,
            },
            ProgrammaticCallerPolicyRevisionReferenceV1 {
                policy_id: POLICY_B,
                revision: 1,
            },
        ]
    );
    assert_eq!(snapshot.scope_provenance.len(), 2);
    assert_eq!(snapshot.ordered_rules.len(), 2);
    assert_eq!(
        snapshot.snapshot_digest,
        effective_policy_snapshot_digest(&snapshot)
    );

    let reordered = resolve_effective_policy_snapshot(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        &[strict, permissive],
    )
    .expect("applicable policies intersect");
    assert_eq!(
        reordered, snapshot,
        "snapshot resolution is deterministic for the same policy set"
    );

    let interactive_only = applicable(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        POLICY_A,
        ProgrammaticAdmissionRuleV1::Prohibited,
        Vec::new(),
    );
    assert_eq!(
        rejection_code(resolve_effective_policy_snapshot(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            &[interactive_only]
        )),
        PROGRAMMATIC_POLICY_SNAPSHOT_UNAVAILABLE,
        "an origin-less applicable set cannot create a harness snapshot"
    );
}

#[test]
fn snapshot_falls_back_to_the_baseline_only_for_policy_less_interactive_roots() {
    let snapshot =
        resolve_effective_policy_snapshot(ProgrammaticRootOriginKindV1::InteractiveUser, &[])
            .expect("policy-less interactive snapshot resolves");
    assert_eq!(
        snapshot.baseline,
        Some(InteractiveLocalReadBaselineV1::v1())
    );
    assert_eq!(
        snapshot.decision_ceiling,
        ProgrammaticAdmissionRuleV1::DirectLocalRead
    );
    assert!(snapshot.policy_references.is_empty());
    assert!(snapshot.calendar_counter_references.is_empty());
    assert_eq!(
        snapshot.run_limits,
        ProgrammaticRunLimitsV1::new(
            INTERACTIVE_LOCAL_READ_BASELINE_ACTIONS,
            INTERACTIVE_LOCAL_READ_BASELINE_CONCURRENT_ACTIONS
        )
        .expect("baseline limits are valid")
    );
    assert_eq!(
        resolve_snapshot_decision(&snapshot, &read_call()),
        ProgrammaticAdmissionRuleV1::DirectLocalRead
    );
    assert_eq!(
        resolve_snapshot_decision(&snapshot, &descendant_call()),
        ProgrammaticAdmissionRuleV1::Prohibited,
        "the baseline never widens to a descendant automatically"
    );

    assert_eq!(
        rejection_code(resolve_effective_policy_snapshot(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            &[]
        )),
        PROGRAMMATIC_POLICY_SNAPSHOT_UNAVAILABLE
    );

    let durable = resolve_effective_policy_snapshot(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        &[applicable(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            POLICY_A,
            ProgrammaticAdmissionRuleV1::DirectLocalRead,
            vec![read_rule(ProgrammaticAdmissionRuleV1::DirectLocalRead)],
        )],
    )
    .expect("durable interactive snapshot resolves");
    assert_eq!(
        durable.baseline,
        Some(InteractiveLocalReadBaselineV1::v1()),
        "a durable policy can only narrow the code-owned baseline"
    );
}

#[test]
fn interactive_baseline_is_256_actions_and_16_concurrent() {
    let baseline = InteractiveLocalReadBaselineV1::v1();
    assert_eq!(baseline.maximum_action_count, 256);
    assert_eq!(baseline.maximum_concurrent_actions, 16);
    baseline.validate().expect("the frozen baseline is valid");

    let altered = InteractiveLocalReadBaselineV1 {
        maximum_action_count: INTERACTIVE_LOCAL_READ_BASELINE_ACTIONS + 1,
        maximum_concurrent_actions: INTERACTIVE_LOCAL_READ_BASELINE_CONCURRENT_ACTIONS,
    };
    assert_eq!(
        rejection_code(altered.validate()),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );

    assert_eq!(
        resolve_interactive_local_read_baseline(&interactive_origin(), true)
            .expect("the interactive root run resolves the baseline"),
        Some(InteractiveLocalReadBaselineV1::v1())
    );
    assert_eq!(
        resolve_interactive_local_read_baseline(&interactive_origin(), false)
            .expect("a descendant has no automatic baseline"),
        None
    );
    assert_eq!(
        rejection_code(resolve_interactive_local_read_baseline(
            &harness_origin(),
            true
        )),
        PROGRAMMATIC_POLICY_ORIGIN_INVALID
    );
}

#[test]
fn snapshot_decision_uses_most_restrictive_intersection_and_closed_default() {
    let permissive = applicable(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        POLICY_A,
        ProgrammaticAdmissionRuleV1::DirectLocalRead,
        vec![read_rule(ProgrammaticAdmissionRuleV1::DirectLocalRead)],
    );
    let snapshot = resolve_effective_policy_snapshot(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        &[permissive],
    )
    .expect("snapshot resolves");
    assert_eq!(
        resolve_snapshot_decision(&snapshot, &read_call()),
        ProgrammaticAdmissionRuleV1::DirectLocalRead
    );

    let execute_call = ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ROOT_SESSION,
            ROOT_RUN,
            ROOT_SESSION,
            ROOT_RUN,
            [15; 16],
        ),
        "execute",
        2,
        vec![ProgrammaticToolEffectFlagV1::LocalExecute],
        None,
        Vec::new(),
        Digest256::sha256(b"execute-input"),
    )
    .expect("execute call is valid");
    assert_eq!(
        resolve_snapshot_decision(&snapshot, &execute_call),
        ProgrammaticAdmissionRuleV1::Prohibited,
        "a call no selected rule covers stays closed by default"
    );

    let strict = applicable(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        POLICY_A,
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        vec![read_rule(ProgrammaticAdmissionRuleV1::DirectLocalRead)],
    );
    let strict_snapshot =
        resolve_effective_policy_snapshot(ProgrammaticRootOriginKindV1::InteractiveUser, &[strict])
            .expect("snapshot resolves");
    assert_eq!(
        resolve_snapshot_decision(&strict_snapshot, &read_call()),
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        "the most restrictive selected decision wins"
    );

    let harness_policy = applicable(
        ProgrammaticRootOriginKindV1::ContinualHarness,
        POLICY_A,
        ProgrammaticAdmissionRuleV1::DirectLocalRead,
        vec![read_rule(ProgrammaticAdmissionRuleV1::DirectLocalRead)],
    );
    let harness_snapshot = resolve_effective_policy_snapshot(
        ProgrammaticRootOriginKindV1::ContinualHarness,
        &[harness_policy],
    )
    .expect("harness snapshot resolves");
    assert_eq!(
        resolve_snapshot_decision(&harness_snapshot, &harness_call()),
        ProgrammaticAdmissionRuleV1::Prohibited,
        "a harness root can never take the interactive direct-read decision"
    );
}

#[test]
fn admission_rule_selectors_constrain_by_effects_and_exact_tools() {
    let effect_rule = entry(
        ProgrammaticAdmissionRuleV1::DirectLocalRead,
        vec![
            ProgrammaticRuleSelectorV1::effect_profile(vec![
                ProgrammaticToolEffectFlagV1::LocalRead,
            ])
            .expect("effect selector is valid"),
        ],
    );
    assert!(effect_rule.matches(&read_call()));
    let write_call = ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ROOT_SESSION,
            ROOT_RUN,
            ROOT_SESSION,
            ROOT_RUN,
            [16; 16],
        ),
        "write",
        3,
        vec![ProgrammaticToolEffectFlagV1::LocalWrite],
        None,
        Vec::new(),
        Digest256::sha256(b"write-input"),
    )
    .expect("write call is valid");
    assert!(
        !effect_rule.matches(&write_call),
        "an effect selector never admits a tool whose declared effects differ"
    );

    let mcp_selector = ProgrammaticRuleSelectorV1::mcp_method([41; 16], "tools/search", "schema-1")
        .expect("mcp selector is valid");
    let mcp_call = ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ROOT_SESSION,
            ROOT_RUN,
            ROOT_SESSION,
            ROOT_RUN,
            [17; 16],
        ),
        "mcp",
        1,
        vec![ProgrammaticToolEffectFlagV1::McpInvocation],
        Some(
            ProgrammaticMcpMethodReferenceV1::new([41; 16], "tools/search", "schema-1")
                .expect("mcp method reference is valid"),
        ),
        Vec::new(),
        Digest256::sha256(b"mcp-input"),
    )
    .expect("mcp call is valid");
    assert!(mcp_selector.matches(&mcp_call));
    assert!(
        !ProgrammaticRuleSelectorV1::mcp_method([42; 16], "tools/search", "schema-1")
            .expect("mcp selector is valid")
            .matches(&mcp_call),
        "an mcp exact selector is bound to its connection and schema revision"
    );

    assert_eq!(
        ProgrammaticAdmissionRuleV1::DirectLocalRead
            .most_restrictive(ProgrammaticAdmissionRuleV1::Prohibited),
        ProgrammaticAdmissionRuleV1::Prohibited
    );
    assert_eq!(
        ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired
            .most_restrictive(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired),
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired
    );
}

#[test]
fn typed_input_constraints_are_closed_and_bounded() {
    let constraint =
        DescriptorInputConstraintSelectionV1::new("closed_query_v1", 1, vec!["alpha".to_owned()])
            .expect("closed constraint is valid");
    assert_eq!(constraint.family, "closed_query_v1");
    assert_eq!(constraint.typed_values, vec!["alpha".to_owned()]);

    assert_eq!(
        rejection_code(DescriptorInputConstraintSelectionV1::new("", 1, Vec::new())),
        PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH
    );
    assert_eq!(
        rejection_code(DescriptorInputConstraintSelectionV1::new(
            "closed_query_v1",
            0,
            Vec::new()
        )),
        PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH
    );
    let over_limit_values = vec!["value".to_owned(); MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE + 1];
    assert_eq!(
        rejection_code(DescriptorInputConstraintSelectionV1::new(
            "closed_query_v1",
            1,
            over_limit_values
        )),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );
    assert_eq!(
        rejection_code(DescriptorInputConstraintSelectionV1::new(
            "closed_query_v1",
            1,
            vec!["raw\nvalue".to_owned()]
        )),
        PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH
    );
}

#[test]
fn direct_local_read_is_interactive_only_and_limited_to_five_tools() {
    for tool_id in ["read", "glob", "grep", "expand", "retrieve"] {
        validate_direct_local_read_tool(ProgrammaticRootOriginKindV1::InteractiveUser, tool_id)
            .expect("the closed direct-local-read tool set is admitted");
    }
    for tool_id in [
        "execute",
        "fetch_url",
        "mcp",
        "write",
        "edit",
        USER_INTERACTION_TOOL_ID,
    ] {
        assert_eq!(
            rejection_code(validate_direct_local_read_tool(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                tool_id
            )),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE,
            "{tool_id} never receives direct admission"
        );
    }
    assert_eq!(
        rejection_code(validate_direct_local_read_tool(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            "read"
        )),
        PROGRAMMATIC_POLICY_ORIGIN_INVALID
    );
}

#[test]
fn bounded_corridors_require_a_descriptor_input_constraint_family() {
    assert_eq!(
        rejection_code(validate_bounded_corridor_admission("execute", true)),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE,
        "execute never participates in a bounded corridor"
    );
    assert_eq!(
        rejection_code(validate_bounded_corridor_admission("execute", false)),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );
    assert_eq!(
        rejection_code(validate_bounded_corridor_admission("fetch_url", false)),
        PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH
    );
    validate_bounded_corridor_admission("fetch_url", true)
        .expect("fetch_url may use a bounded corridor through a descriptor constraint family");
    assert_eq!(
        rejection_code(validate_bounded_corridor_admission("mcp", false)),
        PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH
    );
    validate_bounded_corridor_admission("mcp", true)
        .expect("mcp may use a bounded corridor through one selected method constraint family");
    assert_eq!(
        rejection_code(validate_bounded_corridor_admission("read", false)),
        PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH
    );
    validate_bounded_corridor_admission("read", true)
        .expect("a descriptor-declared constraint family admits the bounded corridor");
}

#[test]
fn exact_confirmations_bind_one_exact_call_and_expire_without_reuse() {
    let snapshot =
        resolve_effective_policy_snapshot(ProgrammaticRootOriginKindV1::InteractiveUser, &[])
            .expect("snapshot resolves");
    let confirmation = ProgrammaticExactConfirmationV1::new(
        [50; 16],
        ProgrammaticConfirmationBindingV1::from_call(
            &read_call(),
            snapshot.snapshot_digest,
            snapshot.run_limits,
        ),
        1_000,
        2_000,
    )
    .expect("exact confirmation is valid");
    assert!(
        confirmation.matches_binding(&ProgrammaticConfirmationBindingV1::from_call(
            &read_call(),
            snapshot.snapshot_digest,
            snapshot.run_limits
        ))
    );
    confirmation
        .authorizes(&read_call(), &snapshot, 1_500)
        .expect("the exact confirmation admits its one bound call");

    let reused = ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ROOT_SESSION,
            ROOT_RUN,
            ROOT_SESSION,
            ROOT_RUN,
            [5; 16],
        ),
        "read",
        4,
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        None,
        Vec::new(),
        Digest256::sha256(b"other-input"),
    )
    .expect("changed call is valid");
    assert_eq!(
        rejection_code(confirmation.authorizes(&reused, &snapshot, 1_500)),
        PROGRAMMATIC_POLICY_CONFIRMATION_REQUIRED,
        "a confirmation never re-binds to another typed input"
    );
    assert_eq!(
        rejection_code(confirmation.authorizes(&read_call(), &snapshot, 2_001)),
        PROGRAMMATIC_POLICY_CONFIRMATION_EXPIRED
    );
    assert_eq!(
        rejection_code(ProgrammaticExactConfirmationV1::new(
            [51; 16],
            ProgrammaticConfirmationBindingV1::from_call(
                &read_call(),
                snapshot.snapshot_digest,
                snapshot.run_limits
            ),
            1_000,
            1_000,
        )),
        PROGRAMMATIC_POLICY_CONFIRMATION_EXPIRED,
        "a confirmation never carries a zero-length validity window"
    );
}

#[test]
fn corridors_are_root_tree_scoped_and_expire_at_run_terminal() {
    let snapshot = baseline_snapshot();
    let corridor = corridor_for(snapshot.snapshot_digest);
    validate_corridor_use(&corridor, &read_call())
        .expect("the root-run call is inside the corridor");
    validate_corridor_use(&corridor, &descendant_call())
        .expect("the corridor reaches every descendant of its one active root tree");

    let sibling = ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ROOT_SESSION,
            [99; 16],
            ROOT_SESSION,
            [99; 16],
            [5; 16],
        ),
        "read",
        4,
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        None,
        Vec::new(),
        Digest256::sha256(b"read-input"),
    )
    .expect("sibling call is valid");
    assert_eq!(
        rejection_code(validate_corridor_use(&corridor, &sibling)),
        PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
        "a corridor never reaches a sibling root tree"
    );
    assert_eq!(
        rejection_code(validate_corridor_use(&corridor, &harness_call())),
        PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE
    );

    validate_corridor_active(true).expect("the corridor expires only at run terminal");
    assert!(corridor.expires_at_run_terminal);
    assert_eq!(
        rejection_code(validate_corridor_active(false)),
        PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE
    );
}

#[test]
fn corridors_only_narrow_for_children_and_never_extend() {
    let snapshot = baseline_snapshot();
    let corridor = corridor_for(snapshot.snapshot_digest);

    let equal = ProgrammaticCorridorSelectorsV1::new(
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        vec![read_selector()],
        Vec::new(),
    )
    .expect("selector set is valid");
    validate_corridor_child_selectors(&corridor, &equal)
        .expect("an equal child selector stays inside the corridor");

    let narrower = ProgrammaticCorridorSelectorsV1::new(
        vec![
            ProgrammaticToolEffectFlagV1::LocalRead,
            ProgrammaticToolEffectFlagV1::LocalWrite,
        ],
        vec![read_selector()],
        Vec::new(),
    )
    .expect("selector set is valid");
    validate_corridor_child_selectors(&corridor, &narrower)
        .expect("a child may require additional declared effects");

    let added_tool = ProgrammaticCorridorSelectorsV1::new(
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        vec![ProgrammaticRuleSelectorV1::exact_tool("write", 1).expect("selector is valid")],
        Vec::new(),
    )
    .expect("selector set is valid");
    assert_eq!(
        rejection_code(validate_corridor_child_selectors(&corridor, &added_tool)),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );

    let changed_revision = ProgrammaticCorridorSelectorsV1::new(
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        vec![ProgrammaticRuleSelectorV1::exact_tool("read", 5).expect("selector is valid")],
        Vec::new(),
    )
    .expect("selector set is valid");
    assert_eq!(
        rejection_code(validate_corridor_child_selectors(
            &corridor,
            &changed_revision
        )),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );

    assert_eq!(
        rejection_code(validate_corridor_extension(&corridor, 9, 2)),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
        "no post-hoc corridor extension exists"
    );
    validate_corridor_extension(&corridor, 8, 2).expect("equal bounds stay valid");
    validate_corridor_extension(&corridor, 4, 1).expect("smaller bounds narrow");
}

#[test]
fn corridor_input_constraints_must_match_the_selected_typed_family() {
    let constraint =
        DescriptorInputConstraintSelectionV1::new("closed_query_v1", 1, vec!["alpha".to_owned()])
            .expect("constraint is valid");
    let snapshot = baseline_snapshot();
    let selectors = ProgrammaticCorridorSelectorsV1::new(
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        vec![read_selector()],
        vec![constraint.clone()],
    )
    .expect("selector set is valid");
    let corridor = corridor_with(snapshot.snapshot_digest, selectors);

    validate_corridor_use(&corridor, &read_call_with_constraints(vec![constraint]))
        .expect("the equal selected typed family is inside the corridor");
    let mismatched =
        DescriptorInputConstraintSelectionV1::new("closed_query_v1", 1, vec!["beta".to_owned()])
            .expect("constraint is valid");
    assert_eq!(
        rejection_code(validate_corridor_use(
            &corridor,
            &read_call_with_constraints(vec![mismatched])
        )),
        PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH
    );
    assert_eq!(
        rejection_code(validate_corridor_use(&corridor, &read_call())),
        PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH,
        "an absent typed selection never satisfies the selected constraint family"
    );
}

#[test]
fn corridor_usage_is_shared_and_exhausts_without_reopening() {
    let snapshot = baseline_snapshot();
    let corridor = corridor_for(snapshot.snapshot_digest);
    let mut usage = ProgrammaticCorridorUsageV1::new(&corridor);
    assert_eq!(usage.remaining_actions, 8);
    assert_eq!(usage.remaining_concurrent_actions, 2);

    usage.reserve(&corridor).expect("first reservation");
    usage.reserve(&corridor).expect("second reservation");
    assert_eq!(usage.remaining_actions, 6);
    assert_eq!(usage.remaining_concurrent_actions, 0);
    assert_eq!(
        rejection_code(usage.reserve(&corridor)),
        PROGRAMMATIC_POLICY_CORRIDOR_EXHAUSTED
    );

    usage
        .release_unstarted(&corridor)
        .expect("known pre-effect failure releases both slots");
    assert_eq!(usage.remaining_actions, 7);
    assert_eq!(usage.remaining_concurrent_actions, 1);
    usage.reserve(&corridor).expect("third reservation");
    usage
        .consume_started(&corridor)
        .expect("started action releases only concurrency");
    assert_eq!(usage.remaining_actions, 6);
    assert_eq!(usage.remaining_concurrent_actions, 1);
    usage
        .consume_started(&corridor)
        .expect("started action releases only concurrency");
    assert_eq!(usage.remaining_concurrent_actions, 2);

    let other = corridor_for(Digest256::sha256(b"other-snapshot"));
    assert_eq!(
        rejection_code(usage.reserve(&other)),
        PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE
    );
}

#[test]
fn policy_lifecycle_is_closed_and_live_tightening_only_narrows() {
    use ProgrammaticCallerPolicyLifecycleStateV1::{Active, Archived, Revoked, Suspended};

    validate_policy_lifecycle_transition(Active, Suspended).expect("suspend is permitted");
    validate_policy_lifecycle_transition(Suspended, Active).expect("resume is permitted");
    validate_policy_lifecycle_transition(Active, Revoked).expect("revoke is permitted");
    validate_policy_lifecycle_transition(Suspended, Revoked).expect("revoke is permitted");
    validate_policy_lifecycle_transition(Revoked, Archived)
        .expect("only a revoked policy may be archived");
    assert_eq!(
        rejection_code(validate_policy_lifecycle_transition(Revoked, Active)),
        PROGRAMMATIC_POLICY_REVOKED,
        "revocation is never undone by reactivating an old revision"
    );
    assert_eq!(
        rejection_code(validate_policy_lifecycle_transition(Archived, Active)),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );
    assert_eq!(
        rejection_code(validate_policy_lifecycle_transition(Active, Archived)),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );
    assert_eq!(
        rejection_code(validate_policy_lifecycle_transition(Suspended, Archived)),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );
    assert_eq!(
        rejection_code(validate_policy_lifecycle_transition(Active, Active)),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );

    assert_eq!(
        rejection_code(validate_live_policy_state(Suspended, false)),
        PROGRAMMATIC_POLICY_SUSPENDED
    );
    validate_live_policy_state(Suspended, true)
        .expect("a started action is never blocked by a later live suspension");
    assert_eq!(
        rejection_code(validate_live_policy_state(Revoked, false)),
        PROGRAMMATIC_POLICY_REVOKED
    );
    validate_live_policy_state(Revoked, true)
        .expect("a started action keeps its independently selected recovery evidence");
    assert_eq!(
        rejection_code(validate_live_policy_state(Archived, false)),
        PROGRAMMATIC_POLICY_NOT_APPLICABLE
    );
    validate_live_policy_state(Active, false).expect("an active policy admits");

    assert_eq!(
        tighten_admission_rule(
            ProgrammaticAdmissionRuleV1::DirectLocalRead,
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired
        )
        .expect("live tightening narrows without a restart"),
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired
    );
    assert_eq!(
        tighten_admission_rule(
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired
        )
        .expect("a repeated tightening is equal, not weaker"),
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired
    );
    assert_eq!(
        rejection_code(tighten_admission_rule(
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
            ProgrammaticAdmissionRuleV1::DirectLocalRead
        )),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
    );
}

#[test]
fn one_pending_draft_per_scope_coalesces_equal_proposals() {
    let project = ProgrammaticCallerPolicyScopeV1::Project {
        project_id: PROJECT,
    };
    let pending = ProgrammaticCallerPolicyDraftV1::new(
        [80; 16],
        project,
        None,
        draft_content(),
        "tighten reads after a policy denial",
    )
    .expect("draft is valid");
    let equal = ProgrammaticCallerPolicyDraftV1::new(
        [81; 16],
        project,
        None,
        draft_content(),
        "tighten reads after a policy denial",
    )
    .expect("draft is valid");
    assert_eq!(pending.scope, project);
    assert_eq!(
        propose_policy_draft(None, &pending).expect("the first proposal becomes pending"),
        ProgrammaticDraftProposalOutcomeV1::Pending
    );
    assert_eq!(
        propose_policy_draft(Some(&pending), &equal).expect("an equal proposal coalesces evidence"),
        ProgrammaticDraftProposalOutcomeV1::Coalesced
    );

    let conflicting = ProgrammaticCallerPolicyDraftV1::new(
        [82; 16],
        project,
        None,
        draft_content_with(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired),
        "tighten reads after a policy denial",
    )
    .expect("draft is valid");
    assert_eq!(
        rejection_code(propose_policy_draft(Some(&pending), &conflicting)),
        PROGRAMMATIC_POLICY_DRAFT_CONFLICT
    );

    let other_scope = ProgrammaticCallerPolicyDraftV1::new(
        [83; 16],
        ProgrammaticCallerPolicyScopeV1::Goal {
            project_id: PROJECT,
            goal_id: [9; 16],
        },
        None,
        draft_content(),
        "tighten reads after a policy denial",
    )
    .expect("draft is valid");
    assert_eq!(
        propose_policy_draft(Some(&pending), &other_scope)
            .expect("another owner scope keeps its own one pending draft"),
        ProgrammaticDraftProposalOutcomeV1::Pending
    );
    assert_eq!(
        rejection_code(validate_draft_size(MAX_POLICY_RECORD_BYTES + 1)),
        PROGRAMMATIC_POLICY_DRAFT_TOO_LARGE
    );
}

#[test]
fn draft_decisions_create_or_reject_only_against_the_exact_base_state() {
    assert_eq!(
        apply_policy_draft_decision(
            ProgrammaticPolicyDraftDecisionV1::Reject,
            Some((POLICY_A, 1)),
            Some(2),
        )
        .expect("rejection always succeeds and changes no policy"),
        ProgrammaticPolicyDraftOutcomeV1::NoPolicyChange
    );
    assert_eq!(
        apply_policy_draft_decision(ProgrammaticPolicyDraftDecisionV1::Accept, None, None)
            .expect("acceptance without an existing policy creates one"),
        ProgrammaticPolicyDraftOutcomeV1::PolicyCreated
    );
    assert_eq!(
        apply_policy_draft_decision(ProgrammaticPolicyDraftDecisionV1::EditAndAccept, None, None)
            .expect("edit-and-accept without an existing policy creates one"),
        ProgrammaticPolicyDraftOutcomeV1::PolicyCreated
    );
    assert_eq!(
        apply_policy_draft_decision(
            ProgrammaticPolicyDraftDecisionV1::Accept,
            Some((POLICY_A, 1)),
            Some(1)
        )
        .expect("acceptance against the exact base revision creates a revision"),
        ProgrammaticPolicyDraftOutcomeV1::RevisionCreated
    );
    assert_eq!(
        rejection_code(apply_policy_draft_decision(
            ProgrammaticPolicyDraftDecisionV1::Accept,
            Some((POLICY_A, 1)),
            Some(2),
        )),
        PROGRAMMATIC_POLICY_REVISION_CONFLICT,
        "acceptance never silently substitutes a different base revision"
    );
}

#[test]
fn reservations_reserve_before_start_release_before_effect_and_become_permanent_on_start() {
    let mut state = ProgrammaticLimitStateV1::new(
        ProgrammaticRunLimitsV1::new(4, 2).expect("run limits are valid"),
        8,
    );
    let binding = reserve_binding([60; 16], &Digest256::sha256(b"input"));
    let first = reserve_limit_action(&mut state, binding.clone(), &[], [70; 16], [71; 16], 1_000)
        .expect("the first reservation fits");
    assert_eq!(state.run_reserved_actions, 1);
    assert_eq!(state.calendar_reserved_actions, 1);
    assert_eq!(state.run_in_flight_actions, 0);

    let replay = reserve_limit_action(
        &mut state,
        binding,
        std::slice::from_ref(&first),
        [99; 16],
        [98; 16],
        1_100,
    )
    .expect("an equal repeated operation reads the accepted binding");
    assert_eq!(replay, first);
    assert_eq!(
        state.run_reserved_actions, 1,
        "an idempotent equal replay never reserves a second unit"
    );

    commit_reservation_started(&mut state, &first).expect("start consumes the reservation");
    assert_eq!(state.run_reserved_actions, 0);
    assert_eq!(state.run_started_actions, 1);
    assert_eq!(state.run_in_flight_actions, 1);
    assert_eq!(state.calendar_reserved_actions, 0);
    assert_eq!(state.calendar_started_actions, 1);

    finish_started_action(&mut state, &first)
        .expect("terminal start evidence releases concurrency");
    assert_eq!(state.run_in_flight_actions, 0);
    assert_eq!(state.run_started_actions, 1);

    let mut released = ProgrammaticLimitStateV1::new(
        ProgrammaticRunLimitsV1::new(4, 2).expect("run limits are valid"),
        8,
    );
    let pending = reserve_limit_action(
        &mut released,
        reserve_binding([61; 16], &Digest256::sha256(b"release")),
        &[],
        [72; 16],
        [73; 16],
        1_200,
    )
    .expect("reservation fits");
    release_unstarted_reservation(&mut released, &pending)
        .expect("a known pre-effect failure releases each reservation atomically");
    assert_eq!(released.run_reserved_actions, 0);
    assert_eq!(released.calendar_reserved_actions, 0);
    assert_eq!(released.run_started_actions, 0);
    assert_eq!(released.calendar_started_actions, 0);
}

#[test]
fn reservation_limits_and_counter_unavailability_reject_before_effect() {
    let mut run_limited = ProgrammaticLimitStateV1::new(
        ProgrammaticRunLimitsV1::new(1, 1).expect("run limits are valid"),
        4,
    );
    let first = reserve_limit_action(
        &mut run_limited,
        reserve_binding([62; 16], &Digest256::sha256(b"one")),
        &[],
        [74; 16],
        [75; 16],
        1_000,
    )
    .expect("first reservation fits");
    assert_eq!(
        rejection_code(reserve_limit_action(
            &mut run_limited,
            reserve_binding([63; 16], &Digest256::sha256(b"two")),
            std::slice::from_ref(&first),
            [76; 16],
            [77; 16],
            1_001,
        )),
        PROGRAMMATIC_POLICY_RUN_LIMIT_EXCEEDED
    );

    let mut calendar_limited = ProgrammaticLimitStateV1::new(
        ProgrammaticRunLimitsV1::new(8, 4).expect("run limits are valid"),
        1,
    );
    let calendar_first = reserve_limit_action(
        &mut calendar_limited,
        reserve_binding([64; 16], &Digest256::sha256(b"three")),
        &[],
        [78; 16],
        [79; 16],
        1_000,
    )
    .expect("first calendar reservation fits");
    assert_eq!(
        rejection_code(reserve_limit_action(
            &mut calendar_limited,
            reserve_binding([65; 16], &Digest256::sha256(b"four")),
            std::slice::from_ref(&calendar_first),
            [80; 16],
            [81; 16],
            1_001,
        )),
        PROGRAMMATIC_POLICY_CALENDAR_LIMIT_EXCEEDED
    );

    assert!(validate_counter_available(Some(&run_limited)).is_ok());
    assert_eq!(
        rejection_code(validate_counter_available(None)),
        PROGRAMMATIC_POLICY_COUNTER_UNAVAILABLE
    );

    assert_eq!(
        rejection_code(reserve_limit_action(
            &mut run_limited,
            reserve_binding([62; 16], &Digest256::sha256(b"different-input")),
            std::slice::from_ref(&first),
            [82; 16],
            [83; 16],
            1_002,
        )),
        PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
        "one tool call id never re-binds to a different typed input"
    );

    let mut fresh = ProgrammaticLimitStateV1::new(
        ProgrammaticRunLimitsV1::new(4, 2).expect("run limits are valid"),
        4,
    );
    assert_eq!(
        rejection_code(release_unstarted_reservation(&mut fresh, &first)),
        PROGRAMMATIC_POLICY_RESERVATION_CONFLICT
    );
}

#[test]
fn recovery_releases_unstarted_work_and_never_retries_started_effects() {
    let mut state = ProgrammaticLimitStateV1::new(
        ProgrammaticRunLimitsV1::new(4, 2).expect("run limits are valid"),
        4,
    );
    let unstarted = reserve_limit_action(
        &mut state,
        reserve_binding([66; 16], &Digest256::sha256(b"recover-unstarted")),
        &[],
        [84; 16],
        [85; 16],
        1_000,
    )
    .expect("reservation fits");
    assert_eq!(
        recovery_disposition_for(false),
        ProgrammaticRecoveryDispositionV1::InterruptedBeforeStart
    );
    assert_eq!(
        apply_recovery_disposition(&mut state, &unstarted, false)
            .expect("unstarted work releases its reservation"),
        ProgrammaticRecoveryDispositionV1::InterruptedBeforeStart
    );
    assert_eq!(state.run_reserved_actions, 0);
    assert_eq!(state.calendar_reserved_actions, 0);

    let started = reserve_limit_action(
        &mut state,
        reserve_binding([67; 16], &Digest256::sha256(b"recover-started")),
        &[],
        [86; 16],
        [87; 16],
        1_100,
    )
    .expect("reservation fits");
    commit_reservation_started(&mut state, &started).expect("start is permanent");
    assert_eq!(
        recovery_disposition_for(true),
        ProgrammaticRecoveryDispositionV1::ExternalEffectUnknown
    );
    assert_eq!(
        apply_recovery_disposition(&mut state, &started, true)
            .expect("a started ambiguous action keeps its permanent consumption"),
        ProgrammaticRecoveryDispositionV1::ExternalEffectUnknown
    );
    assert_eq!(state.run_started_actions, 1);
    assert_eq!(state.calendar_started_actions, 1);
    assert_eq!(
        rejection_code(release_unstarted_reservation(&mut state, &unstarted)),
        PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
        "recovery never recreates or releases an already consumed reservation"
    );
}

#[test]
fn root_only_interaction_and_harness_delegation_stay_closed() {
    validate_interaction_admission(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        USER_INTERACTION_TOOL_ID,
        true,
    )
    .expect("only the root run starts ask_user");
    assert_eq!(
        rejection_code(validate_interaction_admission(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            USER_INTERACTION_TOOL_ID,
            false
        )),
        PROGRAMMATIC_POLICY_ROOT_ONLY_INTERACTION
    );
    assert_eq!(
        rejection_code(validate_interaction_admission(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            USER_INTERACTION_TOOL_ID,
            true
        )),
        PROGRAMMATIC_POLICY_HARNESS_DELEGATION_FORBIDDEN,
        "a harness never calls ask_user"
    );
    validate_interaction_admission(ProgrammaticRootOriginKindV1::InteractiveUser, "read", false)
        .expect("the root-only rule applies only to the interaction tool");

    assert_eq!(
        rejection_code(validate_harness_delegation(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            HARNESS_DELEGATION_TOOL_ID,
            false
        )),
        PROGRAMMATIC_POLICY_HARNESS_DELEGATION_FORBIDDEN,
        "harness child admission must fit the user-approved corridor"
    );
    validate_harness_delegation(
        ProgrammaticRootOriginKindV1::ContinualHarness,
        HARNESS_DELEGATION_TOOL_ID,
        true,
    )
    .expect("a corridor-selected harness sub_agent is admitted");
    validate_harness_delegation(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        HARNESS_DELEGATION_TOOL_ID,
        false,
    )
    .expect("CON-071 keeps separately issued ordinary delegation ungated");
}

#[test]
fn per_scope_bounds_and_fork_inheritance_keep_counters_shared() {
    validate_effective_policy_snapshot_size(MAX_EFFECTIVE_POLICY_SNAPSHOT_BYTES)
        .expect("the effective snapshot bound is inclusive");
    assert_eq!(
        rejection_code(validate_effective_policy_snapshot_size(
            MAX_EFFECTIVE_POLICY_SNAPSHOT_BYTES + 1
        )),
        PROGRAMMATIC_POLICY_SNAPSHOT_TOO_LARGE
    );
    validate_policy_record_size(MAX_POLICY_RECORD_BYTES).expect("the record bound is inclusive");
    validate_policies_in_project(MAX_POLICIES_PER_PROJECT).expect("the project bound is inclusive");
    assert_eq!(
        rejection_code(validate_policies_in_project(MAX_POLICIES_PER_PROJECT + 1)),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );
    validate_policies_attached_to_goal(MAX_POLICIES_PER_GOAL).expect("the goal bound is inclusive");
    assert_eq!(
        rejection_code(validate_policies_attached_to_goal(
            MAX_POLICIES_PER_GOAL + 1
        )),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );
    validate_session_policy_references(MAX_POLICY_REFERENCES_PER_SESSION)
        .expect("the session reference bound is inclusive");
    assert_eq!(
        rejection_code(validate_session_policy_references(
            MAX_POLICY_REFERENCES_PER_SESSION + 1
        )),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );
    validate_unfinished_corridors_per_root_tree(MAX_UNFINISHED_CORRIDORS_PER_ROOT_TREE)
        .expect("the unfinished corridor bound is inclusive");
    assert_eq!(
        rejection_code(validate_unfinished_corridors_per_root_tree(
            MAX_UNFINISHED_CORRIDORS_PER_ROOT_TREE + 1
        )),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );
    validate_awaiting_confirmations_per_root_tree(MAX_AWAITING_CONFIRMATIONS_PER_ROOT_TREE)
        .expect("the awaiting confirmation bound is inclusive");
    assert_eq!(
        rejection_code(validate_awaiting_confirmations_per_root_tree(
            MAX_AWAITING_CONFIRMATIONS_PER_ROOT_TREE + 1
        )),
        PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
    );

    let source = vec![ProgrammaticCallerPolicyRevisionReferenceV1 {
        policy_id: POLICY_A,
        revision: 1,
    }];
    validate_fork_policy_inheritance(&source, &source)
        .expect("a fork keeps the immutable source references");
    let substituted = vec![ProgrammaticCallerPolicyRevisionReferenceV1 {
        policy_id: POLICY_B,
        revision: 1,
    }];
    assert_eq!(
        rejection_code(validate_fork_policy_inheritance(&source, &substituted)),
        PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
        "a fork never substitutes a copied policy identity for a fresh allowance"
    );

    let policy = applicable(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        POLICY_A,
        ProgrammaticAdmissionRuleV1::DirectLocalRead,
        vec![read_rule(ProgrammaticAdmissionRuleV1::DirectLocalRead)],
    );
    let source_snapshot = resolve_effective_policy_snapshot(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        std::slice::from_ref(&policy),
    )
    .expect("source snapshot resolves");
    let fork_snapshot = resolve_effective_policy_snapshot(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        std::slice::from_ref(&policy),
    )
    .expect("fork snapshot resolves");
    assert_eq!(
        source_snapshot.calendar_counter_references, fork_snapshot.calendar_counter_references,
        "the counter identity belongs to the policy id, never to one session"
    );
    assert_eq!(
        source_snapshot.calendar_counter_references[0].policy_id,
        POLICY_A
    );
}

#[test]
fn policy_identity_records_calendar_period_and_exposes_a_deterministic_digest() {
    let policy = ProgrammaticCallerPolicyV1::new(
        POLICY_A,
        ProgrammaticCallerPolicyScopeV1::Project {
            project_id: PROJECT,
        },
        ProgrammaticCalendarPeriodKindV1::Day,
        1,
    )
    .expect("policy identity is valid");
    assert_eq!(policy.policy_id, POLICY_A);
    assert_eq!(
        policy.lifecycle_state,
        ProgrammaticCallerPolicyLifecycleStateV1::Active
    );
    assert_eq!(policy.active_revision, 1);
    assert_eq!(
        policy.canonical_policy_digest,
        policy_identity_digest(
            POLICY_A,
            &ProgrammaticCallerPolicyScopeV1::Project {
                project_id: PROJECT
            },
            ProgrammaticCalendarPeriodKindV1::Day,
            1
        )
    );
    let monthly = ProgrammaticCallerPolicyV1::new(
        POLICY_A,
        ProgrammaticCallerPolicyScopeV1::Project {
            project_id: PROJECT,
        },
        ProgrammaticCalendarPeriodKindV1::Month,
        1,
    )
    .expect("policy identity is valid");
    assert_ne!(
        policy.canonical_policy_digest, monthly.canonical_policy_digest,
        "the immutable calendar period kind participates in policy identity"
    );
    assert_eq!(
        rejection_code(ProgrammaticCallerPolicyV1::new(
            POLICY_A,
            ProgrammaticCallerPolicyScopeV1::Project {
                project_id: PROJECT
            },
            ProgrammaticCalendarPeriodKindV1::Day,
            0,
        )),
        PROGRAMMATIC_POLICY_REVISION_CONFLICT
    );
}

#[test]
fn closed_failure_set_is_reachable_from_pre_effect_rejections() {
    let mut codes = BTreeSet::new();

    // programmatic_policy_limit_exceeded
    codes.insert(rejection_code(validate_policies_in_project(
        MAX_POLICIES_PER_PROJECT + 1,
    )));
    // programmatic_policy_snapshot_too_large
    codes.insert(rejection_code(validate_effective_policy_snapshot_size(
        MAX_EFFECTIVE_POLICY_SNAPSHOT_BYTES + 1,
    )));
    // programmatic_policy_snapshot_unavailable
    codes.insert(rejection_code(resolve_effective_policy_snapshot(
        ProgrammaticRootOriginKindV1::ContinualHarness,
        &[],
    )));
    // programmatic_policy_revision_conflict
    codes.insert(rejection_code(ProgrammaticCallerPolicyRevisionV1::new(
        POLICY_A,
        0,
        vec![ProgrammaticRootOriginRuleV1::new(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ProgrammaticAdmissionRuleV1::DirectLocalRead,
        )],
        Vec::new(),
        run_limits(),
        calendar(),
        Vec::new(),
    )));
    // programmatic_policy_inheritance_widening_forbidden
    codes.insert(rejection_code(validate_revision_narrowing(
        run_limits(),
        &calendar(),
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        ProgrammaticRunLimitsV1::new(
            MAX_POLICY_ACTIONS_PER_RUN,
            MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN,
        )
        .expect("maximum run limits are valid"),
        &calendar(),
        ProgrammaticAdmissionRuleV1::DirectLocalRead,
    )));
    // programmatic_policy_origin_invalid
    codes.insert(rejection_code(new_provenance(
        interactive_origin(),
        &read_call(),
        links(vec![[30; 16]], None, None),
        [20; 16],
        [21; 16],
        Vec::new(),
    )));
    // programmatic_policy_not_applicable
    codes.insert(rejection_code(validate_direct_local_read_tool(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        "execute",
    )));
    // programmatic_policy_suspended
    codes.insert(rejection_code(validate_live_policy_state(
        ProgrammaticCallerPolicyLifecycleStateV1::Suspended,
        false,
    )));
    // programmatic_policy_revoked
    codes.insert(rejection_code(validate_live_policy_state(
        ProgrammaticCallerPolicyLifecycleStateV1::Revoked,
        false,
    )));

    let snapshot =
        resolve_effective_policy_snapshot(ProgrammaticRootOriginKindV1::InteractiveUser, &[])
            .expect("policy-less interactive snapshot resolves");
    let confirmation = ProgrammaticExactConfirmationV1::new(
        [50; 16],
        ProgrammaticConfirmationBindingV1::from_call(
            &read_call(),
            snapshot.snapshot_digest,
            snapshot.run_limits,
        ),
        1_000,
        2_000,
    )
    .expect("exact confirmation is valid");
    // programmatic_policy_confirmation_required
    let changed_call = ProgrammaticAdmissionCallV1::new(
        context(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            ROOT_SESSION,
            ROOT_RUN,
            ROOT_SESSION,
            ROOT_RUN,
            [5; 16],
        ),
        "read",
        4,
        vec![ProgrammaticToolEffectFlagV1::LocalRead],
        None,
        Vec::new(),
        Digest256::sha256(b"changed-input"),
    )
    .expect("changed call is valid");
    codes.insert(rejection_code(confirmation.authorizes(
        &changed_call,
        &snapshot,
        1_500,
    )));
    // programmatic_policy_confirmation_expired
    codes.insert(rejection_code(confirmation.authorizes(
        &read_call(),
        &snapshot,
        2_001,
    )));
    // programmatic_policy_root_only_interaction
    codes.insert(rejection_code(validate_interaction_admission(
        ProgrammaticRootOriginKindV1::InteractiveUser,
        USER_INTERACTION_TOOL_ID,
        false,
    )));
    // programmatic_policy_corridor_unavailable
    codes.insert(rejection_code(validate_corridor_active(false)));
    // programmatic_policy_corridor_exhausted
    let corridor = corridor_for(snapshot.snapshot_digest);
    let mut usage = ProgrammaticCorridorUsageV1::new(&corridor);
    for _ in 0..2 {
        usage
            .reserve(&corridor)
            .expect("corridor reservation stays inside its bound");
    }
    codes.insert(rejection_code(usage.reserve(&corridor)));
    // programmatic_policy_input_constraint_mismatch
    codes.insert(rejection_code(validate_bounded_corridor_admission(
        "mcp", false,
    )));
    // programmatic_policy_run_limit_exceeded
    let mut short_run = ProgrammaticLimitStateV1::new(
        ProgrammaticRunLimitsV1::new(1, 1).expect("run limits are valid"),
        4,
    );
    let first = reserve_limit_action(
        &mut short_run,
        reserve_binding([60; 16], &Digest256::sha256(b"first")),
        &[],
        [70; 16],
        [71; 16],
        1_000,
    )
    .expect("first reservation fits");
    codes.insert(rejection_code(reserve_limit_action(
        &mut short_run,
        reserve_binding([61; 16], &Digest256::sha256(b"second")),
        std::slice::from_ref(&first),
        [72; 16],
        [73; 16],
        1_001,
    )));
    // programmatic_policy_calendar_limit_exceeded
    let mut short_calendar = ProgrammaticLimitStateV1::new(
        ProgrammaticRunLimitsV1::new(8, 4).expect("run limits are valid"),
        1,
    );
    let calendar_first = reserve_limit_action(
        &mut short_calendar,
        reserve_binding([62; 16], &Digest256::sha256(b"third")),
        &[],
        [74; 16],
        [75; 16],
        1_000,
    )
    .expect("first calendar reservation fits");
    codes.insert(rejection_code(reserve_limit_action(
        &mut short_calendar,
        reserve_binding([63; 16], &Digest256::sha256(b"fourth")),
        std::slice::from_ref(&calendar_first),
        [76; 16],
        [77; 16],
        1_001,
    )));
    // programmatic_policy_counter_unavailable
    codes.insert(rejection_code(validate_counter_available(None)));
    // programmatic_policy_reservation_conflict
    codes.insert(rejection_code(reserve_limit_action(
        &mut short_run,
        reserve_binding([60; 16], &Digest256::sha256(b"different")),
        std::slice::from_ref(&first),
        [78; 16],
        [79; 16],
        1_002,
    )));
    // programmatic_policy_harness_delegation_forbidden
    codes.insert(rejection_code(validate_harness_delegation(
        ProgrammaticRootOriginKindV1::ContinualHarness,
        HARNESS_DELEGATION_TOOL_ID,
        false,
    )));
    // programmatic_policy_draft_conflict
    let project = ProgrammaticCallerPolicyScopeV1::Project {
        project_id: PROJECT,
    };
    let pending = ProgrammaticCallerPolicyDraftV1::new(
        [80; 16],
        project,
        None,
        draft_content(),
        "tighten reads",
    )
    .expect("draft is valid");
    let conflicting = ProgrammaticCallerPolicyDraftV1::new(
        [81; 16],
        project,
        None,
        draft_content_with(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired),
        "tighten reads",
    )
    .expect("draft is valid");
    codes.insert(rejection_code(propose_policy_draft(
        Some(&pending),
        &conflicting,
    )));
    // programmatic_policy_draft_too_large
    codes.insert(rejection_code(validate_draft_size(
        MAX_POLICY_RECORD_BYTES + 1,
    )));

    let expected: BTreeSet<String> = PROGRAMMATIC_POLICY_FAILURE_CODES
        .iter()
        .map(|code| (*code).to_owned())
        .collect();
    assert_eq!(
        codes, expected,
        "every closed failure code is reachable as a typed pre-effect rejection"
    );
}
