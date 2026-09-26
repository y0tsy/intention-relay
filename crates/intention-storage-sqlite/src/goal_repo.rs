//! Durable Goal aggregate and verification repository.
//!
//! Owner: architecture 28 with architecture 17 and ADR 0044. This module owns
//! the current single-version schema for Goal identity and tree edges, gate
//! and evidence references, memory cards, Skills, roles, gate templates,
//! proposals, compaction working state, and the unified verification records,
//! plus the store-level transaction and fault-injection evidence for the
//! Slice 3 Goal surface.
//!
//! Every method validates its DTO before any row is written, writes through
//! parameterized SQL only, and commits multi-row state in one immediate
//! transaction. Durable list fields use a private delimiter-separated
//! canonical text codec whose elements are validated control-free before
//! encoding, so the unit and record separators are unambiguous.

use intention_domain::goal_domain::{
    GOAL_MAX_ACTIVE_MEMORY_CARDS, GOAL_MAX_GATES_PER_GOAL, GOAL_MAX_SESSION_LINKS_PER_PROJECT_GOAL,
    validate_goal_allocation, validate_goal_tree_attachment,
};
use intention_storage::goal_repo::{
    AppendGoalGateRevisionInputDto, AppendGoalRevisionInputDto, ApplyVerifierMutationOutcomeDto,
    AttachGoalChildInputDto, ConversationSummaryRecordDto, CreateGoalGateInputDto,
    CreateGoalInputDto, CreateGoalSessionLinkInputDto, GoalCardRepositoryDto,
    GoalCompactionRepositoryDto, GoalCompactionWorkingFormRecordDto, GoalEvidenceKindDto,
    GoalEvidenceReferenceDto, GoalGateDefinitionDto, GoalGateExceptionDto, GoalGateRecordDto,
    GoalGateRepositoryDto, GoalGateResultRecordDto, GoalGateRevisionReferenceDto,
    GoalGateTemplateRecordDto, GoalLifecycleStateDto, GoalMemoryCardRecordDto,
    GoalMemoryCardReplacementInputDto, GoalMemoryCardRollbackInputDto, GoalParentLinkRecordDto,
    GoalProposalRepositoryDto, GoalReadinessStateDto, GoalRecordDto, GoalRecordScopeDto,
    GoalRepositoryDto, GoalRevisionRecordDto, GoalRoleCardRecordDto, GoalScopeDto,
    GoalSessionLinkRecordDto, GoalSkillCardRecordDto, GoalTemplateLifecycleStateDto,
    GoalTemplateProvenanceKindDto, GoalTemplateScopeDto, GoalUserDecisionStateDto,
    GoalVerificationRepositoryDto, RecordCompactionSuffixReferenceInputDto,
    RecordGoalUserDecisionInputDto, RefinementDraftRecordDto, RefinementDraftStateDto,
    RefinementEditRecordDto, RevokeVerifierAuthorityInputDto, SetGoalReadinessInputDto,
    TransitionGoalGateTemplateInputDto, TransitionGoalLifecycleInputDto,
    VerifierAuditBaselineRecordDto, VerifierAuditEvidenceRecordDto, VerifierAuditVerdictRecordDto,
    VerifierAuthorityConsumptionRuleDto, VerifierAuthorityConsumptionStateDto,
    VerifierAuthorityRecordDto, VerifierAuthorityReferenceDto, VerifierContractReferenceDto,
    VerifierEvidenceKindDto, VerifierFrozenReferencesDto, VerifierGoalReferenceDto,
    VerifierOperationDto, VerifierOperationIdentityDto, VerifierTargetLifecycleDto,
    VerifierTargetMutationRecordDto, VerifierTargetReferenceDto, VerifierTargetSetReferenceDto,
};
use intention_types::{DtoResult, ErrorDto};
use sqlite::OptionalExtension;

use super::{
    SqliteStorageRepository, codec_error, conflict, not_found, sqlite_integer, storage_error,
};

