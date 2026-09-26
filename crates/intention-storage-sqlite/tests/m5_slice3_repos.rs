//! Slice 3 durable repository contracts for the harness, programmatic-caller
//! policy, Goal, and unified verification table families. Each store-level
//! family is exercised for round-trip durability, ordering, atomic conflicts,
//! idempotent replay, stale and duplicate rejection, rollback, and the
//! credential-free storage property, in the established Slice 2 repository
//! test style.

#![allow(
    clippy::expect_used,
    reason = "SQLite repository fixtures use expect for precise test diagnostics."
)]

use intention_storage::goal_repo::{
    AppendGoalGateRevisionInputDto, AppendGoalRevisionInputDto, AttachGoalChildInputDto,
    ConversationSummaryRecordDto, CreateGoalGateInputDto, CreateGoalInputDto,
    CreateGoalSessionLinkInputDto, GoalCardRepositoryDto, GoalCompactionRepositoryDto,
    GoalCompactionWorkingFormRecordDto, GoalEvidenceKindDto, GoalEvidenceReferenceDto,
    GoalGateDefinitionDto, GoalGateExceptionDto, GoalGateExceptionKindDto, GoalGateInputFamilyDto,
    GoalGateOutcomeDispositionDto, GoalGateOutcomeKindDto, GoalGateRecordDto,
    GoalGateRepositoryDto, GoalGateResultRecordDto, GoalGateTemplateRecordDto,
    GoalLifecycleStateDto, GoalMemoryCardRecordDto, GoalMemoryCardReplacementInputDto,
    GoalMemoryCardRollbackInputDto, GoalMilestoneDto, GoalProposalRepositoryDto,
    GoalReadinessStateDto, GoalRecordDto, GoalRecordScopeDto, GoalRepositoryDto,
    GoalRevisionRecordDto, GoalRoleCardRecordDto, GoalRoleClassDto, GoalScopeDto,
    GoalSessionLinkRecordDto, GoalSkillCardRecordDto, GoalTemplateLifecycleStateDto,
    GoalTemplateProvenanceKindDto, GoalTemplateScopeDto, GoalUserDecisionStateDto,
    GoalVerificationRepositoryDto, MemoryKindDto, RecordCompactionSuffixReferenceInputDto,
    RecordGoalUserDecisionInputDto, RefinementDraftRecordDto, RefinementDraftStateDto,
    RefinementEditKindDto, RefinementEditRecordDto, RevokeVerifierAuthorityInputDto,
    SetGoalReadinessInputDto, TransitionGoalGateTemplateInputDto, TransitionGoalLifecycleInputDto,
    VerificationAuditVerdictDto, VerifierAuditBaselineRecordDto, VerifierAuditEvidenceRecordDto,
    VerifierAuditVerdictRecordDto, VerifierAuthorityConsumptionRuleDto,
    VerifierAuthorityConsumptionStateDto, VerifierAuthorityRecordDto,
    VerifierAuthorityReferenceDto, VerifierContractReferenceDto, VerifierEvidenceKindDto,
    VerifierFrozenReferencesDto, VerifierGoalReferenceDto, VerifierOperationDto,
    VerifierOperationIdentityDto, VerifierTargetLifecycleDto, VerifierTargetMutationRecordDto,
    VerifierTargetReferenceDto, VerifierTargetSetReferenceDto,
};
use intention_storage::harness_repo::{
    AppendHarnessJournalRecordInputDto, CaptureHarnessTriggerInputDto,
    CommitHarnessCheckpointInputDto, CreateHarnessRuleInputDto, HarnessCheckpointRecordDto,
    HarnessCheckpointRepositoryDto, HarnessDossierRecordDto, HarnessExecutionClassDto,
    HarnessJournalRecordKindDto, HarnessPresentationModeDto, HarnessRuleLifecycleStateDto,
    HarnessRuleOperationDto, HarnessRuleRecordDto, HarnessRuleRepositoryDto,
    HarnessRuleRevisionRecordDto, HarnessRuleScopeDto, HarnessRunOutcomeDto, HarnessSourceKindDto,
    HarnessTaskModeDto, HarnessTriggerCaptureOutcomeDto, HarnessTriggerReasonStateDto,
    HarnessTriggerRepositoryDto, LoadHarnessJournalInputDto, RecordHarnessLaunchInputDto,
    ReviseHarnessRuleInputDto, TransitionHarnessRuleLifecycleInputDto,
};
use intention_storage::programmatic_policy_repo::{
    AppendProgrammaticPolicyRevisionInputDto, CommitProgrammaticReservationStartedInputDto,
    ConsumeProgrammaticCorridorActionInputDto, CreateProgrammaticPolicyInputDto,
    DecideProgrammaticPolicyConfirmationInputDto, ProgrammaticAdmissionDecisionDto,
    ProgrammaticAdmissionRepositoryDto, ProgrammaticAuthorizationCorridorRecordDto,
    ProgrammaticCalendarCounterReferenceDto, ProgrammaticCalendarLimitRecordDto,
    ProgrammaticCalendarPeriodKindDto, ProgrammaticConfirmationRepositoryDto,
    ProgrammaticConfirmationStateDto, ProgrammaticCorridorStateDto,
    ProgrammaticPolicyConfirmationRecordDto, ProgrammaticPolicyDraftRecordDto,
    ProgrammaticPolicyDraftStateDto, ProgrammaticPolicyLifecycleOperationDto,
    ProgrammaticPolicyLifecycleStateDto, ProgrammaticPolicyRecordDto,
    ProgrammaticPolicyRepositoryDto, ProgrammaticPolicyReservationRecordDto,
    ProgrammaticPolicyRevisionRecordDto, ProgrammaticPolicyRevisionReferenceDto,
    ProgrammaticPolicyScopeDto, ProgrammaticPolicySnapshotRecordDto,
    ProgrammaticReservationStateDto, ProgrammaticRootOriginKindDto,
    ProgrammaticRootOriginRuleRecordDto, RecoverProgrammaticPolicyReservationInputDto,
    ReleaseProgrammaticPolicyReservationInputDto, ReserveProgrammaticPolicyActionInputDto,
    TransitionProgrammaticPolicyLifecycleInputDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use tempfile::TempDir;

fn repository() -> (TempDir, SqliteStorageRepository) {
    let directory = TempDir::new().expect("temporary directory exists");
    let location = directory
        .path()
        .join("storage.sqlite")
        .to_string_lossy()
        .into_owned();
    let store = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(location).expect("temp location is absolute"),
    )
    .expect("database opens");
    (directory, store)
}

fn raw_connection(directory: &TempDir) -> sqlite::Connection {
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens for inspection");
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("foreign keys disable for raw inspection");
    connection
}

fn query_count(connection: &sqlite::Connection, sql: &str) -> i64 {
    connection
        .query_row(sql, [], |row| row.get(0))
        .expect("inspection count reads")
}

fn query_strings(connection: &sqlite::Connection, sql: &str) -> Vec<String> {
    let mut statement = connection.prepare(sql).expect("inspection query prepares");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("inspection query runs");
    rows.map(|row| row.expect("inspection row reads")).collect()
}

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

// ---------------------------------------------------------------------------
// Harness family.
// ---------------------------------------------------------------------------

fn harness_rule(harness_id: &str) -> HarnessRuleRecordDto {
    HarnessRuleRecordDto {
        harness_id: harness_id.to_owned(),
        scope: intention_storage::harness_repo::HarnessRuleScopeDto::Project {
            project_id: "project-harness".to_owned(),
        },
        lifecycle_state: HarnessRuleLifecycleStateDto::Active,
        active_revision: 1,
        service_session_id: format!("service-{harness_id}"),
        updated_at_ms: 10,
    }
}

fn harness_revision(harness_id: &str, revision: u64, seed: char) -> HarnessRuleRevisionRecordDto {
    HarnessRuleRevisionRecordDto {
        harness_id: harness_id.to_owned(),
        revision,
        task_digest: digest(seed),
        class: HarnessExecutionClassDto::Light,
        task_mode: HarnessTaskModeDto::RepeatedTask,
        presentation_mode: HarnessPresentationModeDto::JournalOnly,
        applied_time_zone: "UTC".to_owned(),
        source_kinds: vec![HarnessSourceKindDto::FixedInterval],
        source_references: vec!["source-calendar".to_owned()],
        interval_anchor_ms: Some(1_000),
        interval_ms: Some(60_000),
        calendar_expression: None,
        completion_link_reference: None,
        completion_outcomes: Vec::new(),
        canonical_revision_digest: digest(seed),
        created_at_ms: 11,
    }
}

#[test]
fn harness_rule_revision_lifecycle_and_count_round_trip() {
    let (_directory, store) = repository();
    let rule = harness_rule("harness-1");
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: rule.clone(),
            revision: harness_revision("harness-1", 1, 'a'),
        })
        .expect("harness rule creates");
    assert_eq!(
        store
            .load_harness_rule("harness-1".to_owned())
            .expect("rule loads"),
        rule
    );
    assert_eq!(
        store
            .load_harness_rule_revision("harness-1".to_owned(), 1)
            .expect("revision loads"),
        harness_revision("harness-1", 1, 'a')
    );
    assert_eq!(
        store
            .count_harness_rules("project-harness".to_owned())
            .expect("count loads"),
        1
    );

    // Creating the equal rule is a replay; unequal content is a conflict.
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: rule.clone(),
            revision: harness_revision("harness-1", 1, 'a'),
        })
        .expect("equal rule replays");
    let mut unequal = rule;
    unequal.service_session_id = "service-different".to_owned();
    assert_eq!(
        store
            .create_harness_rule(CreateHarnessRuleInputDto {
                rule: unequal,
                revision: harness_revision("harness-1", 1, 'a'),
            })
            .expect_err("unequal rule conflicts")
            .code(),
        "harness_revision_conflict"
    );

    let revised = store
        .revise_harness_rule(ReviseHarnessRuleInputDto {
            harness_id: "harness-1".to_owned(),
            expected_revision: 1,
            revision: harness_revision("harness-1", 2, 'b'),
        })
        .expect("revision two appends");
    assert_eq!(revised.active_revision, 2);
    assert_eq!(
        store
            .revise_harness_rule(ReviseHarnessRuleInputDto {
                harness_id: "harness-1".to_owned(),
                expected_revision: 1,
                revision: harness_revision("harness-1", 2, 'b'),
            })
            .expect_err("stale expected revision rejects")
            .code(),
        "harness_revision_conflict"
    );

    let paused = store
        .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
            harness_id: "harness-1".to_owned(),
            expected_revision: 2,
            operation: HarnessRuleOperationDto::Pause,
            has_active_run: false,
            occurred_at_ms: 20,
        })
        .expect("pause applies");
    assert_eq!(paused.lifecycle_state, HarnessRuleLifecycleStateDto::Paused);
    let resumed = store
        .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
            harness_id: "harness-1".to_owned(),
            expected_revision: 2,
            operation: HarnessRuleOperationDto::Resume,
            has_active_run: false,
            occurred_at_ms: 21,
        })
        .expect("resume applies");
    assert_eq!(
        resumed.lifecycle_state,
        HarnessRuleLifecycleStateDto::Active
    );
    let archived = store
        .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
            harness_id: "harness-1".to_owned(),
            expected_revision: 2,
            operation: HarnessRuleOperationDto::Archive,
            has_active_run: false,
            occurred_at_ms: 22,
        })
        .expect("archive applies");
    assert_eq!(
        archived.lifecycle_state,
        HarnessRuleLifecycleStateDto::Archived
    );
    assert_eq!(
        store
            .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
                harness_id: "harness-1".to_owned(),
                expected_revision: 2,
                operation: HarnessRuleOperationDto::Resume,
                has_active_run: false,
                occurred_at_ms: 23,
            })
            .expect_err("archived rule rejects transitions")
            .code(),
        "harness_archived"
    );
}

#[test]
fn harness_trigger_capture_coalescing_and_launch_consumption() {
    let (_directory, store) = repository();
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-trigger"),
            revision: harness_revision("harness-trigger", 1, 'a'),
        })
        .expect("harness rule creates");

    let captured = store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-trigger".to_owned(),
            reason_id: "reason-1".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 100,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: vec!["slot-1".to_owned()],
            catch_up_missed_slots: 0,
        })
        .expect("trigger captures");
    assert_eq!(captured.outcome, HarnessTriggerCaptureOutcomeDto::Captured);
    assert_eq!(captured.reason.coalesced_count, 1);
    assert_eq!(
        captured.reason.state,
        intention_storage::harness_repo::HarnessTriggerReasonStateDto::Pending
    );

    let redelivered = store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-trigger".to_owned(),
            reason_id: "reason-1".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 200,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: vec!["slot-1".to_owned()],
            catch_up_missed_slots: 0,
        })
        .expect("redelivery is safe");
    assert_eq!(
        redelivered.outcome,
        HarnessTriggerCaptureOutcomeDto::Redelivered
    );
    assert_eq!(redelivered.reason.coalesced_count, 1);
    assert_eq!(redelivered.reason.last_observed_at_ms, 100);

    let coalesced = store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-trigger".to_owned(),
            reason_id: "reason-2".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 300,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: vec!["slot-2".to_owned()],
            catch_up_missed_slots: 0,
        })
        .expect("coalescing capture succeeds");
    assert_eq!(
        coalesced.outcome,
        HarnessTriggerCaptureOutcomeDto::Coalesced
    );
    assert_eq!(coalesced.reason.coalesced_count, 2);
    assert_eq!(coalesced.reason.last_observed_at_ms, 300);

    let catch_up = store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-trigger".to_owned(),
            reason_id: "reason-3".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 400,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: vec!["slot-3".to_owned()],
            catch_up_missed_slots: 3,
        })
        .expect("catch-up capture succeeds");
    assert_eq!(catch_up.outcome, HarnessTriggerCaptureOutcomeDto::CatchUp);
    let pending = store
        .load_pending_harness_trigger("harness-trigger".to_owned())
        .expect("pending trigger loads")
        .expect("one pending trigger exists");
    assert_eq!(pending.reason_id, catch_up.reason.reason_id);

    let launch = store
        .record_harness_launch(RecordHarnessLaunchInputDto {
            harness_id: "harness-trigger".to_owned(),
            reason_id: pending.reason_id,
            cause_chain_depth: 1,
            direct_successor: false,
            occurred_at_ms: 500,
        })
        .expect("launch consumes the pending reason");
    assert_eq!(launch.counters.total_launches, 1);
    assert_eq!(launch.counters.concurrent_non_terminal, 1);
    assert!(
        store
            .load_pending_harness_trigger("harness-trigger".to_owned())
            .expect("pending trigger loads")
            .is_none()
    );
}

#[test]
fn harness_checkpoint_journal_and_dossier_round_trip() {
    let (_directory, store) = repository();
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-checkpoint"),
            revision: harness_revision("harness-checkpoint", 1, 'a'),
        })
        .expect("harness rule creates");

    let first = store
        .append_harness_journal_record(AppendHarnessJournalRecordInputDto {
            harness_id: "harness-checkpoint".to_owned(),
            record_kind: HarnessJournalRecordKindDto::TriggerCaptured,
            safe_summary: "captured one trigger".to_owned(),
            canonical_record_digest: digest('c'),
            occurred_at_ms: 30,
        })
        .expect("journal record appends");
    assert_eq!(first.sequence, 1);
    let second = store
        .append_harness_journal_record(AppendHarnessJournalRecordInputDto {
            harness_id: "harness-checkpoint".to_owned(),
            record_kind: HarnessJournalRecordKindDto::LaunchAdmitted,
            safe_summary: "admitted one launch".to_owned(),
            canonical_record_digest: digest('d'),
            occurred_at_ms: 31,
        })
        .expect("journal record appends");
    assert_eq!(second.sequence, 2);
    let page = store
        .load_harness_journal(
            "harness-checkpoint".to_owned(),
            LoadHarnessJournalInputDto {
                after_sequence: 0,
                limit: 1,
            },
        )
        .expect("journal page loads");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].sequence, 1);
    let tail = store
        .load_harness_journal(
            "harness-checkpoint".to_owned(),
            LoadHarnessJournalInputDto {
                after_sequence: 1,
                limit: 8,
            },
        )
        .expect("journal tail loads");
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].sequence, 2);

    let candidate = intention_storage::harness_repo::HarnessCheckpointRecordDto {
        checkpoint_id: "checkpoint-1".to_owned(),
        harness_id: "harness-checkpoint".to_owned(),
        rule_revision: 1,
        producing_run_id: "run-checkpoint".to_owned(),
        checkpoint_revision: 1,
        content_digest: digest('e'),
        checkpoint_bytes: 4_096,
        is_current: false,
        created_at_ms: 40,
    };
    let replaced = store
        .commit_harness_checkpoint(CommitHarnessCheckpointInputDto {
            harness_id: "harness-checkpoint".to_owned(),
            producing_run_id: "run-checkpoint".to_owned(),
            run_outcome: HarnessRunOutcomeDto::Completed,
            candidate: Some(candidate),
            occurred_at_ms: 41,
        })
        .expect("checkpoint commits");
    assert_eq!(
        replaced.disposition,
        intention_storage::harness_repo::HarnessCheckpointDispositionDto::Replaced
    );
    let current = store
        .load_current_harness_checkpoint("harness-checkpoint".to_owned())
        .expect("checkpoint loads")
        .expect("one current checkpoint exists");
    assert_eq!(current.checkpoint_id, "checkpoint-1");
    let retained = store
        .commit_harness_checkpoint(CommitHarnessCheckpointInputDto {
            harness_id: "harness-checkpoint".to_owned(),
            producing_run_id: "run-checkpoint".to_owned(),
            run_outcome: HarnessRunOutcomeDto::Failed,
            candidate: None,
            occurred_at_ms: 42,
        })
        .expect("failed run retains the previous checkpoint");
    assert_eq!(
        retained.disposition,
        intention_storage::harness_repo::HarnessCheckpointDispositionDto::RetainedPrevious
    );
    assert_eq!(
        retained
            .current
            .expect("retained checkpoint loads")
            .checkpoint_id,
        "checkpoint-1"
    );

    let dossier = HarnessDossierRecordDto {
        dossier_id: "dossier-1".to_owned(),
        harness_id: "harness-checkpoint".to_owned(),
        rule_revision: 1,
        reason_id: "reason-checkpoint".to_owned(),
        task_digest: digest('a'),
        source_references: vec!["source-calendar".to_owned()],
        typed_references: vec!["reference-1".to_owned()],
        checkpoint_reference: Some("checkpoint-1".to_owned()),
        dossier_bytes: 1_024,
        canonical_dossier_digest: digest('f'),
        created_at_ms: 43,
    };
    assert_eq!(
        store
            .store_harness_dossier(dossier.clone())
            .expect("dossier stores"),
        dossier
    );
    assert_eq!(
        store
            .load_harness_dossier("dossier-1".to_owned())
            .expect("dossier loads"),
        dossier
    );
}

// ---------------------------------------------------------------------------
// Programmatic-caller policy family.
// ---------------------------------------------------------------------------

fn policy(id: &str, revision: u64) -> ProgrammaticPolicyRecordDto {
    ProgrammaticPolicyRecordDto {
        policy_id: id.to_owned(),
        scope: ProgrammaticPolicyScopeDto::Project {
            project_id: "project-policy".to_owned(),
        },
        calendar_period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        lifecycle_state: ProgrammaticPolicyLifecycleStateDto::Active,
        active_revision: revision,
        canonical_policy_digest: digest('a'),
        updated_at_ms: 10,
    }
}

fn policy_revision(id: &str, revision: u64) -> ProgrammaticPolicyRevisionRecordDto {
    ProgrammaticPolicyRevisionRecordDto {
        policy_id: id.to_owned(),
        revision,
        root_origin_rules: vec![
            ProgrammaticRootOriginRuleRecordDto {
                root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
                maximum_decision: ProgrammaticAdmissionDecisionDto::DirectLocalRead,
            },
            ProgrammaticRootOriginRuleRecordDto {
                root_origin_kind: ProgrammaticRootOriginKindDto::ContinualHarness,
                maximum_decision: ProgrammaticAdmissionDecisionDto::Prohibited,
            },
        ],
        admission_decisions: vec![ProgrammaticAdmissionDecisionDto::DirectLocalRead],
        max_actions_per_run: 8,
        max_concurrent_actions_per_run: 2,
        calendar_period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        calendar_max_actions: 64,
        inherited_policy_references: Vec::new(),
        canonical_revision_digest: digest('b'),
    }
}

#[test]
fn programmatic_policy_revision_snapshot_and_lifecycle() {
    let (_directory, store) = repository();
    let created = store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-1", 1),
            revision: policy_revision("policy-1", 1),
        })
        .expect("policy creates");
    assert_eq!(
        created.lifecycle_state,
        ProgrammaticPolicyLifecycleStateDto::Active
    );
    assert_eq!(
        store
            .load_programmatic_policy_revision("policy-1".to_owned(), 1)
            .expect("revision loads"),
        policy_revision("policy-1", 1)
    );
    assert_eq!(
        store
            .count_programmatic_policies_in_project("project-policy".to_owned())
            .expect("project count loads"),
        1
    );

    let appended = store
        .append_programmatic_policy_revision(AppendProgrammaticPolicyRevisionInputDto {
            policy_id: "policy-1".to_owned(),
            expected_revision: 1,
            revision: policy_revision("policy-1", 2),
        })
        .expect("revision two appends");
    assert_eq!(appended.active_revision, 2);
    assert_eq!(
        store
            .append_programmatic_policy_revision(AppendProgrammaticPolicyRevisionInputDto {
                policy_id: "policy-1".to_owned(),
                expected_revision: 1,
                revision: policy_revision("policy-1", 2),
            })
            .expect_err("stale expected revision rejects")
            .code(),
        "programmatic_policy_revision_conflict"
    );

    let suspended = store
        .transition_programmatic_policy_lifecycle(TransitionProgrammaticPolicyLifecycleInputDto {
            policy_id: "policy-1".to_owned(),
            expected_revision: 2,
            operation: ProgrammaticPolicyLifecycleOperationDto::Suspend,
            has_active_dependent_tree: false,
            occurred_at_ms: 20,
        })
        .expect("suspend applies");
    assert_eq!(
        suspended.lifecycle_state,
        ProgrammaticPolicyLifecycleStateDto::Suspended
    );
    let resumed = store
        .transition_programmatic_policy_lifecycle(TransitionProgrammaticPolicyLifecycleInputDto {
            policy_id: "policy-1".to_owned(),
            expected_revision: 2,
            operation: ProgrammaticPolicyLifecycleOperationDto::Resume,
            has_active_dependent_tree: false,
            occurred_at_ms: 21,
        })
        .expect("resume applies");
    assert_eq!(
        resumed.lifecycle_state,
        ProgrammaticPolicyLifecycleStateDto::Active
    );

    let snapshot = ProgrammaticPolicySnapshotRecordDto {
        snapshot_id: "snapshot-1".to_owned(),
        root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
        policy_references: vec![ProgrammaticPolicyRevisionReferenceDto {
            policy_id: "policy-1".to_owned(),
            revision: 2,
        }],
        decision_ceiling: ProgrammaticAdmissionDecisionDto::DirectLocalRead,
        max_actions_per_run: 8,
        max_concurrent_actions_per_run: 2,
        calendar_limits: vec![ProgrammaticCalendarLimitRecordDto {
            period_kind: ProgrammaticCalendarPeriodKindDto::Day,
            max_actions: 64,
        }],
        calendar_counter_references: vec![ProgrammaticCalendarCounterReferenceDto {
            policy_id: "policy-1".to_owned(),
            period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        }],
        baseline_max_actions: Some(4),
        baseline_max_concurrent_actions: Some(1),
        snapshot_digest: digest('c'),
        created_at_ms: 25,
    };
    assert_eq!(
        store
            .store_programmatic_policy_snapshot(snapshot.clone())
            .expect("snapshot stores"),
        snapshot
    );
    assert_eq!(
        store
            .load_programmatic_policy_snapshot("snapshot-1".to_owned())
            .expect("snapshot loads"),
        snapshot
    );
}

#[test]
fn programmatic_policy_confirmation_corridor_and_draft() {
    let (_directory, store) = repository();
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-confirm", 1),
            revision: policy_revision("policy-confirm", 1),
        })
        .expect("policy creates");

    let confirmation = ProgrammaticPolicyConfirmationRecordDto {
        confirmation_id: "confirmation-1".to_owned(),
        root_session_id: "session-root".to_owned(),
        root_run_id: "run-root".to_owned(),
        tool_call_id: "call-1".to_owned(),
        tool_id: "tool-write".to_owned(),
        descriptor_revision: "descriptor-1".to_owned(),
        mcp_method_reference: None,
        typed_input_digest: digest('d'),
        policy_snapshot_digest: digest('e'),
        state: ProgrammaticConfirmationStateDto::Awaiting,
        created_at_ms: 30,
        decided_at_ms: None,
    };
    assert_eq!(
        store
            .create_programmatic_policy_confirmation(confirmation.clone())
            .expect("confirmation creates"),
        confirmation
    );
    assert_eq!(
        store
            .load_programmatic_policy_confirmation("call-1".to_owned())
            .expect("confirmation loads")
            .expect("confirmation exists"),
        confirmation
    );
    let decided = store
        .decide_programmatic_policy_confirmation(DecideProgrammaticPolicyConfirmationInputDto {
            confirmation_id: "confirmation-1".to_owned(),
            tool_call_id: "call-1".to_owned(),
            typed_input_digest: digest('d'),
            state: ProgrammaticConfirmationStateDto::Accepted,
            decided_at_ms: 31,
        })
        .expect("decision applies");
    assert_eq!(decided.state, ProgrammaticConfirmationStateDto::Accepted);
    assert_eq!(decided.decided_at_ms, Some(31));
    assert_eq!(
        store
            .decide_programmatic_policy_confirmation(DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: "confirmation-1".to_owned(),
                tool_call_id: "call-1".to_owned(),
                typed_input_digest: digest('d'),
                state: ProgrammaticConfirmationStateDto::Rejected,
                decided_at_ms: 32,
            })
            .expect_err("a decided confirmation rejects")
            .code(),
        "programmatic_policy_confirmation_expired"
    );

    let corridor = ProgrammaticAuthorizationCorridorRecordDto {
        corridor_digest: digest('f'),
        root_session_id: "session-root".to_owned(),
        root_run_id: "run-root".to_owned(),
        root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
        policy_snapshot_reference: "snapshot-1".to_owned(),
        required_effect_selectors: vec!["effect-write".to_owned()],
        exact_tool_or_method_selectors: vec!["tool-write".to_owned()],
        descriptor_input_constraint_selections: vec!["input-bounded".to_owned()],
        maximum_action_count: 2,
        maximum_concurrent_actions: 1,
        confirmation_reference: "confirmation-1".to_owned(),
        state: ProgrammaticCorridorStateDto::Active,
        consumed_action_count: 0,
        created_at_ms: 33,
    };
    assert_eq!(
        store
            .create_programmatic_authorization_corridor(corridor.clone())
            .expect("corridor creates"),
        corridor
    );
    let consumed = store
        .consume_programmatic_corridor_action(ConsumeProgrammaticCorridorActionInputDto {
            corridor_digest: digest('f'),
            root_run_id: "run-root".to_owned(),
            occurred_at_ms: 34,
        })
        .expect("corridor action consumes");
    assert_eq!(consumed.consumed_action_count, 1);
    assert_eq!(consumed.state, ProgrammaticCorridorStateDto::Active);
    let exhausted = store
        .consume_programmatic_corridor_action(ConsumeProgrammaticCorridorActionInputDto {
            corridor_digest: digest('f'),
            root_run_id: "run-root".to_owned(),
            occurred_at_ms: 35,
        })
        .expect("second action consumes");
    assert_eq!(exhausted.consumed_action_count, 2);
    assert_eq!(exhausted.state, ProgrammaticCorridorStateDto::Exhausted);
    assert_eq!(
        store
            .consume_programmatic_corridor_action(ConsumeProgrammaticCorridorActionInputDto {
                corridor_digest: digest('f'),
                root_run_id: "run-root".to_owned(),
                occurred_at_ms: 36,
            })
            .expect_err("an exhausted corridor rejects")
            .code(),
        "programmatic_policy_corridor_unavailable"
    );
    assert_eq!(
        store
            .close_programmatic_corridors_for_root(
                "run-root".to_owned(),
                ProgrammaticCorridorStateDto::Revoked,
                37
            )
            .expect("corridor closure applies"),
        1
    );

    let draft = ProgrammaticPolicyDraftRecordDto {
        draft_id: "draft-1".to_owned(),
        scope: ProgrammaticPolicyScopeDto::Project {
            project_id: "project-policy".to_owned(),
        },
        record_kind: "programmatic_policy".to_owned(),
        base_revision: 1,
        evidence_references: vec!["evidence-1".to_owned()],
        safe_rationale: "narrow the policy".to_owned(),
        canonical_draft_digest: digest('a'),
        state: ProgrammaticPolicyDraftStateDto::Pending,
        coalesced_evidence_count: 1,
        created_at_ms: 40,
        decided_at_ms: None,
    };
    let stored = store
        .propose_programmatic_policy_draft(draft.clone())
        .expect("draft proposes");
    assert_eq!(stored.draft_id, "draft-1");
    let mut coalesced = draft.clone();
    coalesced.draft_id = "draft-2".to_owned();
    coalesced.evidence_references = vec!["evidence-2".to_owned()];
    let stored = store
        .propose_programmatic_policy_draft(coalesced)
        .expect("equal draft coalesces");
    assert_eq!(stored.draft_id, "draft-1");
    assert_eq!(stored.evidence_references.len(), 2);
    assert_eq!(stored.coalesced_evidence_count, 2);
    let mut unequal = draft;
    unequal.canonical_draft_digest = digest('b');
    assert_eq!(
        store
            .propose_programmatic_policy_draft(unequal)
            .expect_err("unequal draft conflicts")
            .code(),
        "programmatic_policy_draft_conflict"
    );
    let decided = store
        .decide_programmatic_policy_draft(
            "draft-1".to_owned(),
            ProgrammaticPolicyDraftStateDto::Accepted,
            41,
        )
        .expect("draft decision applies");
    assert_eq!(decided.state, ProgrammaticPolicyDraftStateDto::Accepted);
    assert!(
        store
            .load_pending_programmatic_policy_draft(
                ProgrammaticPolicyScopeDto::Project {
                    project_id: "project-policy".to_owned(),
                },
                "programmatic_policy".to_owned(),
            )
            .expect("pending draft loads")
            .is_none()
    );
}

