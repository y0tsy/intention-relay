#![allow(
    clippy::expect_used,
    reason = "M5+ harness domain fixtures use expect for precise test diagnostics."
)]

//! Slice 3 continual-harness domain contract tests.
//!
//! Owner: architecture 26 with ADR 0030 and the ADR 0035 slice sequence. Every
//! test names the architecture 26 required-evidence row it satisfies.

use intention_domain::canonical::Digest256;
use intention_domain::harness::{
    HARNESS_ARCHIVED, HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED, HARNESS_CHECKPOINT_TOO_LARGE,
    HARNESS_CHECKPOINT_UNAVAILABLE, HARNESS_CLOSED_SAFE_FAILURES,
    HARNESS_CONCURRENCY_LIMIT_EXCEEDED, HARNESS_DOSSIER_TOO_LARGE, HARNESS_INTERVAL_TOO_SHORT,
    HARNESS_MAX_DOSSIER_REFERENCES, HARNESS_MAX_RULES, HARNESS_MAX_SOURCES, HARNESS_MAX_SUCCESSORS,
    HARNESS_MIN_INTERVAL_MS, HARNESS_NOT_ACTIVE, HARNESS_RESULT_TOO_LARGE,
    HARNESS_REVISION_CONFLICT, HARNESS_RULE_LIMIT_EXCEEDED, HARNESS_SCHEDULE_INVALID,
    HARNESS_SOURCE_LIMIT_EXCEEDED, HARNESS_SOURCE_UNAVAILABLE, HARNESS_TRIGGER_CYCLE,
    HarnessAdmissionCapacityV1, HarnessCalendarScheduleV1, HarnessClientConnectionV1,
    HarnessCompletionLinkV1, HarnessConclusionV1, HarnessDisconnectContractV1, HarnessDossierV1,
    HarnessDstTransitionV1, HarnessIntervalScheduleV1, HarnessLaunchOriginV1,
    HarnessLocalTimeResolutionV1, HarnessPresentationModeV1, HarnessReadDelegateToolV1,
    HarnessRuleLifecycleStateV1, HarnessRuleOperationV1, HarnessRuleRevisionV1, HarnessRuleScopeV1,
    HarnessRuleSourceV1, HarnessRuleV1, HarnessRunOutcomeV1, HarnessSubAgentCorridorV1,
    HarnessTaskModeV1, HarnessTerminalOutcomeV1, HarnessTriggerCaptureOutcomeV1,
    HarnessTriggerObservationV1, HarnessTriggerRecordV1, HarnessVerifiedCheckpointV1,
    INVALID_HARNESS_CLASS_RESOLUTION, INVALID_HARNESS_DISCONNECT_CONTRACT, admit_harness_trigger,
    applied_harness_time_zone, apply_harness_rule_operation, capture_harness_catch_up,
    capture_harness_trigger, harness_class_rank, plan_harness_goal_continuation,
    resolve_harness_class, resolve_harness_local_time, revise_harness_rule,
    validate_harness_cause_depth, validate_harness_checkpoint_bytes,
    validate_harness_class_narrowing, validate_harness_completion_link,
    validate_harness_conclusion_bytes, validate_harness_concurrency_count,
    validate_harness_disconnect_contract, validate_harness_dossier_bytes,
    validate_harness_interval_ms, validate_harness_narrowed_tools, validate_harness_rule_count,
    validate_harness_rule_operation, validate_harness_source_available,
    validate_harness_source_count, validate_harness_successor_count, validate_harness_time_zone,
    validate_harness_total_launches, verified_checkpoint_after_run,
};
use intention_domain::slice3_selections::{
    HARNESS_MAX_CAUSE_DEPTH, HARNESS_MAX_CHECKPOINT_BYTES, HARNESS_MAX_CONCLUSION_BYTES,
    HARNESS_MAX_CONCURRENT, HARNESS_MAX_DOSSIER_BYTES, HARNESS_MAX_NARROWED_TOOLS,
    HARNESS_MAX_TOTAL_LAUNCHES, HarnessExecutionClassV1, HarnessSourceKindV1,
    HarnessTriggerReasonV1,
};
use intention_types::DtoResult;

/// Evidence row 1: rule lifecycle, revision immutability, pause/resume/archive,
/// and archive-rejected-while-active fixtures.
#[test]
fn rule_lifecycle_covers_create_pause_resume_archive_and_active_run_rejection() {
    let rule = rule_one();
    assert_eq!(rule.lifecycle_state(), HarnessRuleLifecycleStateV1::Active);
    assert!(rule.lifecycle_state().is_active());
    assert!(rule.lifecycle_state().permits_automatic_launch());

    let paused = apply_harness_rule_operation(&rule, false, HarnessRuleOperationV1::Pause)
        .expect("pause is permitted while active");
    assert_eq!(
        paused.lifecycle_state(),
        HarnessRuleLifecycleStateV1::Paused
    );
    assert!(!paused.lifecycle_state().permits_automatic_launch());
    assert!(!paused.lifecycle_state().is_archived());

    let resumed = apply_harness_rule_operation(&paused, false, HarnessRuleOperationV1::Resume)
        .expect("resume is permitted while paused");
    assert_eq!(
        resumed.lifecycle_state(),
        HarnessRuleLifecycleStateV1::Active
    );

    let archived = apply_harness_rule_operation(&resumed, false, HarnessRuleOperationV1::Archive)
        .expect("archive is permitted while no run is active");
    assert!(archived.lifecycle_state().is_archived());
    assert_eq!(
        rejection_code(apply_harness_rule_operation(
            &archived,
            false,
            HarnessRuleOperationV1::Resume
        )),
        HARNESS_ARCHIVED,
        "archived retention rejects every operation in this scope"
    );

    assert_eq!(
        rejection_code(apply_harness_rule_operation(
            &rule,
            true,
            HarnessRuleOperationV1::Archive
        )),
        HARNESS_NOT_ACTIVE,
        "archiving is rejected while the harness has an active run"
    );
    let cancelling =
        apply_harness_rule_operation(&rule, true, HarnessRuleOperationV1::CancelActiveRun)
            .expect("cancellation is permitted with an active run");
    let archived_after_cancel =
        apply_harness_rule_operation(&cancelling, false, HarnessRuleOperationV1::Archive)
            .expect("archive follows the ordinary two-step cancellation path");
    assert!(archived_after_cancel.lifecycle_state().is_archived());
}

/// Evidence row 1: every operation against a non-active or archived rule fails
/// with the closed `harness_not_active` or `harness_archived` code.
#[test]
fn rule_operations_reject_non_active_and_archived_states_with_closed_codes() {
    assert_eq!(
        rejection_code(apply_harness_rule_operation(
            &rule_in_state(HarnessRuleLifecycleStateV1::Paused),
            false,
            HarnessRuleOperationV1::Pause
        )),
        HARNESS_NOT_ACTIVE
    );
    assert_eq!(
        rejection_code(apply_harness_rule_operation(
            &rule_one(),
            false,
            HarnessRuleOperationV1::Resume
        )),
        HARNESS_NOT_ACTIVE
    );
    assert_eq!(
        rejection_code(apply_harness_rule_operation(
            &rule_one(),
            false,
            HarnessRuleOperationV1::CancelActiveRun
        )),
        HARNESS_NOT_ACTIVE
    );
    assert!(
        validate_harness_rule_operation(
            HarnessRuleLifecycleStateV1::Active,
            false,
            HarnessRuleOperationV1::ExplicitLaunch
        )
        .is_ok()
    );
    assert!(
        validate_harness_rule_operation(
            HarnessRuleLifecycleStateV1::Paused,
            false,
            HarnessRuleOperationV1::ExplicitLaunch
        )
        .is_ok()
    );
    assert!(
        validate_harness_rule_operation(
            HarnessRuleLifecycleStateV1::Paused,
            true,
            HarnessRuleOperationV1::CancelActiveRun
        )
        .is_ok()
    );
    assert!(
        validate_harness_rule_operation(
            HarnessRuleLifecycleStateV1::Paused,
            false,
            HarnessRuleOperationV1::UpdateRevision
        )
        .is_ok()
    );

    for operation in [
        HarnessRuleOperationV1::UpdateRevision,
        HarnessRuleOperationV1::Pause,
        HarnessRuleOperationV1::Resume,
        HarnessRuleOperationV1::ExplicitLaunch,
        HarnessRuleOperationV1::CancelActiveRun,
        HarnessRuleOperationV1::Archive,
    ] {
        assert_eq!(
            rejection_code(validate_harness_rule_operation(
                HarnessRuleLifecycleStateV1::Archived,
                false,
                operation
            )),
            HARNESS_ARCHIVED,
            "{operation:?} on an archived rule must be rejected"
        );
    }
}

