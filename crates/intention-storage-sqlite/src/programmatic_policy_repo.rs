//! Durable programmatic-caller policy repository and admission reservations.
//!
//! Owner: architecture 27 with ADR 0044. This module owns the current
//! single-version schema records for policy identities, immutable policy
//! revisions, effective snapshots, exact confirmations, corridors, inactive
//! drafts, shared counters, and admission reservations, plus the store-level
//! atomicity of the Slice 3 policy surface.
//!
//! Every method validates its DTO before any row is written, writes through
//! parameterized SQL only, and commits multi-row state in one immediate
//! transaction. Durable list fields use a private delimiter-separated
//! canonical text codec whose elements are validated control-free before
//! encoding, so the unit and record separators are unambiguous.

use intention_domain::programmatic_policy as policy_domain;
use intention_storage::programmatic_policy_repo::{
    AppendProgrammaticPolicyRevisionInputDto, CommitProgrammaticReservationStartedInputDto,
    ConsumeProgrammaticCorridorActionInputDto, CreateProgrammaticPolicyInputDto,
    DecideProgrammaticPolicyConfirmationInputDto, ProgrammaticAdmissionDecisionDto,
    ProgrammaticAdmissionRepositoryDto, ProgrammaticAuthorizationCorridorRecordDto,
    ProgrammaticCalendarCounterReferenceDto, ProgrammaticCalendarLimitRecordDto,
    ProgrammaticCalendarPeriodKindDto, ProgrammaticConfirmationRepositoryDto,
    ProgrammaticConfirmationStateDto, ProgrammaticCorridorStateDto,
    ProgrammaticPolicyCounterRecordDto, ProgrammaticPolicyDraftRecordDto,
    ProgrammaticPolicyDraftStateDto, ProgrammaticPolicyLifecycleOperationDto,
    ProgrammaticPolicyLifecycleStateDto, ProgrammaticPolicyRecordDto,
    ProgrammaticPolicyRepositoryDto, ProgrammaticPolicyReservationRecordDto,
    ProgrammaticPolicyRevisionRecordDto, ProgrammaticPolicyRevisionReferenceDto,
    ProgrammaticPolicyScopeDto, ProgrammaticPolicySnapshotRecordDto,
    ProgrammaticReservationStateDto, ProgrammaticRootOriginKindDto,
    ProgrammaticRootOriginRuleRecordDto, RecoverProgrammaticPolicyReservationInputDto,
    ReleaseProgrammaticPolicyReservationInputDto, ReserveProgrammaticPolicyActionInputDto,
    ReserveProgrammaticPolicyActionOutcomeDto, TransitionProgrammaticPolicyLifecycleInputDto,
};
use intention_types::{DtoResult, ErrorDto};
use sqlite::OptionalExtension;

use super::{
    SqliteStorageRepository, codec_error, conflict, not_found, sqlite_integer, storage_error,
};

/// The current single-version DDL of the Slice 3 programmatic-caller policy family.
pub const SCHEMA_POLICY_SQL: &str = "
-- Slice 3 programmatic-caller policy family (owner: src/programmatic_policy_repo.rs):
-- policy identities with immutable revisions, effective snapshots, exact
-- confirmations, bounded corridors, inactive drafts, shared counters, and
-- admission reservations.
CREATE TABLE IF NOT EXISTS programmatic_policies (
  policy_id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','goal','session')),
  project_id TEXT NOT NULL,
  goal_id TEXT,
  owner_session_id TEXT,
  calendar_period_kind TEXT NOT NULL CHECK(calendar_period_kind IN ('day','week','month')),
  lifecycle_state TEXT NOT NULL CHECK(lifecycle_state IN ('active','suspended','revoked','archived')),
  active_revision INTEGER NOT NULL CHECK(active_revision > 0),
  canonical_policy_digest TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
  CHECK((scope_kind = 'project' AND goal_id IS NULL AND owner_session_id IS NULL)
     OR (scope_kind = 'goal' AND goal_id IS NOT NULL AND owner_session_id IS NULL)
     OR (scope_kind = 'session' AND goal_id IS NULL AND owner_session_id IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS programmatic_policies_by_project ON programmatic_policies(project_id);
CREATE INDEX IF NOT EXISTS programmatic_policies_by_goal ON programmatic_policies(goal_id);
CREATE TABLE IF NOT EXISTS programmatic_policy_revisions (
  policy_id TEXT NOT NULL REFERENCES programmatic_policies(policy_id),
  revision INTEGER NOT NULL CHECK(revision > 0),
  root_origin_rules TEXT NOT NULL,
  admission_decisions TEXT NOT NULL,
  max_actions_per_run INTEGER NOT NULL CHECK(max_actions_per_run > 0),
  max_concurrent_actions_per_run INTEGER NOT NULL CHECK(max_concurrent_actions_per_run > 0),
  calendar_period_kind TEXT NOT NULL CHECK(calendar_period_kind IN ('day','week','month')),
  calendar_max_actions INTEGER NOT NULL CHECK(calendar_max_actions > 0),
  inherited_policy_references TEXT NOT NULL,
  canonical_revision_digest TEXT NOT NULL,
  PRIMARY KEY(policy_id, revision)
);
CREATE TABLE IF NOT EXISTS programmatic_policy_snapshots (
  snapshot_id TEXT PRIMARY KEY,
  root_origin_kind TEXT NOT NULL CHECK(root_origin_kind IN ('interactive_user','continual_harness')),
  policy_references TEXT NOT NULL,
  decision_ceiling TEXT NOT NULL CHECK(decision_ceiling IN ('prohibited','direct_local_read','exact_confirmation_required','bounded_confirmation_required')),
  max_actions_per_run INTEGER NOT NULL CHECK(max_actions_per_run > 0),
  max_concurrent_actions_per_run INTEGER NOT NULL CHECK(max_concurrent_actions_per_run > 0),
  calendar_limits TEXT NOT NULL,
  calendar_counter_references TEXT NOT NULL,
  baseline_max_actions INTEGER CHECK(baseline_max_actions IS NULL OR baseline_max_actions > 0),
  baseline_max_concurrent_actions INTEGER CHECK(baseline_max_concurrent_actions IS NULL OR baseline_max_concurrent_actions > 0),
  snapshot_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
);
CREATE TABLE IF NOT EXISTS programmatic_policy_confirmations (
  confirmation_id TEXT PRIMARY KEY,
  root_session_id TEXT NOT NULL,
  root_run_id TEXT NOT NULL,
  tool_call_id TEXT NOT NULL UNIQUE,
  tool_id TEXT NOT NULL,
  descriptor_revision TEXT NOT NULL,
  mcp_method_reference TEXT,
  typed_input_digest TEXT NOT NULL,
  policy_snapshot_digest TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('awaiting','accepted','rejected','expired','cancelled')),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  decided_at_ms INTEGER CHECK(decided_at_ms IS NULL OR decided_at_ms >= 0),
  CHECK((state = 'awaiting' AND decided_at_ms IS NULL) OR state <> 'awaiting')
);
CREATE INDEX IF NOT EXISTS programmatic_confirmations_by_root ON programmatic_policy_confirmations(root_run_id, state);
CREATE TABLE IF NOT EXISTS programmatic_authorization_corridors (
  corridor_digest TEXT PRIMARY KEY,
  root_session_id TEXT NOT NULL,
  root_run_id TEXT NOT NULL,
  root_origin_kind TEXT NOT NULL CHECK(root_origin_kind IN ('interactive_user','continual_harness')),
  policy_snapshot_reference TEXT NOT NULL,
  required_effect_selectors TEXT NOT NULL,
  exact_tool_or_method_selectors TEXT NOT NULL,
  descriptor_input_constraint_selections TEXT NOT NULL,
  maximum_action_count INTEGER NOT NULL CHECK(maximum_action_count > 0),
  maximum_concurrent_actions INTEGER NOT NULL CHECK(maximum_concurrent_actions > 0),
  confirmation_reference TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('active','expired','exhausted','revoked')),
  consumed_action_count INTEGER NOT NULL CHECK(consumed_action_count >= 0),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  CHECK(maximum_concurrent_actions <= maximum_action_count),
  CHECK(consumed_action_count <= maximum_action_count)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_active_policy_corridor_per_root ON programmatic_authorization_corridors(root_run_id) WHERE state = 'active';
CREATE TABLE IF NOT EXISTS programmatic_policy_drafts (
  draft_id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','goal','session')),
  project_id TEXT NOT NULL,
  owner_id TEXT,
  record_kind TEXT NOT NULL,
  base_revision INTEGER NOT NULL CHECK(base_revision > 0),
  evidence_references TEXT NOT NULL,
  safe_rationale TEXT NOT NULL,
  canonical_draft_digest TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending','accepted','rejected')),
  coalesced_evidence_count INTEGER NOT NULL CHECK(coalesced_evidence_count > 0),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  decided_at_ms INTEGER CHECK(decided_at_ms IS NULL OR decided_at_ms >= 0),
  CHECK((scope_kind = 'project' AND owner_id IS NULL) OR (scope_kind <> 'project' AND owner_id IS NOT NULL)),
  CHECK((state = 'pending' AND decided_at_ms IS NULL) OR state <> 'pending')
);
CREATE UNIQUE INDEX IF NOT EXISTS one_pending_policy_draft ON programmatic_policy_drafts(scope_kind, project_id, COALESCE(owner_id, ''), record_kind) WHERE state = 'pending';
CREATE TABLE IF NOT EXISTS programmatic_policy_counters (
  policy_id TEXT PRIMARY KEY REFERENCES programmatic_policies(policy_id),
  run_started_actions INTEGER NOT NULL CHECK(run_started_actions >= 0),
  run_reserved_actions INTEGER NOT NULL CHECK(run_reserved_actions >= 0),
  run_in_flight_actions INTEGER NOT NULL CHECK(run_in_flight_actions >= 0),
  calendar_started_actions INTEGER NOT NULL CHECK(calendar_started_actions >= 0),
  calendar_reserved_actions INTEGER NOT NULL CHECK(calendar_reserved_actions >= 0),
  calendar_window_start_ms INTEGER NOT NULL CHECK(calendar_window_start_ms >= 0),
  calendar_window_end_ms INTEGER NOT NULL CHECK(calendar_window_end_ms >= 0),
  calendar_window_time_zone TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0)
);
CREATE TABLE IF NOT EXISTS programmatic_policy_reservations (
  reservation_reference TEXT PRIMARY KEY,
  policy_id TEXT NOT NULL REFERENCES programmatic_policies(policy_id),
  policy_revision INTEGER NOT NULL CHECK(policy_revision > 0),
  root_run_id TEXT NOT NULL,
  tool_call_id TEXT NOT NULL UNIQUE,
  typed_input_digest TEXT NOT NULL,
  calendar_counter_reference TEXT NOT NULL,
  reserved_at_ms INTEGER NOT NULL CHECK(reserved_at_ms >= 0),
  state TEXT NOT NULL CHECK(state IN ('reserved','permanent_on_start','released_on_known_pre_effect','interrupted_before_start','external_effect_unknown')),
  finished_at_ms INTEGER CHECK(finished_at_ms IS NULL OR finished_at_ms >= 0)
);
CREATE INDEX IF NOT EXISTS programmatic_reservations_by_run ON programmatic_policy_reservations(root_run_id, reserved_at_ms);
";

/// The unit separator joining encoded fields or list items.
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

/// Reads one non-negative integer column as `u64`.
fn u64_column(row: &sqlite::Row<'_>, index: usize) -> Result<u64, sqlite::Error> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| sqlite::Error::IntegralValueOutOfRange(index, value))
}

fn policy_not_applicable_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        not_found(
            "programmatic_policy_not_applicable",
            "the requested durable policy does not exist",
        )
    } else {
        storage_error(error)
    }
}

