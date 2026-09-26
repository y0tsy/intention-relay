//! Daemon continual-harness scheduling, journal, and recovery evidence.
//!
//! These fixtures drive the daemon-owned [`HarnessScheduleHost`] against a
//! temporary SQLite database and deterministic times: the daemon owns the tick
//! cadence, the project time zone, the live concurrency signal, the durable
//! journal read surface, and restart recovery. No fixture sleeps, binds a port,
//! or reaches the network, and restart recovery never resumes, reattaches,
//! retries, or reruns an interrupted launch.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Focused daemon harness-recovery fixtures use assertion conveniences for precise diagnostics."
)]

use intention::{
    DaemonApplicationFacade, HarnessAdmittedLaunchDto, HarnessLaunchOutcomeDto,
    harness_repo::{
        CreateHarnessRuleInputDto, HarnessExecutionClassDto, HarnessJournalRecordKindDto,
        HarnessPresentationModeDto, HarnessRuleLifecycleStateDto, HarnessRuleOperationDto,
        HarnessRuleRecordDto, HarnessRuleRevisionRecordDto, HarnessRuleScopeDto,
        HarnessSourceKindDto, HarnessTaskModeDto, HarnessTriggerCaptureOutcomeDto,
        HarnessTriggerReasonStateDto, TransitionHarnessRuleLifecycleInputDto,
    },
};
use intention_config::ConfigSnapshotDto;
use intention_daemon::HarnessScheduleHost;
use intention_domain::{
    CreateSessionCommandDto, RunModeDto, WorkspaceRootDto, harness::HarnessRunOutcomeV1,
};
use intention_protocol::{ProtocolCommandDto, ProtocolCommandResultDto};
use intention_types::{
    ConfigRevisionId, ProjectId, RunId, SchemaVersionDto, SessionId, TimestampDto, WorkspaceId,
};
use tempfile::TempDir;

/// The durable equal-interval anchor of every fixture rule.
const INTERVAL_ANCHOR_MS: u64 = 1_000_000;
/// The fixed fixture cadence, above the one-minute minimum.
const INTERVAL_MS: u64 = 60_000;
/// The daemon-owned recovery time of every fixture restart.
const RECOVERY_MS: u64 = 9_000_000;

fn fixture_digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn fixture_snapshot() -> ConfigSnapshotDto {
    let source = intention_config::ConfigSourceDto::Explicit(
        intention_config::ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-daemon-slice3-recovery.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture configuration path is absolute"),
    );
    let resolved = intention_config::ResolvedConfigDto::parse_resolve(
        intention_config::RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"",
            source,
        ),
    )
    .expect("fixture configuration resolves");
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid"),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

/// Opens the durable composition of one fixture daemon epoch.
fn open_facade(directory: &TempDir) -> DaemonApplicationFacade {
    DaemonApplicationFacade::open_for_test_support(
        directory.path().join("harness-recovery.sqlite"),
        fixture_snapshot(),
    )
    .expect("fixture facade opens")
}

/// Creates one ordinary user session and returns its identity.
fn create_session(facade: &DaemonApplicationFacade) -> (ProjectId, SessionId) {
    let project_id = ProjectId::new();
    let session_id = SessionId::new();
    // A workspace root binds exactly one workspace identity, so every fixture
    // session names its own workspace directory below the platform temp root.
    let workspace = std::env::temp_dir().join(format!("intention-harness-{session_id}"));
    let created = facade.command(ProtocolCommandDto::CreateSession(
        CreateSessionCommandDto::new(
            project_id,
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(workspace.to_string_lossy().into_owned())
                .expect("fixture workspace is absolute"),
            RunModeDto::Build,
        ),
    ));
    assert!(
        matches!(created, ProtocolCommandResultDto::Accepted(_)),
        "fixture session creates: {created:?}"
    );
    (project_id, session_id)
}

