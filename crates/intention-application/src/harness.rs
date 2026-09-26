//! Slice 3 harness runtime: admission, dossiers, cancellation, publication.
//!
//! Owner: architecture 26 with ADR 0044. This module owns the application
//! transactions that admit a durable trigger into a fresh ordinary run,
//! assemble the dossier and publish it after commit, cascade cancellation, and
//! expose the journal read surface. Scheduling ticks and restart recovery live
//! in `intention-daemon`.
//!
//! # Durable capture and atomic admission
//!
//! A trigger is durably captured before any admission: the frozen repository
//! transaction owns coalescing, redelivery, and catch-up, while this service
//! binds the request to the rule's live revision and applied project time zone
//! and appends the durable journal record. Admission then re-reads the rule,
//! the exact active revision, the single pending reason, and the durable
//! counters; the code-owned capacity, cause-chain, successor, and total-launch
//! bounds are checked before `record_harness_launch` consumes the reason and
//! increments the counters in one atomic transaction. Waiting for a free slot
//! retains the coalesced reason and appends a `LaunchRetained` record instead
//! of dropping it.
//!
//! # Boundary
//!
//! This service creates no `RunId`: the fresh ordinary run identity of one
//! launch is daemon-assigned and supplied by the caller, and the launch never
//! becomes a rule, dossier, or counter. Task text, transcripts, paths, grants,
//! provider resources, and process handles never cross this boundary; a
//! dossier carries bounded typed references, its byte size, and its canonical
//! digest only.
//!
//! # Publication
//!
//! A checkpoint decision and its run-terminal record commit before the safe
//! conclusion is published, and publication re-reads the durable terminal
//! journal record and the current checkpoint first. Journal-only presentation
//! publishes the journal record itself; a compact linked-activity entry is
//! published through [`HarnessPublicationPort`] and then recorded by the
//! `ConclusionPublished` journal record.

use intention_domain::canonical::Digest256;
use intention_domain::harness::{
    HarnessAdmissionCapacityV1, HarnessConclusionV1, HarnessDisconnectContractV1, HarnessDossierV1,
    HarnessLaunchOriginV1, HarnessRuleLifecycleStateV1, HarnessRuleOperationV1,
    HarnessRunOutcomeV1, HarnessSubAgentCorridorV1, HarnessTriggerAdmissionV1,
    HarnessVerifiedCheckpointV1, admit_harness_trigger, resolve_harness_class,
    validate_harness_cause_depth, validate_harness_checkpoint_bytes,
    validate_harness_class_narrowing, validate_harness_conclusion_bytes,
    validate_harness_concurrency_count, validate_harness_disconnect_contract,
    validate_harness_dossier_bytes, validate_harness_narrowed_tools,
    validate_harness_rule_operation, validate_harness_successor_count,
    validate_harness_total_launches, verified_checkpoint_after_run,
};
use intention_domain::slice3_selections::{
    HARNESS_MAX_CONCURRENT, HARNESS_MAX_NARROWED_TOOLS, HarnessClassResolutionV1,
    HarnessExecutionClassV1, HarnessSourceKindV1, HarnessTriggerReasonV1,
};
use intention_storage::harness_repo::{
    AppendHarnessJournalRecordInputDto, CaptureHarnessTriggerInputDto,
    CommitHarnessCheckpointInputDto, HarnessCheckpointDispositionDto, HarnessCheckpointRecordDto,
    HarnessCheckpointRepositoryDto, HarnessConclusionRecordDto, HarnessCounterRecordDto,
    HarnessDossierRecordDto, HarnessExecutionClassDto, HarnessJournalRecordDto,
    HarnessJournalRecordKindDto, HarnessPresentationModeDto, HarnessRuleLifecycleStateDto,
    HarnessRuleRepositoryDto, HarnessRuleRevisionRecordDto, HarnessRunOutcomeDto,
    HarnessSourceKindDto, HarnessTaskModeDto, HarnessTriggerCaptureOutcomeDto,
    HarnessTriggerCaptureRecordDto, HarnessTriggerReasonRecordDto, HarnessTriggerRepositoryDto,
    LoadHarnessJournalInputDto, MAX_HARNESS_JOURNAL_PAGE, MAX_HARNESS_TRIGGER_REFERENCES,
    RecordHarnessLaunchInputDto,
};
use intention_types::{DtoResult, ErrorDto};

use crate::programmatic_policy::{
    derived_identity, digest_bytes, digest_text, identity_bytes, identity_text, push_framed,
};

/// Builds one typed harness rejection.
fn harness_error(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::validation(code, message)
}

/// Parses one durable harness identity as canonical identity text.
///
/// # Errors
///
/// Returns `harness_source_unavailable` for a durable value that is not a
/// canonical daemon-assigned identity.
fn harness_identity(value: &str) -> DtoResult<[u8; 16]> {
    identity_bytes(value).map_err(|_| {
        harness_error(
            "harness_source_unavailable",
            "a durable harness reference is a canonical identity",
        )
    })
}

/// Parses one durable harness digest as canonical digest text.
///
/// # Errors
///
/// Returns `harness_source_unavailable` for a durable value that is not a
/// canonical `sha256:<64 lowercase hex>` digest.
fn harness_digest(value: &str) -> DtoResult<Digest256> {
    digest_bytes(value).map_err(|_| {
        harness_error(
            "harness_source_unavailable",
            "a durable harness digest is canonical",
        )
    })
}

/// Converts one durable source kind into its domain value.
const fn source_kind_from_storage(kind: HarnessSourceKindDto) -> HarnessSourceKindV1 {
    match kind {
        HarnessSourceKindDto::ExplicitUserLaunch => HarnessSourceKindV1::ExplicitUserLaunch,
        HarnessSourceKindDto::CalendarTime => HarnessSourceKindV1::CalendarTime,
        HarnessSourceKindDto::FixedInterval => HarnessSourceKindV1::FixedInterval,
        HarnessSourceKindDto::TerminalOutcomeLink => HarnessSourceKindV1::TerminalOutcomeLink,
    }
}

/// Converts one domain source kind into its durable value.
const fn source_kind_to_storage(kind: HarnessSourceKindV1) -> HarnessSourceKindDto {
    match kind {
        HarnessSourceKindV1::ExplicitUserLaunch => HarnessSourceKindDto::ExplicitUserLaunch,
        HarnessSourceKindV1::CalendarTime => HarnessSourceKindDto::CalendarTime,
        HarnessSourceKindV1::FixedInterval => HarnessSourceKindDto::FixedInterval,
        HarnessSourceKindV1::TerminalOutcomeLink => HarnessSourceKindDto::TerminalOutcomeLink,
    }
}

/// Converts one durable harness rule lifecycle state into its domain value.
const fn lifecycle_from_storage(
    state: HarnessRuleLifecycleStateDto,
) -> HarnessRuleLifecycleStateV1 {
    match state {
        HarnessRuleLifecycleStateDto::Active => HarnessRuleLifecycleStateV1::Active,
        HarnessRuleLifecycleStateDto::Paused => HarnessRuleLifecycleStateV1::Paused,
        HarnessRuleLifecycleStateDto::Archived => HarnessRuleLifecycleStateV1::Archived,
    }
}

