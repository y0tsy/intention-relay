//! Durable continual-harness repository: rules, revisions, triggers, journal.
//!
//! Owner: architecture 26 with ADR 0044. This module owns the current
//! single-version schema records for harness rules, immutable rule revisions,
//! durable trigger reasons, launch counters, dossiers, checkpoints, and the
//! durable journal, plus the store-level transitions for the Slice 3 harness
//! surface.
//!
//! Every method validates its DTO before any row is written, writes through
//! parameterized SQL only, and commits multi-row state in one immediate
//! transaction. Durable list fields are encoded as private delimiter-separated
//! canonical text: every element is validated control-free before encoding, so
//! the unit and record separators are unambiguous and the codec round-trips.

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
    RecordHarnessLaunchInputDto, RecordHarnessLaunchOutcomeDto, ReviseHarnessRuleInputDto,
    TransitionHarnessRuleLifecycleInputDto,
};
use intention_types::{DtoResult, ErrorDto};
use sqlite::OptionalExtension;

use super::{
    SqliteStorageRepository, codec_error, conflict, not_found, sqlite_integer, storage_error,
};

/// The current single-version DDL of the Slice 3 continual-harness family.
pub const SCHEMA_HARNESS_SQL: &str = "
-- Slice 3 harness family (owner: src/harness_repo.rs): user-managed rules,
-- immutable rule revisions, durable trigger reasons, launch counters,
-- dossiers, verified checkpoints, and the durable harness journal.
CREATE TABLE IF NOT EXISTS harness_rules (
  harness_id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project','user_session')),
  project_id TEXT NOT NULL,
  session_id TEXT,
  lifecycle_state TEXT NOT NULL CHECK(lifecycle_state IN ('active','paused','archived')),
  active_revision INTEGER NOT NULL CHECK(active_revision > 0),
  service_session_id TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
  CHECK((scope_kind = 'project' AND session_id IS NULL) OR (scope_kind = 'user_session' AND session_id IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS harness_rules_by_project ON harness_rules(project_id);
CREATE TABLE IF NOT EXISTS harness_rule_revisions (
  harness_id TEXT NOT NULL REFERENCES harness_rules(harness_id),
  revision INTEGER NOT NULL CHECK(revision > 0),
  task_digest TEXT NOT NULL,
  class TEXT NOT NULL CHECK(class IN ('light','medium','heavy')),
  task_mode TEXT NOT NULL CHECK(task_mode IN ('repeated_task','goal_directed')),
  presentation_mode TEXT NOT NULL CHECK(presentation_mode IN ('journal_only','journal_and_activity_entry')),
  applied_time_zone TEXT NOT NULL,
  source_kinds TEXT NOT NULL,
  source_references TEXT NOT NULL,
  interval_anchor_ms INTEGER CHECK(interval_anchor_ms IS NULL OR interval_anchor_ms >= 0),
  interval_ms INTEGER CHECK(interval_ms IS NULL OR interval_ms >= 0),
  calendar_expression TEXT,
  completion_link_reference TEXT,
  completion_outcomes TEXT NOT NULL,
  canonical_revision_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  PRIMARY KEY(harness_id, revision)
);
CREATE TABLE IF NOT EXISTS harness_trigger_reasons (
  reason_id TEXT PRIMARY KEY,
  harness_id TEXT NOT NULL REFERENCES harness_rules(harness_id),
  source_kind TEXT NOT NULL CHECK(source_kind IN ('explicit_user_launch','calendar_time','fixed_interval','terminal_outcome_link')),
  rule_revision INTEGER NOT NULL CHECK(rule_revision > 0),
  first_observed_at_ms INTEGER NOT NULL CHECK(first_observed_at_ms >= 0),
  last_observed_at_ms INTEGER NOT NULL CHECK(last_observed_at_ms >= first_observed_at_ms),
  coalesced_count INTEGER NOT NULL CHECK(coalesced_count > 0),
  applied_time_zone TEXT NOT NULL,
  cause_chain_reference TEXT,
  bounded_references TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending','admitted'))
);
CREATE UNIQUE INDEX IF NOT EXISTS one_pending_harness_trigger ON harness_trigger_reasons(harness_id) WHERE state = 'pending';
CREATE INDEX IF NOT EXISTS harness_trigger_reasons_by_harness ON harness_trigger_reasons(harness_id, first_observed_at_ms);
CREATE TABLE IF NOT EXISTS harness_dossiers (
  dossier_id TEXT PRIMARY KEY,
  harness_id TEXT NOT NULL REFERENCES harness_rules(harness_id),
  rule_revision INTEGER NOT NULL CHECK(rule_revision > 0),
  reason_id TEXT NOT NULL,
  task_digest TEXT NOT NULL,
  source_references TEXT NOT NULL,
  typed_references TEXT NOT NULL,
  checkpoint_reference TEXT,
  dossier_bytes INTEGER NOT NULL CHECK(dossier_bytes >= 0),
  canonical_dossier_digest TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
);
CREATE INDEX IF NOT EXISTS harness_dossiers_by_harness ON harness_dossiers(harness_id, created_at_ms);
CREATE TABLE IF NOT EXISTS harness_checkpoints (
  checkpoint_id TEXT PRIMARY KEY,
  harness_id TEXT NOT NULL REFERENCES harness_rules(harness_id),
  rule_revision INTEGER NOT NULL CHECK(rule_revision > 0),
  producing_run_id TEXT NOT NULL,
  checkpoint_revision INTEGER NOT NULL CHECK(checkpoint_revision > 0),
  content_digest TEXT NOT NULL,
  checkpoint_bytes INTEGER NOT NULL CHECK(checkpoint_bytes >= 0),
  is_current INTEGER NOT NULL CHECK(is_current IN (0,1)),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  UNIQUE(harness_id, checkpoint_revision)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_current_harness_checkpoint ON harness_checkpoints(harness_id) WHERE is_current = 1;
CREATE TABLE IF NOT EXISTS harness_journal (
  harness_id TEXT NOT NULL REFERENCES harness_rules(harness_id),
  sequence INTEGER NOT NULL CHECK(sequence > 0),
  record_kind TEXT NOT NULL CHECK(record_kind IN ('trigger_captured','launch_admitted','launch_retained','run_terminal','checkpoint_accepted','checkpoint_retained','conclusion_published')),
  safe_summary TEXT NOT NULL,
  canonical_record_digest TEXT NOT NULL,
  occurred_at_ms INTEGER NOT NULL CHECK(occurred_at_ms >= 0),
  PRIMARY KEY(harness_id, sequence)
);
CREATE TABLE IF NOT EXISTS harness_counters (
  harness_id TEXT PRIMARY KEY REFERENCES harness_rules(harness_id),
  cause_chain_depth INTEGER NOT NULL CHECK(cause_chain_depth >= 0),
  concurrent_non_terminal INTEGER NOT NULL CHECK(concurrent_non_terminal >= 0),
  total_launches INTEGER NOT NULL CHECK(total_launches >= 0),
  direct_successors INTEGER NOT NULL CHECK(direct_successors >= 0),
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0)
);
";

/// The unit separator joining encoded list items.
const UNIT_SEPARATOR: char = '\u{1f}';

/// Encodes one flat list of safe values as delimiter-separated text.
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

/// Reads one non-negative integer column as `u64`.
fn u64_column(row: &sqlite::Row<'_>, index: usize) -> Result<u64, sqlite::Error> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| sqlite::Error::IntegralValueOutOfRange(index, value))
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

fn harness_not_active() -> ErrorDto {
    not_found(
        "harness_not_active",
        "the requested durable harness rule does not exist or is not active",
    )
}

fn harness_revision_conflict() -> ErrorDto {
    conflict(
        "harness_revision_conflict",
        "the durable harness revision is stale or already bound to different content",
    )
}

fn harness_source_unavailable() -> ErrorDto {
    not_found(
        "harness_source_unavailable",
        "the requested durable harness record does not exist",
    )
}

fn harness_not_active_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        harness_not_active()
    } else {
        storage_error(error)
    }
}

fn harness_source_unavailable_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        harness_source_unavailable()
    } else {
        storage_error(error)
    }
}