#[test]
fn programmatic_policy_reservation_replay_release_and_consumption() {
    let (_directory, store) = repository();
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-admit", 1),
            revision: policy_revision("policy-admit", 1),
        })
        .expect("policy creates");
    assert_eq!(
        store
            .load_programmatic_policy_counters("policy-admit".to_owned())
            .expect("counters load")
            .expect("counters exist")
            .run_started_actions,
        0
    );

    let reservation =
        |reference: &str, call: &str, digest_seed: char| ProgrammaticPolicyReservationRecordDto {
            reservation_reference: reference.to_owned(),
            policy_id: "policy-admit".to_owned(),
            policy_revision: 1,
            root_run_id: "run-admit".to_owned(),
            tool_call_id: call.to_owned(),
            typed_input_digest: digest(digest_seed),
            calendar_counter_reference: "policy-admit:day".to_owned(),
            reserved_at_ms: 50,
            state: ProgrammaticReservationStateDto::Reserved,
            finished_at_ms: None,
        };
    let input = ReserveProgrammaticPolicyActionInputDto {
        reservation: reservation("reservation-1", "call-admit-1", 'a'),
        max_actions_per_run: 2,
        max_concurrent_actions_per_run: 1,
        calendar_max_actions: 8,
        calendar_window_start_ms: 0,
        calendar_window_end_ms: 86_400_000,
        calendar_window_time_zone: "UTC".to_owned(),
    };
    let reserved = store
        .reserve_programmatic_policy_action(input.clone())
        .expect("reservation commits");
    assert!(!reserved.replayed);
    assert_eq!(reserved.counters.run_reserved_actions, 1);
    assert_eq!(reserved.counters.calendar_reserved_actions, 1);

    let replayed = store
        .reserve_programmatic_policy_action(input)
        .expect("equal replay returns the binding");
    assert!(replayed.replayed);
    assert_eq!(replayed.counters.run_reserved_actions, 1);
    assert_eq!(
        replayed.reservation.state,
        ProgrammaticReservationStateDto::Reserved
    );

    let mut different = reservation("reservation-2", "call-admit-1", 'b');
    different.reserved_at_ms = 51;
    assert_eq!(
        store
            .reserve_programmatic_policy_action(ReserveProgrammaticPolicyActionInputDto {
                reservation: different,
                max_actions_per_run: 2,
                max_concurrent_actions_per_run: 1,
                calendar_max_actions: 8,
                calendar_window_start_ms: 0,
                calendar_window_end_ms: 86_400_000,
                calendar_window_time_zone: "UTC".to_owned(),
            })
            .expect_err("the same call cannot bind different input")
            .code(),
        "programmatic_policy_reservation_conflict"
    );

    let released = store
        .release_programmatic_policy_reservation(ReleaseProgrammaticPolicyReservationInputDto {
            reservation_reference: "reservation-1".to_owned(),
            tool_call_id: "call-admit-1".to_owned(),
            released_at_ms: 60,
        })
        .expect("release applies");
    assert_eq!(
        released.state,
        ProgrammaticReservationStateDto::ReleasedOnKnownPreEffect
    );

    let second = store
        .reserve_programmatic_policy_action(ReserveProgrammaticPolicyActionInputDto {
            reservation: reservation("reservation-2", "call-admit-2", 'c'),
            max_actions_per_run: 2,
            max_concurrent_actions_per_run: 1,
            calendar_max_actions: 8,
            calendar_window_start_ms: 0,
            calendar_window_end_ms: 86_400_000,
            calendar_window_time_zone: "UTC".to_owned(),
        })
        .expect("second reservation commits");
    let started = store
        .commit_programmatic_reservation_started(CommitProgrammaticReservationStartedInputDto {
            reservation_reference: second.reservation.reservation_reference,
            tool_call_id: "call-admit-2".to_owned(),
            started_at_ms: 70,
        })
        .expect("start consumes permanently");
    assert_eq!(
        started.state,
        ProgrammaticReservationStateDto::PermanentOnStart
    );
    let counters = store
        .load_programmatic_policy_counters("policy-admit".to_owned())
        .expect("counters load")
        .expect("counters exist");
    assert_eq!(counters.run_started_actions, 1);
    assert_eq!(counters.run_in_flight_actions, 1);
    assert_eq!(counters.run_reserved_actions, 0);

    let recovered = store
        .recover_programmatic_policy_reservation(RecoverProgrammaticPolicyReservationInputDto {
            reservation_reference: "reservation-2".to_owned(),
            tool_call_started: true,
            recovered_at_ms: 80,
        })
        .expect("recovery records unknown external effect");
    assert_eq!(
        recovered.state,
        ProgrammaticReservationStateDto::ExternalEffectUnknown
    );

    assert_eq!(
        store
            .reserve_programmatic_policy_action(ReserveProgrammaticPolicyActionInputDto {
                reservation: reservation("reservation-3", "call-admit-3", 'd'),
                max_actions_per_run: 1,
                max_concurrent_actions_per_run: 1,
                calendar_max_actions: 8,
                calendar_window_start_ms: 0,
                calendar_window_end_ms: 86_400_000,
                calendar_window_time_zone: "UTC".to_owned(),
            })
            .expect_err("the run action bound rejects")
            .code(),
        "programmatic_policy_run_limit_exceeded"
    );
}

// ---------------------------------------------------------------------------
// Goal family.
// ---------------------------------------------------------------------------

fn goal(id: &str, active_revision: u64) -> GoalRecordDto {
    GoalRecordDto {
        goal_id: id.to_owned(),
        scope: GoalScopeDto::Project {
            project_id: "project-goal".to_owned(),
        },
        active_revision,
        lifecycle_state: GoalLifecycleStateDto::Active,
        readiness_state: GoalReadinessStateDto::NotReady,
        user_decision_state: GoalUserDecisionStateDto::Unaccepted,
        created_at_ms: 10,
        updated_at_ms: 10,
    }
}

fn goal_revision(id: &str, revision: u64, seed: char) -> GoalRevisionRecordDto {
    GoalRevisionRecordDto {
        goal_id: id.to_owned(),
        revision,
        title: "Goal title".to_owned(),
        objective: "Goal objective".to_owned(),
        inherited_rule_references: vec!["rule-inherited".to_owned()],
        local_rule_references: vec!["rule-local".to_owned()],
        required_gate_references: Vec::new(),
        canonical_revision_digest: digest(seed),
        created_at_ms: 11,
    }
}

fn evidence(id: &str, kind: GoalEvidenceKindDto) -> GoalEvidenceReferenceDto {
    GoalEvidenceReferenceDto {
        evidence_id: id.to_owned(),
        revision: 1,
        kind,
    }
}

#[test]
fn goal_revision_tree_and_session_link_round_trip() {
    let (_directory, store) = repository();
    let parent = store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-parent", 1),
            revision: goal_revision("goal-parent", 1, 'a'),
        })
        .expect("parent Goal creates");
    assert_eq!(parent.goal_id, "goal-parent");
    let child = store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-child", 1),
            revision: goal_revision("goal-child", 1, 'b'),
        })
        .expect("child Goal creates");
    assert_eq!(child.goal_id, "goal-child");
    assert_eq!(
        store
            .count_goals_in_project("project-goal".to_owned())
            .expect("project count loads"),
        2
    );

    let link = store
        .attach_goal_child(AttachGoalChildInputDto {
            parent_goal_id: "goal-parent".to_owned(),
            child_goal_id: "goal-child".to_owned(),
            child_revision_at_link: 1,
            canonical_link_digest: digest('c'),
            created_at_ms: 20,
        })
        .expect("child attaches");
    assert_eq!(link.child_goal_id, "goal-child");
    assert_eq!(
        store
            .list_goal_children("goal-parent".to_owned())
            .expect("children load"),
        vec![link]
    );
    assert_eq!(
        store
            .load_goal_tree_depth("goal-child".to_owned())
            .expect("depth loads"),
        2
    );
    assert_eq!(
        store
            .load_goal_tree_depth("goal-parent".to_owned())
            .expect("root depth loads"),
        1
    );
    assert_eq!(
        store
            .attach_goal_child(AttachGoalChildInputDto {
                parent_goal_id: "goal-child".to_owned(),
                child_goal_id: "goal-parent".to_owned(),
                child_revision_at_link: 1,
                canonical_link_digest: digest('d'),
                created_at_ms: 21,
            })
            .expect_err("a cycle rejects")
            .code(),
        "goal_cycle_detected"
    );

    let appending = store
        .append_goal_revision(AppendGoalRevisionInputDto {
            goal_id: "goal-parent".to_owned(),
            expected_revision: 1,
            revision: goal_revision("goal-parent", 2, 'e'),
        })
        .expect("revision two appends");
    assert_eq!(appending.active_revision, 2);
    assert_eq!(
        store
            .load_goal_revision("goal-parent".to_owned(), 2)
            .expect("revision two loads"),
        goal_revision("goal-parent", 2, 'e')
    );
    assert_eq!(
        store
            .append_goal_revision(AppendGoalRevisionInputDto {
                goal_id: "goal-parent".to_owned(),
                expected_revision: 1,
                revision: goal_revision("goal-parent", 2, 'e'),
            })
            .expect_err("stale expected revision rejects")
            .code(),
        "goal_revision_conflict"
    );

    let session_link = GoalSessionLinkRecordDto {
        link_id: "link-1".to_owned(),
        project_goal_id: "goal-parent".to_owned(),
        session_id: "session-goal".to_owned(),
        effective_from_revision: 2,
        canonical_link_digest: digest('f'),
        created_at_ms: 22,
    };
    assert_eq!(
        store
            .create_goal_session_link(CreateGoalSessionLinkInputDto {
                link: session_link.clone()
            })
            .expect("session link creates"),
        session_link
    );
    assert_eq!(
        store
            .load_goal_session_link("link-1".to_owned())
            .expect("session link loads"),
        session_link
    );
    assert_eq!(
        store
            .load_goal_session_links("goal-parent".to_owned())
            .expect("session links load"),
        vec![session_link]
    );
}

#[test]
fn goal_lifecycle_readiness_and_user_decision() {
    let (_directory, store) = repository();
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-decision", 1),
            revision: goal_revision("goal-decision", 1, 'a'),
        })
        .expect("Goal creates");

    let ready = store
        .set_goal_readiness(SetGoalReadinessInputDto {
            goal_id: "goal-decision".to_owned(),
            expected_revision: 1,
            readiness_state: GoalReadinessStateDto::Ready {
                verified_evidence_set: vec![evidence(
                    "evidence-1",
                    GoalEvidenceKindDto::AcceptedUserDeclaration,
                )],
            },
            occurred_at_ms: 30,
        })
        .expect("readiness records");
    assert!(ready.readiness_state.is_ready());
    let accepted = store
        .record_goal_user_decision(RecordGoalUserDecisionInputDto {
            goal_id: "goal-decision".to_owned(),
            expected_revision: 1,
            user_decision_state: GoalUserDecisionStateDto::Accepted,
            occurred_at_ms: 31,
        })
        .expect("acceptance records");
    assert_eq!(
        accepted.user_decision_state,
        GoalUserDecisionStateDto::Accepted
    );

    // A non-Ready Goal cannot be plainly accepted.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-unready", 1),
            revision: goal_revision("goal-unready", 1, 'b'),
        })
        .expect("Goal creates");
    assert_eq!(
        store
            .record_goal_user_decision(RecordGoalUserDecisionInputDto {
                goal_id: "goal-unready".to_owned(),
                expected_revision: 1,
                user_decision_state: GoalUserDecisionStateDto::Accepted,
                occurred_at_ms: 32,
            })
            .expect_err("plain acceptance requires readiness")
            .code(),
        "goal_not_ready"
    );
    let exception = store
        .record_goal_user_decision(RecordGoalUserDecisionInputDto {
            goal_id: "goal-unready".to_owned(),
            expected_revision: 1,
            user_decision_state: GoalUserDecisionStateDto::AcceptedWithException {
                exception_evidence_set: vec![GoalGateExceptionDto {
                    gate_id: "gate-1".to_owned(),
                    gate_revision: 1,
                    kind: GoalGateExceptionKindDto::Unavailable,
                    evidence: evidence("evidence-2", GoalEvidenceKindDto::ExecutableGateResult),
                }],
            },
            occurred_at_ms: 33,
        })
        .expect("an exception acceptance records");
    assert!(exception.user_decision_state.is_terminal());

    let rework = store
        .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
            goal_id: "goal-decision".to_owned(),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::NeedsRework,
            occurred_at_ms: 34,
        })
        .expect("rework transition applies");
    assert_eq!(rework.lifecycle_state, GoalLifecycleStateDto::NeedsRework);
    let active = store
        .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
            goal_id: "goal-decision".to_owned(),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Active,
            occurred_at_ms: 35,
        })
        .expect("recovery transition applies");
    assert_eq!(active.lifecycle_state, GoalLifecycleStateDto::Active);
    let archived = store
        .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
            goal_id: "goal-decision".to_owned(),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Archived,
            occurred_at_ms: 36,
        })
        .expect("a user-accepted Goal archives");
    assert_eq!(archived.lifecycle_state, GoalLifecycleStateDto::Archived);
    assert_eq!(
        store
            .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
                goal_id: "goal-decision".to_owned(),
                expected_revision: 1,
                lifecycle_state: GoalLifecycleStateDto::Archived,
                occurred_at_ms: 37,
            })
            .expect("archive is idempotent")
            .lifecycle_state,
        GoalLifecycleStateDto::Archived
    );

    // A live unaccepted Goal cannot be archived.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-live", 1),
            revision: goal_revision("goal-live", 1, 'c'),
        })
        .expect("Goal creates");
    assert_eq!(
        store
            .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
                goal_id: "goal-live".to_owned(),
                expected_revision: 1,
                lifecycle_state: GoalLifecycleStateDto::Archived,
                occurred_at_ms: 38,
            })
            .expect_err("a live Goal is not archivable")
            .code(),
        "goal_archive_not_terminal"
    );
}

#[test]
fn goal_gate_template_and_result_round_trip() {
    let (_directory, store) = repository();
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-gate", 1),
            revision: goal_revision("goal-gate", 1, 'a'),
        })
        .expect("Goal creates");

    let gate = GoalGateRecordDto {
        gate_id: "gate-1".to_owned(),
        goal_id: "goal-gate".to_owned(),
        definition: GoalGateDefinitionDto::Reference {
            evidence_contract_revision: 1,
            accepted_reference_kinds: vec![GoalEvidenceKindDto::ExecutableGateResult],
        },
        revision: 1,
        canonical_revision_digest: digest('b'),
        created_at_ms: 40,
    };
    assert_eq!(
        store
            .create_goal_gate(CreateGoalGateInputDto { gate: gate.clone() })
            .expect("gate creates"),
        gate
    );
    let executable = GoalGateDefinitionDto::Executable {
        template_id: "template-1".to_owned(),
        template_revision: 1,
    };
    let revised = store
        .append_goal_gate_revision(AppendGoalGateRevisionInputDto {
            gate_id: "gate-1".to_owned(),
            expected_revision: 1,
            definition: executable.clone(),
            canonical_revision_digest: digest('c'),
            occurred_at_ms: 41,
        })
        .expect("gate revision two appends");
    assert_eq!(revised.revision, 2);
    assert_eq!(revised.definition, executable);
    assert_eq!(
        store
            .append_goal_gate_revision(AppendGoalGateRevisionInputDto {
                gate_id: "gate-1".to_owned(),
                expected_revision: 1,
                definition: executable,
                canonical_revision_digest: digest('c'),
                occurred_at_ms: 42,
            })
            .expect_err("stale gate revision rejects")
            .code(),
        "goal_revision_conflict"
    );

    let template = GoalGateTemplateRecordDto {
        template_id: "template-1".to_owned(),
        revision: 1,
        scope: GoalTemplateScopeDto::Goal {
            goal_id: "goal-gate".to_owned(),
        },
        capability_reference: "capability-check".to_owned(),
        input_family: intention_storage::goal_repo::GoalGateInputFamilyDto::ClosedTextV1,
        requires_confirmation: false,
        lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
        provenance_kind: GoalTemplateProvenanceKindDto::User,
        provenance_draft_id: None,
        provenance_accepted_by_user: false,
        canonical_digest: digest('d'),
        created_at_ms: 43,
    };
    assert_eq!(
        store
            .create_goal_gate_template(template.clone())
            .expect("template creates"),
        template
    );
    assert_eq!(
        store
            .load_goal_gate_template("template-1".to_owned(), 1)
            .expect("template loads"),
        template
    );
    let archived = store
        .transition_goal_gate_template_lifecycle(TransitionGoalGateTemplateInputDto {
            template_id: "template-1".to_owned(),
            revision: 1,
            lifecycle_state: GoalTemplateLifecycleStateDto::Archived,
            occurred_at_ms: 44,
        })
        .expect("template archives");
    assert_eq!(
        archived.lifecycle_state,
        GoalTemplateLifecycleStateDto::Archived
    );
    let restored = store
        .transition_goal_gate_template_lifecycle(TransitionGoalGateTemplateInputDto {
            template_id: "template-1".to_owned(),
            revision: 1,
            lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
            occurred_at_ms: 45,
        })
        .expect("template restores");
    assert_eq!(
        restored.lifecycle_state,
        GoalTemplateLifecycleStateDto::Enabled
    );

    let result = GoalGateResultRecordDto {
        gate_id: "gate-1".to_owned(),
        gate_revision: 2,
        producing_run_id: "run-gate".to_owned(),
        outcome_kind: intention_storage::goal_repo::GoalGateOutcomeKindDto::Passed,
        disposition: intention_storage::goal_repo::GoalGateOutcomeDispositionDto::Passed,
        evidence: Some(evidence(
            "evidence-gate",
            GoalEvidenceKindDto::ExecutableGateResult,
        )),
        canonical_result_digest: digest('e'),
        occurred_at_ms: 46,
    };
    assert_eq!(
        store
            .record_goal_gate_result(result.clone())
            .expect("gate result records"),
        result
    );
    assert_eq!(
        store
            .load_goal_gate_result("gate-1".to_owned(), 2)
            .expect("gate result loads"),
        result
    );
    let mut unequal = result;
    unequal.canonical_result_digest = digest('f');
    assert_eq!(
        store
            .record_goal_gate_result(unequal)
            .expect_err("an unequal duplicate result rejects")
            .code(),
        "goal_revision_conflict"
    );
}

#[test]
fn goal_card_replacement_rollback_and_scopes() {
    let (_directory, store) = repository();
    let scope = GoalRecordScopeDto::Goal {
        goal_id: "goal-cards".to_owned(),
    };
    let card = |record_id: &str, revision: u64, seed: char| GoalMemoryCardRecordDto {
        record_id: record_id.to_owned(),
        revision,
        kind: MemoryKindDto::Fact,
        scope: scope.clone(),
        title: "Memory title".to_owned(),
        safe_purpose: "Memory purpose".to_owned(),
        retained_content_reference: format!("content-{record_id}"),
        canonical_digest: digest(seed),
        created_at_ms: 50,
    };
    assert_eq!(
        store
            .store_goal_memory_card(card("memory-1", 1, 'a'))
            .expect("memory card stores"),
        card("memory-1", 1, 'a')
    );
    store
        .store_goal_memory_card(card("memory-1", 2, 'b'))
        .expect("a newer revision stores");
    assert_eq!(
        store
            .count_active_goal_memory_cards(scope.clone())
            .expect("active count loads"),
        1
    );
    assert_eq!(
        store
            .load_goal_memory_card("memory-1".to_owned(), 2)
            .expect("memory revision loads"),
        card("memory-1", 2, 'b')
    );

    let replacement = store
        .replace_goal_memory_card(GoalMemoryCardReplacementInputDto {
            replaced_record_id: "memory-1".to_owned(),
            replaced_revision: 2,
            replacement: card("memory-2", 1, 'c'),
            occurred_at_ms: 51,
        })
        .expect("replacement commits");
    assert_eq!(replacement.record_id, "memory-2");
    assert_eq!(
        store
            .count_active_goal_memory_cards(scope.clone())
            .expect("active count loads"),
        2
    );
    assert_eq!(
        store
            .replace_goal_memory_card(GoalMemoryCardReplacementInputDto {
                replaced_record_id: "memory-missing".to_owned(),
                replaced_revision: 1,
                replacement: card("memory-3", 1, 'd'),
                occurred_at_ms: 52,
            })
            .expect_err("a missing replaced revision rejects")
            .code(),
        "memory_replacement_conflict"
    );

    let rolled_back = store
        .rollback_goal_memory_card(GoalMemoryCardRollbackInputDto {
            restored_record_id: "memory-1".to_owned(),
            restored_revision: 1,
            replacement: card("memory-1", 3, 'e'),
            occurred_at_ms: 53,
        })
        .expect("rollback commits");
    assert_eq!(rolled_back.revision, 3);

    let skill = GoalSkillCardRecordDto {
        skill_id: "skill-1".to_owned(),
        revision: 1,
        canonical_name: "bounded-research".to_owned(),
        description: "Route bounded research work".to_owned(),
        owner_scope: GoalRecordScopeDto::Project {
            project_id: "project-goal".to_owned(),
        },
        content_reference: "skill-content-1".to_owned(),
        canonical_digest: digest('f'),
        created_at_ms: 54,
    };
    assert_eq!(
        store
            .store_goal_skill_card(skill.clone())
            .expect("Skill card stores"),
        skill
    );
    assert_eq!(
        store
            .load_goal_skill_card("skill-1".to_owned(), 1)
            .expect("Skill card loads"),
        skill
    );

    let role = GoalRoleCardRecordDto {
        role_id: "role-1".to_owned(),
        revision: 1,
        canonical_name: "narrow-reviewer".to_owned(),
        task: "Review one bounded change".to_owned(),
        permitted_class: GoalRoleClassDto::Light,
        tool_subset: vec!["read".to_owned(), "grep".to_owned()],
        context_limit_bytes: 4_096,
        result_limit_bytes: 2_048,
        canonical_digest: digest('a'),
        created_at_ms: 55,
    };
    assert_eq!(
        store
            .store_goal_role_card(role.clone())
            .expect("role card stores"),
        role
    );
    assert_eq!(
        store
            .load_goal_role_card("role-1".to_owned(), 1)
            .expect("role card loads"),
        role
    );
    let mut widened = role;
    widened.context_limit_bytes = 8_192;
    assert_eq!(
        store
            .store_goal_role_card(widened)
            .expect_err("an unequal duplicate role rejects")
            .code(),
        "delegation_role_invalid"
    );
}

#[test]
fn refinement_draft_coalescing_and_decision() {
    let (_directory, store) = repository();
    let draft = RefinementDraftRecordDto {
        draft_id: "draft-refine-1".to_owned(),
        source_run_id: "run-refine".to_owned(),
        leading_goal_id: "goal-refine".to_owned(),
        milestone: GoalMilestoneDto::TechnicalReadiness,
        base_goal_revision: 1,
        base_record_reference: "goal-refine".to_owned(),
        base_record_revision: 1,
        edits: vec![RefinementEditRecordDto {
            kind: RefinementEditKindDto::ReadinessClaim,
            evidence: evidence("evidence-refine", GoalEvidenceKindDto::TerminalChildResult),
        }],
        evidence_references: vec![evidence(
            "evidence-refine",
            GoalEvidenceKindDto::TerminalChildResult,
        )],
        safe_rationale: "claim readiness".to_owned(),
        canonical_digest: digest('b'),
        state: RefinementDraftStateDto::Pending,
        coalesced_evidence_count: 1,
        created_at_ms: 60,
        decided_at_ms: None,
    };
    assert_eq!(
        store
            .propose_refinement_draft(draft.clone())
            .expect("draft proposes"),
        draft
    );
    let mut coalescing = draft.clone();
    coalescing.draft_id = "draft-refine-2".to_owned();
    coalescing.evidence_references = vec![evidence(
        "evidence-refine-2",
        GoalEvidenceKindDto::ExecutableGateResult,
    )];
    let coalesced = store
        .propose_refinement_draft(coalescing)
        .expect("an equal proposal coalesces");
    assert_eq!(coalesced.draft_id, "draft-refine-1");
    assert_eq!(coalesced.evidence_references.len(), 2);
    assert_eq!(coalesced.coalesced_evidence_count, 2);
    let mut unequal = draft;
    unequal.canonical_digest = digest('c');
    assert_eq!(
        store
            .propose_refinement_draft(unequal)
            .expect_err("an unequal proposal conflicts")
            .code(),
        "refinement_draft_conflict"
    );
    assert_eq!(
        store
            .load_pending_refinement_draft("goal-refine".to_owned())
            .expect("pending draft loads")
            .expect("one pending draft exists")
            .draft_id,
        "draft-refine-1"
    );
    let decided = store
        .decide_refinement_draft(
            "draft-refine-1".to_owned(),
            RefinementDraftStateDto::Rejected,
            61,
        )
        .expect("decision applies");
    assert_eq!(decided.state, RefinementDraftStateDto::Rejected);
    assert_eq!(decided.decided_at_ms, Some(61));
    assert!(
        store
            .load_pending_refinement_draft("goal-refine".to_owned())
            .expect("pending draft loads")
            .is_none()
    );
    assert_eq!(
        store
            .decide_refinement_draft(
                "draft-refine-1".to_owned(),
                RefinementDraftStateDto::Accepted,
                62,
            )
            .expect_err("a decided draft rejects")
            .code(),
        "refinement_draft_conflict"
    );
}

