#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Harness runtime fixtures use expect and panic for precise diagnostics."
)]

//! Slice 3 harness runtime and recovery tests.
//!
//! Owner: architecture 26 with ADR 0044. These tests pin trigger admission
//! into a fresh run, dossier assembly and post-commit publication,
//! cancellation cascade, and restart behavior with no resume or reattach.
//!
//! Every fixture is hermetic and deterministic: the in-memory repositories
//! below mirror the durable capture, coalescing, bound, checkpoint, counter,
//! and journal invariants; time is fixed Unix milliseconds; and no test
//! sleeps, binds a port, or touches the network. The fake-secret sweep checks
//! that no service DTO `Debug` output carries credential-shaped text.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use intention_application::harness::{
    CaptureHarnessTriggerRequestDto, HarnessAdmittedLaunchDto, HarnessCancellationInputDto,
    HarnessCatchUpObservationDto, HarnessCheckpointCandidateDto, HarnessConclusionInputDto,
    HarnessConclusionPublicationDto, HarnessGoalReferenceDto, HarnessLaunchOutcomeDto,
    HarnessLaunchRequestDto, HarnessPublicationPort, HarnessRestartRecoveryInputDto,
    HarnessRetainedTriggerDto, HarnessRunOutcomeInputDto, HarnessRuntimeService,
    HarnessSubAgentCorridorSelectionDto,
};
use intention_domain::canonical::Digest256;
use intention_domain::harness::{
    HarnessDisconnectContractV1, HarnessLaunchOriginV1, HarnessRunOutcomeV1,
};
use intention_domain::slice3_selections::{HarnessExecutionClassV1, HarnessSourceKindV1};
use intention_storage::harness_repo::{
    AppendHarnessJournalRecordInputDto, CaptureHarnessTriggerInputDto,
    CommitHarnessCheckpointInputDto, CommitHarnessCheckpointOutcomeDto, CreateHarnessRuleInputDto,
    HarnessCheckpointDispositionDto, HarnessCheckpointRecordDto, HarnessCheckpointRepositoryDto,
    HarnessCounterRecordDto, HarnessDossierRecordDto, HarnessExecutionClassDto,
    HarnessJournalRecordDto, HarnessJournalRecordKindDto, HarnessPresentationModeDto,
    HarnessRuleLifecycleStateDto, HarnessRuleOperationDto, HarnessRuleRecordDto,
    HarnessRuleRepositoryDto, HarnessRuleRevisionRecordDto, HarnessRuleScopeDto,
    HarnessRunOutcomeDto, HarnessSourceKindDto, HarnessTaskModeDto,
    HarnessTriggerCaptureOutcomeDto, HarnessTriggerCaptureRecordDto, HarnessTriggerReasonRecordDto,
    HarnessTriggerReasonStateDto, HarnessTriggerRepositoryDto, LoadHarnessJournalInputDto,
    MAX_HARNESS_JOURNAL_PAGE, MAX_HARNESS_TRIGGER_REFERENCES, RecordHarnessLaunchInputDto,
    RecordHarnessLaunchOutcomeDto, ReviseHarnessRuleInputDto,
    TransitionHarnessRuleLifecycleInputDto,
};
use intention_types::{DtoResult, ErrorDto};

const MAX_FILE_BYTES: u64 = 512 * 1024;