fn policy_revision_conflict_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        policy_revision_conflict()
    } else {
        storage_error(error)
    }
}

fn policy_revision_conflict() -> ErrorDto {
    conflict(
        "programmatic_policy_revision_conflict",
        "the durable policy revision is stale or already bound to different content",
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

fn load_policy(
    connection: &sqlite::Connection,
    policy_id: &str,
) -> DtoResult<ProgrammaticPolicyRecordDto> {
    connection
        .query_row(
            "SELECT scope_kind, project_id, goal_id, owner_session_id, calendar_period_kind,
                    lifecycle_state, active_revision, canonical_policy_digest, updated_at_ms
             FROM programmatic_policies WHERE policy_id=?1",
            [policy_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    u64_column(row, 6)?,
                    row.get::<_, String>(7)?,
                    u64_column(row, 8)?,
                ))
            },
        )
        .map_err(policy_not_applicable_or_storage)
        .and_then(|row| {
            let scope = match (row.0.as_str(), row.2, row.3) {
                ("project", None, None) => {
                    ProgrammaticPolicyScopeDto::Project { project_id: row.1 }
                }
                ("goal", Some(goal_id), None) => ProgrammaticPolicyScopeDto::Goal {
                    project_id: row.1,
                    goal_id,
                },
                ("session", None, Some(owner_session_id)) => ProgrammaticPolicyScopeDto::Session {
                    project_id: row.1,
                    owner_session_id,
                },
                _ => return Err(codec_error("persisted policy scope is malformed")),
            };
            Ok(ProgrammaticPolicyRecordDto {
                policy_id: policy_id.to_owned(),
                scope,
                calendar_period_kind: ProgrammaticCalendarPeriodKindDto::parse(&row.4)?,
                lifecycle_state: ProgrammaticPolicyLifecycleStateDto::parse(&row.5)?,
                active_revision: row.6,
                canonical_policy_digest: row.7,
                updated_at_ms: row.8,
            })
        })
}

fn decode_root_origin_rules(encoded: &str) -> DtoResult<Vec<ProgrammaticRootOriginRuleRecordDto>> {
    decode_records(encoded)?
        .into_iter()
        .map(|fields| {
            if fields.len() != 2 {
                return Err(codec_error(
                    "persisted root-origin rule framing is malformed",
                ));
            }
            Ok(ProgrammaticRootOriginRuleRecordDto {
                root_origin_kind: ProgrammaticRootOriginKindDto::parse(&fields[0])?,
                maximum_decision: ProgrammaticAdmissionDecisionDto::parse(&fields[1])?,
            })
        })
        .collect()
}