/// Converts one durable harness execution class into its domain value.
const fn class_from_storage(class: HarnessExecutionClassDto) -> HarnessExecutionClassV1 {
    match class {
        HarnessExecutionClassDto::Light => HarnessExecutionClassV1::Light,
        HarnessExecutionClassDto::Medium => HarnessExecutionClassV1::Medium,
        HarnessExecutionClassDto::Heavy => HarnessExecutionClassV1::Heavy,
    }
}

/// Converts one domain harness execution class into its durable value.
const fn class_to_storage(class: HarnessExecutionClassV1) -> HarnessExecutionClassDto {
    match class {
        HarnessExecutionClassV1::Light => HarnessExecutionClassDto::Light,
        HarnessExecutionClassV1::Medium => HarnessExecutionClassDto::Medium,
        HarnessExecutionClassV1::Heavy => HarnessExecutionClassDto::Heavy,
    }
}

/// Converts one domain harness run outcome into its durable value.
const fn run_outcome_to_storage(outcome: HarnessRunOutcomeV1) -> HarnessRunOutcomeDto {
    match outcome {
        HarnessRunOutcomeV1::Completed => HarnessRunOutcomeDto::Completed,
        HarnessRunOutcomeV1::Failed => HarnessRunOutcomeDto::Failed,
        HarnessRunOutcomeV1::Cancelled => HarnessRunOutcomeDto::Cancelled,
        HarnessRunOutcomeV1::Interrupted => HarnessRunOutcomeDto::Interrupted,
        HarnessRunOutcomeV1::ExternalEffectUnknown => HarnessRunOutcomeDto::ExternalEffectUnknown,
    }
}

/// Converts one durable trigger reason into its domain value.
///
/// # Errors
///
/// Returns `harness_source_unavailable` for a durable reason identity that is
/// not canonical identity text.
fn trigger_reason_from_storage(
    record: &HarnessTriggerReasonRecordDto,
) -> DtoResult<HarnessTriggerReasonV1> {
    Ok(HarnessTriggerReasonV1 {
        reason_id: harness_identity(&record.reason_id)?,
        source_kind: source_kind_from_storage(record.source_kind),
        first_observed_at_ms: record.first_observed_at_ms,
        last_observed_at_ms: record.last_observed_at_ms,
        coalesced_count: record.coalesced_count,
    })
}

/// Converts one durable verified checkpoint into its domain value.
///
/// # Errors
///
/// Returns the typed identity, digest, revision, or size failures of the
/// durable checkpoint record.
fn checkpoint_from_storage(
    record: &HarnessCheckpointRecordDto,
) -> DtoResult<HarnessVerifiedCheckpointV1> {
    HarnessVerifiedCheckpointV1::new(
        harness_identity(&record.checkpoint_id)?,
        harness_identity(&record.producing_run_id)?,
        record.checkpoint_revision,
        harness_digest(&record.content_digest)?,
        record.checkpoint_bytes,
    )
}

/// Whether one durable trigger reason binds the launch to a direct successor.
///
/// A reason that originated from a selected known terminal outcome is a direct
/// successor of that outcome and consumes one successor unit.
const fn is_direct_successor(record: &HarnessTriggerReasonRecordDto) -> bool {
    matches!(
        record.source_kind,
        HarnessSourceKindDto::TerminalOutcomeLink
    )
}

/// Computes the deterministic digest of one durable trigger reason.
fn trigger_reason_digest(record: &HarnessTriggerReasonRecordDto) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "harness-trigger-reason-v1");
    push_framed(&mut input, &record.harness_id);
    push_framed(&mut input, &record.reason_id);
    push_framed(&mut input, record.source_kind.name());
    push_framed(&mut input, &record.rule_revision.to_string());
    push_framed(&mut input, &record.first_observed_at_ms.to_string());
    push_framed(&mut input, &record.last_observed_at_ms.to_string());
    push_framed(&mut input, &record.coalesced_count.to_string());
    push_framed(&mut input, record.state.name());
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one run-terminal decision.
fn run_terminal_digest(
    harness_id: &str,
    producing_run_id: &str,
    outcome: HarnessRunOutcomeV1,
    occurred_at_ms: u64,
) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "harness-run-terminal-v1");
    push_framed(&mut input, harness_id);
    push_framed(&mut input, producing_run_id);
    push_framed(&mut input, run_outcome_to_storage(outcome).name());
    push_framed(&mut input, &occurred_at_ms.to_string());
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one verified-checkpoint decision.
fn checkpoint_decision_digest(
    harness_id: &str,
    producing_run_id: &str,
    disposition: HarnessCheckpointDispositionDto,
    current: Option<&HarnessCheckpointRecordDto>,
) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "harness-checkpoint-decision-v1");
    push_framed(&mut input, harness_id);
    push_framed(&mut input, producing_run_id);
    push_framed(&mut input, disposition.name());
    if let Some(current) = current {
        push_framed(&mut input, &current.checkpoint_id);
        push_framed(&mut input, &current.checkpoint_revision.to_string());
        push_framed(&mut input, &current.content_digest);
    }
    Digest256::sha256(&input)
}

/// Computes the deterministic digest of one admitted launch.
fn launch_digest(
    harness_id: &str,
    rule_revision: u64,
    reason_id: &str,
    run_id: &str,
    occurred_at_ms: u64,
) -> Digest256 {
    let mut input = Vec::new();
    push_framed(&mut input, "harness-launch-v1");
    push_framed(&mut input, harness_id);
    push_framed(&mut input, &rule_revision.to_string());
    push_framed(&mut input, reason_id);
    push_framed(&mut input, run_id);
    push_framed(&mut input, &occurred_at_ms.to_string());
    Digest256::sha256(&input)
}

/// Computes the deterministic identity of one assembled dossier.
fn dossier_identity(harness_id: &str, rule_revision: u64, reason_id: &str) -> [u8; 16] {
    let mut input = Vec::new();
    push_framed(&mut input, "harness-dossier-v1");
    push_framed(&mut input, harness_id);
    push_framed(&mut input, &rule_revision.to_string());
    push_framed(&mut input, reason_id);
    derived_identity(Digest256::sha256(&input))
}

/// One closed catch-up observation of a missed schedule window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessCatchUpObservationDto {
    /// The number of missed slots coalesced into this one reason.
    pub missed_slots: u64,
}

/// One request capturing a durable harness trigger observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureHarnessTriggerRequestDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The stable reason identity of this observation.
    pub reason_id: String,
    /// The closed source kind of this observation.
    pub source_kind: HarnessSourceKindV1,
    /// The observation time in Unix milliseconds.
    pub observed_at_ms: u64,
    /// The cause-chain reference of this observation, when one exists.
    pub cause_chain_reference: Option<String>,
    /// The bounded typed references carried by this observation.
    pub bounded_references: Vec<String>,
    /// The coalesced catch-up observation of a missed window, when any.
    pub catch_up: Option<HarnessCatchUpObservationDto>,
}