#[test]
fn compaction_working_form_and_summary_chain() {
    let (_directory, store) = repository();
    let scope = GoalRecordScopeDto::Goal {
        goal_id: "goal-compact".to_owned(),
    };
    for (reference, time) in [("history-1", 70), ("history-2", 71), ("history-3", 72)] {
        store
            .record_compaction_suffix_reference(RecordCompactionSuffixReferenceInputDto {
                scope: scope.clone(),
                history_reference: reference.to_owned(),
                occurred_at_ms: time,
            })
            .expect("history reference appends");
    }
    // Replaying an equal reference is idempotent.
    store
        .record_compaction_suffix_reference(RecordCompactionSuffixReferenceInputDto {
            scope: scope.clone(),
            history_reference: "history-1".to_owned(),
            occurred_at_ms: 73,
        })
        .expect("duplicate history reference is idempotent");
    let working = store
        .load_goal_compaction_working_form(scope.clone())
        .expect("working form loads");
    assert_eq!(
        working,
        GoalCompactionWorkingFormRecordDto {
            current_summary: None,
            uncompacted_suffix: vec![
                "history-1".to_owned(),
                "history-2".to_owned(),
                "history-3".to_owned(),
            ],
        }
    );

    let summary = ConversationSummaryRecordDto {
        summary_id: "summary-1".to_owned(),
        revision: 1,
        scope: scope.clone(),
        previous_summary_reference: None,
        source_range_start: "history-1".to_owned(),
        source_range_end: "history-2".to_owned(),
        safe_content: "compacted summary".to_owned(),
        canonical_digest: digest('d'),
        created_at_ms: 80,
    };
    assert_eq!(
        store
            .store_conversation_summary(summary.clone())
            .expect("summary stores"),
        summary
    );
    assert_eq!(
        store
            .load_conversation_summary("summary-1".to_owned(), 1)
            .expect("summary loads"),
        summary
    );
    let working = store
        .load_goal_compaction_working_form(scope.clone())
        .expect("working form loads");
    assert_eq!(
        working
            .current_summary
            .as_ref()
            .expect("summary current")
            .summary_id,
        "summary-1"
    );
    assert_eq!(working.uncompacted_suffix, vec!["history-3".to_owned()]);

    let next = ConversationSummaryRecordDto {
        summary_id: "summary-2".to_owned(),
        revision: 2,
        scope: scope.clone(),
        previous_summary_reference: Some("summary-1".to_owned()),
        source_range_start: "history-3".to_owned(),
        source_range_end: "history-3".to_owned(),
        safe_content: "compacted suffix".to_owned(),
        canonical_digest: digest('e'),
        created_at_ms: 81,
    };
    store
        .store_conversation_summary(next)
        .expect("summary two stores");
    let working = store
        .load_goal_compaction_working_form(scope.clone())
        .expect("working form loads");
    assert_eq!(
        working.current_summary.expect("summary current").summary_id,
        "summary-2"
    );
    assert!(working.uncompacted_suffix.is_empty());

    // A predecessor mismatch is a typed rejection.
    assert_eq!(
        store
            .store_conversation_summary(ConversationSummaryRecordDto {
                summary_id: "summary-3".to_owned(),
                revision: 3,
                scope,
                previous_summary_reference: Some("summary-1".to_owned()),
                source_range_start: "history-4".to_owned(),
                source_range_end: "history-4".to_owned(),
                safe_content: "stale predecessor".to_owned(),
                canonical_digest: digest('f'),
                created_at_ms: 82,
            })
            .expect_err("a stale predecessor rejects")
            .code(),
        "compaction_history_unavailable"
    );
}

// ---------------------------------------------------------------------------
// Unified verification family.
// ---------------------------------------------------------------------------

fn authority(
    authority_id: &str,
    rule: VerifierAuthorityConsumptionRuleDto,
) -> VerifierAuthorityRecordDto {
    VerifierAuthorityRecordDto {
        authority_id: authority_id.to_owned(),
        authority_revision: 1,
        verifier_mandate_id: "verifier-mandate-1".to_owned(),
        immutable_target_set_reference: VerifierTargetSetReferenceDto {
            target_set_id: "target-set-1".to_owned(),
            canonical_target_set_digest: digest('a'),
        },
        allowed_operations: vec![
            VerifierOperationDto::MarkComplete,
            VerifierOperationDto::Stop,
        ],
        audit_contract_reference: VerifierContractReferenceDto {
            contract_id: "audit-contract-1".to_owned(),
            contract_revision: 1,
            canonical_contract_digest: digest('b'),
        },
        issued_at_ms: 100,
        expires_at_ms: Some(10_000),
        revoked_at_ms: None,
        revocation_reference: None,
        consumption_rule: rule,
        consumption_state: VerifierAuthorityConsumptionStateDto::Unconsumed,
        consumed_by_mutation_reference: None,
        canonical_authority_digest: digest('c'),
    }
}

fn baseline(authority: &VerifierAuthorityRecordDto) -> VerifierAuditBaselineRecordDto {
    VerifierAuditBaselineRecordDto {
        authority_reference: VerifierAuthorityReferenceDto {
            authority_id: authority.authority_id.clone(),
            authority_revision: authority.authority_revision,
            canonical_authority_digest: authority.canonical_authority_digest.clone(),
        },
        verifier_mandate_revision: 1,
        target_mandate_id: "target-mandate-1".to_owned(),
        target_revision: 5,
        target_sequence: 3,
        target_lifecycle: VerifierTargetLifecycleDto::Active,
        frozen_references: VerifierFrozenReferencesDto {
            goal_references: vec![VerifierGoalReferenceDto {
                goal_id: "goal-verify".to_owned(),
                goal_revision: 1,
                canonical_goal_revision_digest: digest('d'),
            }],
            contract_references: vec![VerifierContractReferenceDto {
                contract_id: "gate-contract-1".to_owned(),
                contract_revision: 1,
                canonical_contract_digest: digest('e'),
            }],
        },
        optional_unknown_effect_reference: None,
        audit_contract_reference: authority.audit_contract_reference.clone(),
        graph_epoch: None,
        canonical_baseline_digest: digest('f'),
        created_at_ms: 110,
    }
}

fn mutation(
    authority: &VerifierAuthorityRecordDto,
    mutation_id: &str,
    operation_id: &str,
    created_at_ms: u64,
) -> VerifierTargetMutationRecordDto {
    VerifierTargetMutationRecordDto {
        mutation_id: mutation_id.to_owned(),
        authority_reference: VerifierAuthorityReferenceDto {
            authority_id: authority.authority_id.clone(),
            authority_revision: authority.authority_revision,
            canonical_authority_digest: authority.canonical_authority_digest.clone(),
        },
        audit_contract_reference: authority.audit_contract_reference.clone(),
        target_reference: VerifierTargetReferenceDto {
            target_mandate_id: "target-mandate-1".to_owned(),
            target_revision: 5,
        },
        operation: VerifierOperationDto::MarkComplete,
        audit_evidence_references: vec!["evidence-audit-1".to_owned()],
        expected_target_revision: 5,
        expected_target_sequence: 3,
        expected_baseline_digest: digest('f'),
        idempotency: VerifierOperationIdentityDto {
            operation_id: operation_id.to_owned(),
            operation_digest: digest('a'),
        },
        canonical_mutation_digest: digest('b'),
        created_at_ms,
    }
}

#[test]
fn verifier_records_and_target_mutation_apply() {
    let (_directory, store) = repository();
    let single_use = authority(
        "authority-1",
        VerifierAuthorityConsumptionRuleDto::SingleUse,
    );
    assert_eq!(
        store
            .record_verifier_authority(single_use.clone())
            .expect("authority stores"),
        single_use
    );
    assert_eq!(
        store
            .load_verifier_authority("authority-1".to_owned(), 1)
            .expect("authority loads"),
        single_use
    );
    let audit_baseline = baseline(&single_use);
    assert_eq!(
        store
            .record_verifier_audit_baseline(audit_baseline.clone())
            .expect("baseline stores"),
        audit_baseline
    );
    assert_eq!(
        store
            .load_verifier_audit_baseline(digest('f'))
            .expect("baseline loads"),
        audit_baseline
    );
    let evidence = VerifierAuditEvidenceRecordDto {
        evidence_id: "evidence-audit-1".to_owned(),
        authority_reference: audit_baseline.authority_reference.clone(),
        target_reference: VerifierTargetReferenceDto {
            target_mandate_id: audit_baseline.target_mandate_id.clone(),
            target_revision: audit_baseline.target_revision,
        },
        frozen_references: audit_baseline.frozen_references.clone(),
        evidence_kind: VerifierEvidenceKindDto::UnconditionalPass,
        retained_content_reference: "retained-evidence-1".to_owned(),
        canonical_evidence_digest: digest('c'),
        created_at_ms: 111,
    };
    assert_eq!(
        store
            .record_verifier_audit_evidence(evidence.clone())
            .expect("evidence stores"),
        evidence
    );
    assert_eq!(
        store
            .load_verifier_audit_evidence("evidence-audit-1".to_owned())
            .expect("evidence loads"),
        evidence
    );
    let verdict = VerifierAuditVerdictRecordDto {
        verdict_id: "verdict-1".to_owned(),
        authority_reference: audit_baseline.authority_reference.clone(),
        target_reference: VerifierTargetReferenceDto {
            target_mandate_id: audit_baseline.target_mandate_id.clone(),
            target_revision: audit_baseline.target_revision,
        },
        baseline_digest: digest('f'),
        verdict: VerificationAuditVerdictDto::Pass,
        evidence_references: vec!["evidence-audit-1".to_owned()],
        canonical_verdict_digest: digest('d'),
        created_at_ms: 112,
    };
    assert_eq!(
        store
            .record_verifier_audit_verdict(verdict.clone())
            .expect("verdict stores"),
        verdict
    );
    assert_eq!(
        store
            .load_verifier_audit_verdict("verdict-1".to_owned())
            .expect("verdict loads"),
        verdict
    );

    let applied = store
        .apply_verifier_target_mutation(mutation(&single_use, "mutation-1", "operation-1", 200))
        .expect("mutation applies");
    assert!(!applied.replayed);
    assert_eq!(
        applied.authority.consumption_state,
        VerifierAuthorityConsumptionStateDto::Consumed
    );
    assert_eq!(
        applied.authority.consumed_by_mutation_reference.as_deref(),
        Some("mutation-1")
    );
    let replayed = store
        .apply_verifier_target_mutation(mutation(&single_use, "mutation-1", "operation-1", 200))
        .expect("an equal operation replays");
    assert!(replayed.replayed);
    assert_eq!(replayed.mutation.mutation_id, "mutation-1");
    let consumed = store
        .apply_verifier_target_mutation(mutation(&single_use, "mutation-2", "operation-2", 201))
        .expect_err("a consumed single-use authority rejects");
    assert_eq!(consumed.code(), "verifier_authority_consumed");

    // A revoked authority rejects before any effect.
    let revocable = authority(
        "authority-2",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    store
        .record_verifier_authority(revocable.clone())
        .expect("authority stores");
    let revoked = store
        .revoke_verifier_authority(RevokeVerifierAuthorityInputDto {
            authority_id: "authority-2".to_owned(),
            authority_revision: 1,
            revocation_reference: "revocation-1".to_owned(),
            revoked_at_ms: 150,
        })
        .expect("revocation applies");
    assert!(revoked.revoked_at_ms.is_some());
    assert_eq!(
        store
            .revoke_verifier_authority(RevokeVerifierAuthorityInputDto {
                authority_id: "authority-2".to_owned(),
                authority_revision: 1,
                revocation_reference: "revocation-2".to_owned(),
                revoked_at_ms: 151,
            })
            .expect_err("a revoked authority rejects")
            .code(),
        "verifier_authority_revoked"
    );
    assert_eq!(
        store
            .apply_verifier_target_mutation(mutation(&revocable, "mutation-3", "operation-3", 152))
            .expect_err("a revoked authority cannot apply")
            .code(),
        "verifier_authority_revoked"
    );

    // An expired authority and a disallowed operation fail closed.
    let mut expiring = authority(
        "authority-3",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    expiring.expires_at_ms = Some(300);
    store
        .record_verifier_authority(expiring.clone())
        .expect("authority stores");
    assert_eq!(
        store
            .apply_verifier_target_mutation(mutation(&expiring, "mutation-4", "operation-4", 400))
            .expect_err("an expired authority rejects")
            .code(),
        "verifier_authority_expired"
    );
    let mut restricted = authority(
        "authority-4",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    restricted.allowed_operations = vec![VerifierOperationDto::Stop];
    store
        .record_verifier_authority(restricted.clone())
        .expect("authority stores");
    assert_eq!(
        store
            .apply_verifier_target_mutation(mutation(&restricted, "mutation-5", "operation-5", 200))
            .expect_err("a disallowed operation rejects")
            .code(),
        "verifier_authority_operation_not_allowed"
    );

    // A stale baseline rejects against the frozen revision and sequence.
    let stale_authority = authority(
        "authority-5",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    store
        .record_verifier_authority(stale_authority.clone())
        .expect("authority stores");
    let mut stale_baseline = baseline(&stale_authority);
    stale_baseline.canonical_baseline_digest = digest('9');
    store
        .record_verifier_audit_baseline(stale_baseline)
        .expect("stale baseline stores");
    let mut stale = mutation(&stale_authority, "mutation-6", "operation-6", 200);
    stale.expected_baseline_digest = digest('9');
    stale.expected_target_sequence = 99;
    assert_eq!(
        store
            .apply_verifier_target_mutation(stale)
            .expect_err("a stale baseline rejects")
            .code(),
        "verifier_baseline_stale_target_revision"
    );
}

#[test]
fn slice3_tables_reject_credential_shaped_content() {
    let (directory, store) = repository();
    let credential_title = "api_key=secret-value".to_owned();
    let mut credential_goal = goal("goal-credential", 1);
    credential_goal.scope = GoalScopeDto::Project {
        project_id: "project-goal".to_owned(),
    };
    let mut revision = goal_revision("goal-credential", 1, 'a');
    revision.title = credential_title.clone();
    assert_eq!(
        store
            .create_goal(CreateGoalInputDto {
                goal: credential_goal,
                revision,
            })
            .expect_err("credential-shaped Goal text rejects")
            .code(),
        "credentials_forbidden"
    );
    let card = GoalMemoryCardRecordDto {
        record_id: "memory-credential".to_owned(),
        revision: 1,
        kind: MemoryKindDto::Decision,
        scope: GoalRecordScopeDto::Project {
            project_id: "project-goal".to_owned(),
        },
        title: "Safe title".to_owned(),
        safe_purpose: credential_title,
        retained_content_reference: "content-1".to_owned(),
        canonical_digest: digest('b'),
        created_at_ms: 10,
    };
    assert_eq!(
        store
            .store_goal_memory_card(card)
            .expect_err("credential-shaped card text rejects")
            .code(),
        "credentials_forbidden"
    );

    // Valid durable content stores, and no rejected secret ever reaches a row.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-safe", 1),
            revision: goal_revision("goal-safe", 1, 'c'),
        })
        .expect("safe Goal stores");
    let connection = raw_connection(&directory);
    assert_eq!(query_count(&connection, "SELECT COUNT(*) FROM goals"), 1);
    let titles = query_strings(&connection, "SELECT title FROM goal_revisions");
    assert!(!titles.iter().any(|title| title.contains("secret-value")));
}

// ---------------------------------------------------------------------------
// Harness boundary coverage: missing records, persisted decode guards, atomic
// conflict branches, counter bounds, and unrepresentable integer fields.
// ---------------------------------------------------------------------------

/// Applies raw SQL through an inspection connection with durable CHECK
/// constraints disabled, so tests can seed impossible-looking rows and tamper
/// with persisted encodings.
fn tamper(directory: &TempDir, statements: &str) {
    let connection = raw_connection(directory);
    connection
        .execute_batch("PRAGMA ignore_check_constraints = ON;")
        .expect("check constraints disable for raw tampering");
    connection
        .execute_batch(statements)
        .expect("raw tampering statements commit");
}

/// One raw durable harness revision row used to pre-bind ghost content.
fn raw_harness_revision_insert(harness_id: &str, revision: u64, seed: char) -> String {
    let encoded = digest(seed);
    format!(
        "INSERT INTO harness_rule_revisions(harness_id, revision, task_digest, class, \
         task_mode, presentation_mode, applied_time_zone, source_kinds, source_references, \
         interval_anchor_ms, interval_ms, calendar_expression, completion_link_reference, \
         completion_outcomes, canonical_revision_digest, created_at_ms) VALUES \
         ('{harness_id}', {revision}, '{encoded}', 'light', 'repeated_task', 'journal_only', \
         'UTC', 'fixed_interval', 'source-calendar', 1000, 60000, NULL, NULL, '', '{encoded}', 11);"
    )
}

fn user_session_harness_rule(harness_id: &str, session_id: &str) -> HarnessRuleRecordDto {
    HarnessRuleRecordDto {
        harness_id: harness_id.to_owned(),
        scope: HarnessRuleScopeDto::UserSession {
            project_id: "project-harness".to_owned(),
            session_id: session_id.to_owned(),
        },
        lifecycle_state: HarnessRuleLifecycleStateDto::Active,
        active_revision: 1,
        service_session_id: format!("service-{harness_id}"),
        updated_at_ms: 10,
    }
}

#[test]
fn harness_missing_records_decodes_and_conflicts() {
    let (directory, store) = repository();
    assert_eq!(
        store
            .load_harness_rule("harness-absent".to_owned())
            .expect_err("a missing rule rejects")
            .code(),
        "harness_not_active"
    );
    assert_eq!(
        store
            .load_harness_rule_revision("harness-absent".to_owned(), 1)
            .expect_err("a missing revision rejects")
            .code(),
        "harness_revision_conflict"
    );
    assert_eq!(
        store
            .load_harness_dossier("dossier-absent".to_owned())
            .expect_err("a missing dossier rejects")
            .code(),
        "harness_source_unavailable"
    );
    assert_eq!(
        store
            .load_harness_trigger_reason("reason-absent".to_owned())
            .expect_err("a missing trigger reason rejects")
            .code(),
        "harness_source_unavailable"
    );
    assert_eq!(
        store
            .load_harness_counters("harness-absent".to_owned())
            .expect_err("missing counters reject")
            .code(),
        "harness_not_active"
    );

    // A user-session-scoped rule round-trips through its persisted columns.
    let session_rule = user_session_harness_rule("harness-session", "session-harness");
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: session_rule.clone(),
            revision: harness_revision("harness-session", 1, 'a'),
        })
        .expect("a user-session rule creates");
    assert_eq!(
        store
            .load_harness_rule("harness-session".to_owned())
            .expect("the session rule loads"),
        session_rule
    );

    // A raw revision row pre-bound to different content rejects the creation.
    tamper(
        &directory,
        &raw_harness_revision_insert("harness-ghost", 1, 'b'),
    );
    assert_eq!(
        store
            .create_harness_rule(CreateHarnessRuleInputDto {
                rule: harness_rule("harness-ghost"),
                revision: harness_revision("harness-ghost", 1, 'a'),
            })
            .expect_err("a pre-bound revision rejects")
            .code(),
        "harness_revision_conflict"
    );

    // An unrepresentable revision timestamp fails the whole atomic write.
    let mut overflowing = harness_revision("harness-overflow", 1, 'c');
    overflowing.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .create_harness_rule(CreateHarnessRuleInputDto {
                rule: harness_rule("harness-overflow"),
                revision: overflowing,
            })
            .expect_err("an unrepresentable timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // A malformed persisted scope discriminator is a decode failure.
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-scope-tampered"),
            revision: harness_revision("harness-scope-tampered", 1, 'd'),
        })
        .expect("rule creates");
    tamper(
        &directory,
        "UPDATE harness_rules SET scope_kind='broken' \
         WHERE harness_id='harness-scope-tampered';",
    );
    assert_eq!(
        store
            .load_harness_rule("harness-scope-tampered".to_owned())
            .expect_err("a malformed scope rejects")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn harness_lifecycle_trigger_bounds_and_launch_edges() {
    let (_directory, store) = repository();
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-edge"),
            revision: harness_revision("harness-edge", 1, 'a'),
        })
        .expect("rule creates");

    // Cancelling with an active run is state-preserving.
    assert_eq!(
        store
            .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
                harness_id: "harness-edge".to_owned(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::CancelActiveRun,
                has_active_run: true,
                occurred_at_ms: 20,
            })
            .expect("cancelling with an active run is state-preserving")
            .lifecycle_state,
        HarnessRuleLifecycleStateDto::Active
    );
    store
        .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
            harness_id: "harness-edge".to_owned(),
            expected_revision: 1,
            operation: HarnessRuleOperationDto::Pause,
            has_active_run: false,
            occurred_at_ms: 21,
        })
        .expect("pause applies");
    assert_eq!(
        store
            .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
                harness_id: "harness-edge".to_owned(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Pause,
                has_active_run: false,
                occurred_at_ms: 22,
            })
            .expect_err("pausing a paused rule rejects")
            .code(),
        "harness_not_active"
    );
    assert_eq!(
        store
            .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
                harness_id: "harness-edge".to_owned(),
                expected_revision: 99,
                operation: HarnessRuleOperationDto::Resume,
                has_active_run: false,
                occurred_at_ms: 23,
            })
            .expect_err("a stale expected revision rejects")
            .code(),
        "harness_revision_conflict"
    );
    store
        .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
            harness_id: "harness-edge".to_owned(),
            expected_revision: 1,
            operation: HarnessRuleOperationDto::Resume,
            has_active_run: false,
            occurred_at_ms: 24,
        })
        .expect("resume applies");

    // Archived rules and zero revisions fail closed before capture.
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-archived"),
            revision: harness_revision("harness-archived", 1, 'b'),
        })
        .expect("archived rule creates");
    store
        .transition_harness_rule_lifecycle(TransitionHarnessRuleLifecycleInputDto {
            harness_id: "harness-archived".to_owned(),
            expected_revision: 1,
            operation: HarnessRuleOperationDto::Archive,
            has_active_run: false,
            occurred_at_ms: 25,
        })
        .expect("archive applies");
    assert_eq!(
        store
            .capture_harness_trigger(CaptureHarnessTriggerInputDto {
                harness_id: "harness-archived".to_owned(),
                reason_id: "reason-archived".to_owned(),
                source_kind: HarnessSourceKindDto::FixedInterval,
                rule_revision: 1,
                observed_at_ms: 100,
                applied_time_zone: "UTC".to_owned(),
                cause_chain_reference: None,
                bounded_references: vec!["slot-1".to_owned()],
                catch_up_missed_slots: 0,
            })
            .expect_err("an archived rule rejects triggers")
            .code(),
        "harness_archived"
    );
    assert_eq!(
        store
            .capture_harness_trigger(CaptureHarnessTriggerInputDto {
                harness_id: "harness-edge".to_owned(),
                reason_id: "reason-zero".to_owned(),
                source_kind: HarnessSourceKindDto::FixedInterval,
                rule_revision: 0,
                observed_at_ms: 100,
                applied_time_zone: "UTC".to_owned(),
                cause_chain_reference: None,
                bounded_references: vec!["slot-1".to_owned()],
                catch_up_missed_slots: 0,
            })
            .expect_err("a zero rule revision rejects")
            .code(),
        "harness_revision_conflict"
    );

    // A reason identity bound to another harness is unavailable here.
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-other"),
            revision: harness_revision("harness-other", 1, 'c'),
        })
        .expect("other rule creates");
    store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-other".to_owned(),
            reason_id: "reason-other".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 100,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: vec!["slot-1".to_owned()],
            catch_up_missed_slots: 0,
        })
        .expect("other reason captures");
    assert_eq!(
        store
            .capture_harness_trigger(CaptureHarnessTriggerInputDto {
                harness_id: "harness-edge".to_owned(),
                reason_id: "reason-other".to_owned(),
                source_kind: HarnessSourceKindDto::FixedInterval,
                rule_revision: 1,
                observed_at_ms: 101,
                applied_time_zone: "UTC".to_owned(),
                cause_chain_reference: None,
                bounded_references: vec!["slot-1".to_owned()],
                catch_up_missed_slots: 0,
            })
            .expect_err("a foreign reason identity rejects")
            .code(),
        "harness_source_unavailable"
    );

    // The bounded trigger reference set rejects overflow on both write paths.
    let references = |count: usize, prefix: &str| {
        (0..count)
            .map(|index| format!("{prefix}-{index}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        store
            .capture_harness_trigger(CaptureHarnessTriggerInputDto {
                harness_id: "harness-edge".to_owned(),
                reason_id: "reason-wide".to_owned(),
                source_kind: HarnessSourceKindDto::FixedInterval,
                rule_revision: 1,
                observed_at_ms: 200,
                applied_time_zone: "UTC".to_owned(),
                cause_chain_reference: None,
                bounded_references: references(65, "slot-wide"),
                catch_up_missed_slots: 0,
            })
            .expect_err("an over-wide new trigger rejects")
            .code(),
        "harness_source_limit_exceeded"
    );
    store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-edge".to_owned(),
            reason_id: "reason-full".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 210,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: references(64, "slot-full"),
            catch_up_missed_slots: 0,
        })
        .expect("a full reference set captures");
    assert_eq!(
        store
            .capture_harness_trigger(CaptureHarnessTriggerInputDto {
                harness_id: "harness-edge".to_owned(),
                reason_id: "reason-full-extra".to_owned(),
                source_kind: HarnessSourceKindDto::FixedInterval,
                rule_revision: 1,
                observed_at_ms: 211,
                applied_time_zone: "UTC".to_owned(),
                cause_chain_reference: None,
                bounded_references: vec!["slot-extra".to_owned()],
                catch_up_missed_slots: 0,
            })
            .expect_err("coalescing past the reference bound rejects")
            .code(),
        "harness_source_limit_exceeded"
    );
    let coalesced = store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-edge".to_owned(),
            reason_id: "reason-full-replay".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 212,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: vec!["slot-full-0".to_owned()],
            catch_up_missed_slots: 0,
        })
        .expect("a contained reference coalesces");
    assert_eq!(
        coalesced.outcome,
        HarnessTriggerCaptureOutcomeDto::Coalesced
    );
    assert_eq!(coalesced.reason.coalesced_count, 2);

    // A direct successor launch validates and increments its own counter.
    let launch = store
        .record_harness_launch(RecordHarnessLaunchInputDto {
            harness_id: "harness-edge".to_owned(),
            reason_id: "reason-full".to_owned(),
            cause_chain_depth: 1,
            direct_successor: true,
            occurred_at_ms: 300,
        })
        .expect("a direct successor launch applies");
    assert_eq!(launch.counters.total_launches, 1);
    assert_eq!(launch.counters.direct_successors, 1);
    assert_eq!(launch.reason.state, HarnessTriggerReasonStateDto::Admitted);
    assert_eq!(
        store
            .record_harness_launch(RecordHarnessLaunchInputDto {
                harness_id: "harness-edge".to_owned(),
                reason_id: "reason-full".to_owned(),
                cause_chain_depth: 1,
                direct_successor: false,
                occurred_at_ms: 301,
            })
            .expect_err("an admitted reason cannot launch again")
            .code(),
        "harness_source_unavailable"
    );

    // A fresh catch-up capture reports the catch-up outcome.
    let catch_up = store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-edge".to_owned(),
            reason_id: "reason-catch-up".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 400,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: vec!["slot-catch-up".to_owned()],
            catch_up_missed_slots: 3,
        })
        .expect("a catch-up capture inserts");
    assert_eq!(catch_up.outcome, HarnessTriggerCaptureOutcomeDto::CatchUp);
    assert_eq!(catch_up.reason.coalesced_count, 3);
    assert_eq!(
        store
            .load_harness_trigger_reason("reason-full".to_owned())
            .expect("the admitted reason loads")
            .state,
        HarnessTriggerReasonStateDto::Admitted
    );
    assert_eq!(
        store
            .load_harness_counters("harness-edge".to_owned())
            .expect("counters load")
            .direct_successors,
        1
    );
}