fn harness_revision_conflict_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        harness_revision_conflict()
    } else {
        storage_error(error)
    }
}

const RULE_COLUMNS: &str = "scope_kind, project_id, session_id, lifecycle_state, active_revision, service_session_id, updated_at_ms";

fn load_rule(connection: &sqlite::Connection, harness_id: &str) -> DtoResult<HarnessRuleRecordDto> {
    connection
        .query_row(
            &format!("SELECT {RULE_COLUMNS} FROM harness_rules WHERE harness_id=?1"),
            [harness_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    u64_column(row, 4)?,
                    row.get::<_, String>(5)?,
                    u64_column(row, 6)?,
                ))
            },
        )
        .map_err(harness_not_active_or_storage)
        .and_then(|row| rule_from_columns(harness_id, &row))
}

type RuleColumns = (String, String, Option<String>, String, u64, String, u64);

fn rule_from_columns(harness_id: &str, row: &RuleColumns) -> DtoResult<HarnessRuleRecordDto> {
    let scope = match (&row.2, row.0.as_str()) {
        (Some(session_id), "user_session") => HarnessRuleScopeDto::UserSession {
            project_id: row.1.clone(),
            session_id: session_id.clone(),
        },
        (None, "project") => HarnessRuleScopeDto::Project {
            project_id: row.1.clone(),
        },
        _ => return Err(codec_error("persisted harness scope is malformed")),
    };
    Ok(HarnessRuleRecordDto {
        harness_id: harness_id.to_owned(),
        scope,
        lifecycle_state: HarnessRuleLifecycleStateDto::parse(&row.3)?,
        active_revision: row.4,
        service_session_id: row.5.clone(),
        updated_at_ms: row.6,
    })
}

type RevisionColumns = (
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
    String,
    i64,
);