/// The durable outcome of one redelivery-safe trigger capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessTriggerCaptureResultDto {
    /// The single pending reason of the rule after capture.
    pub reason: HarnessTriggerReasonRecordDto,
    /// The capture outcome.
    pub outcome: HarnessTriggerCaptureOutcomeDto,
    /// The `TriggerCaptured` journal record, absent for a redelivery.
    pub journal_record: Option<HarnessJournalRecordDto>,
}

/// The fixed sub-agent corridor selection of one launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessSubAgentCorridorSelectionDto {
    /// The corridor selection identity.
    pub corridor_reference: [u8; 16],
    /// The class fixed by the corridor.
    pub permitted_class: HarnessExecutionClassV1,
    /// The depth fixed by the corridor.
    pub permitted_depth: u64,
    /// The child count fixed by the corridor.
    pub permitted_child_count: u64,
}

/// The active goal of one goal-directed launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessGoalReferenceDto {
    /// The active goal identity.
    pub goal_id: [u8; 16],
    /// The exact live goal revision.
    pub goal_revision: u64,
}

/// One request admitting the single pending reason of one rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessLaunchRequestDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The daemon-assigned fresh ordinary run identity of this launch.
    pub proposed_run_id: String,
    /// The origin of this launch request.
    pub origin: HarnessLaunchOriginV1,
    /// Whether the daemon-owned concurrency slot is free at this observation.
    pub daemon_concurrency_available: bool,
    /// The cause-chain depth measured from the launch's cause lineage.
    pub cause_chain_depth: u64,
    /// The inherited base class requested by the launch profile.
    pub requested_class: HarnessExecutionClassV1,
    /// The candidate narrowed read-and-delegate tool selection.
    pub narrowed_tool_ids: Vec<String>,
    /// The sub-agent corridor selection, required by a `sub_agent` tool.
    pub sub_agent_corridor: Option<HarnessSubAgentCorridorSelectionDto>,
    /// The active goal of a goal-directed rule revision.
    pub goal: Option<HarnessGoalReferenceDto>,
    /// The complete bounded dossier size measured by the daemon.
    pub dossier_bytes: u64,
    /// The admission time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One admitted goal continuation of a goal-directed launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessGoalContinuationDto {
    /// The active goal identity.
    pub goal_id: [u8; 16],
    /// The exact live goal revision.
    pub goal_revision: u64,
    /// The rule revision that admitted the continuation.
    pub rule_revision: u64,
    /// The separately admitted launch identity of this continuation.
    pub launch_reference: String,
}

/// One admitted harness launch with its dossier and class resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessAdmittedLaunchDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The daemon-assigned fresh ordinary run identity of this launch.
    pub run_id: String,
    /// The exact immutable rule revision that admitted the launch.
    pub rule_revision: u64,
    /// The consumed reason in its admitted state.
    pub reason: HarnessTriggerReasonRecordDto,
    /// The resulting durable counters of the rule.
    pub counters: HarnessCounterRecordDto,
    /// The separately admitted launch identity of this continuation.
    pub launch_reference: String,
    /// The resolved class and narrowed tool selection of this launch.
    pub class_resolution: HarnessClassResolutionV1,
    /// The corridor selection gating `sub_agent`, when one was used.
    pub corridor_reference: Option<[u8; 16]>,
    /// The goal continuation of a goal-directed launch, when any.
    pub goal_continuation: Option<HarnessGoalContinuationDto>,
    /// The bounded dossier stored at admission.
    pub dossier: HarnessDossierRecordDto,
    /// The `LaunchAdmitted` journal record.
    pub journal_record: HarnessJournalRecordDto,
}

/// One retained pending reason with no capacity or no permitted launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRetainedTriggerDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The retained pending reason.
    pub reason: HarnessTriggerReasonRecordDto,
    /// The `LaunchRetained` journal record.
    pub journal_record: HarnessJournalRecordDto,
}

/// The typed outcome of one harness launch attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HarnessLaunchOutcomeDto {
    /// The reason admitted exactly one new independent launch.
    Admitted(Box<HarnessAdmittedLaunchDto>),
    /// The reason was retained while no slot was available; nothing launched.
    Retained(Box<HarnessRetainedTriggerDto>),
}

/// One completely validated checkpoint candidate of a successful run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessCheckpointCandidateDto {
    /// The daemon-assigned checkpoint identity.
    pub checkpoint_id: [u8; 16],
    /// The checkpoint revision, versioned from one.
    pub checkpoint_revision: u64,
    /// The canonical checkpoint content digest.
    pub content_digest: Digest256,
    /// The checkpoint size in bytes, never truncated.
    pub checkpoint_bytes: u64,
}

/// One bounded safe conclusion of a run, supplied by the daemon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessConclusionInputDto {
    /// The canonical conclusion content digest.
    pub content_digest: Digest256,
    /// The conclusion size in bytes, never truncated.
    pub conclusion_bytes: u64,
}

/// One run-terminal decision of one harness run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRunOutcomeInputDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The producing run identity.
    pub producing_run_id: String,
    /// The known outcome of the producing run.
    pub outcome: HarnessRunOutcomeV1,
    /// The completely validated candidate of a successful run, when present.
    pub candidate_checkpoint: Option<HarnessCheckpointCandidateDto>,
    /// The bounded safe conclusion of the run, when one exists.
    pub conclusion: Option<HarnessConclusionInputDto>,
    /// The durable decision time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The committed run-terminal decision with its journal records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRunTerminalDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The producing run identity.
    pub producing_run_id: String,
    /// The known outcome of the producing run.
    pub outcome: HarnessRunOutcomeV1,
    /// Whether the candidate replaced the checkpoint or the previous one stayed.
    pub checkpoint_disposition: HarnessCheckpointDispositionDto,
    /// The current verified checkpoint after the decision, when any exists.
    pub current_checkpoint: Option<HarnessCheckpointRecordDto>,
    /// The journal records committed by this decision, in order.
    pub journal_records: Vec<HarnessJournalRecordDto>,
    /// The safe conclusion record published after the durable commit.
    pub conclusion: Option<HarnessConclusionRecordDto>,
    /// Whether the conclusion reached its selected presentation.
    pub published: bool,
}

/// One daemon restart observation of an interrupted harness run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessRestartRecoveryInputDto {
    /// The owning harness identity.
    pub harness_id: [u8; 16],
    /// The interrupted producing run identity.
    pub interrupted_run_id: [u8; 16],
    /// The post-disconnect state the daemon observed during recovery.
    pub observed_contract: HarnessDisconnectContractV1,
    /// The recovery time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The committed restart recovery of one interrupted harness run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRestartRecoveryDto {
    /// The interrupted run left in its `Interrupted` outcome.
    pub terminal: HarnessRunTerminalDto,
    /// The one live post-disconnect contract.
    pub contract: HarnessDisconnectContractV1,
    /// Whether any previously started external work resumes. Always false.
    pub external_work_resumes: bool,
    /// Whether a later attempt is a separately admitted launch. Always true.
    pub requires_separate_admission: bool,
}