#[test]
fn harness_checkpoint_dossier_and_journal_edges() {
    let (_directory, store) = repository();
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-record"),
            revision: harness_revision("harness-record", 1, 'a'),
        })
        .expect("harness rule creates");
    let dossier = |harness_id: &str, dossier_id: &str, revision: u64, created_at: u64| {
        HarnessDossierRecordDto {
            dossier_id: dossier_id.to_owned(),
            harness_id: harness_id.to_owned(),
            rule_revision: revision,
            reason_id: "reason-record".to_owned(),
            task_digest: digest('a'),
            source_references: vec!["source-calendar".to_owned()],
            typed_references: vec!["reference-1".to_owned()],
            checkpoint_reference: None,
            dossier_bytes: 1_024,
            canonical_dossier_digest: digest('b'),
            created_at_ms: created_at,
        }
    };

    // A dossier needs an existing harness rule.
    assert_eq!(
        store
            .store_harness_dossier(dossier("harness-absent", "dossier-absent", 1, 43))
            .expect_err("a dossier without a rule rejects")
            .code(),
        "harness_not_active"
    );
    let stored_dossier = dossier("harness-record", "dossier-edge", 1, 43);
    assert_eq!(
        store
            .store_harness_dossier(stored_dossier.clone())
            .expect("dossier stores"),
        stored_dossier
    );
    assert_eq!(
        store
            .store_harness_dossier(stored_dossier.clone())
            .expect("an equal dossier replays"),
        stored_dossier
    );
    let mut unequal_dossier = stored_dossier;
    unequal_dossier.rule_revision = 2;
    assert_eq!(
        store
            .store_harness_dossier(unequal_dossier)
            .expect_err("an unequal dossier rejects")
            .code(),
        "harness_source_unavailable"
    );

    // Unrepresentable dossier fields fail the atomic write.
    assert_eq!(
        store
            .store_harness_dossier(dossier(
                "harness-record",
                "dossier-revision-overflow",
                u64::MAX,
                43,
            ))
            .expect_err("an unrepresentable rule revision rejects")
            .code(),
        "storage_decode_failed"
    );
    assert_eq!(
        store
            .store_harness_dossier(dossier(
                "harness-record",
                "dossier-time-overflow",
                1,
                u64::MAX,
            ))
            .expect_err("an unrepresentable dossier timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    let candidate =
        |checkpoint_id: &str, revision: u64, run_id: &str, rule_revision: u64, created_at: u64| {
            HarnessCheckpointRecordDto {
                checkpoint_id: checkpoint_id.to_owned(),
                harness_id: "harness-record".to_owned(),
                rule_revision,
                producing_run_id: run_id.to_owned(),
                checkpoint_revision: revision,
                content_digest: digest('c'),
                checkpoint_bytes: 4_096,
                is_current: false,
                created_at_ms: created_at,
            }
        };
    let commit = |candidate: Option<HarnessCheckpointRecordDto>,
                  outcome: HarnessRunOutcomeDto,
                  occurred_at_ms: u64| CommitHarnessCheckpointInputDto {
        harness_id: "harness-record".to_owned(),
        producing_run_id: "run-record".to_owned(),
        run_outcome: outcome,
        candidate,
        occurred_at_ms,
    };

    // A foreign candidate, a missing candidate, and a skipped revision fail.
    assert_eq!(
        store
            .commit_harness_checkpoint(commit(
                Some(candidate("checkpoint-foreign", 1, "run-other", 1, 40)),
                HarnessRunOutcomeDto::Completed,
                41,
            ))
            .expect_err("a foreign candidate rejects")
            .code(),
        "harness_checkpoint_unavailable"
    );
    assert_eq!(
        store
            .commit_harness_checkpoint(commit(None, HarnessRunOutcomeDto::Completed, 42))
            .expect_err("a completed run needs a candidate")
            .code(),
        "harness_checkpoint_unavailable"
    );
    assert_eq!(
        store
            .commit_harness_checkpoint(commit(
                Some(candidate("checkpoint-skipped", 3, "run-record", 1, 40)),
                HarnessRunOutcomeDto::Completed,
                43,
            ))
            .expect_err("a skipped checkpoint revision rejects")
            .code(),
        "harness_revision_conflict"
    );

    // Unrepresentable checkpoint fields fail the atomic write.
    assert_eq!(
        store
            .commit_harness_checkpoint(commit(
                Some(candidate(
                    "checkpoint-rule-overflow",
                    1,
                    "run-record",
                    u64::MAX,
                    40
                )),
                HarnessRunOutcomeDto::Completed,
                44,
            ))
            .expect_err("an unrepresentable rule revision rejects")
            .code(),
        "storage_decode_failed"
    );
    assert_eq!(
        store
            .commit_harness_checkpoint(commit(
                Some(candidate(
                    "checkpoint-time-overflow",
                    1,
                    "run-record",
                    1,
                    u64::MAX
                )),
                HarnessRunOutcomeDto::Completed,
                45,
            ))
            .expect_err("an unrepresentable checkpoint timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Successful commits replace the current checkpoint; other outcomes retain.
    let first = store
        .commit_harness_checkpoint(commit(
            Some(candidate("checkpoint-one", 1, "run-record", 1, 46)),
            HarnessRunOutcomeDto::Completed,
            47,
        ))
        .expect("checkpoint one commits");
    assert_eq!(
        first.disposition,
        intention_storage::harness_repo::HarnessCheckpointDispositionDto::Replaced
    );
    let second = store
        .commit_harness_checkpoint(commit(
            Some(candidate("checkpoint-two", 2, "run-record", 1, 48)),
            HarnessRunOutcomeDto::Completed,
            49,
        ))
        .expect("checkpoint two commits");
    assert_eq!(
        second
            .current
            .expect("the new checkpoint is current")
            .checkpoint_id,
        "checkpoint-two"
    );
    let retained = store
        .commit_harness_checkpoint(commit(None, HarnessRunOutcomeDto::Failed, 50))
        .expect("a failed run retains the previous checkpoint");
    assert_eq!(
        retained
            .current
            .expect("the retained checkpoint loads")
            .checkpoint_id,
        "checkpoint-two"
    );

    // The journal appends sequences and rejects an unusable page limit.
    let record = store
        .append_harness_journal_record(AppendHarnessJournalRecordInputDto {
            harness_id: "harness-record".to_owned(),
            record_kind: HarnessJournalRecordKindDto::CheckpointAccepted,
            safe_summary: "accepted one checkpoint".to_owned(),
            canonical_record_digest: digest('d'),
            occurred_at_ms: 60,
        })
        .expect("journal record appends");
    assert_eq!(record.sequence, 1);
    assert_eq!(
        store
            .append_harness_journal_record(AppendHarnessJournalRecordInputDto {
                harness_id: "harness-record".to_owned(),
                record_kind: HarnessJournalRecordKindDto::CheckpointAccepted,
                safe_summary: "an unrepresentable record".to_owned(),
                canonical_record_digest: digest('e'),
                occurred_at_ms: u64::MAX,
            })
            .expect_err("an unrepresentable journal timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    for limit in [0, 65] {
        assert_eq!(
            store
                .load_harness_journal(
                    "harness-record".to_owned(),
                    LoadHarnessJournalInputDto {
                        after_sequence: 0,
                        limit,
                    },
                )
                .expect_err("an unusable journal page limit rejects")
                .code(),
            "harness_source_unavailable"
        );
    }
    assert_eq!(
        store
            .load_harness_journal(
                "harness-record".to_owned(),
                LoadHarnessJournalInputDto {
                    after_sequence: 0,
                    limit: 8,
                },
            )
            .expect("the journal page loads")
            .len(),
        1
    );
}

// ---------------------------------------------------------------------------
// Programmatic-caller policy boundary coverage: missing records, persisted
// decode guards, replay conflicts, lifecycle branches, corridor exhaustion,
// and reservation counter edges.
// ---------------------------------------------------------------------------

fn goal_scoped_policy(policy_id: &str, goal_id: &str) -> ProgrammaticPolicyRecordDto {
    let mut record = policy(policy_id, 1);
    record.scope = ProgrammaticPolicyScopeDto::Goal {
        project_id: "project-policy".to_owned(),
        goal_id: goal_id.to_owned(),
    };
    record
}

fn session_scoped_policy(policy_id: &str, owner_session_id: &str) -> ProgrammaticPolicyRecordDto {
    let mut record = policy(policy_id, 1);
    record.scope = ProgrammaticPolicyScopeDto::Session {
        project_id: "project-policy".to_owned(),
        owner_session_id: owner_session_id.to_owned(),
    };
    record
}

fn policy_snapshot(snapshot_id: &str, seed: char) -> ProgrammaticPolicySnapshotRecordDto {
    ProgrammaticPolicySnapshotRecordDto {
        snapshot_id: snapshot_id.to_owned(),
        root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
        policy_references: vec![ProgrammaticPolicyRevisionReferenceDto {
            policy_id: "policy-decode".to_owned(),
            revision: 1,
        }],
        decision_ceiling: ProgrammaticAdmissionDecisionDto::DirectLocalRead,
        max_actions_per_run: 8,
        max_concurrent_actions_per_run: 2,
        calendar_limits: vec![ProgrammaticCalendarLimitRecordDto {
            period_kind: ProgrammaticCalendarPeriodKindDto::Day,
            max_actions: 64,
        }],
        calendar_counter_references: vec![ProgrammaticCalendarCounterReferenceDto {
            policy_id: "policy-decode".to_owned(),
            period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        }],
        baseline_max_actions: None,
        baseline_max_concurrent_actions: None,
        snapshot_digest: digest(seed),
        created_at_ms: 25,
    }
}

fn policy_confirmation(
    confirmation_id: &str,
    call_id: &str,
) -> ProgrammaticPolicyConfirmationRecordDto {
    ProgrammaticPolicyConfirmationRecordDto {
        confirmation_id: confirmation_id.to_owned(),
        root_session_id: "session-root".to_owned(),
        root_run_id: "run-confirmation".to_owned(),
        tool_call_id: call_id.to_owned(),
        tool_id: "tool-write".to_owned(),
        descriptor_revision: "descriptor-1".to_owned(),
        mcp_method_reference: None,
        typed_input_digest: digest('d'),
        policy_snapshot_digest: digest('e'),
        state:
            intention_storage::programmatic_policy_repo::ProgrammaticConfirmationStateDto::Awaiting,
        created_at_ms: 30,
        decided_at_ms: None,
    }
}

fn policy_draft(
    draft_id: &str,
    scope: ProgrammaticPolicyScopeDto,
    record_kind: &str,
) -> ProgrammaticPolicyDraftRecordDto {
    ProgrammaticPolicyDraftRecordDto {
        draft_id: draft_id.to_owned(),
        scope,
        record_kind: record_kind.to_owned(),
        base_revision: 1,
        evidence_references: vec!["evidence-1".to_owned()],
        safe_rationale: "narrow the policy".to_owned(),
        canonical_draft_digest: digest('a'),
        state: ProgrammaticPolicyDraftStateDto::Pending,
        coalesced_evidence_count: 1,
        created_at_ms: 40,
        decided_at_ms: None,
    }
}

#[test]
fn programmatic_policy_missing_records_and_decode_guards() {
    let (directory, store) = repository();
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-decode", 1),
            revision: policy_revision("policy-decode", 1),
        })
        .expect("policy creates");

    assert_eq!(
        store
            .load_programmatic_policy("policy-absent".to_owned())
            .expect_err("a missing policy rejects")
            .code(),
        "programmatic_policy_not_applicable"
    );
    assert_eq!(
        store
            .load_programmatic_policy_revision("policy-decode".to_owned(), 9)
            .expect_err("a missing revision rejects")
            .code(),
        "programmatic_policy_revision_conflict"
    );
    assert_eq!(
        store
            .load_programmatic_policy_snapshot("snapshot-absent".to_owned())
            .expect_err("a missing snapshot rejects")
            .code(),
        "programmatic_policy_snapshot_unavailable"
    );
    assert!(
        store
            .load_programmatic_policy_confirmation("call-absent".to_owned())
            .expect("a missing confirmation binding reads as absent")
            .is_none()
    );
    assert_eq!(
        store
            .decide_programmatic_policy_confirmation(DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: "confirmation-absent".to_owned(),
                tool_call_id: "call-absent".to_owned(),
                typed_input_digest: digest('d'),
                state: intention_storage::programmatic_policy_repo::ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 31,
            })
            .expect_err("deciding a missing confirmation rejects")
            .code(),
        "programmatic_policy_confirmation_required"
    );
    assert!(
        store
            .load_active_programmatic_corridor("run-absent".to_owned())
            .expect("a missing active corridor reads as absent")
            .is_none()
    );
    assert_eq!(
        store
            .consume_programmatic_corridor_action(ConsumeProgrammaticCorridorActionInputDto {
                corridor_digest: digest('a'),
                root_run_id: "run-absent".to_owned(),
                occurred_at_ms: 34,
            })
            .expect_err("consuming a missing corridor rejects")
            .code(),
        "programmatic_policy_corridor_unavailable"
    );
    assert!(
        store
            .load_pending_programmatic_policy_draft(
                ProgrammaticPolicyScopeDto::Project {
                    project_id: "project-policy".to_owned(),
                },
                "programmatic_policy".to_owned(),
            )
            .expect("a missing pending draft reads as absent")
            .is_none()
    );
    assert_eq!(
        store
            .decide_programmatic_policy_draft(
                "draft-absent".to_owned(),
                ProgrammaticPolicyDraftStateDto::Accepted,
                41,
            )
            .expect_err("deciding a missing draft rejects")
            .code(),
        "programmatic_policy_draft_conflict"
    );

    // Goal- and session-scoped policy identities round-trip their columns.
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: goal_scoped_policy("policy-decode-goal", "goal-policy"),
            revision: policy_revision("policy-decode-goal", 1),
        })
        .expect("a goal-scoped policy creates");
    assert_eq!(
        store
            .load_programmatic_policy("policy-decode-goal".to_owned())
            .expect("the goal-scoped policy loads")
            .scope,
        ProgrammaticPolicyScopeDto::Goal {
            project_id: "project-policy".to_owned(),
            goal_id: "goal-policy".to_owned(),
        }
    );
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: session_scoped_policy("policy-decode-session", "session-policy"),
            revision: policy_revision("policy-decode-session", 1),
        })
        .expect("a session-scoped policy creates");
    assert_eq!(
        store
            .load_programmatic_policy("policy-decode-session".to_owned())
            .expect("the session-scoped policy loads")
            .scope,
        ProgrammaticPolicyScopeDto::Session {
            project_id: "project-policy".to_owned(),
            owner_session_id: "session-policy".to_owned(),
        }
    );

    // Empty admission decisions decode as an empty rule set.
    let mut empty_decisions = policy_revision("policy-decode", 2);
    empty_decisions.admission_decisions = Vec::new();
    store
        .append_programmatic_policy_revision(AppendProgrammaticPolicyRevisionInputDto {
            policy_id: "policy-decode".to_owned(),
            expected_revision: 1,
            revision: empty_decisions,
        })
        .expect("an empty decision set appends");
    assert!(
        store
            .load_programmatic_policy_revision("policy-decode".to_owned(), 2)
            .expect("the appended revision loads")
            .admission_decisions
            .is_empty()
    );

    // Tampered persisted encodings are decode failures.
    tamper(
        &directory,
        "UPDATE programmatic_policies SET scope_kind='broken' WHERE policy_id='policy-decode-session';",
    );
    assert_eq!(
        store
            .load_programmatic_policy("policy-decode-session".to_owned())
            .expect_err("a malformed policy scope rejects")
            .code(),
        "storage_decode_failed"
    );
    tamper(
        &directory,
        "UPDATE programmatic_policy_revisions SET root_origin_rules='only-one-field' \
         WHERE policy_id='policy-decode' AND revision=1;",
    );
    assert_eq!(
        store
            .load_programmatic_policy_revision("policy-decode".to_owned(), 1)
            .expect_err("malformed root-origin framing rejects")
            .code(),
        "storage_decode_failed"
    );
    tamper(
        &directory,
        "UPDATE programmatic_policy_revisions SET inherited_policy_references='only-one-field' \
         WHERE policy_id='policy-decode' AND revision=2;",
    );
    assert_eq!(
        store
            .load_programmatic_policy_revision("policy-decode".to_owned(), 2)
            .expect_err("malformed policy reference framing rejects")
            .code(),
        "storage_decode_failed"
    );

    store
        .store_programmatic_policy_snapshot(policy_snapshot("snapshot-decode", 'c'))
        .expect("snapshot stores");
    tamper(
        &directory,
        "UPDATE programmatic_policy_snapshots SET calendar_limits='only-one-field' \
         WHERE snapshot_id='snapshot-decode';",
    );
    assert_eq!(
        store
            .load_programmatic_policy_snapshot("snapshot-decode".to_owned())
            .expect_err("a malformed calendar limit rejects")
            .code(),
        "storage_decode_failed"
    );
    tamper(
        &directory,
        "UPDATE programmatic_policy_snapshots SET calendar_limits='', \
         calendar_counter_references='only-one-field' WHERE snapshot_id='snapshot-decode';",
    );
    assert_eq!(
        store
            .load_programmatic_policy_snapshot("snapshot-decode".to_owned())
            .expect_err("a malformed calendar counter rejects")
            .code(),
        "storage_decode_failed"
    );

    store
        .propose_programmatic_policy_draft(policy_draft(
            "draft-decode",
            ProgrammaticPolicyScopeDto::Project {
                project_id: "project-policy".to_owned(),
            },
            "programmatic_policy",
        ))
        .expect("draft proposes");
    tamper(
        &directory,
        "UPDATE programmatic_policy_drafts SET scope_kind='broken' WHERE draft_id='draft-decode';",
    );
    assert_eq!(
        store
            .decide_programmatic_policy_draft(
                "draft-decode".to_owned(),
                ProgrammaticPolicyDraftStateDto::Accepted,
                42,
            )
            .expect_err("a malformed draft scope rejects")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn programmatic_policy_replay_lifecycle_and_scope_edges() {
    let (_directory, store) = repository();
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-life", 1),
            revision: policy_revision("policy-life", 1),
        })
        .expect("policy creates");
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-life", 1),
            revision: policy_revision("policy-life", 1),
        })
        .expect("an equal policy creation replays");
    let mut unequal = policy("policy-life", 1);
    unequal.canonical_policy_digest = digest('z');
    assert_eq!(
        store
            .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
                policy: unequal,
                revision: policy_revision("policy-life", 1),
            })
            .expect_err("an unequal policy creation conflicts")
            .code(),
        "programmatic_policy_revision_conflict"
    );

    // Goal-scoped policies count through the goal column.
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: goal_scoped_policy("policy-goal-edge", "goal-policy"),
            revision: policy_revision("policy-goal-edge", 1),
        })
        .expect("a goal-scoped policy creates");
    assert_eq!(
        store
            .count_programmatic_policies_on_goal("goal-policy".to_owned())
            .expect("goal counts load"),
        1
    );
    assert_eq!(
        store
            .count_programmatic_policies_in_project("project-policy".to_owned())
            .expect("project counts load"),
        2
    );

    // Appending is exact about the period kind and the observed revision.
    store
        .append_programmatic_policy_revision(AppendProgrammaticPolicyRevisionInputDto {
            policy_id: "policy-life".to_owned(),
            expected_revision: 1,
            revision: policy_revision("policy-life", 2),
        })
        .expect("revision two appends");
    assert_eq!(
        store
            .append_programmatic_policy_revision(AppendProgrammaticPolicyRevisionInputDto {
                policy_id: "policy-life".to_owned(),
                expected_revision: 1,
                revision: policy_revision("policy-life", 2),
            })
            .expect_err("a stale append rejects")
            .code(),
        "programmatic_policy_revision_conflict"
    );
    let mut changed_period = policy_revision("policy-life", 3);
    changed_period.calendar_period_kind = ProgrammaticCalendarPeriodKindDto::Week;
    assert_eq!(
        store
            .append_programmatic_policy_revision(AppendProgrammaticPolicyRevisionInputDto {
                policy_id: "policy-life".to_owned(),
                expected_revision: 2,
                revision: changed_period,
            })
            .expect_err("a changed period kind rejects")
            .code(),
        "programmatic_policy_revision_conflict"
    );

    // The lifecycle state machine closes every unsupported operation.
    assert_eq!(
        store
            .transition_programmatic_policy_lifecycle(
                TransitionProgrammaticPolicyLifecycleInputDto {
                    policy_id: "policy-life".to_owned(),
                    expected_revision: 9,
                    operation: ProgrammaticPolicyLifecycleOperationDto::Suspend,
                    has_active_dependent_tree: false,
                    occurred_at_ms: 20,
                }
            )
            .expect_err("a stale lifecycle revision rejects")
            .code(),
        "programmatic_policy_revision_conflict"
    );
    assert_eq!(
        store
            .transition_programmatic_policy_lifecycle(
                TransitionProgrammaticPolicyLifecycleInputDto {
                    policy_id: "policy-life".to_owned(),
                    expected_revision: 2,
                    operation: ProgrammaticPolicyLifecycleOperationDto::Archive,
                    has_active_dependent_tree: false,
                    occurred_at_ms: 21,
                }
            )
            .expect_err("an active policy cannot archive directly")
            .code(),
        "programmatic_policy_suspended"
    );
    store
        .transition_programmatic_policy_lifecycle(TransitionProgrammaticPolicyLifecycleInputDto {
            policy_id: "policy-life".to_owned(),
            expected_revision: 2,
            operation: ProgrammaticPolicyLifecycleOperationDto::Suspend,
            has_active_dependent_tree: false,
            occurred_at_ms: 22,
        })
        .expect("suspend applies");
    store
        .transition_programmatic_policy_lifecycle(TransitionProgrammaticPolicyLifecycleInputDto {
            policy_id: "policy-life".to_owned(),
            expected_revision: 2,
            operation: ProgrammaticPolicyLifecycleOperationDto::Resume,
            has_active_dependent_tree: false,
            occurred_at_ms: 23,
        })
        .expect("resume applies");
    let revoked = store
        .transition_programmatic_policy_lifecycle(TransitionProgrammaticPolicyLifecycleInputDto {
            policy_id: "policy-life".to_owned(),
            expected_revision: 2,
            operation: ProgrammaticPolicyLifecycleOperationDto::Revoke,
            has_active_dependent_tree: false,
            occurred_at_ms: 24,
        })
        .expect("revocation applies");
    assert_eq!(
        revoked.lifecycle_state,
        ProgrammaticPolicyLifecycleStateDto::Revoked
    );
    assert_eq!(
        store
            .transition_programmatic_policy_lifecycle(
                TransitionProgrammaticPolicyLifecycleInputDto {
                    policy_id: "policy-life".to_owned(),
                    expected_revision: 2,
                    operation: ProgrammaticPolicyLifecycleOperationDto::Archive,
                    has_active_dependent_tree: true,
                    occurred_at_ms: 25,
                }
            )
            .expect_err("an active dependent tree blocks archival")
            .code(),
        "programmatic_policy_suspended"
    );
    let archived = store
        .transition_programmatic_policy_lifecycle(TransitionProgrammaticPolicyLifecycleInputDto {
            policy_id: "policy-life".to_owned(),
            expected_revision: 2,
            operation: ProgrammaticPolicyLifecycleOperationDto::Archive,
            has_active_dependent_tree: false,
            occurred_at_ms: 26,
        })
        .expect("revoked archival applies");
    assert_eq!(
        archived.lifecycle_state,
        ProgrammaticPolicyLifecycleStateDto::Archived
    );
    assert_eq!(
        store
            .transition_programmatic_policy_lifecycle(
                TransitionProgrammaticPolicyLifecycleInputDto {
                    policy_id: "policy-life".to_owned(),
                    expected_revision: 2,
                    operation: ProgrammaticPolicyLifecycleOperationDto::Resume,
                    has_active_dependent_tree: false,
                    occurred_at_ms: 27,
                }
            )
            .expect_err("an archived policy rejects operations")
            .code(),
        "programmatic_policy_revoked"
    );
}