/// Evidence row 1: the closed operation table is total over states, run
/// presence, and the six typed operations.
#[test]
fn rule_operation_validation_is_total_over_the_closed_state_and_operation_sets() {
    for state in [
        HarnessRuleLifecycleStateV1::Active,
        HarnessRuleLifecycleStateV1::Paused,
        HarnessRuleLifecycleStateV1::Archived,
    ] {
        for has_active_run in [false, true] {
            for operation in [
                HarnessRuleOperationV1::UpdateRevision,
                HarnessRuleOperationV1::Pause,
                HarnessRuleOperationV1::Resume,
                HarnessRuleOperationV1::ExplicitLaunch,
                HarnessRuleOperationV1::CancelActiveRun,
                HarnessRuleOperationV1::Archive,
            ] {
                let permitted = match operation {
                    HarnessRuleOperationV1::UpdateRevision
                    | HarnessRuleOperationV1::ExplicitLaunch => true,
                    HarnessRuleOperationV1::Pause => state.is_active(),
                    HarnessRuleOperationV1::Resume => !state.is_active(),
                    HarnessRuleOperationV1::CancelActiveRun => has_active_run,
                    HarnessRuleOperationV1::Archive => !has_active_run,
                };
                let expected = if state.is_archived() {
                    Err(HARNESS_ARCHIVED)
                } else if permitted {
                    Ok(())
                } else {
                    Err(HARNESS_NOT_ACTIVE)
                };
                let result = validate_harness_rule_operation(state, has_active_run, operation);
                match expected {
                    Ok(()) => assert!(
                        result.is_ok(),
                        "{state:?} {has_active_run} {operation:?} must be permitted"
                    ),
                    Err(code) => assert_eq!(
                        result
                            .expect_err("fixture operation must be rejected")
                            .code(),
                        code,
                        "{state:?} {has_active_run} {operation:?}"
                    ),
                }
            }
        }
    }
}

/// Evidence row 1: updating a rule creates a new immutable revision, an
/// admitted run keeps its revision, and revision misuse is a typed conflict.
#[test]
fn rule_revisions_are_immutable_and_revision_conflicts_are_typed() {
    let active = revision_one();
    let updated = HarnessRuleRevisionV1::new(
        id(1),
        2,
        digest(5),
        HarnessExecutionClassV1::Light,
        HarnessTaskModeV1::GoalDirected,
        vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
        HarnessPresentationModeV1::JournalAndActivityEntry,
        "America/New_York".to_owned(),
    )
    .expect("fixture revision is valid");
    let revised = revise_harness_rule(&active, 1, updated)
        .expect("the expected revision continues the active revision");
    assert_eq!(revised.revision(), 2);
    assert_eq!(
        active.revision(),
        1,
        "the previous revision stays immutable"
    );
    assert_eq!(revised.harness_id(), id(1));
    assert_eq!(revised.class(), HarnessExecutionClassV1::Light);
    assert_eq!(revised.task_mode(), HarnessTaskModeV1::GoalDirected);
    assert!(revised.presentation_mode().publishes_activity_entry());

    assert_eq!(
        rejection_code(revise_harness_rule(&active, 2, revised)),
        HARNESS_REVISION_CONFLICT,
        "a changed expected revision is a conflict"
    );
    let skipped = HarnessRuleRevisionV1::new(
        id(1),
        4,
        digest(5),
        HarnessExecutionClassV1::Medium,
        HarnessTaskModeV1::RepeatedTask,
        vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
        HarnessPresentationModeV1::JournalOnly,
        zone(),
    )
    .expect("fixture revision is valid");
    assert_eq!(
        rejection_code(revise_harness_rule(&active, 1, skipped)),
        HARNESS_REVISION_CONFLICT,
        "a revision that skips the next number is a conflict"
    );
    let foreign = HarnessRuleRevisionV1::new(
        id(7),
        2,
        digest(5),
        HarnessExecutionClassV1::Medium,
        HarnessTaskModeV1::RepeatedTask,
        vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
        HarnessPresentationModeV1::JournalOnly,
        zone(),
    )
    .expect("fixture revision is valid");
    assert_eq!(
        rejection_code(revise_harness_rule(&active, 1, foreign)),
        HARNESS_REVISION_CONFLICT,
        "a revision of another harness never continues this rule"
    );
    assert_eq!(
        rejection_code(HarnessRuleRevisionV1::new(
            id(1),
            0,
            digest(5),
            HarnessExecutionClassV1::Medium,
            HarnessTaskModeV1::RepeatedTask,
            vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
            HarnessPresentationModeV1::JournalOnly,
            zone(),
        )),
        HARNESS_REVISION_CONFLICT
    );
    assert_eq!(
        rejection_code(HarnessRuleRevisionV1::new(
            id(1),
            1,
            digest(5),
            HarnessExecutionClassV1::Medium,
            HarnessTaskModeV1::RepeatedTask,
            vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
            HarnessPresentationModeV1::JournalOnly,
            String::new(),
        )),
        HARNESS_SCHEDULE_INVALID
    );
    let last = HarnessRuleRevisionV1::new(
        id(1),
        u64::MAX,
        digest(5),
        HarnessExecutionClassV1::Medium,
        HarnessTaskModeV1::RepeatedTask,
        vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
        HarnessPresentationModeV1::JournalOnly,
        zone(),
    )
    .expect("the last revision number is still a valid revision");
    assert_eq!(
        rejection_code(revise_harness_rule(&last, u64::MAX, revision_one())),
        HARNESS_REVISION_CONFLICT,
        "the last revision number cannot be continued"
    );
    assert_eq!(
        rejection_code(HarnessRuleV1::new(
            id(7),
            HarnessRuleScopeV1::Project { project_id: id(9) },
            revision_one(),
            id(8),
        )),
        HARNESS_REVISION_CONFLICT,
        "a rule and its active revision share one harness identity"
    );
}

/// Evidence row 2: a coalesced but not-yet-admitted reason uses the newest
/// active revision, while an admitted run keeps the revision it was admitted
/// under.
#[test]
fn pending_reasons_use_the_newest_active_revision_and_admitted_runs_keep_theirs() {
    let rule = rule_one();
    let admitted = rule.active_revision().clone();
    assert_eq!(rule.revision_for_pending_reason(), 1);
    assert_eq!(admitted.revision(), 1);

    let updated = HarnessRuleRevisionV1::new(
        id(1),
        2,
        digest(5),
        HarnessExecutionClassV1::Medium,
        HarnessTaskModeV1::RepeatedTask,
        vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
        HarnessPresentationModeV1::JournalOnly,
        zone(),
    )
    .expect("fixture revision is valid");
    let revised = revise_harness_rule(rule.active_revision(), 1, updated)
        .expect("the revision continues the rule");
    let revised_rule = HarnessRuleV1::new(
        rule.harness_id(),
        rule.scope(),
        revised,
        rule.service_session_id(),
    )
    .expect("the revised rule keeps its identity");
    assert_eq!(revised_rule.revision_for_pending_reason(), 2);
    assert_eq!(admitted.revision(), 1, "an admitted run keeps its revision");

    let unchanged =
        apply_harness_rule_operation(&revised_rule, false, HarnessRuleOperationV1::UpdateRevision)
            .expect("update-as-revision is permitted while the rule is not archived");
    assert_eq!(unchanged, revised_rule);
}