/// Creates one session-scoped active rule with a single fixed-interval source.
fn create_interval_rule(
    facade: &DaemonApplicationFacade,
    project_id: ProjectId,
    session_id: SessionId,
    applied_time_zone: &str,
) -> String {
    let harness_id = SessionId::new().to_string();
    facade
        .create_harness_rule_for_daemon(CreateHarnessRuleInputDto {
            rule: HarnessRuleRecordDto {
                harness_id: harness_id.clone(),
                scope: HarnessRuleScopeDto::UserSession {
                    project_id: project_id.to_string(),
                    session_id: session_id.to_string(),
                },
                lifecycle_state: HarnessRuleLifecycleStateDto::Active,
                active_revision: 1,
                service_session_id: SessionId::new().to_string(),
                updated_at_ms: 1_000,
            },
            revision: HarnessRuleRevisionRecordDto {
                harness_id: harness_id.clone(),
                revision: 1,
                task_digest: fixture_digest('a'),
                class: HarnessExecutionClassDto::Light,
                task_mode: HarnessTaskModeDto::RepeatedTask,
                presentation_mode: HarnessPresentationModeDto::JournalOnly,
                applied_time_zone: applied_time_zone.to_owned(),
                source_kinds: vec![HarnessSourceKindDto::FixedInterval],
                source_references: Vec::new(),
                interval_anchor_ms: Some(INTERVAL_ANCHOR_MS),
                interval_ms: Some(INTERVAL_MS),
                calendar_expression: None,
                completion_link_reference: None,
                completion_outcomes: Vec::new(),
                canonical_revision_digest: fixture_digest('b'),
                created_at_ms: 1_000,
            },
        })
        .expect("fixture harness rule creates");
    harness_id
}

/// Returns the admitted launch of one tick, failing when none was admitted.
fn admitted_launch(
    outcome: &intention::HarnessScheduleTickOutcomeDto,
) -> &HarnessAdmittedLaunchDto {
    let Some(HarnessLaunchOutcomeDto::Admitted(admitted)) = outcome.launch.as_ref() else {
        panic!("the fixture tick admits exactly one launch")
    };
    admitted
}

/// Admits one launch through the daemon tick and returns its run identity.
fn admit_one_launch(host: &HarnessScheduleHost, harness_id: &str, observed_at_ms: u64) -> String {
    let outcome = host
        .tick(harness_id, observed_at_ms)
        .expect("the daemon tick admits one launch");
    admitted_launch(&outcome).run_id.clone()
}

#[tokio::test]
async fn schedule_tick_captures_the_newest_due_slot_and_coalesces_missed_slots() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade.clone());
    assert_eq!(host.project_time_zone(), "UTC");
    assert_eq!(host.tick_cadence().as_secs(), 60);
    let (project_id, session_id) = create_session(&facade);
    let harness_id =
        create_interval_rule(&facade, project_id, session_id, host.project_time_zone());

    // Five interval slots have passed since the anchor; exactly the newest one
    // is captured and admitted.
    let observed_at_ms = INTERVAL_ANCHOR_MS + (5 * INTERVAL_MS) + 30_000;
    let outcome = host
        .tick(&harness_id, observed_at_ms)
        .expect("the daemon tick captures and admits");
    assert_eq!(outcome.harness_id, harness_id);
    assert_eq!(outcome.observations.len(), 1);
    assert_eq!(
        outcome.observations[0].observed_at_ms,
        INTERVAL_ANCHOR_MS + (5 * INTERVAL_MS)
    );
    assert_eq!(
        outcome.observations[0].capture.outcome,
        HarnessTriggerCaptureOutcomeDto::Captured
    );
    assert!(
        outcome.observations[0].capture.journal_record.is_some(),
        "a newly captured reason appends its durable journal record"
    );
    let admitted = admitted_launch(&outcome);
    assert_eq!(admitted.harness_id, harness_id);
    assert_eq!(admitted.rule_revision, 1);
    assert_eq!(
        admitted.reason.state,
        HarnessTriggerReasonStateDto::Admitted
    );
    assert!(RunId::parse(&admitted.run_id).is_ok());
    assert!(outcome.pending_reason.is_none());

    let journal = host
        .load_journal(session_id, &harness_id, 0, 64)
        .expect("the durable journal reads");
    let kinds: Vec<HarnessJournalRecordKindDto> =
        journal.iter().map(|record| record.record_kind).collect();
    assert_eq!(
        kinds,
        vec![
            HarnessJournalRecordKindDto::TriggerCaptured,
            HarnessJournalRecordKindDto::LaunchAdmitted
        ]
    );
    assert_eq!(journal[0].sequence, 1);
    assert_eq!(journal[1].sequence, 2);
    let journal_json = format!("{journal:?}");
    assert!(
        !journal_json.contains(&std::env::temp_dir().to_string_lossy().into_owned()),
        "durable journal records never disclose a filesystem path"
    );
}