/// One cancellation cascade of the active run and its descendant subtree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessCancellationInputDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The cancelled direct run identity.
    pub producing_run_id: String,
    /// Whether the rule owned a non-terminal run before the cascade.
    pub has_active_run: bool,
    /// The cancelled direct run and every cancelled descendant, deduplicated.
    pub cascaded_run_ids: Vec<String>,
    /// The cancellation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The committed cancellation cascade of one harness run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessCancellationDto {
    /// The cancelled direct run identity.
    pub cancelled_run_id: String,
    /// The number of cancelled runs in the cascade.
    pub cascaded_run_count: u64,
    /// The run-terminal decision of the cancelled direct run.
    pub terminal: HarnessRunTerminalDto,
    /// Whether any previously started external work resumes. Always false.
    pub external_work_resumes: bool,
    /// Whether a later attempt is a separately admitted launch. Always true.
    pub later_attempt_requires_separate_admission: bool,
}

/// The bounded safe projection published after one durable commit and reread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessConclusionPublicationDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The producing run identity.
    pub producing_run_id: String,
    /// The canonical conclusion content digest.
    pub content_digest: String,
    /// The conclusion size in bytes, never truncated.
    pub conclusion_bytes: u64,
    /// The selected presentation mode of the admitting revision.
    pub presentation_mode: HarnessPresentationModeDto,
    /// The durable `RunTerminal` journal sequence this publication follows.
    pub journal_sequence: u64,
    /// The publication time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// Publication seam invoked after the durable harness result has been
/// committed and independently re-read.
pub trait HarnessPublicationPort {
    /// Publishes the compact safe linked-activity entry of one conclusion.
    ///
    /// # Errors
    ///
    /// Returns the typed error reported by the daemon-owned publication
    /// boundary.
    fn publish_harness_conclusion(&self, input: &HarnessConclusionPublicationDto) -> DtoResult<()>;
}

impl HarnessPublicationPort for () {
    fn publish_harness_conclusion(&self, _: &HarnessConclusionPublicationDto) -> DtoResult<()> {
        Ok(())
    }
}

/// The harness runtime over DTO-only durable repository contracts.
pub struct HarnessRuntimeService<'a, Rules, Triggers, Checkpoints>
where
    Rules: HarnessRuleRepositoryDto,
    Triggers: HarnessTriggerRepositoryDto,
    Checkpoints: HarnessCheckpointRepositoryDto,
{
    rules: &'a Rules,
    triggers: &'a Triggers,
    checkpoints: &'a Checkpoints,
}