/// Evidence row 2: durable capture before admission, one pending reason per
/// rule, and redelivery that never produces a second run.
#[test]
fn trigger_capture_is_durable_before_admission_and_redelivery_never_duplicates() {
    let observation =
        HarnessTriggerObservationV1::new(id(3), HarnessSourceKindV1::FixedInterval, 1_000);
    assert_eq!(observation.reason_id(), id(3));
    assert_eq!(
        observation.source_kind(),
        HarnessSourceKindV1::FixedInterval
    );
    assert_eq!(observation.observed_at_ms(), 1_000);

    let captured = capture_harness_trigger(None, &observation);
    assert_eq!(captured.outcome(), HarnessTriggerCaptureOutcomeV1::Captured);
    assert_eq!(captured.reason().reason_id, id(3));
    assert_eq!(captured.reason().coalesced_count, 1);
    assert_eq!(captured.reason().first_observed_at_ms, 1_000);
    assert_eq!(captured.reason().last_observed_at_ms, 1_000);

    let redelivered = capture_harness_trigger(Some(captured.reason()), &observation);
    assert_eq!(
        redelivered.outcome(),
        HarnessTriggerCaptureOutcomeV1::Redelivered
    );
    assert_eq!(
        redelivered.reason(),
        captured.reason(),
        "redelivery keeps the one durable reason unchanged"
    );

    let later = HarnessTriggerObservationV1::new(id(4), HarnessSourceKindV1::CalendarTime, 2_000);
    let coalesced = capture_harness_trigger(Some(captured.reason()), &later);
    assert_eq!(
        coalesced.outcome(),
        HarnessTriggerCaptureOutcomeV1::Coalesced
    );
    assert_eq!(
        coalesced.reason().reason_id,
        id(3),
        "one pending reason per rule keeps its identity"
    );
    assert_eq!(
        coalesced.reason().source_kind,
        HarnessSourceKindV1::FixedInterval
    );
    assert_eq!(coalesced.reason().coalesced_count, 2);
    assert_eq!(coalesced.reason().first_observed_at_ms, 1_000);
    assert_eq!(coalesced.reason().last_observed_at_ms, 2_000);

    let capacity = HarnessAdmissionCapacityV1::available(true, true);
    assert!(capacity.rule_slot_available());
    assert!(capacity.concurrency_slot_available());
    assert!(capacity.has_capacity());
    let admitted = admit_harness_trigger(
        coalesced.reason(),
        HarnessRuleLifecycleStateV1::Active,
        HarnessLaunchOriginV1::AutomaticSource,
        capacity,
    )
    .expect("the single pending reason admits exactly one launch");
    assert!(admitted.is_admitted());
    assert_eq!(admitted.reason(), coalesced.reason());
}

/// Evidence row 2: full capacity retains the coalesced reason instead of
/// dropping it, and a paused rule captures automatic reasons without launching.
#[test]
fn full_capacity_retains_the_reason_and_paused_rules_launch_only_explicitly() {
    let pending = pending_reason();
    for capacity in [
        HarnessAdmissionCapacityV1::available(false, true),
        HarnessAdmissionCapacityV1::available(true, false),
    ] {
        let retained = admit_harness_trigger(
            &pending,
            HarnessRuleLifecycleStateV1::Active,
            HarnessLaunchOriginV1::AutomaticSource,
            capacity,
        )
        .expect("waiting for a free slot retains the coalesced reason");
        assert!(!retained.is_admitted());
        assert_eq!(retained.reason(), &pending);
    }
    assert!(!HarnessAdmissionCapacityV1::available(false, false).has_capacity());

    assert_eq!(
        rejection_code(admit_harness_trigger(
            &pending,
            HarnessRuleLifecycleStateV1::Paused,
            HarnessLaunchOriginV1::AutomaticSource,
            HarnessAdmissionCapacityV1::available(true, true),
        )),
        HARNESS_NOT_ACTIVE,
        "automatic sources are captured but do not launch while paused"
    );
    assert!(
        admit_harness_trigger(
            &pending,
            HarnessRuleLifecycleStateV1::Paused,
            HarnessLaunchOriginV1::ExplicitUserLaunch,
            HarnessAdmissionCapacityV1::available(true, true),
        )
        .expect("an explicit user launch remains allowed while paused")
        .is_admitted()
    );
    for origin in [
        HarnessLaunchOriginV1::AutomaticSource,
        HarnessLaunchOriginV1::ExplicitUserLaunch,
    ] {
        assert_eq!(
            rejection_code(admit_harness_trigger(
                &pending,
                HarnessRuleLifecycleStateV1::Archived,
                origin,
                HarnessAdmissionCapacityV1::available(true, true),
            )),
            HARNESS_ARCHIVED
        );
    }
}

/// Evidence row 2: catch-up admits at most one coalesced reason after downtime
/// and a burst of every missed slot is forbidden.
#[test]
fn catch_up_admits_at_most_one_coalesced_reason_after_downtime() {
    assert!(
        capture_harness_catch_up(
            None,
            id(3),
            HarnessSourceKindV1::FixedInterval,
            0,
            1_000,
            1_000
        )
        .is_none(),
        "no missed slot means no catch-up reason"
    );
    let catch_up = capture_harness_catch_up(
        None,
        id(3),
        HarnessSourceKindV1::FixedInterval,
        5,
        1_000,
        5_000,
    )
    .expect("missed slots coalesce into one catch-up reason");
    assert_eq!(catch_up.outcome(), HarnessTriggerCaptureOutcomeV1::CatchUp);
    assert_eq!(catch_up.reason().coalesced_count, 5);
    assert_eq!(catch_up.reason().first_observed_at_ms, 1_000);
    assert_eq!(catch_up.reason().last_observed_at_ms, 5_000);

    let merged = capture_harness_catch_up(
        Some(catch_up.reason()),
        id(4),
        HarnessSourceKindV1::FixedInterval,
        3,
        6_000,
        8_000,
    )
    .expect("catch-up merges into the single pending reason");
    assert_eq!(merged.reason().reason_id, id(3));
    assert_eq!(merged.reason().coalesced_count, 8);
    assert_eq!(merged.reason().last_observed_at_ms, 8_000);

    let reversed = capture_harness_catch_up(
        None,
        id(5),
        HarnessSourceKindV1::FixedInterval,
        2,
        9_000,
        7_000,
    )
    .expect("a reversed window canonicalizes deterministically");
    assert_eq!(reversed.reason().first_observed_at_ms, 7_000);
    assert_eq!(reversed.reason().last_observed_at_ms, 9_000);
}

/// Evidence row 2: completion links select known terminal outcomes explicitly
/// and are rejected on cause cycles.
#[test]
fn completion_links_select_known_outcomes_and_reject_cycles_and_deep_chains() {
    assert_eq!(
        rejection_code(HarnessCompletionLinkV1::new(id(2), Vec::new())),
        HARNESS_SOURCE_UNAVAILABLE,
        "a completion link chooses its known terminal outcomes explicitly"
    );
    let link = HarnessCompletionLinkV1::new(
        id(2),
        vec![
            HarnessTerminalOutcomeV1::Interrupted,
            HarnessTerminalOutcomeV1::Failed,
            HarnessTerminalOutcomeV1::Cancelled,
            HarnessTerminalOutcomeV1::Completed,
            HarnessTerminalOutcomeV1::Failed,
        ],
    )
    .expect("a completion link with known outcomes is valid");
    assert_eq!(link.source_reference(), id(2));
    assert_eq!(
        link.outcomes(),
        [
            HarnessTerminalOutcomeV1::Completed,
            HarnessTerminalOutcomeV1::Failed,
            HarnessTerminalOutcomeV1::Cancelled,
            HarnessTerminalOutcomeV1::Interrupted
        ]
        .as_slice(),
        "the closed outcome selection is unique and canonical"
    );

    assert_eq!(
        rejection_code(validate_harness_completion_link(&link, id(2), &[])),
        HARNESS_TRIGGER_CYCLE,
        "a self link is a cause cycle"
    );
    assert_eq!(
        rejection_code(validate_harness_completion_link(
            &link,
            id(9),
            &[id(7), id(2)]
        )),
        HARNESS_TRIGGER_CYCLE,
        "an existing cause ancestor is a cycle"
    );
    assert!(validate_harness_completion_link(&link, id(9), &[id(7), id(8)]).is_ok());
    assert_eq!(
        rejection_code(validate_harness_completion_link(&link, id(9), &[id(7); 8])),
        HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED
    );
    assert!(validate_harness_completion_link(&link, id(9), &[id(7); 7]).is_ok());

    assert_eq!(
        rejection_code(validate_harness_source_available(
            HarnessSourceKindV1::TerminalOutcomeLink,
            false
        )),
        HARNESS_SOURCE_UNAVAILABLE
    );
    assert!(
        validate_harness_source_available(HarnessSourceKindV1::TerminalOutcomeLink, true).is_ok()
    );
    for kind in [
        HarnessSourceKindV1::ExplicitUserLaunch,
        HarnessSourceKindV1::CalendarTime,
        HarnessSourceKindV1::FixedInterval,
    ] {
        assert!(validate_harness_source_available(kind, false).is_ok());
    }
}