/// Formats one fixture identity as canonical UUID text.
fn identity_text(byte: u8) -> String {
    let hex: String = std::iter::repeat_n(format!("{byte:02x}"), 16).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// A fixed daemon-assigned identity of one repeated byte.
const fn identity(byte: u8) -> [u8; 16] {
    [byte; 16]
}

/// A fixed deterministic digest.
fn digest(seed: u8) -> Digest256 {
    Digest256::sha256(&[seed; 16])
}

/// The canonical digest text of one fixed digest.
fn digest_text(seed: u8) -> String {
    digest(seed).to_string()
}

/// The fixture harness rule, one revision, fixed interval source.
fn rule(state: HarnessRuleLifecycleStateDto) -> HarnessRuleRecordDto {
    HarnessRuleRecordDto {
        harness_id: identity_text(1),
        scope: HarnessRuleScopeDto::Project {
            project_id: identity_text(2),
        },
        lifecycle_state: state,
        active_revision: 1,
        service_session_id: identity_text(3),
        updated_at_ms: 1_000,
    }
}

/// The fixture immutable rule revision.
fn revision() -> HarnessRuleRevisionRecordDto {
    HarnessRuleRevisionRecordDto {
        harness_id: identity_text(1),
        revision: 1,
        task_digest: digest_text(4),
        class: HarnessExecutionClassDto::Light,
        task_mode: HarnessTaskModeDto::RepeatedTask,
        presentation_mode: HarnessPresentationModeDto::JournalAndActivityEntry,
        applied_time_zone: "Europe/Berlin".to_owned(),
        source_kinds: vec![HarnessSourceKindDto::FixedInterval],
        source_references: vec![identity_text(5)],
        interval_anchor_ms: Some(0),
        interval_ms: Some(60_000),
        calendar_expression: None,
        completion_link_reference: None,
        completion_outcomes: Vec::new(),
        canonical_revision_digest: digest_text(6),
        created_at_ms: 1_000,
    }
}

/// The fixture pending reason of one rule.
fn pending_reason(reason_id: String, observed_at_ms: u64) -> HarnessTriggerReasonRecordDto {
    HarnessTriggerReasonRecordDto {
        reason_id,
        harness_id: identity_text(1),
        source_kind: HarnessSourceKindDto::FixedInterval,
        rule_revision: 1,
        first_observed_at_ms: observed_at_ms,
        last_observed_at_ms: observed_at_ms,
        coalesced_count: 1,
        applied_time_zone: "Europe/Berlin".to_owned(),
        cause_chain_reference: None,
        bounded_references: vec![identity_text(12)],
        state: HarnessTriggerReasonStateDto::Pending,
    }
}

/// The conventional pending reason of one fixture rule.
fn default_pending() -> HarnessTriggerReasonRecordDto {
    pending_reason(identity_text(11), 1_000)
}

/// The fixture durable counters of an idle rule.
const fn counters() -> HarnessCounterRecordDto {
    HarnessCounterRecordDto {
        cause_chain_depth: 0,
        concurrent_non_terminal: 0,
        total_launches: 0,
        direct_successors: 0,
        updated_at_ms: 1_000,
    }
}

/// The fixture current verified checkpoint of one revision.
fn checkpoint(revision: u64) -> HarnessCheckpointRecordDto {
    HarnessCheckpointRecordDto {
        checkpoint_id: identity_text(61),
        harness_id: identity_text(1),
        rule_revision: 1,
        producing_run_id: identity_text(62),
        checkpoint_revision: revision,
        content_digest: digest_text(63),
        checkpoint_bytes: 4_096,
        is_current: true,
        created_at_ms: 1_000,
    }
}

/// One capture request of the fixture rule.
fn capture_request(
    reason_id: String,
    source_kind: HarnessSourceKindV1,
    observed_at_ms: u64,
) -> CaptureHarnessTriggerRequestDto {
    CaptureHarnessTriggerRequestDto {
        harness_id: identity_text(1),
        reason_id,
        source_kind,
        observed_at_ms,
        cause_chain_reference: None,
        bounded_references: vec![identity_text(12)],
        catch_up: None,
    }
}

/// One launch request of the fixture rule.
fn launch_request(
    narrowed_tool_ids: Vec<String>,
    sub_agent_corridor: Option<HarnessSubAgentCorridorSelectionDto>,
    goal: Option<HarnessGoalReferenceDto>,
) -> HarnessLaunchRequestDto {
    HarnessLaunchRequestDto {
        harness_id: identity_text(1),
        proposed_run_id: identity_text(21),
        origin: HarnessLaunchOriginV1::AutomaticSource,
        daemon_concurrency_available: true,
        cause_chain_depth: 1,
        requested_class: HarnessExecutionClassV1::Heavy,
        narrowed_tool_ids,
        sub_agent_corridor,
        goal,
        dossier_bytes: 2_048,
        occurred_at_ms: 2_000,
    }
}

/// One run-terminal input of the fixture rule.
fn run_outcome_input(
    outcome: HarnessRunOutcomeV1,
    candidate_checkpoint: Option<HarnessCheckpointCandidateDto>,
    conclusion: Option<HarnessConclusionInputDto>,
) -> HarnessRunOutcomeInputDto {
    HarnessRunOutcomeInputDto {
        harness_id: identity_text(1),
        producing_run_id: identity_text(21),
        outcome,
        candidate_checkpoint,
        conclusion,
        occurred_at_ms: 3_000,
    }
}

/// One completely validated checkpoint candidate of the next revision.
fn candidate(revision: u64) -> HarnessCheckpointCandidateDto {
    HarnessCheckpointCandidateDto {
        checkpoint_id: identity(64),
        checkpoint_revision: revision,
        content_digest: digest(65),
        checkpoint_bytes: 8_192,
    }
}

/// One bounded safe conclusion of the fixture run.
fn conclusion() -> HarnessConclusionInputDto {
    HarnessConclusionInputDto {
        content_digest: digest(66),
        conclusion_bytes: 1_024,
    }
}

/// Extracts the admitted launch of one outcome.
fn admitted(outcome: HarnessLaunchOutcomeDto) -> HarnessAdmittedLaunchDto {
    match outcome {
        HarnessLaunchOutcomeDto::Admitted(admitted) => *admitted,
        HarnessLaunchOutcomeDto::Retained(retained) => {
            panic!("expected an admitted launch, got a retained reason: {retained:?}")
        }
    }
}

/// Extracts the retained reason of one outcome.
fn retained(outcome: HarnessLaunchOutcomeDto) -> HarnessRetainedTriggerDto {
    match outcome {
        HarnessLaunchOutcomeDto::Retained(retained) => *retained,
        HarnessLaunchOutcomeDto::Admitted(admitted) => {
            panic!("expected a retained reason, got an admitted launch: {admitted:?}")
        }
    }
}

/// The in-memory harness repositories shared by every harness runtime test.
struct FakeHarness {
    log: Rc<RefCell<Vec<String>>>,
    rule: RefCell<HarnessRuleRecordDto>,
    revision: RefCell<HarnessRuleRevisionRecordDto>,
    reason: RefCell<Option<HarnessTriggerReasonRecordDto>>,
    counters: RefCell<HarnessCounterRecordDto>,
    dossiers: RefCell<Vec<HarnessDossierRecordDto>>,
    checkpoints: RefCell<Vec<HarnessCheckpointRecordDto>>,
    journal: RefCell<Vec<HarnessJournalRecordDto>>,
    launch_denial: RefCell<Option<&'static str>>,
    fail_journal_read: Cell<bool>,
    fail_journal_append: Cell<bool>,
    fail_journal_append_on: Cell<u32>,
    journal_append_attempts: Cell<u32>,
    fail_checkpoint_commit: Cell<bool>,
    hide_journal_read: Cell<bool>,
}

impl FakeHarness {
    /// Creates an idle rule with no pending reason, checkpoint, or journal.
    fn new() -> Self {
        Self::with_log(Rc::new(RefCell::new(Vec::new())))
    }

    /// Creates a fake sharing one deterministic call log.
    fn with_log(log: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            log,
            rule: RefCell::new(rule(HarnessRuleLifecycleStateDto::Active)),
            revision: RefCell::new(revision()),
            reason: RefCell::new(None),
            counters: RefCell::new(counters()),
            dossiers: RefCell::new(Vec::new()),
            checkpoints: RefCell::new(Vec::new()),
            journal: RefCell::new(Vec::new()),
            launch_denial: RefCell::new(None),
            fail_journal_read: Cell::new(false),
            fail_journal_append: Cell::new(false),
            fail_journal_append_on: Cell::new(0),
            journal_append_attempts: Cell::new(0),
            fail_checkpoint_commit: Cell::new(false),
            hide_journal_read: Cell::new(false),
        }
    }

    /// Records one observed call.
    fn record(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }

    /// Returns every observed call in order.
    fn calls(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    /// Installs the single pending reason of the rule.
    fn set_pending(&self, reason: HarnessTriggerReasonRecordDto) {
        *self.reason.borrow_mut() = Some(reason);
    }

    /// Returns the current pending reason, if any.
    fn pending(&self) -> Option<HarnessTriggerReasonRecordDto> {
        self.reason.borrow().clone()
    }

    /// Installs the durable counters of the rule.
    fn set_counters(&self, counters: HarnessCounterRecordDto) {
        *self.counters.borrow_mut() = counters;
    }

    /// Sets the durable lifecycle state of the rule.
    fn set_lifecycle(&self, state: HarnessRuleLifecycleStateDto) {
        self.rule.borrow_mut().lifecycle_state = state;
    }

    /// Sets the closed task mode of the rule revision.
    fn set_task_mode(&self, mode: HarnessTaskModeDto) {
        self.revision.borrow_mut().task_mode = mode;
    }

    /// Sets the presentation mode of the rule revision.
    fn set_presentation_mode(&self, mode: HarnessPresentationModeDto) {
        self.revision.borrow_mut().presentation_mode = mode;
    }

    /// Sets the closed execution class of the rule revision.
    fn set_class(&self, class: HarnessExecutionClassDto) {
        self.revision.borrow_mut().class = class;
    }

    /// Sets the applied project time zone of the rule revision.
    fn set_applied_time_zone(&self, time_zone: &str) {
        self.revision.borrow_mut().applied_time_zone = time_zone.to_owned();
    }

    /// Fails the journal append at the given attempt, counting from one.
    fn fail_journal_append_on(&self, attempt: u32) {
        self.fail_journal_append_on.set(attempt);
    }

    /// Installs one current verified checkpoint.
    fn set_checkpoint(&self, checkpoint: HarnessCheckpointRecordDto) {
        self.checkpoints.borrow_mut().push(checkpoint);
    }

    /// Installs the denial one atomic launch transaction observes.
    fn deny_launch(&self, code: &'static str) {
        *self.launch_denial.borrow_mut() = Some(code);
    }

    /// Returns the current verified checkpoint, if any.
    fn current_checkpoint(&self) -> Option<HarnessCheckpointRecordDto> {
        self.checkpoints
            .borrow()
            .iter()
            .find(|checkpoint| checkpoint.is_current)
            .cloned()
    }

    /// Returns the durable journal record kinds in order.
    fn journal_kinds(&self) -> Vec<HarnessJournalRecordKindDto> {
        self.journal
            .borrow()
            .iter()
            .map(|record| record.record_kind)
            .collect()
    }
}

impl HarnessRuleRepositoryDto for FakeHarness {
    fn create_harness_rule(
        &self,
        input: CreateHarnessRuleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        self.record("rule-create");
        input.validate()?;
        *self.rule.borrow_mut() = input.rule.clone();
        *self.revision.borrow_mut() = input.revision;
        Ok(input.rule)
    }

    fn load_harness_rule(&self, harness_id: String) -> DtoResult<HarnessRuleRecordDto> {
        self.record("rule-read");
        let rule = self.rule.borrow().clone();
        if rule.harness_id != harness_id {
            return Err(ErrorDto::validation(
                "harness_not_active",
                "the durable harness rule is unknown",
            ));
        }
        rule.validate()?;
        Ok(rule)
    }

    fn load_harness_rule_revision(
        &self,
        harness_id: String,
        revision: u64,
    ) -> DtoResult<HarnessRuleRevisionRecordDto> {
        self.record("revision-read");
        let revision_record = self.revision.borrow().clone();
        if revision_record.harness_id != harness_id || revision_record.revision != revision {
            return Err(ErrorDto::validation(
                "harness_revision_conflict",
                "the exact rule revision is absent",
            ));
        }
        revision_record.validate()?;
        Ok(revision_record)
    }

    fn revise_harness_rule(
        &self,
        input: ReviseHarnessRuleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        self.record("rule-revise");
        input.validate()?;
        *self.revision.borrow_mut() = input.revision;
        self.rule.borrow_mut().active_revision = input.expected_revision + 1;
        Ok(self.rule.borrow().clone())
    }

    fn transition_harness_rule_lifecycle(
        &self,
        input: TransitionHarnessRuleLifecycleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        self.record("rule-transition");
        let state = match input.operation {
            HarnessRuleOperationDto::Pause => HarnessRuleLifecycleStateDto::Paused,
            HarnessRuleOperationDto::Resume => HarnessRuleLifecycleStateDto::Active,
            HarnessRuleOperationDto::Archive => HarnessRuleLifecycleStateDto::Archived,
            HarnessRuleOperationDto::UpdateRevision
            | HarnessRuleOperationDto::ExplicitLaunch
            | HarnessRuleOperationDto::CancelActiveRun => self.rule.borrow().lifecycle_state,
        };
        self.rule.borrow_mut().lifecycle_state = state;
        Ok(self.rule.borrow().clone())
    }

    fn count_harness_rules(&self, _project_id: String) -> DtoResult<u64> {
        self.record("rule-count");
        Ok(1)
    }
}

impl HarnessTriggerRepositoryDto for FakeHarness {
    fn capture_harness_trigger(
        &self,
        input: CaptureHarnessTriggerInputDto,
    ) -> DtoResult<HarnessTriggerCaptureRecordDto> {
        self.record(format!(
            "capture:{}:{}:{}",
            input.reason_id, input.catch_up_missed_slots, input.applied_time_zone
        ));
        let mut reason = self.reason.borrow_mut();
        if let Some(existing) = reason.as_ref() {
            if existing.reason_id == input.reason_id {
                return Ok(HarnessTriggerCaptureRecordDto {
                    outcome: HarnessTriggerCaptureOutcomeDto::Redelivered,
                    reason: existing.clone(),
                });
            }
            let mut references = existing.bounded_references.clone();
            for reference in &input.bounded_references {
                if !references.contains(reference) {
                    references.push(reference.clone());
                }
            }
            let updated = HarnessTriggerReasonRecordDto {
                first_observed_at_ms: existing.first_observed_at_ms.min(input.observed_at_ms),
                last_observed_at_ms: existing.last_observed_at_ms.max(input.observed_at_ms),
                coalesced_count: if input.catch_up_missed_slots > 0 {
                    existing.coalesced_count + input.catch_up_missed_slots
                } else {
                    existing.coalesced_count + 1
                },
                bounded_references: references,
                ..existing.clone()
            };
            updated.validate()?;
            let outcome = if input.catch_up_missed_slots > 0 {
                HarnessTriggerCaptureOutcomeDto::CatchUp
            } else {
                HarnessTriggerCaptureOutcomeDto::Coalesced
            };
            *reason = Some(updated.clone());
            return Ok(HarnessTriggerCaptureRecordDto {
                outcome,
                reason: updated,
            });
        }
        let created = HarnessTriggerReasonRecordDto {
            reason_id: input.reason_id,
            harness_id: input.harness_id,
            source_kind: input.source_kind,
            rule_revision: input.rule_revision,
            first_observed_at_ms: input.observed_at_ms,
            last_observed_at_ms: input.observed_at_ms,
            coalesced_count: if input.catch_up_missed_slots > 0 {
                input.catch_up_missed_slots
            } else {
                1
            },
            applied_time_zone: input.applied_time_zone,
            cause_chain_reference: input.cause_chain_reference,
            bounded_references: input.bounded_references,
            state: HarnessTriggerReasonStateDto::Pending,
        };
        created.validate()?;
        let outcome = if input.catch_up_missed_slots > 0 {
            HarnessTriggerCaptureOutcomeDto::CatchUp
        } else {
            HarnessTriggerCaptureOutcomeDto::Captured
        };
        *reason = Some(created.clone());
        Ok(HarnessTriggerCaptureRecordDto {
            outcome,
            reason: created,
        })
    }

    fn load_pending_harness_trigger(
        &self,
        harness_id: String,
    ) -> DtoResult<Option<HarnessTriggerReasonRecordDto>> {
        self.record("pending-read");
        let pending = self.reason.borrow().clone();
        Ok(pending.filter(|reason| {
            reason.harness_id == harness_id && reason.state == HarnessTriggerReasonStateDto::Pending
        }))
    }

    fn load_harness_trigger_reason(
        &self,
        reason_id: String,
    ) -> DtoResult<HarnessTriggerReasonRecordDto> {
        self.record("reason-read");
        self.reason
            .borrow()
            .clone()
            .filter(|reason| reason.reason_id == reason_id)
            .ok_or_else(|| {
                ErrorDto::validation(
                    "harness_source_unavailable",
                    "the durable trigger reason is absent",
                )
            })
    }

    fn record_harness_launch(
        &self,
        input: RecordHarnessLaunchInputDto,
    ) -> DtoResult<RecordHarnessLaunchOutcomeDto> {
        self.record(format!("launch:{}", input.reason_id));
        if let Some(code) = *self.launch_denial.borrow() {
            return Err(ErrorDto::validation(
                code,
                "the durable launch transaction denied the launch",
            ));
        }
        let mut reason = self.reason.borrow_mut();
        let Some(existing) = reason.clone() else {
            return Err(ErrorDto::validation(
                "harness_source_unavailable",
                "the durable trigger reason is absent",
            ));
        };
        if existing.reason_id != input.reason_id
            || existing.state != HarnessTriggerReasonStateDto::Pending
        {
            return Err(ErrorDto::validation(
                "harness_source_unavailable",
                "no matching pending reason exists",
            ));
        }
        let mut counters = self.counters.borrow_mut();
        intention_domain::harness::validate_harness_cause_depth(input.cause_chain_depth)?;
        intention_domain::harness::validate_harness_total_launches(counters.total_launches + 1)?;
        intention_domain::harness::validate_harness_concurrency_count(
            counters.concurrent_non_terminal + 1,
        )?;
        if input.direct_successor {
            intention_domain::harness::validate_harness_successor_count(
                counters.direct_successors + 1,
            )?;
        }
        counters.cause_chain_depth = input.cause_chain_depth;
        counters.concurrent_non_terminal += 1;
        counters.total_launches += 1;
        if input.direct_successor {
            counters.direct_successors += 1;
        }
        let admitted = HarnessTriggerReasonRecordDto {
            state: HarnessTriggerReasonStateDto::Admitted,
            ..existing
        };
        *reason = Some(admitted.clone());
        Ok(RecordHarnessLaunchOutcomeDto {
            reason: admitted,
            counters: *counters,
        })
    }

    fn load_harness_counters(&self, _harness_id: String) -> DtoResult<HarnessCounterRecordDto> {
        self.record("counters-read");
        Ok(*self.counters.borrow())
    }
}

impl HarnessCheckpointRepositoryDto for FakeHarness {
    fn store_harness_dossier(
        &self,
        input: HarnessDossierRecordDto,
    ) -> DtoResult<HarnessDossierRecordDto> {
        self.record(format!("dossier-store:{}", input.dossier_id));
        input.validate()?;
        let mut dossiers = self.dossiers.borrow_mut();
        if let Some(stored) = dossiers
            .iter()
            .find(|stored| stored.dossier_id == input.dossier_id)
        {
            if *stored != input {
                return Err(ErrorDto::validation(
                    "harness_source_unavailable",
                    "the dossier identity is already bound to different content",
                ));
            }
            return Ok(stored.clone());
        }
        dossiers.push(input.clone());
        Ok(input)
    }

    fn load_harness_dossier(&self, dossier_id: String) -> DtoResult<HarnessDossierRecordDto> {
        self.record("dossier-read");
        self.dossiers
            .borrow()
            .iter()
            .find(|dossier| dossier.dossier_id == dossier_id)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation(
                    "harness_source_unavailable",
                    "the durable dossier is absent",
                )
            })
    }

    fn commit_harness_checkpoint(
        &self,
        input: CommitHarnessCheckpointInputDto,
    ) -> DtoResult<CommitHarnessCheckpointOutcomeDto> {
        self.record(format!("checkpoint-commit:{}", input.run_outcome.name()));
        if self.fail_checkpoint_commit.get() {
            return Err(ErrorDto::validation(
                "harness_checkpoint_unavailable",
                "the durable checkpoint commit failed",
            ));
        }
        let mut checkpoints = self.checkpoints.borrow_mut();
        let current = checkpoints
            .iter()
            .find(|checkpoint| checkpoint.is_current)
            .cloned();
        if input.run_outcome != HarnessRunOutcomeDto::Completed {
            return Ok(CommitHarnessCheckpointOutcomeDto {
                disposition: HarnessCheckpointDispositionDto::RetainedPrevious,
                current,
            });
        }
        let Some(candidate) = input.candidate else {
            return Err(ErrorDto::validation(
                "harness_checkpoint_unavailable",
                "a completed run supplies its completely validated checkpoint",
            ));
        };
        for checkpoint in checkpoints.iter_mut() {
            checkpoint.is_current = false;
        }
        checkpoints.push(candidate.clone());
        Ok(CommitHarnessCheckpointOutcomeDto {
            disposition: HarnessCheckpointDispositionDto::Replaced,
            current: Some(candidate),
        })
    }

    fn load_current_harness_checkpoint(
        &self,
        _harness_id: String,
    ) -> DtoResult<Option<HarnessCheckpointRecordDto>> {
        self.record("checkpoint-read");
        Ok(self
            .checkpoints
            .borrow()
            .iter()
            .find(|checkpoint| checkpoint.is_current)
            .cloned())
    }

    fn append_harness_journal_record(
        &self,
        input: AppendHarnessJournalRecordInputDto,
    ) -> DtoResult<HarnessJournalRecordDto> {
        self.record(format!("journal-append:{}", input.record_kind.name()));
        let attempt = self.journal_append_attempts.get() + 1;
        self.journal_append_attempts.set(attempt);
        if self.fail_journal_append.get() || self.fail_journal_append_on.get() == attempt {
            return Err(ErrorDto::unavailable(
                "harness_journal_unavailable",
                "the durable journal append failed",
            ));
        }
        let mut journal = self.journal.borrow_mut();
        let record = HarnessJournalRecordDto {
            harness_id: input.harness_id,
            sequence: u64::try_from(journal.len()).unwrap_or(u64::MAX) + 1,
            record_kind: input.record_kind,
            safe_summary: input.safe_summary,
            canonical_record_digest: input.canonical_record_digest,
            occurred_at_ms: input.occurred_at_ms,
        };
        record.validate()?;
        journal.push(record.clone());
        Ok(record)
    }

    fn load_harness_journal(
        &self,
        _harness_id: String,
        input: LoadHarnessJournalInputDto,
    ) -> DtoResult<Vec<HarnessJournalRecordDto>> {
        self.record("journal-read");
        if self.fail_journal_read.get() {
            return Err(ErrorDto::unavailable(
                "harness_journal_unavailable",
                "the durable journal is unreadable",
            ));
        }
        if self.hide_journal_read.get() {
            return Ok(Vec::new());
        }
        Ok(self
            .journal
            .borrow()
            .iter()
            .filter(|record| record.sequence > input.after_sequence)
            .take(usize::try_from(input.limit).unwrap_or(usize::MAX))
            .cloned()
            .collect())
    }
}