fn revision_from_columns(
    harness_id: &str,
    row: &RevisionColumns,
) -> DtoResult<HarnessRuleRevisionRecordDto> {
    let revision = u64::try_from(row.0).map_err(|_| codec_error("invalid harness revision"))?;
    let source_kinds = decode_items(&row.6)?
        .iter()
        .map(|name| HarnessSourceKindDto::parse(name))
        .collect::<DtoResult<Vec<_>>>()?;
    let interval_anchor_ms = row
        .8
        .map(|value| u64::try_from(value).map_err(|_| codec_error("invalid interval anchor")))
        .transpose()?;
    let interval_ms = row
        .9
        .map(|value| u64::try_from(value).map_err(|_| codec_error("invalid interval")))
        .transpose()?;
    let completion_link_reference = row.11.clone();
    let completion_outcomes = decode_items(&row.12)?;
    let created_at_ms =
        u64::try_from(row.14).map_err(|_| codec_error("invalid harness timestamp"))?;
    Ok(HarnessRuleRevisionRecordDto {
        harness_id: harness_id.to_owned(),
        revision,
        task_digest: row.1.clone(),
        class: HarnessExecutionClassDto::parse(&row.2)?,
        task_mode: HarnessTaskModeDto::parse(&row.3)?,
        presentation_mode: HarnessPresentationModeDto::parse(&row.4)?,
        applied_time_zone: row.5.clone(),
        source_kinds,
        source_references: decode_items(&row.7)?,
        interval_anchor_ms,
        interval_ms,
        calendar_expression: row.10.clone(),
        completion_link_reference,
        completion_outcomes,
        canonical_revision_digest: row.13.clone(),
        created_at_ms,
    })
}

fn load_revision(
    connection: &sqlite::Connection,
    harness_id: &str,
    revision: u64,
) -> DtoResult<HarnessRuleRevisionRecordDto> {
    connection
        .query_row(
            "SELECT revision, task_digest, class, task_mode, presentation_mode, applied_time_zone,
                    source_kinds, source_references, interval_anchor_ms, interval_ms,
                    calendar_expression, completion_link_reference, completion_outcomes,
                    canonical_revision_digest, created_at_ms
             FROM harness_rule_revisions WHERE harness_id=?1 AND revision=?2",
            sqlite::params![
                harness_id,
                sqlite_integer(revision, "revision is outside the SQLite range")?
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, i64>(14)?,
                ))
            },
        )
        .map_err(harness_revision_conflict_or_storage)
        .and_then(|row| revision_from_columns(harness_id, &row))
}

fn insert_revision(
    connection: &sqlite::Connection,
    revision: &HarnessRuleRevisionRecordDto,
) -> DtoResult<()> {
    let source_kinds = revision
        .source_kinds
        .iter()
        .map(|kind| kind.name().to_owned())
        .collect::<Vec<_>>();
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO harness_rule_revisions(
                harness_id, revision, task_digest, class, task_mode, presentation_mode,
                applied_time_zone, source_kinds, source_references, interval_anchor_ms,
                interval_ms, calendar_expression, completion_link_reference,
                completion_outcomes, canonical_revision_digest, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            sqlite::params![
                revision.harness_id,
                sqlite_integer(revision.revision, "revision is outside the SQLite range")?,
                revision.task_digest,
                revision.class.name(),
                revision.task_mode.name(),
                revision.presentation_mode.name(),
                revision.applied_time_zone,
                encode_items(&source_kinds),
                encode_items(&revision.source_references),
                revision
                    .interval_anchor_ms
                    .map(|value| sqlite_integer(value, "anchor is outside the SQLite range"))
                    .transpose()?,
                revision
                    .interval_ms
                    .map(|value| sqlite_integer(value, "interval is outside the SQLite range"))
                    .transpose()?,
                revision.calendar_expression,
                revision.completion_link_reference,
                encode_items(&revision.completion_outcomes),
                revision.canonical_revision_digest,
                sqlite_integer(
                    revision.created_at_ms,
                    "timestamp is outside the SQLite range"
                )?,
            ],
        )
        .map_err(storage_error)?;
    if inserted == 0 {
        let existing = load_revision(connection, &revision.harness_id, revision.revision)?;
        if existing != *revision {
            return Err(harness_revision_conflict());
        }
    }
    Ok(())
}

fn trigger_from_columns(
    reason_id: &str,
    row: &(
        String,
        String,
        i64,
        i64,
        i64,
        i64,
        String,
        Option<String>,
        String,
        String,
    ),
    harness_id: String,
) -> DtoResult<HarnessTriggerReasonRecordDto> {
    let rule_revision =
        u64::try_from(row.2).map_err(|_| codec_error("invalid harness revision"))?;
    let first_observed_at_ms =
        u64::try_from(row.3).map_err(|_| codec_error("invalid observation time"))?;
    let last_observed_at_ms =
        u64::try_from(row.4).map_err(|_| codec_error("invalid observation time"))?;
    let coalesced_count =
        u64::try_from(row.5).map_err(|_| codec_error("invalid coalesced count"))?;
    Ok(HarnessTriggerReasonRecordDto {
        reason_id: reason_id.to_owned(),
        harness_id,
        source_kind: HarnessSourceKindDto::parse(&row.0)?,
        rule_revision,
        first_observed_at_ms,
        last_observed_at_ms,
        coalesced_count,
        applied_time_zone: row.6.clone(),
        cause_chain_reference: row.7.clone(),
        bounded_references: decode_items(&row.8)?,
        state: HarnessTriggerReasonStateDto::parse(&row.9)?,
    })
}

type TriggerRow = (
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    Option<String>,
    String,
    String,
);

