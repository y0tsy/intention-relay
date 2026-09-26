//! Slice 3 DTO contract tests for the durable harness and programmatic-caller
//! policy storage modules.
//!
//! Every public discriminator, accessor, scope projection, and record validator
//! of `intention_storage::harness_repo` and
//! `intention_storage::programmatic_policy_repo` is exercised for its accepted
//! canonical value and for each closed rejection branch: unknown
//! discriminators, blank and control-bearing identities, credential-shaped
//! values, non-canonical digests, oversized labels and content, exact length
//! and count bounds, and incoherent cross-field combinations.

use intention_domain::harness::HARNESS_MIN_INTERVAL_MS;
use intention_domain::programmatic_policy::{
    MAX_CALENDAR_ACTION_LIMIT, MAX_POLICY_ACTIONS_PER_RUN, MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN,
    MAX_RULES_PER_POLICY_REVISION, MIN_CALENDAR_ACTION_LIMIT,
};
use intention_domain::slice3_selections::{
    HARNESS_MAX_CHECKPOINT_BYTES, HARNESS_MAX_CONCLUSION_BYTES, HARNESS_MAX_DOSSIER_BYTES,
};
use intention_storage::harness_repo::{
    AppendHarnessJournalRecordInputDto, CaptureHarnessTriggerInputDto,
    CommitHarnessCheckpointInputDto, CommitHarnessCheckpointOutcomeDto, CreateHarnessRuleInputDto,
    HarnessCheckpointDispositionDto, HarnessCheckpointRecordDto, HarnessConclusionRecordDto,
    HarnessCounterRecordDto, HarnessDossierRecordDto, HarnessExecutionClassDto,
    HarnessJournalRecordDto, HarnessJournalRecordKindDto, HarnessPresentationModeDto,
    HarnessRuleLifecycleStateDto, HarnessRuleOperationDto, HarnessRuleRecordDto,
    HarnessRuleRevisionRecordDto, HarnessRuleScopeDto, HarnessRunOutcomeDto, HarnessSourceKindDto,
    HarnessTaskModeDto, HarnessTriggerCaptureOutcomeDto, HarnessTriggerCaptureRecordDto,
    HarnessTriggerReasonRecordDto, HarnessTriggerReasonStateDto, LoadHarnessJournalInputDto,
    MAX_HARNESS_JOURNAL_PAGE, MAX_HARNESS_RULE_SOURCES, MAX_HARNESS_TRIGGER_REFERENCES,
    RecordHarnessLaunchInputDto, RecordHarnessLaunchOutcomeDto, ReviseHarnessRuleInputDto,
    TransitionHarnessRuleLifecycleInputDto,
};
use intention_storage::programmatic_policy_repo::{
    AppendProgrammaticPolicyRevisionInputDto, CommitProgrammaticReservationStartedInputDto,
    ConsumeProgrammaticCorridorActionInputDto, CreateProgrammaticPolicyInputDto,
    DecideProgrammaticPolicyConfirmationInputDto, MAX_POLICY_INHERITED_REFERENCES,
    MAX_POLICY_RECORD_PAGE, MAX_POLICY_SELECTORS, MAX_POLICY_SNAPSHOT_REFERENCES,
    ProgrammaticAdmissionDecisionDto, ProgrammaticAuthorizationCorridorRecordDto,
    ProgrammaticCalendarCounterReferenceDto, ProgrammaticCalendarLimitRecordDto,
    ProgrammaticCalendarPeriodKindDto, ProgrammaticConfirmationStateDto,
    ProgrammaticCorridorStateDto, ProgrammaticDraftCoalescingDto,
    ProgrammaticPolicyConfirmationRecordDto, ProgrammaticPolicyCounterRecordDto,
    ProgrammaticPolicyDraftRecordDto, ProgrammaticPolicyDraftStateDto,
    ProgrammaticPolicyLifecycleOperationDto, ProgrammaticPolicyLifecycleStateDto,
    ProgrammaticPolicyRecordDto, ProgrammaticPolicyReservationRecordDto,
    ProgrammaticPolicyRevisionRecordDto, ProgrammaticPolicyRevisionReferenceDto,
    ProgrammaticPolicyScopeDto, ProgrammaticPolicySnapshotRecordDto,
    ProgrammaticReservationStateDto, ProgrammaticRootOriginKindDto,
    ProgrammaticRootOriginRuleRecordDto, RecoverProgrammaticPolicyReservationInputDto,
    ReleaseProgrammaticPolicyReservationInputDto, ReserveProgrammaticPolicyActionInputDto,
    ReserveProgrammaticPolicyActionOutcomeDto, TransitionProgrammaticPolicyLifecycleInputDto,
};
use intention_storage::{MAX_SAFE_CONTENT_BYTES, MAX_SAFE_LABEL_CHARS};
use intention_types::DtoResult;

/// Builds one canonical `sha256:<64 lowercase hex>` digest from a hex seed.
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

/// Builds one bounded list of distinct safe labels.
fn labels(prefix: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("{prefix}-{index}"))
        .collect()
}

/// Returns the closed error code of a rejected DTO operation.
fn rejection_code<T: std::fmt::Debug>(result: DtoResult<T>) -> String {
    assert!(result.is_err(), "an invalid value must be rejected");
    result
        .err()
        .map(|error| error.code().to_owned())
        .unwrap_or_default()
}