impl<'a, Rules, Triggers, Checkpoints> HarnessRuntimeService<'a, Rules, Triggers, Checkpoints>
where
    Rules: HarnessRuleRepositoryDto,
    Triggers: HarnessTriggerRepositoryDto,
    Checkpoints: HarnessCheckpointRepositoryDto,
{
    /// Creates the harness runtime over its durable repositories.
    #[must_use]
    pub const fn new(
        rules: &'a Rules,
        triggers: &'a Triggers,
        checkpoints: &'a Checkpoints,
    ) -> Self {
        Self {
            rules,
            triggers,
            checkpoints,
        }
    }

    /// Durably captures one trigger observation before any admission.
    ///
    /// The request is bound to the rule's live revision and applied project
    /// time zone, never to a caller-supplied revision; a changed pending reason
    /// appends its `TriggerCaptured` journal record after the durable capture
    /// commits, and a redelivery writes nothing.
    ///
    /// # Errors
    ///
    /// Returns the structural `invalid_harness_trigger_capture` and
    /// `harness_source_limit_exceeded` rejections, `harness_schedule_invalid`
    /// for an incoherent catch-up, the typed rule or revision read failures,
    /// `harness_archived` and `harness_revision_conflict` from the durable
    /// capture transaction, and the typed journal failures.
    pub fn capture_trigger(
        &self,
        request: &CaptureHarnessTriggerRequestDto,
    ) -> DtoResult<HarnessTriggerCaptureResultDto> {
        self.validate_capture_request(request)?;
        let rule = self.rules.load_harness_rule(request.harness_id.clone())?;
        let revision = self
            .rules
            .load_harness_rule_revision(rule.harness_id.clone(), rule.active_revision)?;
        let capture = self
            .triggers
            .capture_harness_trigger(CaptureHarnessTriggerInputDto {
                harness_id: rule.harness_id.clone(),
                reason_id: request.reason_id.clone(),
                source_kind: source_kind_to_storage(request.source_kind),
                rule_revision: rule.active_revision,
                observed_at_ms: request.observed_at_ms,
                applied_time_zone: revision.applied_time_zone,
                cause_chain_reference: request.cause_chain_reference.clone(),
                bounded_references: request.bounded_references.clone(),
                catch_up_missed_slots: request.catch_up.map_or(0, |catch_up| catch_up.missed_slots),
            })?;
        let journal_record = self.capture_journal_record(&capture, request.observed_at_ms)?;
        Ok(HarnessTriggerCaptureResultDto {
            reason: capture.reason,
            outcome: capture.outcome,
            journal_record,
        })
    }

    /// Atomically admits the single pending reason of one rule.
    ///
    /// The rule, its exact active revision, the pending reason, and the durable
    /// counters are re-read; the resolved class, the narrowed tool selection,
    /// the corridor, the code-owned bounds, and the goal continuation are
    /// validated before the reason is consumed. Without capacity the reason is
    /// retained and no launch occurs.
    ///
    /// # Errors
    ///
    /// Returns `harness_archived` for an archived rule and `harness_not_active`
    /// for an automatic source of a paused rule, the
    /// `invalid_harness_class_resolution` selection failures,
    /// `harness_cause_chain_limit_exceeded`,
    /// `harness_concurrency_limit_exceeded`, and
    /// `harness_source_unavailable` bound or reason failures, the typed
    /// repository failures of the atomic launch, and the typed dossier or
    /// journal failures.
    pub fn admit_pending_trigger(
        &self,
        request: &HarnessLaunchRequestDto,
    ) -> DtoResult<HarnessLaunchOutcomeDto> {
        self.validate_launch_request(request)?;
        let rule = self.rules.load_harness_rule(request.harness_id.clone())?;
        let revision = self
            .rules
            .load_harness_rule_revision(rule.harness_id.clone(), rule.active_revision)?;
        let lifecycle = lifecycle_from_storage(rule.lifecycle_state);
        let resolved_class = self.validate_launch_selection(request, &revision, lifecycle)?;
        let pending = self
            .triggers
            .load_pending_harness_trigger(rule.harness_id.clone())?
            .ok_or_else(|| {
                harness_error(
                    "harness_source_unavailable",
                    "a rule admits only its single pending reason",
                )
            })?;
        let reason = trigger_reason_from_storage(&pending)?;
        let counters = self
            .triggers
            .load_harness_counters(rule.harness_id.clone())?;
        let capacity = HarnessAdmissionCapacityV1::available(
            counters.concurrent_non_terminal == 0,
            request.daemon_concurrency_available,
        );
        match admit_harness_trigger(&reason, lifecycle, request.origin, capacity)? {
            HarnessTriggerAdmissionV1::Retained(retained) => {
                let mut record = pending;
                record.first_observed_at_ms = retained.first_observed_at_ms;
                record.last_observed_at_ms = retained.last_observed_at_ms;
                record.coalesced_count = retained.coalesced_count;
                let journal_record = self.append_journal(
                    &rule.harness_id,
                    HarnessJournalRecordKindDto::LaunchRetained,
                    format!(
                        "harness launch retained {} coalesced={} window={}-{}",
                        record.source_kind.name(),
                        record.coalesced_count,
                        record.first_observed_at_ms,
                        record.last_observed_at_ms
                    ),
                    trigger_reason_digest(&record),
                    request.occurred_at_ms,
                )?;
                Ok(HarnessLaunchOutcomeDto::Retained(Box::new(
                    HarnessRetainedTriggerDto {
                        harness_id: rule.harness_id,
                        reason: record,
                        journal_record,
                    },
                )))
            }
            HarnessTriggerAdmissionV1::Admitted(admitted) => {
                self.validate_launch_bounds(counters, &pending, request)?;
                let launch_reference = derived_identity(launch_digest(
                    &rule.harness_id,
                    rule.active_revision,
                    &pending.reason_id,
                    &request.proposed_run_id,
                    request.occurred_at_ms,
                ));
                let goal_continuation =
                    self.goal_continuation(&revision, lifecycle, request, launch_reference)?;
                let dossier = self.assemble_dossier(
                    &rule.harness_id,
                    rule.active_revision,
                    &revision,
                    &pending,
                    request,
                )?;
                let stored_dossier = self.checkpoints.store_harness_dossier(dossier)?;
                let launch = self
                    .triggers
                    .record_harness_launch(RecordHarnessLaunchInputDto {
                        harness_id: rule.harness_id.clone(),
                        reason_id: pending.reason_id.clone(),
                        cause_chain_depth: request.cause_chain_depth,
                        direct_successor: is_direct_successor(&pending),
                        occurred_at_ms: request.occurred_at_ms,
                    })?;
                let journal_record = self.append_journal(
                    &rule.harness_id,
                    HarnessJournalRecordKindDto::LaunchAdmitted,
                    format!(
                        "harness launch admitted revision={} run={} class={}",
                        rule.active_revision,
                        request.proposed_run_id,
                        class_to_storage(resolved_class).name()
                    ),
                    launch_digest(
                        &rule.harness_id,
                        rule.active_revision,
                        &identity_text(admitted.reason_id),
                        &request.proposed_run_id,
                        request.occurred_at_ms,
                    ),
                    request.occurred_at_ms,
                )?;
                Ok(HarnessLaunchOutcomeDto::Admitted(Box::new(
                    HarnessAdmittedLaunchDto {
                        harness_id: rule.harness_id,
                        run_id: request.proposed_run_id.clone(),
                        rule_revision: rule.active_revision,
                        reason: launch.reason,
                        counters: launch.counters,
                        launch_reference: identity_text(launch_reference),
                        class_resolution: HarnessClassResolutionV1 {
                            class: resolved_class,
                            narrowed_tool_ids: request.narrowed_tool_ids.clone(),
                        },
                        corridor_reference: request
                            .sub_agent_corridor
                            .map(|corridor| corridor.corridor_reference),
                        goal_continuation,
                        dossier: stored_dossier,
                        journal_record,
                    },
                )))
            }
        }
    }

    /// Commits one run-terminal decision without publishing a conclusion.
    ///
    /// # Errors
    ///
    /// Returns the typed structural, checkpoint, or journal failures of the
    /// decision.
    pub fn commit_run_outcome(
        &self,
        input: &HarnessRunOutcomeInputDto,
    ) -> DtoResult<HarnessRunTerminalDto> {
        self.commit_run_outcome_with_publication(input, &())
    }

    /// Commits one run-terminal decision and publishes its safe conclusion.
    ///
    /// The verified-checkpoint decision and the run-terminal journal record
    /// commit first; publication then re-reads the durable terminal record and
    /// current checkpoint before the selected presentation is applied.
    ///
    /// # Errors
    ///
    /// Returns `harness_checkpoint_unavailable` when a successful run supplies
    /// no completely validated candidate, the typed checkpoint or journal
    /// failures of the commit, `harness_source_unavailable` when the durable
    /// reread does not observe the committed evidence, and the typed
    /// publication failure.
    pub fn commit_run_outcome_with_publication<P: HarnessPublicationPort>(
        &self,
        input: &HarnessRunOutcomeInputDto,
        publisher: &P,
    ) -> DtoResult<HarnessRunTerminalDto> {
        self.validate_run_outcome(input)?;
        let rule = self.rules.load_harness_rule(input.harness_id.clone())?;
        let revision = self
            .rules
            .load_harness_rule_revision(rule.harness_id.clone(), rule.active_revision)?;
        let previous = self.load_domain_checkpoint(&rule.harness_id)?;
        let candidate = self.candidate_checkpoint(&rule.harness_id, &revision, input)?;
        let candidate_domain = candidate
            .as_ref()
            .map(checkpoint_from_storage)
            .transpose()?;
        let resolved =
            verified_checkpoint_after_run(previous.as_ref(), input.outcome, candidate_domain)?;
        let committed =
            self.checkpoints
                .commit_harness_checkpoint(CommitHarnessCheckpointInputDto {
                    harness_id: rule.harness_id.clone(),
                    producing_run_id: input.producing_run_id.clone(),
                    run_outcome: run_outcome_to_storage(input.outcome),
                    candidate: if resolved.is_some() { candidate } else { None },
                    occurred_at_ms: input.occurred_at_ms,
                })?;
        let checkpoint_journal = self.append_journal(
            &rule.harness_id,
            match committed.disposition {
                HarnessCheckpointDispositionDto::Replaced => {
                    HarnessJournalRecordKindDto::CheckpointAccepted
                }
                HarnessCheckpointDispositionDto::RetainedPrevious => {
                    HarnessJournalRecordKindDto::CheckpointRetained
                }
            },
            format!(
                "harness checkpoint {} outcome={}",
                committed.disposition.name(),
                run_outcome_to_storage(input.outcome).name()
            ),
            checkpoint_decision_digest(
                &rule.harness_id,
                &input.producing_run_id,
                committed.disposition,
                committed.current.as_ref(),
            ),
            input.occurred_at_ms,
        )?;
        let run_journal = self.append_journal(
            &rule.harness_id,
            HarnessJournalRecordKindDto::RunTerminal,
            format!(
                "harness run terminal {}",
                run_outcome_to_storage(input.outcome).name()
            ),
            run_terminal_digest(
                &rule.harness_id,
                &input.producing_run_id,
                input.outcome,
                input.occurred_at_ms,
            ),
            input.occurred_at_ms,
        )?;
        let mut journal_records = vec![checkpoint_journal, run_journal.clone()];
        let (conclusion, published) = self.publish_conclusion(
            publisher,
            &rule.harness_id,
            &revision,
            input,
            run_journal.sequence,
        )?;
        if let Some(record) = &conclusion {
            journal_records.push(self.append_journal(
                &rule.harness_id,
                HarnessJournalRecordKindDto::ConclusionPublished,
                format!(
                    "harness conclusion published bytes={} presentation={}",
                    record.conclusion_bytes,
                    record.presentation_mode.name()
                ),
                harness_digest(&record.content_digest)?,
                input.occurred_at_ms,
            )?);
        }
        Ok(HarnessRunTerminalDto {
            harness_id: rule.harness_id,
            producing_run_id: input.producing_run_id.clone(),
            outcome: input.outcome,
            checkpoint_disposition: committed.disposition,
            current_checkpoint: committed.current,
            journal_records,
            conclusion,
            published,
        })
    }

    /// Leaves one interrupted harness run in its `Interrupted` outcome.
    ///
    /// The observed post-disconnect state must be the one live contract; no
    /// provider request, tool, process, child agent, bridge operation, or
    /// external action resumes, retries, reattaches, or reruns, and a later
    /// attempt is a separately admitted launch with new identities.
    ///
    /// # Errors
    ///
    /// Returns `invalid_harness_disconnect_contract` for an observed state that
    /// would resume old external work, and the typed failures of the
    /// interrupted run-terminal commit.
    pub fn recover_interrupted(
        &self,
        input: &HarnessRestartRecoveryInputDto,
    ) -> DtoResult<HarnessRestartRecoveryDto> {
        validate_harness_disconnect_contract(&input.observed_contract)?;
        let terminal = self.commit_run_outcome(&HarnessRunOutcomeInputDto {
            harness_id: identity_text(input.harness_id),
            producing_run_id: identity_text(input.interrupted_run_id),
            outcome: HarnessRunOutcomeV1::Interrupted,
            candidate_checkpoint: None,
            conclusion: None,
            occurred_at_ms: input.occurred_at_ms,
        })?;
        Ok(HarnessRestartRecoveryDto {
            terminal,
            contract: HarnessDisconnectContractV1::frozen(),
            external_work_resumes: false,
            requires_separate_admission: true,
        })
    }

    /// Commits one cancellation cascade of the rule's active run.
    ///
    /// Cancellation follows the ordinary two-step lifecycle of the direct run
    /// and cascades through its whole descendant subtree; the report must name
    /// the direct run, stay inside the code-owned subtree bound, and repeat no
    /// identity. The previous verified checkpoint is retained.
    ///
    /// # Errors
    ///
    /// Returns `harness_not_active` when the rule owns no active run or rejects
    /// the operation, `invalid_harness_launch_request` for an incoherent
    /// cascade report, and the typed failures of the cancelled run-terminal
    /// commit.
    pub fn cancel_active_run(
        &self,
        input: &HarnessCancellationInputDto,
    ) -> DtoResult<HarnessCancellationDto> {
        self.validate_cancellation(input)?;
        let rule = self.rules.load_harness_rule(input.harness_id.clone())?;
        validate_harness_rule_operation(
            lifecycle_from_storage(rule.lifecycle_state),
            input.has_active_run,
            HarnessRuleOperationV1::CancelActiveRun,
        )?;
        let terminal = self.commit_run_outcome(&HarnessRunOutcomeInputDto {
            harness_id: rule.harness_id,
            producing_run_id: input.producing_run_id.clone(),
            outcome: HarnessRunOutcomeV1::Cancelled,
            candidate_checkpoint: None,
            conclusion: None,
            occurred_at_ms: input.occurred_at_ms,
        })?;
        Ok(HarnessCancellationDto {
            cancelled_run_id: input.producing_run_id.clone(),
            cascaded_run_count: u64::try_from(input.cascaded_run_ids.len()).unwrap_or(u64::MAX),
            terminal,
            external_work_resumes: false,
            later_attempt_requires_separate_admission: true,
        })
    }

    /// Loads one bounded page of one rule's durable journal.
    ///
    /// # Errors
    ///
    /// Returns `invalid_harness_journal_page` for a zero or over-bound page
    /// limit, the typed rule read failures, and the typed journal read
    /// failures.
    pub fn load_journal(
        &self,
        harness_id: &str,
        after_sequence: u64,
        limit: u64,
    ) -> DtoResult<Vec<HarnessJournalRecordDto>> {
        if limit == 0 || limit > MAX_HARNESS_JOURNAL_PAGE {
            return Err(harness_error(
                "invalid_harness_journal_page",
                "a journal page stays inside its code-owned bound",
            ));
        }
        let rule = self.rules.load_harness_rule(harness_id.to_owned())?;
        self.checkpoints.load_harness_journal(
            rule.harness_id,
            LoadHarnessJournalInputDto {
                after_sequence,
                limit,
            },
        )
    }

    /// Loads one durable dossier by identity.
    ///
    /// # Errors
    ///
    /// Returns the typed dossier read failures.
    pub fn load_dossier(&self, dossier_id: &str) -> DtoResult<HarnessDossierRecordDto> {
        self.checkpoints.load_harness_dossier(dossier_id.to_owned())
    }

    /// Validates the structural shape of one trigger capture request.
    fn validate_capture_request(&self, request: &CaptureHarnessTriggerRequestDto) -> DtoResult<()> {
        harness_identity(&request.harness_id)?;
        harness_identity(&request.reason_id)?;
        if let Some(reference) = &request.cause_chain_reference {
            harness_identity(reference)?;
        }
        for reference in &request.bounded_references {
            harness_identity(reference)?;
        }
        if request.bounded_references.len() > MAX_HARNESS_TRIGGER_REFERENCES {
            return Err(harness_error(
                "harness_source_limit_exceeded",
                "a trigger reason stays inside its bounded reference count",
            ));
        }
        if request
            .catch_up
            .is_some_and(|catch_up| catch_up.missed_slots == 0)
        {
            return Err(harness_error(
                "harness_schedule_invalid",
                "a catch-up reason coalesces at least one missed slot",
            ));
        }
        Ok(())
    }

    /// Validates the structural shape of one launch request.
    fn validate_launch_request(&self, request: &HarnessLaunchRequestDto) -> DtoResult<()> {
        harness_identity(&request.harness_id)?;
        harness_identity(&request.proposed_run_id)?;
        if request.cause_chain_depth == 0 {
            return Err(harness_error(
                "invalid_harness_launch_request",
                "a launch measures its cause-chain depth from its cause lineage",
            ));
        }
        if request.narrowed_tool_ids.len() > HARNESS_MAX_NARROWED_TOOLS {
            return Err(harness_error(
                "invalid_harness_class_resolution",
                "the narrowed tool selection exceeds its bound",
            ));
        }
        if let Some(goal) = request.goal
            && goal.goal_revision == 0
        {
            return Err(harness_error(
                "harness_revision_conflict",
                "a goal continuation binds one exact live goal revision",
            ));
        }
        validate_harness_dossier_bytes(request.dossier_bytes)?;
        Ok(())
    }

    /// Validates the class, tool, and goal selection of one launch.
    ///
    /// # Errors
    ///
    /// Returns the `invalid_harness_class_resolution` selection failures,
    /// `harness_archived` for an archived rule, and the closed goal-mode
    /// failures of a goal-directed or repeated-task revision.
    fn validate_launch_selection(
        &self,
        request: &HarnessLaunchRequestDto,
        revision: &HarnessRuleRevisionRecordDto,
        lifecycle: HarnessRuleLifecycleStateV1,
    ) -> DtoResult<HarnessExecutionClassV1> {
        let corridor = request
            .sub_agent_corridor
            .as_ref()
            .map(sub_agent_corridor)
            .transpose()?;
        validate_harness_narrowed_tools(&request.narrowed_tool_ids, corridor.as_ref())?;
        let resolved =
            resolve_harness_class(request.requested_class, class_from_storage(revision.class));
        validate_harness_class_narrowing(request.requested_class, resolved)?;
        if lifecycle.is_archived() {
            return Err(harness_error(
                "harness_archived",
                "an archived harness rule admits no launch",
            ));
        }
        match revision.task_mode {
            HarnessTaskModeDto::GoalDirected => {
                if request.goal.is_none() {
                    return Err(harness_error(
                        "harness_not_active",
                        "goal mode continues only against an active goal",
                    ));
                }
            }
            HarnessTaskModeDto::RepeatedTask => {
                if request.goal.is_some() {
                    return Err(harness_error(
                        "invalid_harness_launch_request",
                        "a repeated-task revision continues no goal",
                    ));
                }
            }
        }
        Ok(resolved)
    }

    /// Validates the code-owned capacity, chain, successor, and launch bounds.
    fn validate_launch_bounds(
        &self,
        counters: HarnessCounterRecordDto,
        pending: &HarnessTriggerReasonRecordDto,
        request: &HarnessLaunchRequestDto,
    ) -> DtoResult<()> {
        validate_harness_cause_depth(request.cause_chain_depth)?;
        validate_harness_concurrency_count(counters.concurrent_non_terminal.saturating_add(1))?;
        validate_harness_total_launches(counters.total_launches.saturating_add(1))?;
        if is_direct_successor(pending) {
            validate_harness_successor_count(counters.direct_successors.saturating_add(1))?;
        }
        Ok(())
    }

    /// Builds the goal continuation of one goal-directed launch, when any.
    fn goal_continuation(
        &self,
        revision: &HarnessRuleRevisionRecordDto,
        lifecycle: HarnessRuleLifecycleStateV1,
        request: &HarnessLaunchRequestDto,
        launch_reference: [u8; 16],
    ) -> DtoResult<Option<HarnessGoalContinuationDto>> {
        if !matches!(revision.task_mode, HarnessTaskModeDto::GoalDirected) {
            return Ok(None);
        }
        if lifecycle.is_archived() {
            return Err(harness_error(
                "harness_archived",
                "an archived rule never continues a goal",
            ));
        }
        let goal = request.goal.ok_or_else(|| {
            harness_error(
                "harness_not_active",
                "goal mode continues only against an active goal",
            )
        })?;
        Ok(Some(HarnessGoalContinuationDto {
            goal_id: goal.goal_id,
            goal_revision: goal.goal_revision,
            rule_revision: revision.revision,
            launch_reference: identity_text(launch_reference),
        }))
    }

    /// Assembles the bounded two-layer dossier of one admitted launch.
    ///
    /// # Errors
    ///
    /// Returns the typed dossier bound, reference, time zone, checkpoint, or
    /// digest failures of the assembled record.
    fn assemble_dossier(
        &self,
        harness_id: &str,
        rule_revision: u64,
        revision: &HarnessRuleRevisionRecordDto,
        pending: &HarnessTriggerReasonRecordDto,
        request: &HarnessLaunchRequestDto,
    ) -> DtoResult<HarnessDossierRecordDto> {
        let checkpoint = self
            .checkpoints
            .load_current_harness_checkpoint(harness_id.to_owned())?;
        let checkpoint_reference = checkpoint
            .as_ref()
            .map(|record| harness_identity(&record.checkpoint_id))
            .transpose()?;
        let source_references = revision
            .source_references
            .iter()
            .map(|reference| harness_identity(reference))
            .collect::<DtoResult<Vec<[u8; 16]>>>()?;
        let typed_references = pending
            .bounded_references
            .iter()
            .map(|reference| harness_identity(reference))
            .collect::<DtoResult<Vec<[u8; 16]>>>()?;
        let dossier = HarnessDossierV1::new(
            harness_digest(&revision.task_digest)?,
            harness_identity(&pending.reason_id)?,
            source_references,
            typed_references,
            checkpoint_reference,
            revision.applied_time_zone.clone(),
            request.dossier_bytes,
            Digest256::sha256(&dossier_digest_input(
                harness_id,
                rule_revision,
                pending,
                request,
            )),
        )?;
        let record = HarnessDossierRecordDto {
            dossier_id: identity_text(dossier_identity(
                harness_id,
                rule_revision,
                &pending.reason_id,
            )),
            harness_id: harness_id.to_owned(),
            rule_revision,
            reason_id: pending.reason_id.clone(),
            task_digest: digest_text(dossier.task_digest()),
            source_references: dossier
                .source_references()
                .iter()
                .map(|identity| identity_text(*identity))
                .collect(),
            typed_references: dossier
                .typed_references()
                .iter()
                .map(|identity| identity_text(*identity))
                .collect(),
            checkpoint_reference: dossier.checkpoint_reference().map(identity_text),
            dossier_bytes: dossier.dossier_bytes(),
            canonical_dossier_digest: digest_text(dossier.dossier_digest()),
            created_at_ms: request.occurred_at_ms,
        };
        record.validate()?;
        Ok(record)
    }

    /// Validates the structural shape of one run-terminal decision.
    fn validate_run_outcome(&self, input: &HarnessRunOutcomeInputDto) -> DtoResult<()> {
        harness_identity(&input.harness_id)?;
        harness_identity(&input.producing_run_id)?;
        if let Some(candidate) = &input.candidate_checkpoint {
            if input.outcome != HarnessRunOutcomeV1::Completed {
                return Err(harness_error(
                    "invalid_harness_run_outcome",
                    "a non-successful run retains the previous verified checkpoint",
                ));
            }
            if candidate.checkpoint_revision == 0 {
                return Err(harness_error(
                    "harness_revision_conflict",
                    "a checkpoint is versioned from one",
                ));
            }
            validate_harness_checkpoint_bytes(candidate.checkpoint_bytes)?;
        }
        if let Some(conclusion) = &input.conclusion {
            validate_harness_conclusion_bytes(conclusion.conclusion_bytes)?;
        }
        Ok(())
    }

    /// Builds the durable checkpoint candidate of one successful run.
    fn candidate_checkpoint(
        &self,
        harness_id: &str,
        revision: &HarnessRuleRevisionRecordDto,
        input: &HarnessRunOutcomeInputDto,
    ) -> DtoResult<Option<HarnessCheckpointRecordDto>> {
        let Some(candidate) = &input.candidate_checkpoint else {
            return Ok(None);
        };
        let record = HarnessCheckpointRecordDto {
            checkpoint_id: identity_text(candidate.checkpoint_id),
            harness_id: harness_id.to_owned(),
            rule_revision: revision.revision,
            producing_run_id: input.producing_run_id.clone(),
            checkpoint_revision: candidate.checkpoint_revision,
            content_digest: digest_text(candidate.content_digest),
            checkpoint_bytes: candidate.checkpoint_bytes,
            is_current: true,
            created_at_ms: input.occurred_at_ms,
        };
        record.validate()?;
        Ok(Some(record))
    }

    /// Loads the current verified checkpoint as its domain value, when any.
    fn load_domain_checkpoint(
        &self,
        harness_id: &str,
    ) -> DtoResult<Option<HarnessVerifiedCheckpointV1>> {
        self.checkpoints
            .load_current_harness_checkpoint(harness_id.to_owned())?
            .as_ref()
            .map(checkpoint_from_storage)
            .transpose()
    }

    /// Publishes the safe conclusion of one run after a durable reread.
    ///
    /// # Errors
    ///
    /// Returns the typed conclusion bound or `harness_source_unavailable`
    /// reread failures and the typed publication failure.
    fn publish_conclusion<P: HarnessPublicationPort>(
        &self,
        publisher: &P,
        harness_id: &str,
        revision: &HarnessRuleRevisionRecordDto,
        input: &HarnessRunOutcomeInputDto,
        terminal_sequence: u64,
    ) -> DtoResult<(Option<HarnessConclusionRecordDto>, bool)> {
        let Some(conclusion) = &input.conclusion else {
            return Ok((None, false));
        };
        let validated = HarnessConclusionV1::new(
            harness_identity(&input.producing_run_id)?,
            conclusion.content_digest,
            conclusion.conclusion_bytes,
        )?;
        let record = HarnessConclusionRecordDto {
            harness_id: harness_id.to_owned(),
            producing_run_id: input.producing_run_id.clone(),
            content_digest: digest_text(validated.content_digest()),
            conclusion_bytes: validated.conclusion_bytes(),
            presentation_mode: revision.presentation_mode,
            created_at_ms: input.occurred_at_ms,
        };
        record.validate()?;
        self.reread_terminal_evidence(harness_id, terminal_sequence)?;
        if revision.presentation_mode == HarnessPresentationModeDto::JournalAndActivityEntry {
            publisher.publish_harness_conclusion(&HarnessConclusionPublicationDto {
                harness_id: harness_id.to_owned(),
                producing_run_id: input.producing_run_id.clone(),
                content_digest: record.content_digest.clone(),
                conclusion_bytes: record.conclusion_bytes,
                presentation_mode: record.presentation_mode,
                journal_sequence: terminal_sequence,
                occurred_at_ms: input.occurred_at_ms,
            })?;
        }
        Ok((Some(record), true))
    }

    /// Independently re-reads the committed terminal evidence.
    ///
    /// # Errors
    ///
    /// Returns `harness_source_unavailable` when the durable terminal journal
    /// record or the current checkpoint cannot be re-read.
    fn reread_terminal_evidence(&self, harness_id: &str, terminal_sequence: u64) -> DtoResult<()> {
        let journal = self.checkpoints.load_harness_journal(
            harness_id.to_owned(),
            LoadHarnessJournalInputDto {
                after_sequence: terminal_sequence.saturating_sub(1),
                limit: 1,
            },
        )?;
        if journal.first().map(|record| record.sequence) != Some(terminal_sequence) {
            return Err(harness_error(
                "harness_source_unavailable",
                "the durable harness journal is readable after recovery",
            ));
        }
        self.checkpoints
            .load_current_harness_checkpoint(harness_id.to_owned())?;
        Ok(())
    }

    /// Validates the structural shape of one cancellation cascade report.
    fn validate_cancellation(&self, input: &HarnessCancellationInputDto) -> DtoResult<()> {
        harness_identity(&input.harness_id)?;
        harness_identity(&input.producing_run_id)?;
        if input.cascaded_run_ids.is_empty()
            || u64::try_from(input.cascaded_run_ids.len()).unwrap_or(u64::MAX)
                > HARNESS_MAX_CONCURRENT
        {
            return Err(harness_error(
                "invalid_harness_launch_request",
                "a cancellation cascade names one bounded descendant subtree",
            ));
        }
        let mut seen = Vec::new();
        for run_id in &input.cascaded_run_ids {
            harness_identity(run_id)?;
            if seen.contains(run_id) {
                return Err(harness_error(
                    "invalid_harness_launch_request",
                    "a cancellation cascade repeats no run identity",
                ));
            }
            seen.push(run_id.clone());
        }
        if !seen.contains(&input.producing_run_id) {
            return Err(harness_error(
                "invalid_harness_launch_request",
                "a cancellation cascade contains its direct run",
            ));
        }
        Ok(())
    }

    /// Appends one durable harness journal record.
    fn append_journal(
        &self,
        harness_id: &str,
        record_kind: HarnessJournalRecordKindDto,
        safe_summary: String,
        canonical_record_digest: Digest256,
        occurred_at_ms: u64,
    ) -> DtoResult<HarnessJournalRecordDto> {
        self.checkpoints
            .append_harness_journal_record(AppendHarnessJournalRecordInputDto {
                harness_id: harness_id.to_owned(),
                record_kind,
                safe_summary,
                canonical_record_digest: digest_text(canonical_record_digest),
                occurred_at_ms,
            })
    }

    /// Appends the journal record of one changed durable trigger capture.
    fn capture_journal_record(
        &self,
        capture: &HarnessTriggerCaptureRecordDto,
        occurred_at_ms: u64,
    ) -> DtoResult<Option<HarnessJournalRecordDto>> {
        if !capture.outcome.changed_pending_reason() {
            return Ok(None);
        }
        let record = self.append_journal(
            &capture.reason.harness_id,
            HarnessJournalRecordKindDto::TriggerCaptured,
            format!(
                "harness trigger {} {} coalesced={} window={}-{}",
                capture.reason.source_kind.name(),
                capture.outcome.name(),
                capture.reason.coalesced_count,
                capture.reason.first_observed_at_ms,
                capture.reason.last_observed_at_ms
            ),
            trigger_reason_digest(&capture.reason),
            occurred_at_ms,
        )?;
        Ok(Some(record))
    }
}