#[tokio::test]
async fn daemon_project_time_zone_applies_to_non_archived_rules_only() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade.clone());
    let (project_id, session_id) = create_session(&facade);
    let harness_id = create_interval_rule(&facade, project_id, session_id, "America/New_York");

    let observed_at_ms = INTERVAL_ANCHOR_MS + INTERVAL_MS;
    let error = host
        .tick(&harness_id, observed_at_ms)
        .expect_err("a non-archived rule follows the daemon project time zone");
    assert_eq!(error.code(), "harness_schedule_invalid");

    // Archiving retains the zone recorded by the immutable revision, so the
    // zone mismatch no longer applies and the archived rule rejects capture.
    facade
        .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
            harness_id: harness_id.clone(),
            expected_revision: 1,
            operation: HarnessRuleOperationDto::Archive,
            has_active_run: false,
            occurred_at_ms: 2_000,
        })
        .expect("the fixture rule archives");
    let error = host
        .tick(&harness_id, observed_at_ms)
        .expect_err("an archived rule rejects capture");
    assert_eq!(error.code(), "harness_archived");
}

#[tokio::test]
async fn an_occupied_concurrency_signal_retains_the_coalesced_reason() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade.clone());
    let (project_id, session_id) = create_session(&facade);
    let harness_id =
        create_interval_rule(&facade, project_id, session_id, host.project_time_zone());

    let observed_at_ms = INTERVAL_ANCHOR_MS + INTERVAL_MS;
    host.observe_concurrency(false);
    let outcome = host
        .tick(&harness_id, observed_at_ms)
        .expect("the daemon tick captures and retains");
    let Some(HarnessLaunchOutcomeDto::Retained(_)) = outcome.launch else {
        panic!("an occupied daemon slot retains the launch")
    };
    let pending = outcome
        .pending_reason
        .expect("the coalesced reason stays durable while the daemon slot is occupied");
    assert_eq!(pending.state, HarnessTriggerReasonStateDto::Pending);
    assert_eq!(pending.coalesced_count, 1);
    assert_eq!(pending.last_observed_at_ms, observed_at_ms);

    // The next observation of the same durable reason admits it: a retained
    // reason is never lost and never launched twice.
    host.observe_concurrency(true);
    let outcome = host
        .tick(&harness_id, observed_at_ms)
        .expect("the next tick admits the retained reason");
    assert_eq!(
        outcome.observations[0].capture.outcome,
        HarnessTriggerCaptureOutcomeDto::Redelivered
    );
    let admitted = admitted_launch(&outcome);
    assert_eq!(
        admitted.reason.state,
        HarnessTriggerReasonStateDto::Admitted
    );
    assert!(outcome.pending_reason.is_none());
}

#[tokio::test]
async fn durable_journal_is_readable_after_a_restart_and_scoped_to_its_session() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade.clone());
    let (project_id, session_id) = create_session(&facade);
    let harness_id =
        create_interval_rule(&facade, project_id, session_id, host.project_time_zone());
    let observed_at_ms = INTERVAL_ANCHOR_MS + INTERVAL_MS;
    admit_one_launch(&host, &harness_id, observed_at_ms);
    drop(host);
    drop(facade);

    // A fresh daemon epoch re-reads the journal of the same durable rule.
    let restarted_facade = open_facade(&directory);
    let restarted_host = HarnessScheduleHost::new(restarted_facade.clone());
    let journal = restarted_host
        .load_journal(session_id, &harness_id, 0, 64)
        .expect("the journal reads after a restart");
    assert_eq!(journal.len(), 2);
    assert!(journal.iter().all(|record| record.harness_id == harness_id));
    let page = restarted_host
        .load_journal(session_id, &harness_id, 1, 64)
        .expect("the journal pages after a restart");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].sequence, 2);

    let (_other_project, other_session) = create_session(&restarted_facade);
    let error = restarted_host
        .load_journal(other_session, &harness_id, 0, 64)
        .expect_err("a harness journal is readable only by its owning session");
    assert_eq!(error.code(), "harness_source_unavailable");
    let error = restarted_host
        .load_journal(session_id, &harness_id, 0, 0)
        .expect_err("an empty journal page fails closed");
    assert_eq!(error.code(), "invalid_harness_journal_page");
}