/// The deterministic publication seam of one harness runtime test.
struct FakePublisher {
    log: Rc<RefCell<Vec<String>>>,
    calls: RefCell<Vec<HarnessConclusionPublicationDto>>,
    fail: bool,
}

impl FakePublisher {
    const fn new(log: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            log,
            calls: RefCell::new(Vec::new()),
            fail: false,
        }
    }

    const fn failing(log: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            log,
            calls: RefCell::new(Vec::new()),
            fail: true,
        }
    }
}

impl HarnessPublicationPort for FakePublisher {
    fn publish_harness_conclusion(&self, input: &HarnessConclusionPublicationDto) -> DtoResult<()> {
        self.log.borrow_mut().push("publish".to_owned());
        self.calls.borrow_mut().push(input.clone());
        if self.fail {
            return Err(ErrorDto::unavailable(
                "harness_publication_unavailable",
                "the linked-activity publication failed",
            ));
        }
        Ok(())
    }
}

/// Creates the harness runtime over one shared fake repository.
const fn service(
    fake: &FakeHarness,
) -> HarnessRuntimeService<'_, FakeHarness, FakeHarness, FakeHarness> {
    HarnessRuntimeService::new(fake, fake, fake)
}

#[test]
fn capture_binds_the_live_revision_and_time_zone() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let result = service
        .capture_trigger(&capture_request(
            identity_text(11),
            HarnessSourceKindV1::FixedInterval,
            1_500,
        ))
        .expect("a fresh observation is captured");
    assert_eq!(result.outcome, HarnessTriggerCaptureOutcomeDto::Captured);
    assert_eq!(result.reason.rule_revision, 1);
    assert_eq!(result.reason.applied_time_zone, "Europe/Berlin");
    assert_eq!(result.reason.coalesced_count, 1);
    assert_eq!(result.reason.state, HarnessTriggerReasonStateDto::Pending);
    let journal = result
        .journal_record
        .expect("a changed capture writes its journal record");
    assert_eq!(
        journal.record_kind,
        HarnessJournalRecordKindDto::TriggerCaptured
    );
    assert_eq!(journal.sequence, 1);
    assert!(
        journal
            .safe_summary
            .starts_with("harness trigger fixed_interval captured")
    );
    assert_eq!(fake.journal_kinds().len(), 1);
    assert!(
        fake.calls()
            .iter()
            .any(|call| call.starts_with("capture:") && call.ends_with(":0:Europe/Berlin"))
    );
}