/// Evidence row 3: the equal-interval grid is anchored durably, never shifts,
/// and rejects a cadence below one minute.
#[test]
fn equal_interval_schedules_keep_their_durable_grid_and_minimum() {
    assert_eq!(
        rejection_code(HarnessIntervalScheduleV1::new(
            0,
            HARNESS_MIN_INTERVAL_MS - 1
        )),
        HARNESS_INTERVAL_TOO_SHORT
    );
    assert_eq!(
        rejection_code(validate_harness_interval_ms(0)),
        HARNESS_INTERVAL_TOO_SHORT
    );
    assert!(validate_harness_interval_ms(HARNESS_MIN_INTERVAL_MS).is_ok());

    let schedule =
        HarnessIntervalScheduleV1::new(1_000, HARNESS_MIN_INTERVAL_MS).expect("fixture grid");
    assert_eq!(schedule.anchor_ms(), 1_000);
    assert_eq!(schedule.interval_ms(), HARNESS_MIN_INTERVAL_MS);
    assert_eq!(
        schedule.slot_at_or_before(500),
        None,
        "no slot exists before the durable anchor"
    );
    assert_eq!(schedule.slot_at_or_before(1_000), Some(1_000));
    assert_eq!(schedule.slot_at_or_before(60_999), Some(1_000));
    assert_eq!(schedule.slot_at_or_before(120_999), Some(61_000));
    assert_eq!(schedule.next_slot_after(500), 1_000);
    assert_eq!(
        schedule.next_slot_after(1_000),
        61_000,
        "the next slot is strictly after the given instant"
    );
    assert_eq!(schedule.next_slot_after(61_000), 121_000);
}

/// Evidence row 3: a clock change never repeats an already captured durable
/// trigger, and missed slots coalesce into one catch-up slot.
#[test]
fn clock_changes_never_repeat_an_already_captured_trigger_slot() {
    let schedule =
        HarnessIntervalScheduleV1::new(0, HARNESS_MIN_INTERVAL_MS).expect("fixture grid");
    assert_eq!(
        schedule.next_capture(None, 120_001),
        Some(120_000),
        "the first capture coalesces every missed slot into the newest one"
    );
    assert_eq!(
        schedule.next_capture(Some(120_000), 60_000),
        None,
        "a backwards clock never repeats a captured slot"
    );
    assert_eq!(schedule.next_capture(Some(120_000), 120_000), None);
    assert_eq!(
        schedule.next_capture(Some(120_000), 300_001),
        Some(300_000),
        "a forward jump admits one catch-up slot, never a burst"
    );

    let delayed = HarnessIntervalScheduleV1::new(1_000, HARNESS_MIN_INTERVAL_MS).expect("grid");
    assert_eq!(delayed.next_capture(None, 500), None);
}

/// Evidence row 3: both calendar forms canonicalize to one record and
/// contradictory equivalent inputs are rejected before admission.
#[test]
fn calendar_forms_canonicalize_to_one_record_and_contradictions_are_rejected() {
    let typed = HarnessCalendarScheduleV1::typed(Some(5), Some(4), None, None, None)
        .expect("a closed typed calendar form is valid");
    let both = HarnessCalendarScheduleV1::canonicalize(Some(&typed), Some("5 4 * * *"))
        .expect("equivalent forms canonicalize to one record");
    assert_eq!(both, typed);
    assert_eq!(both.minute(), Some(5));
    assert_eq!(both.hour(), Some(4));
    assert_eq!(both.day_of_month(), None);
    assert_eq!(both.month(), None);
    assert_eq!(both.day_of_week(), None);
    assert_eq!(
        HarnessCalendarScheduleV1::canonicalize(
            Some(&HarnessCalendarScheduleV1::every_minute()),
            None
        )
        .expect("the unrestricted calendar form is valid"),
        HarnessCalendarScheduleV1::every_minute()
    );

    let parsed = HarnessCalendarScheduleV1::canonicalize(None, Some("* * * * 7"))
        .expect("seven is the Sunday alias");
    assert_eq!(parsed.day_of_week(), Some(0));
    let typed_sunday = HarnessCalendarScheduleV1::typed(None, None, None, None, Some(7))
        .expect("a typed seven normalizes to Sunday");
    assert_eq!(typed_sunday.day_of_week(), Some(0));
    assert_eq!(
        HarnessCalendarScheduleV1::canonicalize(Some(&typed_sunday), Some("* * * * 0"))
            .expect("both Sunday forms agree"),
        typed_sunday
    );

    assert_eq!(
        rejection_code(HarnessCalendarScheduleV1::canonicalize(None, None)),
        HARNESS_SCHEDULE_INVALID
    );
    assert_eq!(
        rejection_code(HarnessCalendarScheduleV1::canonicalize(
            Some(&typed),
            Some("6 4 * * *")
        )),
        HARNESS_SCHEDULE_INVALID,
        "contradictory equivalent inputs are rejected before admission"
    );
    for expression in [
        "5 4 * *",
        "5 4 * * * *",
        "60 4 * * *",
        "5 24 * * *",
        "* * 30 2 *",
        "5-10 4 * * *",
        "5 4 * * monday",
        "5 4 0 * *",
    ] {
        assert_eq!(
            rejection_code(HarnessCalendarScheduleV1::canonicalize(
                None,
                Some(expression)
            )),
            HARNESS_SCHEDULE_INVALID,
            "{expression} must be rejected"
        );
    }
    assert_eq!(
        rejection_code(HarnessCalendarScheduleV1::typed(
            Some(31),
            None,
            Some(31),
            Some(4),
            None
        )),
        HARNESS_SCHEDULE_INVALID,
        "April has no 31st"
    );
    assert!(
        HarnessCalendarScheduleV1::typed(Some(31), None, Some(31), None, None).is_ok(),
        "an unrestricted month keeps a 31st valid"
    );
    assert!(
        HarnessCalendarScheduleV1::typed(Some(0), None, Some(31), Some(1), None).is_ok(),
        "January has a 31st"
    );
    assert!(
        HarnessCalendarScheduleV1::typed(Some(0), Some(0), Some(29), Some(2), Some(0)).is_ok(),
        "a leap day is a valid typed calendar slot"
    );
}

/// Evidence row 3: daylight-saving resolution selects the nearest valid local
/// time, forwarding a nonexistent time and firing a repeated time once.
#[test]
fn dst_resolution_moves_nonexistent_times_forward_and_fires_repeated_times_once() {
    assert_eq!(
        rejection_code(HarnessDstTransitionV1::new(1_000_000, 0, 0)),
        HARNESS_SCHEDULE_INVALID,
        "a daylight-saving transition changes its offset"
    );

    let spring = HarnessDstTransitionV1::new(1_000_000, 0, 3_600)
        .expect("a spring-forward transition is valid");
    assert_eq!(spring.transition_at_ms(), 1_000_000);
    assert_eq!(spring.offset_before_seconds(), 0);
    assert_eq!(spring.offset_after_seconds(), 3_600);
    assert_eq!(
        resolve_harness_local_time(2_000_000, &spring),
        HarnessLocalTimeResolutionV1::ForwardedToFirstValid { utc_ms: 1_000_000 },
        "a nonexistent local time moves forward to the first valid time"
    );
    assert_eq!(
        resolve_harness_local_time(2_000_000, &spring).utc_ms(),
        1_000_000
    );
    assert_eq!(
        resolve_harness_local_time(500_000, &spring),
        HarnessLocalTimeResolutionV1::Exact { utc_ms: 500_000 }
    );
    assert_eq!(
        resolve_harness_local_time(500_000, &spring).utc_ms(),
        500_000
    );
    assert_eq!(
        resolve_harness_local_time(5_000_000, &spring),
        HarnessLocalTimeResolutionV1::Exact { utc_ms: 1_400_000 }
    );
    assert_eq!(
        resolve_harness_local_time(5_000_000, &spring).utc_ms(),
        1_400_000
    );
    assert_eq!(
        HarnessLocalTimeResolutionV1::ForwardedToFirstValid { utc_ms: 1 }.utc_ms(),
        1
    );

    let fall =
        HarnessDstTransitionV1::new(1_000_000, 3_600, 0).expect("a fall-back transition is valid");
    let repeated = resolve_harness_local_time(2_000_000, &fall);
    assert_eq!(
        repeated,
        HarnessLocalTimeResolutionV1::RepeatedFiresOnce { utc_ms: -1_600_000 },
        "a repeated local time fires once at its earlier instant"
    );
    assert_eq!(repeated.utc_ms(), -1_600_000);
    assert_eq!(
        resolve_harness_local_time(500_000, &fall),
        HarnessLocalTimeResolutionV1::Exact { utc_ms: -3_100_000 }
    );
    assert_eq!(
        resolve_harness_local_time(5_000_000, &fall),
        HarnessLocalTimeResolutionV1::Exact { utc_ms: 5_000_000 }
    );
}