fn encode_root_origin_rules(rules: &[ProgrammaticRootOriginRuleRecordDto]) -> String {
    encode_records(
        &rules
            .iter()
            .map(|rule| {
                vec![
                    rule.root_origin_kind.name().to_owned(),
                    rule.maximum_decision.name().to_owned(),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

fn decode_references(encoded: &str) -> DtoResult<Vec<ProgrammaticPolicyRevisionReferenceDto>> {
    decode_records(encoded)?
        .into_iter()
        .map(|fields| {
            if fields.len() != 2 {
                return Err(codec_error(
                    "persisted policy reference framing is malformed",
                ));
            }
            let revision = fields[1]
                .parse::<u64>()
                .map_err(|_| codec_error("persisted policy reference revision is malformed"))?;
            Ok(ProgrammaticPolicyRevisionReferenceDto {
                policy_id: fields[0].clone(),
                revision,
            })
        })
        .collect()
}

fn encode_references(references: &[ProgrammaticPolicyRevisionReferenceDto]) -> String {
    encode_records(
        &references
            .iter()
            .map(|reference| vec![reference.policy_id.clone(), reference.revision.to_string()])
            .collect::<Vec<_>>(),
    )
}

fn load_policy_revision(
    connection: &sqlite::Connection,
    policy_id: &str,
    revision: u64,
) -> DtoResult<ProgrammaticPolicyRevisionRecordDto> {
    connection
        .query_row(
            "SELECT revision, root_origin_rules, admission_decisions, max_actions_per_run,
                    max_concurrent_actions_per_run, calendar_period_kind, calendar_max_actions,
                    inherited_policy_references, canonical_revision_digest
             FROM programmatic_policy_revisions WHERE policy_id=?1 AND revision=?2",
            sqlite::params![
                policy_id,
                sqlite_integer(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    u64_column(row, 0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    u64_column(row, 3)?,
                    u64_column(row, 4)?,
                    row.get::<_, String>(5)?,
                    u64_column(row, 6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .map_err(policy_revision_conflict_or_storage)
        .and_then(|row| {
            Ok(ProgrammaticPolicyRevisionRecordDto {
                policy_id: policy_id.to_owned(),
                revision: row.0,
                root_origin_rules: decode_root_origin_rules(&row.1)?,
                admission_decisions: decode_items(&row.2)?
                    .iter()
                    .map(|name| ProgrammaticAdmissionDecisionDto::parse(name))
                    .collect::<DtoResult<Vec<_>>>()?,
                max_actions_per_run: row.3,
                max_concurrent_actions_per_run: row.4,
                calendar_period_kind: ProgrammaticCalendarPeriodKindDto::parse(&row.5)?,
                calendar_max_actions: row.6,
                inherited_policy_references: decode_references(&row.7)?,
                canonical_revision_digest: row.8,
            })
        })
}

fn insert_policy_revision(
    connection: &sqlite::Connection,
    revision: &ProgrammaticPolicyRevisionRecordDto,
) -> DtoResult<()> {
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO programmatic_policy_revisions(
                policy_id, revision, root_origin_rules, admission_decisions, max_actions_per_run,
                max_concurrent_actions_per_run, calendar_period_kind, calendar_max_actions,
                inherited_policy_references, canonical_revision_digest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            sqlite::params![
                revision.policy_id,
                sqlite_integer(revision.revision, "revision is outside the SQLite range")?,
                encode_root_origin_rules(&revision.root_origin_rules),
                encode_items(
                    &revision
                        .admission_decisions
                        .iter()
                        .map(|decision| decision.name().to_owned())
                        .collect::<Vec<_>>()
                ),
                sqlite_integer(
                    revision.max_actions_per_run,
                    "limit is outside the SQLite range"
                )?,
                sqlite_integer(
                    revision.max_concurrent_actions_per_run,
                    "limit is outside the SQLite range"
                )?,
                revision.calendar_period_kind.name(),
                sqlite_integer(
                    revision.calendar_max_actions,
                    "limit is outside the SQLite range"
                )?,
                encode_references(&revision.inherited_policy_references),
                revision.canonical_revision_digest,
            ],
        )
        .map_err(storage_error)?;
    if inserted == 0 {
        let existing = load_policy_revision(connection, &revision.policy_id, revision.revision)?;
        if existing != *revision {
            return Err(policy_revision_conflict());
        }
    }
    Ok(())
}

type SnapshotRow = (
    String,
    String,
    String,
    u64,
    u64,
    String,
    String,
    Option<u64>,
    Option<u64>,
    String,
    u64,
);

fn snapshot_from_columns(
    snapshot_id: &str,
    row: &SnapshotRow,
) -> DtoResult<ProgrammaticPolicySnapshotRecordDto> {
    Ok(ProgrammaticPolicySnapshotRecordDto {
        snapshot_id: snapshot_id.to_owned(),
        root_origin_kind: ProgrammaticRootOriginKindDto::parse(&row.0)?,
        policy_references: decode_references(&row.1)?,
        decision_ceiling: ProgrammaticAdmissionDecisionDto::parse(&row.2)?,
        max_actions_per_run: row.3,
        max_concurrent_actions_per_run: row.4,
        calendar_limits: decode_records(&row.5)?
            .into_iter()
            .map(|fields| {
                if fields.len() != 2 {
                    return Err(codec_error("persisted calendar limit is malformed"));
                }
                Ok(ProgrammaticCalendarLimitRecordDto {
                    period_kind: ProgrammaticCalendarPeriodKindDto::parse(&fields[0])?,
                    max_actions: fields[1]
                        .parse::<u64>()
                        .map_err(|_| codec_error("persisted calendar limit value is malformed"))?,
                })
            })
            .collect::<DtoResult<Vec<_>>>()?,
        calendar_counter_references: decode_records(&row.6)?
            .into_iter()
            .map(|fields| {
                if fields.len() != 2 {
                    return Err(codec_error("persisted calendar counter is malformed"));
                }
                Ok(ProgrammaticCalendarCounterReferenceDto {
                    policy_id: fields[0].clone(),
                    period_kind: ProgrammaticCalendarPeriodKindDto::parse(&fields[1])?,
                })
            })
            .collect::<DtoResult<Vec<_>>>()?,
        baseline_max_actions: row.7,
        baseline_max_concurrent_actions: row.8,
        snapshot_digest: row.9.clone(),
        created_at_ms: row.10,
    })
}

fn load_snapshot(
    connection: &sqlite::Connection,
    snapshot_id: &str,
) -> DtoResult<ProgrammaticPolicySnapshotRecordDto> {
    connection
        .query_row(
            "SELECT root_origin_kind, policy_references, decision_ceiling, max_actions_per_run,
                    max_concurrent_actions_per_run, calendar_limits, calendar_counter_references,
                    baseline_max_actions, baseline_max_concurrent_actions, snapshot_digest,
                    created_at_ms
             FROM programmatic_policy_snapshots WHERE snapshot_id=?1",
            [snapshot_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    u64_column(row, 3)?,
                    u64_column(row, 4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<i64>>(7)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| sqlite::Error::IntegralValueOutOfRange(7, i64::MIN))?,
                    row.get::<_, Option<i64>>(8)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| sqlite::Error::IntegralValueOutOfRange(8, i64::MIN))?,
                    row.get::<_, String>(9)?,
                    u64_column(row, 10)?,
                ))
            },
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                not_found(
                    "programmatic_policy_snapshot_unavailable",
                    "the requested effective policy snapshot does not exist",
                )
            } else {
                storage_error(error)
            }
        })
        .and_then(|row| snapshot_from_columns(snapshot_id, &row))
}

type ConfirmationRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    u64,
    Option<u64>,
);

fn confirmation_from_columns(
    confirmation_id: &str,
    row: &ConfirmationRow,
) -> DtoResult<intention_storage::programmatic_policy_repo::ProgrammaticPolicyConfirmationRecordDto>
{
    Ok(
        intention_storage::programmatic_policy_repo::ProgrammaticPolicyConfirmationRecordDto {
            confirmation_id: confirmation_id.to_owned(),
            root_session_id: row.0.clone(),
            root_run_id: row.1.clone(),
            tool_call_id: row.2.clone(),
            tool_id: row.3.clone(),
            descriptor_revision: row.4.clone(),
            mcp_method_reference: row.5.clone(),
            typed_input_digest: row.6.clone(),
            policy_snapshot_digest: row.7.clone(),
            state: ProgrammaticConfirmationStateDto::parse(&row.8)?,
            created_at_ms: row.9,
            decided_at_ms: row.10,
        },
    )
}

fn load_confirmation(
    connection: &sqlite::Connection,
    confirmation_id: &str,
) -> DtoResult<intention_storage::programmatic_policy_repo::ProgrammaticPolicyConfirmationRecordDto>
{
    connection
        .query_row(
            "SELECT root_session_id, root_run_id, tool_call_id, tool_id, descriptor_revision,
                    mcp_method_reference, typed_input_digest, policy_snapshot_digest, state,
                    created_at_ms, decided_at_ms
             FROM programmatic_policy_confirmations WHERE confirmation_id=?1",
            [confirmation_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    u64_column(row, 9)?,
                    row.get::<_, Option<i64>>(10)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| sqlite::Error::IntegralValueOutOfRange(10, i64::MIN))?,
                ))
            },
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                not_found(
                    "programmatic_policy_confirmation_required",
                    "the requested exact confirmation does not exist",
                )
            } else {
                storage_error(error)
            }
        })
        .and_then(|row| confirmation_from_columns(confirmation_id, &row))
}

fn load_confirmation_by_call(
    connection: &sqlite::Connection,
    tool_call_id: &str,
) -> DtoResult<
    Option<intention_storage::programmatic_policy_repo::ProgrammaticPolicyConfirmationRecordDto>,
> {
    connection
        .query_row(
            "SELECT confirmation_id, root_session_id, root_run_id, tool_call_id, tool_id,
                    descriptor_revision, mcp_method_reference, typed_input_digest,
                    policy_snapshot_digest, state, created_at_ms, decided_at_ms
             FROM programmatic_policy_confirmations WHERE tool_call_id=?1",
            [tool_call_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    u64_column(row, 10)?,
                    row.get::<_, Option<i64>>(11)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| sqlite::Error::IntegralValueOutOfRange(11, i64::MIN))?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
        .and_then(|row| {
            row.map(|row| {
                let confirmation_id = row.0;
                confirmation_from_columns(
                    &confirmation_id,
                    &(
                        row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10,
                        row.11,
                    ),
                )
            })
            .transpose()
        })
}

type CorridorRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    u64,
    u64,
    String,
    String,
    u64,
    u64,
);

fn corridor_from_columns(
    corridor_digest: &str,
    row: &CorridorRow,
) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto> {
    Ok(ProgrammaticAuthorizationCorridorRecordDto {
        corridor_digest: corridor_digest.to_owned(),
        root_session_id: row.0.clone(),
        root_run_id: row.1.clone(),
        root_origin_kind: ProgrammaticRootOriginKindDto::parse(&row.2)?,
        policy_snapshot_reference: row.3.clone(),
        required_effect_selectors: decode_items(&row.4)?,
        exact_tool_or_method_selectors: decode_items(&row.5)?,
        descriptor_input_constraint_selections: decode_items(&row.6)?,
        maximum_action_count: row.7,
        maximum_concurrent_actions: row.8,
        confirmation_reference: row.9.clone(),
        state: ProgrammaticCorridorStateDto::parse(&row.10)?,
        consumed_action_count: row.11,
        created_at_ms: row.12,
    })
}

fn load_corridor(
    connection: &sqlite::Connection,
    corridor_digest: &str,
) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto> {
    connection
        .query_row(
            "SELECT root_session_id, root_run_id, root_origin_kind, policy_snapshot_reference,
                    required_effect_selectors, exact_tool_or_method_selectors,
                    descriptor_input_constraint_selections, maximum_action_count,
                    maximum_concurrent_actions, confirmation_reference, state,
                    consumed_action_count, created_at_ms
             FROM programmatic_authorization_corridors WHERE corridor_digest=?1",
            [corridor_digest],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    u64_column(row, 7)?,
                    u64_column(row, 8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    u64_column(row, 11)?,
                    u64_column(row, 12)?,
                ))
            },
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                not_found(
                    "programmatic_policy_corridor_unavailable",
                    "the requested confirmation corridor does not exist",
                )
            } else {
                storage_error(error)
            }
        })
        .and_then(|row| corridor_from_columns(corridor_digest, &row))
}