#[test]
fn coalesced_capture_keeps_the_one_pending_identity() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let result = service
        .capture_trigger(&capture_request(
            identity_text(13),
            HarnessSourceKindV1::FixedInterval,
            2_000,
        ))
        .expect("a later observation coalesces");
    assert_eq!(result.outcome, HarnessTriggerCaptureOutcomeDto::Coalesced);
    assert_eq!(result.reason.reason_id, identity_text(11));
    assert_eq!(result.reason.coalesced_count, 2);
    assert_eq!(result.reason.first_observed_at_ms, 1_000);
    assert_eq!(result.reason.last_observed_at_ms, 2_000);
    assert!(result.journal_record.is_some());
}

#[test]
fn redelivered_capture_writes_nothing() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let result = service
        .capture_trigger(&capture_request(
            identity_text(11),
            HarnessSourceKindV1::FixedInterval,
            1_000,
        ))
        .expect("a redelivered observation is accepted");
    assert_eq!(result.outcome, HarnessTriggerCaptureOutcomeDto::Redelivered);
    assert!(result.journal_record.is_none());
    assert_eq!(result.reason.coalesced_count, 1);
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn catch_up_requires_at_least_one_missed_slot() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let mut request = capture_request(identity_text(11), HarnessSourceKindV1::FixedInterval, 5_000);
    request.catch_up = Some(HarnessCatchUpObservationDto { missed_slots: 0 });
    let error = service
        .capture_trigger(&request)
        .expect_err("an empty catch-up is incoherent");
    assert_eq!(error.code(), "harness_schedule_invalid");
    assert!(!fake.calls().iter().any(|call| call.starts_with("capture:")));

    request.catch_up = Some(HarnessCatchUpObservationDto { missed_slots: 3 });
    let result = service
        .capture_trigger(&request)
        .expect("one coalesced catch-up is captured");
    assert_eq!(result.outcome, HarnessTriggerCaptureOutcomeDto::CatchUp);
    assert_eq!(result.reason.coalesced_count, 3);
}

#[test]
fn capture_rejects_an_unbounded_reference_list() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let mut request = capture_request(identity_text(11), HarnessSourceKindV1::FixedInterval, 1_000);
    request.bounded_references = (20_u8..85).map(identity_text).collect();
    assert_eq!(
        request.bounded_references.len(),
        MAX_HARNESS_TRIGGER_REFERENCES + 1
    );
    let error = service
        .capture_trigger(&request)
        .expect_err("an over-bound reference list is rejected");
    assert_eq!(error.code(), "harness_source_limit_exceeded");
}

#[test]
fn capture_rejects_a_non_identity_reason() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let request = capture_request(
        "not-an-identity".to_owned(),
        HarnessSourceKindV1::FixedInterval,
        1_000,
    );
    let error = service
        .capture_trigger(&request)
        .expect_err("a foreign reason identity is rejected");
    assert_eq!(error.code(), "harness_source_unavailable");
}