#[test]
fn programmatic_policy_snapshot_confirmation_and_corridor_edges() {
    let (_directory, store) = repository();
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-decode", 1),
            revision: policy_revision("policy-decode", 1),
        })
        .expect("policy creates");

    // Snapshots replay exactly and conflict on unequal content.
    let snapshot = policy_snapshot("snapshot-edge", 'c');
    let mut empty_limits = snapshot.clone();
    empty_limits.snapshot_id = "snapshot-empty".to_owned();
    empty_limits.snapshot_digest = digest('d');
    empty_limits.calendar_limits = Vec::new();
    empty_limits.calendar_counter_references = Vec::new();
    assert_eq!(
        store
            .store_programmatic_policy_snapshot(empty_limits.clone())
            .expect("empty calendar records store"),
        empty_limits
    );
    assert_eq!(
        store
            .load_programmatic_policy_snapshot("snapshot-empty".to_owned())
            .expect("the empty snapshot loads"),
        empty_limits
    );
    assert_eq!(
        store
            .store_programmatic_policy_snapshot(snapshot.clone())
            .expect("snapshot stores"),
        snapshot
    );
    assert_eq!(
        store
            .store_programmatic_policy_snapshot(snapshot.clone())
            .expect("an equal snapshot replays"),
        snapshot
    );
    let mut unequal_snapshot = snapshot.clone();
    unequal_snapshot.created_at_ms = 99;
    assert_eq!(
        store
            .store_programmatic_policy_snapshot(unequal_snapshot)
            .expect_err("an unequal snapshot conflicts")
            .code(),
        "programmatic_policy_snapshot_unavailable"
    );

    // Unrepresentable snapshot bounds fail the atomic write.
    let mut wide = snapshot.clone();
    wide.snapshot_id = "snapshot-wide".to_owned();
    wide.snapshot_digest = digest('e');
    wide.max_actions_per_run = u64::MAX;
    assert_eq!(
        store
            .store_programmatic_policy_snapshot(wide)
            .expect_err("an unrepresentable action bound rejects")
            .code(),
        "storage_decode_failed"
    );
    let mut concurrent = snapshot.clone();
    concurrent.snapshot_id = "snapshot-concurrent".to_owned();
    concurrent.snapshot_digest = digest('f');
    concurrent.max_concurrent_actions_per_run = u64::MAX;
    assert_eq!(
        store
            .store_programmatic_policy_snapshot(concurrent)
            .expect_err("an unrepresentable concurrency bound rejects")
            .code(),
        "storage_decode_failed"
    );
    let mut timestamped = snapshot;
    timestamped.snapshot_id = "snapshot-time-overflow".to_owned();
    timestamped.snapshot_digest = digest('1');
    timestamped.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .store_programmatic_policy_snapshot(timestamped)
            .expect_err("an unrepresentable snapshot timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Confirmations replay exactly, conflict on rebinding, and fail closed.
    let confirmation = policy_confirmation("confirmation-edge", "call-edge");
    assert_eq!(
        store
            .create_programmatic_policy_confirmation(confirmation.clone())
            .expect("confirmation creates"),
        confirmation
    );
    assert_eq!(
        store
            .create_programmatic_policy_confirmation(confirmation.clone())
            .expect("an equal confirmation replays"),
        confirmation
    );
    let mut rebound = confirmation.clone();
    rebound.confirmation_id = "confirmation-rebound".to_owned();
    rebound.tool_id = "tool-other".to_owned();
    assert_eq!(
        store
            .create_programmatic_policy_confirmation(rebound)
            .expect_err("a rebound tool call conflicts")
            .code(),
        "programmatic_policy_confirmation_required"
    );
    let mut awaiting_overflow = confirmation.clone();
    awaiting_overflow.confirmation_id = "confirmation-time-overflow".to_owned();
    awaiting_overflow.tool_call_id = "call-time-overflow".to_owned();
    awaiting_overflow.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .create_programmatic_policy_confirmation(awaiting_overflow)
            .expect_err("an unrepresentable confirmation timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    let mut decided_overflow = confirmation;
    decided_overflow.confirmation_id = "confirmation-decided-overflow".to_owned();
    decided_overflow.tool_call_id = "call-decided-overflow".to_owned();
    decided_overflow.state =
        intention_storage::programmatic_policy_repo::ProgrammaticConfirmationStateDto::Accepted;
    decided_overflow.decided_at_ms = Some(u64::MAX);
    assert_eq!(
        store
            .create_programmatic_policy_confirmation(decided_overflow)
            .expect_err("an unrepresentable decision timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    assert_eq!(
        store
            .decide_programmatic_policy_confirmation(DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: "confirmation-edge".to_owned(),
                tool_call_id: "call-edge".to_owned(),
                typed_input_digest: digest('d'),
                state: intention_storage::programmatic_policy_repo::ProgrammaticConfirmationStateDto::Awaiting,
                decided_at_ms: 35,
            })
            .expect_err("a decision resolves an awaiting confirmation")
            .code(),
        "programmatic_policy_confirmation_required"
    );
    assert_eq!(
        store
            .decide_programmatic_policy_confirmation(DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: "confirmation-edge".to_owned(),
                tool_call_id: "call-other".to_owned(),
                typed_input_digest: digest('d'),
                state: intention_storage::programmatic_policy_repo::ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 36,
            })
            .expect_err("a mismatched decision binding rejects")
            .code(),
        "programmatic_policy_confirmation_required"
    );

    // Corridors replay exactly, conflict on unequal content, and exhaust.
    let corridor = |corridor_digest: String, run_id: &str, maximum: u64, consumed: u64| {
        ProgrammaticAuthorizationCorridorRecordDto {
            corridor_digest,
            root_session_id: "session-root".to_owned(),
            root_run_id: run_id.to_owned(),
            root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
            policy_snapshot_reference: "snapshot-edge".to_owned(),
            required_effect_selectors: vec!["effect-write".to_owned()],
            exact_tool_or_method_selectors: vec!["tool-write".to_owned()],
            descriptor_input_constraint_selections: vec!["input-bounded".to_owned()],
            maximum_action_count: maximum,
            maximum_concurrent_actions: 1,
            confirmation_reference: "confirmation-edge".to_owned(),
            state: ProgrammaticCorridorStateDto::Active,
            consumed_action_count: consumed,
            created_at_ms: 33,
        }
    };
    let active_corridor = corridor(digest('a'), "run-corridor-edge", 2, 0);
    assert_eq!(
        store
            .create_programmatic_authorization_corridor(active_corridor.clone())
            .expect("corridor creates"),
        active_corridor
    );
    assert_eq!(
        store
            .create_programmatic_authorization_corridor(active_corridor.clone())
            .expect("an equal corridor replays"),
        active_corridor
    );
    let mut unequal_corridor = active_corridor;
    unequal_corridor.created_at_ms = 99;
    assert_eq!(
        store
            .create_programmatic_authorization_corridor(unequal_corridor)
            .expect_err("an unequal corridor conflicts")
            .code(),
        "programmatic_policy_corridor_unavailable"
    );
    assert_eq!(
        store
            .load_active_programmatic_corridor("run-corridor-edge".to_owned())
            .expect("the active corridor loads")
            .expect("an active corridor exists")
            .corridor_digest,
        digest('a')
    );
    let mut wide_corridor = corridor(digest('b'), "run-corridor-wide", u64::MAX, 0);
    wide_corridor.maximum_action_count = u64::MAX;
    assert_eq!(
        store
            .create_programmatic_authorization_corridor(wide_corridor)
            .expect_err("an unrepresentable corridor bound rejects")
            .code(),
        "storage_decode_failed"
    );
    let mut time_overflow = corridor(digest('c'), "run-corridor-time", 2, 0);
    time_overflow.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .create_programmatic_authorization_corridor(time_overflow)
            .expect_err("an unrepresentable corridor timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    let pre_consumed = corridor(digest('d'), "run-corridor-exhausted", 1, 1);
    store
        .create_programmatic_authorization_corridor(pre_consumed)
        .expect("a fully consumed corridor stores");
    assert_eq!(
        store
            .consume_programmatic_corridor_action(ConsumeProgrammaticCorridorActionInputDto {
                corridor_digest: digest('d'),
                root_run_id: "run-corridor-exhausted".to_owned(),
                occurred_at_ms: 37,
            })
            .expect_err("an exhausted corridor rejects")
            .code(),
        "programmatic_policy_corridor_exhausted"
    );
    for state in [
        ProgrammaticCorridorStateDto::Active,
        ProgrammaticCorridorStateDto::Exhausted,
    ] {
        assert_eq!(
            store
                .close_programmatic_corridors_for_root(
                    "run-corridor-exhausted".to_owned(),
                    state,
                    38,
                )
                .expect_err("a non-terminal closure state rejects")
                .code(),
            "programmatic_policy_corridor_unavailable"
        );
    }
}

#[test]
fn programmatic_policy_draft_and_reservation_edges() {
    let (directory, store) = repository();
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-edge", 1),
            revision: policy_revision("policy-edge", 1),
        })
        .expect("policy creates");

    // Drafts cover every owner scope and reject unusable content.
    for (draft_id, scope, record_kind) in [
        (
            "draft-project",
            ProgrammaticPolicyScopeDto::Project {
                project_id: "project-policy".to_owned(),
            },
            "programmatic_policy",
        ),
        (
            "draft-goal",
            ProgrammaticPolicyScopeDto::Goal {
                project_id: "project-policy".to_owned(),
                goal_id: "goal-policy".to_owned(),
            },
            "programmatic_policy",
        ),
        (
            "draft-session",
            ProgrammaticPolicyScopeDto::Session {
                project_id: "project-policy".to_owned(),
                owner_session_id: "session-policy".to_owned(),
            },
            "programmatic_policy",
        ),
    ] {
        let draft = policy_draft(draft_id, scope.clone(), record_kind);
        store
            .propose_programmatic_policy_draft(draft.clone())
            .expect("draft proposes");
        assert_eq!(
            store
                .load_pending_programmatic_policy_draft(scope, record_kind.to_owned())
                .expect("the pending draft loads")
                .expect("a pending draft exists")
                .draft_id,
            draft_id
        );
    }

    // A pending draft coalesces and rejects an unrepresentable coalesced count.
    let mut coalesce = policy_draft(
        "draft-coalesce",
        ProgrammaticPolicyScopeDto::Project {
            project_id: "project-policy".to_owned(),
        },
        "coalescing_policy",
    );
    store
        .propose_programmatic_policy_draft(coalesce.clone())
        .expect("coalescing draft proposes");
    coalesce.coalesced_evidence_count = u64::MAX;
    assert_eq!(
        store
            .propose_programmatic_policy_draft(coalesce)
            .expect_err("an unrepresentable coalesced count rejects")
            .code(),
        "storage_decode_failed"
    );

    // Unrepresentable draft fields fail the atomic insert.
    let overflow_cases = [
        ("draft-base-overflow", 0_u8, u64::MAX, u64::MAX, 40, None),
        ("draft-count-overflow", 0_u8, 1, u64::MAX, 40, None),
        ("draft-time-overflow", 0_u8, 1, 1, u64::MAX, None),
        ("draft-decided-overflow", 1_u8, 1, 1, 40, Some(u64::MAX)),
    ];
    for (draft_id, mode, base_revision, coalesced, created_at, decided_at) in overflow_cases {
        let mut draft = policy_draft(
            draft_id,
            ProgrammaticPolicyScopeDto::Project {
                project_id: "project-policy".to_owned(),
            },
            draft_id,
        );
        if mode == 1 {
            draft.state = ProgrammaticPolicyDraftStateDto::Rejected;
        }
        draft.base_revision = base_revision;
        draft.coalesced_evidence_count = coalesced;
        draft.created_at_ms = created_at;
        draft.decided_at_ms = decided_at;
        assert_eq!(
            store
                .propose_programmatic_policy_draft(draft)
                .expect_err("an unrepresentable draft rejects")
                .code(),
            "storage_decode_failed"
        );
    }

    // Decisions resolve pending drafts exactly once.
    let decided = store
        .decide_programmatic_policy_draft(
            "draft-project".to_owned(),
            ProgrammaticPolicyDraftStateDto::Rejected,
            44,
        )
        .expect("the draft decision applies");
    assert_eq!(decided.state, ProgrammaticPolicyDraftStateDto::Rejected);
    assert_eq!(
        store
            .decide_programmatic_policy_draft(
                "draft-project".to_owned(),
                ProgrammaticPolicyDraftStateDto::Accepted,
                45,
            )
            .expect_err("a decided draft rejects")
            .code(),
        "programmatic_policy_draft_conflict"
    );
    assert_eq!(
        store
            .decide_programmatic_policy_draft(
                "draft-goal".to_owned(),
                ProgrammaticPolicyDraftStateDto::Pending,
                46,
            )
            .expect_err("a pending resolution rejects")
            .code(),
        "programmatic_policy_draft_conflict"
    );
    assert_eq!(
        store
            .decide_programmatic_policy_draft(
                "draft-session".to_owned(),
                ProgrammaticPolicyDraftStateDto::Accepted,
                u64::MAX,
            )
            .expect_err("an unrepresentable decision timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Reservations cover unusable bounds, counter absence, and counter windows.
    let reservation =
        |reference: &str, call: &str, run: &str| ProgrammaticPolicyReservationRecordDto {
            reservation_reference: reference.to_owned(),
            policy_id: "policy-edge".to_owned(),
            policy_revision: 1,
            root_run_id: run.to_owned(),
            tool_call_id: call.to_owned(),
            typed_input_digest: digest('a'),
            calendar_counter_reference: "policy-edge:day".to_owned(),
            reserved_at_ms: 50,
            state: ProgrammaticReservationStateDto::Reserved,
            finished_at_ms: None,
        };
    let reserve = |reservation: ProgrammaticPolicyReservationRecordDto,
                   max_actions: u64,
                   max_concurrent: u64,
                   calendar_max: u64,
                   window: (u64, u64)| ReserveProgrammaticPolicyActionInputDto {
        reservation,
        max_actions_per_run: max_actions,
        max_concurrent_actions_per_run: max_concurrent,
        calendar_max_actions: calendar_max,
        calendar_window_start_ms: window.0,
        calendar_window_end_ms: window.1,
        calendar_window_time_zone: "UTC".to_owned(),
    };
    assert_eq!(
        store
            .reserve_programmatic_policy_action(reserve(
                reservation("reservation-invalid", "call-invalid", "run-edge"),
                8,
                2,
                8,
                (100, 100),
            ))
            .expect_err("unusable calendar bounds reject")
            .code(),
        "programmatic_policy_limit_exceeded"
    );
    let reserved = store
        .reserve_programmatic_policy_action(reserve(
            reservation("reservation-one", "call-one", "run-edge"),
            2,
            1,
            8,
            (0, 1_000),
        ))
        .expect("the first reservation commits");
    assert_eq!(reserved.counters.run_reserved_actions, 1);
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-bare", 1),
            revision: policy_revision("policy-bare", 1),
        })
        .expect("counter-free policy creates");
    tamper(
        &directory,
        "DELETE FROM programmatic_policy_counters WHERE policy_id='policy-bare';",
    );
    let mut bare = reservation("reservation-bare", "call-bare", "run-bare");
    bare.policy_id = "policy-bare".to_owned();
    assert_eq!(
        store
            .reserve_programmatic_policy_action(reserve(bare, 8, 2, 8, (0, 1_000)))
            .expect_err("a missing counter rejects")
            .code(),
        "programmatic_policy_counter_unavailable"
    );
    assert_eq!(
        store
            .reserve_programmatic_policy_action(reserve(
                reservation("reservation-window", "call-window", "run-edge"),
                8,
                2,
                8,
                (100, 1_000),
            ))
            .expect_err("a mismatched calendar window rejects")
            .code(),
        "programmatic_policy_counter_unavailable"
    );
    assert_eq!(
        store
            .reserve_programmatic_policy_action(reserve(
                reservation("reservation-concurrent", "call-concurrent", "run-edge"),
                2,
                1,
                8,
                (0, 1_000),
            ))
            .expect_err("the concurrency bound rejects")
            .code(),
        "programmatic_policy_run_limit_exceeded"
    );
    assert_eq!(
        store
            .reserve_programmatic_policy_action(reserve(
                reservation("reservation-calendar", "call-calendar", "run-edge"),
                2,
                2,
                1,
                (0, 1_000),
            ))
            .expect_err("the calendar bound rejects")
            .code(),
        "programmatic_policy_calendar_limit_exceeded"
    );

    // Reservation reads and run pages expose every counter state.
    assert!(
        store
            .load_programmatic_policy_reservation("reservation-absent".to_owned())
            .expect("a missing reservation reads as absent")
            .is_none()
    );
    assert_eq!(
        store
            .load_programmatic_policy_reservation("reservation-one".to_owned())
            .expect("the reservation loads")
            .expect("the reservation exists")
            .state,
        ProgrammaticReservationStateDto::Reserved
    );
    assert_eq!(
        store
            .load_programmatic_policy_reservations_for_run("run-edge".to_owned())
            .expect("the reservation page loads")
            .len(),
        1
    );
    assert!(
        store
            .load_programmatic_policy_reservations_for_run("run-absent".to_owned())
            .expect("a missing run page reads as empty")
            .is_empty()
    );

    // Release and start are exact about the bound tool call.
    assert_eq!(
        store
            .release_programmatic_policy_reservation(ReleaseProgrammaticPolicyReservationInputDto {
                reservation_reference: "reservation-one".to_owned(),
                tool_call_id: "call-other".to_owned(),
                released_at_ms: 60,
            })
            .expect_err("a mismatched release rejects")
            .code(),
        "programmatic_policy_reservation_conflict"
    );
    assert_eq!(
        store
            .commit_programmatic_reservation_started(CommitProgrammaticReservationStartedInputDto {
                reservation_reference: "reservation-one".to_owned(),
                tool_call_id: "call-other".to_owned(),
                started_at_ms: 61,
            })
            .expect_err("a mismatched start rejects")
            .code(),
        "programmatic_policy_reservation_conflict"
    );
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-release", 1),
            revision: policy_revision("policy-release", 1),
        })
        .expect("release policy creates");
    let mut releasing = reservation("reservation-release", "call-release", "run-release");
    releasing.policy_id = "policy-release".to_owned();
    store
        .reserve_programmatic_policy_action(reserve(releasing, 8, 2, 8, (0, 1_000)))
        .expect("the release reservation commits");
    tamper(
        &directory,
        "DELETE FROM programmatic_policy_counters WHERE policy_id='policy-release';",
    );
    assert_eq!(
        store
            .release_programmatic_policy_reservation(ReleaseProgrammaticPolicyReservationInputDto {
                reservation_reference: "reservation-release".to_owned(),
                tool_call_id: "call-release".to_owned(),
                released_at_ms: 62,
            })
            .expect_err("a missing counter blocks the release")
            .code(),
        "programmatic_policy_counter_unavailable"
    );

    // Recovery distinguishes started and never-started reservations.
    let mut recovering = reservation("reservation-recover", "call-recover", "run-recover");
    recovering.policy_id = "policy-edge".to_owned();
    store
        .reserve_programmatic_policy_action(reserve(recovering, 8, 2, 8, (0, 1_000)))
        .expect("the recovery reservation commits");
    assert_eq!(
        store
            .recover_programmatic_policy_reservation(RecoverProgrammaticPolicyReservationInputDto {
                reservation_reference: "reservation-recover".to_owned(),
                tool_call_started: true,
                recovered_at_ms: 63,
            })
            .expect_err("a started recovery needs the started state")
            .code(),
        "programmatic_policy_reservation_conflict"
    );
    let interrupted = store
        .recover_programmatic_policy_reservation(RecoverProgrammaticPolicyReservationInputDto {
            reservation_reference: "reservation-recover".to_owned(),
            tool_call_started: false,
            recovered_at_ms: 64,
        })
        .expect("a never-started recovery applies");
    assert_eq!(
        interrupted.state,
        ProgrammaticReservationStateDto::InterruptedBeforeStart
    );
    assert_eq!(interrupted.finished_at_ms, Some(64));
}

// ---------------------------------------------------------------------------
// Goal aggregate boundary coverage: missing records, persisted decode guards,
// lifecycle and readiness edges, tree cycles, and bounded link limits.
// ---------------------------------------------------------------------------

fn goal_session(id: &str, session_id: &str) -> GoalRecordDto {
    let mut record = goal(id, 1);
    record.scope = GoalScopeDto::Session {
        project_id: "project-goal".to_owned(),
        session_id: session_id.to_owned(),
    };
    record
}

/// One raw persisted Goal row used for bounded-limit seeding.
fn raw_goal_row(goal_id: &str, session_id: Option<&str>) -> String {
    let session = session_id.map_or_else(|| "NULL".to_owned(), |value| format!("'{value}'"));
    match session_id {
        Some(_) => format!(
            "INSERT INTO goals(goal_id, scope_kind, project_id, session_id, active_revision, \
             lifecycle_state, readiness_kind, readiness_evidence, user_decision_kind, \
             user_decision_exceptions, created_at_ms, updated_at_ms) VALUES \
             ('{goal_id}', 'session', 'project-goal', {session}, 1, 'active', 'not_ready', '', \
             'unaccepted', '', 10, 10);"
        ),
        None => format!(
            "INSERT INTO goals(goal_id, scope_kind, project_id, session_id, active_revision, \
             lifecycle_state, readiness_kind, readiness_evidence, user_decision_kind, \
             user_decision_exceptions, created_at_ms, updated_at_ms) VALUES \
             ('{goal_id}', 'project', 'project-goal', NULL, 1, 'active', 'not_ready', '', \
             'unaccepted', '', 10, 10);"
        ),
    }
}

fn memory_card(
    record_id: &str,
    revision: u64,
    scope: GoalRecordScopeDto,
    seed: char,
) -> GoalMemoryCardRecordDto {
    GoalMemoryCardRecordDto {
        record_id: record_id.to_owned(),
        revision,
        kind: MemoryKindDto::Fact,
        scope,
        title: "Memory title".to_owned(),
        safe_purpose: "Memory purpose".to_owned(),
        retained_content_reference: format!("content-{record_id}"),
        canonical_digest: digest(seed),
        created_at_ms: 50,
    }
}