fn load_active_corridor(
    connection: &sqlite::Connection,
    root_run_id: &str,
) -> DtoResult<Option<ProgrammaticAuthorizationCorridorRecordDto>> {
    connection
        .query_row(
            "SELECT corridor_digest, root_session_id, root_run_id, root_origin_kind,
                    policy_snapshot_reference, required_effect_selectors,
                    exact_tool_or_method_selectors, descriptor_input_constraint_selections,
                    maximum_action_count, maximum_concurrent_actions, confirmation_reference,
                    state, consumed_action_count, created_at_ms
             FROM programmatic_authorization_corridors
             WHERE root_run_id=?1 AND state='active' LIMIT 1",
            [root_run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    u64_column(row, 8)?,
                    u64_column(row, 9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    u64_column(row, 12)?,
                    u64_column(row, 13)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
        .and_then(|row| {
            row.map(|row| {
                let corridor_digest = row.0;
                corridor_from_columns(
                    &corridor_digest,
                    &(
                        row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10,
                        row.11, row.12, row.13,
                    ),
                )
            })
            .transpose()
        })
}

fn draft_from_columns(
    draft_id: &str,
    row: &DraftRow,
) -> DtoResult<ProgrammaticPolicyDraftRecordDto> {
    let scope = match row.1.as_str() {
        "project" => ProgrammaticPolicyScopeDto::Project {
            project_id: row.2.clone(),
        },
        "goal" => ProgrammaticPolicyScopeDto::Goal {
            project_id: row.2.clone(),
            goal_id: row.3.clone().unwrap_or_default(),
        },
        "session" => ProgrammaticPolicyScopeDto::Session {
            project_id: row.2.clone(),
            owner_session_id: row.3.clone().unwrap_or_default(),
        },
        _ => return Err(codec_error("persisted policy draft scope is malformed")),
    };
    Ok(ProgrammaticPolicyDraftRecordDto {
        draft_id: draft_id.to_owned(),
        scope,
        record_kind: row.4.clone(),
        base_revision: row.5,
        evidence_references: decode_items(&row.6)?,
        safe_rationale: row.7.clone(),
        canonical_draft_digest: row.8.clone(),
        state: ProgrammaticPolicyDraftStateDto::parse(&row.9)?,
        coalesced_evidence_count: row.10,
        created_at_ms: row.11,
        decided_at_ms: row.12,
    })
}

type DraftRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    u64,
    String,
    String,
    String,
    String,
    u64,
    u64,
    Option<u64>,
);

fn load_draft(
    connection: &sqlite::Connection,
    draft_id: &str,
) -> DtoResult<ProgrammaticPolicyDraftRecordDto> {
    let row: DraftRow = connection
        .query_row(
            "SELECT draft_id, scope_kind, project_id, owner_id, record_kind, base_revision,
                    evidence_references, safe_rationale, canonical_draft_digest, state,
                    coalesced_evidence_count, created_at_ms, decided_at_ms
             FROM programmatic_policy_drafts WHERE draft_id=?1",
            [draft_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    u64_column(row, 5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    u64_column(row, 10)?,
                    u64_column(row, 11)?,
                    row.get::<_, Option<i64>>(12)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| sqlite::Error::IntegralValueOutOfRange(12, i64::MIN))?,
                ))
            },
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                conflict(
                    "programmatic_policy_draft_conflict",
                    "the requested policy draft does not exist",
                )
            } else {
                storage_error(error)
            }
        })?;
    draft_from_columns(draft_id, &row)
}