#[test]
fn admission_consumes_the_reason_and_stores_bounded_evidence() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let outcome = service
        .admit_pending_trigger(&launch_request(
            vec!["read".to_owned(), "grep".to_owned()],
            None,
            None,
        ))
        .expect("an automatic launch is admitted");
    let admitted = admitted(outcome);
    assert_eq!(admitted.harness_id, identity_text(1));
    assert_eq!(admitted.run_id, identity_text(21));
    assert_eq!(admitted.rule_revision, 1);
    assert_eq!(
        admitted.reason.state,
        HarnessTriggerReasonStateDto::Admitted
    );
    assert_eq!(admitted.counters.total_launches, 1);
    assert_eq!(admitted.counters.concurrent_non_terminal, 1);
    assert_eq!(admitted.counters.cause_chain_depth, 1);
    assert_eq!(
        admitted.class_resolution.class,
        HarnessExecutionClassV1::Light
    );
    assert_eq!(
        admitted.class_resolution.narrowed_tool_ids,
        vec!["read".to_owned(), "grep".to_owned()]
    );
    assert!(admitted.corridor_reference.is_none());
    assert!(admitted.goal_continuation.is_none());
    assert_eq!(admitted.dossier.reason_id, identity_text(11));
    assert_eq!(admitted.dossier.rule_revision, 1);
    assert_eq!(admitted.dossier.task_digest, digest_text(4));
    assert_eq!(admitted.dossier.source_references, vec![identity_text(5)]);
    assert_eq!(admitted.dossier.typed_references, vec![identity_text(12)]);
    assert!(admitted.dossier.checkpoint_reference.is_none());
    assert_eq!(admitted.dossier.dossier_bytes, 2_048);
    assert_eq!(admitted.dossier.canonical_dossier_digest.len(), 71);
    assert_eq!(
        admitted.journal_record.record_kind,
        HarnessJournalRecordKindDto::LaunchAdmitted
    );
    assert_eq!(fake.dossiers.borrow().len(), 1);
    assert!(fake.current_checkpoint().is_none());
    assert_eq!(
        fake.pending()
            .expect("the consumed reason stays durable")
            .state,
        HarnessTriggerReasonStateDto::Admitted
    );
    assert!(!format!("{admitted:?}").contains("sk-harness-fixture"));
}

#[test]
fn admission_retains_the_reason_without_rule_capacity() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    fake.set_counters(HarnessCounterRecordDto {
        concurrent_non_terminal: 1,
        ..counters()
    });
    let service = service(&fake);
    let outcome = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect("no rule capacity retains the reason");
    let retained = retained(outcome);
    assert_eq!(retained.reason.state, HarnessTriggerReasonStateDto::Pending);
    assert_eq!(
        retained.journal_record.record_kind,
        HarnessJournalRecordKindDto::LaunchRetained
    );
    assert_eq!(
        fake.pending().expect("the reason stays pending").state,
        HarnessTriggerReasonStateDto::Pending
    );
    assert!(fake.dossiers.borrow().is_empty());
}

#[test]
fn admission_retains_while_the_daemon_concurrency_slot_is_taken() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let mut request = launch_request(vec!["read".to_owned()], None, None);
    request.daemon_concurrency_available = false;
    let outcome = service
        .admit_pending_trigger(&request)
        .expect("a full daemon retains the reason");
    assert_eq!(
        retained(outcome).journal_record.record_kind,
        HarnessJournalRecordKindDto::LaunchRetained
    );
    assert_eq!(
        fake.pending().expect("the reason stays pending").state,
        HarnessTriggerReasonStateDto::Pending
    );
}

#[test]
fn admission_rejects_a_paused_automatic_source() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    fake.set_lifecycle(HarnessRuleLifecycleStateDto::Paused);
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("a paused rule admits no automatic launch");
    assert_eq!(error.code(), "harness_not_active");
    assert!(fake.journal_kinds().is_empty());
    assert_eq!(
        fake.pending().expect("the reason stays pending").state,
        HarnessTriggerReasonStateDto::Pending
    );

    let mut explicit = launch_request(vec!["read".to_owned()], None, None);
    explicit.origin = HarnessLaunchOriginV1::ExplicitUserLaunch;
    let outcome = service
        .admit_pending_trigger(&explicit)
        .expect("an explicit user launch stays allowed while paused");
    assert_eq!(admitted(outcome).counters.concurrent_non_terminal, 1);
}

#[test]
fn admission_rejects_an_archived_rule() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    fake.set_lifecycle(HarnessRuleLifecycleStateDto::Archived);
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("an archived rule admits no launch");
    assert_eq!(error.code(), "harness_archived");
    assert!(fake.journal_kinds().is_empty());
    assert!(fake.pending().is_some());
}

#[test]
fn admission_rejects_sub_agent_without_a_corridor() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(
            vec!["read".to_owned(), "sub_agent".to_owned()],
            None,
            None,
        ))
        .expect_err("sub_agent needs the corridor");
    assert_eq!(error.code(), "invalid_harness_class_resolution");
    assert!(fake.pending().is_some());
}

#[test]
fn admission_admits_sub_agent_through_its_corridor() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let corridor = HarnessSubAgentCorridorSelectionDto {
        corridor_reference: identity(31),
        permitted_class: HarnessExecutionClassV1::Light,
        permitted_depth: 2,
        permitted_child_count: 4,
    };
    let outcome = service
        .admit_pending_trigger(&launch_request(
            vec!["read".to_owned(), "sub_agent".to_owned()],
            Some(corridor),
            None,
        ))
        .expect("the corridor admits sub_agent");
    assert_eq!(admitted(outcome).corridor_reference, Some(identity(31)));
}

#[test]
fn admission_rejects_a_tool_outside_the_read_and_delegate_set() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(
            vec!["read".to_owned(), "write".to_owned()],
            None,
            None,
        ))
        .expect_err("a write tool is outside the harness boundary");
    assert_eq!(error.code(), "invalid_harness_class_resolution");
}

#[test]
fn admission_rejects_an_over_deep_cause_chain() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let mut request = launch_request(vec!["read".to_owned()], None, None);
    request.cause_chain_depth = 9;
    let error = service
        .admit_pending_trigger(&request)
        .expect_err("the cause chain stays inside its bound");
    assert_eq!(error.code(), "harness_cause_chain_limit_exceeded");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn admission_rejects_an_oversized_dossier_before_any_launch() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let mut request = launch_request(vec!["read".to_owned()], None, None);
    request.dossier_bytes = MAX_FILE_BYTES + 1;
    let error = service
        .admit_pending_trigger(&request)
        .expect_err("an oversized dossier is rejected rather than truncated");
    assert_eq!(error.code(), "harness_dossier_too_large");
    assert!(fake.dossiers.borrow().is_empty());
    assert!(fake.pending().is_some());
}

#[test]
fn admission_propagates_a_durable_concurrency_denial() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    fake.deny_launch("harness_concurrency_limit_exceeded");
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("the atomic transaction denies the concurrent launch");
    assert_eq!(error.code(), "harness_concurrency_limit_exceeded");
    assert_eq!(
        fake.pending().expect("the reason stays pending").state,
        HarnessTriggerReasonStateDto::Pending
    );
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn admission_rejects_a_missing_pending_reason() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("a rule admits only its single pending reason");
    assert_eq!(error.code(), "harness_source_unavailable");
}

#[test]
fn a_goal_directed_revision_requires_and_plans_its_goal() {
    let fake = FakeHarness::new();
    fake.set_task_mode(HarnessTaskModeDto::GoalDirected);
    fake.set_pending(default_pending());
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("goal mode needs one active goal");
    assert_eq!(error.code(), "harness_not_active");

    let goal = HarnessGoalReferenceDto {
        goal_id: identity(41),
        goal_revision: 7,
    };
    let outcome = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, Some(goal)))
        .expect("the goal continuation is admitted separately");
    let admitted = admitted(outcome);
    let continuation = admitted
        .goal_continuation
        .expect("a goal-directed launch carries its continuation");
    assert_eq!(continuation.goal_id, identity(41));
    assert_eq!(continuation.goal_revision, 7);
    assert_eq!(continuation.rule_revision, 1);
    assert_eq!(continuation.launch_reference, admitted.launch_reference);
}

#[test]
fn a_repeated_task_revision_rejects_a_goal() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);
    let goal = HarnessGoalReferenceDto {
        goal_id: identity(41),
        goal_revision: 1,
    };
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, Some(goal)))
        .expect_err("a repeated-task revision continues no goal");
    assert_eq!(error.code(), "invalid_harness_launch_request");
}