fn load_trigger(
    connection: &sqlite::Connection,
    reason_id: &str,
) -> DtoResult<HarnessTriggerReasonRecordDto> {
    let row: TriggerRow = connection
        .query_row(
            "SELECT source_kind, harness_id, rule_revision, first_observed_at_ms,
                    last_observed_at_ms, coalesced_count, applied_time_zone,
                    cause_chain_reference, bounded_references, state
             FROM harness_trigger_reasons WHERE reason_id=?1",
            [reason_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )
        .map_err(harness_source_unavailable_or_storage)?;
    let harness_id = row.1.clone();
    trigger_from_columns(reason_id, &row, harness_id)
}

fn load_pending_trigger(
    connection: &sqlite::Connection,
    harness_id: &str,
) -> DtoResult<Option<HarnessTriggerReasonRecordDto>> {
    connection
        .query_row(
            "SELECT reason_id, source_kind, harness_id, rule_revision, first_observed_at_ms,
                    last_observed_at_ms, coalesced_count, applied_time_zone,
                    cause_chain_reference, bounded_references, state
             FROM harness_trigger_reasons WHERE harness_id=?1 AND state='pending' ORDER BY first_observed_at_ms LIMIT 1",
            [harness_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
        .and_then(|row| {
            row.map(|row| {
                let reason_id = row.0;
                let pending_harness_id = row.2.clone();
                trigger_from_columns(
                    &reason_id,
                    &(row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10),
                    pending_harness_id,
                )
            })
            .transpose()
        })
}

fn counters_from_columns(row: &(i64, i64, i64, i64, i64)) -> DtoResult<HarnessCounterRecordDto> {
    Ok(HarnessCounterRecordDto {
        cause_chain_depth: u64::try_from(row.0).map_err(|_| codec_error("invalid depth"))?,
        concurrent_non_terminal: u64::try_from(row.1)
            .map_err(|_| codec_error("invalid concurrency"))?,
        total_launches: u64::try_from(row.2).map_err(|_| codec_error("invalid total"))?,
        direct_successors: u64::try_from(row.3).map_err(|_| codec_error("invalid successors"))?,
        updated_at_ms: u64::try_from(row.4).map_err(|_| codec_error("invalid timestamp"))?,
    })
}

fn load_counters(
    connection: &sqlite::Connection,
    harness_id: &str,
) -> DtoResult<HarnessCounterRecordDto> {
    connection
        .query_row(
            "SELECT cause_chain_depth, concurrent_non_terminal, total_launches,
                    direct_successors, updated_at_ms
             FROM harness_counters WHERE harness_id=?1",
            [harness_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .map_err(harness_not_active_or_storage)
        .and_then(|row| counters_from_columns(&row))
}

fn count_rules(connection: &sqlite::Connection, project_id: &str) -> DtoResult<u64> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM harness_rules WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    u64::try_from(count).map_err(|_| codec_error("invalid rule count"))
}

fn load_journal_page(
    connection: &sqlite::Connection,
    harness_id: &str,
    after_sequence: u64,
    limit: u64,
) -> DtoResult<Vec<HarnessJournalRecordDto>> {
    let mut statement = connection
        .prepare(
            "SELECT sequence, record_kind, safe_summary, canonical_record_digest, occurred_at_ms
             FROM harness_journal WHERE harness_id=?1 AND sequence > ?2
             ORDER BY sequence LIMIT ?3",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map(
            sqlite::params![
                harness_id,
                sqlite_integer(after_sequence, "sequence is outside the SQLite range")?,
                sqlite_integer(limit, "limit is outside the SQLite range")?,
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .map_err(storage_error)?;
    rows.map(|row| {
        let (sequence, kind, summary, digest, occurred_at_ms) = row.map_err(storage_error)?;
        Ok(HarnessJournalRecordDto {
            harness_id: harness_id.to_owned(),
            sequence: u64::try_from(sequence)
                .map_err(|_| codec_error("invalid journal sequence"))?,
            record_kind: HarnessJournalRecordKindDto::parse(&kind)?,
            safe_summary: summary,
            canonical_record_digest: digest,
            occurred_at_ms: u64::try_from(occurred_at_ms)
                .map_err(|_| codec_error("invalid journal timestamp"))?,
        })
    })
    .collect()
}

impl HarnessRuleRepositoryDto for SqliteStorageRepository {
    fn create_harness_rule(
        &self,
        input: CreateHarnessRuleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        input.validate()?;
        let rule = input.rule;
        let revision = input.revision;
        write(self, |connection| {
            let existing: Option<String> = connection
                .query_row(
                    "SELECT harness_id FROM harness_rules WHERE harness_id=?1",
                    [&rule.harness_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            if existing.is_some() {
                let stored = load_rule(connection, &rule.harness_id)?;
                if stored != rule {
                    return Err(harness_revision_conflict());
                }
                return Ok(stored);
            }
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM harness_rules WHERE project_id=?1",
                    [rule.scope.project_id()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let count = u64::try_from(count).map_err(|_| codec_error("invalid rule count"))?;
            intention_domain::harness::validate_harness_rule_count(count + 1)?;
            connection
                .execute(
                    "INSERT INTO harness_rules(
                        harness_id, scope_kind, project_id, session_id, lifecycle_state,
                        active_revision, service_session_id, updated_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    sqlite::params![
                        rule.harness_id,
                        rule.scope.kind_name(),
                        rule.scope.project_id(),
                        rule.scope.session_id(),
                        rule.lifecycle_state.name(),
                        sqlite_integer(
                            rule.active_revision,
                            "revision is outside the SQLite range"
                        )?,
                        rule.service_session_id,
                        sqlite_integer(
                            rule.updated_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            insert_revision(connection, &revision)?;
            connection
                .execute(
                    "INSERT INTO harness_counters(
                        harness_id, cause_chain_depth, concurrent_non_terminal,
                        total_launches, direct_successors, updated_at_ms)
                     VALUES (?1, 0, 0, 0, 0, ?2)",
                    sqlite::params![
                        rule.harness_id,
                        sqlite_integer(
                            rule.updated_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            Ok(rule)
        })
    }

    fn load_harness_rule(&self, harness_id: String) -> DtoResult<HarnessRuleRecordDto> {
        let connection = self.connection()?;
        load_rule(&connection, &harness_id)
    }

    fn load_harness_rule_revision(
        &self,
        harness_id: String,
        revision: u64,
    ) -> DtoResult<HarnessRuleRevisionRecordDto> {
        let connection = self.connection()?;
        load_revision(&connection, &harness_id, revision)
    }

    fn revise_harness_rule(
        &self,
        input: ReviseHarnessRuleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        input.validate()?;
        let harness_id = input.harness_id;
        let expected_revision = input.expected_revision;
        let revision = input.revision;
        write(self, |connection| {
            let rule = load_rule(connection, &harness_id)?;
            if rule.active_revision != expected_revision {
                return Err(harness_revision_conflict());
            }
            insert_revision(connection, &revision)?;
            connection
                .execute(
                    "UPDATE harness_rules SET active_revision=?2, updated_at_ms=?3 WHERE harness_id=?1",
                    sqlite::params![
                        harness_id,
                        sqlite_integer(revision.revision, "revision is outside the SQLite range")?,
                        sqlite_integer(
                            revision.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            load_rule(connection, &harness_id)
        })
    }

    fn transition_harness_rule_lifecycle(
        &self,
        input: TransitionHarnessRuleLifecycleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        let harness_id = input.harness_id;
        let expected_revision = input.expected_revision;
        let operation = input.operation;
        let has_active_run = input.has_active_run;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let rule = load_rule(connection, &harness_id)?;
            let lifecycle_state = match (rule.lifecycle_state, operation, has_active_run) {
                (HarnessRuleLifecycleStateDto::Archived, _, _) => {
                    return Err(conflict(
                        "harness_archived",
                        "an archived harness rule retains state and rejects operations",
                    ));
                }
                (HarnessRuleLifecycleStateDto::Active, HarnessRuleOperationDto::Pause, _) => {
                    HarnessRuleLifecycleStateDto::Paused
                }
                (HarnessRuleLifecycleStateDto::Paused, HarnessRuleOperationDto::Resume, _) => {
                    HarnessRuleLifecycleStateDto::Active
                }
                (
                    HarnessRuleLifecycleStateDto::Active,
                    HarnessRuleOperationDto::ExplicitLaunch
                    | HarnessRuleOperationDto::UpdateRevision,
                    _,
                )
                | (
                    HarnessRuleLifecycleStateDto::Active,
                    HarnessRuleOperationDto::CancelActiveRun,
                    true,
                ) => rule.lifecycle_state,
                (
                    HarnessRuleLifecycleStateDto::Active | HarnessRuleLifecycleStateDto::Paused,
                    HarnessRuleOperationDto::Archive,
                    false,
                ) => HarnessRuleLifecycleStateDto::Archived,
                _ => {
                    return Err(conflict(
                        "harness_not_active",
                        "the rule is not in the state this operation requires",
                    ));
                }
            };
            if rule.active_revision != expected_revision {
                return Err(harness_revision_conflict());
            }
            connection
                .execute(
                    "UPDATE harness_rules SET lifecycle_state=?2, updated_at_ms=?3 WHERE harness_id=?1",
                    sqlite::params![
                        harness_id,
                        lifecycle_state.name(),
                        sqlite_integer(occurred_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            load_rule(connection, &harness_id)
        })
    }

    fn count_harness_rules(&self, project_id: String) -> DtoResult<u64> {
        let connection = self.connection()?;
        count_rules(&connection, &project_id)
    }
}

impl HarnessTriggerRepositoryDto for SqliteStorageRepository {
    fn capture_harness_trigger(
        &self,
        input: CaptureHarnessTriggerInputDto,
    ) -> DtoResult<HarnessTriggerCaptureRecordDto> {
        let harness_id = input.harness_id.clone();
        let reason_id = input.reason_id.clone();
        let source_kind = input.source_kind;
        let rule_revision = input.rule_revision;
        let observed_at_ms = input.observed_at_ms;
        let applied_time_zone = input.applied_time_zone.clone();
        let cause_chain_reference = input.cause_chain_reference.clone();
        let bounded_references = input.bounded_references.clone();
        let missed_slots = input.catch_up_missed_slots;
        write(self, |connection| {
            let rule = load_rule(connection, &harness_id)?;
            if rule.lifecycle_state == HarnessRuleLifecycleStateDto::Archived {
                return Err(conflict(
                    "harness_archived",
                    "an archived harness rule retains state and rejects triggers",
                ));
            }
            if rule_revision == 0 {
                return Err(conflict(
                    "harness_revision_conflict",
                    "a trigger reason originates from a live rule revision",
                ));
            }
            let existing = connection
                .query_row(
                    "SELECT reason_id FROM harness_trigger_reasons WHERE reason_id=?1",
                    [&reason_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage_error)?;
            if let Some(stored_id) = existing {
                let stored = load_trigger(connection, &stored_id)?;
                if stored.harness_id != harness_id {
                    return Err(harness_source_unavailable());
                }
                return Ok(HarnessTriggerCaptureRecordDto {
                    outcome: HarnessTriggerCaptureOutcomeDto::Redelivered,
                    reason: stored,
                });
            }
            let pending = load_pending_trigger(connection, &harness_id)?;
            let (reason, outcome) = if let Some(pending) = pending {
                let mut references = pending.bounded_references.clone();
                for reference in &bounded_references {
                    if !references.contains(reference) {
                        references.push(reference.clone());
                    }
                }
                if references.len()
                    > intention_storage::harness_repo::MAX_HARNESS_TRIGGER_REFERENCES
                {
                    return Err(conflict(
                        "harness_source_limit_exceeded",
                        "the bounded trigger reference bound is exceeded",
                    ));
                }
                let first_observed_at_ms = pending.first_observed_at_ms.min(observed_at_ms);
                let last_observed_at_ms = pending.last_observed_at_ms.max(observed_at_ms);
                let coalesced_count = if missed_slots > 0 {
                    pending.coalesced_count.saturating_add(missed_slots)
                } else {
                    pending.coalesced_count.saturating_add(1)
                };
                connection
                    .execute(
                        "UPDATE harness_trigger_reasons SET first_observed_at_ms=?2,
                            last_observed_at_ms=?3, coalesced_count=?4, bounded_references=?5
                         WHERE reason_id=?1",
                        sqlite::params![
                            pending.reason_id,
                            sqlite_integer(
                                first_observed_at_ms,
                                "timestamp is outside the SQLite range"
                            )?,
                            sqlite_integer(
                                last_observed_at_ms,
                                "timestamp is outside the SQLite range"
                            )?,
                            sqlite_integer(coalesced_count, "count is outside the SQLite range")?,
                            encode_items(&references),
                        ],
                    )
                    .map_err(storage_error)?;
                (
                    load_trigger(connection, &pending.reason_id)?,
                    if missed_slots > 0 {
                        HarnessTriggerCaptureOutcomeDto::CatchUp
                    } else {
                        HarnessTriggerCaptureOutcomeDto::Coalesced
                    },
                )
            } else {
                let coalesced_count = if missed_slots > 0 { missed_slots } else { 1 };
                if bounded_references.len()
                    > intention_storage::harness_repo::MAX_HARNESS_TRIGGER_REFERENCES
                {
                    return Err(conflict(
                        "harness_source_limit_exceeded",
                        "the bounded trigger reference bound is exceeded",
                    ));
                }
                connection
                    .execute(
                        "INSERT INTO harness_trigger_reasons(
                            reason_id, harness_id, source_kind, rule_revision,
                            first_observed_at_ms, last_observed_at_ms, coalesced_count,
                            applied_time_zone, cause_chain_reference, bounded_references, state)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8, ?9, 'pending')",
                        sqlite::params![
                            reason_id,
                            harness_id,
                            source_kind.name(),
                            sqlite_integer(rule_revision, "revision is outside the SQLite range")?,
                            sqlite_integer(
                                observed_at_ms,
                                "timestamp is outside the SQLite range"
                            )?,
                            sqlite_integer(coalesced_count, "count is outside the SQLite range")?,
                            applied_time_zone,
                            cause_chain_reference,
                            encode_items(&bounded_references),
                        ],
                    )
                    .map_err(storage_error)?;
                (
                    load_trigger(connection, &reason_id)?,
                    if missed_slots > 0 {
                        HarnessTriggerCaptureOutcomeDto::CatchUp
                    } else {
                        HarnessTriggerCaptureOutcomeDto::Captured
                    },
                )
            };
            Ok(HarnessTriggerCaptureRecordDto { outcome, reason })
        })
    }

    fn load_pending_harness_trigger(
        &self,
        harness_id: String,
    ) -> DtoResult<Option<HarnessTriggerReasonRecordDto>> {
        let connection = self.connection()?;
        load_pending_trigger(&connection, &harness_id)
    }

    fn load_harness_trigger_reason(
        &self,
        reason_id: String,
    ) -> DtoResult<HarnessTriggerReasonRecordDto> {
        let connection = self.connection()?;
        load_trigger(&connection, &reason_id)
    }

    fn record_harness_launch(
        &self,
        input: RecordHarnessLaunchInputDto,
    ) -> DtoResult<RecordHarnessLaunchOutcomeDto> {
        let harness_id = input.harness_id;
        let reason_id = input.reason_id;
        let cause_chain_depth = input.cause_chain_depth;
        let direct_successor = input.direct_successor;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            let reason = load_trigger(connection, &reason_id)?;
            if reason.harness_id != harness_id
                || reason.state != HarnessTriggerReasonStateDto::Pending
            {
                return Err(harness_source_unavailable());
            }
            let counters = load_counters(connection, &harness_id)?;
            intention_domain::harness::validate_harness_cause_depth(cause_chain_depth)?;
            intention_domain::harness::validate_harness_total_launches(
                counters.total_launches.saturating_add(1),
            )?;
            intention_domain::harness::validate_harness_concurrency_count(
                counters.concurrent_non_terminal.saturating_add(1),
            )?;
            let direct_successors = if direct_successor {
                intention_domain::harness::validate_harness_successor_count(
                    counters.direct_successors.saturating_add(1),
                )?;
                counters.direct_successors.saturating_add(1)
            } else {
                counters.direct_successors
            };
            connection
                .execute(
                    "UPDATE harness_trigger_reasons SET state='admitted' WHERE reason_id=?1",
                    [&reason_id],
                )
                .map_err(storage_error)?;
            connection
                .execute(
                    "UPDATE harness_counters SET cause_chain_depth=?2,
                        concurrent_non_terminal=?3, total_launches=?4,
                        direct_successors=?5, updated_at_ms=?6
                     WHERE harness_id=?1",
                    sqlite::params![
                        harness_id,
                        sqlite_integer(
                            counters.cause_chain_depth.max(cause_chain_depth),
                            "depth is outside the SQLite range"
                        )?,
                        sqlite_integer(
                            counters.concurrent_non_terminal.saturating_add(1),
                            "concurrency is outside the SQLite range"
                        )?,
                        sqlite_integer(
                            counters.total_launches.saturating_add(1),
                            "total is outside the SQLite range"
                        )?,
                        sqlite_integer(
                            direct_successors,
                            "successors are outside the SQLite range"
                        )?,
                        sqlite_integer(occurred_at_ms, "timestamp is outside the SQLite range")?,
                    ],
                )
                .map_err(storage_error)?;
            Ok(RecordHarnessLaunchOutcomeDto {
                reason: load_trigger(connection, &reason_id)?,
                counters: load_counters(connection, &harness_id)?,
            })
        })
    }

    fn load_harness_counters(&self, harness_id: String) -> DtoResult<HarnessCounterRecordDto> {
        let connection = self.connection()?;
        load_counters(&connection, &harness_id)
    }
}

impl HarnessCheckpointRepositoryDto for SqliteStorageRepository {
    fn store_harness_dossier(
        &self,
        input: HarnessDossierRecordDto,
    ) -> DtoResult<HarnessDossierRecordDto> {
        input.validate()?;
        write(self, |connection| {
            let exists: Option<String> = connection
                .query_row(
                    "SELECT harness_id FROM harness_rules WHERE harness_id=?1",
                    [&input.harness_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            if exists.is_none() {
                return Err(harness_not_active());
            }
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO harness_dossiers(
                        dossier_id, harness_id, rule_revision, reason_id, task_digest,
                        source_references, typed_references, checkpoint_reference,
                        dossier_bytes, canonical_dossier_digest, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    sqlite::params![
                        input.dossier_id,
                        input.harness_id,
                        sqlite_integer(
                            input.rule_revision,
                            "revision is outside the SQLite range"
                        )?,
                        input.reason_id,
                        input.task_digest,
                        encode_items(&input.source_references),
                        encode_items(&input.typed_references),
                        input.checkpoint_reference,
                        sqlite_integer(input.dossier_bytes, "size is outside the SQLite range")?,
                        input.canonical_dossier_digest,
                        sqlite_integer(
                            input.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                let stored = load_dossier(connection, &input.dossier_id)?;
                if stored != input {
                    return Err(conflict(
                        "harness_source_unavailable",
                        "the dossier identity is already bound to different content",
                    ));
                }
            }
            Ok(input)
        })
    }

    fn load_harness_dossier(&self, dossier_id: String) -> DtoResult<HarnessDossierRecordDto> {
        let connection = self.connection()?;
        load_dossier(&connection, &dossier_id)
    }

    fn commit_harness_checkpoint(
        &self,
        input: CommitHarnessCheckpointInputDto,
    ) -> DtoResult<CommitHarnessCheckpointOutcomeDto> {
        let harness_id = input.harness_id;
        let producing_run_id = input.producing_run_id;
        let run_outcome = input.run_outcome;
        let candidate = input.candidate;
        write(self, |connection| {
            load_rule(connection, &harness_id)?;
            if let Some(candidate) = &candidate {
                candidate.validate()?;
                if candidate.harness_id != harness_id
                    || candidate.producing_run_id != producing_run_id
                {
                    return Err(conflict(
                        "harness_checkpoint_unavailable",
                        "the checkpoint candidate does not belong to this rule and run",
                    ));
                }
            }
            let current = load_current_checkpoint(connection, &harness_id)?;
            if run_outcome != HarnessRunOutcomeDto::Completed {
                return Ok(CommitHarnessCheckpointOutcomeDto {
                    disposition: HarnessCheckpointDispositionDto::RetainedPrevious,
                    current,
                });
            }
            let Some(candidate) = candidate else {
                return Err(conflict(
                    "harness_checkpoint_unavailable",
                    "a completed harness run supplies a completely validated candidate",
                ));
            };
            let expected_revision = current.as_ref().map_or(1, |checkpoint| {
                checkpoint.checkpoint_revision.saturating_add(1)
            });
            if candidate.checkpoint_revision != expected_revision {
                return Err(harness_revision_conflict());
            }
            connection
                .execute(
                    "UPDATE harness_checkpoints SET is_current=0 WHERE harness_id=?1 AND is_current=1",
                    [&harness_id],
                )
                .map_err(storage_error)?;
            connection
                .execute(
                    "INSERT INTO harness_checkpoints(
                        checkpoint_id, harness_id, rule_revision, producing_run_id,
                        checkpoint_revision, content_digest, checkpoint_bytes, is_current,
                        created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
                    sqlite::params![
                        candidate.checkpoint_id,
                        candidate.harness_id,
                        sqlite_integer(
                            candidate.rule_revision,
                            "revision is outside the SQLite range"
                        )?,
                        candidate.producing_run_id,
                        sqlite_integer(
                            candidate.checkpoint_revision,
                            "revision is outside the SQLite range"
                        )?,
                        candidate.content_digest,
                        sqlite_integer(
                            candidate.checkpoint_bytes,
                            "size is outside the SQLite range"
                        )?,
                        sqlite_integer(
                            candidate.created_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            Ok(CommitHarnessCheckpointOutcomeDto {
                disposition: HarnessCheckpointDispositionDto::Replaced,
                current: Some(candidate),
            })
        })
    }

    fn load_current_harness_checkpoint(
        &self,
        harness_id: String,
    ) -> DtoResult<Option<HarnessCheckpointRecordDto>> {
        let connection = self.connection()?;
        load_current_checkpoint(&connection, &harness_id)
    }

    fn append_harness_journal_record(
        &self,
        input: AppendHarnessJournalRecordInputDto,
    ) -> DtoResult<HarnessJournalRecordDto> {
        let harness_id = input.harness_id;
        let record_kind = input.record_kind;
        let safe_summary = input.safe_summary;
        let canonical_record_digest = input.canonical_record_digest;
        let occurred_at_ms = input.occurred_at_ms;
        write(self, |connection| {
            load_rule(connection, &harness_id)?;
            let next: i64 = connection
                .query_row(
                    "SELECT COALESCE(MAX(sequence), 0) + 1 FROM harness_journal WHERE harness_id=?1",
                    [&harness_id],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let sequence =
                u64::try_from(next).map_err(|_| codec_error("invalid journal sequence"))?;
            let record = HarnessJournalRecordDto {
                harness_id,
                sequence,
                record_kind,
                safe_summary,
                canonical_record_digest,
                occurred_at_ms,
            };
            record.validate()?;
            connection
                .execute(
                    "INSERT INTO harness_journal(
                        harness_id, sequence, record_kind, safe_summary,
                        canonical_record_digest, occurred_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    sqlite::params![
                        record.harness_id,
                        sqlite_integer(record.sequence, "sequence is outside the SQLite range")?,
                        record.record_kind.name(),
                        record.safe_summary,
                        record.canonical_record_digest,
                        sqlite_integer(
                            record.occurred_at_ms,
                            "timestamp is outside the SQLite range"
                        )?,
                    ],
                )
                .map_err(storage_error)?;
            Ok(record)
        })
    }

    fn load_harness_journal(
        &self,
        harness_id: String,
        input: LoadHarnessJournalInputDto,
    ) -> DtoResult<Vec<HarnessJournalRecordDto>> {
        if input.limit == 0
            || input.limit > intention_storage::harness_repo::MAX_HARNESS_JOURNAL_PAGE
        {
            return Err(intention_types::ErrorDto::validation(
                "harness_source_unavailable",
                "the harness journal page limit is unusable",
            ));
        }
        let connection = self.connection()?;
        load_journal_page(&connection, &harness_id, input.after_sequence, input.limit)
    }
}

fn load_dossier(
    connection: &sqlite::Connection,
    dossier_id: &str,
) -> DtoResult<HarnessDossierRecordDto> {
    connection
        .query_row(
            "SELECT harness_id, rule_revision, reason_id, task_digest, source_references,
                    typed_references, checkpoint_reference, dossier_bytes,
                    canonical_dossier_digest, created_at_ms
             FROM harness_dossiers WHERE dossier_id=?1",
            [dossier_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        )
        .map_err(harness_source_unavailable_or_storage)
        .and_then(|row| {
            Ok(HarnessDossierRecordDto {
                dossier_id: dossier_id.to_owned(),
                harness_id: row.0,
                rule_revision: u64::try_from(row.1).map_err(|_| codec_error("invalid revision"))?,
                reason_id: row.2,
                task_digest: row.3,
                source_references: decode_items(&row.4)?,
                typed_references: decode_items(&row.5)?,
                checkpoint_reference: row.6,
                dossier_bytes: u64::try_from(row.7).map_err(|_| codec_error("invalid size"))?,
                canonical_dossier_digest: row.8,
                created_at_ms: u64::try_from(row.9)
                    .map_err(|_| codec_error("invalid timestamp"))?,
            })
        })
}

fn load_current_checkpoint(
    connection: &sqlite::Connection,
    harness_id: &str,
) -> DtoResult<Option<HarnessCheckpointRecordDto>> {
    connection
        .query_row(
            "SELECT checkpoint_id, rule_revision, producing_run_id, checkpoint_revision,
                    content_digest, checkpoint_bytes, created_at_ms
             FROM harness_checkpoints WHERE harness_id=?1 AND is_current=1",
            [harness_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
        .and_then(|row| {
            row.map(|row| {
                Ok(HarnessCheckpointRecordDto {
                    checkpoint_id: row.0,
                    harness_id: harness_id.to_owned(),
                    rule_revision: u64::try_from(row.1)
                        .map_err(|_| codec_error("invalid revision"))?,
                    producing_run_id: row.2,
                    checkpoint_revision: u64::try_from(row.3)
                        .map_err(|_| codec_error("invalid revision"))?,
                    content_digest: row.4,
                    checkpoint_bytes: u64::try_from(row.5)
                        .map_err(|_| codec_error("invalid size"))?,
                    is_current: true,
                    created_at_ms: u64::try_from(row.6)
                        .map_err(|_| codec_error("invalid timestamp"))?,
                })
            })
            .transpose()
        })
}