#[tokio::test]
async fn restart_recovery_leaves_an_interrupted_launch_interrupted_without_resuming_it() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade.clone());
    let (project_id, session_id) = create_session(&facade);
    let harness_id =
        create_interval_rule(&facade, project_id, session_id, host.project_time_zone());
    let observed_at_ms = INTERVAL_ANCHOR_MS + INTERVAL_MS;
    let interrupted_run_id = admit_one_launch(&host, &harness_id, observed_at_ms);
    drop(host);
    drop(facade);

    // The restarted daemon owns the no-resume recovery of the launch it
    // observed before the restart.
    let restarted_facade = open_facade(&directory);
    let restarted_host = HarnessScheduleHost::new(restarted_facade);
    let recovery = restarted_host
        .recover_interrupted_launch(&harness_id, &interrupted_run_id, RECOVERY_MS)
        .expect("the interrupted launch recovers");
    assert_eq!(recovery.terminal.outcome, HarnessRunOutcomeV1::Interrupted);
    assert_eq!(recovery.terminal.harness_id, harness_id);
    assert_eq!(recovery.terminal.producing_run_id, interrupted_run_id);
    assert!(!recovery.external_work_resumes);
    assert!(recovery.requires_separate_admission);
    assert!(
        recovery.terminal.journal_records.iter().any(|record| {
            record.record_kind == HarnessJournalRecordKindDto::RunTerminal
                && record.safe_summary.contains("interrupted")
        }),
        "the interrupted terminal decision is durable"
    );

    // No resumed work and no scheduled successor: the already captured slot is
    // redelivered without a new launch and without a new pending reason.
    let outcome = restarted_host
        .tick(&harness_id, observed_at_ms)
        .expect("the restarted tick observes no new slot");
    assert_eq!(
        outcome.observations[0].capture.outcome,
        HarnessTriggerCaptureOutcomeDto::Redelivered
    );
    assert!(outcome.launch.is_none());
    assert!(outcome.pending_reason.is_none());
    let journal = restarted_host
        .load_journal(session_id, &harness_id, 0, 64)
        .expect("the restarted journal reads");
    assert!(
        journal
            .iter()
            .any(|record| record.record_kind == HarnessJournalRecordKindDto::RunTerminal)
    );
}

#[tokio::test]
async fn a_redelivered_slot_is_idempotent_and_never_launches_twice() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade.clone());
    let (project_id, session_id) = create_session(&facade);
    let harness_id =
        create_interval_rule(&facade, project_id, session_id, host.project_time_zone());
    let observed_at_ms = INTERVAL_ANCHOR_MS + INTERVAL_MS;
    admit_one_launch(&host, &harness_id, observed_at_ms);
    let journal_before = host
        .load_journal(session_id, &harness_id, 0, 64)
        .expect("the journal reads");

    // The equal observation is a redelivery: the same reason identity changes
    // nothing durable and admits nothing.
    let outcome = host
        .tick(&harness_id, observed_at_ms)
        .expect("the redelivered tick changes nothing");
    assert_eq!(
        outcome.observations[0].capture.outcome,
        HarnessTriggerCaptureOutcomeDto::Redelivered
    );
    assert!(outcome.observations[0].capture.journal_record.is_none());
    assert!(outcome.launch.is_none());
    assert!(outcome.pending_reason.is_none());
    let journal_after = host
        .load_journal(session_id, &harness_id, 0, 64)
        .expect("the journal reads");
    assert_eq!(journal_after.len(), journal_before.len());

    // A later slot is captured, but the rule keeps its single live launch and
    // the daemon retains the newest coalesced reason for a later observation.
    let outcome = host
        .tick(&harness_id, observed_at_ms + INTERVAL_MS)
        .expect("the later slot is captured and retained");
    assert_eq!(
        outcome.observations[0].capture.outcome,
        HarnessTriggerCaptureOutcomeDto::Captured
    );
    let Some(HarnessLaunchOutcomeDto::Retained(_)) = outcome.launch else {
        panic!("the rule keeps at most one live launch")
    };
    let pending = outcome.pending_reason.expect("the retained reason stays");
    assert_eq!(pending.state, HarnessTriggerReasonStateDto::Pending);
    assert_eq!(pending.first_observed_at_ms, observed_at_ms + INTERVAL_MS);
}