#[test]
fn a_completed_run_replaces_the_checkpoint_and_publishes_after_reread() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let fake = FakeHarness::with_log(Rc::clone(&log));
    fake.set_checkpoint(checkpoint(1));
    let publisher = FakePublisher::new(Rc::clone(&log));
    let service = service(&fake);
    let terminal = service
        .commit_run_outcome_with_publication(
            &run_outcome_input(
                HarnessRunOutcomeV1::Completed,
                Some(candidate(2)),
                Some(conclusion()),
            ),
            &publisher,
        )
        .expect("a successful run replaces the verified checkpoint");
    assert_eq!(terminal.outcome, HarnessRunOutcomeV1::Completed);
    assert_eq!(
        terminal.checkpoint_disposition,
        HarnessCheckpointDispositionDto::Replaced
    );
    assert_eq!(
        terminal
            .current_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.checkpoint_revision),
        Some(2)
    );
    let conclusion = terminal.conclusion.expect("the conclusion is recorded");
    assert_eq!(
        conclusion.presentation_mode,
        HarnessPresentationModeDto::JournalAndActivityEntry
    );
    assert!(terminal.published);
    assert_eq!(
        terminal
            .journal_records
            .iter()
            .map(|record| record.record_kind)
            .collect::<Vec<_>>(),
        vec![
            HarnessJournalRecordKindDto::CheckpointAccepted,
            HarnessJournalRecordKindDto::RunTerminal,
            HarnessJournalRecordKindDto::ConclusionPublished,
        ]
    );
    assert_eq!(publisher.calls.borrow().len(), 1);
    assert_eq!(publisher.calls.borrow()[0].journal_sequence, 2);
    assert_eq!(publisher.calls.borrow()[0].conclusion_bytes, 1_024);
    let calls = fake.calls();
    let reread = calls
        .iter()
        .position(|call| call == "journal-read")
        .expect("the publication re-reads the durable terminal record");
    let publish = calls
        .iter()
        .position(|call| call == "publish")
        .expect("the conclusion is published");
    assert!(reread < publish);
    assert!(
        calls
            .iter()
            .rposition(|call| call == "checkpoint-read")
            .expect("the publication re-reads the current checkpoint")
            < publish
    );
}

#[test]
fn journal_only_presentation_keeps_the_conclusion_in_the_journal() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let fake = FakeHarness::with_log(Rc::clone(&log));
    fake.set_presentation_mode(HarnessPresentationModeDto::JournalOnly);
    let publisher = FakePublisher::new(Rc::clone(&log));
    let service = service(&fake);
    let terminal = service
        .commit_run_outcome_with_publication(
            &run_outcome_input(
                HarnessRunOutcomeV1::Completed,
                Some(candidate(1)),
                Some(conclusion()),
            ),
            &publisher,
        )
        .expect("a journal-only conclusion commits");
    assert!(terminal.published);
    assert_eq!(publisher.calls.borrow().len(), 0);
    assert_eq!(
        terminal
            .journal_records
            .iter()
            .map(|record| record.record_kind)
            .collect::<Vec<_>>(),
        vec![
            HarnessJournalRecordKindDto::CheckpointAccepted,
            HarnessJournalRecordKindDto::RunTerminal,
            HarnessJournalRecordKindDto::ConclusionPublished,
        ]
    );
}

#[test]
fn a_failed_run_retains_the_previous_checkpoint() {
    let fake = FakeHarness::new();
    fake.set_checkpoint(checkpoint(1));
    let service = service(&fake);
    let terminal = service
        .commit_run_outcome(&run_outcome_input(HarnessRunOutcomeV1::Failed, None, None))
        .expect("a failed run retains the previous verified checkpoint");
    assert_eq!(
        terminal.checkpoint_disposition,
        HarnessCheckpointDispositionDto::RetainedPrevious
    );
    assert_eq!(
        terminal
            .current_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.checkpoint_revision),
        Some(1)
    );
    assert!(terminal.conclusion.is_none());
    assert!(!terminal.published);
    assert_eq!(
        terminal
            .journal_records
            .iter()
            .map(|record| record.record_kind)
            .collect::<Vec<_>>(),
        vec![
            HarnessJournalRecordKindDto::CheckpointRetained,
            HarnessJournalRecordKindDto::RunTerminal,
        ]
    );
}

#[test]
fn a_completed_run_without_a_validated_candidate_fails_closed() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let error = service
        .commit_run_outcome(&run_outcome_input(
            HarnessRunOutcomeV1::Completed,
            None,
            None,
        ))
        .expect_err("a completed run supplies its completely validated checkpoint");
    assert_eq!(error.code(), "harness_checkpoint_unavailable");
    assert!(fake.journal_kinds().is_empty());
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| call.starts_with("checkpoint-commit"))
    );
}

#[test]
fn a_non_successful_run_cannot_supply_a_candidate() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let error = service
        .commit_run_outcome(&run_outcome_input(
            HarnessRunOutcomeV1::Cancelled,
            Some(candidate(1)),
            None,
        ))
        .expect_err("only a successful run supplies a candidate");
    assert_eq!(error.code(), "invalid_harness_run_outcome");
}

#[test]
fn oversized_checkpoint_and_conclusion_bounds_fail_closed() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let oversized = HarnessCheckpointCandidateDto {
        checkpoint_bytes: MAX_FILE_BYTES + 1,
        ..candidate(1)
    };
    let error = service
        .commit_run_outcome(&run_outcome_input(
            HarnessRunOutcomeV1::Completed,
            Some(oversized),
            None,
        ))
        .expect_err("an oversized checkpoint is rejected rather than truncated");
    assert_eq!(error.code(), "harness_checkpoint_too_large");

    let error = service
        .commit_run_outcome(&run_outcome_input(
            HarnessRunOutcomeV1::Completed,
            Some(candidate(1)),
            Some(HarnessConclusionInputDto {
                conclusion_bytes: MAX_FILE_BYTES + 1,
                ..conclusion()
            }),
        ))
        .expect_err("an oversized conclusion is rejected rather than truncated");
    assert_eq!(error.code(), "harness_result_too_large");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn restart_interrupts_the_run_without_resuming_it() {
    let fake = FakeHarness::new();
    fake.set_checkpoint(checkpoint(1));
    let service = service(&fake);
    let recovery = service
        .recover_interrupted(&HarnessRestartRecoveryInputDto {
            harness_id: identity(1),
            interrupted_run_id: identity(51),
            observed_contract: HarnessDisconnectContractV1::frozen(),
            occurred_at_ms: 9_000,
        })
        .expect("a restart leaves the unfinished run interrupted");
    assert_eq!(recovery.terminal.outcome, HarnessRunOutcomeV1::Interrupted);
    assert_eq!(
        recovery.terminal.checkpoint_disposition,
        HarnessCheckpointDispositionDto::RetainedPrevious
    );
    assert_eq!(recovery.terminal.producing_run_id, identity_text(51));
    assert_eq!(recovery.contract, HarnessDisconnectContractV1::frozen());
    assert!(!recovery.external_work_resumes);
    assert!(recovery.requires_separate_admission);
    assert!(recovery.terminal.conclusion.is_none());
    assert_eq!(
        recovery
            .terminal
            .journal_records
            .iter()
            .map(|record| record.record_kind)
            .collect::<Vec<_>>(),
        vec![
            HarnessJournalRecordKindDto::CheckpointRetained,
            HarnessJournalRecordKindDto::RunTerminal,
        ]
    );
}

#[test]
fn restart_rejects_a_contract_that_resumes_old_work() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let error = service
        .recover_interrupted(&HarnessRestartRecoveryInputDto {
            harness_id: identity(1),
            interrupted_run_id: identity(51),
            observed_contract: HarnessDisconnectContractV1::new(true, true, true, true),
            occurred_at_ms: 9_000,
        })
        .expect_err("post-disconnect work never resumes old external work");
    assert_eq!(error.code(), "invalid_harness_disconnect_contract");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn cancellation_cascades_through_the_descendant_subtree() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let cancellation = service
        .cancel_active_run(&HarnessCancellationInputDto {
            harness_id: identity_text(1),
            producing_run_id: identity_text(21),
            has_active_run: true,
            cascaded_run_ids: vec![identity_text(21), identity_text(22), identity_text(23)],
            occurred_at_ms: 7_000,
        })
        .expect("the cancellation cascade commits");
    assert_eq!(cancellation.cancelled_run_id, identity_text(21));
    assert_eq!(cancellation.cascaded_run_count, 3);
    assert_eq!(
        cancellation.terminal.outcome,
        HarnessRunOutcomeV1::Cancelled
    );
    assert!(!cancellation.external_work_resumes);
    assert!(cancellation.later_attempt_requires_separate_admission);
    assert_eq!(
        cancellation
            .terminal
            .journal_records
            .iter()
            .map(|record| record.record_kind)
            .collect::<Vec<_>>(),
        vec![
            HarnessJournalRecordKindDto::CheckpointRetained,
            HarnessJournalRecordKindDto::RunTerminal,
        ]
    );
}