#[test]
fn goal_missing_records_and_decode_guards() {
    let (directory, store) = repository();
    assert_eq!(
        store
            .load_goal("goal-absent".to_owned())
            .expect_err("a missing Goal rejects")
            .code(),
        "goal_not_active"
    );
    assert_eq!(
        store
            .load_goal_revision("goal-absent".to_owned(), 1)
            .expect_err("a missing Goal revision rejects")
            .code(),
        "goal_revision_conflict"
    );
    assert_eq!(
        store
            .load_goal_gate("gate-absent".to_owned())
            .expect_err("a missing gate rejects")
            .code(),
        "goal_gate_unavailable"
    );
    assert_eq!(
        store
            .load_goal_gate_result("gate-absent".to_owned(), 1)
            .expect_err("a missing gate result rejects")
            .code(),
        "goal_gate_unavailable"
    );
    assert_eq!(
        store
            .load_goal_skill_card("skill-absent".to_owned(), 1)
            .expect_err("a missing Skill card rejects")
            .code(),
        "skill_reference_unavailable"
    );
    assert_eq!(
        store
            .load_goal_role_card("role-absent".to_owned(), 1)
            .expect_err("a missing role card rejects")
            .code(),
        "delegation_role_invalid"
    );
    assert_eq!(
        store
            .load_goal_memory_card("memory-absent".to_owned(), 1)
            .expect_err("a missing memory card rejects")
            .code(),
        "memory_reference_unavailable"
    );
    assert_eq!(
        store
            .load_conversation_summary("summary-absent".to_owned(), 1)
            .expect_err("a missing summary rejects")
            .code(),
        "compaction_summary_unavailable"
    );

    // A session-scoped Goal round-trips its scope columns and count.
    let session_goal = goal_session("goal-session-scope", "session-goal-scope");
    store
        .create_goal(CreateGoalInputDto {
            goal: session_goal.clone(),
            revision: goal_revision("goal-session-scope", 1, 'a'),
        })
        .expect("a session Goal creates");
    assert_eq!(
        store
            .load_goal("goal-session-scope".to_owned())
            .expect("the session Goal loads"),
        session_goal
    );
    assert_eq!(
        store
            .count_goals_in_session("session-goal-scope".to_owned())
            .expect("the session count loads"),
        1
    );

    // Tampered Goal columns are decode failures.
    for (goal_id, statements) in [
        (
            "goal-decode-readiness",
            "UPDATE goals SET readiness_kind='broken' \
             WHERE goal_id='goal-decode-readiness';",
        ),
        (
            "goal-decode-decision",
            "UPDATE goals SET user_decision_kind='broken' \
             WHERE goal_id='goal-decode-decision';",
        ),
        (
            "goal-decode-evidence",
            "UPDATE goals SET readiness_kind='ready', readiness_evidence='one-field' \
             WHERE goal_id='goal-decode-evidence';",
        ),
        (
            "goal-decode-exception",
            "UPDATE goals SET user_decision_kind='accepted_with_exception', \
             user_decision_exceptions='one-field' WHERE goal_id='goal-decode-exception';",
        ),
    ] {
        store
            .create_goal(CreateGoalInputDto {
                goal: goal(goal_id, 1),
                revision: goal_revision(goal_id, 1, 'b'),
            })
            .expect("decode fixture Goal creates");
        tamper(&directory, statements);
        assert_eq!(
            store
                .load_goal(goal_id.to_owned())
                .expect_err("a tampered Goal column rejects")
                .code(),
            "storage_decode_failed"
        );
    }
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-decode-gates", 1),
            revision: goal_revision("goal-decode-gates", 1, 'c'),
        })
        .expect("decode fixture Goal creates");
    tamper(
        &directory,
        "UPDATE goal_revisions SET required_gate_references='one-field' \
         WHERE goal_id='goal-decode-gates';",
    );
    assert_eq!(
        store
            .load_goal_revision("goal-decode-gates".to_owned(), 1)
            .expect_err("a malformed gate reference rejects")
            .code(),
        "storage_decode_failed"
    );

    // Gate definitions and template scopes are decoded defensively.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-decode-gate", 1),
            revision: goal_revision("goal-decode-gate", 1, 'd'),
        })
        .expect("decode fixture Goal creates");
    let gate = GoalGateRecordDto {
        gate_id: "gate-decode".to_owned(),
        goal_id: "goal-decode-gate".to_owned(),
        definition: GoalGateDefinitionDto::Reference {
            evidence_contract_revision: 1,
            accepted_reference_kinds: vec![GoalEvidenceKindDto::ExecutableGateResult],
        },
        revision: 1,
        canonical_revision_digest: digest('e'),
        created_at_ms: 40,
    };
    store
        .create_goal_gate(CreateGoalGateInputDto { gate })
        .expect("gate creates");
    tamper(
        &directory,
        "UPDATE goal_gates SET definition='' WHERE gate_id='gate-decode';",
    );
    assert_eq!(
        store
            .load_goal_gate("gate-decode".to_owned())
            .expect_err("a malformed reference gate rejects")
            .code(),
        "storage_decode_failed"
    );
    tamper(
        &directory,
        "UPDATE goal_gates SET definition_kind='executable', definition='one-field' \
         WHERE gate_id='gate-decode';",
    );
    assert_eq!(
        store
            .load_goal_gate("gate-decode".to_owned())
            .expect_err("a malformed executable gate rejects")
            .code(),
        "storage_decode_failed"
    );

    let template =
        |template_id: &str, scope: GoalTemplateScopeDto, seed: char| GoalGateTemplateRecordDto {
            template_id: template_id.to_owned(),
            revision: 1,
            scope,
            capability_reference: "capability-check".to_owned(),
            input_family: GoalGateInputFamilyDto::ClosedTextV1,
            requires_confirmation: false,
            lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
            provenance_kind: GoalTemplateProvenanceKindDto::User,
            provenance_draft_id: None,
            provenance_accepted_by_user: false,
            canonical_digest: digest(seed),
            created_at_ms: 43,
        };
    store
        .create_goal_gate_template(template(
            "template-decode",
            GoalTemplateScopeDto::Session {
                session_id: "session-template".to_owned(),
            },
            'f',
        ))
        .expect("template creates");
    tamper(
        &directory,
        "UPDATE goal_gate_templates SET scope_kind='broken' WHERE template_id='template-decode';",
    );
    assert_eq!(
        store
            .load_goal_gate_template("template-decode".to_owned(), 1)
            .expect_err("a malformed template scope rejects")
            .code(),
        "storage_decode_failed"
    );

    // Gate result evidence is decoded as one exact reference.
    store
        .create_goal_gate(CreateGoalGateInputDto {
            gate: GoalGateRecordDto {
                gate_id: "gate-decode-result".to_owned(),
                goal_id: "goal-decode-gate".to_owned(),
                definition: GoalGateDefinitionDto::Reference {
                    evidence_contract_revision: 1,
                    accepted_reference_kinds: vec![GoalEvidenceKindDto::ExecutableGateResult],
                },
                revision: 1,
                canonical_revision_digest: digest('1'),
                created_at_ms: 40,
            },
        })
        .expect("result gate creates");
    store
        .record_goal_gate_result(GoalGateResultRecordDto {
            gate_id: "gate-decode-result".to_owned(),
            gate_revision: 1,
            producing_run_id: "run-decode".to_owned(),
            outcome_kind: GoalGateOutcomeKindDto::Passed,
            disposition: GoalGateOutcomeDispositionDto::Passed,
            evidence: Some(evidence(
                "evidence-decode",
                GoalEvidenceKindDto::ExecutableGateResult,
            )),
            canonical_result_digest: digest('2'),
            occurred_at_ms: 46,
        })
        .expect("gate result records");
    tamper(
        &directory,
        "UPDATE goal_gate_results SET evidence='one-field' \
         WHERE gate_id='gate-decode-result' AND gate_revision=1;",
    );
    assert_eq!(
        store
            .load_goal_gate_result("gate-decode-result".to_owned(), 1)
            .expect_err("malformed gate result evidence rejects")
            .code(),
        "storage_decode_failed"
    );

    // Memory cards and refinement drafts decode their own records.
    store
        .store_goal_memory_card(memory_card(
            "memory-decode",
            1,
            GoalRecordScopeDto::Goal {
                goal_id: "goal-decode-gate".to_owned(),
            },
            '3',
        ))
        .expect("memory card stores");
    tamper(
        &directory,
        "UPDATE goal_memory_cards SET scope_kind='broken' WHERE record_id='memory-decode';",
    );
    assert_eq!(
        store
            .load_goal_memory_card("memory-decode".to_owned(), 1)
            .expect_err("a malformed memory scope rejects")
            .code(),
        "storage_decode_failed"
    );
    store
        .propose_refinement_draft(RefinementDraftRecordDto {
            draft_id: "draft-decode".to_owned(),
            source_run_id: "run-decode".to_owned(),
            leading_goal_id: "goal-decode-gate".to_owned(),
            milestone: GoalMilestoneDto::TechnicalReadiness,
            base_goal_revision: 1,
            base_record_reference: "goal-decode-gate".to_owned(),
            base_record_revision: 1,
            edits: vec![RefinementEditRecordDto {
                kind: RefinementEditKindDto::ReadinessClaim,
                evidence: evidence("evidence-decode", GoalEvidenceKindDto::TerminalChildResult),
            }],
            evidence_references: vec![evidence(
                "evidence-decode",
                GoalEvidenceKindDto::TerminalChildResult,
            )],
            safe_rationale: "claim readiness".to_owned(),
            canonical_digest: digest('4'),
            state: RefinementDraftStateDto::Pending,
            coalesced_evidence_count: 1,
            created_at_ms: 60,
            decided_at_ms: None,
        })
        .expect("refinement draft proposes");
    tamper(
        &directory,
        "UPDATE refinement_drafts SET edits='one-field' WHERE draft_id='draft-decode';",
    );
    assert_eq!(
        store
            .load_pending_refinement_draft("goal-decode-gate".to_owned())
            .expect_err("malformed refinement edits reject")
            .code(),
        "storage_decode_failed"
    );

    // Frozen verifier references are decoded defensively.
    let frozen_authority = authority(
        "authority-decode",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    store
        .record_verifier_authority(frozen_authority.clone())
        .expect("authority stores");
    let mut goal_tampered = baseline(&frozen_authority);
    goal_tampered.canonical_baseline_digest = digest('5');
    store
        .record_verifier_audit_baseline(goal_tampered)
        .expect("baseline stores");
    let mut contract_tampered = baseline(&frozen_authority);
    contract_tampered.canonical_baseline_digest = digest('6');
    store
        .record_verifier_audit_baseline(contract_tampered)
        .expect("baseline stores");
    tamper(
        &directory,
        "UPDATE verifier_audit_baselines SET frozen_goal_references='one-field' \
         WHERE canonical_baseline_digest='sha256:5555555555555555555555555555555555555555555555555555555555555555';",
    );
    assert_eq!(
        store
            .load_verifier_audit_baseline(digest('5'))
            .expect_err("malformed frozen Goal references reject")
            .code(),
        "storage_decode_failed"
    );
    tamper(
        &directory,
        "UPDATE verifier_audit_baselines SET frozen_contract_references='one-field' \
         WHERE canonical_baseline_digest='sha256:6666666666666666666666666666666666666666666666666666666666666666';",
    );
    assert_eq!(
        store
            .load_verifier_audit_baseline(digest('6'))
            .expect_err("malformed frozen contract references reject")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn goal_lifecycle_readiness_and_decision_edges() {
    let (_directory, store) = repository();
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-state-edge", 1),
            revision: goal_revision("goal-state-edge", 1, 'a'),
        })
        .expect("Goal creates");
    assert_eq!(
        store
            .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
                goal_id: "goal-state-edge".to_owned(),
                expected_revision: 9,
                lifecycle_state: GoalLifecycleStateDto::Stopped,
                occurred_at_ms: 30,
            })
            .expect_err("a stale lifecycle revision rejects")
            .code(),
        "goal_revision_conflict"
    );
    store
        .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
            goal_id: "goal-state-edge".to_owned(),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Stopped,
            occurred_at_ms: 31,
        })
        .expect("the stop transition applies");
    assert_eq!(
        store
            .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
                goal_id: "goal-state-edge".to_owned(),
                expected_revision: 1,
                lifecycle_state: GoalLifecycleStateDto::NeedsRework,
                occurred_at_ms: 32,
            })
            .expect_err("a stopped Goal cannot rework")
            .code(),
        "goal_not_active"
    );
    let archived = store
        .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
            goal_id: "goal-state-edge".to_owned(),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Archived,
            occurred_at_ms: 33,
        })
        .expect("a stopped Goal archives");
    assert_eq!(archived.lifecycle_state, GoalLifecycleStateDto::Archived);

    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-ready-edge", 1),
            revision: goal_revision("goal-ready-edge", 1, 'b'),
        })
        .expect("Goal creates");
    assert_eq!(
        store
            .set_goal_readiness(SetGoalReadinessInputDto {
                goal_id: "goal-ready-edge".to_owned(),
                expected_revision: 9,
                readiness_state: GoalReadinessStateDto::NotReady,
                occurred_at_ms: 34,
            })
            .expect_err("a stale readiness revision rejects")
            .code(),
        "goal_revision_conflict"
    );
    store
        .set_goal_readiness(SetGoalReadinessInputDto {
            goal_id: "goal-ready-edge".to_owned(),
            expected_revision: 1,
            readiness_state: GoalReadinessStateDto::Ready {
                verified_evidence_set: vec![evidence(
                    "evidence-ready",
                    GoalEvidenceKindDto::AcceptedUserDeclaration,
                )],
            },
            occurred_at_ms: 35,
        })
        .expect("readiness records");
    let reset = store
        .set_goal_readiness(SetGoalReadinessInputDto {
            goal_id: "goal-ready-edge".to_owned(),
            expected_revision: 1,
            readiness_state: GoalReadinessStateDto::NotReady,
            occurred_at_ms: 36,
        })
        .expect("readiness resets");
    assert_eq!(reset.readiness_state, GoalReadinessStateDto::NotReady);
    assert_eq!(
        store
            .record_goal_user_decision(RecordGoalUserDecisionInputDto {
                goal_id: "goal-ready-edge".to_owned(),
                expected_revision: 9,
                user_decision_state: GoalUserDecisionStateDto::Unaccepted,
                occurred_at_ms: 37,
            })
            .expect_err("a stale decision revision rejects")
            .code(),
        "goal_revision_conflict"
    );
    store
        .record_goal_user_decision(RecordGoalUserDecisionInputDto {
            goal_id: "goal-ready-edge".to_owned(),
            expected_revision: 1,
            user_decision_state: GoalUserDecisionStateDto::Unaccepted,
            occurred_at_ms: 38,
        })
        .expect("an unaccepted decision records");
    store
        .record_goal_user_decision(RecordGoalUserDecisionInputDto {
            goal_id: "goal-ready-edge".to_owned(),
            expected_revision: 1,
            user_decision_state: GoalUserDecisionStateDto::AcceptedWithException {
                exception_evidence_set: vec![GoalGateExceptionDto {
                    gate_id: "gate-1".to_owned(),
                    gate_revision: 1,
                    kind: GoalGateExceptionKindDto::Unavailable,
                    evidence: evidence(
                        "evidence-exception",
                        GoalEvidenceKindDto::ExecutableGateResult,
                    ),
                }],
            },
            occurred_at_ms: 39,
        })
        .expect("an exception acceptance records on a not-ready Goal");
    store
        .set_goal_readiness(SetGoalReadinessInputDto {
            goal_id: "goal-ready-edge".to_owned(),
            expected_revision: 1,
            readiness_state: GoalReadinessStateDto::Ready {
                verified_evidence_set: vec![evidence(
                    "evidence-ready",
                    GoalEvidenceKindDto::AcceptedUserDeclaration,
                )],
            },
            occurred_at_ms: 40,
        })
        .expect("readiness records");
    assert_eq!(
        store
            .record_goal_user_decision(RecordGoalUserDecisionInputDto {
                goal_id: "goal-ready-edge".to_owned(),
                expected_revision: 1,
                user_decision_state: GoalUserDecisionStateDto::AcceptedWithException {
                    exception_evidence_set: vec![GoalGateExceptionDto {
                        gate_id: "gate-1".to_owned(),
                        gate_revision: 1,
                        kind: GoalGateExceptionKindDto::Unavailable,
                        evidence: evidence(
                            "evidence-exception",
                            GoalEvidenceKindDto::ExecutableGateResult,
                        ),
                    }],
                },
                occurred_at_ms: 41,
            })
            .expect_err("an exception never creates Ready")
            .code(),
        "goal_acceptance_exception_invalid"
    );
    store
        .record_goal_user_decision(RecordGoalUserDecisionInputDto {
            goal_id: "goal-ready-edge".to_owned(),
            expected_revision: 1,
            user_decision_state: GoalUserDecisionStateDto::Accepted,
            occurred_at_ms: 42,
        })
        .expect("plain acceptance records on a ready Goal");
}

#[test]
fn goal_tree_cycles_links_and_limits() {
    let (directory, store) = repository();
    for goal_id in ["goal-a", "goal-b", "goal-c", "goal-d", "goal-e"] {
        store
            .create_goal(CreateGoalInputDto {
                goal: goal(goal_id, 1),
                revision: goal_revision(goal_id, 1, 'a'),
            })
            .expect("tree Goal creates");
    }
    let link = |parent: &str, child: &str, seed: char| AttachGoalChildInputDto {
        parent_goal_id: parent.to_owned(),
        child_goal_id: child.to_owned(),
        child_revision_at_link: 1,
        canonical_link_digest: digest(seed),
        created_at_ms: 20,
    };
    let first = store
        .attach_goal_child(link("goal-a", "goal-b", 'b'))
        .expect("the first parent link attaches");
    assert_eq!(
        store
            .attach_goal_child(link("goal-a", "goal-b", 'b'))
            .expect("an equal parent link replays"),
        first
    );
    assert_eq!(
        store
            .attach_goal_child(link("goal-a", "goal-b", 'c'))
            .expect_err("an unequal parent link conflicts")
            .code(),
        "goal_cycle_detected"
    );
    store
        .attach_goal_child(link("goal-b", "goal-c", 'd'))
        .expect("the chain extends");
    store
        .attach_goal_child(link("goal-c", "goal-d", 'e'))
        .expect("the chain extends");
    assert_eq!(
        store
            .load_goal_tree_depth("goal-d".to_owned())
            .expect("the depth loads"),
        4
    );
    assert_eq!(
        store
            .attach_goal_child(link("goal-c", "goal-b", 'f'))
            .expect_err("a Goal holds one obligatory parent")
            .code(),
        "goal_cycle_detected"
    );
    store
        .attach_goal_child(link("goal-d", "goal-e", '1'))
        .expect("the ancestor walk completes");

    // A recursively stored cycle is detected on depth reads.
    for goal_id in ["goal-x", "goal-y", "goal-z"] {
        tamper(&directory, &raw_goal_row(goal_id, None));
    }
    tamper(
        &directory,
        "INSERT INTO goal_parent_links(parent_goal_id, child_goal_id, child_revision_at_link, \
         canonical_link_digest, created_at_ms) VALUES \
         ('goal-y', 'goal-x', 1, 'sha256:1111111111111111111111111111111111111111111111111111111111111111', 20), \
         ('goal-z', 'goal-y', 1, 'sha256:1111111111111111111111111111111111111111111111111111111111111111', 21), \
         ('goal-x', 'goal-z', 1, 'sha256:1111111111111111111111111111111111111111111111111111111111111111', 22);",
    );
    assert_eq!(
        store
            .load_goal_tree_depth("goal-x".to_owned())
            .expect_err("a stored cycle rejects")
            .code(),
        "goal_cycle_detected"
    );

    // Session links attach project Goals only and are exact about content.
    for (goal_id, session_id) in [
        ("goal-links", "session-links"),
        ("goal-links-second", "session-links-second"),
        ("goal-links-limit", "session-links-limit"),
    ] {
        store
            .create_goal(CreateGoalInputDto {
                goal: goal(goal_id, 1),
                revision: goal_revision(goal_id, 1, '2'),
            })
            .expect("link Goal creates");
        let _ = session_id;
    }
    let session_target = goal_session("goal-links-session-target", "session-links-target");
    store
        .create_goal(CreateGoalInputDto {
            goal: session_target,
            revision: goal_revision("goal-links-session-target", 1, '3'),
        })
        .expect("session Goal creates");
    let session_link =
        |link_id: &str, goal_id: &str, session_id: &str, seed: char, revision: u64| {
            GoalSessionLinkRecordDto {
                link_id: link_id.to_owned(),
                project_goal_id: goal_id.to_owned(),
                session_id: session_id.to_owned(),
                effective_from_revision: revision,
                canonical_link_digest: digest(seed),
                created_at_ms: 22,
            }
        };
    assert_eq!(
        store
            .create_goal_session_link(CreateGoalSessionLinkInputDto {
                link: session_link(
                    "link-session-target",
                    "goal-links-session-target",
                    "session-one",
                    'a',
                    1,
                ),
            })
            .expect_err("session links attach project Goals only")
            .code(),
        "goal_not_active"
    );
    store
        .create_goal_session_link(CreateGoalSessionLinkInputDto {
            link: session_link("link-one", "goal-links", "session-one", 'b', 1),
        })
        .expect("the session link creates");
    assert_eq!(
        store
            .create_goal_session_link(CreateGoalSessionLinkInputDto {
                link: session_link("link-one", "goal-links", "session-one", 'b', 1),
            })
            .expect("an equal session link replays")
            .link_id,
        "link-one"
    );
    assert_eq!(
        store
            .create_goal_session_link(CreateGoalSessionLinkInputDto {
                link: session_link("link-one", "goal-links", "session-one", 'c', 1),
            })
            .expect_err("an unequal session link conflicts")
            .code(),
        "goal_revision_conflict"
    );
    assert_eq!(
        store
            .create_goal_session_link(CreateGoalSessionLinkInputDto {
                link: session_link("link-two", "goal-links", "session-one", 'd', 1),
            })
            .expect_err("a duplicated session link conflicts")
            .code(),
        "goal_revision_conflict"
    );
    assert_eq!(
        store
            .create_goal_session_link(CreateGoalSessionLinkInputDto {
                link: session_link(
                    "link-overflow",
                    "goal-links-second",
                    "session-two",
                    'e',
                    u64::MAX
                ),
            })
            .expect_err("an unrepresentable link revision rejects")
            .code(),
        "storage_decode_failed"
    );

    // The session link bound of one project Goal rejects.
    let mut seed_links = String::new();
    for index in 0..64 {
        seed_links.push_str(&format!(
            "INSERT INTO goal_session_links(link_id, project_goal_id, session_id, \
             effective_from_revision, canonical_link_digest, created_at_ms) VALUES \
             ('link-seed-{index}', 'goal-links-limit', 'session-seed-{index}', 1, \
             'sha256:1111111111111111111111111111111111111111111111111111111111111111', 22);\n"
        ));
    }
    tamper(&directory, &seed_links);
    assert_eq!(
        store
            .create_goal_session_link(CreateGoalSessionLinkInputDto {
                link: session_link("link-limit", "goal-links-limit", "session-limit", 'f', 1),
            })
            .expect_err("the session link bound rejects")
            .code(),
        "goal_session_link_limit_exceeded"
    );

    // The per-session Goal bound rejects through the allocation validator.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal_session("goal-session-count", "session-count"),
            revision: goal_revision("goal-session-count", 1, '4'),
        })
        .expect("the counted session Goal creates");
    let mut seed_goals = String::new();
    for index in 1..64 {
        seed_goals.push_str(&raw_goal_row(
            &format!("goal-session-seed-{index}"),
            Some("session-count"),
        ));
        seed_goals.push('\n');
    }
    tamper(&directory, &seed_goals);
    assert_eq!(
        store
            .create_goal(CreateGoalInputDto {
                goal: goal_session("goal-session-overflow", "session-count"),
                revision: goal_revision("goal-session-overflow", 1, '5'),
            })
            .expect_err("the session Goal bound rejects")
            .code(),
        "goal_limit_exceeded"
    );
}

fn reference_gate(gate_id: &str, goal_id: &str, seed: char) -> GoalGateRecordDto {
    GoalGateRecordDto {
        gate_id: gate_id.to_owned(),
        goal_id: goal_id.to_owned(),
        definition: GoalGateDefinitionDto::Reference {
            evidence_contract_revision: 1,
            accepted_reference_kinds: vec![GoalEvidenceKindDto::ExecutableGateResult],
        },
        revision: 1,
        canonical_revision_digest: digest(seed),
        created_at_ms: 40,
    }
}

fn gate_template(
    template_id: &str,
    scope: GoalTemplateScopeDto,
    seed: char,
) -> GoalGateTemplateRecordDto {
    GoalGateTemplateRecordDto {
        template_id: template_id.to_owned(),
        revision: 1,
        scope,
        capability_reference: "capability-check".to_owned(),
        input_family: GoalGateInputFamilyDto::ClosedTextV1,
        requires_confirmation: false,
        lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
        provenance_kind: GoalTemplateProvenanceKindDto::User,
        provenance_draft_id: None,
        provenance_accepted_by_user: false,
        canonical_digest: digest(seed),
        created_at_ms: 43,
    }
}