/// Evidence row 3: a non-archived rule follows a future project time-zone
/// change while each revision records the applied zone.
#[test]
fn project_time_zone_changes_apply_to_non_archived_rules_only() {
    assert!(validate_harness_time_zone("Europe/Paris").is_ok());
    assert_eq!(
        rejection_code(validate_harness_time_zone("")),
        HARNESS_SCHEDULE_INVALID
    );
    assert_eq!(
        rejection_code(validate_harness_time_zone("Europe Paris")),
        HARNESS_SCHEDULE_INVALID
    );
    assert_eq!(
        rejection_code(validate_harness_time_zone("Europe/Paris\n")),
        HARNESS_SCHEDULE_INVALID
    );
    assert_eq!(
        rejection_code(validate_harness_time_zone(&"z".repeat(257))),
        HARNESS_SCHEDULE_INVALID
    );
    assert_eq!(
        rejection_code(validate_harness_time_zone("sk-live-zone")),
        "credentials_forbidden"
    );

    assert_eq!(
        applied_harness_time_zone(
            HarnessRuleLifecycleStateV1::Active,
            "Europe/Paris",
            "America/New_York"
        )
        .expect("both zones are valid"),
        "America/New_York"
    );
    assert_eq!(
        applied_harness_time_zone(
            HarnessRuleLifecycleStateV1::Paused,
            "Europe/Paris",
            "America/New_York"
        )
        .expect("both zones are valid"),
        "America/New_York"
    );
    assert_eq!(
        applied_harness_time_zone(
            HarnessRuleLifecycleStateV1::Archived,
            "Europe/Paris",
            "America/New_York"
        )
        .expect("both zones are valid"),
        "Europe/Paris",
        "an archived rule retains its recorded zone"
    );
    assert_eq!(
        rejection_code(applied_harness_time_zone(
            HarnessRuleLifecycleStateV1::Active,
            "",
            "America/New_York"
        )),
        HARNESS_SCHEDULE_INVALID
    );
    assert_eq!(
        rejection_code(applied_harness_time_zone(
            HarnessRuleLifecycleStateV1::Active,
            "Europe/Paris",
            ""
        )),
        HARNESS_SCHEDULE_INVALID
    );
}

/// Evidence row 4: the dossier bound rejects over-limit input and canonicalizes
/// its explicit sources and typed references.
#[test]
fn dossier_bounds_reject_over_limit_and_canonicalize_its_references() {
    let dossier = HarnessDossierV1::new(
        digest(6),
        id(3),
        vec![id(5), id(4), id(4)],
        vec![id(9), id(8), id(8)],
        Some(id(7)),
        zone(),
        HARNESS_MAX_DOSSIER_BYTES,
        digest(7),
    )
    .expect("a bounded dossier is valid");
    assert_eq!(
        dossier.source_references(),
        [id(4), id(5)].as_slice(),
        "explicit sources canonicalize to ascending unique order"
    );
    assert_eq!(dossier.typed_references(), [id(8), id(9)].as_slice());
    assert_eq!(dossier.task_digest(), digest(6));
    assert_eq!(dossier.trigger_reason_id(), id(3));
    assert_eq!(dossier.checkpoint_reference(), Some(id(7)));
    assert_eq!(dossier.applied_time_zone(), zone());
    assert_eq!(dossier.dossier_bytes(), HARNESS_MAX_DOSSIER_BYTES);
    assert_eq!(dossier.dossier_digest(), digest(7));

    let sources: Vec<[u8; 16]> = (0_u8..17).map(id).collect();
    assert_eq!(
        rejection_code(HarnessDossierV1::new(
            digest(6),
            id(3),
            sources,
            Vec::new(),
            None,
            zone(),
            1,
            digest(7),
        )),
        HARNESS_DOSSIER_TOO_LARGE
    );
    let typed: Vec<[u8; 16]> = (0_u8..65).map(id).collect();
    assert_eq!(
        rejection_code(HarnessDossierV1::new(
            digest(6),
            id(3),
            Vec::new(),
            typed,
            None,
            zone(),
            1,
            digest(7),
        )),
        HARNESS_DOSSIER_TOO_LARGE
    );
    assert_eq!(
        rejection_code(HarnessDossierV1::new(
            digest(6),
            id(3),
            Vec::new(),
            Vec::new(),
            None,
            zone(),
            HARNESS_MAX_DOSSIER_BYTES + 1,
            digest(7),
        )),
        HARNESS_DOSSIER_TOO_LARGE
    );
    assert_eq!(
        rejection_code(validate_harness_dossier_bytes(
            HARNESS_MAX_DOSSIER_BYTES + 1
        )),
        HARNESS_DOSSIER_TOO_LARGE
    );
    assert!(validate_harness_dossier_bytes(HARNESS_MAX_DOSSIER_BYTES).is_ok());
    assert_eq!(
        rejection_code(HarnessDossierV1::new(
            digest(6),
            id(3),
            Vec::new(),
            Vec::new(),
            None,
            String::new(),
            1,
            digest(7),
        )),
        HARNESS_SCHEDULE_INVALID
    );
}

/// Evidence row 4: a successful run replaces the verified checkpoint only
/// after complete validation; every other outcome retains the previous one.
#[test]
fn checkpoint_replacement_requires_a_complete_validated_checkpoint() {
    assert_eq!(
        rejection_code(HarnessVerifiedCheckpointV1::new(
            id(1),
            id(2),
            0,
            digest(3),
            1
        )),
        HARNESS_REVISION_CONFLICT
    );
    assert_eq!(
        rejection_code(HarnessVerifiedCheckpointV1::new(
            id(1),
            id(2),
            1,
            digest(3),
            HARNESS_MAX_CHECKPOINT_BYTES + 1
        )),
        HARNESS_CHECKPOINT_TOO_LARGE
    );
    assert_eq!(
        rejection_code(validate_harness_checkpoint_bytes(
            HARNESS_MAX_CHECKPOINT_BYTES + 1
        )),
        HARNESS_CHECKPOINT_TOO_LARGE
    );
    assert!(validate_harness_checkpoint_bytes(HARNESS_MAX_CHECKPOINT_BYTES).is_ok());

    let previous =
        HarnessVerifiedCheckpointV1::new(id(1), id(2), 1, digest(3), HARNESS_MAX_CHECKPOINT_BYTES)
            .expect("a bounded verified checkpoint is valid");
    assert_eq!(previous.checkpoint_id(), id(1));
    assert_eq!(previous.producing_run_id(), id(2));
    assert_eq!(previous.revision(), 1);
    assert_eq!(previous.content_digest(), digest(3));
    assert_eq!(previous.checkpoint_bytes(), HARNESS_MAX_CHECKPOINT_BYTES);
    assert!(previous.is_produced_by(id(2)));
    assert!(!previous.is_produced_by(id(4)));

    let candidate = HarnessVerifiedCheckpointV1::new(id(4), id(5), 2, digest(6), 128)
        .expect("a bounded candidate checkpoint is valid");
    assert_eq!(
        verified_checkpoint_after_run(
            Some(&previous),
            HarnessRunOutcomeV1::Completed,
            Some(candidate)
        )
        .expect("a completed run may replace the verified checkpoint"),
        Some(candidate)
    );
    assert_eq!(
        rejection_code(verified_checkpoint_after_run(
            Some(&previous),
            HarnessRunOutcomeV1::Completed,
            None
        )),
        HARNESS_CHECKPOINT_UNAVAILABLE,
        "a missing checkpoint never reruns the producing run"
    );
    for outcome in [
        HarnessRunOutcomeV1::Failed,
        HarnessRunOutcomeV1::Cancelled,
        HarnessRunOutcomeV1::Interrupted,
        HarnessRunOutcomeV1::ExternalEffectUnknown,
    ] {
        let retained = verified_checkpoint_after_run(Some(&previous), outcome, None)
            .expect("a non-successful run retains the previous verified checkpoint");
        assert_eq!(
            retained,
            Some(previous),
            "an older checkpoint is never presented as the current run's state"
        );
    }
    assert_eq!(
        verified_checkpoint_after_run(None, HarnessRunOutcomeV1::Cancelled, None)
            .expect("no previous checkpoint is a valid retained state"),
        None
    );
}