#[test]
fn cancellation_rejects_an_incoherent_cascade() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let mut input = HarnessCancellationInputDto {
        harness_id: identity_text(1),
        producing_run_id: identity_text(21),
        has_active_run: true,
        cascaded_run_ids: vec![identity_text(22)],
        occurred_at_ms: 7_000,
    };
    let error = service
        .cancel_active_run(&input)
        .expect_err("a cascade contains its direct run");
    assert_eq!(error.code(), "invalid_harness_launch_request");

    input.cascaded_run_ids = vec![identity_text(21), identity_text(21)];
    let error = service
        .cancel_active_run(&input)
        .expect_err("a cascade repeats no run identity");
    assert_eq!(error.code(), "invalid_harness_launch_request");

    input.cascaded_run_ids = (40_u8..57).map(identity_text).collect();
    let error = service
        .cancel_active_run(&input)
        .expect_err("a cascade stays inside its subtree bound");
    assert_eq!(error.code(), "invalid_harness_launch_request");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn cancellation_requires_an_active_run() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let error = service
        .cancel_active_run(&HarnessCancellationInputDto {
            harness_id: identity_text(1),
            producing_run_id: identity_text(21),
            has_active_run: false,
            cascaded_run_ids: vec![identity_text(21)],
            occurred_at_ms: 7_000,
        })
        .expect_err("cancellation needs the ordinary active-run state");
    assert_eq!(error.code(), "harness_not_active");
}

#[test]
fn journal_pages_stay_inside_the_code_owned_bound() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let error = service
        .load_journal(&identity_text(1), 0, 0)
        .expect_err("a zero-sized page is unusable");
    assert_eq!(error.code(), "invalid_harness_journal_page");
    let error = service
        .load_journal(&identity_text(1), 0, MAX_HARNESS_JOURNAL_PAGE + 1)
        .expect_err("an over-bound page is unusable");
    assert_eq!(error.code(), "invalid_harness_journal_page");

    service
        .capture_trigger(&capture_request(
            identity_text(11),
            HarnessSourceKindV1::FixedInterval,
            1_000,
        ))
        .expect("the capture writes one journal record");
    let page = service
        .load_journal(&identity_text(1), 0, MAX_HARNESS_JOURNAL_PAGE)
        .expect("the journal is readable after the durable commit");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].sequence, 1);
    assert!(
        service
            .load_journal(&identity_text(1), 1, MAX_HARNESS_JOURNAL_PAGE)
            .expect("a later page is readable")
            .is_empty()
    );
}

#[test]
fn a_missing_dossier_read_is_source_unavailable() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let error = service
        .load_dossier(&identity_text(71))
        .expect_err("an absent dossier is a source failure");
    assert_eq!(error.code(), "harness_source_unavailable");
}

#[test]
fn publication_failure_is_surfaced_without_the_published_record() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let fake = FakeHarness::with_log(Rc::clone(&log));
    let publisher = FakePublisher::failing(Rc::clone(&log));
    let service = service(&fake);
    let error = service
        .commit_run_outcome_with_publication(
            &run_outcome_input(
                HarnessRunOutcomeV1::Completed,
                Some(candidate(1)),
                Some(conclusion()),
            ),
            &publisher,
        )
        .expect_err("a publication failure is surfaced");
    assert_eq!(error.code(), "harness_publication_unavailable");
    assert_eq!(publisher.calls.borrow().len(), 1);
    assert_eq!(
        fake.journal_kinds(),
        vec![
            HarnessJournalRecordKindDto::CheckpointAccepted,
            HarnessJournalRecordKindDto::RunTerminal,
        ]
    );
}

#[test]
fn a_missing_durable_reread_blocks_publication() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let fake = FakeHarness::with_log(Rc::clone(&log));
    fake.fail_journal_read.set(true);
    let publisher = FakePublisher::new(Rc::clone(&log));
    let service = service(&fake);
    let error = service
        .commit_run_outcome_with_publication(
            &run_outcome_input(
                HarnessRunOutcomeV1::Completed,
                Some(candidate(1)),
                Some(conclusion()),
            ),
            &publisher,
        )
        .expect_err("publication needs the independent durable reread");
    assert_eq!(error.code(), "harness_journal_unavailable");
    assert_eq!(publisher.calls.borrow().len(), 0);
    assert_eq!(
        fake.journal_kinds(),
        vec![
            HarnessJournalRecordKindDto::CheckpointAccepted,
            HarnessJournalRecordKindDto::RunTerminal,
        ]
    );
}

#[test]
fn a_journal_append_failure_stops_the_run_terminal_commit() {
    let fake = FakeHarness::new();
    fake.fail_journal_append.set(true);
    let service = service(&fake);
    let error = service
        .commit_run_outcome(&run_outcome_input(HarnessRunOutcomeV1::Failed, None, None))
        .expect_err("an unreadable journal surfaces its typed failure");
    assert_eq!(error.code(), "harness_journal_unavailable");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn no_service_dto_or_error_carries_credential_shaped_text() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let scan = |text: &str| {
        assert!(
            !text.contains("sk-"),
            "no service value discloses a credential"
        );
        assert!(
            !text.contains("Bearer "),
            "no service value discloses a token"
        );
        assert!(
            !text.contains("/home/"),
            "no service value discloses a path"
        );
        assert!(!text.contains("/tmp/"), "no service value discloses a path");
    };
    let capture = service
        .capture_trigger(&capture_request(
            identity_text(11),
            HarnessSourceKindV1::FixedInterval,
            1_000,
        ))
        .expect("the capture succeeds");
    scan(&format!("{capture:?}"));
    scan(
        &capture
            .journal_record
            .as_ref()
            .expect("the capture journal record exists")
            .safe_summary,
    );
    let outcome = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect("the launch is admitted");
    scan(&format!("{outcome:?}"));
    let error = service
        .load_dossier(&identity_text(71))
        .expect_err("an absent dossier fails closed");
    scan(&format!("{error:?}"));
    scan(error.message());
    scan(error.code());
}

#[test]
fn capture_merges_the_fixture_source_kinds_and_surfaces_a_merge_that_violates_its_bound() {
    for (kind, stored) in [
        (
            HarnessSourceKindV1::CalendarTime,
            HarnessSourceKindDto::CalendarTime,
        ),
        (
            HarnessSourceKindV1::FixedInterval,
            HarnessSourceKindDto::FixedInterval,
        ),
        (
            HarnessSourceKindV1::TerminalOutcomeLink,
            HarnessSourceKindDto::TerminalOutcomeLink,
        ),
    ] {
        let fake = FakeHarness::new();
        let service = service(&fake);
        let mut request = capture_request(identity_text(11), kind, 1_000);
        request.cause_chain_reference = Some(identity_text(13));
        let capture = service
            .capture_trigger(&request)
            .expect("the capture binds its closed source kind");
        assert_eq!(capture.reason.source_kind, stored);
        let outcome = service
            .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
            .expect("an active rule admits its pending reason");
        assert_eq!(admitted(outcome).reason.source_kind, stored);
    }

    let fake = FakeHarness::new();
    let mut bounded = pending_reason(identity_text(11), 1_000);
    bounded.bounded_references = (20_u8..84).map(identity_text).collect();
    assert_eq!(bounded.bounded_references.len(), 64);
    fake.set_pending(bounded);
    let service = service(&fake);
    let error = service
        .capture_trigger(&capture_request(
            identity_text(14),
            HarnessSourceKindV1::FixedInterval,
            1_100,
        ))
        .expect_err("a merged reason stays inside its bounded reference count");
    assert_eq!(error.code(), "harness_source_unavailable");
}