/// The current single-version DDL of the Slice 3 Goal and verification family.
pub const SCHEMA_GOAL_SQL: &str = "
CREATE TABLE IF NOT EXISTS goals (
  goal_id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','session')),
  project_id TEXT NOT NULL,
  session_id TEXT,
  active_revision INTEGER NOT NULL CHECK(active_revision > 0),
  lifecycle_state TEXT NOT NULL CHECK(lifecycle_state IN ('active','needs_rework','paused','stopped','archived')),
  readiness_kind TEXT NOT NULL CHECK(readiness_kind IN ('not_ready','ready')),
  readiness_evidence TEXT NOT NULL,
  user_decision_kind TEXT NOT NULL CHECK(user_decision_kind IN ('unaccepted','accepted','accepted_with_exception')),
  user_decision_exceptions TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
  CHECK((scope_kind = 'project' AND session_id IS NULL) OR (scope_kind = 'session' AND session_id IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS goals_by_project ON goals(project_id);
CREATE INDEX IF NOT EXISTS goals_by_session ON goals(session_id);
CREATE TABLE IF NOT EXISTS goal_revisions (
  goal_id TEXT NOT NULL REFERENCES goals(goal_id),
  revision INTEGER NOT NULL CHECK(revision > 0),
  title TEXT NOT NULL,
  objective TEXT NOT NULL,
  inherited_rule_references TEXT NOT NULL,
  local_rule_references TEXT NOT NULL,
  required_gate_references TEXT NOT NULL,
  canonical_revision_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  PRIMARY KEY(goal_id, revision)
);
CREATE TABLE IF NOT EXISTS goal_parent_links (
  parent_goal_id TEXT NOT NULL REFERENCES goals(goal_id),
  child_goal_id TEXT NOT NULL REFERENCES goals(goal_id),
  child_revision_at_link INTEGER NOT NULL CHECK(child_revision_at_link > 0),
  canonical_link_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  PRIMARY KEY(parent_goal_id, child_goal_id),
  CHECK(parent_goal_id <> child_goal_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_obligatory_parent_per_goal ON goal_parent_links(child_goal_id);
CREATE TABLE IF NOT EXISTS goal_session_links (
  link_id TEXT PRIMARY KEY,
  project_goal_id TEXT NOT NULL REFERENCES goals(goal_id),
  session_id TEXT NOT NULL,
  effective_from_revision INTEGER NOT NULL CHECK(effective_from_revision > 0),
  canonical_link_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_session_link_per_goal ON goal_session_links(project_goal_id, session_id);
CREATE TABLE IF NOT EXISTS goal_gates (
  gate_id TEXT PRIMARY KEY,
  goal_id TEXT NOT NULL REFERENCES goals(goal_id),
  revision INTEGER NOT NULL CHECK(revision > 0),
  definition_kind TEXT NOT NULL CHECK(definition_kind IN ('reference','executable')),
  definition TEXT NOT NULL,
  canonical_revision_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
);
CREATE INDEX IF NOT EXISTS goal_gates_by_goal ON goal_gates(goal_id);
CREATE TABLE IF NOT EXISTS goal_gate_revisions (
  gate_id TEXT NOT NULL REFERENCES goal_gates(gate_id),
  revision INTEGER NOT NULL CHECK(revision > 0),
  definition_kind TEXT NOT NULL CHECK(definition_kind IN ('reference','executable')),
  definition TEXT NOT NULL,
  canonical_revision_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  PRIMARY KEY(gate_id, revision)
);
CREATE TABLE IF NOT EXISTS goal_gate_templates (
  template_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK(revision > 0),
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','goal','session')),
  owner_id TEXT NOT NULL,
  capability_reference TEXT NOT NULL,
  input_family TEXT NOT NULL CHECK(input_family IN ('closed_text_v1','closed_path_set_v1')),
  requires_confirmation INTEGER NOT NULL CHECK(requires_confirmation IN (0,1)),
  lifecycle_state TEXT NOT NULL CHECK(lifecycle_state IN ('enabled','archived')),
  provenance_kind TEXT NOT NULL CHECK(provenance_kind IN ('user','model_proposal')),
  provenance_draft_id TEXT,
  provenance_accepted_by_user INTEGER NOT NULL CHECK(provenance_accepted_by_user IN (0,1)),
  canonical_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
  PRIMARY KEY(template_id, revision)
);
CREATE INDEX IF NOT EXISTS goal_gate_templates_by_owner ON goal_gate_templates(scope_kind, owner_id);
CREATE TABLE IF NOT EXISTS goal_gate_results (
  gate_id TEXT NOT NULL REFERENCES goal_gates(gate_id),
  gate_revision INTEGER NOT NULL CHECK(gate_revision > 0),
  producing_run_id TEXT NOT NULL,
  outcome_kind TEXT NOT NULL CHECK(outcome_kind IN ('passed','failed','timed_out','cancelled','output_bound_exceeded','external_effect_unknown','reference_unavailable','revision_stale','template_unavailable')),
  disposition TEXT NOT NULL CHECK(disposition IN ('passed','failed','unavailable','unknown_effect')),
  evidence TEXT,
  canonical_result_digest TEXT NOT NULL,
  occurred_at_ms INTEGER NOT NULL CHECK(occurred_at_ms >= 0),
  PRIMARY KEY(gate_id, gate_revision)
);
CREATE TABLE IF NOT EXISTS goal_memory_cards (
  record_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK(revision > 0),
  kind TEXT NOT NULL CHECK(kind IN ('fact','decision','preference','past_failure')),
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','goal','session')),
  owner_id TEXT NOT NULL,
  title TEXT NOT NULL,
  safe_purpose TEXT NOT NULL,
  retained_content_reference TEXT NOT NULL,
  canonical_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  PRIMARY KEY(record_id, revision)
);
CREATE INDEX IF NOT EXISTS goal_memory_cards_by_owner ON goal_memory_cards(scope_kind, owner_id);
CREATE TABLE IF NOT EXISTS goal_memory_relations (
  relation_kind TEXT NOT NULL CHECK(relation_kind IN ('replacement','rollback')),
  source_record_id TEXT NOT NULL,
  source_revision INTEGER NOT NULL CHECK(source_revision > 0),
  target_record_id TEXT NOT NULL,
  target_revision INTEGER NOT NULL CHECK(target_revision > 0),
  occurred_at_ms INTEGER NOT NULL CHECK(occurred_at_ms >= 0),
  PRIMARY KEY(relation_kind, source_record_id, source_revision, target_record_id, target_revision)
);
CREATE TABLE IF NOT EXISTS goal_skill_cards (
  skill_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK(revision > 0),
  canonical_name TEXT NOT NULL,
  description TEXT NOT NULL,
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','goal','session')),
  owner_id TEXT NOT NULL,
  content_reference TEXT NOT NULL,
  canonical_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  PRIMARY KEY(skill_id, revision)
);
CREATE INDEX IF NOT EXISTS goal_skill_cards_by_owner ON goal_skill_cards(scope_kind, owner_id);
CREATE TABLE IF NOT EXISTS goal_role_cards (
  role_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK(revision > 0),
  canonical_name TEXT NOT NULL,
  task TEXT NOT NULL,
  permitted_class TEXT NOT NULL CHECK(permitted_class IN ('light','medium','heavy')),
  tool_subset TEXT NOT NULL,
  context_limit_bytes INTEGER NOT NULL CHECK(context_limit_bytes > 0),
  result_limit_bytes INTEGER NOT NULL CHECK(result_limit_bytes > 0),
  canonical_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  PRIMARY KEY(role_id, revision)
);
CREATE TABLE IF NOT EXISTS refinement_drafts (
  draft_id TEXT PRIMARY KEY,
  source_run_id TEXT NOT NULL,
  leading_goal_id TEXT NOT NULL,
  milestone TEXT NOT NULL CHECK(milestone IN ('technical_readiness','user_acceptance','acceptance_with_exception','stop','required_gate_failure','obligatory_child_terminal_outcome')),
  base_goal_revision INTEGER NOT NULL CHECK(base_goal_revision > 0),
  base_record_reference TEXT NOT NULL,
  base_record_revision INTEGER NOT NULL CHECK(base_record_revision > 0),
  edits TEXT NOT NULL,
  evidence_references TEXT NOT NULL,
  safe_rationale TEXT NOT NULL,
  canonical_digest TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending','accepted','rejected')),
  coalesced_evidence_count INTEGER NOT NULL CHECK(coalesced_evidence_count > 0),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  decided_at_ms INTEGER CHECK(decided_at_ms IS NULL OR decided_at_ms >= 0),
  CHECK((state = 'pending' AND decided_at_ms IS NULL) OR state <> 'pending')
);
CREATE UNIQUE INDEX IF NOT EXISTS one_pending_refinement_draft ON refinement_drafts(leading_goal_id) WHERE state = 'pending';
CREATE TABLE IF NOT EXISTS conversation_summaries (
  summary_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK(revision > 0),
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','goal','session')),
  owner_id TEXT NOT NULL,
  previous_summary_reference TEXT,
  source_range_start TEXT NOT NULL,
  source_range_end TEXT NOT NULL,
  safe_content TEXT NOT NULL,
  canonical_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  PRIMARY KEY(summary_id, revision)
);
CREATE TABLE IF NOT EXISTS compaction_working_forms (
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','goal','session')),
  owner_id TEXT NOT NULL,
  current_summary_id TEXT,
  current_summary_revision INTEGER CHECK(current_summary_revision IS NULL OR current_summary_revision > 0),
  uncompacted_suffix TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
  PRIMARY KEY(scope_kind, owner_id)
);
CREATE TABLE IF NOT EXISTS verifier_authorities (
  authority_id TEXT NOT NULL,
  authority_revision INTEGER NOT NULL CHECK(authority_revision > 0),
  verifier_mandate_id TEXT NOT NULL,
  target_set_id TEXT NOT NULL,
  target_set_digest TEXT NOT NULL,
  allowed_operations TEXT NOT NULL,
  audit_contract_id TEXT NOT NULL,
  audit_contract_revision INTEGER NOT NULL CHECK(audit_contract_revision > 0),
  audit_contract_digest TEXT NOT NULL,
  issued_at_ms INTEGER NOT NULL CHECK(issued_at_ms >= 0),
  expires_at_ms INTEGER CHECK(expires_at_ms IS NULL OR expires_at_ms >= 0),
  revoked_at_ms INTEGER CHECK(revoked_at_ms IS NULL OR revoked_at_ms >= 0),
  revocation_reference TEXT,
  consumption_rule TEXT NOT NULL CHECK(consumption_rule IN ('single_use','reusable_while_active')),
  consumption_state TEXT NOT NULL CHECK(consumption_state IN ('unconsumed','consumed')),
  consumed_by_mutation_reference TEXT,
  canonical_authority_digest TEXT NOT NULL,
  PRIMARY KEY(authority_id, authority_revision),
  CHECK((revoked_at_ms IS NULL) = (revocation_reference IS NULL)),
  CHECK((consumption_state = 'consumed') = (consumed_by_mutation_reference IS NOT NULL))
);
CREATE TABLE IF NOT EXISTS verifier_audit_baselines (
  canonical_baseline_digest TEXT PRIMARY KEY,
  authority_id TEXT NOT NULL,
  authority_revision INTEGER NOT NULL CHECK(authority_revision > 0),
  authority_digest TEXT NOT NULL,
  verifier_mandate_revision INTEGER NOT NULL CHECK(verifier_mandate_revision > 0),
  target_mandate_id TEXT NOT NULL,
  target_revision INTEGER NOT NULL CHECK(target_revision > 0),
  target_sequence INTEGER NOT NULL CHECK(target_sequence >= 0),
  target_lifecycle TEXT NOT NULL CHECK(target_lifecycle IN ('draft','active','working','paused','paused_awaiting_decision','needs_rework','completed','stopped','archived')),
  frozen_goal_references TEXT NOT NULL,
  frozen_contract_references TEXT NOT NULL,
  optional_unknown_effect_reference TEXT,
  audit_contract_id TEXT NOT NULL,
  audit_contract_revision INTEGER NOT NULL CHECK(audit_contract_revision > 0),
  audit_contract_digest TEXT NOT NULL,
  graph_epoch INTEGER CHECK(graph_epoch IS NULL OR graph_epoch >= 0),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
);
CREATE INDEX IF NOT EXISTS verifier_audit_baselines_by_target ON verifier_audit_baselines(target_mandate_id, target_revision);
CREATE TABLE IF NOT EXISTS verifier_audit_evidence (
  evidence_id TEXT PRIMARY KEY,
  authority_id TEXT NOT NULL,
  authority_revision INTEGER NOT NULL CHECK(authority_revision > 0),
  authority_digest TEXT NOT NULL,
  target_mandate_id TEXT NOT NULL,
  target_revision INTEGER NOT NULL CHECK(target_revision > 0),
  frozen_goal_references TEXT NOT NULL,
  frozen_contract_references TEXT NOT NULL,
  evidence_kind TEXT NOT NULL CHECK(evidence_kind IN ('unconditional_pass','qualifying_fail','inconclusive','graph_terminalization_closure','reconciliation_standard_proof')),
  retained_content_reference TEXT NOT NULL,
  canonical_evidence_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
);
CREATE TABLE IF NOT EXISTS verifier_audit_verdicts (
  verdict_id TEXT PRIMARY KEY,
  authority_id TEXT NOT NULL,
  authority_revision INTEGER NOT NULL CHECK(authority_revision > 0),
  authority_digest TEXT NOT NULL,
  target_mandate_id TEXT NOT NULL,
  target_revision INTEGER NOT NULL CHECK(target_revision > 0),
  baseline_digest TEXT NOT NULL,
  verdict TEXT NOT NULL CHECK(verdict IN ('pass','fail','inconclusive','target_revision_stale','target_unavailable','verifier_unavailable','verifier_external_effect_unknown')),
  evidence_references TEXT NOT NULL,
  canonical_verdict_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
);
CREATE TABLE IF NOT EXISTS verifier_target_mutations (
  mutation_id TEXT PRIMARY KEY,
  operation_id TEXT NOT NULL UNIQUE,
  operation_digest TEXT NOT NULL,
  authority_id TEXT NOT NULL,
  authority_revision INTEGER NOT NULL CHECK(authority_revision > 0),
  authority_digest TEXT NOT NULL,
  audit_contract_id TEXT NOT NULL,
  audit_contract_revision INTEGER NOT NULL CHECK(audit_contract_revision > 0),
  audit_contract_digest TEXT NOT NULL,
  target_mandate_id TEXT NOT NULL,
  target_revision INTEGER NOT NULL CHECK(target_revision > 0),
  operation TEXT NOT NULL CHECK(operation IN ('mark_needs_rework','mark_complete','stop','revise_full','resolve_unknown_effect')),
  audit_evidence_references TEXT NOT NULL,
  expected_target_revision INTEGER NOT NULL CHECK(expected_target_revision > 0),
  expected_target_sequence INTEGER NOT NULL CHECK(expected_target_sequence >= 0),
  expected_baseline_digest TEXT NOT NULL,
  canonical_mutation_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
);
";

/// The unit separator joining encoded list items or record fields.
const UNIT_SEPARATOR: char = '\u{1f}';
/// The record separator joining encoded records.
const RECORD_SEPARATOR: char = '\u{1e}';

/// Encodes one flat list of safe values.
fn encode_items(items: &[String]) -> String {
    items.join(&UNIT_SEPARATOR.to_string())
}

/// Decodes one flat list of safe values.
fn decode_items(encoded: &str) -> DtoResult<Vec<String>> {
    if encoded.is_empty() {
        return Ok(Vec::new());
    }
    Ok(encoded.split(UNIT_SEPARATOR).map(str::to_owned).collect())
}

/// Encodes ordered records of delimiter-joined fields.
fn encode_records(records: &[Vec<String>]) -> String {
    records
        .iter()
        .map(|fields| fields.join(&UNIT_SEPARATOR.to_string()))
        .collect::<Vec<_>>()
        .join(&RECORD_SEPARATOR.to_string())
}

/// Decodes ordered records of delimiter-joined fields.
fn decode_records(encoded: &str) -> DtoResult<Vec<Vec<String>>> {
    if encoded.is_empty() {
        return Ok(Vec::new());
    }
    Ok(encoded
        .split(RECORD_SEPARATOR)
        .map(|record| record.split(UNIT_SEPARATOR).map(str::to_owned).collect())
        .collect())
}

/// Parses one persisted canonical unsigned integer field.
fn parse_u64(text: &str) -> DtoResult<u64> {
    text.parse::<u64>()
        .map_err(|_| codec_error("persisted integer is malformed"))
}

/// Reads one non-negative integer column as `u64`.
fn u64_column(row: &sqlite::Row<'_>, index: usize) -> Result<u64, sqlite::Error> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| sqlite::Error::IntegralValueOutOfRange(index, value))
}

/// Reads one optional non-negative integer column as `u64`.
fn optional_u64_column(row: &sqlite::Row<'_>, index: usize) -> Result<Option<u64>, sqlite::Error> {
    row.get::<_, Option<i64>>(index)?
        .map(u64::try_from)
        .transpose()
        .map_err(|_| sqlite::Error::IntegralValueOutOfRange(index, i64::MIN))
}

/// Reads one boolean column stored as an integer flag.
fn bool_column(row: &sqlite::Row<'_>, index: usize) -> Result<bool, sqlite::Error> {
    Ok(row.get::<_, i64>(index)? != 0)
}

/// Converts one unsigned value into its SQLite integer encoding.
fn int(value: u64, message: &'static str) -> DtoResult<i64> {
    sqlite_integer(value, message)
}

fn goal_not_active_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        not_found(
            "goal_not_active",
            "the requested durable Goal does not exist",
        )
    } else {
        storage_error(error)
    }
}

fn goal_revision_conflict() -> ErrorDto {
    conflict(
        "goal_revision_conflict",
        "the durable Goal revision is stale or already bound to different content",
    )
}

/// Runs one immediate write transaction and commits it on success.
fn write<T>(
    repository: &SqliteStorageRepository,
    operation: impl FnOnce(&sqlite::Connection) -> DtoResult<T>,
) -> DtoResult<T> {
    let mut connection = repository.connection()?;
    let result = (|| {
        let transaction = repository.begin(&mut connection)?;
        let value = operation(&transaction)?;
        transaction.commit().map_err(storage_error)?;
        Ok(value)
    })();
    drop(connection);
    result
}

/// Encodes one ordered evidence reference set.
fn encode_evidence(evidence: &[GoalEvidenceReferenceDto]) -> String {
    encode_records(
        &evidence
            .iter()
            .map(|reference| {
                vec![
                    reference.evidence_id.clone(),
                    reference.revision.to_string(),
                    reference.kind.name().to_owned(),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

/// Decodes one ordered evidence reference set.
fn decode_evidence(encoded: &str) -> DtoResult<Vec<GoalEvidenceReferenceDto>> {
    decode_records(encoded)?
        .into_iter()
        .map(|fields| {
            if fields.len() != 3 {
                return Err(codec_error("persisted evidence reference is malformed"));
            }
            Ok(GoalEvidenceReferenceDto {
                evidence_id: fields[0].clone(),
                revision: parse_u64(&fields[1])?,
                kind: GoalEvidenceKindDto::parse(&fields[2])?,
            })
        })
        .collect()
}

/// Encodes one ordered gate exception set.
fn encode_exceptions(exceptions: &[GoalGateExceptionDto]) -> String {
    encode_records(
        &exceptions
            .iter()
            .map(|exception| {
                vec![
                    exception.gate_id.clone(),
                    exception.gate_revision.to_string(),
                    exception.kind.name().to_owned(),
                    exception.evidence.evidence_id.clone(),
                    exception.evidence.revision.to_string(),
                    exception.evidence.kind.name().to_owned(),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

/// Decodes one ordered gate exception set.
fn decode_exceptions(encoded: &str) -> DtoResult<Vec<GoalGateExceptionDto>> {
    decode_records(encoded)?
        .into_iter()
        .map(|fields| {
            if fields.len() != 6 {
                return Err(codec_error("persisted gate exception is malformed"));
            }
            Ok(GoalGateExceptionDto {
                gate_id: fields[0].clone(),
                gate_revision: parse_u64(&fields[1])?,
                kind: intention_storage::goal_repo::GoalGateExceptionKindDto::parse(&fields[2])?,
                evidence: GoalEvidenceReferenceDto {
                    evidence_id: fields[3].clone(),
                    revision: parse_u64(&fields[4])?,
                    kind: GoalEvidenceKindDto::parse(&fields[5])?,
                },
            })
        })
        .collect()
}

/// Encodes one ordered gate revision reference set.
fn encode_gate_references(references: &[GoalGateRevisionReferenceDto]) -> String {
    encode_records(
        &references
            .iter()
            .map(|reference| vec![reference.gate_id.clone(), reference.revision.to_string()])
            .collect::<Vec<_>>(),
    )
}

/// Decodes one ordered gate revision reference set.
fn decode_gate_references(encoded: &str) -> DtoResult<Vec<GoalGateRevisionReferenceDto>> {
    decode_records(encoded)?
        .into_iter()
        .map(|fields| {
            if fields.len() != 2 {
                return Err(codec_error("persisted gate reference is malformed"));
            }
            Ok(GoalGateRevisionReferenceDto {
                gate_id: fields[0].clone(),
                revision: parse_u64(&fields[1])?,
            })
        })
        .collect()
}

type GoalRow = (
    String,
    String,
    String,
    Option<String>,
    u64,
    String,
    String,
    String,
    String,
    String,
    u64,
    u64,
);

const GOAL_COLUMNS: &str = "goal_id, scope_kind, project_id, session_id, active_revision, \
     lifecycle_state, readiness_kind, readiness_evidence, user_decision_kind, \
     user_decision_exceptions, created_at_ms, updated_at_ms";

fn goal_from_columns(row: &GoalRow) -> DtoResult<GoalRecordDto> {
    let scope = match row.1.as_str() {
        "project" => GoalScopeDto::Project {
            project_id: row.2.clone(),
        },
        "session" => GoalScopeDto::Session {
            project_id: row.2.clone(),
            session_id: row.3.clone().unwrap_or_default(),
        },
        _ => return Err(codec_error("persisted Goal scope is malformed")),
    };
    let readiness_state = match row.6.as_str() {
        "not_ready" => GoalReadinessStateDto::NotReady,
        "ready" => GoalReadinessStateDto::Ready {
            verified_evidence_set: decode_evidence(&row.7)?,
        },
        _ => return Err(codec_error("persisted Goal readiness is malformed")),
    };
    let user_decision_state = match row.8.as_str() {
        "unaccepted" => GoalUserDecisionStateDto::Unaccepted,
        "accepted" => GoalUserDecisionStateDto::Accepted,
        "accepted_with_exception" => GoalUserDecisionStateDto::AcceptedWithException {
            exception_evidence_set: decode_exceptions(&row.9)?,
        },
        _ => return Err(codec_error("persisted Goal decision is malformed")),
    };
    Ok(GoalRecordDto {
        goal_id: row.0.clone(),
        scope,
        active_revision: row.4,
        lifecycle_state: GoalLifecycleStateDto::parse(&row.5)?,
        readiness_state,
        user_decision_state,
        created_at_ms: row.10,
        updated_at_ms: row.11,
    })
}

fn load_goal_row(connection: &sqlite::Connection, goal_id: &str) -> DtoResult<GoalRecordDto> {
    connection
        .query_row(
            &format!("SELECT {GOAL_COLUMNS} FROM goals WHERE goal_id=?1"),
            [goal_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    u64_column(row, 4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    u64_column(row, 10)?,
                    u64_column(row, 11)?,
                ))
            },
        )
        .map_err(goal_not_active_or_storage)
        .and_then(|row| goal_from_columns(&row))
}

fn load_goal_optional(
    connection: &sqlite::Connection,
    goal_id: &str,
) -> DtoResult<Option<GoalRecordDto>> {
    connection
        .query_row(
            &format!("SELECT {GOAL_COLUMNS} FROM goals WHERE goal_id=?1"),
            [goal_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    u64_column(row, 4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    u64_column(row, 10)?,
                    u64_column(row, 11)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .map(|row| goal_from_columns(&row))
        .transpose()
}

fn insert_goal_row(connection: &sqlite::Connection, goal: &GoalRecordDto) -> DtoResult<()> {
    let (scope_kind, project_id, session_id) = match &goal.scope {
        GoalScopeDto::Project { project_id } => ("project", project_id.clone(), None),
        GoalScopeDto::Session {
            project_id,
            session_id,
        } => ("session", project_id.clone(), Some(session_id.clone())),
    };
    let readiness_evidence = match &goal.readiness_state {
        GoalReadinessStateDto::NotReady => String::new(),
        GoalReadinessStateDto::Ready {
            verified_evidence_set,
        } => encode_evidence(verified_evidence_set),
    };
    let exceptions = match &goal.user_decision_state {
        GoalUserDecisionStateDto::AcceptedWithException {
            exception_evidence_set,
        } => encode_exceptions(exception_evidence_set),
        GoalUserDecisionStateDto::Unaccepted | GoalUserDecisionStateDto::Accepted => String::new(),
    };
    connection
        .execute(
            "INSERT INTO goals(goal_id, scope_kind, project_id, session_id, active_revision,
                lifecycle_state, readiness_kind, readiness_evidence, user_decision_kind,
                user_decision_exceptions, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            sqlite::params![
                goal.goal_id,
                scope_kind,
                project_id,
                session_id,
                int(goal.active_revision, "revision is outside the SQLite range")?,
                goal.lifecycle_state.name(),
                goal.readiness_state.kind_name(),
                readiness_evidence,
                goal.user_decision_state.kind_name(),
                exceptions,
                int(goal.created_at_ms, "timestamp is outside the SQLite range")?,
                int(goal.updated_at_ms, "timestamp is outside the SQLite range")?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

type RevisionRow = (u64, String, String, String, String, String, String, u64);

fn revision_from_columns(goal_id: &str, row: &RevisionRow) -> DtoResult<GoalRevisionRecordDto> {
    Ok(GoalRevisionRecordDto {
        goal_id: goal_id.to_owned(),
        revision: row.0,
        title: row.1.clone(),
        objective: row.2.clone(),
        inherited_rule_references: decode_items(&row.3)?,
        local_rule_references: decode_items(&row.4)?,
        required_gate_references: decode_gate_references(&row.5)?,
        canonical_revision_digest: row.6.clone(),
        created_at_ms: row.7,
    })
}

const REVISION_COLUMNS: &str = "revision, title, objective, inherited_rule_references, \
     local_rule_references, required_gate_references, canonical_revision_digest, created_at_ms";

fn load_goal_revision_optional(
    connection: &sqlite::Connection,
    goal_id: &str,
    revision: u64,
) -> DtoResult<Option<GoalRevisionRecordDto>> {
    connection
        .query_row(
            &format!(
                "SELECT {REVISION_COLUMNS} FROM goal_revisions WHERE goal_id=?1 AND revision=?2"
            ),
            sqlite::params![
                goal_id,
                int(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    u64_column(row, 0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    u64_column(row, 7)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .map(|row| revision_from_columns(goal_id, &row))
        .transpose()
}

fn insert_goal_revision(
    connection: &sqlite::Connection,
    revision: &GoalRevisionRecordDto,
) -> DtoResult<()> {
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO goal_revisions(goal_id, revision, title, objective,
                inherited_rule_references, local_rule_references, required_gate_references,
                canonical_revision_digest, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            sqlite::params![
                revision.goal_id,
                int(revision.revision, "revision is outside the SQLite range")?,
                revision.title,
                revision.objective,
                encode_items(&revision.inherited_rule_references),
                encode_items(&revision.local_rule_references),
                encode_gate_references(&revision.required_gate_references),
                revision.canonical_revision_digest,
                int(
                    revision.created_at_ms,
                    "timestamp is outside the SQLite range"
                )?,
            ],
        )
        .map_err(storage_error)?;
    if inserted == 0 {
        let existing =
            load_goal_revision_optional(connection, &revision.goal_id, revision.revision)?
                .ok_or_else(goal_revision_conflict)?;
        if existing != *revision {
            return Err(goal_revision_conflict());
        }
    }
    Ok(())
}

fn count_goals_in_column(
    connection: &sqlite::Connection,
    column: &str,
    value: &str,
) -> DtoResult<u64> {
    let sql = match column {
        "project_id" => "SELECT COUNT(*) FROM goals WHERE project_id=?1",
        "session_id" => "SELECT COUNT(*) FROM goals WHERE scope_kind='session' AND session_id=?1",
        _ => return Err(codec_error("invalid Goal count column")),
    };
    let count: i64 = connection
        .query_row(sql, [value], |row| row.get(0))
        .map_err(storage_error)?;
    u64::try_from(count).map_err(|_| codec_error("invalid Goal count"))
}

type ParentLinkRow = (String, String, u64, String, u64);

fn parent_link_from_columns(row: &ParentLinkRow) -> GoalParentLinkRecordDto {
    GoalParentLinkRecordDto {
        parent_goal_id: row.0.clone(),
        child_goal_id: row.1.clone(),
        child_revision_at_link: row.2,
        canonical_link_digest: row.3.clone(),
        created_at_ms: row.4,
    }
}

fn load_parent_link_optional(
    connection: &sqlite::Connection,
    parent_goal_id: &str,
    child_goal_id: &str,
) -> DtoResult<Option<GoalParentLinkRecordDto>> {
    connection
        .query_row(
            "SELECT parent_goal_id, child_goal_id, child_revision_at_link,
                    canonical_link_digest, created_at_ms
             FROM goal_parent_links WHERE parent_goal_id=?1 AND child_goal_id=?2",
            sqlite::params![parent_goal_id, child_goal_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    u64_column(row, 2)?,
                    row.get::<_, String>(3)?,
                    u64_column(row, 4)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
        .map(|row| row.as_ref().map(parent_link_from_columns))
}

fn parent_of(connection: &sqlite::Connection, child_goal_id: &str) -> DtoResult<Option<String>> {
    connection
        .query_row(
            "SELECT parent_goal_id FROM goal_parent_links WHERE child_goal_id=?1",
            [child_goal_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)
}

fn count_children(connection: &sqlite::Connection, parent_goal_id: &str) -> DtoResult<u64> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM goal_parent_links WHERE parent_goal_id=?1",
            [parent_goal_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    u64::try_from(count).map_err(|_| codec_error("invalid child count"))
}

fn list_parent_links(
    connection: &sqlite::Connection,
    parent_goal_id: &str,
) -> DtoResult<Vec<GoalParentLinkRecordDto>> {
    let mut statement = connection
        .prepare(
            "SELECT parent_goal_id, child_goal_id, child_revision_at_link,
                    canonical_link_digest, created_at_ms
             FROM goal_parent_links WHERE parent_goal_id=?1
             ORDER BY created_at_ms, child_goal_id",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([parent_goal_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                u64_column(row, 2)?,
                row.get::<_, String>(3)?,
                u64_column(row, 4)?,
            ))
        })
        .map_err(storage_error)?;
    rows.map(|row| {
        row.map(|row| parent_link_from_columns(&row))
            .map_err(storage_error)
    })
    .collect()
}

fn tree_depth(connection: &sqlite::Connection, goal_id: &str) -> DtoResult<u64> {
    let mut depth = 1_u64;
    let mut current = goal_id.to_owned();
    let mut visited: Vec<String> = Vec::new();
    while let Some(parent) = parent_of(connection, &current)? {
        if visited.contains(&parent) || parent == current {
            return Err(conflict(
                "goal_cycle_detected",
                "the stored Goal graph holds a cycle",
            ));
        }
        visited.push(current);
        current = parent;
        depth = depth
            .checked_add(1)
            .ok_or_else(|| codec_error("invalid Goal depth"))?;
    }
    Ok(depth)
}

fn create_goal_session_link_row(
    connection: &sqlite::Connection,
    link: &GoalSessionLinkRecordDto,
) -> DtoResult<()> {
    connection
        .execute(
            "INSERT INTO goal_session_links(link_id, project_goal_id, session_id,
                effective_from_revision, canonical_link_digest, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            sqlite::params![
                link.link_id,
                link.project_goal_id,
                link.session_id,
                int(
                    link.effective_from_revision,
                    "revision is outside the SQLite range"
                )?,
                link.canonical_link_digest,
                int(link.created_at_ms, "timestamp is outside the SQLite range")?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

type SessionLinkRow = (String, String, String, u64, String, u64);

fn session_link_from_columns(row: &SessionLinkRow) -> GoalSessionLinkRecordDto {
    GoalSessionLinkRecordDto {
        link_id: row.0.clone(),
        project_goal_id: row.1.clone(),
        session_id: row.2.clone(),
        effective_from_revision: row.3,
        canonical_link_digest: row.4.clone(),
        created_at_ms: row.5,
    }
}

const SESSION_LINK_COLUMNS: &str = "link_id, project_goal_id, session_id, \
     effective_from_revision, canonical_link_digest, created_at_ms";

fn list_session_links(
    connection: &sqlite::Connection,
    project_goal_id: &str,
) -> DtoResult<Vec<GoalSessionLinkRecordDto>> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {SESSION_LINK_COLUMNS} FROM goal_session_links
             WHERE project_goal_id=?1 ORDER BY created_at_ms, link_id"
        ))
        .map_err(storage_error)?;
    let rows = statement
        .query_map([project_goal_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                u64_column(row, 3)?,
                row.get::<_, String>(4)?,
                u64_column(row, 5)?,
            ))
        })
        .map_err(storage_error)?;
    rows.map(|row| {
        row.map(|row| session_link_from_columns(&row))
            .map_err(storage_error)
    })
    .collect()
}

impl GoalRepositoryDto for SqliteStorageRepository {
    fn create_goal(&self, input: CreateGoalInputDto) -> DtoResult<GoalRecordDto> {
        input.validate()?;
        let goal = input.goal;
        let revision = input.revision;
        write(self, |connection| {
            if let Some(stored) = load_goal_optional(connection, &goal.goal_id)? {
                return if stored == goal {
                    Ok(stored)
                } else {
                    Err(goal_revision_conflict())
                };
            }
            let project_count =
                count_goals_in_column(connection, "project_id", goal.scope.project_id())?;
            let session_count = match goal.scope.session_id() {
                Some(session_id) => count_goals_in_column(connection, "session_id", session_id)?,
                None => 0,
            };
            validate_goal_allocation(
                usize::try_from(project_count + 1)
                    .map_err(|_| codec_error("invalid Goal count"))?,
                usize::try_from(session_count + 1)
                    .map_err(|_| codec_error("invalid Goal count"))?,
            )?;
            insert_goal_row(connection, &goal)?;
            insert_goal_revision(connection, &revision)?;
            load_goal_row(connection, &goal.goal_id)
        })
    }

    fn load_goal(&self, goal_id: String) -> DtoResult<GoalRecordDto> {
        let connection = self.connection()?;
        load_goal_row(&connection, &goal_id)
    }

    fn load_goal_revision(
        &self,
        goal_id: String,
        revision: u64,
    ) -> DtoResult<GoalRevisionRecordDto> {
        let connection = self.connection()?;
        load_goal_revision_optional(&connection, &goal_id, revision)?
            .ok_or_else(goal_revision_conflict)
    }

    fn append_goal_revision(&self, input: AppendGoalRevisionInputDto) -> DtoResult<GoalRecordDto> {
        input.validate()?;
        let goal_id = input.goal_id;
        let expected_revision = input.expected_revision;
        let revision = input.revision;
        write(self, |connection| {
            let stored = load_goal_row(connection, &goal_id)?;
            if stored.active_revision != expected_revision {
                return Err(goal_revision_conflict());
            }
            let next = expected_revision
                .checked_add(1)
                .ok_or_else(goal_revision_conflict)?;
            if revision.revision != next {
                return Err(goal_revision_conflict());
            }
            match load_goal_revision_optional(connection, &goal_id, next)? {
                Some(existing) if existing != revision => return Err(goal_revision_conflict()),
                Some(_) => {}
                None => insert_goal_revision(connection, &revision)?,
            }
            connection
                .execute(
                    "UPDATE goals SET active_revision=?2, updated_at_ms=?3 WHERE goal_id=?1",
                    sqlite::params![
                        goal_id,
                        int(revision.revision, "revision is outside the SQLite range")?,
                        int(
                            revision.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            load_goal_row(connection, &goal_id)
        })
    }

    fn transition_goal_lifecycle(
        &self,
        input: TransitionGoalLifecycleInputDto,
    ) -> DtoResult<GoalRecordDto> {
        let goal_id = input.goal_id;
        let expected_revision = input.expected_revision;
        let requested = input.lifecycle_state;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let stored = load_goal_row(connection, &goal_id)?;
            if stored.active_revision != expected_revision {
                return Err(goal_revision_conflict());
            }
            if requested == stored.lifecycle_state {
                return Ok(stored);
            }
            if requested == GoalLifecycleStateDto::Archived {
                if stored.lifecycle_state == GoalLifecycleStateDto::Archived {
                    return Err(conflict(
                        "goal_archive_not_terminal",
                        "the Goal is already archived",
                    ));
                }
                if stored.lifecycle_state != GoalLifecycleStateDto::Stopped
                    && !stored.user_decision_state.is_terminal()
                {
                    return Err(conflict(
                        "goal_archive_not_terminal",
                        "only a terminal idle Goal may be archived",
                    ));
                }
            } else {
                use GoalLifecycleStateDto::{Active, Archived, NeedsRework, Paused, Stopped};
                let allowed = matches!(
                    (stored.lifecycle_state, requested),
                    (Active, NeedsRework)
                        | (Active, Paused)
                        | (Active, Stopped)
                        | (NeedsRework, Active)
                        | (NeedsRework, Paused)
                        | (NeedsRework, Stopped)
                        | (Paused, Active)
                        | (Paused, Stopped)
                        | (Archived, Active)
                        | (Archived, Stopped)
                );
                if !allowed {
                    return Err(conflict(
                        "goal_not_active",
                        "the Goal lifecycle transition is not permitted",
                    ));
                }
            }
            connection
                .execute(
                    "UPDATE goals SET lifecycle_state=?2, updated_at_ms=?3 WHERE goal_id=?1",
                    sqlite::params![
                        goal_id,
                        requested.name(),
                        int(occurred_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_goal_row(connection, &goal_id)
        })
    }

    fn set_goal_readiness(&self, input: SetGoalReadinessInputDto) -> DtoResult<GoalRecordDto> {
        input.readiness_state.validate()?;
        let goal_id = input.goal_id;
        let expected_revision = input.expected_revision;
        let readiness_state = input.readiness_state;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let stored = load_goal_row(connection, &goal_id)?;
            if stored.active_revision != expected_revision {
                return Err(goal_revision_conflict());
            }
            let evidence = match &readiness_state {
                GoalReadinessStateDto::NotReady => String::new(),
                GoalReadinessStateDto::Ready {
                    verified_evidence_set,
                } => encode_evidence(verified_evidence_set),
            };
            connection
                .execute(
                    "UPDATE goals SET readiness_kind=?2, readiness_evidence=?3, updated_at_ms=?4
                     WHERE goal_id=?1",
                    sqlite::params![
                        goal_id,
                        readiness_state.kind_name(),
                        evidence,
                        int(occurred_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_goal_row(connection, &goal_id)
        })
    }

    fn record_goal_user_decision(
        &self,
        input: RecordGoalUserDecisionInputDto,
    ) -> DtoResult<GoalRecordDto> {
        input.user_decision_state.validate()?;
        let goal_id = input.goal_id;
        let expected_revision = input.expected_revision;
        let user_decision_state = input.user_decision_state;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let stored = load_goal_row(connection, &goal_id)?;
            if stored.active_revision != expected_revision {
                return Err(goal_revision_conflict());
            }
            match &user_decision_state {
                GoalUserDecisionStateDto::Accepted => {
                    if !stored.readiness_state.is_ready() {
                        return Err(conflict(
                            "goal_not_ready",
                            "user acceptance requires a Ready Goal",
                        ));
                    }
                }
                GoalUserDecisionStateDto::AcceptedWithException { .. } => {
                    if stored.readiness_state.is_ready() {
                        return Err(conflict(
                            "goal_acceptance_exception_invalid",
                            "an exception never creates Ready",
                        ));
                    }
                }
                GoalUserDecisionStateDto::Unaccepted => {}
            }
            let exceptions = match &user_decision_state {
                GoalUserDecisionStateDto::AcceptedWithException {
                    exception_evidence_set,
                } => encode_exceptions(exception_evidence_set),
                GoalUserDecisionStateDto::Unaccepted | GoalUserDecisionStateDto::Accepted => {
                    String::new()
                }
            };
            connection
                .execute(
                    "UPDATE goals SET user_decision_kind=?2, user_decision_exceptions=?3,
                        updated_at_ms=?4 WHERE goal_id=?1",
                    sqlite::params![
                        goal_id,
                        user_decision_state.kind_name(),
                        exceptions,
                        int(occurred_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_goal_row(connection, &goal_id)
        })
    }

    fn attach_goal_child(
        &self,
        input: AttachGoalChildInputDto,
    ) -> DtoResult<GoalParentLinkRecordDto> {
        let link = GoalParentLinkRecordDto {
            parent_goal_id: input.parent_goal_id,
            child_goal_id: input.child_goal_id,
            child_revision_at_link: input.child_revision_at_link,
            canonical_link_digest: input.canonical_link_digest,
            created_at_ms: input.created_at_ms,
        };
        link.validate()?;
        write(self, |connection| {
            let _parent = load_goal_row(connection, &link.parent_goal_id)?;
            let _child = load_goal_row(connection, &link.child_goal_id)?;
            if let Some(existing) =
                load_parent_link_optional(connection, &link.parent_goal_id, &link.child_goal_id)?
            {
                return if existing == link {
                    Ok(existing)
                } else {
                    Err(conflict(
                        "goal_cycle_detected",
                        "the parent link is already bound to different content",
                    ))
                };
            }
            if parent_of(connection, &link.child_goal_id)?.is_some() {
                return Err(conflict(
                    "goal_cycle_detected",
                    "a Goal holds exactly one obligatory parent link",
                ));
            }
            let parent_depth = tree_depth(connection, &link.parent_goal_id)?;
            let direct_children = count_children(connection, &link.parent_goal_id)?;
            validate_goal_tree_attachment(
                u32::try_from(parent_depth).map_err(|_| codec_error("invalid Goal depth"))?,
                u32::try_from(direct_children).map_err(|_| codec_error("invalid child count"))?,
            )?;
            let mut ancestor = link.parent_goal_id.clone();
            let mut visited: Vec<String> = Vec::new();
            while let Some(parent) = parent_of(connection, &ancestor)? {
                if parent == link.child_goal_id {
                    return Err(conflict(
                        "goal_cycle_detected",
                        "the child is an ancestor of the requested parent",
                    ));
                }
                if visited.contains(&parent) {
                    return Err(conflict(
                        "goal_cycle_detected",
                        "the stored Goal graph holds a cycle",
                    ));
                }
                visited.push(ancestor);
                ancestor = parent;
            }
            connection
                .execute(
                    "INSERT INTO goal_parent_links(parent_goal_id, child_goal_id,
                        child_revision_at_link, canonical_link_digest, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    sqlite::params![
                        link.parent_goal_id,
                        link.child_goal_id,
                        int(
                            link.child_revision_at_link,
                            "revision is outside the SQLite range"
                        )?,
                        link.canonical_link_digest,
                        int(link.created_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            Ok(link)
        })
    }

    fn list_goal_children(
        &self,
        parent_goal_id: String,
    ) -> DtoResult<Vec<GoalParentLinkRecordDto>> {
        let connection = self.connection()?;
        list_parent_links(&connection, &parent_goal_id)
    }

    fn load_goal_tree_depth(&self, goal_id: String) -> DtoResult<u64> {
        let connection = self.connection()?;
        let _ = load_goal_row(&connection, &goal_id)?;
        tree_depth(&connection, &goal_id)
    }

    fn create_goal_session_link(
        &self,
        input: CreateGoalSessionLinkInputDto,
    ) -> DtoResult<GoalSessionLinkRecordDto> {
        let link = input.link;
        link.validate()?;
        write(self, |connection| {
            let goal = load_goal_row(connection, &link.project_goal_id)?;
            if !goal.scope.is_project() {
                return Err(not_found(
                    "goal_not_active",
                    "session links attach project Goals only",
                ));
            }
            if let Some(existing) = connection
                .query_row(
                    &format!(
                        "SELECT {SESSION_LINK_COLUMNS} FROM goal_session_links WHERE link_id=?1"
                    ),
                    [link.link_id.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            u64_column(row, 3)?,
                            row.get::<_, String>(4)?,
                            u64_column(row, 5)?,
                        ))
                    },
                )
                .optional()
                .map_err(storage_error)?
            {
                let existing = session_link_from_columns(&existing);
                return if existing == link {
                    Ok(existing)
                } else {
                    Err(goal_revision_conflict())
                };
            }
            let duplicate: Option<String> = connection
                .query_row(
                    "SELECT link_id FROM goal_session_links
                     WHERE project_goal_id=?1 AND session_id=?2",
                    sqlite::params![link.project_goal_id, link.session_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            if duplicate.is_some() {
                return Err(goal_revision_conflict());
            }
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM goal_session_links WHERE project_goal_id=?1",
                    [link.project_goal_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let count = u64::try_from(count).map_err(|_| codec_error("invalid link count"))?;
            if count + 1 > GOAL_MAX_SESSION_LINKS_PER_PROJECT_GOAL as u64 {
                return Err(conflict(
                    "goal_session_link_limit_exceeded",
                    "the session link bound of one project Goal is exceeded",
                ));
            }
            create_goal_session_link_row(connection, &link)?;
            Ok(link)
        })
    }

    fn load_goal_session_link(&self, link_id: String) -> DtoResult<GoalSessionLinkRecordDto> {
        let connection = self.connection()?;
        connection
            .query_row(
                &format!("SELECT {SESSION_LINK_COLUMNS} FROM goal_session_links WHERE link_id=?1"),
                [link_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        u64_column(row, 3)?,
                        row.get::<_, String>(4)?,
                        u64_column(row, 5)?,
                    ))
                },
            )
            .map_err(goal_not_active_or_storage)
            .map(|row| session_link_from_columns(&row))
    }

    fn load_goal_session_links(
        &self,
        project_goal_id: String,
    ) -> DtoResult<Vec<GoalSessionLinkRecordDto>> {
        let connection = self.connection()?;
        list_session_links(&connection, &project_goal_id)
    }

    fn count_goals_in_project(&self, project_id: String) -> DtoResult<u64> {
        let connection = self.connection()?;
        count_goals_in_column(&connection, "project_id", &project_id)
    }

    fn count_goals_in_session(&self, session_id: String) -> DtoResult<u64> {
        let connection = self.connection()?;
        count_goals_in_column(&connection, "session_id", &session_id)
    }
}

fn goal_gate_unavailable_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        not_found(
            "goal_gate_unavailable",
            "the requested durable gate record does not exist",
        )
    } else {
        storage_error(error)
    }
}

/// Encodes one typed gate definition.
fn encode_gate_definition(definition: &GoalGateDefinitionDto) -> String {
    match definition {
        GoalGateDefinitionDto::Reference {
            evidence_contract_revision,
            accepted_reference_kinds,
        } => {
            let mut fields = vec![evidence_contract_revision.to_string()];
            fields.extend(
                accepted_reference_kinds
                    .iter()
                    .map(|kind| kind.name().to_owned()),
            );
            encode_records(&[fields])
        }
        GoalGateDefinitionDto::Executable {
            template_id,
            template_revision,
        } => encode_records(&[vec![template_id.clone(), template_revision.to_string()]]),
    }
}

/// Decodes one typed gate definition.
fn decode_gate_definition(kind: &str, encoded: &str) -> DtoResult<GoalGateDefinitionDto> {
    let mut records = decode_records(encoded)?;
    if records.len() != 1 {
        return Err(codec_error("persisted gate definition is malformed"));
    }
    let fields = records.remove(0);
    match kind {
        "reference" => {
            if fields.is_empty() {
                return Err(codec_error("persisted reference gate is malformed"));
            }
            Ok(GoalGateDefinitionDto::Reference {
                evidence_contract_revision: parse_u64(&fields[0])?,
                accepted_reference_kinds: fields[1..]
                    .iter()
                    .map(|name| GoalEvidenceKindDto::parse(name))
                    .collect::<DtoResult<Vec<_>>>()?,
            })
        }
        "executable" => {
            if fields.len() != 2 {
                return Err(codec_error("persisted executable gate is malformed"));
            }
            Ok(GoalGateDefinitionDto::Executable {
                template_id: fields[0].clone(),
                template_revision: parse_u64(&fields[1])?,
            })
        }
        _ => Err(codec_error("persisted gate definition kind is malformed")),
    }
}

type GateRow = (String, String, String, String, u64, String, u64);

fn gate_from_columns(row: &GateRow) -> DtoResult<GoalGateRecordDto> {
    Ok(GoalGateRecordDto {
        gate_id: row.0.clone(),
        goal_id: row.1.clone(),
        definition: decode_gate_definition(&row.2, &row.3)?,
        revision: row.4,
        canonical_revision_digest: row.5.clone(),
        created_at_ms: row.6,
    })
}

const GATE_COLUMNS: &str = "gate_id, goal_id, definition_kind, definition, revision, \
     canonical_revision_digest, created_at_ms";

fn load_gate_row(connection: &sqlite::Connection, gate_id: &str) -> DtoResult<GoalGateRecordDto> {
    connection
        .query_row(
            &format!("SELECT {GATE_COLUMNS} FROM goal_gates WHERE gate_id=?1"),
            [gate_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    u64_column(row, 4)?,
                    row.get::<_, String>(5)?,
                    u64_column(row, 6)?,
                ))
            },
        )
        .map_err(goal_gate_unavailable_or_storage)
        .and_then(|row| gate_from_columns(&row))
}

fn load_gate_revision_optional(
    connection: &sqlite::Connection,
    gate_id: &str,
    revision: u64,
) -> DtoResult<Option<(String, String, String)>> {
    connection
        .query_row(
            "SELECT definition_kind, definition, canonical_revision_digest
             FROM goal_gate_revisions WHERE gate_id=?1 AND revision=?2",
            sqlite::params![
                gate_id,
                int(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
}

fn insert_gate_revision(
    connection: &sqlite::Connection,
    gate_id: &str,
    revision: u64,
    definition: &GoalGateDefinitionDto,
    canonical_revision_digest: &str,
    created_at_ms: u64,
) -> DtoResult<()> {
    let kind = if definition.is_executable() {
        "executable"
    } else {
        "reference"
    };
    connection
        .execute(
            "INSERT INTO goal_gate_revisions(gate_id, revision, definition_kind, definition,
                canonical_revision_digest, created_at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            sqlite::params![
                gate_id,
                int(revision, "revision is outside the SQLite range")?,
                kind,
                encode_gate_definition(definition),
                canonical_revision_digest,
                int(created_at_ms, "timestamp is outside the SQLite range")?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

type TemplateRow = (
    String,
    u64,
    String,
    String,
    String,
    String,
    bool,
    String,
    String,
    Option<String>,
    bool,
    String,
    u64,
    u64,
);

fn template_from_columns(row: &TemplateRow) -> DtoResult<GoalGateTemplateRecordDto> {
    let scope = match row.2.as_str() {
        "project" => GoalTemplateScopeDto::Project {
            project_id: row.3.clone(),
        },
        "goal" => GoalTemplateScopeDto::Goal {
            goal_id: row.3.clone(),
        },
        "session" => GoalTemplateScopeDto::Session {
            session_id: row.3.clone(),
        },
        _ => return Err(codec_error("persisted template scope is malformed")),
    };
    Ok(GoalGateTemplateRecordDto {
        template_id: row.0.clone(),
        revision: row.1,
        scope,
        capability_reference: row.4.clone(),
        input_family: intention_storage::goal_repo::GoalGateInputFamilyDto::parse(&row.5)?,
        requires_confirmation: row.6,
        lifecycle_state: GoalTemplateLifecycleStateDto::parse(&row.7)?,
        provenance_kind: GoalTemplateProvenanceKindDto::parse(&row.8)?,
        provenance_draft_id: row.9.clone(),
        provenance_accepted_by_user: row.10,
        canonical_digest: row.11.clone(),
        created_at_ms: row.12,
    })
}

const TEMPLATE_COLUMNS: &str = "template_id, revision, scope_kind, owner_id, capability_reference, \
     input_family, requires_confirmation, lifecycle_state, provenance_kind, provenance_draft_id, \
     provenance_accepted_by_user, canonical_digest, created_at_ms, updated_at_ms";

fn load_template_row(
    connection: &sqlite::Connection,
    template_id: &str,
    revision: u64,
) -> DtoResult<GoalGateTemplateRecordDto> {
    connection
        .query_row(
            &format!(
                "SELECT {TEMPLATE_COLUMNS} FROM goal_gate_templates
                 WHERE template_id=?1 AND revision=?2"
            ),
            sqlite::params![
                template_id,
                int(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    u64_column(row, 1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    bool_column(row, 6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    bool_column(row, 10)?,
                    row.get::<_, String>(11)?,
                    u64_column(row, 12)?,
                    u64_column(row, 13)?,
                ))
            },
        )
        .map_err(goal_gate_unavailable_or_storage)
        .and_then(|row| template_from_columns(&row))
}

impl GoalGateRepositoryDto for SqliteStorageRepository {
    fn create_goal_gate(&self, input: CreateGoalGateInputDto) -> DtoResult<GoalGateRecordDto> {
        let gate = input.gate;
        gate.validate()?;
        write(self, |connection| {
            let goal_exists: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM goals WHERE goal_id=?1",
                    [gate.goal_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if goal_exists == 0 {
                return Err(not_found(
                    "goal_gate_unavailable",
                    "the owning Goal of the gate does not exist",
                ));
            }
            let existing: Option<String> = connection
                .query_row(
                    "SELECT gate_id FROM goal_gates WHERE gate_id=?1",
                    [gate.gate_id.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            if existing.is_some() {
                let stored = load_gate_row(connection, &gate.gate_id)?;
                return if stored == gate {
                    Ok(stored)
                } else {
                    Err(conflict(
                        "goal_gate_unavailable",
                        "the gate identity is already bound to different content",
                    ))
                };
            }
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM goal_gates WHERE goal_id=?1",
                    [gate.goal_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let count = u64::try_from(count).map_err(|_| codec_error("invalid gate count"))?;
            if count + 1 > GOAL_MAX_GATES_PER_GOAL as u64 {
                return Err(conflict(
                    "goal_gate_limit_exceeded",
                    "the gate bound of one Goal is exceeded",
                ));
            }
            let kind = if gate.definition.is_executable() {
                "executable"
            } else {
                "reference"
            };
            connection
                .execute(
                    "INSERT INTO goal_gates(gate_id, goal_id, revision, definition_kind,
                        definition, canonical_revision_digest, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    sqlite::params![
                        gate.gate_id,
                        gate.goal_id,
                        int(gate.revision, "revision is outside the SQLite range")?,
                        kind,
                        encode_gate_definition(&gate.definition),
                        gate.canonical_revision_digest,
                        int(gate.created_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            insert_gate_revision(
                connection,
                &gate.gate_id,
                gate.revision,
                &gate.definition,
                &gate.canonical_revision_digest,
                gate.created_at_ms,
            )?;
            load_gate_row(connection, &gate.gate_id)
        })
    }

    fn load_goal_gate(&self, gate_id: String) -> DtoResult<GoalGateRecordDto> {
        let connection = self.connection()?;
        load_gate_row(&connection, &gate_id)
    }

    fn append_goal_gate_revision(
        &self,
        input: AppendGoalGateRevisionInputDto,
    ) -> DtoResult<GoalGateRecordDto> {
        input.validate()?;
        let gate_id = input.gate_id;
        let expected_revision = input.expected_revision;
        let definition = input.definition;
        let canonical_revision_digest = input.canonical_revision_digest;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let stored = load_gate_row(connection, &gate_id)?;
            if stored.revision != expected_revision {
                return Err(goal_revision_conflict());
            }
            let next = expected_revision
                .checked_add(1)
                .ok_or_else(goal_revision_conflict)?;
            let encoded = encode_gate_definition(&definition);
            let kind = if definition.is_executable() {
                "executable"
            } else {
                "reference"
            };
            match load_gate_revision_optional(connection, &gate_id, next)? {
                Some((existing_kind, existing_definition, existing_digest))
                    if existing_kind != kind
                        || existing_definition != encoded
                        || existing_digest != canonical_revision_digest =>
                {
                    return Err(goal_revision_conflict());
                }
                Some(_) => {}
                None => insert_gate_revision(
                    connection,
                    &gate_id,
                    next,
                    &definition,
                    &canonical_revision_digest,
                    occurred_at_ms,
                )?,
            }
            connection
                .execute(
                    "UPDATE goal_gates SET revision=?2, definition_kind=?3, definition=?4,
                        canonical_revision_digest=?5 WHERE gate_id=?1",
                    sqlite::params![
                        gate_id,
                        int(next, "revision is outside the SQLite range")?,
                        kind,
                        encoded,
                        canonical_revision_digest,
                    ],
                )
                .map_err(storage_error)?;
            load_gate_row(connection, &gate_id)
        })
    }

    fn create_goal_gate_template(
        &self,
        input: GoalGateTemplateRecordDto,
    ) -> DtoResult<GoalGateTemplateRecordDto> {
        input.validate()?;
        let template = input;
        write(self, |connection| {
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO goal_gate_templates(template_id, revision, scope_kind,
                        owner_id, capability_reference, input_family, requires_confirmation,
                        lifecycle_state, provenance_kind, provenance_draft_id,
                        provenance_accepted_by_user, canonical_digest, created_at_ms, updated_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
                    sqlite::params![
                        template.template_id,
                        int(template.revision, "revision is outside the SQLite range")?,
                        template.scope.kind_name(),
                        template.scope.owner_id(),
                        template.capability_reference,
                        template.input_family.name(),
                        template.requires_confirmation,
                        template.lifecycle_state.name(),
                        template.provenance_kind.name(),
                        template.provenance_draft_id,
                        template.provenance_accepted_by_user,
                        template.canonical_digest,
                        int(
                            template.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored =
                    load_template_row(connection, &template.template_id, template.revision)?;
                if stored != template {
                    return Err(conflict(
                        "goal_gate_unavailable",
                        "the template revision is already bound to different content",
                    ));
                }
                return Ok(stored);
            }
            Ok(template)
        })
    }

    fn load_goal_gate_template(
        &self,
        template_id: String,
        revision: u64,
    ) -> DtoResult<GoalGateTemplateRecordDto> {
        let connection = self.connection()?;
        load_template_row(&connection, &template_id, revision)
    }

    fn transition_goal_gate_template_lifecycle(
        &self,
        input: TransitionGoalGateTemplateInputDto,
    ) -> DtoResult<GoalGateTemplateRecordDto> {
        if input.revision == 0 {
            return Err(ErrorDto::validation(
                "goal_gate_unavailable",
                "a template transition names an exact nonzero revision",
            ));
        }
        let template_id = input.template_id;
        let revision = input.revision;
        let requested = input.lifecycle_state;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let stored = load_template_row(connection, &template_id, revision)?;
            if stored.lifecycle_state == requested {
                return Ok(stored);
            }
            let allowed = matches!(
                (stored.lifecycle_state, requested),
                (
                    GoalTemplateLifecycleStateDto::Enabled,
                    GoalTemplateLifecycleStateDto::Archived
                ) | (
                    GoalTemplateLifecycleStateDto::Archived,
                    GoalTemplateLifecycleStateDto::Enabled
                )
            );
            if !allowed {
                return Err(conflict(
                    "goal_gate_unavailable",
                    "the gate template lifecycle transition is not permitted",
                ));
            }
            connection
                .execute(
                    "UPDATE goal_gate_templates SET lifecycle_state=?3, updated_at_ms=?4
                     WHERE template_id=?1 AND revision=?2",
                    sqlite::params![
                        template_id,
                        int(revision, "revision is outside the SQLite range")?,
                        requested.name(),
                        int(occurred_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_template_row(connection, &template_id, revision)
        })
    }

    fn record_goal_gate_result(
        &self,
        input: GoalGateResultRecordDto,
    ) -> DtoResult<GoalGateResultRecordDto> {
        input.validate()?;
        let result = input;
        write(self, |connection| {
            let evidence = result
                .evidence
                .as_ref()
                .map(|reference| encode_evidence(std::slice::from_ref(reference)));
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO goal_gate_results(gate_id, gate_revision,
                        producing_run_id, outcome_kind, disposition, evidence,
                        canonical_result_digest, occurred_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    sqlite::params![
                        result.gate_id,
                        int(result.gate_revision, "revision is outside the SQLite range")?,
                        result.producing_run_id,
                        result.outcome_kind.name(),
                        result.disposition.name(),
                        evidence,
                        result.canonical_result_digest,
                        int(
                            result.occurred_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored =
                    load_gate_result_row(connection, &result.gate_id, result.gate_revision)?;
                if stored != result {
                    return Err(goal_revision_conflict());
                }
                return Ok(stored);
            }
            Ok(result)
        })
    }

    fn load_goal_gate_result(
        &self,
        gate_id: String,
        gate_revision: u64,
    ) -> DtoResult<GoalGateResultRecordDto> {
        let connection = self.connection()?;
        load_gate_result_row(&connection, &gate_id, gate_revision)
    }
}

type GateResultRow = (
    String,
    u64,
    String,
    String,
    String,
    Option<String>,
    String,
    u64,
);

fn gate_result_from_columns(row: &GateResultRow) -> DtoResult<GoalGateResultRecordDto> {
    let evidence = match &row.5 {
        Some(encoded) => {
            let mut references = decode_evidence(encoded)?;
            if references.len() != 1 {
                return Err(codec_error("persisted gate result evidence is malformed"));
            }
            Some(references.remove(0))
        }
        None => None,
    };
    Ok(GoalGateResultRecordDto {
        gate_id: row.0.clone(),
        gate_revision: row.1,
        producing_run_id: row.2.clone(),
        outcome_kind: intention_storage::goal_repo::GoalGateOutcomeKindDto::parse(&row.3)?,
        disposition: intention_storage::goal_repo::GoalGateOutcomeDispositionDto::parse(&row.4)?,
        evidence,
        canonical_result_digest: row.6.clone(),
        occurred_at_ms: row.7,
    })
}

fn load_gate_result_row(
    connection: &sqlite::Connection,
    gate_id: &str,
    gate_revision: u64,
) -> DtoResult<GoalGateResultRecordDto> {
    connection
        .query_row(
            "SELECT gate_id, gate_revision, producing_run_id, outcome_kind, disposition,
                    evidence, canonical_result_digest, occurred_at_ms
             FROM goal_gate_results WHERE gate_id=?1 AND gate_revision=?2",
            sqlite::params![
                gate_id,
                int(gate_revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    u64_column(row, 1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    u64_column(row, 7)?,
                ))
            },
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                not_found(
                    "goal_gate_unavailable",
                    "the requested durable gate result does not exist",
                )
            } else {
                storage_error(error)
            }
        })
        .and_then(|row| gate_result_from_columns(&row))
}

fn skill_reference_unavailable_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        not_found(
            "skill_reference_unavailable",
            "the requested durable Skill card revision does not exist",
        )
    } else {
        storage_error(error)
    }
}

fn delegation_role_invalid_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        not_found(
            "delegation_role_invalid",
            "the requested durable role card revision does not exist",
        )
    } else {
        storage_error(error)
    }
}

fn memory_replacement_conflict() -> ErrorDto {
    conflict(
        "memory_replacement_conflict",
        "the memory replacement or rollback relation is invalid or its target revision is absent",
    )
}

/// Splits one durable record scope into its discriminator and owner identity.
fn record_scope_columns(scope: &GoalRecordScopeDto) -> (&'static str, String) {
    match scope {
        GoalRecordScopeDto::Project { project_id } => ("project", project_id.clone()),
        GoalRecordScopeDto::Goal { goal_id } => ("goal", goal_id.clone()),
        GoalRecordScopeDto::Session { session_id } => ("session", session_id.clone()),
    }
}

/// Rebuilds one durable record scope from its persisted columns.
fn record_scope_from_columns(kind: &str, owner_id: &str) -> DtoResult<GoalRecordScopeDto> {
    match kind {
        "project" => Ok(GoalRecordScopeDto::Project {
            project_id: owner_id.to_owned(),
        }),
        "goal" => Ok(GoalRecordScopeDto::Goal {
            goal_id: owner_id.to_owned(),
        }),
        "session" => Ok(GoalRecordScopeDto::Session {
            session_id: owner_id.to_owned(),
        }),
        _ => Err(codec_error("persisted record scope is malformed")),
    }
}

type MemoryRow = (
    String,
    u64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    u64,
);

fn memory_card_from_columns(row: &MemoryRow) -> DtoResult<GoalMemoryCardRecordDto> {
    Ok(GoalMemoryCardRecordDto {
        record_id: row.0.clone(),
        revision: row.1,
        kind: intention_storage::goal_repo::MemoryKindDto::parse(&row.2)?,
        scope: record_scope_from_columns(&row.3, &row.4)?,
        title: row.5.clone(),
        safe_purpose: row.6.clone(),
        retained_content_reference: row.7.clone(),
        canonical_digest: row.8.clone(),
        created_at_ms: row.9,
    })
}

const MEMORY_COLUMNS: &str = "record_id, revision, kind, scope_kind, owner_id, title, \
     safe_purpose, retained_content_reference, canonical_digest, created_at_ms";

fn load_memory_card_optional(
    connection: &sqlite::Connection,
    record_id: &str,
    revision: u64,
) -> DtoResult<Option<GoalMemoryCardRecordDto>> {
    connection
        .query_row(
            &format!(
                "SELECT {MEMORY_COLUMNS} FROM goal_memory_cards
                 WHERE record_id=?1 AND revision=?2"
            ),
            sqlite::params![
                record_id,
                int(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    u64_column(row, 1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    u64_column(row, 9)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .map(|row| memory_card_from_columns(&row))
        .transpose()
}

fn load_memory_card(
    connection: &sqlite::Connection,
    record_id: &str,
    revision: u64,
) -> DtoResult<GoalMemoryCardRecordDto> {
    load_memory_card_optional(connection, record_id, revision)?.ok_or_else(|| {
        not_found(
            "memory_reference_unavailable",
            "the requested durable memory card revision does not exist",
        )
    })
}

/// Stores one validated memory card revision with its active identity bound.
fn store_memory_card(
    connection: &sqlite::Connection,
    card: &GoalMemoryCardRecordDto,
) -> DtoResult<GoalMemoryCardRecordDto> {
    card.validate()?;
    if let Some(existing) = load_memory_card_optional(connection, &card.record_id, card.revision)? {
        return if existing == *card {
            Ok(existing)
        } else {
            Err(conflict(
                "memory_reference_unavailable",
                "the memory card revision is already bound to different content",
            ))
        };
    }
    let (scope_kind, owner_id) = record_scope_columns(&card.scope);
    let identity_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM goal_memory_cards
             WHERE scope_kind=?1 AND owner_id=?2 AND record_id=?3",
            sqlite::params![scope_kind, owner_id, card.record_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if identity_count == 0 {
        let active: i64 = connection
            .query_row(
                "SELECT COUNT(DISTINCT record_id) FROM goal_memory_cards
                 WHERE scope_kind=?1 AND owner_id=?2",
                sqlite::params![scope_kind, owner_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let active = u64::try_from(active).map_err(|_| codec_error("invalid memory count"))?;
        if active + 1 > GOAL_MAX_ACTIVE_MEMORY_CARDS {
            return Err(conflict(
                "memory_entry_limit_exceeded",
                "the active memory card bound of the owner scope is exceeded",
            ));
        }
    }
    connection
        .execute(
            "INSERT INTO goal_memory_cards(record_id, revision, kind, scope_kind, owner_id,
                title, safe_purpose, retained_content_reference, canonical_digest, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            sqlite::params![
                card.record_id,
                int(card.revision, "revision is outside the SQLite range")?,
                card.kind.name(),
                scope_kind,
                owner_id,
                card.title,
                card.safe_purpose,
                card.retained_content_reference,
                card.canonical_digest,
                int(card.created_at_ms, "timestamp is outside the SQLite range")?,
            ],
        )
        .map_err(storage_error)?;
    load_memory_card(connection, &card.record_id, card.revision)
}

fn insert_memory_relation(
    connection: &sqlite::Connection,
    relation_kind: &str,
    source_record_id: &str,
    source_revision: u64,
    target: &GoalMemoryCardRecordDto,
    occurred_at_ms: u64,
) -> DtoResult<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO goal_memory_relations(relation_kind, source_record_id,
                source_revision, target_record_id, target_revision, occurred_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            sqlite::params![
                relation_kind,
                source_record_id,
                int(source_revision, "revision is outside the SQLite range")?,
                target.record_id,
                int(target.revision, "revision is outside the SQLite range")?,
                int(occurred_at_ms, "timestamp is outside the SQLite range")?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

type SkillRow = (
    String,
    u64,
    String,
    String,
    String,
    String,
    String,
    String,
    u64,
);

fn skill_card_from_columns(row: &SkillRow) -> DtoResult<GoalSkillCardRecordDto> {
    Ok(GoalSkillCardRecordDto {
        skill_id: row.0.clone(),
        revision: row.1,
        canonical_name: row.2.clone(),
        description: row.3.clone(),
        owner_scope: record_scope_from_columns(&row.4, &row.5)?,
        content_reference: row.6.clone(),
        canonical_digest: row.7.clone(),
        created_at_ms: row.8,
    })
}

const SKILL_COLUMNS: &str = "skill_id, revision, canonical_name, description, scope_kind, \
     owner_id, content_reference, canonical_digest, created_at_ms";

fn load_skill_card_row(
    connection: &sqlite::Connection,
    skill_id: &str,
    revision: u64,
) -> DtoResult<GoalSkillCardRecordDto> {
    connection
        .query_row(
            &format!(
                "SELECT {SKILL_COLUMNS} FROM goal_skill_cards WHERE skill_id=?1 AND revision=?2"
            ),
            sqlite::params![
                skill_id,
                int(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    u64_column(row, 1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    u64_column(row, 8)?,
                ))
            },
        )
        .map_err(skill_reference_unavailable_or_storage)
        .and_then(|row| skill_card_from_columns(&row))
}

type RoleRow = (
    String,
    u64,
    String,
    String,
    String,
    String,
    u64,
    u64,
    String,
    u64,
);

fn role_card_from_columns(row: &RoleRow) -> DtoResult<GoalRoleCardRecordDto> {
    Ok(GoalRoleCardRecordDto {
        role_id: row.0.clone(),
        revision: row.1,
        canonical_name: row.2.clone(),
        task: row.3.clone(),
        permitted_class: intention_storage::goal_repo::GoalRoleClassDto::parse(&row.4)?,
        tool_subset: decode_items(&row.5)?,
        context_limit_bytes: row.6,
        result_limit_bytes: row.7,
        canonical_digest: row.8.clone(),
        created_at_ms: row.9,
    })
}

const ROLE_COLUMNS: &str = "role_id, revision, canonical_name, task, permitted_class, \
     tool_subset, context_limit_bytes, result_limit_bytes, canonical_digest, created_at_ms";

fn load_role_card_row(
    connection: &sqlite::Connection,
    role_id: &str,
    revision: u64,
) -> DtoResult<GoalRoleCardRecordDto> {
    connection
        .query_row(
            &format!("SELECT {ROLE_COLUMNS} FROM goal_role_cards WHERE role_id=?1 AND revision=?2"),
            sqlite::params![
                role_id,
                int(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    u64_column(row, 1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    u64_column(row, 6)?,
                    u64_column(row, 7)?,
                    row.get::<_, String>(8)?,
                    u64_column(row, 9)?,
                ))
            },
        )
        .map_err(delegation_role_invalid_or_storage)
        .and_then(|row| role_card_from_columns(&row))
}

/// Counts the distinct active memory card identities of one owner scope.
fn count_active_memory_cards(
    connection: &sqlite::Connection,
    scope: &GoalRecordScopeDto,
) -> DtoResult<u64> {
    let (scope_kind, owner_id) = record_scope_columns(scope);
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT record_id) FROM goal_memory_cards
             WHERE scope_kind=?1 AND owner_id=?2",
            sqlite::params![scope_kind, owner_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    u64::try_from(count).map_err(|_| codec_error("invalid memory count"))
}

impl GoalCardRepositoryDto for SqliteStorageRepository {
    fn store_goal_memory_card(
        &self,
        input: GoalMemoryCardRecordDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        write(self, |connection| store_memory_card(connection, &input))
    }

    fn load_goal_memory_card(
        &self,
        record_id: String,
        revision: u64,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        let connection = self.connection()?;
        load_memory_card(&connection, &record_id, revision)
    }

    fn replace_goal_memory_card(
        &self,
        input: GoalMemoryCardReplacementInputDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        input.validate()?;
        let replaced_record_id = input.replaced_record_id;
        let replaced_revision = input.replaced_revision;
        let replacement = input.replacement;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            load_memory_card_optional(connection, &replaced_record_id, replaced_revision)?
                .ok_or_else(memory_replacement_conflict)?;
            let stored = store_memory_card(connection, &replacement)?;
            insert_memory_relation(
                connection,
                "replacement",
                &replaced_record_id,
                replaced_revision,
                &stored,
                occurred_at_ms,
            )?;
            Ok(stored)
        })
    }

    fn rollback_goal_memory_card(
        &self,
        input: GoalMemoryCardRollbackInputDto,
    ) -> DtoResult<GoalMemoryCardRecordDto> {
        input.validate()?;
        let restored_record_id = input.restored_record_id;
        let restored_revision = input.restored_revision;
        let replacement = input.replacement;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            load_memory_card_optional(connection, &restored_record_id, restored_revision)?
                .ok_or_else(memory_replacement_conflict)?;
            let stored = store_memory_card(connection, &replacement)?;
            insert_memory_relation(
                connection,
                "rollback",
                &restored_record_id,
                restored_revision,
                &stored,
                occurred_at_ms,
            )?;
            Ok(stored)
        })
    }

    fn count_active_goal_memory_cards(&self, scope: GoalRecordScopeDto) -> DtoResult<u64> {
        let connection = self.connection()?;
        count_active_memory_cards(&connection, &scope)
    }

    fn store_goal_skill_card(
        &self,
        input: GoalSkillCardRecordDto,
    ) -> DtoResult<GoalSkillCardRecordDto> {
        input.validate()?;
        let card = input;
        write(self, |connection| {
            let (scope_kind, owner_id) = record_scope_columns(&card.owner_scope);
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO goal_skill_cards(skill_id, revision, canonical_name,
                        description, scope_kind, owner_id, content_reference, canonical_digest,
                        created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    sqlite::params![
                        card.skill_id,
                        int(card.revision, "revision is outside the SQLite range")?,
                        card.canonical_name,
                        card.description,
                        scope_kind,
                        owner_id,
                        card.content_reference,
                        card.canonical_digest,
                        int(card.created_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored = load_skill_card_row(connection, &card.skill_id, card.revision)?;
                if stored != card {
                    return Err(conflict(
                        "skill_reference_unavailable",
                        "the Skill card revision is already bound to different content",
                    ));
                }
                return Ok(stored);
            }
            Ok(card)
        })
    }

    fn load_goal_skill_card(
        &self,
        skill_id: String,
        revision: u64,
    ) -> DtoResult<GoalSkillCardRecordDto> {
        let connection = self.connection()?;
        load_skill_card_row(&connection, &skill_id, revision)
    }

    fn store_goal_role_card(
        &self,
        input: GoalRoleCardRecordDto,
    ) -> DtoResult<GoalRoleCardRecordDto> {
        input.validate()?;
        let card = input;
        write(self, |connection| {
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO goal_role_cards(role_id, revision, canonical_name, task,
                        permitted_class, tool_subset, context_limit_bytes, result_limit_bytes,
                        canonical_digest, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    sqlite::params![
                        card.role_id,
                        int(card.revision, "revision is outside the SQLite range")?,
                        card.canonical_name,
                        card.task,
                        card.permitted_class.name(),
                        encode_items(&card.tool_subset),
                        int(
                            card.context_limit_bytes,
                            "limit is outside the SQLite range"
                        )?,
                        int(card.result_limit_bytes, "limit is outside the SQLite range")?,
                        card.canonical_digest,
                        int(card.created_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored = load_role_card_row(connection, &card.role_id, card.revision)?;
                if stored != card {
                    return Err(conflict(
                        "delegation_role_invalid",
                        "the role card revision is already bound to different content",
                    ));
                }
                return Ok(stored);
            }
            Ok(card)
        })
    }

    fn load_goal_role_card(
        &self,
        role_id: String,
        revision: u64,
    ) -> DtoResult<GoalRoleCardRecordDto> {
        let connection = self.connection()?;
        load_role_card_row(&connection, &role_id, revision)
    }
}

fn refinement_draft_conflict() -> ErrorDto {
    conflict(
        "refinement_draft_conflict",
        "the pending refinement draft is already bound to an unequal proposal",
    )
}

fn compaction_history_unavailable() -> ErrorDto {
    conflict(
        "compaction_history_unavailable",
        "no completed uncompacted range is available or the range does not extend the working form",
    )
}

/// Encodes one ordered refinement edit set.
fn encode_refinement_edits(edits: &[RefinementEditRecordDto]) -> String {
    encode_records(
        &edits
            .iter()
            .map(|edit| {
                vec![
                    edit.kind.name().to_owned(),
                    edit.evidence.evidence_id.clone(),
                    edit.evidence.revision.to_string(),
                    edit.evidence.kind.name().to_owned(),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

/// Decodes one ordered refinement edit set.
fn decode_refinement_edits(encoded: &str) -> DtoResult<Vec<RefinementEditRecordDto>> {
    decode_records(encoded)?
        .into_iter()
        .map(|fields| {
            if fields.len() != 4 {
                return Err(codec_error("persisted refinement edit is malformed"));
            }
            Ok(RefinementEditRecordDto {
                kind: intention_storage::goal_repo::RefinementEditKindDto::parse(&fields[0])?,
                evidence: GoalEvidenceReferenceDto {
                    evidence_id: fields[1].clone(),
                    revision: parse_u64(&fields[2])?,
                    kind: GoalEvidenceKindDto::parse(&fields[3])?,
                },
            })
        })
        .collect()
}

type DraftRow = (
    String,
    String,
    String,
    String,
    u64,
    String,
    u64,
    String,
    String,
    String,
    String,
    String,
    u64,
    u64,
    Option<u64>,
);

fn draft_from_columns(row: &DraftRow) -> DtoResult<RefinementDraftRecordDto> {
    Ok(RefinementDraftRecordDto {
        draft_id: row.0.clone(),
        source_run_id: row.1.clone(),
        leading_goal_id: row.2.clone(),
        milestone: intention_storage::goal_repo::GoalMilestoneDto::parse(&row.3)?,
        base_goal_revision: row.4,
        base_record_reference: row.5.clone(),
        base_record_revision: row.6,
        edits: decode_refinement_edits(&row.7)?,
        evidence_references: decode_evidence(&row.8)?,
        safe_rationale: row.9.clone(),
        canonical_digest: row.10.clone(),
        state: RefinementDraftStateDto::parse(&row.11)?,
        coalesced_evidence_count: row.12,
        created_at_ms: row.13,
        decided_at_ms: row.14,
    })
}

const DRAFT_COLUMNS: &str = "draft_id, source_run_id, leading_goal_id, milestone, \
     base_goal_revision, base_record_reference, base_record_revision, edits, \
     evidence_references, safe_rationale, canonical_digest, state, coalesced_evidence_count, \
     created_at_ms, decided_at_ms";

fn draft_row_columns(row: &sqlite::Row<'_>) -> Result<DraftRow, sqlite::Error> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        u64_column(row, 4)?,
        row.get::<_, String>(5)?,
        u64_column(row, 6)?,
        row.get::<_, String>(7)?,
        row.get::<_, String>(8)?,
        row.get::<_, String>(9)?,
        row.get::<_, String>(10)?,
        row.get::<_, String>(11)?,
        u64_column(row, 12)?,
        u64_column(row, 13)?,
        optional_u64_column(row, 14)?,
    ))
}

fn load_draft_row(
    connection: &sqlite::Connection,
    draft_id: &str,
) -> DtoResult<RefinementDraftRecordDto> {
    connection
        .query_row(
            &format!("SELECT {DRAFT_COLUMNS} FROM refinement_drafts WHERE draft_id=?1"),
            [draft_id],
            draft_row_columns,
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                not_found(
                    "refinement_draft_conflict",
                    "the requested refinement draft does not exist",
                )
            } else {
                storage_error(error)
            }
        })
        .and_then(|row| draft_from_columns(&row))
}

fn load_pending_draft_optional(
    connection: &sqlite::Connection,
    leading_goal_id: &str,
) -> DtoResult<Option<RefinementDraftRecordDto>> {
    connection
        .query_row(
            &format!(
                "SELECT {DRAFT_COLUMNS} FROM refinement_drafts
                 WHERE leading_goal_id=?1 AND state='pending' LIMIT 1"
            ),
            [leading_goal_id],
            draft_row_columns,
        )
        .optional()
        .map_err(storage_error)?
        .map(|row| draft_from_columns(&row))
        .transpose()
}

impl GoalProposalRepositoryDto for SqliteStorageRepository {
    fn propose_refinement_draft(
        &self,
        input: RefinementDraftRecordDto,
    ) -> DtoResult<RefinementDraftRecordDto> {
        input.validate()?;
        let draft = input;
        write(self, |connection| {
            if let Some(existing) = load_pending_draft_optional(connection, &draft.leading_goal_id)?
            {
                if existing.canonical_digest != draft.canonical_digest {
                    return Err(refinement_draft_conflict());
                }
                let mut evidence = existing.evidence_references.clone();
                for reference in &draft.evidence_references {
                    if !evidence.iter().any(|stored| stored == reference) {
                        evidence.push(reference.clone());
                    }
                }
                let coalesced = existing
                    .coalesced_evidence_count
                    .saturating_add(draft.coalesced_evidence_count.max(1));
                connection
                    .execute(
                        "UPDATE refinement_drafts SET evidence_references=?2,
                            coalesced_evidence_count=?3 WHERE draft_id=?1",
                        sqlite::params![
                            existing.draft_id,
                            encode_evidence(&evidence),
                            int(coalesced, "count is outside the SQLite range")?,
                        ],
                    )
                    .map_err(storage_error)?;
                return load_draft_row(connection, &existing.draft_id);
            }
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO refinement_drafts(draft_id, source_run_id,
                        leading_goal_id, milestone, base_goal_revision, base_record_reference,
                        base_record_revision, edits, evidence_references, safe_rationale,
                        canonical_digest, state, coalesced_evidence_count, created_at_ms,
                        decided_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    sqlite::params![
                        draft.draft_id,
                        draft.source_run_id,
                        draft.leading_goal_id,
                        draft.milestone.name(),
                        int(
                            draft.base_goal_revision,
                            "revision is outside the SQLite range"
                        )?,
                        draft.base_record_reference,
                        int(
                            draft.base_record_revision,
                            "revision is outside the SQLite range"
                        )?,
                        encode_refinement_edits(&draft.edits),
                        encode_evidence(&draft.evidence_references),
                        draft.safe_rationale,
                        draft.canonical_digest,
                        draft.state.name(),
                        int(
                            draft.coalesced_evidence_count.max(1),
                            "count is outside the SQLite range"
                        )?,
                        int(draft.created_at_ms, "timestamp is outside the SQLite range")?,
                        draft
                            .decided_at_ms
                            .map(|value| int(value, "timestamp is outside the SQLite range"))
                            .transpose()?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored = load_draft_row(connection, &draft.draft_id)?;
                if stored != draft {
                    return Err(refinement_draft_conflict());
                }
                return Ok(stored);
            }
            Ok(draft)
        })
    }

    fn load_pending_refinement_draft(
        &self,
        leading_goal_id: String,
    ) -> DtoResult<Option<RefinementDraftRecordDto>> {
        let connection = self.connection()?;
        load_pending_draft_optional(&connection, &leading_goal_id)
    }

    fn decide_refinement_draft(
        &self,
        draft_id: String,
        state: RefinementDraftStateDto,
        decided_at_ms: u64,
    ) -> DtoResult<RefinementDraftRecordDto> {
        if state == RefinementDraftStateDto::Pending {
            return Err(ErrorDto::validation(
                "refinement_draft_conflict",
                "a decision resolves a pending refinement draft",
            ));
        }
        write(self, |connection| {
            let draft = load_draft_row(connection, &draft_id)?;
            if draft.state != RefinementDraftStateDto::Pending {
                return Err(refinement_draft_conflict());
            }
            connection
                .execute(
                    "UPDATE refinement_drafts SET state=?2, decided_at_ms=?3 WHERE draft_id=?1",
                    sqlite::params![
                        draft_id,
                        state.name(),
                        int(decided_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_draft_row(connection, &draft_id)
        })
    }
}

type SummaryRow = (
    String,
    u64,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    u64,
);

fn summary_from_columns(row: &SummaryRow) -> DtoResult<ConversationSummaryRecordDto> {
    Ok(ConversationSummaryRecordDto {
        summary_id: row.0.clone(),
        revision: row.1,
        scope: record_scope_from_columns(&row.2, &row.3)?,
        previous_summary_reference: row.4.clone(),
        source_range_start: row.5.clone(),
        source_range_end: row.6.clone(),
        safe_content: row.7.clone(),
        canonical_digest: row.8.clone(),
        created_at_ms: row.9,
    })
}

const SUMMARY_COLUMNS: &str = "summary_id, revision, scope_kind, owner_id, \
     previous_summary_reference, source_range_start, source_range_end, safe_content, \
     canonical_digest, created_at_ms";

fn load_summary_optional(
    connection: &sqlite::Connection,
    summary_id: &str,
    revision: u64,
) -> DtoResult<Option<ConversationSummaryRecordDto>> {
    connection
        .query_row(
            &format!(
                "SELECT {SUMMARY_COLUMNS} FROM conversation_summaries
                 WHERE summary_id=?1 AND revision=?2"
            ),
            sqlite::params![
                summary_id,
                int(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    u64_column(row, 1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    u64_column(row, 9)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .map(|row| summary_from_columns(&row))
        .transpose()
}

type WorkingFormRow = (String, String, Option<String>, Option<u64>, String, u64);

fn load_working_form_optional(
    connection: &sqlite::Connection,
    scope: &GoalRecordScopeDto,
) -> DtoResult<Option<WorkingFormRow>> {
    let (scope_kind, owner_id) = record_scope_columns(scope);
    connection
        .query_row(
            "SELECT scope_kind, owner_id, current_summary_id, current_summary_revision,
                    uncompacted_suffix, updated_at_ms
             FROM compaction_working_forms WHERE scope_kind=?1 AND owner_id=?2",
            sqlite::params![scope_kind, owner_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    optional_u64_column(row, 3)?,
                    row.get::<_, String>(4)?,
                    u64_column(row, 5)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
}

fn upsert_working_form(
    connection: &sqlite::Connection,
    scope: &GoalRecordScopeDto,
    current_summary: Option<(&str, u64)>,
    suffix: &[String],
    updated_at_ms: u64,
) -> DtoResult<()> {
    let (scope_kind, owner_id) = record_scope_columns(scope);
    let (summary_id, summary_revision) = match current_summary {
        Some((summary_id, revision)) => (
            Some(summary_id.to_owned()),
            Some(int(revision, "revision is outside the SQLite range")?),
        ),
        None => (None, None),
    };
    connection
        .execute(
            "INSERT INTO compaction_working_forms(scope_kind, owner_id, current_summary_id,
                current_summary_revision, uncompacted_suffix, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(scope_kind, owner_id) DO UPDATE SET
                current_summary_id=excluded.current_summary_id,
                current_summary_revision=excluded.current_summary_revision,
                uncompacted_suffix=excluded.uncompacted_suffix,
                updated_at_ms=excluded.updated_at_ms",
            sqlite::params![
                scope_kind,
                owner_id,
                summary_id,
                summary_revision,
                encode_items(suffix),
                int(updated_at_ms, "timestamp is outside the SQLite range")?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

impl GoalCompactionRepositoryDto for SqliteStorageRepository {
    fn record_compaction_suffix_reference(
        &self,
        input: RecordCompactionSuffixReferenceInputDto,
    ) -> DtoResult<()> {
        input.scope.validate("compaction_history_unavailable")?;
        intention_storage::validate_safe_label(
            "compaction_history_unavailable",
            &input.history_reference,
        )?;
        let scope = input.scope;
        let history_reference = input.history_reference;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let existing = load_working_form_optional(connection, &scope)?;
            let (mut suffix, current) = match existing {
                Some(row) => (
                    decode_items(&row.4)?,
                    match (row.2, row.3) {
                        (Some(summary_id), Some(revision)) => Some((summary_id, revision)),
                        _ => None,
                    },
                ),
                None => (Vec::new(), None),
            };
            if !suffix.iter().any(|stored| stored == &history_reference) {
                suffix.push(history_reference);
            }
            let current = current
                .as_ref()
                .map(|(id, revision)| (id.as_str(), *revision));
            upsert_working_form(connection, &scope, current, &suffix, occurred_at_ms)
        })
    }

    fn store_conversation_summary(
        &self,
        input: ConversationSummaryRecordDto,
    ) -> DtoResult<ConversationSummaryRecordDto> {
        input.validate()?;
        let summary = input;
        write(self, |connection| {
            let working = load_working_form_optional(connection, &summary.scope)?;
            let (suffix, current) = match working {
                Some(row) => (
                    decode_items(&row.4)?,
                    match (row.2, row.3) {
                        (Some(summary_id), Some(revision)) => Some((summary_id, revision)),
                        _ => None,
                    },
                ),
                None => (Vec::new(), None),
            };
            let Some(first) = suffix.first() else {
                return Err(compaction_history_unavailable());
            };
            if first != &summary.source_range_start {
                return Err(compaction_history_unavailable());
            }
            let Some(end_index) = suffix
                .iter()
                .position(|reference| reference == &summary.source_range_end)
            else {
                return Err(compaction_history_unavailable());
            };
            match &current {
                Some((summary_id, revision)) => {
                    if summary.revision != revision.saturating_add(1)
                        || summary.previous_summary_reference.as_deref()
                            != Some(summary_id.as_str())
                    {
                        return Err(conflict(
                            "compaction_summary_unavailable",
                            "a summary revision continues the exact previous selected summary",
                        ));
                    }
                }
                None => {
                    if summary.revision != 1 || summary.previous_summary_reference.is_some() {
                        return Err(conflict(
                            "compaction_summary_unavailable",
                            "the first summary starts the chain at revision one",
                        ));
                    }
                }
            }
            if let Some(existing) =
                load_summary_optional(connection, &summary.summary_id, summary.revision)?
            {
                if existing != summary {
                    return Err(conflict(
                        "compaction_summary_unavailable",
                        "the summary revision is already bound to different content",
                    ));
                }
                return Ok(existing);
            }
            connection
                .execute(
                    "INSERT INTO conversation_summaries(summary_id, revision, scope_kind, owner_id,
                        previous_summary_reference, source_range_start, source_range_end,
                        safe_content, canonical_digest, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    sqlite::params![
                        summary.summary_id,
                        int(summary.revision, "revision is outside the SQLite range")?,
                        summary.scope.kind_name(),
                        summary.scope.owner_id(),
                        summary.previous_summary_reference,
                        summary.source_range_start,
                        summary.source_range_end,
                        summary.safe_content,
                        summary.canonical_digest,
                        int(
                            summary.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            let remaining = suffix[end_index + 1..].to_vec();
            upsert_working_form(
                connection,
                &summary.scope,
                Some((&summary.summary_id, summary.revision)),
                &remaining,
                summary.created_at_ms,
            )?;
            Ok(summary)
        })
    }

    fn load_conversation_summary(
        &self,
        summary_id: String,
        revision: u64,
    ) -> DtoResult<ConversationSummaryRecordDto> {
        let connection = self.connection()?;
        load_summary_optional(&connection, &summary_id, revision)?
            .ok_or_else(compaction_summary_unavailable_or_storage_absent)
    }

    fn load_goal_compaction_working_form(
        &self,
        scope: GoalRecordScopeDto,
    ) -> DtoResult<GoalCompactionWorkingFormRecordDto> {
        let (current_summary, uncompacted_suffix) = {
            let connection = self.connection()?;
            let Some(row) = load_working_form_optional(&connection, &scope)? else {
                return Ok(GoalCompactionWorkingFormRecordDto {
                    current_summary: None,
                    uncompacted_suffix: Vec::new(),
                });
            };
            let current_summary = match (row.2, row.3) {
                (Some(summary_id), Some(revision)) => Some(
                    load_summary_optional(&connection, &summary_id, revision)?
                        .ok_or_else(compaction_summary_unavailable_or_storage_absent)?,
                ),
                _ => None,
            };
            drop(connection);
            (current_summary, decode_items(&row.4)?)
        };
        Ok(GoalCompactionWorkingFormRecordDto {
            current_summary,
            uncompacted_suffix,
        })
    }
}

fn compaction_summary_unavailable_or_storage_absent() -> ErrorDto {
    not_found(
        "compaction_summary_unavailable",
        "the requested durable conversation summary revision does not exist",
    )
}

fn verifier_authority_invalid_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        verifier_authority_invalid()
    } else {
        storage_error(error)
    }
}

fn verifier_authority_invalid() -> ErrorDto {
    not_found(
        "verifier_authority_invalid",
        "the requested durable verifier authority revision does not exist",
    )
}

fn verifier_baseline_invalid() -> ErrorDto {
    not_found(
        "verifier_baseline_invalid",
        "the requested durable verifier audit baseline does not exist or is not coherent",
    )
}

fn verifier_evidence_invalid() -> ErrorDto {
    not_found(
        "verifier_evidence_invalid",
        "the requested durable verifier audit evidence does not exist",
    )
}

fn verifier_verdict_invalid() -> ErrorDto {
    not_found(
        "verifier_verdict_invalid",
        "the requested durable verifier audit verdict does not exist",
    )
}

fn verifier_mutation_invalid() -> ErrorDto {
    not_found(
        "verifier_mutation_invalid",
        "the requested durable target mutation does not exist",
    )
}

/// Encodes one closed operation set.
fn encode_operations(operations: &[VerifierOperationDto]) -> String {
    encode_items(
        &operations
            .iter()
            .map(|operation| operation.name().to_owned())
            .collect::<Vec<_>>(),
    )
}

/// Decodes one closed operation set.
fn decode_operations(encoded: &str) -> DtoResult<Vec<VerifierOperationDto>> {
    decode_items(encoded)?
        .iter()
        .map(|name| VerifierOperationDto::parse(name))
        .collect()
}

/// Encodes the frozen Goal references of one verifier record.
fn encode_frozen_goals(references: &[VerifierGoalReferenceDto]) -> String {
    encode_records(
        &references
            .iter()
            .map(|reference| {
                vec![
                    reference.goal_id.clone(),
                    reference.goal_revision.to_string(),
                    reference.canonical_goal_revision_digest.clone(),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

/// Encodes the frozen contract references of one verifier record.
fn encode_frozen_contracts(references: &[VerifierContractReferenceDto]) -> String {
    encode_records(
        &references
            .iter()
            .map(|reference| {
                vec![
                    reference.contract_id.clone(),
                    reference.contract_revision.to_string(),
                    reference.canonical_contract_digest.clone(),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

/// Decodes the frozen Goal references of one verifier record.
fn decode_frozen_goals(encoded: &str) -> DtoResult<Vec<VerifierGoalReferenceDto>> {
    decode_records(encoded)?
        .into_iter()
        .map(|fields| {
            if fields.len() != 3 {
                return Err(codec_error("persisted frozen Goal reference is malformed"));
            }
            Ok(VerifierGoalReferenceDto {
                goal_id: fields[0].clone(),
                goal_revision: parse_u64(&fields[1])?,
                canonical_goal_revision_digest: fields[2].clone(),
            })
        })
        .collect()
}

/// Decodes the frozen contract references of one verifier record.
fn decode_frozen_contracts(encoded: &str) -> DtoResult<Vec<VerifierContractReferenceDto>> {
    decode_records(encoded)?
        .into_iter()
        .map(|fields| {
            if fields.len() != 3 {
                return Err(codec_error(
                    "persisted frozen contract reference is malformed",
                ));
            }
            Ok(VerifierContractReferenceDto {
                contract_id: fields[0].clone(),
                contract_revision: parse_u64(&fields[1])?,
                canonical_contract_digest: fields[2].clone(),
            })
        })
        .collect()
}

/// Rebuilds one frozen reference pair from its persisted columns.
fn frozen_references_from_columns(
    goals: &str,
    contracts: &str,
) -> DtoResult<VerifierFrozenReferencesDto> {
    Ok(VerifierFrozenReferencesDto {
        goal_references: decode_frozen_goals(goals)?,
        contract_references: decode_frozen_contracts(contracts)?,
    })
}

/// Rebuilds one exact authority reference from its persisted columns.
fn authority_reference_from_columns(
    authority_id: &str,
    authority_revision: u64,
    authority_digest: &str,
) -> VerifierAuthorityReferenceDto {
    VerifierAuthorityReferenceDto {
        authority_id: authority_id.to_owned(),
        authority_revision,
        canonical_authority_digest: authority_digest.to_owned(),
    }
}

/// Rebuilds one exact audit contract reference from its persisted columns.
fn contract_reference_from_columns(
    contract_id: &str,
    contract_revision: u64,
    contract_digest: &str,
) -> VerifierContractReferenceDto {
    VerifierContractReferenceDto {
        contract_id: contract_id.to_owned(),
        contract_revision,
        canonical_contract_digest: contract_digest.to_owned(),
    }
}

type AuthorityRow = (
    String,
    u64,
    String,
    String,
    String,
    String,
    String,
    u64,
    String,
    u64,
    Option<u64>,
    Option<u64>,
    Option<String>,
    String,
    String,
    Option<String>,
    String,
);

fn authority_from_columns(row: &AuthorityRow) -> DtoResult<VerifierAuthorityRecordDto> {
    Ok(VerifierAuthorityRecordDto {
        authority_id: row.0.clone(),
        authority_revision: row.1,
        verifier_mandate_id: row.2.clone(),
        immutable_target_set_reference: VerifierTargetSetReferenceDto {
            target_set_id: row.3.clone(),
            canonical_target_set_digest: row.4.clone(),
        },
        allowed_operations: decode_operations(&row.5)?,
        audit_contract_reference: contract_reference_from_columns(&row.6, row.7, &row.8),
        issued_at_ms: row.9,
        expires_at_ms: row.10,
        revoked_at_ms: row.11,
        revocation_reference: row.12.clone(),
        consumption_rule: VerifierAuthorityConsumptionRuleDto::parse(&row.13)?,
        consumption_state: VerifierAuthorityConsumptionStateDto::parse(&row.14)?,
        consumed_by_mutation_reference: row.15.clone(),
        canonical_authority_digest: row.16.clone(),
    })
}

const AUTHORITY_COLUMNS: &str = "authority_id, authority_revision, verifier_mandate_id, \
     target_set_id, target_set_digest, allowed_operations, audit_contract_id, \
     audit_contract_revision, audit_contract_digest, issued_at_ms, expires_at_ms, revoked_at_ms, \
     revocation_reference, consumption_rule, consumption_state, consumed_by_mutation_reference, \
     canonical_authority_digest";

fn authority_row_columns(row: &sqlite::Row<'_>) -> Result<AuthorityRow, sqlite::Error> {
    Ok((
        row.get::<_, String>(0)?,
        u64_column(row, 1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(5)?,
        row.get::<_, String>(6)?,
        u64_column(row, 7)?,
        row.get::<_, String>(8)?,
        u64_column(row, 9)?,
        optional_u64_column(row, 10)?,
        optional_u64_column(row, 11)?,
        row.get::<_, Option<String>>(12)?,
        row.get::<_, String>(13)?,
        row.get::<_, String>(14)?,
        row.get::<_, Option<String>>(15)?,
        row.get::<_, String>(16)?,
    ))
}

fn load_authority_row(
    connection: &sqlite::Connection,
    authority_id: &str,
    authority_revision: u64,
) -> DtoResult<VerifierAuthorityRecordDto> {
    connection
        .query_row(
            &format!(
                "SELECT {AUTHORITY_COLUMNS} FROM verifier_authorities
                 WHERE authority_id=?1 AND authority_revision=?2"
            ),
            sqlite::params![
                authority_id,
                int(authority_revision, "revision is outside the SQLite range")?
            ],
            authority_row_columns,
        )
        .map_err(verifier_authority_invalid_or_storage)
        .and_then(|row| authority_from_columns(&row))
}

type BaselineRow = (
    String,
    String,
    u64,
    String,
    u64,
    String,
    u64,
    u64,
    String,
    String,
    String,
    Option<String>,
    String,
    u64,
    String,
    Option<u64>,
    u64,
);

fn baseline_from_columns(row: &BaselineRow) -> DtoResult<VerifierAuditBaselineRecordDto> {
    Ok(VerifierAuditBaselineRecordDto {
        authority_reference: authority_reference_from_columns(&row.1, row.2, &row.3),
        verifier_mandate_revision: row.4,
        target_mandate_id: row.5.clone(),
        target_revision: row.6,
        target_sequence: row.7,
        target_lifecycle: VerifierTargetLifecycleDto::parse(&row.8)?,
        frozen_references: frozen_references_from_columns(&row.9, &row.10)?,
        optional_unknown_effect_reference: row.11.clone(),
        audit_contract_reference: contract_reference_from_columns(&row.12, row.13, &row.14),
        graph_epoch: row.15,
        canonical_baseline_digest: row.0.clone(),
        created_at_ms: row.16,
    })
}

const BASELINE_COLUMNS: &str = "canonical_baseline_digest, authority_id, authority_revision, \
     authority_digest, verifier_mandate_revision, target_mandate_id, target_revision, \
     target_sequence, target_lifecycle, frozen_goal_references, frozen_contract_references, \
     optional_unknown_effect_reference, audit_contract_id, audit_contract_revision, \
     audit_contract_digest, graph_epoch, created_at_ms";

fn baseline_row_columns(row: &sqlite::Row<'_>) -> Result<BaselineRow, sqlite::Error> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        u64_column(row, 2)?,
        row.get::<_, String>(3)?,
        u64_column(row, 4)?,
        row.get::<_, String>(5)?,
        u64_column(row, 6)?,
        u64_column(row, 7)?,
        row.get::<_, String>(8)?,
        row.get::<_, String>(9)?,
        row.get::<_, String>(10)?,
        row.get::<_, Option<String>>(11)?,
        row.get::<_, String>(12)?,
        u64_column(row, 13)?,
        row.get::<_, String>(14)?,
        optional_u64_column(row, 15)?,
        u64_column(row, 16)?,
    ))
}

fn load_baseline_optional(
    connection: &sqlite::Connection,
    canonical_baseline_digest: &str,
) -> DtoResult<Option<VerifierAuditBaselineRecordDto>> {
    connection
        .query_row(
            &format!(
                "SELECT {BASELINE_COLUMNS} FROM verifier_audit_baselines
                 WHERE canonical_baseline_digest=?1"
            ),
            [canonical_baseline_digest],
            baseline_row_columns,
        )
        .optional()
        .map_err(storage_error)?
        .map(|row| baseline_from_columns(&row))
        .transpose()
}

type EvidenceRow = (
    String,
    String,
    u64,
    String,
    String,
    u64,
    String,
    String,
    String,
    String,
    String,
    u64,
);

fn evidence_from_columns(row: &EvidenceRow) -> DtoResult<VerifierAuditEvidenceRecordDto> {
    Ok(VerifierAuditEvidenceRecordDto {
        evidence_id: row.0.clone(),
        authority_reference: authority_reference_from_columns(&row.1, row.2, &row.3),
        target_reference: VerifierTargetReferenceDto {
            target_mandate_id: row.4.clone(),
            target_revision: row.5,
        },
        frozen_references: frozen_references_from_columns(&row.6, &row.7)?,
        evidence_kind: VerifierEvidenceKindDto::parse(&row.8)?,
        retained_content_reference: row.9.clone(),
        canonical_evidence_digest: row.10.clone(),
        created_at_ms: row.11,
    })
}

const EVIDENCE_COLUMNS: &str = "evidence_id, authority_id, authority_revision, authority_digest, \
     target_mandate_id, target_revision, frozen_goal_references, frozen_contract_references, \
     evidence_kind, retained_content_reference, canonical_evidence_digest, created_at_ms";

fn evidence_row_columns(row: &sqlite::Row<'_>) -> Result<EvidenceRow, sqlite::Error> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        u64_column(row, 2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        u64_column(row, 5)?,
        row.get::<_, String>(6)?,
        row.get::<_, String>(7)?,
        row.get::<_, String>(8)?,
        row.get::<_, String>(9)?,
        row.get::<_, String>(10)?,
        u64_column(row, 11)?,
    ))
}

fn load_evidence_row(
    connection: &sqlite::Connection,
    evidence_id: &str,
) -> DtoResult<VerifierAuditEvidenceRecordDto> {
    connection
        .query_row(
            &format!("SELECT {EVIDENCE_COLUMNS} FROM verifier_audit_evidence WHERE evidence_id=?1"),
            [evidence_id],
            evidence_row_columns,
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                verifier_evidence_invalid()
            } else {
                storage_error(error)
            }
        })
        .and_then(|row| evidence_from_columns(&row))
}

type VerdictRow = (
    String,
    String,
    u64,
    String,
    String,
    u64,
    String,
    String,
    String,
    String,
    u64,
);

fn verdict_from_columns(row: &VerdictRow) -> DtoResult<VerifierAuditVerdictRecordDto> {
    Ok(VerifierAuditVerdictRecordDto {
        verdict_id: row.0.clone(),
        authority_reference: authority_reference_from_columns(&row.1, row.2, &row.3),
        target_reference: VerifierTargetReferenceDto {
            target_mandate_id: row.4.clone(),
            target_revision: row.5,
        },
        baseline_digest: row.6.clone(),
        verdict: intention_storage::goal_repo::VerificationAuditVerdictDto::parse(&row.7)?,
        evidence_references: decode_items(&row.8)?,
        canonical_verdict_digest: row.9.clone(),
        created_at_ms: row.10,
    })
}

const VERDICT_COLUMNS: &str = "verdict_id, authority_id, authority_revision, authority_digest, \
     target_mandate_id, target_revision, baseline_digest, verdict, evidence_references, \
     canonical_verdict_digest, created_at_ms";

fn verdict_row_columns(row: &sqlite::Row<'_>) -> Result<VerdictRow, sqlite::Error> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        u64_column(row, 2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        u64_column(row, 5)?,
        row.get::<_, String>(6)?,
        row.get::<_, String>(7)?,
        row.get::<_, String>(8)?,
        row.get::<_, String>(9)?,
        u64_column(row, 10)?,
    ))
}

fn load_verdict_row(
    connection: &sqlite::Connection,
    verdict_id: &str,
) -> DtoResult<VerifierAuditVerdictRecordDto> {
    connection
        .query_row(
            &format!("SELECT {VERDICT_COLUMNS} FROM verifier_audit_verdicts WHERE verdict_id=?1"),
            [verdict_id],
            verdict_row_columns,
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                verifier_verdict_invalid()
            } else {
                storage_error(error)
            }
        })
        .and_then(|row| verdict_from_columns(&row))
}

type MutationRow = (
    String,
    String,
    String,
    String,
    u64,
    String,
    String,
    u64,
    String,
    String,
    u64,
    String,
    String,
    u64,
    u64,
    String,
    String,
    u64,
);

fn mutation_from_columns(row: &MutationRow) -> DtoResult<VerifierTargetMutationRecordDto> {
    Ok(VerifierTargetMutationRecordDto {
        mutation_id: row.0.clone(),
        authority_reference: authority_reference_from_columns(&row.3, row.4, &row.5),
        audit_contract_reference: contract_reference_from_columns(&row.6, row.7, &row.8),
        target_reference: VerifierTargetReferenceDto {
            target_mandate_id: row.9.clone(),
            target_revision: row.10,
        },
        operation: VerifierOperationDto::parse(&row.11)?,
        audit_evidence_references: decode_items(&row.12)?,
        expected_target_revision: row.13,
        expected_target_sequence: row.14,
        expected_baseline_digest: row.15.clone(),
        idempotency: VerifierOperationIdentityDto {
            operation_id: row.1.clone(),
            operation_digest: row.2.clone(),
        },
        canonical_mutation_digest: row.16.clone(),
        created_at_ms: row.17,
    })
}

const MUTATION_COLUMNS: &str = "mutation_id, operation_id, operation_digest, authority_id, \
     authority_revision, authority_digest, audit_contract_id, audit_contract_revision, \
     audit_contract_digest, target_mandate_id, target_revision, operation, \
     audit_evidence_references, expected_target_revision, expected_target_sequence, \
     expected_baseline_digest, canonical_mutation_digest, created_at_ms";

fn mutation_row_columns(row: &sqlite::Row<'_>) -> Result<MutationRow, sqlite::Error> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        u64_column(row, 4)?,
        row.get::<_, String>(5)?,
        row.get::<_, String>(6)?,
        u64_column(row, 7)?,
        row.get::<_, String>(8)?,
        row.get::<_, String>(9)?,
        u64_column(row, 10)?,
        row.get::<_, String>(11)?,
        row.get::<_, String>(12)?,
        u64_column(row, 13)?,
        u64_column(row, 14)?,
        row.get::<_, String>(15)?,
        row.get::<_, String>(16)?,
        u64_column(row, 17)?,
    ))
}

fn load_mutation_row(
    connection: &sqlite::Connection,
    mutation_id: &str,
) -> DtoResult<VerifierTargetMutationRecordDto> {
    connection
        .query_row(
            &format!(
                "SELECT {MUTATION_COLUMNS} FROM verifier_target_mutations WHERE mutation_id=?1"
            ),
            [mutation_id],
            mutation_row_columns,
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                verifier_mutation_invalid()
            } else {
                storage_error(error)
            }
        })
        .and_then(|row| mutation_from_columns(&row))
}

fn load_mutation_id_by_operation(
    connection: &sqlite::Connection,
    operation_id: &str,
) -> DtoResult<Option<String>> {
    connection
        .query_row(
            "SELECT mutation_id FROM verifier_target_mutations WHERE operation_id=?1",
            [operation_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)
}

impl GoalVerificationRepositoryDto for SqliteStorageRepository {
    fn record_verifier_authority(
        &self,
        input: VerifierAuthorityRecordDto,
    ) -> DtoResult<VerifierAuthorityRecordDto> {
        input.validate()?;
        let authority = input;
        write(self, |connection| {
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO verifier_authorities(authority_id, authority_revision,
                        verifier_mandate_id, target_set_id, target_set_digest, allowed_operations,
                        audit_contract_id, audit_contract_revision, audit_contract_digest,
                        issued_at_ms, expires_at_ms, revoked_at_ms, revocation_reference,
                        consumption_rule, consumption_state, consumed_by_mutation_reference,
                        canonical_authority_digest)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                        ?16, ?17)",
                    sqlite::params![
                        authority.authority_id,
                        int(
                            authority.authority_revision,
                            "revision is outside the SQLite range"
                        )?,
                        authority.verifier_mandate_id,
                        authority.immutable_target_set_reference.target_set_id,
                        authority
                            .immutable_target_set_reference
                            .canonical_target_set_digest,
                        encode_operations(&authority.allowed_operations),
                        authority.audit_contract_reference.contract_id,
                        int(
                            authority.audit_contract_reference.contract_revision,
                            "revision is outside the SQLite range"
                        )?,
                        authority.audit_contract_reference.canonical_contract_digest,
                        int(
                            authority.issued_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                        authority
                            .expires_at_ms
                            .map(|value| int(value, "timestamp is outside the SQLite range"))
                            .transpose()?,
                        authority
                            .revoked_at_ms
                            .map(|value| int(value, "timestamp is outside the SQLite range"))
                            .transpose()?,
                        authority.revocation_reference,
                        authority.consumption_rule.name(),
                        authority.consumption_state.name(),
                        authority.consumed_by_mutation_reference,
                        authority.canonical_authority_digest,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored = load_authority_row(
                    connection,
                    &authority.authority_id,
                    authority.authority_revision,
                )?;
                if stored != authority {
                    return Err(conflict(
                        "verifier_authority_invalid",
                        "the authority revision is already bound to different content",
                    ));
                }
                return Ok(stored);
            }
            Ok(authority)
        })
    }

    fn load_verifier_authority(
        &self,
        authority_id: String,
        authority_revision: u64,
    ) -> DtoResult<VerifierAuthorityRecordDto> {
        let connection = self.connection()?;
        load_authority_row(&connection, &authority_id, authority_revision)
    }

    fn revoke_verifier_authority(
        &self,
        input: RevokeVerifierAuthorityInputDto,
    ) -> DtoResult<VerifierAuthorityRecordDto> {
        input.validate()?;
        let authority_id = input.authority_id;
        let authority_revision = input.authority_revision;
        let revocation_reference = input.revocation_reference;
        let revoked_at_ms = input.revoked_at_ms;
        write(self, |connection| {
            let stored = load_authority_row(connection, &authority_id, authority_revision)?;
            if stored.revoked_at_ms.is_some() {
                return Err(conflict(
                    "verifier_authority_revoked",
                    "the authority revision is already revoked",
                ));
            }
            if revoked_at_ms < stored.issued_at_ms {
                return Err(conflict(
                    "verifier_authority_invalid",
                    "a revocation never precedes issuance",
                ));
            }
            connection
                .execute(
                    "UPDATE verifier_authorities SET revoked_at_ms=?3, revocation_reference=?4
                     WHERE authority_id=?1 AND authority_revision=?2",
                    sqlite::params![
                        authority_id,
                        int(authority_revision, "revision is outside the SQLite range")?,
                        int(revoked_at_ms, "timestamp is outside the SQLite range")?,
                        revocation_reference,
                    ],
                )
                .map_err(storage_error)?;
            load_authority_row(connection, &authority_id, authority_revision)
        })
    }

    fn record_verifier_audit_baseline(
        &self,
        input: VerifierAuditBaselineRecordDto,
    ) -> DtoResult<VerifierAuditBaselineRecordDto> {
        input.validate()?;
        let baseline = input;
        write(self, |connection| {
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO verifier_audit_baselines(canonical_baseline_digest,
                        authority_id, authority_revision, authority_digest,
                        verifier_mandate_revision, target_mandate_id, target_revision,
                        target_sequence, target_lifecycle, frozen_goal_references,
                        frozen_contract_references, optional_unknown_effect_reference,
                        audit_contract_id, audit_contract_revision, audit_contract_digest,
                        graph_epoch, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                        ?16, ?17)",
                    sqlite::params![
                        baseline.canonical_baseline_digest,
                        baseline.authority_reference.authority_id,
                        int(
                            baseline.authority_reference.authority_revision,
                            "revision is outside the SQLite range"
                        )?,
                        baseline.authority_reference.canonical_authority_digest,
                        int(
                            baseline.verifier_mandate_revision,
                            "revision is outside the SQLite range"
                        )?,
                        baseline.target_mandate_id,
                        int(
                            baseline.target_revision,
                            "revision is outside the SQLite range"
                        )?,
                        int(
                            baseline.target_sequence,
                            "sequence is outside the SQLite range"
                        )?,
                        baseline.target_lifecycle.name(),
                        encode_frozen_goals(&baseline.frozen_references.goal_references),
                        encode_frozen_contracts(&baseline.frozen_references.contract_references),
                        baseline.optional_unknown_effect_reference,
                        baseline.audit_contract_reference.contract_id,
                        int(
                            baseline.audit_contract_reference.contract_revision,
                            "revision is outside the SQLite range"
                        )?,
                        baseline.audit_contract_reference.canonical_contract_digest,
                        baseline
                            .graph_epoch
                            .map(|value| int(value, "epoch is outside the SQLite range"))
                            .transpose()?,
                        int(
                            baseline.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored =
                    load_baseline_optional(connection, &baseline.canonical_baseline_digest)?
                        .ok_or_else(verifier_baseline_invalid)?;
                if stored != baseline {
                    return Err(conflict(
                        "verifier_baseline_invalid",
                        "the baseline digest is already bound to different content",
                    ));
                }
                return Ok(stored);
            }
            Ok(baseline)
        })
    }

    fn load_verifier_audit_baseline(
        &self,
        canonical_baseline_digest: String,
    ) -> DtoResult<VerifierAuditBaselineRecordDto> {
        let connection = self.connection()?;
        load_baseline_optional(&connection, &canonical_baseline_digest)?
            .ok_or_else(verifier_baseline_invalid)
    }

    fn record_verifier_audit_evidence(
        &self,
        input: VerifierAuditEvidenceRecordDto,
    ) -> DtoResult<VerifierAuditEvidenceRecordDto> {
        input.validate()?;
        let evidence = input;
        write(self, |connection| {
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO verifier_audit_evidence(evidence_id, authority_id,
                        authority_revision, authority_digest, target_mandate_id, target_revision,
                        frozen_goal_references, frozen_contract_references, evidence_kind,
                        retained_content_reference, canonical_evidence_digest, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    sqlite::params![
                        evidence.evidence_id,
                        evidence.authority_reference.authority_id,
                        int(
                            evidence.authority_reference.authority_revision,
                            "revision is outside the SQLite range"
                        )?,
                        evidence.authority_reference.canonical_authority_digest,
                        evidence.target_reference.target_mandate_id,
                        int(
                            evidence.target_reference.target_revision,
                            "revision is outside the SQLite range"
                        )?,
                        encode_frozen_goals(&evidence.frozen_references.goal_references),
                        encode_frozen_contracts(&evidence.frozen_references.contract_references),
                        evidence.evidence_kind.name(),
                        evidence.retained_content_reference,
                        evidence.canonical_evidence_digest,
                        int(
                            evidence.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored = load_evidence_row(connection, &evidence.evidence_id)?;
                if stored != evidence {
                    return Err(conflict(
                        "verifier_evidence_invalid",
                        "the evidence identity is already bound to different content",
                    ));
                }
                return Ok(stored);
            }
            Ok(evidence)
        })
    }

    fn load_verifier_audit_evidence(
        &self,
        evidence_id: String,
    ) -> DtoResult<VerifierAuditEvidenceRecordDto> {
        let connection = self.connection()?;
        load_evidence_row(&connection, &evidence_id)
    }

    fn record_verifier_audit_verdict(
        &self,
        input: VerifierAuditVerdictRecordDto,
    ) -> DtoResult<VerifierAuditVerdictRecordDto> {
        input.validate()?;
        let verdict = input;
        write(self, |connection| {
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO verifier_audit_verdicts(verdict_id, authority_id,
                        authority_revision, authority_digest, target_mandate_id, target_revision,
                        baseline_digest, verdict, evidence_references, canonical_verdict_digest,
                        created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    sqlite::params![
                        verdict.verdict_id,
                        verdict.authority_reference.authority_id,
                        int(
                            verdict.authority_reference.authority_revision,
                            "revision is outside the SQLite range"
                        )?,
                        verdict.authority_reference.canonical_authority_digest,
                        verdict.target_reference.target_mandate_id,
                        int(
                            verdict.target_reference.target_revision,
                            "revision is outside the SQLite range"
                        )?,
                        verdict.baseline_digest,
                        verdict.verdict.name(),
                        encode_items(&verdict.evidence_references),
                        verdict.canonical_verdict_digest,
                        int(
                            verdict.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored = load_verdict_row(connection, &verdict.verdict_id)?;
                if stored != verdict {
                    return Err(conflict(
                        "verifier_verdict_invalid",
                        "the verdict identity is already bound to different content",
                    ));
                }
                return Ok(stored);
            }
            Ok(verdict)
        })
    }

    fn load_verifier_audit_verdict(
        &self,
        verdict_id: String,
    ) -> DtoResult<VerifierAuditVerdictRecordDto> {
        let connection = self.connection()?;
        load_verdict_row(&connection, &verdict_id)
    }

    fn apply_verifier_target_mutation(
        &self,
        input: VerifierTargetMutationRecordDto,
    ) -> DtoResult<ApplyVerifierMutationOutcomeDto> {
        input.validate()?;
        let mutation = input;
        write(self, |connection| {
            if let Some(existing_id) =
                load_mutation_id_by_operation(connection, &mutation.idempotency.operation_id)?
            {
                let stored = load_mutation_row(connection, &existing_id)?;
                if stored != mutation {
                    return Err(conflict(
                        "verifier_mutation_invalid",
                        "the idempotent operation is already bound to a different mutation",
                    ));
                }
                let authority = load_authority_row(
                    connection,
                    &mutation.authority_reference.authority_id,
                    mutation.authority_reference.authority_revision,
                )?;
                return Ok(ApplyVerifierMutationOutcomeDto {
                    mutation: stored,
                    authority,
                    replayed: true,
                });
            }
            let authority = load_authority_row(
                connection,
                &mutation.authority_reference.authority_id,
                mutation.authority_reference.authority_revision,
            )?;
            if authority.canonical_authority_digest
                != mutation.authority_reference.canonical_authority_digest
            {
                return Err(conflict(
                    "verifier_authority_digest_mismatch",
                    "the authority reference digest does not match the stored revision",
                ));
            }
            if authority.revoked_at_ms.is_some() {
                return Err(conflict(
                    "verifier_authority_revoked",
                    "the authority revision is revoked",
                ));
            }
            if let Some(expires_at_ms) = authority.expires_at_ms
                && expires_at_ms <= mutation.created_at_ms
            {
                return Err(conflict(
                    "verifier_authority_expired",
                    "the authority revision is expired",
                ));
            }
            if authority.consumption_state == VerifierAuthorityConsumptionStateDto::Consumed {
                return Err(conflict(
                    "verifier_authority_consumed",
                    "the authority revision is consumed",
                ));
            }
            if !authority.allowed_operations.contains(&mutation.operation) {
                return Err(conflict(
                    "verifier_authority_operation_not_allowed",
                    "the authority revision does not allow the requested operation",
                ));
            }
            if authority.audit_contract_reference != mutation.audit_contract_reference {
                return Err(conflict(
                    "verifier_authority_contract_mismatch",
                    "the requested audit contract is not the authority contract",
                ));
            }
            let Some(baseline) =
                load_baseline_optional(connection, &mutation.expected_baseline_digest)?
            else {
                return Err(verifier_baseline_invalid());
            };
            if baseline.authority_reference != mutation.authority_reference {
                return Err(verifier_baseline_invalid());
            }
            if baseline.target_mandate_id != mutation.target_reference.target_mandate_id {
                return Err(conflict(
                    "verifier_baseline_stale_target_identity",
                    "the frozen baseline names a different target identity",
                ));
            }
            if baseline.target_revision != mutation.expected_target_revision
                || baseline.target_revision != mutation.target_reference.target_revision
                || baseline.target_sequence != mutation.expected_target_sequence
            {
                return Err(conflict(
                    "verifier_baseline_stale_target_revision",
                    "the frozen baseline target revision or sequence is stale",
                ));
            }
            if baseline.audit_contract_reference != mutation.audit_contract_reference {
                return Err(conflict(
                    "verifier_authority_contract_mismatch",
                    "the frozen baseline audit contract is not the mutation contract",
                ));
            }
            connection
                .execute(
                    "INSERT INTO verifier_target_mutations(mutation_id, operation_id,
                        operation_digest, authority_id, authority_revision, authority_digest,
                        audit_contract_id, audit_contract_revision, audit_contract_digest,
                        target_mandate_id, target_revision, operation, audit_evidence_references,
                        expected_target_revision, expected_target_sequence,
                        expected_baseline_digest, canonical_mutation_digest, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                        ?16, ?17, ?18)",
                    sqlite::params![
                        mutation.mutation_id,
                        mutation.idempotency.operation_id,
                        mutation.idempotency.operation_digest,
                        mutation.authority_reference.authority_id,
                        int(
                            mutation.authority_reference.authority_revision,
                            "revision is outside the SQLite range"
                        )?,
                        mutation.authority_reference.canonical_authority_digest,
                        mutation.audit_contract_reference.contract_id,
                        int(
                            mutation.audit_contract_reference.contract_revision,
                            "revision is outside the SQLite range"
                        )?,
                        mutation.audit_contract_reference.canonical_contract_digest,
                        mutation.target_reference.target_mandate_id,
                        int(
                            mutation.target_reference.target_revision,
                            "revision is outside the SQLite range"
                        )?,
                        mutation.operation.name(),
                        encode_items(&mutation.audit_evidence_references),
                        int(
                            mutation.expected_target_revision,
                            "revision is outside the SQLite range"
                        )?,
                        int(
                            mutation.expected_target_sequence,
                            "sequence is outside the SQLite range"
                        )?,
                        mutation.expected_baseline_digest,
                        mutation.canonical_mutation_digest,
                        int(
                            mutation.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            if authority.consumption_rule == VerifierAuthorityConsumptionRuleDto::SingleUse {
                connection
                    .execute(
                        "UPDATE verifier_authorities SET consumption_state='consumed',
                            consumed_by_mutation_reference=?3
                         WHERE authority_id=?1 AND authority_revision=?2",
                        sqlite::params![
                            mutation.authority_reference.authority_id,
                            int(
                                mutation.authority_reference.authority_revision,
                                "revision is outside the SQLite range"
                            )?,
                            mutation.mutation_id,
                        ],
                    )
                    .map_err(storage_error)?;
            }
            let stored = load_mutation_row(connection, &mutation.mutation_id)?;
            let authority = load_authority_row(
                connection,
                &mutation.authority_reference.authority_id,
                mutation.authority_reference.authority_revision,
            )?;
            Ok(ApplyVerifierMutationOutcomeDto {
                mutation: stored,
                authority,
                replayed: false,
            })
        })
    }

    fn load_verifier_target_mutation(
        &self,
        mutation_id: String,
    ) -> DtoResult<VerifierTargetMutationRecordDto> {
        let connection = self.connection()?;
        load_mutation_row(&connection, &mutation_id)
    }
}