/// Evidence row 4: the user-visible safe conclusion is rejected rather than
/// truncated when it exceeds its bound.
#[test]
fn conclusion_bound_rejects_oversized_results() {
    assert_eq!(
        rejection_code(HarnessConclusionV1::new(
            id(1),
            digest(2),
            HARNESS_MAX_CONCLUSION_BYTES + 1
        )),
        HARNESS_RESULT_TOO_LARGE
    );
    assert_eq!(
        rejection_code(validate_harness_conclusion_bytes(
            HARNESS_MAX_CONCLUSION_BYTES + 1
        )),
        HARNESS_RESULT_TOO_LARGE
    );
    assert!(validate_harness_conclusion_bytes(HARNESS_MAX_CONCLUSION_BYTES).is_ok());

    let conclusion = HarnessConclusionV1::new(id(1), digest(2), HARNESS_MAX_CONCLUSION_BYTES)
        .expect("a bounded conclusion is valid");
    assert_eq!(conclusion.producing_run_id(), id(1));
    assert_eq!(conclusion.content_digest(), digest(2));
    assert_eq!(conclusion.conclusion_bytes(), HARNESS_MAX_CONCLUSION_BYTES);
}

/// Evidence row 4: durable records reject credential-shaped text before it can
/// enter any identity, digest, or bound.
#[test]
fn credential_shaped_values_are_rejected_before_durable_identity() {
    for secret in ["sk-live-123", "Bearer token", "api_key=abc"] {
        assert_eq!(
            rejection_code(validate_harness_time_zone(secret)),
            "credentials_forbidden",
            "{secret} must never enter a durable harness record"
        );
        assert_eq!(
            rejection_code(HarnessTriggerRecordV1::new(
                pending_reason(),
                1,
                secret.to_owned(),
                None
            )),
            "credentials_forbidden"
        );
    }
}

/// Evidence row 4: a trigger record carries only the frozen reason, the
/// originating revision, the applied zone, and a cause-chain reference.
#[test]
fn durable_trigger_records_carry_revision_zone_and_cause_reference_only() {
    let record = HarnessTriggerRecordV1::new(pending_reason(), 2, zone(), Some(id(4)))
        .expect("a durable trigger record is valid");
    assert_eq!(record.reason(), &pending_reason());
    assert_eq!(record.originating_rule_revision(), 2);
    assert_eq!(record.applied_time_zone(), zone());
    assert_eq!(record.cause_chain_reference(), Some(id(4)));
    assert_eq!(
        rejection_code(HarnessTriggerRecordV1::new(
            pending_reason(),
            0,
            zone(),
            None
        )),
        HARNESS_REVISION_CONFLICT
    );
    assert_eq!(
        rejection_code(HarnessTriggerRecordV1::new(
            pending_reason(),
            2,
            String::new(),
            None
        )),
        HARNESS_SCHEDULE_INVALID
    );
}