#[test]
fn a_capture_journal_append_failure_surfaces() {
    let fake = FakeHarness::new();
    fake.fail_journal_append.set(true);
    let service = service(&fake);
    let error = service
        .capture_trigger(&capture_request(
            identity_text(11),
            HarnessSourceKindV1::FixedInterval,
            1_000,
        ))
        .expect_err("an unreadable capture journal surfaces its typed failure");
    assert_eq!(error.code(), "harness_journal_unavailable");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn a_launch_request_outside_its_structural_bounds_fails_closed() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    let service = service(&fake);

    let mut shallow = launch_request(vec!["read".to_owned()], None, None);
    shallow.cause_chain_depth = 0;
    let error = service
        .admit_pending_trigger(&shallow)
        .expect_err("a launch measures its cause-chain depth from its cause lineage");
    assert_eq!(error.code(), "invalid_harness_launch_request");

    let wide = launch_request(vec!["read".to_owned(); 17], None, None);
    let error = service
        .admit_pending_trigger(&wide)
        .expect_err("the narrowed tool selection exceeds its bound");
    assert_eq!(error.code(), "invalid_harness_class_resolution");

    let unversioned = launch_request(
        vec!["read".to_owned()],
        None,
        Some(HarnessGoalReferenceDto {
            goal_id: identity(41),
            goal_revision: 0,
        }),
    );
    let error = service
        .admit_pending_trigger(&unversioned)
        .expect_err("a goal continuation binds one exact live goal revision");
    assert_eq!(error.code(), "harness_revision_conflict");
    assert!(fake.dossiers.borrow().is_empty());
}

#[test]
fn admission_rejects_a_full_completion_cause_bound() {
    let fake = FakeHarness::new();
    let mut successor = pending_reason(identity_text(11), 1_000);
    successor.source_kind = HarnessSourceKindDto::TerminalOutcomeLink;
    successor.cause_chain_reference = Some(identity_text(13));
    fake.set_pending(successor);
    fake.set_counters(HarnessCounterRecordDto {
        direct_successors: 16,
        ..counters()
    });
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("the completion-cause successor bound is exhausted");
    assert_eq!(error.code(), "harness_cause_chain_limit_exceeded");
    assert!(fake.dossiers.borrow().is_empty());
}

#[test]
fn the_cached_medium_and_heavy_classes_resolve_and_journal() {
    for (configured, resolved) in [
        (
            HarnessExecutionClassDto::Medium,
            HarnessExecutionClassV1::Medium,
        ),
        (
            HarnessExecutionClassDto::Heavy,
            HarnessExecutionClassV1::Heavy,
        ),
    ] {
        let fake = FakeHarness::new();
        fake.set_pending(default_pending());
        fake.set_class(configured);
        let service = service(&fake);
        let outcome = service
            .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
            .expect("the cached execution class resolves against the request");
        assert_eq!(admitted(outcome).class_resolution.class, resolved);
    }
}

#[test]
fn the_default_publication_port_retains_the_previous_checkpoint() {
    let fake = FakeHarness::new();
    fake.set_checkpoint(checkpoint(1));
    let service = service(&fake);
    let terminal = service
        .commit_run_outcome(&run_outcome_input(
            HarnessRunOutcomeV1::ExternalEffectUnknown,
            None,
            Some(conclusion()),
        ))
        .expect("the default publication port commits without an activity entry");
    assert_eq!(terminal.outcome, HarnessRunOutcomeV1::ExternalEffectUnknown);
    assert_eq!(
        terminal.checkpoint_disposition,
        HarnessCheckpointDispositionDto::RetainedPrevious
    );
    assert!(terminal.conclusion.is_some());
    assert!(terminal.published);
    assert_eq!(
        terminal
            .journal_records
            .iter()
            .map(|record| record.record_kind)
            .collect::<Vec<_>>(),
        vec![
            HarnessJournalRecordKindDto::CheckpointRetained,
            HarnessJournalRecordKindDto::RunTerminal,
            HarnessJournalRecordKindDto::ConclusionPublished,
        ]
    );
}

#[test]
fn a_non_canonical_durable_checkpoint_digest_fails_closed() {
    let fake = FakeHarness::new();
    let mut current = checkpoint(1);
    current.content_digest = "sha256:not-canonical".to_owned();
    fake.set_checkpoint(current);
    let service = service(&fake);
    let error = service
        .commit_run_outcome(&run_outcome_input(HarnessRunOutcomeV1::Failed, None, None))
        .expect_err("a durable harness digest is canonical");
    assert_eq!(error.code(), "harness_source_unavailable");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn a_zero_revision_candidate_checkpoint_fails_closed() {
    let fake = FakeHarness::new();
    let service = service(&fake);
    let error = service
        .commit_run_outcome(&run_outcome_input(
            HarnessRunOutcomeV1::Completed,
            Some(candidate(0)),
            None,
        ))
        .expect_err("a checkpoint is versioned from one");
    assert_eq!(error.code(), "harness_revision_conflict");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn a_dossier_outside_its_time_zone_fails_closed() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    fake.set_applied_time_zone("Europe Berlin");
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("the dossier keeps its canonical project time zone");
    assert_eq!(error.code(), "harness_schedule_invalid");
    assert!(fake.dossiers.borrow().is_empty());
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn a_denied_checkpoint_commit_surfaces_its_typed_failure() {
    let fake = FakeHarness::new();
    fake.fail_checkpoint_commit.set(true);
    let service = service(&fake);
    let error = service
        .commit_run_outcome(&run_outcome_input(HarnessRunOutcomeV1::Failed, None, None))
        .expect_err("the run-terminal commit surfaces its checkpoint failure");
    assert_eq!(error.code(), "harness_checkpoint_unavailable");
    assert!(fake.journal_kinds().is_empty());
}

#[test]
fn a_retained_launch_journal_failure_surfaces() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    fake.set_counters(HarnessCounterRecordDto {
        concurrent_non_terminal: 1,
        ..counters()
    });
    fake.fail_journal_append.set(true);
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("a retained launch surfaces its journal failure");
    assert_eq!(error.code(), "harness_journal_unavailable");
    assert!(fake.dossiers.borrow().is_empty());
}

#[test]
fn an_admitted_launch_journal_failure_surfaces() {
    let fake = FakeHarness::new();
    fake.set_pending(default_pending());
    fake.fail_journal_append.set(true);
    let service = service(&fake);
    let error = service
        .admit_pending_trigger(&launch_request(vec!["read".to_owned()], None, None))
        .expect_err("an admitted launch surfaces its journal failure");
    assert_eq!(error.code(), "harness_journal_unavailable");
    assert_eq!(fake.dossiers.borrow().len(), 1);
}

#[test]
fn a_run_terminal_journal_failure_surfaces_after_the_checkpoint() {
    let fake = FakeHarness::new();
    fake.fail_journal_append_on(2);
    let service = service(&fake);
    let error = service
        .commit_run_outcome(&run_outcome_input(HarnessRunOutcomeV1::Failed, None, None))
        .expect_err("the run-terminal journal append surfaces its failure");
    assert_eq!(error.code(), "harness_journal_unavailable");
    assert_eq!(
        fake.journal_kinds(),
        vec![HarnessJournalRecordKindDto::CheckpointRetained]
    );
}

#[test]
fn a_conclusion_journal_failure_surfaces_after_publication() {
    let fake = FakeHarness::new();
    fake.fail_journal_append_on(3);
    let publisher = FakePublisher::new(Rc::new(RefCell::new(Vec::new())));
    let service = service(&fake);
    let error = service
        .commit_run_outcome_with_publication(
            &run_outcome_input(
                HarnessRunOutcomeV1::Completed,
                Some(candidate(1)),
                Some(conclusion()),
            ),
            &publisher,
        )
        .expect_err("the conclusion journal append surfaces its failure");
    assert_eq!(error.code(), "harness_journal_unavailable");
    assert_eq!(publisher.calls.borrow().len(), 1);
    assert_eq!(
        fake.journal_kinds(),
        vec![
            HarnessJournalRecordKindDto::CheckpointAccepted,
            HarnessJournalRecordKindDto::RunTerminal,
        ]
    );
}

#[test]
fn a_missing_terminal_reread_blocks_publication() {
    let fake = FakeHarness::new();
    fake.hide_journal_read.set(true);
    let publisher = FakePublisher::new(Rc::new(RefCell::new(Vec::new())));
    let service = service(&fake);
    let error = service
        .commit_run_outcome_with_publication(
            &run_outcome_input(
                HarnessRunOutcomeV1::Completed,
                Some(candidate(1)),
                Some(conclusion()),
            ),
            &publisher,
        )
        .expect_err("the independent reread observes the committed terminal record");
    assert_eq!(error.code(), "harness_source_unavailable");
    assert_eq!(publisher.calls.borrow().len(), 0);
}