#[tokio::test]
async fn daemon_cadence_pass_isolates_failing_rules_and_schedules_each_observed_rule_once() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade.clone());
    let (project_id, session_id) = create_session(&facade);
    let harness_id =
        create_interval_rule(&facade, project_id, session_id, host.project_time_zone());

    // The daemon owns its observed-rule registry: a repeated observation of the
    // equal identity changes nothing, so the rule is scheduled exactly once.
    host.observe_rule("sk-live-unobservable-rule");
    host.observe_rule(&harness_id);
    host.observe_rule(&harness_id);
    assert_eq!(
        host.observed_rules(),
        vec!["sk-live-unobservable-rule".to_owned(), harness_id.clone()],
        "each observed rule identity is recorded exactly once"
    );

    // One cadence pass applies every observed rule: the credential-shaped rule
    // fails in isolation and the durable rule still admits its newest slot.
    let observed_at_ms = INTERVAL_ANCHOR_MS + INTERVAL_MS;
    host.schedule_pass(observed_at_ms);

    let journal = host
        .load_journal(session_id, &harness_id, 0, 64)
        .expect("the admitted rule journal reads");
    let kinds: Vec<HarnessJournalRecordKindDto> =
        journal.iter().map(|record| record.record_kind).collect();
    assert_eq!(
        kinds,
        vec![
            HarnessJournalRecordKindDto::TriggerCaptured,
            HarnessJournalRecordKindDto::LaunchAdmitted
        ],
        "the pass continues past the failing rule and admits exactly one launch"
    );

    // An equal cadence pass is a redelivery: it admits nothing and appends no
    // new durable record.
    host.schedule_pass(observed_at_ms);
    let outcome = host
        .tick(&harness_id, observed_at_ms)
        .expect("the equal cadence pass stays redelivered");
    assert_eq!(
        outcome.observations[0].capture.outcome,
        HarnessTriggerCaptureOutcomeDto::Redelivered
    );
    assert!(outcome.launch.is_none());
    assert!(outcome.pending_reason.is_none());
    assert_eq!(
        host.load_journal(session_id, &harness_id, 0, 64)
            .expect("the redelivered journal reads")
            .len(),
        journal.len()
    );
}

#[tokio::test]
async fn startup_policy_reservation_recovery_changes_nothing_without_reservations() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade);
    let recovered = host
        .recover_policy_reservations(&RunId::new().to_string(), RECOVERY_MS)
        .expect("startup recovery reads the durable reservations");
    assert!(recovered.is_empty());
}

#[tokio::test]
async fn facade_boundary_rejects_credentials_paths_and_non_canonical_identities() {
    let directory = TempDir::new().expect("temporary directory exists");
    let facade = open_facade(&directory);
    let host = HarnessScheduleHost::new(facade);
    let observed_at_ms = INTERVAL_ANCHOR_MS + INTERVAL_MS;
    let path_shaped = std::env::temp_dir().join("harness-rule.sqlite");
    let path_text = path_shaped.to_string_lossy().into_owned();

    let error = host
        .tick("sk-live-daemon-boundary", observed_at_ms)
        .expect_err("credentials never cross the daemon boundary");
    assert_eq!(error.code(), "credentials_forbidden");

    let error = host
        .tick(&path_text, observed_at_ms)
        .expect_err("a filesystem path is never a harness identity");
    assert_eq!(error.code(), "harness_source_unavailable");

    let error = host
        .tick("harness-rule-1", observed_at_ms)
        .expect_err("a non-canonical identity fails closed");
    assert_eq!(error.code(), "harness_source_unavailable");

    let error = host
        .load_journal(SessionId::new(), "sk-journal-credential", 0, 64)
        .expect_err("credentials never cross the journal boundary");
    assert_eq!(error.code(), "credentials_forbidden");

    let error = host
        .recover_interrupted_launch("sk-recovery", &RunId::new().to_string(), RECOVERY_MS)
        .expect_err("credentials never cross the recovery boundary");
    assert_eq!(error.code(), "credentials_forbidden");

    let error = host
        .recover_interrupted_launch(&RunId::new().to_string(), &path_text, RECOVERY_MS)
        .expect_err("a filesystem path is never an interrupted launch identity");
    assert_eq!(error.code(), "harness_source_unavailable");
}