/// Builds one sub-agent corridor value from its fixed launch selection.
///
/// # Errors
///
/// Returns `invalid_harness_class_resolution` for a zero or over-bound depth or
/// child count.
fn sub_agent_corridor(
    selection: &HarnessSubAgentCorridorSelectionDto,
) -> DtoResult<HarnessSubAgentCorridorV1> {
    HarnessSubAgentCorridorV1::new(
        selection.corridor_reference,
        selection.permitted_class,
        selection.permitted_depth,
        selection.permitted_child_count,
    )
}

/// Builds the framed digest input of one assembled dossier.
fn dossier_digest_input(
    harness_id: &str,
    rule_revision: u64,
    pending: &HarnessTriggerReasonRecordDto,
    request: &HarnessLaunchRequestDto,
) -> Vec<u8> {
    let mut input = Vec::new();
    push_framed(&mut input, "harness-dossier-digest-v1");
    push_framed(&mut input, harness_id);
    push_framed(&mut input, &rule_revision.to_string());
    push_framed(&mut input, &pending.reason_id);
    push_framed(&mut input, &pending.first_observed_at_ms.to_string());
    push_framed(&mut input, &pending.last_observed_at_ms.to_string());
    push_framed(&mut input, &pending.coalesced_count.to_string());
    push_framed(&mut input, &request.dossier_bytes.to_string());
    push_framed(&mut input, &request.occurred_at_ms.to_string());
    input
}