fn load_pending_draft(
    connection: &sqlite::Connection,
    scope_kind: &str,
    project_id: &str,
    owner_id: Option<&str>,
    record_kind: &str,
) -> DtoResult<Option<ProgrammaticPolicyDraftRecordDto>> {
    let row: Option<DraftRow> = connection
        .query_row(
            "SELECT draft_id, scope_kind, project_id, owner_id, record_kind, base_revision,
                    evidence_references, safe_rationale, canonical_draft_digest, state,
                    coalesced_evidence_count, created_at_ms, decided_at_ms
             FROM programmatic_policy_drafts
             WHERE scope_kind=?1 AND project_id=?2 AND COALESCE(owner_id, '')=?3
               AND record_kind=?4 AND state='pending' LIMIT 1",
            sqlite::params![
                scope_kind,
                project_id,
                owner_id.unwrap_or_default(),
                record_kind
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    u64_column(row, 5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    u64_column(row, 10)?,
                    u64_column(row, 11)?,
                    row.get::<_, Option<i64>>(12)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| sqlite::Error::IntegralValueOutOfRange(12, i64::MIN))?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    row.map(|row| {
        let draft_id = row.0.clone();
        draft_from_columns(&draft_id, &row)
    })
    .transpose()
}

fn counters_from_row(policy_id: &str, row: &CounterRow) -> ProgrammaticPolicyCounterRecordDto {
    ProgrammaticPolicyCounterRecordDto {
        policy_id: policy_id.to_owned(),
        run_started_actions: row.0,
        run_reserved_actions: row.1,
        run_in_flight_actions: row.2,
        calendar_started_actions: row.3,
        calendar_reserved_actions: row.4,
        calendar_window_start_ms: row.5,
        calendar_window_end_ms: row.6,
        calendar_window_time_zone: row.7.clone(),
        updated_at_ms: row.8,
    }
}

type CounterRow = (u64, u64, u64, u64, u64, u64, u64, String, u64);

fn load_counters(
    connection: &sqlite::Connection,
    policy_id: &str,
) -> DtoResult<Option<ProgrammaticPolicyCounterRecordDto>> {
    connection
        .query_row(
            "SELECT run_started_actions, run_reserved_actions, run_in_flight_actions,
                    calendar_started_actions, calendar_reserved_actions, calendar_window_start_ms,
                    calendar_window_end_ms, calendar_window_time_zone, updated_at_ms
             FROM programmatic_policy_counters WHERE policy_id=?1",
            [policy_id],
            |row| {
                Ok((
                    u64_column(row, 0)?,
                    u64_column(row, 1)?,
                    u64_column(row, 2)?,
                    u64_column(row, 3)?,
                    u64_column(row, 4)?,
                    u64_column(row, 5)?,
                    u64_column(row, 6)?,
                    row.get::<_, String>(7)?,
                    u64_column(row, 8)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
        .map(|row| row.map(|row| counters_from_row(policy_id, &row)))
}

fn store_counters(
    connection: &sqlite::Connection,
    policy_id: &str,
    counters: &ProgrammaticPolicyCounterRecordDto,
) -> DtoResult<()> {
    connection
        .execute(
            "UPDATE programmatic_policy_counters SET run_started_actions=?2,
                run_reserved_actions=?3, run_in_flight_actions=?4, calendar_started_actions=?5,
                calendar_reserved_actions=?6, calendar_window_start_ms=?7,
                calendar_window_end_ms=?8, calendar_window_time_zone=?9, updated_at_ms=?10
             WHERE policy_id=?1",
            sqlite::params![
                policy_id,
                sqlite_integer(
                    counters.run_started_actions,
                    "counter is outside the SQLite range"
                )?,
                sqlite_integer(
                    counters.run_reserved_actions,
                    "counter is outside the SQLite range"
                )?,
                sqlite_integer(
                    counters.run_in_flight_actions,
                    "counter is outside the SQLite range"
                )?,
                sqlite_integer(
                    counters.calendar_started_actions,
                    "counter is outside the SQLite range"
                )?,
                sqlite_integer(
                    counters.calendar_reserved_actions,
                    "counter is outside the SQLite range"
                )?,
                sqlite_integer(
                    counters.calendar_window_start_ms,
                    "timestamp is outside the SQLite range"
                )?,
                sqlite_integer(
                    counters.calendar_window_end_ms,
                    "timestamp is outside the SQLite range"
                )?,
                counters.calendar_window_time_zone,
                sqlite_integer(
                    counters.updated_at_ms,
                    "timestamp is outside the SQLite range"
                )?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn reservation_from_columns(
    reservation_reference: &str,
    row: &(
        String,
        u64,
        String,
        String,
        String,
        String,
        u64,
        String,
        Option<u64>,
    ),
) -> DtoResult<ProgrammaticPolicyReservationRecordDto> {
    Ok(ProgrammaticPolicyReservationRecordDto {
        reservation_reference: reservation_reference.to_owned(),
        policy_id: row.0.clone(),
        policy_revision: row.1,
        root_run_id: row.2.clone(),
        tool_call_id: row.3.clone(),
        typed_input_digest: row.4.clone(),
        calendar_counter_reference: row.5.clone(),
        reserved_at_ms: row.6,
        state: ProgrammaticReservationStateDto::parse(&row.7)?,
        finished_at_ms: row.8,
    })
}

type ReservationRow = (
    String,
    u64,
    String,
    String,
    String,
    String,
    u64,
    String,
    Option<u64>,
);

fn load_reservation(
    connection: &sqlite::Connection,
    reservation_reference: &str,
) -> DtoResult<ProgrammaticPolicyReservationRecordDto> {
    let row: ReservationRow = connection
        .query_row(
            "SELECT policy_id, policy_revision, root_run_id, tool_call_id, typed_input_digest,
                    calendar_counter_reference, reserved_at_ms, state, finished_at_ms
             FROM programmatic_policy_reservations WHERE reservation_reference=?1",
            [reservation_reference],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    u64_column(row, 1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    u64_column(row, 6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<i64>>(8)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| sqlite::Error::IntegralValueOutOfRange(8, i64::MIN))?,
                ))
            },
        )
        .map_err(|error| {
            if matches!(error, sqlite::Error::QueryReturnedNoRows) {
                not_found(
                    "programmatic_policy_reservation_conflict",
                    "the requested admission reservation does not exist",
                )
            } else {
                storage_error(error)
            }
        })?;
    reservation_from_columns(reservation_reference, &row)
}

fn reservation_conflict() -> ErrorDto {
    conflict(
        "programmatic_policy_reservation_conflict",
        "the admission reservation is not outstanding or does not match its exact binding",
    )
}

fn draft_scope_columns(scope: &ProgrammaticPolicyScopeDto) -> (String, String, Option<String>) {
    match scope {
        ProgrammaticPolicyScopeDto::Project { project_id } => {
            ("project".to_owned(), project_id.clone(), None)
        }
        ProgrammaticPolicyScopeDto::Goal {
            project_id,
            goal_id,
        } => ("goal".to_owned(), project_id.clone(), Some(goal_id.clone())),
        ProgrammaticPolicyScopeDto::Session {
            project_id,
            owner_session_id,
        } => (
            "session".to_owned(),
            project_id.clone(),
            Some(owner_session_id.clone()),
        ),
    }
}

fn insert_policy_row(
    connection: &sqlite::Connection,
    policy: &ProgrammaticPolicyRecordDto,
) -> DtoResult<()> {
    connection
        .execute(
            "INSERT INTO programmatic_policies(
                policy_id, scope_kind, project_id, goal_id, owner_session_id,
                calendar_period_kind, lifecycle_state, active_revision,
                canonical_policy_digest, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            sqlite::params![
                policy.policy_id,
                policy.scope.kind_name(),
                policy.scope.project_id(),
                policy.scope.goal_id(),
                policy.scope.owner_session_id(),
                policy.calendar_period_kind.name(),
                policy.lifecycle_state.name(),
                sqlite_integer(
                    policy.active_revision,
                    "revision is outside the SQLite range"
                )?,
                policy.canonical_policy_digest,
                sqlite_integer(
                    policy.updated_at_ms,
                    "timestamp is outside the SQLite range"
                )?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn insert_counters_row(
    connection: &sqlite::Connection,
    policy_id: &str,
    updated_at_ms: u64,
) -> DtoResult<()> {
    connection
        .execute(
            "INSERT INTO programmatic_policy_counters(
                policy_id, run_started_actions, run_reserved_actions, run_in_flight_actions,
                calendar_started_actions, calendar_reserved_actions, calendar_window_start_ms,
                calendar_window_end_ms, calendar_window_time_zone, updated_at_ms)
             VALUES (?1, 0, 0, 0, 0, 0, 0, 0, '', ?2)",
            sqlite::params![
                policy_id,
                sqlite_integer(updated_at_ms, "timestamp is outside the SQLite range")?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

impl ProgrammaticPolicyRepositoryDto for SqliteStorageRepository {
    fn create_programmatic_policy(
        &self,
        input: CreateProgrammaticPolicyInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto> {
        input.validate()?;
        let policy = input.policy;
        let revision = input.revision;
        write(self, |connection| {
            let existing: Option<String> = connection
                .query_row(
                    "SELECT policy_id FROM programmatic_policies WHERE policy_id=?1",
                    [&policy.policy_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            if existing.is_some() {
                let stored = load_policy(connection, &policy.policy_id)?;
                if stored != policy {
                    return Err(policy_revision_conflict());
                }
                return Ok(stored);
            }
            let project_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM programmatic_policies WHERE project_id=?1",
                    [policy.scope.project_id()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let project_count =
                usize::try_from(project_count).map_err(|_| codec_error("invalid policy count"))?;
            policy_domain::validate_policies_in_project(project_count + 1)?;
            if let Some(goal_id) = policy.scope.goal_id() {
                let goal_count: i64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM programmatic_policies WHERE goal_id=?1",
                        [goal_id],
                        |row| row.get(0),
                    )
                    .map_err(storage_error)?;
                let goal_count =
                    usize::try_from(goal_count).map_err(|_| codec_error("invalid policy count"))?;
                policy_domain::validate_policies_attached_to_goal(goal_count + 1)?;
            }
            insert_policy_row(connection, &policy)?;
            insert_policy_revision(connection, &revision)?;
            insert_counters_row(connection, &policy.policy_id, policy.updated_at_ms)?;
            Ok(policy)
        })
    }

    fn load_programmatic_policy(
        &self,
        policy_id: String,
    ) -> DtoResult<ProgrammaticPolicyRecordDto> {
        let connection = self.connection()?;
        load_policy(&connection, &policy_id)
    }

    fn load_programmatic_policy_revision(
        &self,
        policy_id: String,
        revision: u64,
    ) -> DtoResult<ProgrammaticPolicyRevisionRecordDto> {
        let connection = self.connection()?;
        load_policy_revision(&connection, &policy_id, revision)
    }

    fn append_programmatic_policy_revision(
        &self,
        input: AppendProgrammaticPolicyRevisionInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto> {
        input.validate()?;
        let policy_id = input.policy_id;
        let expected_revision = input.expected_revision;
        let revision = input.revision;
        write(self, |connection| {
            let policy = load_policy(connection, &policy_id)?;
            if policy.active_revision != expected_revision {
                return Err(policy_revision_conflict());
            }
            if revision.calendar_period_kind != policy.calendar_period_kind {
                return Err(policy_revision_conflict());
            }
            insert_policy_revision(connection, &revision)?;
            connection
                .execute(
                    "UPDATE programmatic_policies SET active_revision=?2 WHERE policy_id=?1",
                    sqlite::params![
                        policy_id,
                        sqlite_integer(revision.revision, "revision is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_policy(connection, &policy_id)
        })
    }

    fn transition_programmatic_policy_lifecycle(
        &self,
        input: TransitionProgrammaticPolicyLifecycleInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto> {
        let policy_id = input.policy_id;
        let expected_revision = input.expected_revision;
        let operation = input.operation;
        let has_active_dependent_tree = input.has_active_dependent_tree;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let policy = load_policy(connection, &policy_id)?;
            if policy.active_revision != expected_revision {
                return Err(policy_revision_conflict());
            }
            let lifecycle_state = match (policy.lifecycle_state, operation) {
                (
                    ProgrammaticPolicyLifecycleStateDto::Active,
                    ProgrammaticPolicyLifecycleOperationDto::Suspend,
                ) => ProgrammaticPolicyLifecycleStateDto::Suspended,
                (
                    ProgrammaticPolicyLifecycleStateDto::Suspended,
                    ProgrammaticPolicyLifecycleOperationDto::Resume,
                ) => ProgrammaticPolicyLifecycleStateDto::Active,
                (
                    ProgrammaticPolicyLifecycleStateDto::Active
                    | ProgrammaticPolicyLifecycleStateDto::Suspended,
                    ProgrammaticPolicyLifecycleOperationDto::Revoke,
                ) => ProgrammaticPolicyLifecycleStateDto::Revoked,
                (
                    ProgrammaticPolicyLifecycleStateDto::Revoked,
                    ProgrammaticPolicyLifecycleOperationDto::Archive,
                ) => {
                    if has_active_dependent_tree {
                        return Err(conflict(
                            "programmatic_policy_suspended",
                            "an active dependent tree blocks policy archival",
                        ));
                    }
                    ProgrammaticPolicyLifecycleStateDto::Archived
                }
                (
                    ProgrammaticPolicyLifecycleStateDto::Revoked
                    | ProgrammaticPolicyLifecycleStateDto::Archived,
                    _,
                ) => {
                    return Err(conflict(
                        "programmatic_policy_revoked",
                        "revocation is never undone and an archived policy rejects operations",
                    ));
                }
                _ => {
                    return Err(conflict(
                        "programmatic_policy_suspended",
                        "the requested lifecycle operation is not permitted by the durable state",
                    ));
                }
            };
            connection
                .execute(
                    "UPDATE programmatic_policies SET lifecycle_state=?2, updated_at_ms=?3
                     WHERE policy_id=?1",
                    sqlite::params![
                        policy_id,
                        lifecycle_state.name(),
                        sqlite_integer(occurred_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_policy(connection, &policy_id)
        })
    }

    fn count_programmatic_policies_in_project(&self, project_id: String) -> DtoResult<u64> {
        let connection = self.connection()?;
        count_policies(&connection, "project_id", &project_id)
    }

    fn count_programmatic_policies_on_goal(&self, goal_id: String) -> DtoResult<u64> {
        let connection = self.connection()?;
        count_policies(&connection, "goal_id", &goal_id)
    }

    fn store_programmatic_policy_snapshot(
        &self,
        input: ProgrammaticPolicySnapshotRecordDto,
    ) -> DtoResult<ProgrammaticPolicySnapshotRecordDto> {
        input.validate()?;
        write(self, |connection| {
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO programmatic_policy_snapshots(
                        snapshot_id, root_origin_kind, policy_references, decision_ceiling,
                        max_actions_per_run, max_concurrent_actions_per_run, calendar_limits,
                        calendar_counter_references, baseline_max_actions,
                        baseline_max_concurrent_actions, snapshot_digest, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    sqlite::params![
                        input.snapshot_id,
                        input.root_origin_kind.name(),
                        encode_references(&input.policy_references),
                        input.decision_ceiling.name(),
                        sqlite_integer(
                            input.max_actions_per_run,
                            "limit is outside the SQLite range"
                        )?,
                        sqlite_integer(
                            input.max_concurrent_actions_per_run,
                            "limit is outside the SQLite range"
                        )?,
                        encode_records(
                            &input
                                .calendar_limits
                                .iter()
                                .map(|limit| {
                                    vec![
                                        limit.period_kind.name().to_owned(),
                                        limit.max_actions.to_string(),
                                    ]
                                })
                                .collect::<Vec<_>>()
                        ),
                        encode_records(
                            &input
                                .calendar_counter_references
                                .iter()
                                .map(|counter| {
                                    vec![
                                        counter.policy_id.clone(),
                                        counter.period_kind.name().to_owned(),
                                    ]
                                })
                                .collect::<Vec<_>>()
                        ),
                        input
                            .baseline_max_actions
                            .map(|value| sqlite_integer(value, "limit is outside the SQLite range"))
                            .transpose()?,
                        input
                            .baseline_max_concurrent_actions
                            .map(|value| sqlite_integer(value, "limit is outside the SQLite range"))
                            .transpose()?,
                        input.snapshot_digest,
                        sqlite_integer(
                            input.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored = load_snapshot(connection, &input.snapshot_id)?;
                if stored != input {
                    return Err(conflict(
                        "programmatic_policy_snapshot_unavailable",
                        "the snapshot identity is already bound to different content",
                    ));
                }
            }
            Ok(input)
        })
    }

    fn load_programmatic_policy_snapshot(
        &self,
        snapshot_id: String,
    ) -> DtoResult<ProgrammaticPolicySnapshotRecordDto> {
        let connection = self.connection()?;
        load_snapshot(&connection, &snapshot_id)
    }
}

fn count_policies(
    connection: &sqlite::Connection,
    column: &'static str,
    value: &str,
) -> DtoResult<u64> {
    let count: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM programmatic_policies WHERE {column}=?1"),
            [value],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    u64::try_from(count).map_err(|_| codec_error("invalid policy count"))
}

impl ProgrammaticConfirmationRepositoryDto for SqliteStorageRepository {
    fn create_programmatic_policy_confirmation(
        &self,
        input: intention_storage::programmatic_policy_repo::ProgrammaticPolicyConfirmationRecordDto,
    ) -> DtoResult<
        intention_storage::programmatic_policy_repo::ProgrammaticPolicyConfirmationRecordDto,
    > {
        input.validate()?;
        write(self, |connection| {
            let existing = load_confirmation_by_call(connection, &input.tool_call_id)?;
            if let Some(stored) = existing {
                if stored == input {
                    return Ok(stored);
                }
                return Err(conflict(
                    "programmatic_policy_confirmation_required",
                    "the bound tool call already holds a different confirmation",
                ));
            }
            let awaiting: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM programmatic_policy_confirmations
                     WHERE root_run_id=?1 AND state='awaiting'",
                    [&input.root_run_id],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let awaiting =
                usize::try_from(awaiting).map_err(|_| codec_error("invalid confirmation count"))?;
            policy_domain::validate_awaiting_confirmations_per_root_tree(awaiting + 1)?;
            connection
                .execute(
                    "INSERT INTO programmatic_policy_confirmations(
                        confirmation_id, root_session_id, root_run_id, tool_call_id, tool_id,
                        descriptor_revision, mcp_method_reference, typed_input_digest,
                        policy_snapshot_digest, state, created_at_ms, decided_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    sqlite::params![
                        input.confirmation_id,
                        input.root_session_id,
                        input.root_run_id,
                        input.tool_call_id,
                        input.tool_id,
                        input.descriptor_revision,
                        input.mcp_method_reference,
                        input.typed_input_digest,
                        input.policy_snapshot_digest,
                        input.state.name(),
                        sqlite_integer(
                            input.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                        input
                            .decided_at_ms
                            .map(|value| sqlite_integer(
                                value,
                                "timestamp is outside the SQLite range"
                            ))
                            .transpose()?,
                    ],
                )
                .map_err(storage_error)?;
            Ok(input)
        })
    }

    fn load_programmatic_policy_confirmation(
        &self,
        tool_call_id: String,
    ) -> DtoResult<
        Option<
            intention_storage::programmatic_policy_repo::ProgrammaticPolicyConfirmationRecordDto,
        >,
    > {
        let connection = self.connection()?;
        load_confirmation_by_call(&connection, &tool_call_id)
    }

    fn decide_programmatic_policy_confirmation(
        &self,
        input: DecideProgrammaticPolicyConfirmationInputDto,
    ) -> DtoResult<
        intention_storage::programmatic_policy_repo::ProgrammaticPolicyConfirmationRecordDto,
    > {
        let confirmation_id = input.confirmation_id;
        let tool_call_id = input.tool_call_id;
        let typed_input_digest = input.typed_input_digest;
        let state = input.state;
        let decided_at_ms = input.decided_at_ms;
        if state == ProgrammaticConfirmationStateDto::Awaiting {
            return Err(ErrorDto::validation(
                "programmatic_policy_confirmation_required",
                "a decision resolves an awaiting confirmation",
            ));
        }
        write(self, |connection| {
            let confirmation = load_confirmation(connection, &confirmation_id)?;
            if confirmation.state != ProgrammaticConfirmationStateDto::Awaiting {
                return Err(conflict(
                    "programmatic_policy_confirmation_expired",
                    "the confirmation is no longer awaiting a decision",
                ));
            }
            if confirmation.tool_call_id != tool_call_id
                || confirmation.typed_input_digest != typed_input_digest
            {
                return Err(conflict(
                    "programmatic_policy_confirmation_required",
                    "the decision does not match the exact confirmation binding",
                ));
            }
            connection
                .execute(
                    "UPDATE programmatic_policy_confirmations SET state=?2, decided_at_ms=?3
                     WHERE confirmation_id=?1",
                    sqlite::params![
                        confirmation_id,
                        state.name(),
                        sqlite_integer(decided_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_confirmation(connection, &confirmation_id)
        })
    }

    fn create_programmatic_authorization_corridor(
        &self,
        input: ProgrammaticAuthorizationCorridorRecordDto,
    ) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto> {
        input.validate()?;
        write(self, |connection| {
            let existing: Option<String> = connection
                .query_row(
                    "SELECT corridor_digest FROM programmatic_authorization_corridors WHERE corridor_digest=?1",
                    [&input.corridor_digest],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            if let Some(corridor_digest) = existing {
                let stored = load_corridor(connection, &corridor_digest)?;
                if stored != input {
                    return Err(conflict(
                        "programmatic_policy_corridor_unavailable",
                        "the corridor identity is already bound to different content",
                    ));
                }
                return Ok(stored);
            }
            let unfinished: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM programmatic_authorization_corridors
                     WHERE root_run_id=?1 AND state='active'",
                    [&input.root_run_id],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let unfinished =
                usize::try_from(unfinished).map_err(|_| codec_error("invalid corridor count"))?;
            policy_domain::validate_unfinished_corridors_per_root_tree(unfinished + 1)?;
            connection
                .execute(
                    "INSERT INTO programmatic_authorization_corridors(
                        corridor_digest, root_session_id, root_run_id, root_origin_kind,
                        policy_snapshot_reference, required_effect_selectors,
                        exact_tool_or_method_selectors, descriptor_input_constraint_selections,
                        maximum_action_count, maximum_concurrent_actions, confirmation_reference,
                        state, consumed_action_count, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    sqlite::params![
                        input.corridor_digest,
                        input.root_session_id,
                        input.root_run_id,
                        input.root_origin_kind.name(),
                        input.policy_snapshot_reference,
                        encode_items(&input.required_effect_selectors),
                        encode_items(&input.exact_tool_or_method_selectors),
                        encode_items(&input.descriptor_input_constraint_selections),
                        sqlite_integer(
                            input.maximum_action_count,
                            "limit is outside the SQLite range"
                        )?,
                        sqlite_integer(
                            input.maximum_concurrent_actions,
                            "limit is outside the SQLite range"
                        )?,
                        input.confirmation_reference,
                        input.state.name(),
                        sqlite_integer(
                            input.consumed_action_count,
                            "count is outside the SQLite range"
                        )?,
                        sqlite_integer(
                            input.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            Ok(input)
        })
    }

    fn load_active_programmatic_corridor(
        &self,
        root_run_id: String,
    ) -> DtoResult<Option<ProgrammaticAuthorizationCorridorRecordDto>> {
        let connection = self.connection()?;
        load_active_corridor(&connection, &root_run_id)
    }

    fn consume_programmatic_corridor_action(
        &self,
        input: ConsumeProgrammaticCorridorActionInputDto,
    ) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto> {
        let corridor_digest = input.corridor_digest;
        let root_run_id = input.root_run_id;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let corridor = load_corridor(connection, &corridor_digest)?;
            if corridor.state != ProgrammaticCorridorStateDto::Active
                || corridor.root_run_id != root_run_id
            {
                return Err(conflict(
                    "programmatic_policy_corridor_unavailable",
                    "no active corridor matches this root tree",
                ));
            }
            if corridor.consumed_action_count >= corridor.maximum_action_count {
                connection
                    .execute(
                        "UPDATE programmatic_authorization_corridors SET state='exhausted'
                         WHERE corridor_digest=?1",
                        [&corridor_digest],
                    )
                    .map_err(storage_error)?;
                return Err(conflict(
                    "programmatic_policy_corridor_exhausted",
                    "the bounded corridor has no remaining action",
                ));
            }
            let consumed = corridor.consumed_action_count.saturating_add(1);
            let state = if consumed >= corridor.maximum_action_count {
                ProgrammaticCorridorStateDto::Exhausted
            } else {
                ProgrammaticCorridorStateDto::Active
            };
            connection
                .execute(
                    "UPDATE programmatic_authorization_corridors
                     SET consumed_action_count=?2, state=?3 WHERE corridor_digest=?1",
                    sqlite::params![
                        corridor_digest,
                        sqlite_integer(consumed, "count is outside the SQLite range")?,
                        state.name(),
                    ],
                )
                .map_err(storage_error)?;
            let _ = occurred_at_ms;
            load_corridor(connection, &corridor_digest)
        })
    }

    fn close_programmatic_corridors_for_root(
        &self,
        root_run_id: String,
        state: ProgrammaticCorridorStateDto,
        closed_at_ms: u64,
    ) -> DtoResult<u64> {
        if state == ProgrammaticCorridorStateDto::Active
            || state == ProgrammaticCorridorStateDto::Exhausted
        {
            return Err(ErrorDto::validation(
                "programmatic_policy_corridor_unavailable",
                "a closure names a terminal expired or revoked state",
            ));
        }
        write(self, |connection| {
            let updated = connection
                .execute(
                    "UPDATE programmatic_authorization_corridors SET state=?2
                     WHERE root_run_id=?1",
                    sqlite::params![root_run_id, state.name()],
                )
                .map_err(storage_error)?;
            let _ = closed_at_ms;
            u64::try_from(updated).map_err(|_| codec_error("invalid corridor count"))
        })
    }

    fn propose_programmatic_policy_draft(
        &self,
        input: ProgrammaticPolicyDraftRecordDto,
    ) -> DtoResult<ProgrammaticPolicyDraftRecordDto> {
        input.validate()?;
        write(self, |connection| {
            let (scope_kind, project_id, owner_id) = draft_scope_columns(&input.scope);
            let pending = load_pending_draft(
                connection,
                &scope_kind,
                &project_id,
                owner_id.as_deref(),
                &input.record_kind,
            )?;
            if let Some(pending) = pending {
                if pending.canonical_draft_digest != input.canonical_draft_digest {
                    return Err(conflict(
                        "programmatic_policy_draft_conflict",
                        "at most one pending draft exists per owner scope",
                    ));
                }
                let mut evidence = pending.evidence_references.clone();
                for reference in &input.evidence_references {
                    if !evidence.contains(reference) {
                        evidence.push(reference.clone());
                    }
                }
                let coalesced = pending
                    .coalesced_evidence_count
                    .saturating_add(input.coalesced_evidence_count.max(1));
                connection
                    .execute(
                        "UPDATE programmatic_policy_drafts
                         SET evidence_references=?2, coalesced_evidence_count=?3
                         WHERE draft_id=?1",
                        sqlite::params![
                            pending.draft_id,
                            encode_items(&evidence),
                            sqlite_integer(coalesced, "count is outside the SQLite range")?,
                        ],
                    )
                    .map_err(storage_error)?;
                return load_draft(connection, &pending.draft_id);
            }
            let draft_id = input.draft_id.clone();
            connection
                .execute(
                    "INSERT INTO programmatic_policy_drafts(
                        draft_id, scope_kind, project_id, owner_id, record_kind, base_revision,
                        evidence_references, safe_rationale, canonical_draft_digest, state,
                        coalesced_evidence_count, created_at_ms, decided_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    sqlite::params![
                        input.draft_id,
                        scope_kind,
                        project_id,
                        owner_id,
                        input.record_kind,
                        sqlite_integer(
                            input.base_revision,
                            "revision is outside the SQLite range"
                        )?,
                        encode_items(&input.evidence_references),
                        input.safe_rationale,
                        input.canonical_draft_digest,
                        input.state.name(),
                        sqlite_integer(
                            input.coalesced_evidence_count.max(1),
                            "count is outside the SQLite range"
                        )?,
                        sqlite_integer(
                            input.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                        input
                            .decided_at_ms
                            .map(|value| sqlite_integer(
                                value,
                                "timestamp is outside the SQLite range"
                            ))
                            .transpose()?,
                    ],
                )
                .map_err(storage_error)?;
            load_draft(connection, &draft_id)
        })
    }

    fn load_pending_programmatic_policy_draft(
        &self,
        scope: ProgrammaticPolicyScopeDto,
        record_kind: String,
    ) -> DtoResult<Option<ProgrammaticPolicyDraftRecordDto>> {
        let connection = self.connection()?;
        let (scope_kind, project_id, owner_id) = draft_scope_columns(&scope);
        load_pending_draft(
            &connection,
            &scope_kind,
            &project_id,
            owner_id.as_deref(),
            &record_kind,
        )
    }

    fn decide_programmatic_policy_draft(
        &self,
        draft_id: String,
        state: ProgrammaticPolicyDraftStateDto,
        decided_at_ms: u64,
    ) -> DtoResult<ProgrammaticPolicyDraftRecordDto> {
        if state == ProgrammaticPolicyDraftStateDto::Pending {
            return Err(ErrorDto::validation(
                "programmatic_policy_draft_conflict",
                "a decision resolves a pending draft",
            ));
        }
        write(self, |connection| {
            let draft = load_draft(connection, &draft_id)?;
            if draft.state != ProgrammaticPolicyDraftStateDto::Pending {
                return Err(conflict(
                    "programmatic_policy_draft_conflict",
                    "the draft is no longer pending",
                ));
            }
            connection
                .execute(
                    "UPDATE programmatic_policy_drafts SET state=?2, decided_at_ms=?3
                     WHERE draft_id=?1",
                    sqlite::params![
                        draft_id,
                        state.name(),
                        sqlite_integer(decided_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_draft(connection, &draft_id)
        })
    }
}

impl ProgrammaticAdmissionRepositoryDto for SqliteStorageRepository {
    fn load_programmatic_policy_counters(
        &self,
        policy_id: String,
    ) -> DtoResult<Option<ProgrammaticPolicyCounterRecordDto>> {
        let connection = self.connection()?;
        load_counters(&connection, &policy_id)
    }

    fn reserve_programmatic_policy_action(
        &self,
        input: ReserveProgrammaticPolicyActionInputDto,
    ) -> DtoResult<ReserveProgrammaticPolicyActionOutcomeDto> {
        input.reservation.validate()?;
        if input.max_actions_per_run == 0
            || input.max_concurrent_actions_per_run == 0
            || input.calendar_max_actions == 0
            || input.calendar_window_end_ms <= input.calendar_window_start_ms
        {
            return Err(ErrorDto::validation(
                "programmatic_policy_limit_exceeded",
                "the selected effective reservation bounds are unusable",
            ));
        }
        let reservation = input.reservation;
        let max_actions_per_run = input.max_actions_per_run;
        let max_concurrent_actions_per_run = input.max_concurrent_actions_per_run;
        let calendar_max_actions = input.calendar_max_actions;
        let window_start = input.calendar_window_start_ms;
        let window_end = input.calendar_window_end_ms;
        let window_time_zone = input.calendar_window_time_zone;
        write(self, |connection| {
            let Some(mut counters) = load_counters(connection, &reservation.policy_id)? else {
                return Err(not_found(
                    "programmatic_policy_counter_unavailable",
                    "no compatible durable counter exists for this policy",
                ));
            };
            if let Some(existing) = connection
                .query_row(
                    "SELECT reservation_reference FROM programmatic_policy_reservations
                     WHERE tool_call_id=?1",
                    [&reservation.tool_call_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage_error)?
            {
                let stored = load_reservation(connection, &existing)?;
                if stored.policy_id == reservation.policy_id
                    && stored.policy_revision == reservation.policy_revision
                    && stored.root_run_id == reservation.root_run_id
                    && stored.typed_input_digest == reservation.typed_input_digest
                    && stored.calendar_counter_reference == reservation.calendar_counter_reference
                {
                    return Ok(ReserveProgrammaticPolicyActionOutcomeDto {
                        reservation: stored,
                        counters,
                        replayed: true,
                    });
                }
                return Err(reservation_conflict());
            }
            if (
                counters.calendar_window_start_ms,
                counters.calendar_window_end_ms,
            ) != (window_start, window_end)
            {
                if window_start >= counters.calendar_window_end_ms
                    || counters.calendar_window_end_ms == 0
                {
                    counters.calendar_started_actions = 0;
                    counters.calendar_reserved_actions = 0;
                    counters.calendar_window_start_ms = window_start;
                    counters.calendar_window_end_ms = window_end;
                    counters
                        .calendar_window_time_zone
                        .clone_from(&window_time_zone);
                } else {
                    return Err(not_found(
                        "programmatic_policy_counter_unavailable",
                        "the supplied calendar window does not match the durable counter window",
                    ));
                }
            }
            let run_outstanding = counters
                .run_started_actions
                .saturating_add(counters.run_reserved_actions);
            if run_outstanding.saturating_add(1) > max_actions_per_run {
                return Err(ErrorDto::validation(
                    "programmatic_policy_run_limit_exceeded",
                    "the per-run action limit is exhausted",
                ));
            }
            let concurrent = counters
                .run_in_flight_actions
                .saturating_add(counters.run_reserved_actions);
            if concurrent.saturating_add(1) > max_concurrent_actions_per_run {
                return Err(ErrorDto::validation(
                    "programmatic_policy_run_limit_exceeded",
                    "the per-run concurrency limit is exhausted",
                ));
            }
            let calendar_outstanding = counters
                .calendar_started_actions
                .saturating_add(counters.calendar_reserved_actions);
            if calendar_outstanding.saturating_add(1) > calendar_max_actions {
                return Err(ErrorDto::validation(
                    "programmatic_policy_calendar_limit_exceeded",
                    "the calendar action limit is exhausted",
                ));
            }
            connection
                .execute(
                    "INSERT INTO programmatic_policy_reservations(
                        reservation_reference, policy_id, policy_revision, root_run_id,
                        tool_call_id, typed_input_digest, calendar_counter_reference,
                        reserved_at_ms, state, finished_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'reserved', NULL)",
                    sqlite::params![
                        reservation.reservation_reference,
                        reservation.policy_id,
                        sqlite_integer(
                            reservation.policy_revision,
                            "revision is outside the SQLite range"
                        )?,
                        reservation.root_run_id,
                        reservation.tool_call_id,
                        reservation.typed_input_digest,
                        reservation.calendar_counter_reference,
                        sqlite_integer(
                            reservation.reserved_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            counters.run_reserved_actions = counters.run_reserved_actions.saturating_add(1);
            counters.calendar_reserved_actions =
                counters.calendar_reserved_actions.saturating_add(1);
            counters.updated_at_ms = reservation.reserved_at_ms;
            store_counters(connection, &reservation.policy_id, &counters)?;
            Ok(ReserveProgrammaticPolicyActionOutcomeDto {
                reservation: load_reservation(connection, &reservation.reservation_reference)?,
                counters,
                replayed: false,
            })
        })
    }

    fn load_programmatic_policy_reservation(
        &self,
        reservation_reference: String,
    ) -> DtoResult<Option<ProgrammaticPolicyReservationRecordDto>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT reservation_reference FROM programmatic_policy_reservations
                 WHERE reservation_reference=?1",
                [&reservation_reference],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?
            .map(|_| load_reservation(&connection, &reservation_reference))
            .transpose()
    }

    fn load_programmatic_policy_reservations_for_run(
        &self,
        root_run_id: String,
    ) -> DtoResult<Vec<ProgrammaticPolicyReservationRecordDto>> {
        let connection = self.connection()?;
        load_reservations_for_run(&connection, &root_run_id)
    }

    fn release_programmatic_policy_reservation(
        &self,
        input: ReleaseProgrammaticPolicyReservationInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto> {
        let reservation_reference = input.reservation_reference;
        let tool_call_id = input.tool_call_id;
        let released_at_ms = input.released_at_ms;
        write(self, |connection| {
            let reservation = load_reservation(connection, &reservation_reference)?;
            if reservation.state != ProgrammaticReservationStateDto::Reserved
                || reservation.tool_call_id != tool_call_id
            {
                return Err(reservation_conflict());
            }
            connection
                .execute(
                    "UPDATE programmatic_policy_reservations
                     SET state='released_on_known_pre_effect', finished_at_ms=?2
                     WHERE reservation_reference=?1",
                    sqlite::params![
                        reservation_reference,
                        sqlite_integer(released_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            update_counters_for_reservation(
                connection,
                &reservation,
                ReleaseKind::Released { released_at_ms },
            )?;
            load_reservation(connection, &reservation_reference)
        })
    }

    fn commit_programmatic_reservation_started(
        &self,
        input: CommitProgrammaticReservationStartedInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto> {
        let reservation_reference = input.reservation_reference;
        let tool_call_id = input.tool_call_id;
        let started_at_ms = input.started_at_ms;
        write(self, |connection| {
            let reservation = load_reservation(connection, &reservation_reference)?;
            if reservation.state != ProgrammaticReservationStateDto::Reserved
                || reservation.tool_call_id != tool_call_id
            {
                return Err(reservation_conflict());
            }
            connection
                .execute(
                    "UPDATE programmatic_policy_reservations
                     SET state='permanent_on_start', finished_at_ms=NULL
                     WHERE reservation_reference=?1",
                    [&reservation_reference],
                )
                .map_err(storage_error)?;
            update_counters_for_reservation(
                connection,
                &reservation,
                ReleaseKind::Started { started_at_ms },
            )?;
            load_reservation(connection, &reservation_reference)
        })
    }

    fn recover_programmatic_policy_reservation(
        &self,
        input: RecoverProgrammaticPolicyReservationInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto> {
        let reservation_reference = input.reservation_reference;
        let tool_call_started = input.tool_call_started;
        let recovered_at_ms = input.recovered_at_ms;
        write(self, |connection| {
            let reservation = load_reservation(connection, &reservation_reference)?;
            let expected_state = if tool_call_started {
                ProgrammaticReservationStateDto::PermanentOnStart
            } else {
                ProgrammaticReservationStateDto::Reserved
            };
            if reservation.state != expected_state {
                return Err(reservation_conflict());
            }
            let (state, kind) = if tool_call_started {
                (
                    ProgrammaticReservationStateDto::ExternalEffectUnknown,
                    ReleaseKind::None,
                )
            } else {
                (
                    ProgrammaticReservationStateDto::InterruptedBeforeStart,
                    ReleaseKind::Released {
                        released_at_ms: recovered_at_ms,
                    },
                )
            };
            connection
                .execute(
                    "UPDATE programmatic_policy_reservations SET state=?2, finished_at_ms=?3
                     WHERE reservation_reference=?1",
                    sqlite::params![
                        reservation_reference,
                        state.name(),
                        sqlite_integer(recovered_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            if kind != ReleaseKind::None {
                update_counters_for_reservation(connection, &reservation, kind)?;
            } else {
                update_counters_for_reservation(
                    connection,
                    &reservation,
                    ReleaseKind::UnknownEffect { recovered_at_ms },
                )?;
            }
            load_reservation(connection, &reservation_reference)
        })
    }
}

/// One counter adjustment performed with a reservation state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReleaseKind {
    /// A pre-start reservation is released without consuming a unit.
    Released { released_at_ms: u64 },
    /// A started reservation permanently consumes its run and calendar units.
    Started { started_at_ms: u64 },
    /// A started ambiguous reservation keeps its permanent consumption.
    UnknownEffect { recovered_at_ms: u64 },
    /// No counter adjustment is performed.
    None,
}

fn update_counters_for_reservation(
    connection: &sqlite::Connection,
    reservation: &ProgrammaticPolicyReservationRecordDto,
    kind: ReleaseKind,
) -> DtoResult<()> {
    let Some(mut counters) = load_counters(connection, &reservation.policy_id)? else {
        return Err(not_found(
            "programmatic_policy_counter_unavailable",
            "no compatible durable counter exists for this policy",
        ));
    };
    match kind {
        ReleaseKind::Released { released_at_ms } => {
            counters.run_reserved_actions = counters.run_reserved_actions.saturating_sub(1);
            counters.calendar_reserved_actions =
                counters.calendar_reserved_actions.saturating_sub(1);
            counters.updated_at_ms = released_at_ms;
        }
        ReleaseKind::Started { started_at_ms } => {
            counters.run_reserved_actions = counters.run_reserved_actions.saturating_sub(1);
            counters.calendar_reserved_actions =
                counters.calendar_reserved_actions.saturating_sub(1);
            counters.run_started_actions = counters.run_started_actions.saturating_add(1);
            counters.calendar_started_actions = counters.calendar_started_actions.saturating_add(1);
            counters.run_in_flight_actions = counters.run_in_flight_actions.saturating_add(1);
            counters.updated_at_ms = started_at_ms;
        }
        ReleaseKind::UnknownEffect { recovered_at_ms } => {
            counters.updated_at_ms = recovered_at_ms;
        }
        ReleaseKind::None => {}
    }
    store_counters(connection, &reservation.policy_id, &counters)
}

fn load_reservations_for_run(
    connection: &sqlite::Connection,
    root_run_id: &str,
) -> DtoResult<Vec<ProgrammaticPolicyReservationRecordDto>> {
    let mut statement = connection
        .prepare(
            "SELECT reservation_reference, policy_id, policy_revision, root_run_id, tool_call_id,
                    typed_input_digest, calendar_counter_reference, reserved_at_ms, state,
                    finished_at_ms
             FROM programmatic_policy_reservations WHERE root_run_id=?1
             ORDER BY reserved_at_ms, reservation_reference",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([root_run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                u64_column(row, 2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                u64_column(row, 7)?,
                row.get::<_, String>(8)?,
                row.get::<_, Option<i64>>(9)?
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|_| sqlite::Error::IntegralValueOutOfRange(9, i64::MIN))?,
            ))
        })
        .map_err(storage_error)?;
    rows.map(|row| {
        let row = row.map_err(storage_error)?;
        let reservation_reference = row.0;
        reservation_from_columns(
            &reservation_reference,
            &(
                row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9,
            ),
        )
    })
    .collect()
}