#[test]
fn goal_gate_template_and_result_edges() {
    let (directory, store) = repository();
    assert_eq!(
        store
            .create_goal_gate(CreateGoalGateInputDto {
                gate: reference_gate("gate-missing", "goal-absent", 'a'),
            })
            .expect_err("a gate needs its owning Goal")
            .code(),
        "goal_gate_unavailable"
    );

    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-gate-edge", 1),
            revision: goal_revision("goal-gate-edge", 1, 'a'),
        })
        .expect("Goal creates");
    let gate = reference_gate("gate-edge", "goal-gate-edge", 'b');
    assert_eq!(
        store
            .create_goal_gate(CreateGoalGateInputDto { gate: gate.clone() })
            .expect("gate creates"),
        gate
    );
    assert_eq!(
        store
            .create_goal_gate(CreateGoalGateInputDto { gate: gate.clone() })
            .expect("an equal gate replays"),
        gate
    );
    let mut unequal_gate = gate.clone();
    unequal_gate.canonical_revision_digest = digest('c');
    assert_eq!(
        store
            .create_goal_gate(CreateGoalGateInputDto { gate: unequal_gate })
            .expect_err("an unequal gate conflicts")
            .code(),
        "goal_gate_unavailable"
    );
    assert_eq!(
        store
            .load_goal_gate("gate-edge".to_owned())
            .expect("the gate loads"),
        gate
    );
    let mut executable_gate = reference_gate("gate-executable", "goal-gate-edge", 'd');
    executable_gate.definition = GoalGateDefinitionDto::Executable {
        template_id: "template-x".to_owned(),
        template_revision: 1,
    };
    store
        .create_goal_gate(CreateGoalGateInputDto {
            gate: executable_gate.clone(),
        })
        .expect("an executable gate creates");
    assert_eq!(
        store
            .load_goal_gate("gate-executable".to_owned())
            .expect("the executable gate loads"),
        executable_gate
    );

    // The per-Goal gate bound rejects.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-gate-limit", 1),
            revision: goal_revision("goal-gate-limit", 1, 'e'),
        })
        .expect("Goal creates");
    let mut seed_gates = String::new();
    for index in 0..32 {
        seed_gates.push_str(&format!(
            "INSERT INTO goal_gates(gate_id, goal_id, revision, definition_kind, definition, \
             canonical_revision_digest, created_at_ms) VALUES ('gate-seed-{index}', \
             'goal-gate-limit', 1, 'reference', 'seed', \
             'sha256:1111111111111111111111111111111111111111111111111111111111111111', 40);\n"
        ));
    }
    tamper(&directory, &seed_gates);
    assert_eq!(
        store
            .create_goal_gate(CreateGoalGateInputDto {
                gate: reference_gate("gate-limit", "goal-gate-limit", 'f'),
            })
            .expect_err("the gate bound rejects")
            .code(),
        "goal_gate_limit_exceeded"
    );

    // Gate revisions append reference definitions and replay equal rows.
    store
        .create_goal_gate(CreateGoalGateInputDto {
            gate: reference_gate("gate-append", "goal-gate-edge", '1'),
        })
        .expect("append gate creates");
    store
        .append_goal_gate_revision(AppendGoalGateRevisionInputDto {
            gate_id: "gate-append".to_owned(),
            expected_revision: 1,
            definition: GoalGateDefinitionDto::Reference {
                evidence_contract_revision: 2,
                accepted_reference_kinds: vec![GoalEvidenceKindDto::ExecutableGateResult],
            },
            canonical_revision_digest: digest('2'),
            occurred_at_ms: 41,
        })
        .expect("a reference gate revision appends");
    store
        .create_goal_gate(CreateGoalGateInputDto {
            gate: reference_gate("gate-seed-append", "goal-gate-edge", '3'),
        })
        .expect("seeded append gate creates");
    let seeded_definition = GoalGateDefinitionDto::Executable {
        template_id: "template-seed".to_owned(),
        template_revision: 3,
    };
    tamper(
        &directory,
        "INSERT INTO goal_gate_revisions(gate_id, revision, definition_kind, definition, \
         canonical_revision_digest, created_at_ms) VALUES ('gate-seed-append', 2, 'executable', \
         'template-seed\u{1f}3', \
         'sha256:2222222222222222222222222222222222222222222222222222222222222222', 44);",
    );
    let replayed = store
        .append_goal_gate_revision(AppendGoalGateRevisionInputDto {
            gate_id: "gate-seed-append".to_owned(),
            expected_revision: 1,
            definition: seeded_definition,
            canonical_revision_digest: digest('2'),
            occurred_at_ms: 45,
        })
        .expect("an equal existing gate revision replays");
    assert_eq!(replayed.revision, 2);
    store
        .create_goal_gate(CreateGoalGateInputDto {
            gate: reference_gate("gate-seed-unequal", "goal-gate-edge", '4'),
        })
        .expect("unequal append gate creates");
    tamper(
        &directory,
        "INSERT INTO goal_gate_revisions(gate_id, revision, definition_kind, definition, \
         canonical_revision_digest, created_at_ms) VALUES ('gate-seed-unequal', 2, 'reference', \
         'seed', \
         'sha256:3333333333333333333333333333333333333333333333333333333333333333', 44);",
    );
    assert_eq!(
        store
            .append_goal_gate_revision(AppendGoalGateRevisionInputDto {
                gate_id: "gate-seed-unequal".to_owned(),
                expected_revision: 1,
                definition: GoalGateDefinitionDto::Reference {
                    evidence_contract_revision: 1,
                    accepted_reference_kinds: vec![GoalEvidenceKindDto::ExecutableGateResult],
                },
                canonical_revision_digest: digest('2'),
                occurred_at_ms: 45,
            })
            .expect_err("an unequal existing gate revision conflicts")
            .code(),
        "goal_revision_conflict"
    );

    // Templates cover every owner scope and replay exactly.
    let project_template = gate_template(
        "template-project-edge",
        GoalTemplateScopeDto::Project {
            project_id: "project-goal".to_owned(),
        },
        'a',
    );
    assert_eq!(
        store
            .create_goal_gate_template(project_template.clone())
            .expect("project template creates"),
        project_template
    );
    assert_eq!(
        store
            .load_goal_gate_template("template-project-edge".to_owned(), 1)
            .expect("project template loads"),
        project_template
    );
    let goal_template = gate_template(
        "template-goal-edge",
        GoalTemplateScopeDto::Goal {
            goal_id: "goal-gate-edge".to_owned(),
        },
        'b',
    );
    assert_eq!(
        store
            .create_goal_gate_template(goal_template.clone())
            .expect("goal template creates"),
        goal_template
    );
    assert_eq!(
        store
            .load_goal_gate_template("template-goal-edge".to_owned(), 1)
            .expect("goal template loads"),
        goal_template
    );
    let session_template = gate_template(
        "template-session-edge",
        GoalTemplateScopeDto::Session {
            session_id: "session-template-edge".to_owned(),
        },
        'c',
    );
    store
        .create_goal_gate_template(session_template)
        .expect("session template creates");
    let mut overflow_template = gate_template(
        "template-overflow-edge",
        GoalTemplateScopeDto::Project {
            project_id: "project-goal".to_owned(),
        },
        'd',
    );
    overflow_template.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .create_goal_gate_template(overflow_template)
            .expect_err("an unrepresentable template timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    let replay_template = gate_template(
        "template-replay-edge",
        GoalTemplateScopeDto::Project {
            project_id: "project-goal".to_owned(),
        },
        'e',
    );
    assert_eq!(
        store
            .create_goal_gate_template(replay_template.clone())
            .expect("replay template creates"),
        replay_template
    );
    assert_eq!(
        store
            .create_goal_gate_template(replay_template.clone())
            .expect("an equal template replays"),
        replay_template
    );
    let mut unequal_template = replay_template;
    unequal_template.canonical_digest = digest('f');
    assert_eq!(
        store
            .create_goal_gate_template(unequal_template)
            .expect_err("an unequal template conflicts")
            .code(),
        "goal_gate_unavailable"
    );
    assert_eq!(
        store
            .transition_goal_gate_template_lifecycle(TransitionGoalGateTemplateInputDto {
                template_id: "template-replay-edge".to_owned(),
                revision: 0,
                lifecycle_state: GoalTemplateLifecycleStateDto::Archived,
                occurred_at_ms: 44,
            })
            .expect_err("a zero template revision rejects")
            .code(),
        "goal_gate_unavailable"
    );
    store
        .transition_goal_gate_template_lifecycle(TransitionGoalGateTemplateInputDto {
            template_id: "template-replay-edge".to_owned(),
            revision: 1,
            lifecycle_state: GoalTemplateLifecycleStateDto::Archived,
            occurred_at_ms: 45,
        })
        .expect("template archives");
    assert_eq!(
        store
            .transition_goal_gate_template_lifecycle(TransitionGoalGateTemplateInputDto {
                template_id: "template-replay-edge".to_owned(),
                revision: 1,
                lifecycle_state: GoalTemplateLifecycleStateDto::Archived,
                occurred_at_ms: 46,
            })
            .expect("an equal template transition replays")
            .lifecycle_state,
        GoalTemplateLifecycleStateDto::Archived
    );
    store
        .transition_goal_gate_template_lifecycle(TransitionGoalGateTemplateInputDto {
            template_id: "template-replay-edge".to_owned(),
            revision: 1,
            lifecycle_state: GoalTemplateLifecycleStateDto::Enabled,
            occurred_at_ms: 47,
        })
        .expect("template restores");

    // Gate results replay exactly, carry optional evidence, and fail closed.
    let result = GoalGateResultRecordDto {
        gate_id: "gate-append".to_owned(),
        gate_revision: 2,
        producing_run_id: "run-gate-edge".to_owned(),
        outcome_kind: GoalGateOutcomeKindDto::Passed,
        disposition: GoalGateOutcomeDispositionDto::Passed,
        evidence: Some(evidence(
            "evidence-gate-edge",
            GoalEvidenceKindDto::ExecutableGateResult,
        )),
        canonical_result_digest: digest('5'),
        occurred_at_ms: 46,
    };
    assert_eq!(
        store
            .record_goal_gate_result(result.clone())
            .expect("gate result records"),
        result
    );
    assert_eq!(
        store
            .record_goal_gate_result(result.clone())
            .expect("an equal gate result replays"),
        result
    );
    let no_evidence = GoalGateResultRecordDto {
        gate_id: "gate-executable".to_owned(),
        gate_revision: 1,
        producing_run_id: "run-gate-edge".to_owned(),
        outcome_kind: GoalGateOutcomeKindDto::TimedOut,
        disposition: GoalGateOutcomeDispositionDto::Failed,
        evidence: None,
        canonical_result_digest: digest('6'),
        occurred_at_ms: 47,
    };
    assert_eq!(
        store
            .record_goal_gate_result(no_evidence.clone())
            .expect("an evidence-free result records"),
        no_evidence
    );
    assert!(
        store
            .load_goal_gate_result("gate-executable".to_owned(), 1)
            .expect("the evidence-free result loads")
            .evidence
            .is_none()
    );
    store
        .create_goal_gate(CreateGoalGateInputDto {
            gate: reference_gate("gate-result-overflow", "goal-gate-edge", '7'),
        })
        .expect("overflow gate creates");
    assert_eq!(
        store
            .record_goal_gate_result(GoalGateResultRecordDto {
                gate_id: "gate-result-overflow".to_owned(),
                gate_revision: 1,
                producing_run_id: "run-gate-edge".to_owned(),
                outcome_kind: GoalGateOutcomeKindDto::Cancelled,
                disposition: GoalGateOutcomeDispositionDto::Failed,
                evidence: None,
                canonical_result_digest: digest('8'),
                occurred_at_ms: u64::MAX,
            })
            .expect_err("an unrepresentable result timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn goal_card_memory_limits_and_relations() {
    let (directory, store) = repository();
    let session_scope = GoalRecordScopeDto::Session {
        session_id: "session-card".to_owned(),
    };
    assert_eq!(
        store
            .store_goal_memory_card(memory_card("memory-session", 1, session_scope.clone(), 'a'))
            .expect("a session memory card stores"),
        memory_card("memory-session", 1, session_scope.clone(), 'a')
    );
    assert_eq!(
        store
            .load_goal_memory_card("memory-session".to_owned(), 1)
            .expect("the session memory card loads")
            .scope,
        session_scope
    );

    let goal_scope = GoalRecordScopeDto::Goal {
        goal_id: "goal-cards-edge".to_owned(),
    };
    let card = memory_card("memory-edge", 1, goal_scope.clone(), 'b');
    assert_eq!(
        store
            .store_goal_memory_card(card.clone())
            .expect("memory card stores"),
        card
    );
    assert_eq!(
        store
            .store_goal_memory_card(card.clone())
            .expect("an equal memory card replays"),
        card
    );
    let mut unequal_card = card;
    unequal_card.canonical_digest = digest('c');
    assert_eq!(
        store
            .store_goal_memory_card(unequal_card)
            .expect_err("an unequal memory card conflicts")
            .code(),
        "memory_reference_unavailable"
    );

    // The active memory card bound of one owner scope rejects.
    let mut seed_cards = String::new();
    for index in 0..128 {
        seed_cards.push_str(&format!(
            "INSERT INTO goal_memory_cards(record_id, revision, kind, scope_kind, owner_id, \
             title, safe_purpose, retained_content_reference, canonical_digest, created_at_ms) \
             VALUES ('memory-seed-{index}', 1, 'fact', 'project', 'project-card-limit', \
             'title', 'purpose', 'content', \
             'sha256:1111111111111111111111111111111111111111111111111111111111111111', 50);\n"
        ));
    }
    tamper(&directory, &seed_cards);
    assert_eq!(
        store
            .store_goal_memory_card(memory_card(
                "memory-limit",
                1,
                GoalRecordScopeDto::Project {
                    project_id: "project-card-limit".to_owned(),
                },
                'd',
            ))
            .expect_err("the active memory bound rejects")
            .code(),
        "memory_entry_limit_exceeded"
    );

    // Replacement and rollback relations fail on unrepresentable timestamps.
    store
        .store_goal_memory_card(memory_card("memory-rel-1", 1, goal_scope.clone(), 'e'))
        .expect("replaced card stores");
    assert_eq!(
        store
            .replace_goal_memory_card(GoalMemoryCardReplacementInputDto {
                replaced_record_id: "memory-rel-1".to_owned(),
                replaced_revision: 1,
                replacement: memory_card("memory-rel-2", 1, goal_scope.clone(), 'f'),
                occurred_at_ms: u64::MAX,
            })
            .expect_err("an unrepresentable replacement timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    store
        .store_goal_memory_card(memory_card("memory-rb-1", 1, goal_scope.clone(), '1'))
        .expect("restored card stores");
    assert_eq!(
        store
            .rollback_goal_memory_card(GoalMemoryCardRollbackInputDto {
                restored_record_id: "memory-rb-1".to_owned(),
                restored_revision: 1,
                replacement: memory_card("memory-rb-1", 2, goal_scope, '2'),
                occurred_at_ms: u64::MAX,
            })
            .expect_err("an unrepresentable rollback timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Skill cards replay exactly and reject unequal duplicates.
    let skill = GoalSkillCardRecordDto {
        skill_id: "skill-edge".to_owned(),
        revision: 1,
        canonical_name: "bounded-research".to_owned(),
        description: "Route bounded research work".to_owned(),
        owner_scope: GoalRecordScopeDto::Project {
            project_id: "project-goal".to_owned(),
        },
        content_reference: "skill-content-edge".to_owned(),
        canonical_digest: digest('3'),
        created_at_ms: 54,
    };
    assert_eq!(
        store
            .store_goal_skill_card(skill.clone())
            .expect("Skill card stores"),
        skill
    );
    assert_eq!(
        store
            .store_goal_skill_card(skill.clone())
            .expect("an equal Skill card replays"),
        skill
    );
    let mut unequal_skill = skill;
    unequal_skill.description = "Different bounded description".to_owned();
    assert_eq!(
        store
            .store_goal_skill_card(unequal_skill)
            .expect_err("an unequal Skill card conflicts")
            .code(),
        "skill_reference_unavailable"
    );

    // Role cards replay exactly and fail on unrepresentable limits.
    let role = GoalRoleCardRecordDto {
        role_id: "role-edge".to_owned(),
        revision: 1,
        canonical_name: "narrow-reviewer".to_owned(),
        task: "Review one bounded change".to_owned(),
        permitted_class: GoalRoleClassDto::Light,
        tool_subset: vec!["read".to_owned()],
        context_limit_bytes: 4_096,
        result_limit_bytes: 2_048,
        canonical_digest: digest('4'),
        created_at_ms: 55,
    };
    assert_eq!(
        store
            .store_goal_role_card(role.clone())
            .expect("role card stores"),
        role
    );
    assert_eq!(
        store
            .store_goal_role_card(role.clone())
            .expect("an equal role card replays"),
        role
    );
    let mut wide_role = role;
    wide_role.role_id = "role-overflow".to_owned();
    wide_role.context_limit_bytes = u64::MAX;
    assert_eq!(
        store
            .store_goal_role_card(wide_role)
            .expect_err("an unrepresentable context limit rejects")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn goal_proposal_and_compaction_edges() {
    let (directory, store) = repository();
    let draft = |draft_id: &str, seed: char| RefinementDraftRecordDto {
        draft_id: draft_id.to_owned(),
        source_run_id: "run-proposal".to_owned(),
        leading_goal_id: "goal-proposal".to_owned(),
        milestone: GoalMilestoneDto::TechnicalReadiness,
        base_goal_revision: 1,
        base_record_reference: "goal-proposal".to_owned(),
        base_record_revision: 1,
        edits: vec![RefinementEditRecordDto {
            kind: RefinementEditKindDto::ReadinessClaim,
            evidence: evidence(
                "evidence-proposal",
                GoalEvidenceKindDto::TerminalChildResult,
            ),
        }],
        evidence_references: vec![evidence(
            "evidence-proposal",
            GoalEvidenceKindDto::TerminalChildResult,
        )],
        safe_rationale: "claim readiness".to_owned(),
        canonical_digest: digest(seed),
        state: RefinementDraftStateDto::Pending,
        coalesced_evidence_count: 1,
        created_at_ms: 60,
        decided_at_ms: None,
    };

    // Unrepresentable proposal fields fail the atomic insert.
    for (draft_id, base_goal_revision, base_record_revision, coalesced) in [
        ("draft-goal-overflow", u64::MAX, 1, 1),
        ("draft-record-overflow", 1, u64::MAX, 1),
        ("draft-count-overflow", 1, 1, u64::MAX),
    ] {
        let mut overflowing = draft(draft_id, 'a');
        overflowing.base_goal_revision = base_goal_revision;
        overflowing.base_record_revision = base_record_revision;
        overflowing.coalesced_evidence_count = coalesced;
        assert_eq!(
            store
                .propose_refinement_draft(overflowing)
                .expect_err("an unrepresentable proposal rejects")
                .code(),
            "storage_decode_failed"
        );
    }

    // A decided proposal replays equal content and rejects unequal content.
    let mut decided = draft("draft-decided-edge", 'b');
    decided.state = RefinementDraftStateDto::Rejected;
    decided.decided_at_ms = Some(61);
    assert_eq!(
        store
            .propose_refinement_draft(decided.clone())
            .expect("the decided proposal stores"),
        decided
    );
    assert_eq!(
        store
            .propose_refinement_draft(decided.clone())
            .expect("an equal decided proposal replays"),
        decided
    );
    let mut unequal_decided = decided;
    unequal_decided.canonical_digest = digest('c');
    assert_eq!(
        store
            .propose_refinement_draft(unequal_decided)
            .expect_err("an unequal decided proposal conflicts")
            .code(),
        "refinement_draft_conflict"
    );
    assert_eq!(
        store
            .decide_refinement_draft(
                "draft-absent".to_owned(),
                RefinementDraftStateDto::Accepted,
                62,
            )
            .expect_err("a missing draft rejects")
            .code(),
        "refinement_draft_conflict"
    );
    store
        .propose_refinement_draft(draft("draft-pending-edge", 'd'))
        .expect("pending proposal stores");
    assert_eq!(
        store
            .decide_refinement_draft(
                "draft-pending-edge".to_owned(),
                RefinementDraftStateDto::Pending,
                63,
            )
            .expect_err("a pending resolution rejects")
            .code(),
        "refinement_draft_conflict"
    );

    // Compaction working forms, chain continuation, and replay.
    let scope = GoalRecordScopeDto::Goal {
        goal_id: "goal-compact-edge".to_owned(),
    };
    assert_eq!(
        store
            .load_goal_compaction_working_form(scope.clone())
            .expect("a fresh working form loads"),
        GoalCompactionWorkingFormRecordDto {
            current_summary: None,
            uncompacted_suffix: Vec::new(),
        }
    );
    assert_eq!(
        store
            .record_compaction_suffix_reference(RecordCompactionSuffixReferenceInputDto {
                scope: scope.clone(),
                history_reference: String::new(),
                occurred_at_ms: 70,
            })
            .expect_err("a blank history reference rejects")
            .code(),
        "compaction_history_unavailable"
    );
    assert_eq!(
        store
            .store_conversation_summary(ConversationSummaryRecordDto {
                summary_id: "summary-empty".to_owned(),
                revision: 1,
                scope: scope.clone(),
                previous_summary_reference: None,
                source_range_start: "history-1".to_owned(),
                source_range_end: "history-1".to_owned(),
                safe_content: "no suffix yet".to_owned(),
                canonical_digest: digest('e'),
                created_at_ms: 80,
            })
            .expect_err("a summary without a suffix rejects")
            .code(),
        "compaction_history_unavailable"
    );
    for (reference, time) in [("history-1", 71), ("history-2", 72), ("history-3", 73)] {
        store
            .record_compaction_suffix_reference(RecordCompactionSuffixReferenceInputDto {
                scope: scope.clone(),
                history_reference: reference.to_owned(),
                occurred_at_ms: time,
            })
            .expect("history reference appends");
    }
    assert_eq!(
        store
            .store_conversation_summary(ConversationSummaryRecordDto {
                summary_id: "summary-start-mismatch".to_owned(),
                revision: 1,
                scope: scope.clone(),
                previous_summary_reference: None,
                source_range_start: "history-x".to_owned(),
                source_range_end: "history-2".to_owned(),
                safe_content: "wrong start".to_owned(),
                canonical_digest: digest('f'),
                created_at_ms: 81,
            })
            .expect_err("a range start mismatch rejects")
            .code(),
        "compaction_history_unavailable"
    );
    assert_eq!(
        store
            .store_conversation_summary(ConversationSummaryRecordDto {
                summary_id: "summary-previous".to_owned(),
                revision: 1,
                scope: scope.clone(),
                previous_summary_reference: Some("summary-prev".to_owned()),
                source_range_start: "history-1".to_owned(),
                source_range_end: "history-2".to_owned(),
                safe_content: "first summary with predecessor".to_owned(),
                canonical_digest: digest('1'),
                created_at_ms: 82,
            })
            .expect_err("a first summary carries no predecessor")
            .code(),
        "compaction_summary_unavailable"
    );
    let summary = ConversationSummaryRecordDto {
        summary_id: "summary-edge".to_owned(),
        revision: 1,
        scope: scope.clone(),
        previous_summary_reference: None,
        source_range_start: "history-1".to_owned(),
        source_range_end: "history-2".to_owned(),
        safe_content: "compacted summary".to_owned(),
        canonical_digest: digest('2'),
        created_at_ms: 83,
    };
    assert_eq!(
        store
            .store_conversation_summary(summary.clone())
            .expect("the first summary stores"),
        summary
    );
    assert_eq!(
        store
            .load_goal_compaction_working_form(scope.clone())
            .expect("the working form loads")
            .current_summary
            .expect("a current summary exists")
            .summary_id,
        "summary-edge"
    );
    assert_eq!(
        store
            .store_conversation_summary(ConversationSummaryRecordDto {
                summary_id: "summary-gap".to_owned(),
                revision: 3,
                scope: scope.clone(),
                previous_summary_reference: Some("summary-edge".to_owned()),
                source_range_start: "history-3".to_owned(),
                source_range_end: "history-3".to_owned(),
                safe_content: "skipping a revision".to_owned(),
                canonical_digest: digest('3'),
                created_at_ms: 84,
            })
            .expect_err("a summary revision gap rejects")
            .code(),
        "compaction_summary_unavailable"
    );

    // Replaying an equal summary after a raw working-form reset is a no-op.
    let suffix_reset = "UPDATE compaction_working_forms SET current_summary_id=NULL, \
         current_summary_revision=NULL, uncompacted_suffix='history-1\u{1f}history-2' \
         WHERE scope_kind='goal' AND owner_id='goal-compact-edge';";
    tamper(&directory, suffix_reset);
    assert_eq!(
        store
            .store_conversation_summary(summary.clone())
            .expect("an equal summary replays"),
        summary
    );
    tamper(&directory, suffix_reset);
    let mut unequal_summary = summary;
    unequal_summary.canonical_digest = digest('4');
    assert_eq!(
        store
            .store_conversation_summary(unequal_summary)
            .expect_err("an unequal summary conflicts")
            .code(),
        "compaction_summary_unavailable"
    );
    tamper(&directory, suffix_reset);
    assert_eq!(
        store
            .store_conversation_summary(ConversationSummaryRecordDto {
                summary_id: "summary-time-overflow".to_owned(),
                revision: 1,
                scope,
                previous_summary_reference: None,
                source_range_start: "history-1".to_owned(),
                source_range_end: "history-2".to_owned(),
                safe_content: "unrepresentable timestamp".to_owned(),
                canonical_digest: digest('5'),
                created_at_ms: u64::MAX,
            })
            .expect_err("an unrepresentable summary timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn verifier_replays_missing_and_mutation_gates() {
    let (_directory, store) = repository();
    assert_eq!(
        store
            .load_verifier_authority("authority-absent".to_owned(), 1)
            .expect_err("a missing authority rejects")
            .code(),
        "verifier_authority_invalid"
    );
    assert_eq!(
        store
            .load_verifier_audit_baseline(digest('0'))
            .expect_err("a missing baseline rejects")
            .code(),
        "verifier_baseline_invalid"
    );
    assert_eq!(
        store
            .load_verifier_audit_evidence("evidence-absent".to_owned())
            .expect_err("missing evidence rejects")
            .code(),
        "verifier_evidence_invalid"
    );
    assert_eq!(
        store
            .load_verifier_audit_verdict("verdict-absent".to_owned())
            .expect_err("a missing verdict rejects")
            .code(),
        "verifier_verdict_invalid"
    );
    assert_eq!(
        store
            .load_verifier_target_mutation("mutation-absent".to_owned())
            .expect_err("a missing mutation rejects")
            .code(),
        "verifier_mutation_invalid"
    );

    // Authorities replay exactly, reject unequal content, and revoke in order.
    let replay_authority = authority(
        "authority-replay",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    assert_eq!(
        store
            .record_verifier_authority(replay_authority.clone())
            .expect("authority stores"),
        replay_authority
    );
    assert_eq!(
        store
            .record_verifier_authority(replay_authority.clone())
            .expect("an equal authority replays"),
        replay_authority
    );
    let mut unequal_authority = replay_authority.clone();
    unequal_authority.expires_at_ms = Some(50_000);
    assert_eq!(
        store
            .record_verifier_authority(unequal_authority)
            .expect_err("an unequal authority conflicts")
            .code(),
        "verifier_authority_invalid"
    );
    assert_eq!(
        store
            .revoke_verifier_authority(RevokeVerifierAuthorityInputDto {
                authority_id: "authority-replay".to_owned(),
                authority_revision: 1,
                revocation_reference: "revocation-early".to_owned(),
                revoked_at_ms: 50,
            })
            .expect_err("a revocation never precedes issuance")
            .code(),
        "verifier_authority_invalid"
    );

    // Baselines replay exactly, conflict on unequal content, and bound fields.
    let mut replay_baseline = baseline(&replay_authority);
    replay_baseline.canonical_baseline_digest = digest('1');
    assert_eq!(
        store
            .record_verifier_audit_baseline(replay_baseline.clone())
            .expect("baseline stores"),
        replay_baseline
    );
    assert_eq!(
        store
            .record_verifier_audit_baseline(replay_baseline.clone())
            .expect("an equal baseline replays"),
        replay_baseline
    );
    let mut unequal_baseline = replay_baseline.clone();
    unequal_baseline.created_at_ms = 999;
    assert_eq!(
        store
            .record_verifier_audit_baseline(unequal_baseline)
            .expect_err("an unequal baseline conflicts")
            .code(),
        "verifier_baseline_invalid"
    );
    let mut sequence_overflow = replay_baseline.clone();
    sequence_overflow.canonical_baseline_digest = digest('a');
    sequence_overflow.target_sequence = u64::MAX;
    assert_eq!(
        store
            .record_verifier_audit_baseline(sequence_overflow)
            .expect_err("an unrepresentable target sequence rejects")
            .code(),
        "storage_decode_failed"
    );
    let mut baseline_time_overflow = replay_baseline.clone();
    baseline_time_overflow.canonical_baseline_digest = digest('b');
    baseline_time_overflow.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .record_verifier_audit_baseline(baseline_time_overflow)
            .expect_err("an unrepresentable baseline timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Evidence and verdicts replay exactly and reject unequal duplicates.
    let evidence_record = VerifierAuditEvidenceRecordDto {
        evidence_id: "evidence-replay".to_owned(),
        authority_reference: replay_baseline.authority_reference.clone(),
        target_reference: VerifierTargetReferenceDto {
            target_mandate_id: replay_baseline.target_mandate_id.clone(),
            target_revision: replay_baseline.target_revision,
        },
        frozen_references: replay_baseline.frozen_references.clone(),
        evidence_kind: VerifierEvidenceKindDto::UnconditionalPass,
        retained_content_reference: "retained-replay".to_owned(),
        canonical_evidence_digest: digest('c'),
        created_at_ms: 111,
    };
    assert_eq!(
        store
            .record_verifier_audit_evidence(evidence_record.clone())
            .expect("evidence stores"),
        evidence_record
    );
    assert_eq!(
        store
            .record_verifier_audit_evidence(evidence_record.clone())
            .expect("equal evidence replays"),
        evidence_record
    );
    let mut unequal_evidence = evidence_record.clone();
    unequal_evidence.retained_content_reference = "retained-other".to_owned();
    assert_eq!(
        store
            .record_verifier_audit_evidence(unequal_evidence)
            .expect_err("unequal evidence conflicts")
            .code(),
        "verifier_evidence_invalid"
    );
    let mut evidence_time_overflow = evidence_record;
    evidence_time_overflow.evidence_id = "evidence-time-overflow".to_owned();
    evidence_time_overflow.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .record_verifier_audit_evidence(evidence_time_overflow)
            .expect_err("an unrepresentable evidence timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    let verdict_record = VerifierAuditVerdictRecordDto {
        verdict_id: "verdict-replay".to_owned(),
        authority_reference: replay_baseline.authority_reference.clone(),
        target_reference: VerifierTargetReferenceDto {
            target_mandate_id: replay_baseline.target_mandate_id.clone(),
            target_revision: replay_baseline.target_revision,
        },
        baseline_digest: digest('1'),
        verdict: VerificationAuditVerdictDto::Pass,
        evidence_references: vec!["evidence-replay".to_owned()],
        canonical_verdict_digest: digest('d'),
        created_at_ms: 112,
    };
    assert_eq!(
        store
            .record_verifier_audit_verdict(verdict_record.clone())
            .expect("verdict stores"),
        verdict_record
    );
    assert_eq!(
        store
            .record_verifier_audit_verdict(verdict_record.clone())
            .expect("an equal verdict replays"),
        verdict_record
    );
    let mut unequal_verdict = verdict_record.clone();
    unequal_verdict.created_at_ms = 999;
    assert_eq!(
        store
            .record_verifier_audit_verdict(unequal_verdict)
            .expect_err("an unequal verdict conflicts")
            .code(),
        "verifier_verdict_invalid"
    );
    let mut verdict_time_overflow = verdict_record;
    verdict_time_overflow.verdict_id = "verdict-time-overflow".to_owned();
    verdict_time_overflow.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .record_verifier_audit_verdict(verdict_time_overflow)
            .expect_err("an unrepresentable verdict timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Mutations: single-use consumption, idempotency, and every fail-closed
    // precondition.
    let single_use = authority(
        "authority-single",
        VerifierAuthorityConsumptionRuleDto::SingleUse,
    );
    store
        .record_verifier_authority(single_use.clone())
        .expect("authority stores");
    let mut single_baseline = baseline(&single_use);
    single_baseline.canonical_baseline_digest = digest('4');
    store
        .record_verifier_audit_baseline(single_baseline)
        .expect("baseline stores");
    let mut single_mutation = mutation(&single_use, "mutation-single", "operation-single", 200);
    single_mutation.expected_baseline_digest = digest('4');
    let applied = store
        .apply_verifier_target_mutation(single_mutation.clone())
        .expect("the mutation applies");
    assert!(!applied.replayed);
    assert_eq!(
        applied.authority.consumption_state,
        VerifierAuthorityConsumptionStateDto::Consumed
    );
    assert_eq!(
        applied.authority.consumed_by_mutation_reference.as_deref(),
        Some("mutation-single")
    );
    assert!(
        store
            .apply_verifier_target_mutation(single_mutation)
            .expect("an equal operation replays")
            .replayed
    );
    let mut rebound = mutation(&single_use, "mutation-rebound", "operation-single", 201);
    rebound.expected_baseline_digest = digest('4');
    rebound.canonical_mutation_digest = digest('e');
    assert_eq!(
        store
            .apply_verifier_target_mutation(rebound)
            .expect_err("one operation binds one exact mutation")
            .code(),
        "verifier_mutation_invalid"
    );
    assert_eq!(
        store
            .load_verifier_target_mutation("mutation-single".to_owned())
            .expect("the mutation loads")
            .mutation_id,
        "mutation-single"
    );

    let reusable = authority(
        "authority-reusable",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    store
        .record_verifier_authority(reusable.clone())
        .expect("authority stores");
    let mut reusable_baseline = baseline(&reusable);
    reusable_baseline.canonical_baseline_digest = digest('5');
    store
        .record_verifier_audit_baseline(reusable_baseline)
        .expect("baseline stores");

    // An authority digest mismatch fails before any load.
    let mut digest_mismatch = mutation(&reusable, "mutation-digest", "operation-digest", 200);
    digest_mismatch
        .authority_reference
        .canonical_authority_digest = digest('9');
    digest_mismatch.expected_baseline_digest = digest('5');
    assert_eq!(
        store
            .apply_verifier_target_mutation(digest_mismatch)
            .expect_err("an authority digest mismatch rejects")
            .code(),
        "verifier_authority_digest_mismatch"
    );

    // A missing baseline and a foreign-authority baseline both fail.
    let mut missing_baseline = mutation(&reusable, "mutation-missing", "operation-missing", 200);
    missing_baseline.expected_baseline_digest = digest('7');
    assert_eq!(
        store
            .apply_verifier_target_mutation(missing_baseline)
            .expect_err("a missing baseline rejects")
            .code(),
        "verifier_baseline_invalid"
    );
    let other = authority(
        "authority-other",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    store
        .record_verifier_authority(other.clone())
        .expect("authority stores");
    let mut other_baseline = baseline(&other);
    other_baseline.canonical_baseline_digest = digest('6');
    store
        .record_verifier_audit_baseline(other_baseline)
        .expect("baseline stores");
    let mut foreign_baseline = mutation(&reusable, "mutation-foreign", "operation-foreign", 200);
    foreign_baseline.expected_baseline_digest = digest('6');
    assert_eq!(
        store
            .apply_verifier_target_mutation(foreign_baseline)
            .expect_err("a foreign baseline rejects")
            .code(),
        "verifier_baseline_invalid"
    );

    // The frozen target identity and contract are exact.
    let mut identity_baseline = baseline(&reusable);
    identity_baseline.canonical_baseline_digest = digest('8');
    identity_baseline.target_mandate_id = "target-other".to_owned();
    store
        .record_verifier_audit_baseline(identity_baseline)
        .expect("baseline stores");
    let mut stale_identity = mutation(&reusable, "mutation-identity", "operation-identity", 200);
    stale_identity.expected_baseline_digest = digest('8');
    assert_eq!(
        store
            .apply_verifier_target_mutation(stale_identity)
            .expect_err("a stale target identity rejects")
            .code(),
        "verifier_baseline_stale_target_identity"
    );
    let mut contract_baseline = baseline(&reusable);
    contract_baseline.canonical_baseline_digest = digest('e');
    contract_baseline.audit_contract_reference = VerifierContractReferenceDto {
        contract_id: "audit-contract-2".to_owned(),
        contract_revision: 1,
        canonical_contract_digest: digest('e'),
    };
    store
        .record_verifier_audit_baseline(contract_baseline)
        .expect("baseline stores");
    let mut contract_mismatch = mutation(&reusable, "mutation-contract", "operation-contract", 200);
    contract_mismatch.expected_baseline_digest = digest('e');
    assert_eq!(
        store
            .apply_verifier_target_mutation(contract_mismatch)
            .expect_err("a baseline contract mismatch rejects")
            .code(),
        "verifier_authority_contract_mismatch"
    );

    // An unrepresentable mutation timestamp fails the atomic insert.
    let timeless = authority(
        "authority-timeless",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    let mut timeless_authority = timeless.clone();
    timeless_authority.expires_at_ms = None;
    store
        .record_verifier_authority(timeless_authority)
        .expect("authority stores");
    let mut timeless_baseline = baseline(&timeless);
    timeless_baseline.canonical_baseline_digest = digest('0');
    store
        .record_verifier_audit_baseline(timeless_baseline)
        .expect("baseline stores");
    let mut time_overflow = mutation(
        &timeless,
        "mutation-time-overflow",
        "operation-time-overflow",
        u64::MAX,
    );
    time_overflow.expected_baseline_digest = digest('0');
    assert_eq!(
        store
            .apply_verifier_target_mutation(time_overflow)
            .expect_err("an unrepresentable mutation timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn harness_storage_faults_and_counter_bounds() {
    let (directory, store) = repository();

    // An unrepresentable rule timestamp fails the atomic creation.
    let mut overflowing_rule = harness_rule("harness-rule-overflow");
    overflowing_rule.updated_at_ms = u64::MAX;
    assert_eq!(
        store
            .create_harness_rule(CreateHarnessRuleInputDto {
                rule: overflowing_rule,
                revision: harness_revision("harness-rule-overflow", 1, 'a'),
            })
            .expect_err("an unrepresentable rule timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // An unrepresentable revision timestamp fails the atomic revision.
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-revise-overflow"),
            revision: harness_revision("harness-revise-overflow", 1, 'b'),
        })
        .expect("rule creates");
    let mut overflowing_revision = harness_revision("harness-revise-overflow", 2, 'c');
    overflowing_revision.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .revise_harness_rule(ReviseHarnessRuleInputDto {
                harness_id: "harness-revise-overflow".to_owned(),
                expected_revision: 1,
                revision: overflowing_revision,
            })
            .expect_err("an unrepresentable revision timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Seeded counters trip the total-launch and concurrency validators.
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-counter-bounds"),
            revision: harness_revision("harness-counter-bounds", 1, 'd'),
        })
        .expect("rule creates");
    store
        .capture_harness_trigger(CaptureHarnessTriggerInputDto {
            harness_id: "harness-counter-bounds".to_owned(),
            reason_id: "reason-counter-bounds".to_owned(),
            source_kind: HarnessSourceKindDto::FixedInterval,
            rule_revision: 1,
            observed_at_ms: 100,
            applied_time_zone: "UTC".to_owned(),
            cause_chain_reference: None,
            bounded_references: vec!["slot-counter-bounds".to_owned()],
            catch_up_missed_slots: 0,
        })
        .expect("a pending trigger exists");
    for (columns, message, code) in [
        (
            "total_launches=9223372036854775807",
            "the total launch bound rejects",
            "harness_cause_chain_limit_exceeded",
        ),
        (
            "total_launches=0, concurrent_non_terminal=9223372036854775807",
            "the concurrency bound rejects",
            "harness_concurrency_limit_exceeded",
        ),
    ] {
        tamper(
            &directory,
            &format!(
                "UPDATE harness_counters SET {columns} WHERE harness_id='harness-counter-bounds';"
            ),
        );
        assert_eq!(
            store
                .record_harness_launch(RecordHarnessLaunchInputDto {
                    harness_id: "harness-counter-bounds".to_owned(),
                    reason_id: "reason-counter-bounds".to_owned(),
                    cause_chain_depth: 1,
                    direct_successor: false,
                    occurred_at_ms: 110,
                })
                .expect_err(message)
                .code(),
            code
        );
    }

    // A current checkpoint pinned at the integer bound overflows its successor.
    store
        .create_harness_rule(CreateHarnessRuleInputDto {
            rule: harness_rule("harness-checkpoint-overflow"),
            revision: harness_revision("harness-checkpoint-overflow", 1, 'e'),
        })
        .expect("rule creates");
    let checkpoint = |checkpoint_id: &str, revision: u64| HarnessCheckpointRecordDto {
        checkpoint_id: checkpoint_id.to_owned(),
        harness_id: "harness-checkpoint-overflow".to_owned(),
        rule_revision: 1,
        producing_run_id: "run-checkpoint".to_owned(),
        checkpoint_revision: revision,
        content_digest: digest('f'),
        checkpoint_bytes: 1_024,
        is_current: false,
        created_at_ms: 40,
    };
    let commit = |candidate: Option<HarnessCheckpointRecordDto>, occurred_at_ms: u64| {
        CommitHarnessCheckpointInputDto {
            harness_id: "harness-checkpoint-overflow".to_owned(),
            producing_run_id: "run-checkpoint".to_owned(),
            run_outcome: HarnessRunOutcomeDto::Completed,
            candidate,
            occurred_at_ms,
        }
    };
    store
        .commit_harness_checkpoint(commit(Some(checkpoint("checkpoint-base", 1)), 41))
        .expect("the base checkpoint commits");
    tamper(
        &directory,
        "UPDATE harness_checkpoints SET checkpoint_revision=9223372036854775807 \
         WHERE harness_id='harness-checkpoint-overflow' AND is_current=1;",
    );
    assert_eq!(
        store
            .commit_harness_checkpoint(commit(
                Some(checkpoint("checkpoint-overflow", 9_223_372_036_854_775_808)),
                42,
            ))
            .expect_err("an unrepresentable checkpoint revision rejects")
            .code(),
        "storage_decode_failed"
    );

    // A capture beyond the integer range fails on both the insert and the
    // coalescing update path.
    assert_eq!(
        store
            .capture_harness_trigger(CaptureHarnessTriggerInputDto {
                harness_id: "harness-checkpoint-overflow".to_owned(),
                reason_id: "reason-capture-insert-overflow".to_owned(),
                source_kind: HarnessSourceKindDto::FixedInterval,
                rule_revision: 1,
                observed_at_ms: u64::MAX,
                applied_time_zone: "UTC".to_owned(),
                cause_chain_reference: None,
                bounded_references: vec!["slot-capture".to_owned()],
                catch_up_missed_slots: 0,
            })
            .expect_err("an unrepresentable capture timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    assert_eq!(
        store
            .capture_harness_trigger(CaptureHarnessTriggerInputDto {
                harness_id: "harness-counter-bounds".to_owned(),
                reason_id: "reason-capture-coalesce-overflow".to_owned(),
                source_kind: HarnessSourceKindDto::FixedInterval,
                rule_revision: 1,
                observed_at_ms: u64::MAX,
                applied_time_zone: "UTC".to_owned(),
                cause_chain_reference: None,
                bounded_references: vec!["slot-capture".to_owned()],
                catch_up_missed_slots: 0,
            })
            .expect_err("an unrepresentable coalesced timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Read faults surface as storage unavailability, never as missing records.
    tamper(
        &directory,
        "ALTER TABLE harness_rules RENAME TO harness_rules_hidden;",
    );
    assert_eq!(
        store
            .load_harness_rule("harness-hidden".to_owned())
            .expect_err("an unreadable rule table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE harness_rule_revisions RENAME TO harness_rule_revisions_hidden;",
    );
    assert_eq!(
        store
            .load_harness_rule_revision("harness-hidden".to_owned(), 1)
            .expect_err("an unreadable revision table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE harness_trigger_reasons RENAME TO harness_trigger_reasons_hidden;",
    );
    assert_eq!(
        store
            .load_harness_trigger_reason("reason-hidden".to_owned())
            .expect_err("an unreadable trigger table is a storage fault")
            .code(),
        "storage_unavailable"
    );
}

#[test]
fn programmatic_policy_storage_faults_and_counter_bounds() {
    let (directory, store) = repository();

    // An unrepresentable policy timestamp fails the atomic creation.
    let mut overflowing_policy = policy("policy-overflow", 1);
    overflowing_policy.updated_at_ms = u64::MAX;
    assert_eq!(
        store
            .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
                policy: overflowing_policy,
                revision: policy_revision("policy-overflow", 1),
            })
            .expect_err("an unrepresentable policy timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Reservation identities and timestamps stay inside the SQLite range.
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-bounds", 1),
            revision: policy_revision("policy-bounds", 1),
        })
        .expect("policy creates");
    let reservation =
        |reference: &str, call: &str, run: &str| ProgrammaticPolicyReservationRecordDto {
            reservation_reference: reference.to_owned(),
            policy_id: "policy-bounds".to_owned(),
            policy_revision: 1,
            root_run_id: run.to_owned(),
            tool_call_id: call.to_owned(),
            typed_input_digest: digest('a'),
            calendar_counter_reference: "policy-bounds:day".to_owned(),
            reserved_at_ms: 50,
            state: ProgrammaticReservationStateDto::Reserved,
            finished_at_ms: None,
        };
    let reserve = |reservation: ProgrammaticPolicyReservationRecordDto,
                   max_actions: u64,
                   max_concurrent: u64,
                   calendar_max: u64,
                   window: (u64, u64)| ReserveProgrammaticPolicyActionInputDto {
        reservation,
        max_actions_per_run: max_actions,
        max_concurrent_actions_per_run: max_concurrent,
        calendar_max_actions: calendar_max,
        calendar_window_start_ms: window.0,
        calendar_window_end_ms: window.1,
        calendar_window_time_zone: "UTC".to_owned(),
    };
    let mut revision_overflow = reservation(
        "reservation-revision-overflow",
        "call-revision-overflow",
        "run-bounds",
    );
    revision_overflow.policy_revision = u64::MAX;
    assert_eq!(
        store
            .reserve_programmatic_policy_action(reserve(revision_overflow, 8, 2, 8, (0, 1_000)))
            .expect_err("an unrepresentable policy revision rejects")
            .code(),
        "storage_decode_failed"
    );
    let mut time_overflow = reservation(
        "reservation-time-overflow",
        "call-time-overflow",
        "run-bounds",
    );
    time_overflow.reserved_at_ms = u64::MAX;
    assert_eq!(
        store
            .reserve_programmatic_policy_action(reserve(time_overflow, 8, 2, 8, (0, 1_000)))
            .expect_err("an unrepresentable reservation timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Counter adjustments overflow the durable integer range field by field.
    let set_counters = |columns: &str| {
        tamper(
            &directory,
            &format!(
                "UPDATE programmatic_policy_counters SET {columns} WHERE policy_id='policy-bounds';"
            ),
        );
    };
    for (columns, reference, call, message) in [
        (
            "run_reserved_actions=9223372036854775807",
            "reservation-run-overflow",
            "call-run-overflow",
            "an unrepresentable reserved count rejects",
        ),
        (
            "run_reserved_actions=0, calendar_reserved_actions=9223372036854775807, \
             calendar_window_start_ms=0, calendar_window_end_ms=1000",
            "reservation-calendar-overflow",
            "call-calendar-overflow",
            "an unrepresentable calendar count rejects",
        ),
    ] {
        set_counters(columns);
        assert_eq!(
            store
                .reserve_programmatic_policy_action(reserve(
                    reservation(reference, call, "run-bounds"),
                    u64::MAX,
                    u64::MAX,
                    u64::MAX,
                    (0, 1_000),
                ))
                .expect_err(message)
                .code(),
            "storage_decode_failed"
        );
    }
    for (columns, reference, call, window, message) in [
        (
            "calendar_reserved_actions=0",
            "reservation-window-start-overflow",
            "call-window-start-overflow",
            (u64::MAX - 1, u64::MAX),
            "an unrepresentable calendar window start rejects",
        ),
        (
            "calendar_window_start_ms=0, calendar_window_end_ms=0",
            "reservation-window-end-overflow",
            "call-window-end-overflow",
            (0, u64::MAX),
            "an unrepresentable calendar window end rejects",
        ),
    ] {
        set_counters(columns);
        assert_eq!(
            store
                .reserve_programmatic_policy_action(reserve(
                    reservation(reference, call, "run-bounds"),
                    u64::MAX,
                    u64::MAX,
                    u64::MAX,
                    window,
                ))
                .expect_err(message)
                .code(),
            "storage_decode_failed"
        );
    }

    // A reserved reservation exercises every counter conversion on start.
    set_counters(
        "run_started_actions=0, run_reserved_actions=1, run_in_flight_actions=0, \
         calendar_started_actions=0, calendar_reserved_actions=1, \
         calendar_window_start_ms=0, calendar_window_end_ms=1000, updated_at_ms=0",
    );
    store
        .reserve_programmatic_policy_action(reserve(
            reservation("reservation-started", "call-started", "run-bounds"),
            8,
            2,
            8,
            (0, 1_000),
        ))
        .expect("the start reservation commits");
    for (columns, started_at, message) in [
        (
            "run_started_actions=9223372036854775807",
            60_u64,
            "an unrepresentable started count rejects",
        ),
        (
            "run_started_actions=0, run_in_flight_actions=9223372036854775807",
            61,
            "an unrepresentable in-flight count rejects",
        ),
        (
            "run_in_flight_actions=0, calendar_started_actions=9223372036854775807",
            62,
            "an unrepresentable calendar started count rejects",
        ),
        (
            "calendar_started_actions=0",
            u64::MAX,
            "an unrepresentable start timestamp rejects",
        ),
    ] {
        set_counters(columns);
        assert_eq!(
            store
                .commit_programmatic_reservation_started(
                    CommitProgrammaticReservationStartedInputDto {
                        reservation_reference: "reservation-started".to_owned(),
                        tool_call_id: "call-started".to_owned(),
                        started_at_ms: started_at,
                    },
                )
                .expect_err(message)
                .code(),
            "storage_decode_failed"
        );
    }

    // A deleted counter blocks the start instead of adjusting silently.
    store
        .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
            policy: policy("policy-start-bare", 1),
            revision: policy_revision("policy-start-bare", 1),
        })
        .expect("policy creates");
    let mut bare = reservation(
        "reservation-start-bare",
        "call-start-bare",
        "run-start-bare",
    );
    bare.policy_id = "policy-start-bare".to_owned();
    store
        .reserve_programmatic_policy_action(reserve(bare, 8, 2, 8, (0, 1_000)))
        .expect("the bare reservation commits");
    tamper(
        &directory,
        "DELETE FROM programmatic_policy_counters WHERE policy_id='policy-start-bare';",
    );
    assert_eq!(
        store
            .commit_programmatic_reservation_started(CommitProgrammaticReservationStartedInputDto {
                reservation_reference: "reservation-start-bare".to_owned(),
                tool_call_id: "call-start-bare".to_owned(),
                started_at_ms: 70,
            })
            .expect_err("a missing counter blocks the start")
            .code(),
        "programmatic_policy_counter_unavailable"
    );

    // A corrupt corridor bound fails the replay load through its column codec.
    let corridor = ProgrammaticAuthorizationCorridorRecordDto {
        corridor_digest: digest('c'),
        root_session_id: "session-root".to_owned(),
        root_run_id: "run-corridor-fault".to_owned(),
        root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
        policy_snapshot_reference: "snapshot-fault".to_owned(),
        required_effect_selectors: vec!["effect-write".to_owned()],
        exact_tool_or_method_selectors: vec!["tool-write".to_owned()],
        descriptor_input_constraint_selections: vec!["input-bounded".to_owned()],
        maximum_action_count: 2,
        maximum_concurrent_actions: 1,
        confirmation_reference: "confirmation-fault".to_owned(),
        state: ProgrammaticCorridorStateDto::Active,
        consumed_action_count: 0,
        created_at_ms: 33,
    };
    store
        .create_programmatic_authorization_corridor(corridor.clone())
        .expect("corridor creates");
    tamper(
        &directory,
        &format!(
            "UPDATE programmatic_authorization_corridors SET maximum_action_count=-1 \
             WHERE corridor_digest='{}';",
            digest('c')
        ),
    );
    assert_eq!(
        store
            .create_programmatic_authorization_corridor(corridor)
            .expect_err("a corrupt corridor bound is a storage fault")
            .code(),
        "storage_unavailable"
    );

    // Read faults surface as storage unavailability across every loader.
    tamper(
        &directory,
        "ALTER TABLE programmatic_policies RENAME TO programmatic_policies_hidden;",
    );
    assert_eq!(
        store
            .load_programmatic_policy("policy-hidden".to_owned())
            .expect_err("an unreadable policy table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE programmatic_policy_revisions RENAME TO programmatic_policy_revisions_hidden;",
    );
    assert_eq!(
        store
            .load_programmatic_policy_revision("policy-hidden".to_owned(), 1)
            .expect_err("an unreadable revision table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE programmatic_policy_snapshots RENAME TO programmatic_policy_snapshots_hidden;",
    );
    assert_eq!(
        store
            .load_programmatic_policy_snapshot("snapshot-hidden".to_owned())
            .expect_err("an unreadable snapshot table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE programmatic_policy_confirmations \
         RENAME TO programmatic_policy_confirmations_hidden;",
    );
    assert_eq!(
        store
            .decide_programmatic_policy_confirmation(DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: "confirmation-hidden".to_owned(),
                tool_call_id: "call-hidden".to_owned(),
                typed_input_digest: digest('b'),
                state: ProgrammaticConfirmationStateDto::Rejected,
                decided_at_ms: 10,
            })
            .expect_err("an unreadable confirmation table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE programmatic_policy_drafts RENAME TO programmatic_policy_drafts_hidden;",
    );
    assert_eq!(
        store
            .propose_programmatic_policy_draft(policy_draft(
                "draft-hidden",
                ProgrammaticPolicyScopeDto::Project {
                    project_id: "project-policy".to_owned(),
                },
                "programmatic_policy",
            ))
            .expect_err("an unreadable draft table fails the proposal")
            .code(),
        "storage_unavailable"
    );
    assert_eq!(
        store
            .decide_programmatic_policy_draft(
                "draft-hidden".to_owned(),
                ProgrammaticPolicyDraftStateDto::Rejected,
                10,
            )
            .expect_err("an unreadable draft table fails the decision")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE programmatic_policy_reservations \
         RENAME TO programmatic_policy_reservations_hidden;",
    );
    assert_eq!(
        store
            .release_programmatic_policy_reservation(ReleaseProgrammaticPolicyReservationInputDto {
                reservation_reference: "reservation-hidden".to_owned(),
                tool_call_id: "call-hidden".to_owned(),
                released_at_ms: 10,
            })
            .expect_err("an unreadable reservation table is a storage fault")
            .code(),
        "storage_unavailable"
    );
}

#[test]
fn goal_storage_faults_and_history_edges() {
    let (directory, store) = repository();

    // Re-creating an identical Goal replays; different content conflicts.
    let created = store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-replay-edge", 1),
            revision: goal_revision("goal-replay-edge", 1, 'a'),
        })
        .expect("goal creates");
    assert_eq!(
        store
            .create_goal(CreateGoalInputDto {
                goal: goal("goal-replay-edge", 1),
                revision: goal_revision("goal-replay-edge", 1, 'a'),
            })
            .expect("an identical Goal replays"),
        created
    );
    let mut altered = goal("goal-replay-edge", 1);
    altered.updated_at_ms = 99;
    assert_eq!(
        store
            .create_goal(CreateGoalInputDto {
                goal: altered,
                revision: goal_revision("goal-replay-edge", 1, 'a'),
            })
            .expect_err("different Goal content conflicts")
            .code(),
        "goal_revision_conflict"
    );

    // A pre-bound revision row replays when equal and conflicts when not.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-preseeded-edge", 1),
            revision: goal_revision("goal-preseeded-edge", 1, 'b'),
        })
        .expect("goal creates");
    tamper(
        &directory,
        "DELETE FROM goals WHERE goal_id='goal-preseeded-edge';",
    );
    assert_eq!(
        store
            .create_goal(CreateGoalInputDto {
                goal: goal("goal-preseeded-edge", 1),
                revision: goal_revision("goal-preseeded-edge", 1, 'b'),
            })
            .expect("an equal pre-bound revision replays"),
        goal("goal-preseeded-edge", 1)
    );
    tamper(
        &directory,
        "DELETE FROM goals WHERE goal_id='goal-preseeded-edge';",
    );
    assert_eq!(
        store
            .create_goal(CreateGoalInputDto {
                goal: goal("goal-preseeded-edge", 1),
                revision: goal_revision("goal-preseeded-edge", 1, 'c'),
            })
            .expect_err("an unequal pre-bound revision conflicts")
            .code(),
        "goal_revision_conflict"
    );

    // Appending replays an equal stored revision, rejects an unequal one, and
    // fails closed on an unrepresentable timestamp.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-history-edge", 1),
            revision: goal_revision("goal-history-edge", 1, 'd'),
        })
        .expect("goal creates");
    let appended = store
        .append_goal_revision(AppendGoalRevisionInputDto {
            goal_id: "goal-history-edge".to_owned(),
            expected_revision: 1,
            revision: goal_revision("goal-history-edge", 2, 'e'),
        })
        .expect("revision two appends");
    assert_eq!(appended.active_revision, 2);
    tamper(
        &directory,
        "UPDATE goals SET active_revision=1 WHERE goal_id='goal-history-edge';",
    );
    assert_eq!(
        store
            .append_goal_revision(AppendGoalRevisionInputDto {
                goal_id: "goal-history-edge".to_owned(),
                expected_revision: 1,
                revision: goal_revision("goal-history-edge", 2, 'e'),
            })
            .expect("an equal stored revision replays"),
        appended
    );
    let mut overflowing = goal_revision("goal-history-edge", 3, 'f');
    overflowing.created_at_ms = u64::MAX;
    assert_eq!(
        store
            .append_goal_revision(AppendGoalRevisionInputDto {
                goal_id: "goal-history-edge".to_owned(),
                expected_revision: 2,
                revision: overflowing,
            })
            .expect_err("an unrepresentable revision timestamp rejects")
            .code(),
        "storage_decode_failed"
    );
    tamper(
        &directory,
        "UPDATE goals SET active_revision=1 WHERE goal_id='goal-history-edge';",
    );
    assert_eq!(
        store
            .append_goal_revision(AppendGoalRevisionInputDto {
                goal_id: "goal-history-edge".to_owned(),
                expected_revision: 1,
                revision: goal_revision("goal-history-edge", 2, 'g'),
            })
            .expect_err("an unequal stored revision conflicts")
            .code(),
        "goal_revision_conflict"
    );

    // A stopped Goal cannot pause.
    store
        .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
            goal_id: "goal-replay-edge".to_owned(),
            expected_revision: 1,
            lifecycle_state: GoalLifecycleStateDto::Stopped,
            occurred_at_ms: 60,
        })
        .expect("the Goal stops");
    assert_eq!(
        store
            .transition_goal_lifecycle(TransitionGoalLifecycleInputDto {
                goal_id: "goal-replay-edge".to_owned(),
                expected_revision: 1,
                lifecycle_state: GoalLifecycleStateDto::Paused,
                occurred_at_ms: 61,
            })
            .expect_err("a stopped Goal cannot pause")
            .code(),
        "goal_not_active"
    );

    // An unrepresentable link revision fails the attachment atomically.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-parent-attach", 1),
            revision: goal_revision("goal-parent-attach", 1, '3'),
        })
        .expect("parent creates");
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-child-attach", 1),
            revision: goal_revision("goal-child-attach", 1, '4'),
        })
        .expect("child creates");
    assert_eq!(
        store
            .attach_goal_child(AttachGoalChildInputDto {
                parent_goal_id: "goal-parent-attach".to_owned(),
                child_goal_id: "goal-child-attach".to_owned(),
                child_revision_at_link: u64::MAX,
                canonical_link_digest: digest('5'),
                created_at_ms: 30,
            })
            .expect_err("an unrepresentable child revision rejects")
            .code(),
        "storage_decode_failed"
    );

    // A pre-bound gate revision row fails the gate creation through its insert.
    store
        .create_goal(CreateGoalInputDto {
            goal: goal("goal-gate-pre-edge", 1),
            revision: goal_revision("goal-gate-pre-edge", 1, '6'),
        })
        .expect("gate owner creates");
    tamper(
        &directory,
        "INSERT INTO goal_gate_revisions(gate_id, revision, definition_kind, definition, \
         canonical_revision_digest, created_at_ms) \
         VALUES ('gate-pre-bound', 1, 'reference', 'wire;other', 'sha256:prebound', 30);",
    );
    assert_eq!(
        store
            .create_goal_gate(CreateGoalGateInputDto {
                gate: reference_gate("gate-pre-bound", "goal-gate-pre-edge", '7'),
            })
            .expect_err("a pre-bound gate revision rejects")
            .code(),
        "storage_unavailable"
    );

    // An unrepresentable gate timestamp fails the atomic revision append.
    store
        .create_goal_gate(CreateGoalGateInputDto {
            gate: reference_gate("gate-append-overflow", "goal-gate-pre-edge", '8'),
        })
        .expect("gate creates");
    let definition = reference_gate("gate-append-overflow", "goal-gate-pre-edge", '8').definition;
    assert_eq!(
        store
            .append_goal_gate_revision(AppendGoalGateRevisionInputDto {
                gate_id: "gate-append-overflow".to_owned(),
                expected_revision: 1,
                definition,
                canonical_revision_digest: digest('9'),
                occurred_at_ms: u64::MAX,
            })
            .expect_err("an unrepresentable gate timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Authority revisions and issuance stay inside the SQLite range.
    let mut revision_overflow = authority(
        "authority-revision-overflow",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    revision_overflow.authority_revision = u64::MAX;
    assert_eq!(
        store
            .record_verifier_authority(revision_overflow)
            .expect_err("an unrepresentable authority revision rejects")
            .code(),
        "storage_decode_failed"
    );
    let mut contract_overflow = authority(
        "authority-contract-overflow",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    contract_overflow.audit_contract_reference.contract_revision = u64::MAX;
    assert_eq!(
        store
            .record_verifier_authority(contract_overflow)
            .expect_err("an unrepresentable contract revision rejects")
            .code(),
        "storage_decode_failed"
    );
    let mut issued_overflow = authority(
        "authority-issued-overflow",
        VerifierAuthorityConsumptionRuleDto::ReusableWhileActive,
    );
    issued_overflow.issued_at_ms = u64::MAX;
    issued_overflow.expires_at_ms = None;
    assert_eq!(
        store
            .record_verifier_authority(issued_overflow)
            .expect_err("an unrepresentable issuance timestamp rejects")
            .code(),
        "storage_decode_failed"
    );

    // Read faults surface as storage unavailability across every loader.
    tamper(&directory, "ALTER TABLE goals RENAME TO goals_hidden;");
    assert_eq!(
        store
            .load_goal("goal-hidden".to_owned())
            .expect_err("an unreadable Goal table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE goal_gates RENAME TO goal_gates_hidden;",
    );
    assert_eq!(
        store
            .load_goal_gate("gate-hidden".to_owned())
            .expect_err("an unreadable gate table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE goal_gate_results RENAME TO goal_gate_results_hidden;",
    );
    assert_eq!(
        store
            .load_goal_gate_result("gate-hidden".to_owned(), 1)
            .expect_err("an unreadable gate result table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE goal_skill_cards RENAME TO goal_skill_cards_hidden;",
    );
    assert_eq!(
        store
            .load_goal_skill_card("skill-hidden".to_owned(), 1)
            .expect_err("an unreadable Skill card table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE goal_role_cards RENAME TO goal_role_cards_hidden;",
    );
    assert_eq!(
        store
            .load_goal_role_card("role-hidden".to_owned(), 1)
            .expect_err("an unreadable role card table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE refinement_drafts RENAME TO refinement_drafts_hidden;",
    );
    assert_eq!(
        store
            .decide_refinement_draft(
                "draft-hidden".to_owned(),
                RefinementDraftStateDto::Rejected,
                30,
            )
            .expect_err("an unreadable draft table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE verifier_authorities RENAME TO verifier_authorities_hidden;",
    );
    assert_eq!(
        store
            .load_verifier_authority("authority-hidden".to_owned(), 1)
            .expect_err("an unreadable authority table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE verifier_audit_evidence RENAME TO verifier_audit_evidence_hidden;",
    );
    assert_eq!(
        store
            .load_verifier_audit_evidence("evidence-hidden".to_owned())
            .expect_err("an unreadable evidence table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE verifier_audit_verdicts RENAME TO verifier_audit_verdicts_hidden;",
    );
    assert_eq!(
        store
            .load_verifier_audit_verdict("verdict-hidden".to_owned())
            .expect_err("an unreadable verdict table is a storage fault")
            .code(),
        "storage_unavailable"
    );
    tamper(
        &directory,
        "ALTER TABLE verifier_target_mutations \
         RENAME TO verifier_target_mutations_hidden;",
    );
    assert_eq!(
        store
            .load_verifier_target_mutation("mutation-hidden".to_owned())
            .expect_err("an unreadable mutation table is a storage fault")
            .code(),
        "storage_unavailable"
    );
}