/// Evidence row 5: class resolution narrows by inheritance and never widens the
/// requested class.
#[test]
fn class_resolution_narrows_only_and_never_widens_the_requested_class() {
    assert_eq!(harness_class_rank(HarnessExecutionClassV1::Light), 0);
    assert_eq!(harness_class_rank(HarnessExecutionClassV1::Medium), 1);
    assert_eq!(harness_class_rank(HarnessExecutionClassV1::Heavy), 2);
    assert_eq!(
        resolve_harness_class(
            HarnessExecutionClassV1::Heavy,
            HarnessExecutionClassV1::Light
        ),
        HarnessExecutionClassV1::Light
    );
    assert_eq!(
        resolve_harness_class(
            HarnessExecutionClassV1::Light,
            HarnessExecutionClassV1::Heavy
        ),
        HarnessExecutionClassV1::Light,
        "a harness class never widens its inherited base"
    );
    assert_eq!(
        resolve_harness_class(
            HarnessExecutionClassV1::Medium,
            HarnessExecutionClassV1::Medium
        ),
        HarnessExecutionClassV1::Medium
    );
    assert!(
        validate_harness_class_narrowing(
            HarnessExecutionClassV1::Heavy,
            HarnessExecutionClassV1::Medium
        )
        .is_ok()
    );
    assert_eq!(
        rejection_code(validate_harness_class_narrowing(
            HarnessExecutionClassV1::Light,
            HarnessExecutionClassV1::Medium
        )),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
}

/// Evidence row 5: the read-and-delegate tool set is closed and `sub_agent` is
/// reachable only through the programmatic-policy corridor.
#[test]
fn sub_agent_is_reachable_only_through_the_programmatic_policy_corridor() {
    assert_eq!(
        HarnessReadDelegateToolV1::ALL.map(|tool| tool.id()),
        ["read", "glob", "grep", "expand", "retrieve", "sub_agent"]
    );
    for tool in HarnessReadDelegateToolV1::ALL {
        assert_eq!(HarnessReadDelegateToolV1::from_id(tool.id()), Some(tool));
    }
    assert_eq!(HarnessReadDelegateToolV1::from_id("write"), None);
    assert_eq!(HarnessReadDelegateToolV1::from_id("edit"), None);

    let read_only = ["read".to_owned(), "grep".to_owned()];
    assert!(validate_harness_narrowed_tools(&read_only, None).is_ok());
    assert_eq!(
        rejection_code(validate_harness_narrowed_tools(
            &["sub_agent".to_owned()],
            None
        )),
        INVALID_HARNESS_CLASS_RESOLUTION,
        "sub_agent is unreachable without the corridor"
    );
    assert_eq!(
        rejection_code(validate_harness_narrowed_tools(&["write".to_owned()], None)),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
    assert_eq!(
        rejection_code(validate_harness_narrowed_tools(
            &["read".to_owned(), "read".to_owned()],
            None
        )),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
    let over_bound: Vec<String> = (0..17).map(|_| "read".to_owned()).collect();
    assert_eq!(
        rejection_code(validate_harness_narrowed_tools(&over_bound, None)),
        INVALID_HARNESS_CLASS_RESOLUTION,
        "the narrowed tool selection has a bound"
    );

    let corridor = HarnessSubAgentCorridorV1::new(id(11), HarnessExecutionClassV1::Medium, 2, 3)
        .expect("a bounded corridor is valid");
    assert_eq!(corridor.corridor_reference(), id(11));
    assert_eq!(corridor.permitted_class(), HarnessExecutionClassV1::Medium);
    assert_eq!(corridor.permitted_depth(), 2);
    assert_eq!(corridor.permitted_child_count(), 3);
    assert!(validate_harness_narrowed_tools(&["sub_agent".to_owned()], Some(&corridor)).is_ok());
    assert!(
        corridor
            .validate_use(HarnessExecutionClassV1::Light, 2, 3)
            .is_ok(),
        "a fresh run-bound use may narrow the corridor"
    );
    assert_eq!(
        rejection_code(corridor.validate_use(HarnessExecutionClassV1::Heavy, 1, 1)),
        INVALID_HARNESS_CLASS_RESOLUTION,
        "a launch cannot widen the corridor class"
    );
    assert_eq!(
        rejection_code(corridor.validate_use(HarnessExecutionClassV1::Medium, 3, 1)),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
    assert_eq!(
        rejection_code(corridor.validate_use(HarnessExecutionClassV1::Medium, 1, 4)),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
    assert_eq!(
        rejection_code(HarnessSubAgentCorridorV1::new(
            id(11),
            HarnessExecutionClassV1::Medium,
            0,
            1
        )),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
    assert_eq!(
        rejection_code(HarnessSubAgentCorridorV1::new(
            id(11),
            HarnessExecutionClassV1::Medium,
            HARNESS_MAX_CAUSE_DEPTH + 1,
            1
        )),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
    assert_eq!(
        rejection_code(HarnessSubAgentCorridorV1::new(
            id(11),
            HarnessExecutionClassV1::Medium,
            1,
            0
        )),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
    assert_eq!(
        rejection_code(HarnessSubAgentCorridorV1::new(
            id(11),
            HarnessExecutionClassV1::Medium,
            1,
            HARNESS_MAX_CONCURRENT + 1
        )),
        INVALID_HARNESS_CLASS_RESOLUTION
    );
}

/// Evidence row 4: rule sources map onto the frozen closed source kinds.
#[test]
fn rule_sources_are_closed_and_map_to_the_frozen_source_kinds() {
    assert_eq!(
        HarnessRuleSourceV1::ExplicitUserLaunch.kind(),
        HarnessSourceKindV1::ExplicitUserLaunch
    );
    assert_eq!(
        HarnessRuleSourceV1::CalendarTime(HarnessCalendarScheduleV1::every_minute()).kind(),
        HarnessSourceKindV1::CalendarTime
    );
    assert_eq!(
        interval_source(0, HARNESS_MIN_INTERVAL_MS).kind(),
        HarnessSourceKindV1::FixedInterval
    );
    let link = HarnessCompletionLinkV1::new(id(2), vec![HarnessTerminalOutcomeV1::Completed])
        .expect("a terminal outcome is selected");
    assert_eq!(
        HarnessRuleSourceV1::TerminalOutcomeLink(link).kind(),
        HarnessSourceKindV1::TerminalOutcomeLink
    );
}

/// Evidence row 6: the code-owned bounds equal architecture 26 and each bound
/// rejects one unit beyond its limit with its typed code.
#[test]
fn code_owned_harness_bounds_match_architecture_26_and_reject_over_limit() {
    assert_eq!(HARNESS_MAX_RULES, 64);
    assert_eq!(HARNESS_MAX_SOURCES, 16);
    assert_eq!(HARNESS_MAX_CONCURRENT, 16);
    assert_eq!(HARNESS_MIN_INTERVAL_MS, 60_000);
    assert_eq!(HARNESS_MAX_DOSSIER_BYTES, 512 * 1024);
    assert_eq!(HARNESS_MAX_CHECKPOINT_BYTES, 512 * 1024);
    assert_eq!(HARNESS_MAX_CONCLUSION_BYTES, 512 * 1024);
    assert_eq!(HARNESS_MAX_DOSSIER_REFERENCES, 64);
    assert_eq!(HARNESS_MAX_CAUSE_DEPTH, 8);
    assert_eq!(HARNESS_MAX_SUCCESSORS, 16);
    assert_eq!(HARNESS_MAX_TOTAL_LAUNCHES, 256);
    assert_eq!(HARNESS_MAX_NARROWED_TOOLS, 16);

    assert!(validate_harness_rule_count(HARNESS_MAX_RULES).is_ok());
    assert_eq!(
        rejection_code(validate_harness_rule_count(HARNESS_MAX_RULES + 1)),
        HARNESS_RULE_LIMIT_EXCEEDED
    );
    assert!(validate_harness_source_count(HARNESS_MAX_SOURCES).is_ok());
    assert_eq!(
        rejection_code(validate_harness_source_count(HARNESS_MAX_SOURCES + 1)),
        HARNESS_SOURCE_LIMIT_EXCEEDED
    );
    assert!(validate_harness_concurrency_count(HARNESS_MAX_CONCURRENT).is_ok());
    assert_eq!(
        rejection_code(validate_harness_concurrency_count(
            HARNESS_MAX_CONCURRENT + 1
        )),
        HARNESS_CONCURRENCY_LIMIT_EXCEEDED
    );
    assert!(validate_harness_cause_depth(HARNESS_MAX_CAUSE_DEPTH).is_ok());
    assert_eq!(
        rejection_code(validate_harness_cause_depth(HARNESS_MAX_CAUSE_DEPTH + 1)),
        HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED
    );
    assert!(validate_harness_successor_count(HARNESS_MAX_SUCCESSORS).is_ok());
    assert_eq!(
        rejection_code(validate_harness_successor_count(HARNESS_MAX_SUCCESSORS + 1)),
        HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED
    );
    assert!(validate_harness_total_launches(HARNESS_MAX_TOTAL_LAUNCHES).is_ok());
    assert_eq!(
        rejection_code(validate_harness_total_launches(
            HARNESS_MAX_TOTAL_LAUNCHES + 1
        )),
        HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED
    );

    let sources: Vec<HarnessRuleSourceV1> = (0_u8..17)
        .map(|_| interval_source(0, HARNESS_MIN_INTERVAL_MS))
        .collect();
    assert_eq!(
        rejection_code(HarnessRuleRevisionV1::new(
            id(1),
            1,
            digest(2),
            HarnessExecutionClassV1::Medium,
            HarnessTaskModeV1::RepeatedTask,
            sources,
            HarnessPresentationModeV1::JournalOnly,
            zone(),
        )),
        HARNESS_SOURCE_LIMIT_EXCEEDED
    );
    assert_eq!(
        rejection_code(HarnessRuleRevisionV1::new(
            id(1),
            1,
            digest(2),
            HarnessExecutionClassV1::Medium,
            HarnessTaskModeV1::RepeatedTask,
            Vec::new(),
            HarnessPresentationModeV1::JournalOnly,
            zone(),
        )),
        HARNESS_SOURCE_UNAVAILABLE,
        "a rule names at least one source"
    );
}

/// Evidence rows 1-6: every closed `harness_*` safe failure is declared exactly
/// once and has one reachable typed pre-effect rejection.
#[test]
fn closed_harness_safe_failures_are_fifteen_distinct_reachable_codes() {
    assert_eq!(HARNESS_CLOSED_SAFE_FAILURES.len(), 15);
    let mut declared = HARNESS_CLOSED_SAFE_FAILURES.to_vec();
    declared.sort_unstable();
    declared.dedup();
    assert_eq!(declared.len(), 15, "each closed code is declared once");
    assert!(
        declared.iter().all(|code| code.starts_with("harness_")),
        "the closed set contains only harness_ prefixed codes"
    );

    let self_link = HarnessCompletionLinkV1::new(id(9), vec![HarnessTerminalOutcomeV1::Completed])
        .expect("a terminal outcome is selected");
    let reached = [
        rejection_code(validate_harness_rule_count(HARNESS_MAX_RULES + 1)),
        rejection_code(validate_harness_source_count(HARNESS_MAX_SOURCES + 1)),
        rejection_code(validate_harness_concurrency_count(
            HARNESS_MAX_CONCURRENT + 1,
        )),
        rejection_code(HarnessIntervalScheduleV1::new(0, 1)),
        rejection_code(HarnessCalendarScheduleV1::canonicalize(None, None)),
        rejection_code(validate_harness_completion_link(&self_link, id(9), &[])),
        rejection_code(validate_harness_dossier_bytes(
            HARNESS_MAX_DOSSIER_BYTES + 1,
        )),
        rejection_code(HarnessCompletionLinkV1::new(id(2), Vec::new())),
        rejection_code(validate_harness_checkpoint_bytes(
            HARNESS_MAX_CHECKPOINT_BYTES + 1,
        )),
        rejection_code(verified_checkpoint_after_run(
            None,
            HarnessRunOutcomeV1::Completed,
            None,
        )),
        rejection_code(validate_harness_conclusion_bytes(
            HARNESS_MAX_CONCLUSION_BYTES + 1,
        )),
        rejection_code(apply_harness_rule_operation(
            &rule_one(),
            false,
            HarnessRuleOperationV1::Resume,
        )),
        rejection_code(apply_harness_rule_operation(
            &rule_in_state(HarnessRuleLifecycleStateV1::Archived),
            false,
            HarnessRuleOperationV1::Pause,
        )),
        rejection_code(revise_harness_rule(&revision_one(), 2, revision_one())),
        rejection_code(validate_harness_cause_depth(HARNESS_MAX_CAUSE_DEPTH + 1)),
    ];
    assert_eq!(reached.len(), 15);
    let mut reached_codes = reached.to_vec();
    reached_codes.sort_unstable();
    reached_codes.dedup();
    assert_eq!(
        reached_codes.len(),
        15,
        "each rejection produces its own code"
    );
    let declared_owned: Vec<String> = declared.iter().map(|code| (*code).to_owned()).collect();
    assert_eq!(reached_codes, declared_owned);
}

/// Evidence row 1: the two harness scopes are exactly project and ordinary user
/// session, and a rule carries only safe identity values.
#[test]
fn harness_scopes_are_exactly_project_and_user_session() {
    let project = HarnessRuleScopeV1::Project { project_id: id(1) };
    assert_eq!(project.project_id(), id(1));
    assert_eq!(project.session_id(), None);
    assert!(!project.is_session_linked());

    let session = HarnessRuleScopeV1::UserSession {
        project_id: id(1),
        session_id: id(2),
    };
    assert_eq!(session.project_id(), id(1));
    assert_eq!(session.session_id(), Some(id(2)));
    assert!(session.is_session_linked());

    let rule = HarnessRuleV1::new(id(1), session, revision_one(), id(8))
        .expect("a session-scoped harness owns its service session");
    assert_eq!(rule.harness_id(), id(1));
    assert_eq!(rule.scope(), session);
    assert_eq!(rule.active_revision(), &revision_one());
    assert_eq!(rule.service_session_id(), id(8));
    assert_eq!(rule.active_revision().source_count(), 1);
    assert_eq!(rule.active_revision().applied_time_zone(), zone());
    assert_eq!(rule.active_revision().harness_id(), id(1));
    assert_eq!(rule.active_revision().task_digest(), digest(2));
    assert_eq!(
        rule.active_revision().class(),
        HarnessExecutionClassV1::Medium
    );
    assert_eq!(
        rule.active_revision().task_mode(),
        HarnessTaskModeV1::RepeatedTask
    );
    assert!(
        !rule
            .active_revision()
            .presentation_mode()
            .publishes_activity_entry()
    );
    assert_eq!(rule.active_revision().sources().len(), 1);
}

/// EXC-041: autonomous harness goal mode is separately admitted, continues only
/// against an active goal, and is never a free-running agent.
#[test]
fn autonomous_goal_mode_is_separately_admitted_and_never_free_running() {
    assert_eq!(
        plan_harness_goal_continuation(
            &revision_one(),
            HarnessRuleLifecycleStateV1::Active,
            Some((id(20), 3)),
            id(21)
        )
        .expect("a repeated-task rule has no goal continuation"),
        None
    );

    let goal_revision = HarnessRuleRevisionV1::new(
        id(1),
        2,
        digest(5),
        HarnessExecutionClassV1::Medium,
        HarnessTaskModeV1::GoalDirected,
        vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
        HarnessPresentationModeV1::JournalOnly,
        zone(),
    )
    .expect("fixture goal revision is valid");
    assert_eq!(
        rejection_code(plan_harness_goal_continuation(
            &goal_revision,
            HarnessRuleLifecycleStateV1::Archived,
            Some((id(20), 3)),
            id(21)
        )),
        HARNESS_ARCHIVED
    );
    assert_eq!(
        rejection_code(plan_harness_goal_continuation(
            &goal_revision,
            HarnessRuleLifecycleStateV1::Active,
            None,
            id(21)
        )),
        HARNESS_NOT_ACTIVE,
        "goal mode continues only against an active goal"
    );
    assert_eq!(
        rejection_code(plan_harness_goal_continuation(
            &goal_revision,
            HarnessRuleLifecycleStateV1::Active,
            Some((id(20), 0)),
            id(21)
        )),
        HARNESS_REVISION_CONFLICT
    );

    let continuation = plan_harness_goal_continuation(
        &goal_revision,
        HarnessRuleLifecycleStateV1::Paused,
        Some((id(20), 3)),
        id(21),
    )
    .expect("goal continuation is separately admitted")
    .expect("goal mode yields one continuation");
    assert_eq!(continuation.goal_id(), id(20));
    assert_eq!(continuation.goal_revision(), 3);
    assert_eq!(continuation.rule_revision(), 2);
    assert_eq!(
        continuation.launch_reference(),
        id(21),
        "each continuation is bound to its own separately admitted launch"
    );
}

/// EXC-042: capture and coalescing are client-independent, the journal is
/// readable after reconnect, and no old external work resumes.
#[test]
fn post_disconnect_keeps_capture_and_never_resumes_external_work() {
    let contract = HarnessDisconnectContractV1::frozen();
    assert!(contract.capture_continues());
    assert!(contract.journal_readable_after_reconnect());
    assert!(contract.requires_separate_admission());
    assert!(!contract.external_work_resumes());
    assert_eq!(
        HarnessClientConnectionV1::Connected.disconnect_contract(),
        contract,
        "durable capture and coalescing are client-independent"
    );
    assert_eq!(
        HarnessClientConnectionV1::Disconnected.disconnect_contract(),
        contract
    );
    assert!(validate_harness_disconnect_contract(&contract).is_ok());
    assert_eq!(
        rejection_code(validate_harness_disconnect_contract(
            &HarnessDisconnectContractV1::new(true, true, true, true)
        )),
        INVALID_HARNESS_DISCONNECT_CONTRACT,
        "no post-disconnect state may resume old external work"
    );
    assert_eq!(
        rejection_code(validate_harness_disconnect_contract(
            &HarnessDisconnectContractV1::new(false, true, true, false)
        )),
        INVALID_HARNESS_DISCONNECT_CONTRACT
    );
}

/// A rejection with the expected closed code.
fn rejection_code<T: std::fmt::Debug>(result: DtoResult<T>) -> String {
    result
        .expect_err("fixture call must be rejected before any effect")
        .code()
        .to_owned()
}

/// One stable test identity.
const fn id(byte: u8) -> [u8; 16] {
    [byte; 16]
}

/// One stable test digest.
fn digest(byte: u8) -> Digest256 {
    Digest256::from_bytes(&[byte; 32]).expect("fixture digest is exactly thirty-two bytes")
}

/// The fixture project time zone.
fn zone() -> String {
    "Europe/Paris".to_owned()
}

/// One valid equal-interval rule source.
fn interval_source(anchor_ms: u64, interval_ms: u64) -> HarnessRuleSourceV1 {
    HarnessRuleSourceV1::FixedInterval(
        HarnessIntervalScheduleV1::new(anchor_ms, interval_ms)
            .expect("fixture interval meets the minimum"),
    )
}

/// One valid immutable rule revision.
fn revision_one() -> HarnessRuleRevisionV1 {
    HarnessRuleRevisionV1::new(
        id(1),
        1,
        digest(2),
        HarnessExecutionClassV1::Medium,
        HarnessTaskModeV1::RepeatedTask,
        vec![interval_source(0, HARNESS_MIN_INTERVAL_MS)],
        HarnessPresentationModeV1::JournalOnly,
        zone(),
    )
    .expect("fixture revision is valid")
}

/// One valid project-scoped harness rule in the active state.
fn rule_one() -> HarnessRuleV1 {
    HarnessRuleV1::new(
        id(1),
        HarnessRuleScopeV1::Project { project_id: id(9) },
        revision_one(),
        id(8),
    )
    .expect("fixture rule is valid")
}

/// One valid rule in the requested lifecycle state.
fn rule_in_state(state: HarnessRuleLifecycleStateV1) -> HarnessRuleV1 {
    match state {
        HarnessRuleLifecycleStateV1::Active => rule_one(),
        HarnessRuleLifecycleStateV1::Paused => {
            apply_harness_rule_operation(&rule_one(), false, HarnessRuleOperationV1::Pause)
                .expect("pause is valid from active")
        }
        HarnessRuleLifecycleStateV1::Archived => {
            apply_harness_rule_operation(&rule_one(), false, HarnessRuleOperationV1::Archive)
                .expect("archive is valid without an active run")
        }
    }
}

/// One valid durable trigger reason.
const fn pending_reason() -> HarnessTriggerReasonV1 {
    HarnessTriggerReasonV1 {
        reason_id: id(3),
        source_kind: HarnessSourceKindV1::FixedInterval,
        first_observed_at_ms: 1_000,
        last_observed_at_ms: 1_000,
        coalesced_count: 1,
    }
}