/// Asserts that a canonical DTO value passes its own validation.
fn assert_accepted<T: std::fmt::Debug>(result: DtoResult<T>) {
    assert!(
        result.is_ok(),
        "the canonical value must be accepted: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Harness fixtures.
// ---------------------------------------------------------------------------

fn harness_rule() -> HarnessRuleRecordDto {
    HarnessRuleRecordDto {
        harness_id: "harness-a".to_owned(),
        scope: HarnessRuleScopeDto::Project {
            project_id: "project-a".to_owned(),
        },
        lifecycle_state: HarnessRuleLifecycleStateDto::Active,
        active_revision: 1,
        service_session_id: "service-session-a".to_owned(),
        updated_at_ms: 10,
    }
}

fn harness_revision() -> HarnessRuleRevisionRecordDto {
    HarnessRuleRevisionRecordDto {
        harness_id: "harness-a".to_owned(),
        revision: 1,
        task_digest: digest('a'),
        class: HarnessExecutionClassDto::Light,
        task_mode: HarnessTaskModeDto::RepeatedTask,
        presentation_mode: HarnessPresentationModeDto::JournalOnly,
        applied_time_zone: "Europe/Amsterdam".to_owned(),
        source_kinds: vec![HarnessSourceKindDto::ExplicitUserLaunch],
        source_references: vec!["source-a".to_owned()],
        interval_anchor_ms: None,
        interval_ms: None,
        calendar_expression: None,
        completion_link_reference: None,
        completion_outcomes: Vec::new(),
        canonical_revision_digest: digest('b'),
        created_at_ms: 10,
    }
}

fn trigger_reason() -> HarnessTriggerReasonRecordDto {
    HarnessTriggerReasonRecordDto {
        reason_id: "reason-a".to_owned(),
        harness_id: "harness-a".to_owned(),
        source_kind: HarnessSourceKindDto::ExplicitUserLaunch,
        rule_revision: 1,
        first_observed_at_ms: 10,
        last_observed_at_ms: 20,
        coalesced_count: 2,
        applied_time_zone: "UTC".to_owned(),
        cause_chain_reference: None,
        bounded_references: vec!["reference-a".to_owned()],
        state: HarnessTriggerReasonStateDto::Pending,
    }
}

fn dossier() -> HarnessDossierRecordDto {
    HarnessDossierRecordDto {
        dossier_id: "dossier-a".to_owned(),
        harness_id: "harness-a".to_owned(),
        rule_revision: 1,
        reason_id: "reason-a".to_owned(),
        task_digest: digest('a'),
        source_references: vec!["source-a".to_owned()],
        typed_references: vec!["reference-a".to_owned()],
        checkpoint_reference: None,
        dossier_bytes: 1024,
        canonical_dossier_digest: digest('c'),
        created_at_ms: 30,
    }
}

fn checkpoint() -> HarnessCheckpointRecordDto {
    HarnessCheckpointRecordDto {
        checkpoint_id: "checkpoint-a".to_owned(),
        harness_id: "harness-a".to_owned(),
        rule_revision: 1,
        producing_run_id: "run-a".to_owned(),
        checkpoint_revision: 1,
        content_digest: digest('d'),
        checkpoint_bytes: 2048,
        is_current: true,
        created_at_ms: 40,
    }
}

fn conclusion() -> HarnessConclusionRecordDto {
    HarnessConclusionRecordDto {
        harness_id: "harness-a".to_owned(),
        producing_run_id: "run-a".to_owned(),
        content_digest: digest('e'),
        conclusion_bytes: 512,
        presentation_mode: HarnessPresentationModeDto::JournalOnly,
        created_at_ms: 50,
    }
}

fn journal_record() -> HarnessJournalRecordDto {
    HarnessJournalRecordDto {
        harness_id: "harness-a".to_owned(),
        sequence: 1,
        record_kind: HarnessJournalRecordKindDto::TriggerCaptured,
        safe_summary: "harness trigger captured".to_owned(),
        canonical_record_digest: digest('f'),
        occurred_at_ms: 60,
    }
}

// ---------------------------------------------------------------------------
// Harness discriminators, accessors, and validators.
// ---------------------------------------------------------------------------

#[test]
fn harness_rule_scope_accessors_cover_project_and_user_session_owners() {
    let project = HarnessRuleScopeDto::Project {
        project_id: "project-a".to_owned(),
    };
    assert_eq!(project.project_id(), "project-a");
    assert_eq!(project.session_id(), None);
    assert_eq!(project.kind_name(), "project");
    assert_accepted(project.validate());

    let session = HarnessRuleScopeDto::UserSession {
        project_id: "project-a".to_owned(),
        session_id: "session-a".to_owned(),
    };
    assert_eq!(session.project_id(), "project-a");
    assert_eq!(session.session_id(), Some("session-a"));
    assert_eq!(session.kind_name(), "user_session");
    assert_accepted(session.validate());

    let cloned = session.clone();
    assert_eq!(cloned, session);
    assert!(!format!("{cloned:?}").is_empty());

    assert_eq!(
        rejection_code(
            HarnessRuleScopeDto::Project {
                project_id: "   ".to_owned(),
            }
            .validate()
        ),
        "invalid_harness_rule"
    );
    assert_eq!(
        rejection_code(
            HarnessRuleScopeDto::Project {
                project_id: "line\nbreak".to_owned(),
            }
            .validate()
        ),
        "invalid_harness_rule"
    );
    assert_eq!(
        rejection_code(
            HarnessRuleScopeDto::Project {
                project_id: format!("project-{}", "x".repeat(MAX_SAFE_LABEL_CHARS)),
            }
            .validate()
        ),
        "invalid_harness_rule"
    );
    assert_eq!(
        rejection_code(
            HarnessRuleScopeDto::UserSession {
                project_id: "project-a".to_owned(),
                session_id: "token=live".to_owned(),
            }
            .validate()
        ),
        "credentials_forbidden"
    );
}

#[test]
fn harness_rule_lifecycle_states_parse_each_closed_name() {
    for state in [
        HarnessRuleLifecycleStateDto::Active,
        HarnessRuleLifecycleStateDto::Paused,
        HarnessRuleLifecycleStateDto::Archived,
    ] {
        assert_eq!(
            HarnessRuleLifecycleStateDto::parse(state.name()).ok(),
            Some(state)
        );
    }
    assert_eq!(HarnessRuleLifecycleStateDto::Active.name(), "active");
    assert_eq!(HarnessRuleLifecycleStateDto::Paused.name(), "paused");
    assert_eq!(HarnessRuleLifecycleStateDto::Archived.name(), "archived");
    assert_eq!(
        rejection_code(HarnessRuleLifecycleStateDto::parse("deleted")),
        "harness_not_active"
    );
}

#[test]
fn harness_rule_operations_and_source_kinds_expose_closed_names() {
    assert_eq!(
        HarnessRuleOperationDto::UpdateRevision.name(),
        "update_revision"
    );
    assert_eq!(HarnessRuleOperationDto::Pause.name(), "pause");
    assert_eq!(HarnessRuleOperationDto::Resume.name(), "resume");
    assert_eq!(
        HarnessRuleOperationDto::ExplicitLaunch.name(),
        "explicit_launch"
    );
    assert_eq!(
        HarnessRuleOperationDto::CancelActiveRun.name(),
        "cancel_active_run"
    );
    assert_eq!(HarnessRuleOperationDto::Archive.name(), "archive");

    for kind in [
        HarnessSourceKindDto::ExplicitUserLaunch,
        HarnessSourceKindDto::CalendarTime,
        HarnessSourceKindDto::FixedInterval,
        HarnessSourceKindDto::TerminalOutcomeLink,
    ] {
        assert_eq!(HarnessSourceKindDto::parse(kind.name()).ok(), Some(kind));
    }
    assert_eq!(
        HarnessSourceKindDto::ExplicitUserLaunch.name(),
        "explicit_user_launch"
    );
    assert_eq!(HarnessSourceKindDto::CalendarTime.name(), "calendar_time");
    assert_eq!(HarnessSourceKindDto::FixedInterval.name(), "fixed_interval");
    assert_eq!(
        HarnessSourceKindDto::TerminalOutcomeLink.name(),
        "terminal_outcome_link"
    );
    assert_eq!(
        rejection_code(HarnessSourceKindDto::parse("poll")),
        "harness_source_unavailable"
    );
}

#[test]
fn harness_task_presentation_and_execution_discriminators_parse() {
    for mode in [
        HarnessTaskModeDto::RepeatedTask,
        HarnessTaskModeDto::GoalDirected,
    ] {
        assert_eq!(HarnessTaskModeDto::parse(mode.name()).ok(), Some(mode));
    }
    assert_eq!(HarnessTaskModeDto::RepeatedTask.name(), "repeated_task");
    assert_eq!(HarnessTaskModeDto::GoalDirected.name(), "goal_directed");
    assert_eq!(
        rejection_code(HarnessTaskModeDto::parse("freeform")),
        "harness_revision_conflict"
    );

    for mode in [
        HarnessPresentationModeDto::JournalOnly,
        HarnessPresentationModeDto::JournalAndActivityEntry,
    ] {
        assert_eq!(
            HarnessPresentationModeDto::parse(mode.name()).ok(),
            Some(mode)
        );
    }
    assert_eq!(
        HarnessPresentationModeDto::JournalOnly.name(),
        "journal_only"
    );
    assert_eq!(
        HarnessPresentationModeDto::JournalAndActivityEntry.name(),
        "journal_and_activity_entry"
    );
    assert_eq!(
        rejection_code(HarnessPresentationModeDto::parse("toast")),
        "harness_revision_conflict"
    );

    for class in [
        HarnessExecutionClassDto::Light,
        HarnessExecutionClassDto::Medium,
        HarnessExecutionClassDto::Heavy,
    ] {
        assert_eq!(
            HarnessExecutionClassDto::parse(class.name()).ok(),
            Some(class)
        );
    }
    assert_eq!(HarnessExecutionClassDto::Light.name(), "light");
    assert_eq!(HarnessExecutionClassDto::Medium.name(), "medium");
    assert_eq!(HarnessExecutionClassDto::Heavy.name(), "heavy");
    assert_eq!(
        rejection_code(HarnessExecutionClassDto::parse("turbo")),
        "invalid_harness_class_resolution"
    );
}

#[test]
fn harness_capture_outcomes_and_reason_states_track_pending_changes() {
    assert_eq!(HarnessTriggerCaptureOutcomeDto::Captured.name(), "captured");
    assert_eq!(
        HarnessTriggerCaptureOutcomeDto::Coalesced.name(),
        "coalesced"
    );
    assert_eq!(
        HarnessTriggerCaptureOutcomeDto::Redelivered.name(),
        "redelivered"
    );
    assert_eq!(HarnessTriggerCaptureOutcomeDto::CatchUp.name(), "catch_up");
    assert!(HarnessTriggerCaptureOutcomeDto::Captured.changed_pending_reason());
    assert!(HarnessTriggerCaptureOutcomeDto::Coalesced.changed_pending_reason());
    assert!(HarnessTriggerCaptureOutcomeDto::CatchUp.changed_pending_reason());
    assert!(!HarnessTriggerCaptureOutcomeDto::Redelivered.changed_pending_reason());

    for state in [
        HarnessTriggerReasonStateDto::Pending,
        HarnessTriggerReasonStateDto::Admitted,
    ] {
        assert_eq!(
            HarnessTriggerReasonStateDto::parse(state.name()).ok(),
            Some(state)
        );
    }
    assert_eq!(HarnessTriggerReasonStateDto::Pending.name(), "pending");
    assert_eq!(HarnessTriggerReasonStateDto::Admitted.name(), "admitted");
    assert_eq!(
        rejection_code(HarnessTriggerReasonStateDto::parse("consumed")),
        "harness_source_unavailable"
    );
}

#[test]
fn harness_run_checkpoint_and_journal_discriminators_expose_names() {
    assert_eq!(HarnessRunOutcomeDto::Completed.name(), "completed");
    assert_eq!(HarnessRunOutcomeDto::Failed.name(), "failed");
    assert_eq!(HarnessRunOutcomeDto::Cancelled.name(), "cancelled");
    assert_eq!(HarnessRunOutcomeDto::Interrupted.name(), "interrupted");
    assert_eq!(
        HarnessRunOutcomeDto::ExternalEffectUnknown.name(),
        "external_effect_unknown"
    );

    assert_eq!(HarnessCheckpointDispositionDto::Replaced.name(), "replaced");
    assert_eq!(
        HarnessCheckpointDispositionDto::RetainedPrevious.name(),
        "retained_previous"
    );

    for kind in [
        HarnessJournalRecordKindDto::TriggerCaptured,
        HarnessJournalRecordKindDto::LaunchAdmitted,
        HarnessJournalRecordKindDto::LaunchRetained,
        HarnessJournalRecordKindDto::RunTerminal,
        HarnessJournalRecordKindDto::CheckpointAccepted,
        HarnessJournalRecordKindDto::CheckpointRetained,
        HarnessJournalRecordKindDto::ConclusionPublished,
    ] {
        assert_eq!(
            HarnessJournalRecordKindDto::parse(kind.name()).ok(),
            Some(kind)
        );
    }
    assert_eq!(
        HarnessJournalRecordKindDto::TriggerCaptured.name(),
        "trigger_captured"
    );
    assert_eq!(
        HarnessJournalRecordKindDto::LaunchAdmitted.name(),
        "launch_admitted"
    );
    assert_eq!(
        HarnessJournalRecordKindDto::LaunchRetained.name(),
        "launch_retained"
    );
    assert_eq!(
        HarnessJournalRecordKindDto::RunTerminal.name(),
        "run_terminal"
    );
    assert_eq!(
        HarnessJournalRecordKindDto::CheckpointAccepted.name(),
        "checkpoint_accepted"
    );
    assert_eq!(
        HarnessJournalRecordKindDto::CheckpointRetained.name(),
        "checkpoint_retained"
    );
    assert_eq!(
        HarnessJournalRecordKindDto::ConclusionPublished.name(),
        "conclusion_published"
    );
    assert_eq!(
        rejection_code(HarnessJournalRecordKindDto::parse("unknown_kind")),
        "storage_decode_failed"
    );
}

#[test]
fn harness_rule_record_validation_covers_identity_scope_and_revision() {
    let rule = harness_rule();
    assert_accepted(rule.validate());
    let cloned = rule.clone();
    assert_eq!(cloned, rule);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = HarnessRuleRecordDto {
        harness_id: "  ".to_owned(),
        ..harness_rule()
    };
    assert_eq!(rejection_code(invalid.validate()), "invalid_harness_rule");

    let invalid = HarnessRuleRecordDto {
        service_session_id: "control\u{0}byte".to_owned(),
        ..harness_rule()
    };
    assert_eq!(rejection_code(invalid.validate()), "invalid_harness_rule");

    let invalid = HarnessRuleRecordDto {
        scope: HarnessRuleScopeDto::Project {
            project_id: String::new(),
        },
        ..harness_rule()
    };
    assert_eq!(rejection_code(invalid.validate()), "invalid_harness_rule");

    let invalid = HarnessRuleRecordDto {
        active_revision: 0,
        ..harness_rule()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );
}

#[test]
fn harness_rule_revision_validation_covers_sources_schedules_and_boundaries() {
    let revision = harness_revision();
    assert_accepted(revision.validate());
    let cloned = revision.clone();
    assert_eq!(cloned, revision);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = HarnessRuleRevisionRecordDto {
        harness_id: String::new(),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        revision: 0,
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        task_digest: "sha256:short".to_owned(),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        canonical_revision_digest: format!("sha256:{}", "A".repeat(64)),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        canonical_revision_digest: format!("sha256:{}", "z".repeat(64)),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let accepted = HarnessRuleRevisionRecordDto {
        task_digest: format!("sha256:{}", "0123456789abcdef".repeat(4)),
        canonical_revision_digest: format!("sha256:{}", "fedcba9876543210".repeat(4)),
        ..harness_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessRuleRevisionRecordDto {
        applied_time_zone: " \t ".to_owned(),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_schedule_invalid"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        source_kinds: Vec::new(),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        source_kinds: vec![HarnessSourceKindDto::ExplicitUserLaunch; MAX_HARNESS_RULE_SOURCES + 1],
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let accepted = HarnessRuleRevisionRecordDto {
        source_kinds: vec![HarnessSourceKindDto::ExplicitUserLaunch; MAX_HARNESS_RULE_SOURCES],
        ..harness_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessRuleRevisionRecordDto {
        source_references: labels("source", MAX_HARNESS_TRIGGER_REFERENCES + 1),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let accepted = HarnessRuleRevisionRecordDto {
        source_references: labels("source", MAX_HARNESS_TRIGGER_REFERENCES),
        ..harness_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessRuleRevisionRecordDto {
        source_references: vec!["password=hunter2".to_owned()],
        ..harness_revision()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let accepted = HarnessRuleRevisionRecordDto {
        source_kinds: vec![HarnessSourceKindDto::FixedInterval],
        interval_anchor_ms: Some(0),
        interval_ms: Some(HARNESS_MIN_INTERVAL_MS),
        ..harness_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessRuleRevisionRecordDto {
        source_kinds: vec![HarnessSourceKindDto::FixedInterval],
        interval_anchor_ms: Some(0),
        interval_ms: Some(HARNESS_MIN_INTERVAL_MS - 1),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_interval_too_short"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        source_kinds: vec![HarnessSourceKindDto::FixedInterval],
        interval_anchor_ms: None,
        interval_ms: Some(HARNESS_MIN_INTERVAL_MS),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_schedule_invalid"
    );

    let accepted = HarnessRuleRevisionRecordDto {
        source_kinds: vec![HarnessSourceKindDto::CalendarTime],
        calendar_expression: Some("daily-at-09:00".to_owned()),
        ..harness_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessRuleRevisionRecordDto {
        source_kinds: vec![HarnessSourceKindDto::CalendarTime],
        calendar_expression: None,
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_schedule_invalid"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        source_kinds: vec![HarnessSourceKindDto::CalendarTime],
        calendar_expression: Some("   ".to_owned()),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_schedule_invalid"
    );

    let invalid = HarnessRuleRevisionRecordDto {
        completion_outcomes: labels("outcome", MAX_HARNESS_RULE_SOURCES + 1),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let accepted = HarnessRuleRevisionRecordDto {
        completion_outcomes: labels("outcome", MAX_HARNESS_RULE_SOURCES),
        ..harness_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessRuleRevisionRecordDto {
        completion_outcomes: vec!["token=live".to_owned()],
        ..harness_revision()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let invalid = HarnessRuleRevisionRecordDto {
        completion_link_reference: Some(" ".to_owned()),
        ..harness_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let accepted = HarnessRuleRevisionRecordDto {
        completion_link_reference: Some("completion-link-a".to_owned()),
        ..harness_revision()
    };
    assert_accepted(accepted.validate());
}

#[test]
fn harness_create_rule_input_requires_coherent_first_revision() {
    let input = CreateHarnessRuleInputDto {
        rule: harness_rule(),
        revision: harness_revision(),
    };
    assert_accepted(input.validate());
    let cloned = input.clone();
    assert_eq!(cloned, input);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = CreateHarnessRuleInputDto {
        rule: HarnessRuleRecordDto {
            harness_id: " ".to_owned(),
            ..harness_rule()
        },
        revision: harness_revision(),
    };
    assert_eq!(rejection_code(invalid.validate()), "invalid_harness_rule");

    let invalid = CreateHarnessRuleInputDto {
        rule: harness_rule(),
        revision: HarnessRuleRevisionRecordDto {
            task_digest: "sha256:short".to_owned(),
            ..harness_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = CreateHarnessRuleInputDto {
        rule: harness_rule(),
        revision: HarnessRuleRevisionRecordDto {
            harness_id: "harness-b".to_owned(),
            ..harness_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = CreateHarnessRuleInputDto {
        rule: harness_rule(),
        revision: HarnessRuleRevisionRecordDto {
            revision: 2,
            ..harness_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = CreateHarnessRuleInputDto {
        rule: HarnessRuleRecordDto {
            active_revision: 2,
            ..harness_rule()
        },
        revision: harness_revision(),
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );
}

#[test]
fn harness_revise_rule_input_requires_the_next_revision_of_the_same_harness() {
    let input = ReviseHarnessRuleInputDto {
        harness_id: "harness-a".to_owned(),
        expected_revision: 1,
        revision: HarnessRuleRevisionRecordDto {
            revision: 2,
            ..harness_revision()
        },
    };
    assert_accepted(input.validate());

    let invalid = ReviseHarnessRuleInputDto {
        harness_id: "harness-a".to_owned(),
        expected_revision: 1,
        revision: HarnessRuleRevisionRecordDto {
            harness_id: " ".to_owned(),
            revision: 2,
            ..harness_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = ReviseHarnessRuleInputDto {
        harness_id: "harness-a".to_owned(),
        expected_revision: u64::MAX,
        revision: harness_revision(),
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = ReviseHarnessRuleInputDto {
        harness_id: "harness-b".to_owned(),
        expected_revision: 1,
        revision: HarnessRuleRevisionRecordDto {
            revision: 2,
            ..harness_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = ReviseHarnessRuleInputDto {
        harness_id: "harness-a".to_owned(),
        expected_revision: 1,
        revision: HarnessRuleRevisionRecordDto {
            revision: 3,
            ..harness_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );
}

#[test]
fn harness_trigger_reason_validation_covers_identities_windows_and_references() {
    let reason = trigger_reason();
    assert_accepted(reason.validate());
    let cloned = reason.clone();
    assert_eq!(cloned, reason);
    assert!(!format!("{cloned:?}").is_empty());

    let accepted = HarnessTriggerReasonRecordDto {
        first_observed_at_ms: 10,
        last_observed_at_ms: 10,
        ..trigger_reason()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessTriggerReasonRecordDto {
        reason_id: " ".to_owned(),
        ..trigger_reason()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessTriggerReasonRecordDto {
        harness_id: " ".to_owned(),
        ..trigger_reason()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessTriggerReasonRecordDto {
        applied_time_zone: " ".to_owned(),
        ..trigger_reason()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_schedule_invalid"
    );

    let invalid = HarnessTriggerReasonRecordDto {
        bounded_references: labels("reference", MAX_HARNESS_TRIGGER_REFERENCES + 1),
        ..trigger_reason()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let accepted = HarnessTriggerReasonRecordDto {
        bounded_references: labels("reference", MAX_HARNESS_TRIGGER_REFERENCES),
        ..trigger_reason()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessTriggerReasonRecordDto {
        bounded_references: vec!["api_key=live".to_owned()],
        ..trigger_reason()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let invalid = HarnessTriggerReasonRecordDto {
        rule_revision: 0,
        ..trigger_reason()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = HarnessTriggerReasonRecordDto {
        coalesced_count: 0,
        ..trigger_reason()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessTriggerReasonRecordDto {
        first_observed_at_ms: 20,
        last_observed_at_ms: 10,
        ..trigger_reason()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );
}

#[test]
fn harness_dossier_validation_covers_references_and_byte_bound() {
    let record = dossier();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = HarnessDossierRecordDto {
        dossier_id: " ".to_owned(),
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessDossierRecordDto {
        harness_id: " ".to_owned(),
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessDossierRecordDto {
        reason_id: " ".to_owned(),
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessDossierRecordDto {
        task_digest: "sha256:short".to_owned(),
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessDossierRecordDto {
        canonical_dossier_digest: "sha256:short".to_owned(),
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessDossierRecordDto {
        rule_revision: 0,
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = HarnessDossierRecordDto {
        source_references: labels("source", MAX_HARNESS_RULE_SOURCES + 1),
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_dossier_too_large"
    );

    let invalid = HarnessDossierRecordDto {
        typed_references: labels("reference", MAX_HARNESS_TRIGGER_REFERENCES + 1),
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_dossier_too_large"
    );

    let accepted = HarnessDossierRecordDto {
        source_references: labels("source", MAX_HARNESS_RULE_SOURCES),
        typed_references: labels("reference", MAX_HARNESS_TRIGGER_REFERENCES),
        ..dossier()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessDossierRecordDto {
        source_references: vec!["token=live".to_owned()],
        ..dossier()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let accepted = HarnessDossierRecordDto {
        dossier_bytes: HARNESS_MAX_DOSSIER_BYTES,
        ..dossier()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessDossierRecordDto {
        dossier_bytes: HARNESS_MAX_DOSSIER_BYTES + 1,
        ..dossier()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_dossier_too_large"
    );
}

#[test]
fn harness_checkpoint_validation_covers_identities_revisions_and_bytes() {
    let record = checkpoint();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = HarnessCheckpointRecordDto {
        checkpoint_id: " ".to_owned(),
        ..checkpoint()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_checkpoint_unavailable"
    );

    let invalid = HarnessCheckpointRecordDto {
        harness_id: " ".to_owned(),
        ..checkpoint()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_checkpoint_unavailable"
    );

    let invalid = HarnessCheckpointRecordDto {
        producing_run_id: " ".to_owned(),
        ..checkpoint()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_checkpoint_unavailable"
    );

    let invalid = HarnessCheckpointRecordDto {
        content_digest: "sha256:short".to_owned(),
        ..checkpoint()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_checkpoint_unavailable"
    );

    let invalid = HarnessCheckpointRecordDto {
        rule_revision: 0,
        ..checkpoint()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let invalid = HarnessCheckpointRecordDto {
        checkpoint_revision: 0,
        ..checkpoint()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );

    let accepted = HarnessCheckpointRecordDto {
        checkpoint_bytes: HARNESS_MAX_CHECKPOINT_BYTES,
        ..checkpoint()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessCheckpointRecordDto {
        checkpoint_bytes: HARNESS_MAX_CHECKPOINT_BYTES + 1,
        ..checkpoint()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_checkpoint_too_large"
    );
}

#[test]
fn harness_conclusion_validation_covers_digest_and_result_bound() {
    let record = conclusion();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = HarnessConclusionRecordDto {
        producing_run_id: " ".to_owned(),
        ..conclusion()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessConclusionRecordDto {
        content_digest: "sha256:short".to_owned(),
        ..conclusion()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let accepted = HarnessConclusionRecordDto {
        conclusion_bytes: HARNESS_MAX_CONCLUSION_BYTES,
        ..conclusion()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessConclusionRecordDto {
        conclusion_bytes: HARNESS_MAX_CONCLUSION_BYTES + 1,
        ..conclusion()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_result_too_large"
    );
}

#[test]
fn harness_journal_record_validation_covers_summary_digest_and_sequence() {
    let record = journal_record();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = HarnessJournalRecordDto {
        harness_id: " ".to_owned(),
        ..journal_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessJournalRecordDto {
        safe_summary: " \n ".to_owned(),
        ..journal_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let accepted = HarnessJournalRecordDto {
        safe_summary: "x".repeat(MAX_SAFE_CONTENT_BYTES),
        ..journal_record()
    };
    assert_accepted(accepted.validate());

    let invalid = HarnessJournalRecordDto {
        safe_summary: "x".repeat(MAX_SAFE_CONTENT_BYTES + 1),
        ..journal_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_result_too_large"
    );

    let invalid = HarnessJournalRecordDto {
        safe_summary: "password=hunter2".to_owned(),
        ..journal_record()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let invalid = HarnessJournalRecordDto {
        canonical_record_digest: "sha256:short".to_owned(),
        ..journal_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_source_unavailable"
    );

    let invalid = HarnessJournalRecordDto {
        sequence: 0,
        ..journal_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "harness_revision_conflict"
    );
}

#[test]
fn harness_remaining_records_preserve_typed_fields() {
    let capture = CaptureHarnessTriggerInputDto {
        harness_id: "harness-a".to_owned(),
        reason_id: "reason-a".to_owned(),
        source_kind: HarnessSourceKindDto::CalendarTime,
        rule_revision: 1,
        observed_at_ms: 10,
        applied_time_zone: "UTC".to_owned(),
        cause_chain_reference: Some("cause-a".to_owned()),
        bounded_references: vec!["reference-a".to_owned()],
        catch_up_missed_slots: 3,
    };
    let cloned = capture.clone();
    assert_eq!(cloned, capture);
    assert_eq!(cloned.source_kind, HarnessSourceKindDto::CalendarTime);
    assert_eq!(cloned.catch_up_missed_slots, 3);
    assert!(!format!("{cloned:?}").is_empty());

    let capture_record = HarnessTriggerCaptureRecordDto {
        outcome: HarnessTriggerCaptureOutcomeDto::CatchUp,
        reason: trigger_reason(),
    };
    assert_eq!(capture_record.outcome.name(), "catch_up");
    assert_accepted(capture_record.reason.validate());

    let launch = RecordHarnessLaunchInputDto {
        harness_id: "harness-a".to_owned(),
        reason_id: "reason-a".to_owned(),
        cause_chain_depth: 2,
        direct_successor: true,
        occurred_at_ms: 30,
    };
    let cloned = launch.clone();
    assert_eq!(cloned, launch);
    assert!(cloned.direct_successor);
    assert_eq!(cloned.cause_chain_depth, 2);
    assert!(!format!("{cloned:?}").is_empty());

    let counters = HarnessCounterRecordDto {
        cause_chain_depth: 2,
        concurrent_non_terminal: 1,
        total_launches: 4,
        direct_successors: 1,
        updated_at_ms: 30,
    };
    let outcome = RecordHarnessLaunchOutcomeDto {
        reason: HarnessTriggerReasonRecordDto {
            state: HarnessTriggerReasonStateDto::Admitted,
            ..trigger_reason()
        },
        counters,
    };
    assert_eq!(outcome.counters.total_launches, 4);
    assert_eq!(outcome.counters.cause_chain_depth, 2);
    assert_eq!(outcome.counters.concurrent_non_terminal, 1);
    assert_eq!(outcome.counters.direct_successors, 1);
    assert_eq!(outcome.counters.updated_at_ms, 30);
    assert_eq!(outcome.reason.state.name(), "admitted");

    let commit = CommitHarnessCheckpointInputDto {
        harness_id: "harness-a".to_owned(),
        producing_run_id: "run-a".to_owned(),
        run_outcome: HarnessRunOutcomeDto::Completed,
        candidate: Some(checkpoint()),
        occurred_at_ms: 40,
    };
    let committed = CommitHarnessCheckpointOutcomeDto {
        disposition: HarnessCheckpointDispositionDto::Replaced,
        current: commit.candidate.clone(),
    };
    assert_eq!(commit.run_outcome.name(), "completed");
    assert_eq!(committed.disposition.name(), "replaced");
    assert_eq!(committed.current, commit.candidate);
    assert!(!format!("{committed:?}").is_empty());

    let append = AppendHarnessJournalRecordInputDto {
        harness_id: "harness-a".to_owned(),
        record_kind: HarnessJournalRecordKindDto::ConclusionPublished,
        safe_summary: "conclusion published".to_owned(),
        canonical_record_digest: digest('f'),
        occurred_at_ms: 60,
    };
    assert_eq!(
        append.record_kind.name(),
        HarnessJournalRecordKindDto::ConclusionPublished.name()
    );
    assert_eq!(append.occurred_at_ms, 60);

    let page = LoadHarnessJournalInputDto {
        after_sequence: 0,
        limit: MAX_HARNESS_JOURNAL_PAGE + 1,
    };
    assert_eq!(page.after_sequence, 0);
    assert_eq!(page.limit, MAX_HARNESS_JOURNAL_PAGE + 1);
    assert_eq!(MAX_HARNESS_JOURNAL_PAGE, 64);

    let transition = TransitionHarnessRuleLifecycleInputDto {
        harness_id: "harness-a".to_owned(),
        expected_revision: 1,
        operation: HarnessRuleOperationDto::Pause,
        has_active_run: true,
        occurred_at_ms: 70,
    };
    assert_eq!(transition.operation.name(), "pause");
    assert!(transition.has_active_run);
    assert_eq!(transition.expected_revision, 1);
    assert!(!format!("{transition:?}").is_empty());
}

// ---------------------------------------------------------------------------
// Programmatic policy fixtures.
// ---------------------------------------------------------------------------

const fn policy_origin_rule(
    root_origin_kind: ProgrammaticRootOriginKindDto,
) -> ProgrammaticRootOriginRuleRecordDto {
    ProgrammaticRootOriginRuleRecordDto {
        root_origin_kind,
        maximum_decision: ProgrammaticAdmissionDecisionDto::DirectLocalRead,
    }
}

fn policy_revision() -> ProgrammaticPolicyRevisionRecordDto {
    ProgrammaticPolicyRevisionRecordDto {
        policy_id: "policy-a".to_owned(),
        revision: 1,
        root_origin_rules: vec![policy_origin_rule(
            ProgrammaticRootOriginKindDto::InteractiveUser,
        )],
        admission_decisions: vec![ProgrammaticAdmissionDecisionDto::DirectLocalRead],
        max_actions_per_run: 1,
        max_concurrent_actions_per_run: 1,
        calendar_period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        calendar_max_actions: 1,
        inherited_policy_references: Vec::new(),
        canonical_revision_digest: digest('a'),
    }
}

fn policy_reference() -> ProgrammaticPolicyRevisionReferenceDto {
    ProgrammaticPolicyRevisionReferenceDto {
        policy_id: "policy-b".to_owned(),
        revision: 1,
    }
}

fn policy_record() -> ProgrammaticPolicyRecordDto {
    ProgrammaticPolicyRecordDto {
        policy_id: "policy-a".to_owned(),
        scope: ProgrammaticPolicyScopeDto::Project {
            project_id: "project-a".to_owned(),
        },
        calendar_period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        lifecycle_state: ProgrammaticPolicyLifecycleStateDto::Active,
        active_revision: 1,
        canonical_policy_digest: digest('b'),
        updated_at_ms: 10,
    }
}

fn snapshot() -> ProgrammaticPolicySnapshotRecordDto {
    ProgrammaticPolicySnapshotRecordDto {
        snapshot_id: "snapshot-a".to_owned(),
        root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
        policy_references: vec![policy_reference()],
        decision_ceiling: ProgrammaticAdmissionDecisionDto::DirectLocalRead,
        max_actions_per_run: 1,
        max_concurrent_actions_per_run: 1,
        calendar_limits: vec![ProgrammaticCalendarLimitRecordDto {
            period_kind: ProgrammaticCalendarPeriodKindDto::Day,
            max_actions: 1,
        }],
        calendar_counter_references: vec![ProgrammaticCalendarCounterReferenceDto {
            policy_id: "policy-a".to_owned(),
            period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        }],
        baseline_max_actions: None,
        baseline_max_concurrent_actions: None,
        snapshot_digest: digest('c'),
        created_at_ms: 20,
    }
}

fn confirmation() -> ProgrammaticPolicyConfirmationRecordDto {
    ProgrammaticPolicyConfirmationRecordDto {
        confirmation_id: "confirmation-a".to_owned(),
        root_session_id: "session-a".to_owned(),
        root_run_id: "run-a".to_owned(),
        tool_call_id: "tool-call-a".to_owned(),
        tool_id: "tool-a".to_owned(),
        descriptor_revision: "descriptor-revision-a".to_owned(),
        mcp_method_reference: None,
        typed_input_digest: digest('d'),
        policy_snapshot_digest: digest('e'),
        state: ProgrammaticConfirmationStateDto::Awaiting,
        created_at_ms: 10,
        decided_at_ms: None,
    }
}

fn corridor() -> ProgrammaticAuthorizationCorridorRecordDto {
    ProgrammaticAuthorizationCorridorRecordDto {
        corridor_digest: digest('f'),
        root_session_id: "session-a".to_owned(),
        root_run_id: "run-a".to_owned(),
        root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
        policy_snapshot_reference: "snapshot-a".to_owned(),
        required_effect_selectors: vec!["effect-a".to_owned()],
        exact_tool_or_method_selectors: vec!["tool-a".to_owned()],
        descriptor_input_constraint_selections: vec!["constraint-a".to_owned()],
        maximum_action_count: 4,
        maximum_concurrent_actions: 2,
        confirmation_reference: "confirmation-a".to_owned(),
        state: ProgrammaticCorridorStateDto::Active,
        consumed_action_count: 0,
        created_at_ms: 10,
    }
}

fn draft() -> ProgrammaticPolicyDraftRecordDto {
    ProgrammaticPolicyDraftRecordDto {
        draft_id: "draft-a".to_owned(),
        scope: ProgrammaticPolicyScopeDto::Project {
            project_id: "project-a".to_owned(),
        },
        record_kind: "policy".to_owned(),
        base_revision: 1,
        evidence_references: vec!["evidence-a".to_owned()],
        safe_rationale: "safe draft rationale".to_owned(),
        canonical_draft_digest: digest('f'),
        state: ProgrammaticPolicyDraftStateDto::Pending,
        coalesced_evidence_count: 1,
        created_at_ms: 10,
        decided_at_ms: None,
    }
}

fn counter() -> ProgrammaticPolicyCounterRecordDto {
    ProgrammaticPolicyCounterRecordDto {
        policy_id: "policy-a".to_owned(),
        run_started_actions: 1,
        run_reserved_actions: 0,
        run_in_flight_actions: 1,
        calendar_started_actions: 1,
        calendar_reserved_actions: 0,
        calendar_window_start_ms: 0,
        calendar_window_end_ms: 86_400_000,
        calendar_window_time_zone: "UTC".to_owned(),
        updated_at_ms: 10,
    }
}

fn reservation() -> ProgrammaticPolicyReservationRecordDto {
    ProgrammaticPolicyReservationRecordDto {
        reservation_reference: "reservation-a".to_owned(),
        policy_id: "policy-a".to_owned(),
        policy_revision: 1,
        root_run_id: "run-a".to_owned(),
        tool_call_id: "tool-call-a".to_owned(),
        typed_input_digest: digest('e'),
        calendar_counter_reference: "calendar-counter-a".to_owned(),
        reserved_at_ms: 10,
        state: ProgrammaticReservationStateDto::Reserved,
        finished_at_ms: None,
    }
}

// ---------------------------------------------------------------------------
// Programmatic policy discriminators, accessors, and validators.
// ---------------------------------------------------------------------------

#[test]
fn programmatic_policy_scope_accessors_cover_project_goal_and_session_owners() {
    let project = ProgrammaticPolicyScopeDto::Project {
        project_id: "project-a".to_owned(),
    };
    assert_eq!(project.project_id(), "project-a");
    assert_eq!(project.goal_id(), None);
    assert_eq!(project.owner_session_id(), None);
    assert_eq!(project.kind_name(), "project");
    assert_accepted(project.validate());

    let goal = ProgrammaticPolicyScopeDto::Goal {
        project_id: "project-a".to_owned(),
        goal_id: "goal-a".to_owned(),
    };
    assert_eq!(goal.project_id(), "project-a");
    assert_eq!(goal.goal_id(), Some("goal-a"));
    assert_eq!(goal.owner_session_id(), None);
    assert_eq!(goal.kind_name(), "goal");
    assert_accepted(goal.validate());

    let session = ProgrammaticPolicyScopeDto::Session {
        project_id: "project-a".to_owned(),
        owner_session_id: "session-a".to_owned(),
    };
    assert_eq!(session.project_id(), "project-a");
    assert_eq!(session.goal_id(), None);
    assert_eq!(session.owner_session_id(), Some("session-a"));
    assert_eq!(session.kind_name(), "session");
    assert_accepted(session.validate());

    let cloned = session.clone();
    assert_eq!(cloned, session);
    assert!(!format!("{cloned:?}").is_empty());

    assert_eq!(
        rejection_code(
            ProgrammaticPolicyScopeDto::Project {
                project_id: "  ".to_owned(),
            }
            .validate()
        ),
        "programmatic_policy_not_applicable"
    );
    assert_eq!(
        rejection_code(
            ProgrammaticPolicyScopeDto::Goal {
                project_id: "project-a".to_owned(),
                goal_id: "line\nbreak".to_owned(),
            }
            .validate()
        ),
        "programmatic_policy_not_applicable"
    );
    assert_eq!(
        rejection_code(
            ProgrammaticPolicyScopeDto::Session {
                project_id: "project-a".to_owned(),
                owner_session_id: format!("session-{}", "x".repeat(MAX_SAFE_LABEL_CHARS)),
            }
            .validate()
        ),
        "programmatic_policy_not_applicable"
    );
    assert_eq!(
        rejection_code(
            ProgrammaticPolicyScopeDto::Goal {
                project_id: "project-a".to_owned(),
                goal_id: "token=live".to_owned(),
            }
            .validate()
        ),
        "credentials_forbidden"
    );
}

#[test]
fn programmatic_root_origin_kinds_parse_each_closed_discriminator() {
    for kind in [
        ProgrammaticRootOriginKindDto::InteractiveUser,
        ProgrammaticRootOriginKindDto::ContinualHarness,
    ] {
        assert_eq!(
            ProgrammaticRootOriginKindDto::parse(kind.name()).ok(),
            Some(kind)
        );
    }
    assert_eq!(
        ProgrammaticRootOriginKindDto::InteractiveUser.name(),
        "interactive_user"
    );
    assert_eq!(
        ProgrammaticRootOriginKindDto::ContinualHarness.name(),
        "continual_harness"
    );
    assert_eq!(
        rejection_code(ProgrammaticRootOriginKindDto::parse("external_system")),
        "programmatic_policy_origin_invalid"
    );
}

#[test]
fn programmatic_admission_decisions_and_calendar_periods_parse() {
    for decision in [
        ProgrammaticAdmissionDecisionDto::Prohibited,
        ProgrammaticAdmissionDecisionDto::DirectLocalRead,
        ProgrammaticAdmissionDecisionDto::ExactConfirmationRequired,
        ProgrammaticAdmissionDecisionDto::BoundedConfirmationRequired,
    ] {
        assert_eq!(
            ProgrammaticAdmissionDecisionDto::parse(decision.name()).ok(),
            Some(decision)
        );
    }
    assert_eq!(
        ProgrammaticAdmissionDecisionDto::Prohibited.name(),
        "prohibited"
    );
    assert_eq!(
        ProgrammaticAdmissionDecisionDto::DirectLocalRead.name(),
        "direct_local_read"
    );
    assert_eq!(
        ProgrammaticAdmissionDecisionDto::ExactConfirmationRequired.name(),
        "exact_confirmation_required"
    );
    assert_eq!(
        ProgrammaticAdmissionDecisionDto::BoundedConfirmationRequired.name(),
        "bounded_confirmation_required"
    );
    assert_eq!(
        rejection_code(ProgrammaticAdmissionDecisionDto::parse("allow")),
        "programmatic_policy_input_constraint_mismatch"
    );

    for period in [
        ProgrammaticCalendarPeriodKindDto::Day,
        ProgrammaticCalendarPeriodKindDto::Week,
        ProgrammaticCalendarPeriodKindDto::Month,
    ] {
        assert_eq!(
            ProgrammaticCalendarPeriodKindDto::parse(period.name()).ok(),
            Some(period)
        );
    }
    assert_eq!(ProgrammaticCalendarPeriodKindDto::Day.name(), "day");
    assert_eq!(ProgrammaticCalendarPeriodKindDto::Week.name(), "week");
    assert_eq!(ProgrammaticCalendarPeriodKindDto::Month.name(), "month");
    assert_eq!(
        rejection_code(ProgrammaticCalendarPeriodKindDto::parse("year")),
        "programmatic_policy_revision_conflict"
    );
}

#[test]
fn programmatic_lifecycle_states_and_operations_expose_closed_names() {
    for state in [
        ProgrammaticPolicyLifecycleStateDto::Active,
        ProgrammaticPolicyLifecycleStateDto::Suspended,
        ProgrammaticPolicyLifecycleStateDto::Revoked,
        ProgrammaticPolicyLifecycleStateDto::Archived,
    ] {
        assert_eq!(
            ProgrammaticPolicyLifecycleStateDto::parse(state.name()).ok(),
            Some(state)
        );
    }
    assert_eq!(ProgrammaticPolicyLifecycleStateDto::Active.name(), "active");
    assert_eq!(
        ProgrammaticPolicyLifecycleStateDto::Suspended.name(),
        "suspended"
    );
    assert_eq!(
        ProgrammaticPolicyLifecycleStateDto::Revoked.name(),
        "revoked"
    );
    assert_eq!(
        ProgrammaticPolicyLifecycleStateDto::Archived.name(),
        "archived"
    );
    assert_eq!(
        rejection_code(ProgrammaticPolicyLifecycleStateDto::parse("draft")),
        "programmatic_policy_not_applicable"
    );

    assert_eq!(
        ProgrammaticPolicyLifecycleOperationDto::Suspend.name(),
        "suspend"
    );
    assert_eq!(
        ProgrammaticPolicyLifecycleOperationDto::Resume.name(),
        "resume"
    );
    assert_eq!(
        ProgrammaticPolicyLifecycleOperationDto::Revoke.name(),
        "revoke"
    );
    assert_eq!(
        ProgrammaticPolicyLifecycleOperationDto::Archive.name(),
        "archive"
    );
}

#[test]
fn programmatic_confirmation_corridor_reservation_and_draft_states_parse() {
    for state in [
        ProgrammaticConfirmationStateDto::Awaiting,
        ProgrammaticConfirmationStateDto::Accepted,
        ProgrammaticConfirmationStateDto::Rejected,
        ProgrammaticConfirmationStateDto::Expired,
        ProgrammaticConfirmationStateDto::Cancelled,
    ] {
        assert_eq!(
            ProgrammaticConfirmationStateDto::parse(state.name()).ok(),
            Some(state)
        );
    }
    assert_eq!(
        ProgrammaticConfirmationStateDto::Awaiting.name(),
        "awaiting"
    );
    assert_eq!(
        ProgrammaticConfirmationStateDto::Accepted.name(),
        "accepted"
    );
    assert_eq!(
        ProgrammaticConfirmationStateDto::Rejected.name(),
        "rejected"
    );
    assert_eq!(ProgrammaticConfirmationStateDto::Expired.name(), "expired");
    assert_eq!(
        ProgrammaticConfirmationStateDto::Cancelled.name(),
        "cancelled"
    );
    assert_eq!(
        rejection_code(ProgrammaticConfirmationStateDto::parse("decided")),
        "programmatic_policy_confirmation_required"
    );

    for state in [
        ProgrammaticCorridorStateDto::Active,
        ProgrammaticCorridorStateDto::Expired,
        ProgrammaticCorridorStateDto::Exhausted,
        ProgrammaticCorridorStateDto::Revoked,
    ] {
        assert_eq!(
            ProgrammaticCorridorStateDto::parse(state.name()).ok(),
            Some(state)
        );
    }
    assert_eq!(ProgrammaticCorridorStateDto::Active.name(), "active");
    assert_eq!(ProgrammaticCorridorStateDto::Expired.name(), "expired");
    assert_eq!(ProgrammaticCorridorStateDto::Exhausted.name(), "exhausted");
    assert_eq!(ProgrammaticCorridorStateDto::Revoked.name(), "revoked");
    assert_eq!(
        rejection_code(ProgrammaticCorridorStateDto::parse("paused")),
        "programmatic_policy_corridor_unavailable"
    );

    for state in [
        ProgrammaticReservationStateDto::Reserved,
        ProgrammaticReservationStateDto::PermanentOnStart,
        ProgrammaticReservationStateDto::ReleasedOnKnownPreEffect,
        ProgrammaticReservationStateDto::InterruptedBeforeStart,
        ProgrammaticReservationStateDto::ExternalEffectUnknown,
    ] {
        assert_eq!(
            ProgrammaticReservationStateDto::parse(state.name()).ok(),
            Some(state)
        );
    }
    assert_eq!(ProgrammaticReservationStateDto::Reserved.name(), "reserved");
    assert_eq!(
        ProgrammaticReservationStateDto::PermanentOnStart.name(),
        "permanent_on_start"
    );
    assert_eq!(
        ProgrammaticReservationStateDto::ReleasedOnKnownPreEffect.name(),
        "released_on_known_pre_effect"
    );
    assert_eq!(
        ProgrammaticReservationStateDto::InterruptedBeforeStart.name(),
        "interrupted_before_start"
    );
    assert_eq!(
        ProgrammaticReservationStateDto::ExternalEffectUnknown.name(),
        "external_effect_unknown"
    );
    assert_eq!(
        rejection_code(ProgrammaticReservationStateDto::parse("unknown")),
        "programmatic_policy_reservation_conflict"
    );
    assert!(ProgrammaticReservationStateDto::Reserved.consumes_outstanding_reservation());
    assert!(!ProgrammaticReservationStateDto::PermanentOnStart.consumes_outstanding_reservation());
    assert!(
        !ProgrammaticReservationStateDto::ReleasedOnKnownPreEffect
            .consumes_outstanding_reservation()
    );
    assert!(
        !ProgrammaticReservationStateDto::InterruptedBeforeStart.consumes_outstanding_reservation()
    );
    assert!(
        !ProgrammaticReservationStateDto::ExternalEffectUnknown.consumes_outstanding_reservation()
    );

    for state in [
        ProgrammaticPolicyDraftStateDto::Pending,
        ProgrammaticPolicyDraftStateDto::Accepted,
        ProgrammaticPolicyDraftStateDto::Rejected,
    ] {
        assert_eq!(
            ProgrammaticPolicyDraftStateDto::parse(state.name()).ok(),
            Some(state)
        );
    }
    assert_eq!(ProgrammaticPolicyDraftStateDto::Pending.name(), "pending");
    assert_eq!(ProgrammaticPolicyDraftStateDto::Accepted.name(), "accepted");
    assert_eq!(ProgrammaticPolicyDraftStateDto::Rejected.name(), "rejected");
    assert_eq!(
        rejection_code(ProgrammaticPolicyDraftStateDto::parse("expired")),
        "programmatic_policy_draft_conflict"
    );

    assert_eq!(
        ProgrammaticDraftCoalescingDto::CreatedNew.name(),
        "created_new"
    );
    assert_eq!(
        ProgrammaticDraftCoalescingDto::CoalescedIntoPending.name(),
        "coalesced_into_pending"
    );
}

#[test]
fn programmatic_policy_revision_reference_validation_covers_identity_and_revision() {
    let reference = policy_reference();
    assert_accepted(reference.validate());
    let cloned = reference.clone();
    assert_eq!(cloned, reference);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticPolicyRevisionReferenceDto {
        policy_id: " ".to_owned(),
        revision: 1,
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = ProgrammaticPolicyRevisionReferenceDto {
        policy_id: "key=live".to_owned(),
        revision: 1,
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let invalid = ProgrammaticPolicyRevisionReferenceDto {
        policy_id: "policy-b".to_owned(),
        revision: 0,
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );
}

#[test]
fn programmatic_policy_revision_validation_covers_origins_limits_and_inheritance() {
    let revision = policy_revision();
    assert_accepted(revision.validate());
    let cloned = revision.clone();
    assert_eq!(cloned, revision);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        policy_id: " ".to_owned(),
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        canonical_revision_digest: "sha256:short".to_owned(),
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        revision: 0,
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        root_origin_rules: Vec::new(),
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_origin_invalid"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        root_origin_rules: vec![
            policy_origin_rule(ProgrammaticRootOriginKindDto::InteractiveUser);
            3
        ],
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_origin_invalid"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        root_origin_rules: vec![
            policy_origin_rule(ProgrammaticRootOriginKindDto::InteractiveUser),
            policy_origin_rule(ProgrammaticRootOriginKindDto::InteractiveUser),
        ],
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_origin_invalid"
    );

    let accepted = ProgrammaticPolicyRevisionRecordDto {
        root_origin_rules: vec![
            policy_origin_rule(ProgrammaticRootOriginKindDto::InteractiveUser),
            policy_origin_rule(ProgrammaticRootOriginKindDto::ContinualHarness),
        ],
        ..policy_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        admission_decisions: vec![
            ProgrammaticAdmissionDecisionDto::DirectLocalRead;
            MAX_RULES_PER_POLICY_REVISION + 1
        ],
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let accepted = ProgrammaticPolicyRevisionRecordDto {
        admission_decisions: vec![
            ProgrammaticAdmissionDecisionDto::DirectLocalRead;
            MAX_RULES_PER_POLICY_REVISION
        ],
        ..policy_revision()
    };
    assert_accepted(accepted.validate());

    let excessive_references: Vec<ProgrammaticPolicyRevisionReferenceDto> =
        labels("policy", MAX_POLICY_INHERITED_REFERENCES + 1)
            .into_iter()
            .map(|policy_id| ProgrammaticPolicyRevisionReferenceDto {
                policy_id,
                revision: 1,
            })
            .collect();
    let invalid = ProgrammaticPolicyRevisionRecordDto {
        inherited_policy_references: excessive_references,
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let bounded_references: Vec<ProgrammaticPolicyRevisionReferenceDto> =
        labels("policy", MAX_POLICY_INHERITED_REFERENCES)
            .into_iter()
            .map(|policy_id| ProgrammaticPolicyRevisionReferenceDto {
                policy_id,
                revision: 1,
            })
            .collect();
    let accepted = ProgrammaticPolicyRevisionRecordDto {
        inherited_policy_references: bounded_references,
        ..policy_revision()
    };
    assert_accepted(accepted.validate());
    assert_eq!(MAX_POLICY_INHERITED_REFERENCES, 32);

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        inherited_policy_references: vec![ProgrammaticPolicyRevisionReferenceDto {
            policy_id: "policy-b".to_owned(),
            revision: 0,
        }],
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        max_actions_per_run: 0,
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        max_actions_per_run: MAX_POLICY_ACTIONS_PER_RUN + 1,
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let accepted = ProgrammaticPolicyRevisionRecordDto {
        max_actions_per_run: MAX_POLICY_ACTIONS_PER_RUN,
        ..policy_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        max_concurrent_actions_per_run: 0,
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        max_concurrent_actions_per_run: MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN + 1,
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let accepted = ProgrammaticPolicyRevisionRecordDto {
        max_concurrent_actions_per_run: MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN,
        ..policy_revision()
    };
    assert_accepted(accepted.validate());

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        calendar_max_actions: MIN_CALENDAR_ACTION_LIMIT - 1,
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticPolicyRevisionRecordDto {
        calendar_max_actions: MAX_CALENDAR_ACTION_LIMIT + 1,
        ..policy_revision()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let accepted = ProgrammaticPolicyRevisionRecordDto {
        calendar_max_actions: MAX_CALENDAR_ACTION_LIMIT,
        ..policy_revision()
    };
    assert_accepted(accepted.validate());

    let accepted = ProgrammaticPolicyRevisionRecordDto {
        calendar_max_actions: MIN_CALENDAR_ACTION_LIMIT,
        ..policy_revision()
    };
    assert_accepted(accepted.validate());
}

#[test]
fn programmatic_policy_record_validation_covers_identity_digest_scope_and_revision() {
    let record = policy_record();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticPolicyRecordDto {
        policy_id: " ".to_owned(),
        ..policy_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = ProgrammaticPolicyRecordDto {
        canonical_policy_digest: "sha256:short".to_owned(),
        ..policy_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = ProgrammaticPolicyRecordDto {
        scope: ProgrammaticPolicyScopeDto::Project {
            project_id: " ".to_owned(),
        },
        ..policy_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_not_applicable"
    );

    let invalid = ProgrammaticPolicyRecordDto {
        active_revision: 0,
        ..policy_record()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );
}

#[test]
fn programmatic_create_policy_input_requires_coherent_first_revision() {
    let input = CreateProgrammaticPolicyInputDto {
        policy: policy_record(),
        revision: policy_revision(),
    };
    assert_accepted(input.validate());
    let cloned = input.clone();
    assert_eq!(cloned, input);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = CreateProgrammaticPolicyInputDto {
        policy: ProgrammaticPolicyRecordDto {
            canonical_policy_digest: "sha256:short".to_owned(),
            ..policy_record()
        },
        revision: policy_revision(),
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = CreateProgrammaticPolicyInputDto {
        policy: policy_record(),
        revision: ProgrammaticPolicyRevisionRecordDto {
            canonical_revision_digest: "sha256:short".to_owned(),
            ..policy_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = CreateProgrammaticPolicyInputDto {
        policy: policy_record(),
        revision: ProgrammaticPolicyRevisionRecordDto {
            policy_id: "policy-b".to_owned(),
            ..policy_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = CreateProgrammaticPolicyInputDto {
        policy: policy_record(),
        revision: ProgrammaticPolicyRevisionRecordDto {
            revision: 2,
            ..policy_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = CreateProgrammaticPolicyInputDto {
        policy: ProgrammaticPolicyRecordDto {
            active_revision: 2,
            ..policy_record()
        },
        revision: policy_revision(),
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = CreateProgrammaticPolicyInputDto {
        policy: policy_record(),
        revision: ProgrammaticPolicyRevisionRecordDto {
            calendar_period_kind: ProgrammaticCalendarPeriodKindDto::Week,
            ..policy_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );
}

#[test]
fn programmatic_append_policy_revision_requires_the_next_revision_of_the_same_policy() {
    let input = AppendProgrammaticPolicyRevisionInputDto {
        policy_id: "policy-a".to_owned(),
        expected_revision: 1,
        revision: ProgrammaticPolicyRevisionRecordDto {
            revision: 2,
            ..policy_revision()
        },
    };
    assert_accepted(input.validate());

    let invalid = AppendProgrammaticPolicyRevisionInputDto {
        policy_id: "policy-a".to_owned(),
        expected_revision: 1,
        revision: ProgrammaticPolicyRevisionRecordDto {
            canonical_revision_digest: "sha256:short".to_owned(),
            revision: 2,
            ..policy_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = AppendProgrammaticPolicyRevisionInputDto {
        policy_id: "policy-a".to_owned(),
        expected_revision: u64::MAX,
        revision: policy_revision(),
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = AppendProgrammaticPolicyRevisionInputDto {
        policy_id: "policy-b".to_owned(),
        expected_revision: 1,
        revision: ProgrammaticPolicyRevisionRecordDto {
            revision: 2,
            ..policy_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = AppendProgrammaticPolicyRevisionInputDto {
        policy_id: "policy-a".to_owned(),
        expected_revision: 1,
        revision: ProgrammaticPolicyRevisionRecordDto {
            revision: 3,
            ..policy_revision()
        },
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );
}

#[test]
fn programmatic_policy_snapshot_validation_covers_references_calendars_and_bounds() {
    let record = snapshot();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        snapshot_id: " ".to_owned(),
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_snapshot_unavailable"
    );

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        snapshot_digest: "sha256:short".to_owned(),
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_snapshot_unavailable"
    );

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        policy_references: Vec::new(),
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_snapshot_unavailable"
    );

    let references: Vec<ProgrammaticPolicyRevisionReferenceDto> =
        labels("policy", MAX_POLICY_SNAPSHOT_REFERENCES + 1)
            .into_iter()
            .map(|policy_id| ProgrammaticPolicyRevisionReferenceDto {
                policy_id,
                revision: 1,
            })
            .collect();
    let invalid = ProgrammaticPolicySnapshotRecordDto {
        policy_references: references,
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_snapshot_too_large"
    );

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        calendar_limits: vec![
            ProgrammaticCalendarLimitRecordDto {
                period_kind: ProgrammaticCalendarPeriodKindDto::Day,
                max_actions: 1,
            };
            MAX_POLICY_SNAPSHOT_REFERENCES + 1
        ],
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_snapshot_too_large"
    );

    let counters: Vec<ProgrammaticCalendarCounterReferenceDto> =
        labels("policy", MAX_POLICY_SNAPSHOT_REFERENCES + 1)
            .into_iter()
            .map(|policy_id| ProgrammaticCalendarCounterReferenceDto {
                policy_id,
                period_kind: ProgrammaticCalendarPeriodKindDto::Day,
            })
            .collect();
    let invalid = ProgrammaticPolicySnapshotRecordDto {
        calendar_counter_references: counters,
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_snapshot_too_large"
    );

    let references: Vec<ProgrammaticPolicyRevisionReferenceDto> =
        labels("policy", MAX_POLICY_SNAPSHOT_REFERENCES)
            .into_iter()
            .map(|policy_id| ProgrammaticPolicyRevisionReferenceDto {
                policy_id,
                revision: 1,
            })
            .collect();
    let counters: Vec<ProgrammaticCalendarCounterReferenceDto> =
        labels("policy", MAX_POLICY_SNAPSHOT_REFERENCES)
            .into_iter()
            .map(|policy_id| ProgrammaticCalendarCounterReferenceDto {
                policy_id,
                period_kind: ProgrammaticCalendarPeriodKindDto::Week,
            })
            .collect();
    let accepted = ProgrammaticPolicySnapshotRecordDto {
        policy_references: references,
        calendar_limits: vec![
            ProgrammaticCalendarLimitRecordDto {
                period_kind: ProgrammaticCalendarPeriodKindDto::Week,
                max_actions: 2,
            };
            MAX_POLICY_SNAPSHOT_REFERENCES
        ],
        calendar_counter_references: counters,
        baseline_max_actions: Some(4),
        baseline_max_concurrent_actions: Some(2),
        ..snapshot()
    };
    assert_accepted(accepted.validate());
    assert_eq!(MAX_POLICY_SNAPSHOT_REFERENCES, 64);

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        policy_references: vec![ProgrammaticPolicyRevisionReferenceDto {
            policy_id: "policy-b".to_owned(),
            revision: 0,
        }],
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_revision_conflict"
    );

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        calendar_counter_references: vec![ProgrammaticCalendarCounterReferenceDto {
            policy_id: " ".to_owned(),
            period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        }],
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_counter_unavailable"
    );

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        calendar_counter_references: vec![ProgrammaticCalendarCounterReferenceDto {
            policy_id: "api_key=live".to_owned(),
            period_kind: ProgrammaticCalendarPeriodKindDto::Day,
        }],
        ..snapshot()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        max_actions_per_run: 0,
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticPolicySnapshotRecordDto {
        max_concurrent_actions_per_run: 0,
        ..snapshot()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );
}

#[test]
fn programmatic_confirmation_validation_covers_bindings_digests_and_decision_time() {
    let record = confirmation();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        confirmation_id: " ".to_owned(),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        root_session_id: " ".to_owned(),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        root_run_id: " ".to_owned(),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        tool_call_id: " ".to_owned(),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        tool_id: " ".to_owned(),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        descriptor_revision: " ".to_owned(),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        typed_input_digest: "sha256:short".to_owned(),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        policy_snapshot_digest: "sha256:short".to_owned(),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        mcp_method_reference: Some(" ".to_owned()),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_required"
    );

    let invalid = ProgrammaticPolicyConfirmationRecordDto {
        state: ProgrammaticConfirmationStateDto::Awaiting,
        decided_at_ms: Some(30),
        ..confirmation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_confirmation_expired"
    );

    let accepted = ProgrammaticPolicyConfirmationRecordDto {
        state: ProgrammaticConfirmationStateDto::Accepted,
        decided_at_ms: Some(30),
        ..confirmation()
    };
    assert_accepted(accepted.validate());

    let accepted = ProgrammaticPolicyConfirmationRecordDto {
        mcp_method_reference: Some("tools/call".to_owned()),
        ..confirmation()
    };
    assert_accepted(accepted.validate());
}

#[test]
fn programmatic_corridor_validation_covers_selectors_bounds_and_consumption() {
    let record = corridor();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        corridor_digest: "sha256:short".to_owned(),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_corridor_unavailable"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        root_session_id: " ".to_owned(),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_corridor_unavailable"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        root_run_id: " ".to_owned(),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_corridor_unavailable"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        policy_snapshot_reference: " ".to_owned(),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_corridor_unavailable"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        confirmation_reference: " ".to_owned(),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_corridor_unavailable"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        required_effect_selectors: Vec::new(),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        exact_tool_or_method_selectors: Vec::new(),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        descriptor_input_constraint_selections: Vec::new(),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        required_effect_selectors: labels("effect", MAX_POLICY_SELECTORS + 1),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        exact_tool_or_method_selectors: labels("tool", MAX_POLICY_SELECTORS + 1),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        descriptor_input_constraint_selections: labels("constraint", MAX_POLICY_SELECTORS + 1),
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let accepted = ProgrammaticAuthorizationCorridorRecordDto {
        required_effect_selectors: labels("effect", MAX_POLICY_SELECTORS),
        exact_tool_or_method_selectors: labels("tool", MAX_POLICY_SELECTORS),
        descriptor_input_constraint_selections: labels("constraint", MAX_POLICY_SELECTORS),
        ..corridor()
    };
    assert_accepted(accepted.validate());
    assert_eq!(MAX_POLICY_SELECTORS, 16);

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        required_effect_selectors: vec![" ".to_owned()],
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_corridor_unavailable"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        exact_tool_or_method_selectors: vec!["token=live".to_owned()],
        ..corridor()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        maximum_action_count: 0,
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        maximum_concurrent_actions: 0,
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let invalid = ProgrammaticAuthorizationCorridorRecordDto {
        maximum_concurrent_actions: 5,
        maximum_action_count: 4,
        ..corridor()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_limit_exceeded"
    );

    let exhausted = ProgrammaticAuthorizationCorridorRecordDto {
        consumed_action_count: 5,
        ..corridor()
    };
    assert_eq!(
        rejection_code(exhausted.validate()),
        "programmatic_policy_corridor_exhausted"
    );

    let consumed = ProgrammaticAuthorizationCorridorRecordDto {
        consumed_action_count: 4,
        ..corridor()
    };
    assert_accepted(consumed.validate());
}

#[test]
fn programmatic_draft_validation_covers_scope_evidence_and_rationale() {
    let record = draft();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticPolicyDraftRecordDto {
        draft_id: " ".to_owned(),
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_conflict"
    );

    let invalid = ProgrammaticPolicyDraftRecordDto {
        record_kind: " ".to_owned(),
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_conflict"
    );

    let invalid = ProgrammaticPolicyDraftRecordDto {
        canonical_draft_digest: "sha256:short".to_owned(),
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_conflict"
    );

    let invalid = ProgrammaticPolicyDraftRecordDto {
        scope: ProgrammaticPolicyScopeDto::Project {
            project_id: " ".to_owned(),
        },
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_not_applicable"
    );

    let invalid = ProgrammaticPolicyDraftRecordDto {
        base_revision: 0,
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_conflict"
    );

    let invalid = ProgrammaticPolicyDraftRecordDto {
        evidence_references: Vec::new(),
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_conflict"
    );

    let invalid = ProgrammaticPolicyDraftRecordDto {
        evidence_references: labels("evidence", MAX_POLICY_SELECTORS + 1),
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_too_large"
    );

    let accepted = ProgrammaticPolicyDraftRecordDto {
        evidence_references: labels("evidence", MAX_POLICY_SELECTORS),
        ..draft()
    };
    assert_accepted(accepted.validate());

    let invalid = ProgrammaticPolicyDraftRecordDto {
        evidence_references: vec![" ".to_owned()],
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_conflict"
    );

    let invalid = ProgrammaticPolicyDraftRecordDto {
        evidence_references: vec!["api_key=live".to_owned()],
        ..draft()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let invalid = ProgrammaticPolicyDraftRecordDto {
        safe_rationale: "  ".to_owned(),
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_conflict"
    );

    let accepted = ProgrammaticPolicyDraftRecordDto {
        safe_rationale: "x".repeat(MAX_SAFE_CONTENT_BYTES),
        ..draft()
    };
    assert_accepted(accepted.validate());

    let invalid = ProgrammaticPolicyDraftRecordDto {
        safe_rationale: "x".repeat(MAX_SAFE_CONTENT_BYTES + 1),
        ..draft()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_draft_too_large"
    );

    let invalid = ProgrammaticPolicyDraftRecordDto {
        safe_rationale: "secret=value".to_owned(),
        ..draft()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");
}

#[test]
fn programmatic_counter_validation_covers_window_coherence() {
    let record = counter();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticPolicyCounterRecordDto {
        policy_id: " ".to_owned(),
        ..counter()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_counter_unavailable"
    );

    let invalid = ProgrammaticPolicyCounterRecordDto {
        calendar_window_time_zone: " ".to_owned(),
        ..counter()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_counter_unavailable"
    );

    let invalid = ProgrammaticPolicyCounterRecordDto {
        calendar_window_start_ms: 10,
        calendar_window_end_ms: 10,
        ..counter()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_counter_unavailable"
    );

    let invalid = ProgrammaticPolicyCounterRecordDto {
        calendar_window_start_ms: 20,
        calendar_window_end_ms: 10,
        ..counter()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_counter_unavailable"
    );

    let accepted = ProgrammaticPolicyCounterRecordDto {
        calendar_window_start_ms: 10,
        calendar_window_end_ms: 20,
        ..counter()
    };
    assert_accepted(accepted.validate());
}

#[test]
fn programmatic_reservation_validation_covers_bindings_and_revision() {
    let record = reservation();
    assert_accepted(record.validate());
    let cloned = record.clone();
    assert_eq!(cloned, record);
    assert!(!format!("{cloned:?}").is_empty());

    let invalid = ProgrammaticPolicyReservationRecordDto {
        reservation_reference: " ".to_owned(),
        ..reservation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_origin_invalid"
    );

    let invalid = ProgrammaticPolicyReservationRecordDto {
        policy_id: " ".to_owned(),
        ..reservation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_origin_invalid"
    );

    let invalid = ProgrammaticPolicyReservationRecordDto {
        root_run_id: " ".to_owned(),
        ..reservation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_origin_invalid"
    );

    let invalid = ProgrammaticPolicyReservationRecordDto {
        tool_call_id: " ".to_owned(),
        ..reservation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_origin_invalid"
    );

    let invalid = ProgrammaticPolicyReservationRecordDto {
        calendar_counter_reference: " ".to_owned(),
        ..reservation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_origin_invalid"
    );

    let invalid = ProgrammaticPolicyReservationRecordDto {
        policy_id: "token=live".to_owned(),
        ..reservation()
    };
    assert_eq!(rejection_code(invalid.validate()), "credentials_forbidden");

    let invalid = ProgrammaticPolicyReservationRecordDto {
        typed_input_digest: "sha256:short".to_owned(),
        ..reservation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_reservation_conflict"
    );

    let invalid = ProgrammaticPolicyReservationRecordDto {
        policy_revision: 0,
        ..reservation()
    };
    assert_eq!(
        rejection_code(invalid.validate()),
        "programmatic_policy_reservation_conflict"
    );
}

#[test]
fn programmatic_remaining_records_preserve_typed_fields() {
    assert_eq!(MAX_POLICY_RECORD_PAGE, 64);

    let origin_rule = policy_origin_rule(ProgrammaticRootOriginKindDto::ContinualHarness);
    assert_eq!(
        origin_rule.root_origin_kind,
        ProgrammaticRootOriginKindDto::ContinualHarness
    );
    assert_eq!(
        origin_rule.maximum_decision,
        ProgrammaticAdmissionDecisionDto::DirectLocalRead
    );
    let cloned = origin_rule;
    assert_eq!(cloned, origin_rule);

    let calendar_limit = ProgrammaticCalendarLimitRecordDto {
        period_kind: ProgrammaticCalendarPeriodKindDto::Month,
        max_actions: 128,
    };
    assert_eq!(calendar_limit.period_kind.name(), "month");
    assert_eq!(calendar_limit.max_actions, 128);

    let counter_reference = ProgrammaticCalendarCounterReferenceDto {
        policy_id: "policy-a".to_owned(),
        period_kind: ProgrammaticCalendarPeriodKindDto::Week,
    };
    let cloned = counter_reference.clone();
    assert_eq!(cloned, counter_reference);
    assert_eq!(cloned.period_kind.name(), "week");
    assert!(!format!("{cloned:?}").is_empty());

    let lifecycle = TransitionProgrammaticPolicyLifecycleInputDto {
        policy_id: "policy-a".to_owned(),
        expected_revision: 1,
        operation: ProgrammaticPolicyLifecycleOperationDto::Suspend,
        has_active_dependent_tree: true,
        occurred_at_ms: 30,
    };
    assert_eq!(lifecycle.operation.name(), "suspend");
    assert!(lifecycle.has_active_dependent_tree);
    assert_eq!(lifecycle.expected_revision, 1);

    let decision = DecideProgrammaticPolicyConfirmationInputDto {
        confirmation_id: "confirmation-a".to_owned(),
        tool_call_id: "tool-call-a".to_owned(),
        typed_input_digest: digest('d'),
        state: ProgrammaticConfirmationStateDto::Accepted,
        decided_at_ms: 30,
    };
    let cloned = decision.clone();
    assert_eq!(cloned, decision);
    assert_eq!(cloned.state.name(), "accepted");
    assert_eq!(cloned.decided_at_ms, 30);
    assert!(!format!("{cloned:?}").is_empty());

    let consume = ConsumeProgrammaticCorridorActionInputDto {
        corridor_digest: digest('f'),
        root_run_id: "run-a".to_owned(),
        occurred_at_ms: 40,
    };
    assert_eq!(consume.root_run_id, "run-a");
    assert_eq!(consume.occurred_at_ms, 40);

    let reserve = ReserveProgrammaticPolicyActionInputDto {
        reservation: reservation(),
        max_actions_per_run: 4,
        max_concurrent_actions_per_run: 2,
        calendar_max_actions: 8,
        calendar_window_start_ms: 0,
        calendar_window_end_ms: 86_400_000,
        calendar_window_time_zone: "UTC".to_owned(),
    };
    let cloned = reserve.clone();
    assert_eq!(cloned, reserve);
    assert_eq!(cloned.reservation.state.name(), "reserved");
    assert_eq!(cloned.max_actions_per_run, 4);
    assert_eq!(cloned.max_concurrent_actions_per_run, 2);
    assert_eq!(cloned.calendar_max_actions, 8);
    assert!(!format!("{cloned:?}").is_empty());

    let reserve_outcome = ReserveProgrammaticPolicyActionOutcomeDto {
        reservation: reservation(),
        counters: counter(),
        replayed: true,
    };
    assert!(reserve_outcome.replayed);
    assert_eq!(reserve_outcome.counters.calendar_started_actions, 1);
    assert_eq!(reserve_outcome.reservation.policy_revision, 1);
    assert!(!format!("{reserve_outcome:?}").is_empty());

    let release = ReleaseProgrammaticPolicyReservationInputDto {
        reservation_reference: "reservation-a".to_owned(),
        tool_call_id: "tool-call-a".to_owned(),
        released_at_ms: 50,
    };
    let cloned = release.clone();
    assert_eq!(cloned, release);
    assert_eq!(cloned.released_at_ms, 50);
    assert!(!format!("{cloned:?}").is_empty());

    let started = CommitProgrammaticReservationStartedInputDto {
        reservation_reference: "reservation-a".to_owned(),
        tool_call_id: "tool-call-a".to_owned(),
        started_at_ms: 60,
    };
    assert_eq!(started.started_at_ms, 60);
    assert_eq!(started.reservation_reference, "reservation-a");

    let recovered = RecoverProgrammaticPolicyReservationInputDto {
        reservation_reference: "reservation-a".to_owned(),
        tool_call_started: false,
        recovered_at_ms: 70,
    };
    assert!(!recovered.tool_call_started);
    assert_eq!(recovered.recovered_at_ms, 70);
    assert!(!format!("{recovered:?}").is_empty());
}
