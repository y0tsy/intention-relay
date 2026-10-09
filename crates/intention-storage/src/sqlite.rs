//! SQLite-backed current-state storage implementation.
//!
//! The public boundary is DTO-only. SQLite connections, SQL rows, paths, and
//! JSON codecs remain private implementation details of this crate.
//!
//! The complete current schema is created directly on open and stamped with
//! one integer in the SQLite header: there is no migration chain, and a
//! database file that does not carry the current stamp is discarded and
//! recreated from scratch. Every
//! state-changing method commits its change in exactly one immediate
//! transaction and returns the committed values; there is no event log,
//! snapshot, cursor, or replay.

use std::fs::remove_file;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Mutex;

use crate::{
    AcceptedTurnOutcomeDto, MAX_DISCOVERY_RESULT_BYTES, MAX_PENDING_TURN_PROJECTION_BYTES,
    MAX_REASONING_HISTORY_AGGREGATE_BYTES, MAX_TURN_CONTENT_BYTES, ProfileUsageAggregateDto,
    ProviderCatalogAuditRecordKindDto, ProviderCatalogRevisionDto, ProviderCatalogStateDto,
    ProviderDiscoveryAttemptDto, ProviderDiscoveryAttemptStateDto, ProviderDiscoveryFailureDto,
    ProviderProfilePolicyEntryDto, ReasoningHistorySourceStepDto, RunOutcomeDto,
    SessionProviderProfileChangeDto, SessionProviderProfileDto, StartingRunModelContextDto,
    StorageRepositoryDto, ToolResultEvidenceDto, ToolResultMetadataEntryDto, ToolResultStatusDto,
};
use intention_config::ConfigSnapshotDto;
use intention_proto::provider::{
    CatalogRevisionId, CredentialTransportModeDto, ProviderDiscoveryAttemptId,
    ProviderDiscoveryResultDto, ProviderKindDescriptorRevisionId, ProviderKindDescriptorRevisionV1,
    ProviderKindId, ProviderModelRecordDto, ProviderPricingPolicyDto, ProviderProfileId,
    ProviderProfilePolicyDto, ProviderProfileRevisionId, ProviderProfileRevisionV1,
    ReasoningHistoryManifestDto, ResolvedRunProviderSelectionDto,
};
use intention_proto::{
    ConfigRevisionId, CreateSessionCommandDto, DtoResult, ErrorCategoryDto, ErrorDto,
    ErrorRetryDto, FinishReasonDto, IdempotencyKey, ProjectId, RemoveTurnCommandDto, RunId,
    SessionId, SessionSnapshotDto, TimestampDto, ToolCallId, TurnId, UsageDto, WorkspaceId,
};
use intention_proto::{
    MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto, RunModeDto, RunProjectionDto,
    RunStatusDto, SessionProjectionDto, WorkspaceRootDto, run_status_is_terminal,
    validate_run_status_transition,
};
use sqlite::OptionalExtension;

/// The durable run statuses that accept no further transitions.
const TERMINAL_STATUSES: &str = "'completed','failed','interrupted'";

/// The durable discovery attempt states that accept no further transition.
const TERMINAL_DISCOVERY_STATES: &str = "'completed','failed','interrupted'";

/// The transcript projection columns in their canonical decode order.
const MESSAGE_COLUMNS: &str = "session_id, run_id, kind, text, reasoning, tool_call_id, tool_id";

/// The complete current storage schema (logical version 1): the current-state
/// tables created directly on open, including the provider catalog, selection,
/// discovery, usage, and reasoning-history tables of the control-plane slice.
/// There is no migration chain, and no event log, snapshot, cursor, or journal
/// table exists under the single live schema. `SCHEMA_STAMP` records the
/// version this text implements.
const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS workspace_roots (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  root TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  mode TEXT NOT NULL,
  provider_profile_id TEXT,
  projection_revision INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  config_revision_id TEXT NOT NULL,
  status TEXT NOT NULL,
  provider_kind TEXT,
  model TEXT,
  usage_json TEXT,
  finish_reason TEXT,
  error_code TEXT,
  error_message TEXT,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  UNIQUE(session_id, turn_id)
);
CREATE TABLE IF NOT EXISTS turns (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  proposed_run_id TEXT NOT NULL,
  config_revision_id TEXT NOT NULL,
  content TEXT NOT NULL,
  state TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE(session_id, idempotency_key),
  UNIQUE(proposed_run_id)
);
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  run_id TEXT,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  reasoning TEXT,
  tool_call_id TEXT,
  tool_id TEXT,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS tool_results (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  tool_call_id TEXT NOT NULL UNIQUE,
  session_id TEXT NOT NULL,
  run_id TEXT NOT NULL,
  tool_id TEXT NOT NULL,
  status TEXT NOT NULL,
  content TEXT NOT NULL,
  metadata_json TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS configuration_revisions (
  id TEXT PRIMARY KEY,
  snapshot_json TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS provider_kind_revisions (
  kind_id TEXT NOT NULL,
  descriptor_revision_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (kind_id, descriptor_revision_id)
);
CREATE TABLE IF NOT EXISTS provider_kind_descriptors (
  descriptor_revision_id TEXT PRIMARY KEY,
  kind_id TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS provider_profile_revisions (
  profile_id TEXT NOT NULL,
  revision_id TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (profile_id, revision_id)
);
CREATE TABLE IF NOT EXISTS provider_profile_policies (
  profile_id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  enabled INTEGER NOT NULL,
  pricing_declared INTEGER NOT NULL,
  input_per_million_tokens REAL,
  output_per_million_tokens REAL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS provider_catalog_revisions (
  id TEXT PRIMARY KEY,
  default_profile_id TEXT,
  captured_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS provider_catalog_kinds (
  catalog_revision_id TEXT NOT NULL,
  kind_id TEXT NOT NULL,
  descriptor_revision_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  PRIMARY KEY (catalog_revision_id, kind_id)
);
CREATE TABLE IF NOT EXISTS provider_catalog_profiles (
  catalog_revision_id TEXT NOT NULL,
  profile_id TEXT NOT NULL,
  revision_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  PRIMARY KEY (catalog_revision_id, profile_id)
);
CREATE TABLE IF NOT EXISTS provider_catalog_state (
  id INTEGER PRIMARY KEY,
  accepted_catalog_revision_id TEXT,
  activated_catalog_revision_id TEXT,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS provider_catalog_audit (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  record_kind TEXT NOT NULL,
  catalog_revision_id TEXT,
  recorded_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS provider_profile_tombstones (
  profile_id TEXT NOT NULL,
  removed_catalog_revision_id TEXT NOT NULL,
  removed_at INTEGER NOT NULL,
  PRIMARY KEY (profile_id, removed_catalog_revision_id)
);
CREATE TABLE IF NOT EXISTS provider_kind_tombstones (
  kind_id TEXT NOT NULL,
  removed_catalog_revision_id TEXT NOT NULL,
  removed_at INTEGER NOT NULL,
  PRIMARY KEY (kind_id, removed_catalog_revision_id)
);
CREATE TABLE IF NOT EXISTS provider_discovery_attempts (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL,
  state TEXT NOT NULL,
  prepared_at INTEGER NOT NULL,
  started_at INTEGER,
  terminated_at INTEGER,
  failure_code TEXT,
  failure_message TEXT
);
CREATE TABLE IF NOT EXISTS provider_discovery_result_records (
  attempt_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  model_id TEXT NOT NULL,
  display_name TEXT,
  PRIMARY KEY (attempt_id, ordinal)
);
CREATE TABLE IF NOT EXISTS run_provider_selections (
  run_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  selection_contract_revision INTEGER NOT NULL,
  profile_id TEXT NOT NULL,
  provider_profile_revision_id TEXT NOT NULL,
  kind_id TEXT NOT NULL,
  kind_descriptor_revision_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  normalized_effective_endpoint TEXT,
  credential_transport_mode TEXT NOT NULL,
  credential_transport_safe_header_name TEXT,
  declared_model_capability_subset TEXT NOT NULL,
  resolved_reasoning_policy TEXT NOT NULL,
  effective_execution_policy TEXT NOT NULL,
  effective_loopback_policy TEXT NOT NULL,
  provider_driver_contract_revision TEXT NOT NULL,
  selection_source TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reasoning_history_manifests (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  run_id TEXT NOT NULL UNIQUE,
  transfer TEXT NOT NULL,
  compatibility_id TEXT,
  aggregate_size_bytes INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reasoning_history_manifest_entries (
  manifest_id TEXT NOT NULL,
  source_ordinal INTEGER NOT NULL,
  record_ordinal INTEGER NOT NULL,
  source_session_id TEXT NOT NULL,
  source_run_id TEXT NOT NULL,
  final_assistant_message_id INTEGER,
  category TEXT,
  size_bytes INTEGER,
  PRIMARY KEY (manifest_id, source_ordinal, record_ordinal)
);
CREATE TABLE IF NOT EXISTS reasoning_history_bounds (
  manifest_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  run_id TEXT NOT NULL UNIQUE,
  transfer TEXT NOT NULL,
  compatibility_id TEXT,
  source_entry_count INTEGER NOT NULL,
  aggregate_size_bytes INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS one_active_run_per_session ON runs(session_id)
  WHERE status NOT IN ('completed','failed','interrupted');
CREATE INDEX IF NOT EXISTS messages_session_id_id ON messages(session_id, id);
CREATE INDEX IF NOT EXISTS messages_session_run_id_id ON messages(session_id, run_id, id);
CREATE INDEX IF NOT EXISTS run_provider_selections_usage
  ON run_provider_selections(profile_id, provider_profile_revision_id, model_id);
CREATE INDEX IF NOT EXISTS provider_discovery_attempts_state
  ON provider_discovery_attempts(state);
";

/// The single schema stamp of the current storage schema, written to the
/// `user_version` header field of every database this module creates.
///
/// Bump this integer whenever `SCHEMA_SQL` or the shape of any persisted JSON
/// column changes: the next open discards the whole database file and its
/// rollback journal and recreates the current schema from scratch. That discard
/// is the only version gate; there is no migration path.
const SCHEMA_STAMP: i32 = 4;

/// Returns whether the open database already carries the current schema stamp.
/// A database written under any other stamp is discarded by its opener.
fn carries_current_schema_stamp(connection: &sqlite::Connection) -> DtoResult<bool> {
    match connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0)) {
        Ok(stamp) => Ok(stamp == SCHEMA_STAMP),
        // A file that cannot be read as a database at all is a partial write or
        // a truncated image: it does not carry the current stamp either, so it
        // takes the same discard path instead of failing the whole open.
        Err(sqlite::Error::SqliteFailure(failure, _))
            if matches!(
                failure.code,
                sqlite::ErrorCode::NotADatabase | sqlite::ErrorCode::DatabaseCorrupt
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(storage_error(error)),
    }
}

/// Recreates the database file at a location that does not carry the current
/// schema stamp, removing the file and its rollback journal so the following
/// open creates the current schema from scratch. The discard is reported on
/// stderr because it is the one path that loses durable data.
fn recreate_stale_database(location: &str) -> DtoResult<()> {
    let current = {
        let connection = sqlite::Connection::open(location).map_err(storage_error)?;
        carries_current_schema_stamp(&connection)?
    };
    if current {
        return Ok(());
    }
    // The rollback journal goes first: a crash between the two removals must
    // never leave a journal whose database image no longer exists.
    for suffix in ["-journal", ""] {
        match remove_file(format!("{location}{suffix}")) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(unavailable()),
        }
    }
    report_schema_discard();
    Ok(())
}

/// Reports one discarded database on stderr, the daemon's only observable log.
#[allow(
    clippy::print_stderr,
    reason = "The schema discard is the one path that loses durable data and must be observable."
)]
fn report_schema_discard() {
    eprintln!("storage_schema_discarded: database did not carry schema stamp {SCHEMA_STAMP}");
}

/// A local absolute SQLite database location whose string is never exposed again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteDatabaseLocationDto(String);

impl SqliteDatabaseLocationDto {
    /// Validates a local, absolute database location suitable for explicit test injection.
    ///
    /// # Errors
    ///
    /// Returns a validation error for blank or non-absolute locations.
    pub fn new(location: impl Into<String>) -> DtoResult<Self> {
        let location = location.into();
        if location.trim().is_empty() || !Path::new(&location).is_absolute() {
            return Err(ErrorDto::validation(
                "invalid_storage_location",
                "storage database location must be non-empty and absolute",
            ));
        }
        Ok(Self(location))
    }
}

/// Durable SQLite implementation of the DTO-only storage repository.
pub struct SqliteStorageRepository {
    connection: Mutex<sqlite::Connection>,
    #[cfg(test)]
    fault: Mutex<Option<FaultPoint>>,
}

/// One accepted turn row awaiting its single-transaction insert.
struct NewTurnRow<'a> {
    /// The durable turn identity to insert.
    turn_id: TurnId,
    /// The owning durable session.
    session_id: SessionId,
    /// The run identity this turn proposed or was accepted with.
    proposed_run_id: RunId,
    /// The immutable configuration revision this turn selected.
    config_revision_id: ConfigRevisionId,
    /// The idempotency key that admitted this turn.
    idempotency_key: IdempotencyKey,
    /// The accepted user content.
    content: &'a str,
    /// The durable turn state to record.
    state: &'static str,
    /// The external commit time in whole unix seconds.
    created_at: i64,
}

impl SqliteStorageRepository {
    /// Opens or creates a local database at an explicitly supplied absolute
    /// location. A database that does not carry the current schema stamp is
    /// discarded and recreated from scratch; the current schema is then created
    /// directly on open and stamped with `SCHEMA_STAMP`.
    ///
    /// Opening also terminalizes every unfinished discovery attempt as
    /// interrupted: discovery has no automatic continuation, so a restart must
    /// never leave a before-start or started attempt unfinished. Runs keep their
    /// explicit recovery call, which the composition performs before readiness.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error when the database cannot be opened or
    /// the current schema cannot be created.
    pub fn open(location: SqliteDatabaseLocationDto) -> DtoResult<Self> {
        let location = location.0;
        if Path::new(&location).exists() {
            recreate_stale_database(&location)?;
        }
        let connection = sqlite::Connection::open(&location).map_err(storage_error)?;
        // No pragma is set here: the schema declares no foreign keys, and the
        // repository owns exactly one connection, so WAL's concurrent readers
        // buy nothing. The database deliberately keeps SQLite's default
        // rollback journal.
        connection
            .execute_batch(SCHEMA_SQL)
            .map_err(storage_error)?;
        connection
            .execute_batch(&format!("PRAGMA user_version = {SCHEMA_STAMP};"))
            .map_err(storage_error)?;
        let repository = Self {
            connection: Mutex::new(connection),
            #[cfg(test)]
            fault: Mutex::new(None),
        };
        repository.terminalize_discovery_attempts()?;
        Ok(repository)
    }

    /// Terminalizes every unfinished discovery attempt in one transaction.
    ///
    /// The open-time recovery has no caller-supplied time, so each attempt
    /// records its own last known evidence time as its terminal time: the fact
    /// that recovery closed the attempt is recorded, while no later time is
    /// fabricated. The explicit recovery method records the caller's time
    /// instead.
    fn terminalize_discovery_attempts(&self) -> DtoResult<()> {
        self.with_immediate_transaction(|transaction| {
            transaction
                .execute(
                    &format!(
                        "UPDATE provider_discovery_attempts \
                         SET state='interrupted', terminated_at=COALESCE(started_at, prepared_at) \
                         WHERE state NOT IN ({TERMINAL_DISCOVERY_STATES})"
                    ),
                    [],
                )
                .map_err(storage_error)?;
            Ok(())
        })
    }

    fn connection(&self) -> DtoResult<std::sync::MutexGuard<'_, sqlite::Connection>> {
        self.connection.lock().map_err(|_| unavailable())
    }

    fn begin<'a>(
        &self,
        connection: &'a mut sqlite::Connection,
    ) -> DtoResult<sqlite::Transaction<'a>> {
        connection
            .transaction_with_behavior(sqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)
    }

    /// Runs one durable operation inside exactly one immediate transaction.
    ///
    /// The operation reads and writes through the borrowed transaction. The
    /// helper commits it when the operation returns `Ok` and rolls every staged
    /// row back when it returns `Err`; the shared connection guard lives only
    /// for the duration of this helper, so its borrow is scoped by ordinary
    /// Rust.
    fn with_immediate_transaction<T>(
        &self,
        operation: impl FnOnce(&sqlite::Transaction<'_>) -> DtoResult<T>,
    ) -> DtoResult<T> {
        let mut connection = self.connection()?;
        let transaction = self.begin(&mut connection)?;
        let value = operation(&transaction)?;
        transaction.commit().map_err(storage_error)?;
        drop(connection);
        Ok(value)
    }

    #[cfg(test)]
    fn arm_fault(&self, point: FaultPoint) {
        if let Ok(mut armed) = self.fault.lock() {
            *armed = Some(point);
        }
    }

    #[cfg(test)]
    fn fault(&self, point: FaultPoint) -> DtoResult<()> {
        let triggered = {
            let mut armed = self.fault.lock().map_err(|_| unavailable())?;
            let triggered = *armed == Some(point);
            if triggered {
                *armed = None;
            }
            drop(armed);
            triggered
        };
        if triggered {
            return Err(ErrorDto::new(
                "injected_storage_fault",
                ErrorCategoryDto::Internal,
                "a deterministic storage test fault was injected",
                ErrorRetryDto::Never,
                None,
            )?);
        }
        Ok(())
    }

    #[cfg(not(test))]
    const fn fault(&self, _: FaultPoint) -> DtoResult<()> {
        let _ = self;
        Ok(())
    }

    /// Records one already-redacted configuration revision, idempotently when
    /// the same revision identity carries equal configuration.
    fn store_config(
        connection: &sqlite::Connection,
        snapshot: &ConfigSnapshotDto,
    ) -> DtoResult<()> {
        snapshot.validate_for_persistence()?;
        let encoded = serde_json::to_string(snapshot).map_err(codec_error)?;
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO configuration_revisions(id, snapshot_json, created_at) VALUES (?1, ?2, ?3)",
                sqlite::params![
                    snapshot.revision_id().to_string(),
                    encoded,
                    snapshot.captured_at().unix_seconds()
                ],
            )
            .map_err(storage_error)?;
        if inserted == 0 {
            let existing: String = connection
                .query_row(
                    "SELECT snapshot_json FROM configuration_revisions WHERE id=?1",
                    [snapshot.revision_id().to_string()],
                    |row| row.get(0),
                )
                .map_err(not_found_or_storage)?;
            let existing: ConfigSnapshotDto =
                serde_json::from_str(&existing).map_err(codec_error)?;
            if existing != *snapshot {
                return Err(config_revision_conflict());
            }
        }
        Ok(())
    }

    /// Requires one durable session row to exist.
    fn require_session(connection: &sqlite::Connection, session_id: SessionId) -> DtoResult<()> {
        connection
            .query_row(
                "SELECT 1 FROM sessions WHERE id=?1",
                [session_id.to_string()],
                |_| Ok(()),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(record_not_found)
    }

    /// Requires one committed credential-free selection for a run identity and
    /// returns its profile identity.
    fn require_run_selection(
        connection: &sqlite::Connection,
        run_id: RunId,
    ) -> DtoResult<ProviderProfileId> {
        let profile = connection
            .query_row(
                "SELECT profile_id FROM run_provider_selections WHERE run_id=?1",
                [run_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(|| {
                codec_error("a started run has no committed credential-free provider selection")
            })?;
        ProviderProfileId::parse(&profile).map_err(codec_error)
    }

    /// Reads the current committed projection of one session from its rows.
    fn projection_of(
        connection: &sqlite::Connection,
        session_id: SessionId,
    ) -> DtoResult<SessionProjectionDto> {
        let session = connection
            .query_row(
                "SELECT sessions.project_id, sessions.workspace_id, workspace_roots.root, sessions.mode, \
                 sessions.provider_profile_id, sessions.projection_revision \
                 FROM sessions JOIN workspace_roots ON workspace_roots.id=sessions.workspace_id \
                 WHERE sessions.id=?1",
                [session_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .map_err(not_found_or_storage)?;
        let active = connection
            .query_row(
                &format!(
                    "SELECT runs.id, runs.turn_id, runs.status, runs.config_revision_id, selections.profile_id \
                     FROM runs LEFT JOIN run_provider_selections AS selections ON selections.run_id=runs.id \
                     WHERE runs.session_id=?1 AND runs.status NOT IN ({TERMINAL_STATUSES})"
                ),
                [session_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?
            .map(|(run, turn, status, revision, profile)| {
                run_projection(session_id, &run, &turn, &status, &revision, profile.as_deref())
            })
            .transpose()?;
        let (pending_turns, pending_turns_omitted) = bounded_pending_turns(connection, session_id)?;
        let projection = SessionProjectionDto::new(
            ProjectId::parse(&session.0).map_err(codec_error)?,
            session_id,
            WorkspaceId::parse(&session.1).map_err(codec_error)?,
            WorkspaceRootDto::parse(session.2).map_err(codec_error)?,
            RunModeDto::parse(&session.3)?,
            None,
            active,
            pending_turns,
        )?;
        let projection = projection.with_pending_turns_omitted(pending_turns_omitted);
        let provider_profile_id = session
            .4
            .as_deref()
            .map(ProviderProfileId::parse)
            .transpose()
            .map_err(codec_error)?;
        Ok(with_session_provider_state(
            projection,
            provider_profile_id,
            u64::try_from(session.5).map_err(codec_error)?,
        ))
    }

    /// Inserts one committed transcript row with its external commit time.
    fn insert_message(
        connection: &sqlite::Connection,
        message: &MessageProjectionDto,
        created_at: i64,
    ) -> DtoResult<()> {
        connection
            .execute(
                "INSERT INTO messages(session_id, run_id, kind, text, reasoning, tool_call_id, tool_id, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                sqlite::params![
                    message.session_id().to_string(),
                    message.run_id().map(|run| run.to_string()),
                    message.kind().as_str(),
                    message.text(),
                    message.reasoning(),
                    message.tool_call_id().map(|call| call.to_string()),
                    message.tool_id(),
                    created_at,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    /// Inserts one accepted turn row in its recorded durable state.
    fn insert_turn(connection: &sqlite::Connection, turn: &NewTurnRow<'_>) -> DtoResult<()> {
        connection
            .execute(
                "INSERT INTO turns(id, session_id, proposed_run_id, config_revision_id, content, state, idempotency_key, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                sqlite::params![
                    turn.turn_id.to_string(),
                    turn.session_id.to_string(),
                    turn.proposed_run_id.to_string(),
                    turn.config_revision_id.to_string(),
                    turn.content,
                    turn.state,
                    turn.idempotency_key.to_string(),
                    turn.created_at,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    /// Replays the committed outcome of an already-accepted idempotency key.
    ///
    /// Returns `None` when the key has no durable turn row yet, and a typed
    /// conflict when the key is bound to different durable content.
    fn replayed_turn(
        connection: &sqlite::Connection,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: &str,
        proposed_run_id: RunId,
        config_revision_id: ConfigRevisionId,
    ) -> DtoResult<Option<AcceptedTurnOutcomeDto>> {
        let existing = connection
            .query_row(
                "SELECT id, content, proposed_run_id, config_revision_id, state FROM turns \
                 WHERE session_id=?1 AND idempotency_key=?2",
                sqlite::params![session_id.to_string(), idempotency_key.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;
        let Some((turn, stored_content, stored_run_id, stored_revision_id, state)) = existing
        else {
            return Ok(None);
        };
        if stored_content != content
            || stored_run_id != proposed_run_id.to_string()
            || stored_revision_id != config_revision_id.to_string()
        {
            return Err(turn_idempotency_conflict());
        }
        let turn_id = TurnId::parse(&turn).map_err(codec_error)?;
        // A repeated acceptance replays the current durable outcome: a started
        // turn reports its run's actual status together with the user message
        // that started the run, and a queued or joined turn is still the
        // pending projection it was accepted as.
        let outcome = match state.as_str() {
            "started" => {
                let run = load_scoped_run(connection, session_id, proposed_run_id)?;
                let message = first_user_message(connection, session_id, proposed_run_id)?;
                AcceptedTurnOutcomeDto::Started { run, message }
            }
            "pending" | "appended" => AcceptedTurnOutcomeDto::Pending(
                PendingTurnProjectionDto::new(session_id, turn_id, stored_content)?,
            ),
            "removed" => return Err(turn_idempotency_conflict()),
            _ => return Err(codec_error("invalid durable turn state")),
        };
        Ok(Some(outcome))
    }

    /// Returns whether a proposed run identity is already reserved by any turn
    /// or run.
    fn proposed_run_identity_is_bound(
        connection: &sqlite::Connection,
        proposed_run_id: RunId,
    ) -> DtoResult<bool> {
        connection
            .query_row(
                "SELECT 1 FROM turns WHERE proposed_run_id=?1 \
                 UNION ALL SELECT 1 FROM runs WHERE id=?1 LIMIT 1",
                [proposed_run_id.to_string()],
                |_| Ok(()),
            )
            .optional()
            .map_err(storage_error)
            .map(|bound| bound.is_some())
    }

    /// Returns whether one session already owns a run that accepts no successor.
    fn session_has_active_run(
        connection: &sqlite::Connection,
        session_id: SessionId,
    ) -> DtoResult<bool> {
        connection
            .query_row(
                &format!(
                    "SELECT 1 FROM runs WHERE session_id=?1 AND status NOT IN ({TERMINAL_STATUSES})"
                ),
                [session_id.to_string()],
                |_| Ok(()),
            )
            .optional()
            .map_err(storage_error)
            .map(|active| active.is_some())
    }

    /// Queues one accepted turn behind the session's active run.
    fn queue_pending_turn(
        connection: &sqlite::Connection,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: &str,
        proposed_run_id: RunId,
        config_revision_id: ConfigRevisionId,
        created_at: i64,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        let turn_id = TurnId::new();
        Self::insert_turn(
            connection,
            &NewTurnRow {
                turn_id,
                session_id,
                proposed_run_id,
                config_revision_id,
                idempotency_key,
                content,
                state: "pending",
                created_at,
            },
        )?;
        let turn = PendingTurnProjectionDto::new(session_id, turn_id, content)?;
        Ok(AcceptedTurnOutcomeDto::Pending(turn))
    }

    /// Starts the run that serves either the oldest pending turn or the newly
    /// accepted turn, committing its run row and its starting user message.
    fn start_run_for_turn(
        connection: &sqlite::Connection,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: &str,
        proposed_run_id: RunId,
        config_revision_id: ConfigRevisionId,
        created_at: i64,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        let accepted_turn_id = TurnId::new();
        let (turn_id, run_id, revision_id, message_text) =
            if let Some((pending_turn, pending_run, pending_revision, pending_content)) =
                oldest_pending_turn(connection, session_id)?
            {
                // The newly accepted turn stays queued behind the promoted one.
                Self::insert_turn(
                    connection,
                    &NewTurnRow {
                        turn_id: accepted_turn_id,
                        session_id,
                        proposed_run_id,
                        config_revision_id,
                        idempotency_key,
                        content,
                        state: "pending",
                        created_at,
                    },
                )?;
                connection
                    .execute(
                        "UPDATE turns SET state='started' WHERE session_id=?1 AND id=?2",
                        sqlite::params![session_id.to_string(), pending_turn.to_string()],
                    )
                    .map_err(storage_error)?;
                (pending_turn, pending_run, pending_revision, pending_content)
            } else {
                Self::insert_turn(
                    connection,
                    &NewTurnRow {
                        turn_id: accepted_turn_id,
                        session_id,
                        proposed_run_id,
                        config_revision_id,
                        idempotency_key,
                        content,
                        state: "started",
                        created_at,
                    },
                )?;
                (
                    accepted_turn_id,
                    proposed_run_id,
                    config_revision_id,
                    content.to_owned(),
                )
            };
        connection
            .execute(
                "INSERT INTO runs(id, session_id, turn_id, config_revision_id, status, started_at) \
                 VALUES (?1, ?2, ?3, ?4, 'starting', ?5)",
                sqlite::params![
                    run_id.to_string(),
                    session_id.to_string(),
                    turn_id.to_string(),
                    revision_id.to_string(),
                    created_at
                ],
            )
            .map_err(storage_error)?;
        // The run that starts here is the turn's proposed run identity, whose
        // committed selection was written by this acceptance or by the earlier
        // acceptance that queued the promoted turn; a run without its selection
        // would be unrunnable evidence.
        let provider_profile_id = Self::require_run_selection(connection, run_id)?;
        let message = MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::User,
            message_text,
            None,
            None,
            None,
        )?;
        Self::insert_message(connection, &message, created_at)?;
        let run = with_run_provider_profile(
            RunProjectionDto::new(
                session_id,
                run_id,
                turn_id,
                RunStatusDto::Starting,
                revision_id,
            ),
            Some(provider_profile_id),
        );
        Ok(AcceptedTurnOutcomeDto::Started { run, message })
    }
}

impl StorageRepositoryDto for SqliteStorageRepository {
    fn create_session(
        &self,
        command: CreateSessionCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<SessionProjectionDto> {
        let session_id = command.session_id();
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            if transaction
                .query_row(
                    "SELECT 1 FROM sessions WHERE id=?1",
                    [session_id.to_string()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(storage_error)?
                .is_some()
            {
                return Err(conflict(
                    "session_already_exists",
                    "the durable session already exists",
                ));
            }
            // No product-level project name exists yet, so the stable project
            // identity is its canonical name until a creation command carries
            // one.
            transaction
                .execute(
                    "INSERT INTO projects(id, name, created_at) VALUES (?1, ?1, ?2) \
                     ON CONFLICT(id) DO NOTHING",
                    sqlite::params![command.project_id().to_string(), occurred_at],
                )
                .map_err(storage_error)?;
            // The root binding is unique in both directions: a second workspace
            // identity bound to an already-used root must surface as the same
            // typed conflict as the reverse identity/root mismatch, instead of
            // a raw SQLite constraint failure.
            let root_bound_to_other_identity = transaction
                .query_row(
                    "SELECT id FROM workspace_roots WHERE root=?1",
                    [command.workspace_root().as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage_error)?
                .is_some_and(|bound| bound != command.workspace_id().to_string());
            if root_bound_to_other_identity {
                return Err(conflict(
                    "workspace_root_conflict",
                    "the workspace root is already bound to a different workspace identity",
                ));
            }
            transaction
                .execute(
                    "INSERT INTO workspace_roots(id, project_id, root, created_at) VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT(id) DO NOTHING",
                    sqlite::params![
                        command.workspace_id().to_string(),
                        command.project_id().to_string(),
                        command.workspace_root().as_str(),
                        occurred_at
                    ],
                )
                .map_err(storage_error)?;
            let stored_workspace_root: String = transaction
                .query_row(
                    "SELECT root FROM workspace_roots WHERE id=?1",
                    [command.workspace_id().to_string()],
                    |row| row.get(0),
                )
                .map_err(not_found_or_storage)?;
            if stored_workspace_root != command.workspace_root().as_str() {
                return Err(conflict(
                    "workspace_root_conflict",
                    "the workspace identity is already bound to a different root",
                ));
            }
            transaction
                .execute(
                    "INSERT INTO sessions(id, project_id, workspace_id, mode, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                    sqlite::params![
                        session_id.to_string(),
                        command.project_id().to_string(),
                        command.workspace_id().to_string(),
                        command.mode().as_str(),
                        occurred_at
                    ],
                )
                .map_err(storage_error)?;
            Self::projection_of(transaction, session_id)
        })
    }

    fn accept_user_turn(
        &self,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: &str,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        selection: ResolvedRunProviderSelectionDto,
        reasoning_history: Option<ReasoningHistoryManifestDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_turn_content",
                "user turn content must not be empty",
            ));
        }
        if content.len() > MAX_TURN_CONTENT_BYTES {
            return Err(ErrorDto::validation(
                "invalid_turn_content",
                "user turn content exceeds the durable canonical size limit",
            ));
        }
        if let Some(manifest) = &reasoning_history {
            validate_reasoning_history(manifest)?;
        }
        let config_revision_id = config_snapshot.revision_id();
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            Self::require_session(transaction, session_id)?;
            if let Some(replay) = Self::replayed_turn(
                transaction,
                session_id,
                idempotency_key,
                content,
                proposed_run_id,
                config_revision_id,
            )? {
                return Ok(replay);
            }
            // Proposed run identities are globally unique while the idempotency
            // check above is scoped to the session, so a caller-supplied run
            // identity already durable anywhere must surface as a typed
            // conflict instead of a raw SQLite constraint failure.
            if Self::proposed_run_identity_is_bound(transaction, proposed_run_id)? {
                return Err(turn_identity_conflict());
            }
            Self::store_config(transaction, &config_snapshot)?;
            // The resolved selection is durable for the turn's proposed run
            // identity whether this acceptance starts the run now or the turn
            // stays queued, so every run carries exactly one committed
            // credential-free selection.
            insert_selection(
                transaction,
                session_id,
                proposed_run_id,
                &selection,
                occurred_at,
            )?;
            if let Some(manifest) = &reasoning_history {
                insert_reasoning_history(
                    transaction,
                    session_id,
                    proposed_run_id,
                    manifest,
                    occurred_at,
                )?;
            }
            if Self::session_has_active_run(transaction, session_id)? {
                let queued = Self::queue_pending_turn(
                    transaction,
                    session_id,
                    idempotency_key,
                    content,
                    proposed_run_id,
                    config_revision_id,
                    occurred_at,
                )?;
                self.fault(FaultPoint::TurnAcceptance)?;
                return Ok(queued);
            }
            // An idle session either starts the oldest pending message (its own
            // stored run identity, configuration revision, and content) or
            // starts the newly accepted turn. The accepted turn stays pending
            // in the first case; it joins the promoted run's context later.
            let started = Self::start_run_for_turn(
                transaction,
                session_id,
                idempotency_key,
                content,
                proposed_run_id,
                config_revision_id,
                occurred_at,
            )?;
            self.fault(FaultPoint::TurnAcceptance)?;
            Ok(started)
        })
    }

    fn remove_turn(
        &self,
        command: RemoveTurnCommandDto,
        // A removed pending turn keeps no durable removal time, so the supplied
        // time is intentionally unrecorded; it stays in the contract so every
        // state-changing method carries one uniform time parameter.
        _occurred_at: TimestampDto,
    ) -> DtoResult<PendingTurnProjectionDto> {
        let session_id = command.session_id();
        let turn_id = command.turn_id();
        self.with_immediate_transaction(|transaction| {
            let content: String = transaction
                .query_row(
                    "SELECT content FROM turns WHERE session_id=?1 AND id=?2 AND state='pending'",
                    sqlite::params![session_id.to_string(), turn_id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?
                .ok_or_else(pending_turn_not_found)?;
            transaction
                .execute(
                    "UPDATE turns SET state='removed' WHERE session_id=?1 AND id=?2 AND state='pending'",
                    sqlite::params![session_id.to_string(), turn_id.to_string()],
                )
                .map_err(storage_error)?;
            PendingTurnProjectionDto::new(session_id, turn_id, content)
        })
    }

    fn consume_pending_user_turns(
        &self,
        session_id: SessionId,
        run_id: RunId,
        occurred_at: TimestampDto,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        self.with_immediate_transaction(|transaction| {
            require_active_run(transaction, session_id, run_id)?;
            let pending = pending_turns(transaction, session_id)?;
            let occurred_at = occurred_at.unix_seconds();
            let mut messages = Vec::with_capacity(pending.len());
            for turn in pending {
                let message = MessageProjectionDto::new(
                    session_id,
                    Some(run_id),
                    MessageKindDto::User,
                    turn.content(),
                    None,
                    None,
                    None,
                )?;
                Self::insert_message(transaction, &message, occurred_at)?;
                transaction
                    .execute(
                        "UPDATE turns SET state='appended' WHERE session_id=?1 AND id=?2",
                        sqlite::params![session_id.to_string(), turn.turn_id().to_string()],
                    )
                    .map_err(storage_error)?;
                messages.push(message);
            }
            Ok(messages)
        })
    }

    fn transition_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto> {
        self.with_immediate_transaction(|transaction| {
            let current = load_scoped_run(transaction, session_id, run_id)?;
            validate_run_status_transition(current.status(), status)?;
            // A terminal transition is the run's last write, so it records the
            // supplied time as the finish time; a non-terminal transition only
            // re-writes the NULL it never set.
            let finished_at = run_status_is_terminal(status).then(|| occurred_at.unix_seconds());
            transaction
                .execute(
                    "UPDATE runs SET status=?3, finished_at=?4 WHERE session_id=?1 AND id=?2",
                    sqlite::params![
                        session_id.to_string(),
                        run_id.to_string(),
                        status.as_str(),
                        finished_at
                    ],
                )
                .map_err(storage_error)?;
            Ok(with_run_provider_profile(
                RunProjectionDto::new(
                    session_id,
                    run_id,
                    current.turn_id(),
                    status,
                    current.config_revision_id(),
                ),
                current.provider_profile_id().cloned(),
            ))
        })
    }

    fn finish_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        outcome: RunOutcomeDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto> {
        let status = outcome.status();
        self.with_immediate_transaction(|transaction| {
            let current = load_scoped_run(transaction, session_id, run_id)?;
            let run = with_run_provider_profile(
                RunProjectionDto::new(
                    session_id,
                    run_id,
                    current.turn_id(),
                    status,
                    current.config_revision_id(),
                ),
                current.provider_profile_id().cloned(),
            );
            if current.status() == status {
                // The run already holds a terminal outcome. Repeating the exact
                // recorded outcome is idempotent; a different outcome would
                // silently discard its evidence, so it is a typed conflict.
                if load_run_outcome(transaction, session_id, run_id)? == outcome {
                    return Ok(run);
                }
                return Err(run_outcome_conflict());
            }
            validate_run_status_transition(current.status(), status)?;
            let usage_json = outcome
                .usage()
                .map(|usage| serde_json::to_string(usage).map_err(codec_error))
                .transpose()?;
            let finish_reason = outcome.finish_reason().map(finish_reason_name);
            transaction
                .execute(
                    "UPDATE runs SET status=?3, usage_json=?4, finish_reason=?5, error_code=?6, \
                     error_message=?7, finished_at=?8 WHERE session_id=?1 AND id=?2",
                    sqlite::params![
                        session_id.to_string(),
                        run_id.to_string(),
                        status.as_str(),
                        usage_json,
                        finish_reason,
                        outcome.error_code(),
                        outcome.error_message(),
                        occurred_at.unix_seconds()
                    ],
                )
                .map_err(storage_error)?;
            self.fault(FaultPoint::RunOutcome)?;
            Ok(run)
        })
    }

    fn append_message(
        &self,
        message: MessageProjectionDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<MessageProjectionDto> {
        let session_id = message.session_id();
        self.with_immediate_transaction(|transaction| {
            Self::require_session(transaction, session_id)?;
            if let Some(run_id) = message.run_id() {
                require_active_run(transaction, session_id, run_id)?;
            }
            Self::insert_message(transaction, &message, occurred_at.unix_seconds())?;
            self.fault(FaultPoint::Message)?;
            Ok(message)
        })
    }

    fn write_tool_result(
        &self,
        evidence: ToolResultEvidenceDto,
        message: MessageProjectionDto,
    ) -> DtoResult<ToolResultEvidenceDto> {
        if message.kind() != MessageKindDto::ToolResult
            || message.session_id() != evidence.session_id()
            || message.run_id() != Some(evidence.run_id())
            || message.tool_call_id() != Some(evidence.call_id())
            || message.tool_id() != Some(evidence.tool_id())
        {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "a tool result commits with its own answering transcript row",
            ));
        }
        let session_id = evidence.session_id();
        let run_id = evidence.run_id();
        let call_id = evidence.call_id();
        self.with_immediate_transaction(|transaction| {
            Self::require_session(transaction, session_id)?;
            require_active_run(transaction, session_id, run_id)?;
            let duplicate = transaction
                .query_row(
                    "SELECT 1 FROM tool_results WHERE tool_call_id=?1",
                    [call_id.to_string()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(storage_error)?
                .is_some();
            if duplicate {
                return Err(tool_result_conflict());
            }
            let metadata_json = serde_json::to_string(evidence.metadata()).map_err(codec_error)?;
            transaction
                .execute(
                    "INSERT INTO tool_results(tool_call_id, session_id, run_id, tool_id, status, content, metadata_json, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    sqlite::params![
                        call_id.to_string(),
                        session_id.to_string(),
                        run_id.to_string(),
                        evidence.tool_id(),
                        evidence.status().as_str(),
                        evidence.content(),
                        metadata_json,
                        evidence.occurred_at().unix_seconds()
                    ],
                )
                .map_err(storage_error)?;
            Self::insert_message(transaction, &message, evidence.occurred_at().unix_seconds())?;
            self.fault(FaultPoint::ToolResult)?;
            Ok(evidence)
        })
    }

    fn load_tool_result(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
    ) -> DtoResult<ToolResultEvidenceDto> {
        let row = {
            let connection = self.connection()?;
            connection
                .query_row(
                    "SELECT tool_id, status, content, metadata_json, created_at FROM tool_results \
                     WHERE tool_call_id=?1 AND session_id=?2 AND run_id=?3",
                    sqlite::params![
                        call_id.to_string(),
                        session_id.to_string(),
                        run_id.to_string()
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(storage_error)?
        };
        let Some((tool_id, status, content, metadata_json, created_at)) = row else {
            return Err(not_found(
                "tool_result_not_found",
                "the requested durable tool result does not exist",
            ));
        };
        let metadata: Vec<ToolResultMetadataEntryDto> =
            serde_json::from_str(&metadata_json).map_err(|_| tool_result_unavailable())?;
        ToolResultEvidenceDto::new(
            session_id,
            run_id,
            call_id,
            tool_id,
            ToolResultStatusDto::parse(&status).map_err(|_| tool_result_unavailable())?,
            content,
            metadata,
            TimestampDto::from_unix_seconds(created_at).map_err(|_| tool_result_unavailable())?,
        )
        .map_err(|_| tool_result_unavailable())
    }

    fn load_run_config_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        let connection = self.connection()?;
        let revision_id: String = connection
            .query_row(
                "SELECT config_revision_id FROM runs WHERE session_id=?1 AND id=?2",
                sqlite::params![session_id.to_string(), run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                // Only a genuinely missing run row is a permanent not-found;
                // a backend read failure stays transient unavailability.
                sqlite::Error::QueryReturnedNoRows => run_configuration_not_found(),
                other => storage_error(other),
            })?;
        let snapshot: String = connection
            .query_row(
                "SELECT snapshot_json FROM configuration_revisions WHERE id=?1",
                [revision_id],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                // The selection row is genuinely absent; a backend read
                // failure stays transient unavailability.
                sqlite::Error::QueryReturnedNoRows => run_configuration_unavailable(),
                other => storage_error(other),
            })?;
        drop(connection);
        let snapshot: ConfigSnapshotDto = serde_json::from_str(&snapshot).map_err(codec_error)?;
        // The read returns only a snapshot the storage gate would accept again:
        // a stored selection that is no longer persistable is a decode failure,
        // not a value the caller can act on.
        snapshot.validate_for_persistence().map_err(codec_error)?;
        Ok(snapshot)
    }

    fn load_starting_run_model_context(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto> {
        // A read takes no transaction: the connection guard already serialises
        // every operation on this connection, and an immediate transaction
        // would only add a RESERVED lock for no isolation benefit.
        let connection = self.connection()?;
        // An absent or cross-session run row is the same safe context-specific
        // unavailability as a non-starting run; only a backend failure keeps
        // its classified storage error.
        let target = scoped_run_row(&connection, session_id, run_id)?
            .ok_or_else(run_model_context_unavailable)?;
        let target = run_projection(
            session_id,
            &run_id.to_string(),
            &target.0,
            &target.1,
            &target.2,
            target.3.as_deref(),
        )
        .map_err(|_| run_model_context_unavailable())?;
        if target.status() != RunStatusDto::Starting {
            return Err(run_model_context_unavailable());
        }
        let safe_config = load_config_snapshot(&connection, target.config_revision_id())?;
        let messages = starting_context_messages(&connection, session_id, run_id)?;
        let context = StartingRunModelContextDto::new(session_id, run_id, safe_config, messages)
            .map_err(|_| run_model_context_unavailable())?;
        drop(connection);
        Ok(context)
    }

    fn load_run_projection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunProjectionDto> {
        let connection = self.connection()?;
        let run = load_scoped_run(&connection, session_id, run_id)?;
        drop(connection);
        Ok(run)
    }

    fn load_session_projection(&self, session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        let connection = self.connection()?;
        let projection = Self::projection_of(&connection, session_id)?;
        drop(connection);
        Ok(projection)
    }

    fn load_recent_messages(
        &self,
        session_id: SessionId,
        limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        let connection = self.connection()?;
        let messages = recent_messages(&connection, session_id, limit)?;
        drop(connection);
        Ok(messages)
    }

    /// Loads the session projection and its newest transcript rows from one
    /// committed read instead of two independent reads.
    fn load_session_snapshot(
        &self,
        session_id: SessionId,
        message_limit: u32,
    ) -> DtoResult<SessionSnapshotDto> {
        let connection = self.connection()?;
        let projection = Self::projection_of(&connection, session_id)?;
        let messages = recent_messages(&connection, session_id, message_limit)?;
        drop(connection);
        SessionSnapshotDto::with_projection(session_id, projection, messages)
    }

    fn load_run_messages(
        &self,
        session_id: SessionId,
        run_id: RunId,
        limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        validate_message_limit(limit)?;
        let connection = self.connection()?;
        load_scoped_run(&connection, session_id, run_id)?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT {MESSAGE_COLUMNS} FROM messages WHERE session_id=?1 AND run_id=?2 \
                 ORDER BY id DESC LIMIT ?3"
            ))
            .map_err(storage_error)?;
        let rows = statement
            .query_map(
                sqlite::params![session_id.to_string(), run_id.to_string(), i64::from(limit)],
                raw_message_row,
            )
            .map_err(storage_error)?;
        let mut messages = rows
            .map(|row| {
                let row = row.map_err(storage_error)?;
                decode_message(row)
            })
            .collect::<DtoResult<Vec<_>>>()?;
        messages.reverse();
        drop(statement);
        drop(connection);
        Ok(messages)
    }

    fn recover_unfinished_runs(
        &self,
        recovered_at: TimestampDto,
    ) -> DtoResult<Vec<RunProjectionDto>> {
        let recovered_at = recovered_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let mut statement = transaction
                .prepare(&format!(
                    "SELECT runs.session_id, runs.id, runs.turn_id, runs.config_revision_id, runs.status, \
                     selections.profile_id \
                     FROM runs LEFT JOIN run_provider_selections AS selections ON selections.run_id=runs.id \
                     WHERE runs.status NOT IN ({TERMINAL_STATUSES}) ORDER BY runs.session_id, runs.id"
                ))
                .map_err(storage_error)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                })
                .map_err(storage_error)?;
            let unfinished = rows
                .map(|row| row.map_err(storage_error))
                .collect::<DtoResult<Vec<_>>>()?;
            drop(statement);
            let mut recovered = Vec::with_capacity(unfinished.len());
            for (session, run, turn, revision, status, profile) in unfinished {
                let current = RunStatusDto::parse(&status)?;
                validate_run_status_transition(current, RunStatusDto::Interrupted)?;
                recovered.push(run_projection(
                    SessionId::parse(&session).map_err(codec_error)?,
                    &run,
                    &turn,
                    RunStatusDto::Interrupted.as_str(),
                    &revision,
                    profile.as_deref(),
                )?);
            }
            // One transaction marks every unfinished run, so a crash mid-recovery
            // leaves no partially recovered set.
            transaction
                .execute(
                    &format!(
                        "UPDATE runs SET status='interrupted', finished_at=?1 \
                         WHERE status NOT IN ({TERMINAL_STATUSES})"
                    ),
                    [recovered_at],
                )
                .map_err(storage_error)?;
            Ok(recovered)
        })
    }

    fn accept_configuration_revision(&self, snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        self.with_immediate_transaction(|transaction| {
            Self::store_config(transaction, &snapshot)?;
            self.fault(FaultPoint::ConfigRevision)?;
            Ok(())
        })
    }

    fn accept_catalog_revision(
        &self,
        revision: ProviderCatalogRevisionDto,
    ) -> DtoResult<ProviderCatalogStateDto> {
        let accepted_at = revision.captured_at().unix_seconds();
        let catalog_revision_id = revision.catalog_revision_id();
        self.with_immediate_transaction(|transaction| {
            let state = load_catalog_state(transaction)?;
            // A kind identity keeps the declaration it was first accepted
            // with; the valid path to changed closed parts is a new kind.
            for descriptor in revision.kind_descriptors() {
                require_kind_declaration_is_immutable(transaction, descriptor)?;
            }
            for descriptor in revision.kind_descriptors() {
                store_kind_declaration(transaction, descriptor, accepted_at)?;
            }
            // Profile revision history is append-only: one (profile, revision)
            // identity never binds a different revision body.
            for profile in revision.profile_revisions() {
                store_profile_revision(transaction, profile, accepted_at)?;
            }
            // Display, enabled, and pricing are current state, not revision
            // identity, so acceptance refreshes them for every member.
            for entry in revision.profile_policies() {
                store_profile_policy_row(
                    transaction,
                    entry.profile_id(),
                    entry.policy(),
                    accepted_at,
                )?;
            }
            store_catalog_revision(transaction, &revision, accepted_at)?;
            store_catalog_membership(transaction, &revision)?;
            let removed = append_removal_history(transaction, &revision, &state, accepted_at)?;
            insert_catalog_audit(
                transaction,
                ProviderCatalogAuditRecordKindDto::ProviderCatalogCandidatePrepared,
                Some(catalog_revision_id),
                accepted_at,
            )?;
            if removed {
                insert_catalog_audit(
                    transaction,
                    ProviderCatalogAuditRecordKindDto::ProviderCatalogRemovalPending,
                    Some(catalog_revision_id),
                    accepted_at,
                )?;
                insert_catalog_audit(
                    transaction,
                    ProviderCatalogAuditRecordKindDto::ProviderCatalogRemovalAccepted,
                    Some(catalog_revision_id),
                    accepted_at,
                )?;
            }
            insert_catalog_audit(
                transaction,
                ProviderCatalogAuditRecordKindDto::ProviderCatalogAccepted,
                Some(catalog_revision_id),
                accepted_at,
            )?;
            let state = save_catalog_state(
                transaction,
                Some(catalog_revision_id),
                state.activated_catalog_revision_id(),
                accepted_at,
            )?;
            self.fault(FaultPoint::CatalogAcceptance)?;
            Ok(state)
        })
    }

    fn mark_catalog_activated(
        &self,
        catalog_revision_id: CatalogRevisionId,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderCatalogStateDto> {
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let state = load_catalog_state(transaction)?;
            if state.accepted_catalog_revision_id() != Some(catalog_revision_id) {
                return Err(provider_catalog_changed());
            }
            let previous = state.activated_catalog_revision_id();
            if previous == Some(catalog_revision_id) {
                // The exact accepted catalog is already active: repeating the
                // activation changes nothing.
                return Ok(state);
            }
            if previous.is_some() {
                // Accepted-but-not-activated after a crash: only the exact
                // accepted revision recovers, and only when it becomes active.
                insert_catalog_audit(
                    transaction,
                    ProviderCatalogAuditRecordKindDto::ProviderCatalogActivationRecoveryRequired,
                    Some(catalog_revision_id),
                    occurred_at,
                )?;
            }
            insert_catalog_audit(
                transaction,
                ProviderCatalogAuditRecordKindDto::ProviderCatalogActivated,
                Some(catalog_revision_id),
                occurred_at,
            )?;
            let state = save_catalog_state(
                transaction,
                Some(catalog_revision_id),
                Some(catalog_revision_id),
                occurred_at,
            )?;
            if previous.is_some() {
                insert_catalog_audit(
                    transaction,
                    ProviderCatalogAuditRecordKindDto::ProviderCatalogRecoveryCompleted,
                    Some(catalog_revision_id),
                    occurred_at,
                )?;
            }
            self.fault(FaultPoint::CatalogActivation)?;
            Ok(state)
        })
    }

    fn record_catalog_candidate_rejected(
        &self,
        candidate_revision_id: CatalogRevisionId,
        occurred_at: TimestampDto,
    ) -> DtoResult<()> {
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            insert_catalog_audit(
                transaction,
                ProviderCatalogAuditRecordKindDto::ProviderCatalogRemovalPending,
                Some(candidate_revision_id),
                occurred_at,
            )?;
            insert_catalog_audit(
                transaction,
                ProviderCatalogAuditRecordKindDto::ProviderCatalogCandidateRejected,
                Some(candidate_revision_id),
                occurred_at,
            )?;
            self.fault(FaultPoint::CatalogRejection)?;
            Ok(())
        })
    }

    fn load_catalog_state(&self) -> DtoResult<ProviderCatalogStateDto> {
        let connection = self.connection()?;
        let state = load_catalog_state(&connection)?;
        drop(connection);
        Ok(state)
    }

    fn load_catalog_revision(
        &self,
        catalog_revision_id: CatalogRevisionId,
    ) -> DtoResult<ProviderCatalogRevisionDto> {
        let connection = self.connection()?;
        let revision = load_catalog_revision(&connection, catalog_revision_id)?;
        drop(connection);
        Ok(revision)
    }

    fn store_profile_policy(
        &self,
        profile_id: ProviderProfileId,
        policy: ProviderProfilePolicyDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderProfilePolicyDto> {
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let state = load_catalog_state(transaction)?;
            let accepted = state
                .accepted_catalog_revision_id()
                .ok_or_else(provider_profile_not_found)?;
            if !catalog_declares_profile(transaction, accepted, profile_id.as_str())? {
                return Err(provider_profile_not_found());
            }
            store_profile_policy_row(transaction, &profile_id, &policy, occurred_at)?;
            self.fault(FaultPoint::ProfilePolicy)?;
            Ok(policy)
        })
    }

    fn set_session_provider_profile(
        &self,
        session_id: SessionId,
        profile_id: ProviderProfileId,
        expected_session_projection_revision: u64,
        occurred_at: TimestampDto,
    ) -> DtoResult<SessionProviderProfileChangeDto> {
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let (current, revision) = session_provider_profile_row(transaction, session_id)?;
            let current_revision = u64::try_from(revision).map_err(codec_error)?;
            if current_revision != expected_session_projection_revision {
                return Err(session_projection_revision_conflict());
            }
            if current.as_deref() == Some(profile_id.as_str()) {
                return Ok(SessionProviderProfileChangeDto::new(
                    session_id,
                    false,
                    current_revision,
                ));
            }
            let next = current_revision
                .checked_add(1)
                .ok_or_else(session_projection_revision_conflict)?;
            let next = i64::try_from(next).map_err(codec_error)?;
            transaction
                .execute(
                    "UPDATE sessions SET provider_profile_id=?2, projection_revision=?3, updated_at=?4 \
                     WHERE id=?1",
                    sqlite::params![
                        session_id.to_string(),
                        profile_id.as_str(),
                        next,
                        occurred_at
                    ],
                )
                .map_err(storage_error)?;
            self.fault(FaultPoint::SessionProviderProfile)?;
            Ok(SessionProviderProfileChangeDto::new(
                session_id,
                true,
                u64::try_from(next).map_err(codec_error)?,
            ))
        })
    }

    fn load_session_provider_profile(
        &self,
        session_id: SessionId,
    ) -> DtoResult<SessionProviderProfileDto> {
        let connection = self.connection()?;
        let (profile, revision) = session_provider_profile_row(&connection, session_id)?;
        drop(connection);
        let provider_profile_id = profile
            .as_deref()
            .map(ProviderProfileId::parse)
            .transpose()
            .map_err(codec_error)?;
        Ok(SessionProviderProfileDto::new(
            session_id,
            provider_profile_id,
            u64::try_from(revision).map_err(codec_error)?,
        ))
    }

    fn load_run_provider_selection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ResolvedRunProviderSelectionDto> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                &format!(
                    "SELECT {SELECTION_COLUMNS} FROM run_provider_selections \
                     WHERE session_id=?1 AND run_id=?2"
                ),
                sqlite::params![session_id.to_string(), run_id.to_string()],
                raw_selection_row,
            )
            .optional()
            .map_err(storage_error)?;
        drop(connection);
        row.map(decode_selection)
            .transpose()?
            .ok_or_else(run_provider_selection_not_found)
    }

    fn begin_discovery_attempt(
        &self,
        attempt_id: ProviderDiscoveryAttemptId,
        profile_id: ProviderProfileId,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto> {
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO provider_discovery_attempts(id, profile_id, state, prepared_at) \
                     VALUES (?1, ?2, 'prepared', ?3)",
                    sqlite::params![attempt_id.to_string(), profile_id.as_str(), occurred_at],
                )
                .map_err(storage_error)?;
            if inserted == 0 {
                return Err(discovery_attempt_conflict(
                    "the discovery attempt identity is already durable",
                ));
            }
            let attempt = load_discovery_attempt(transaction, attempt_id)?;
            self.fault(FaultPoint::DiscoveryAttempt)?;
            Ok(attempt)
        })
    }

    fn mark_discovery_started(
        &self,
        attempt_id: ProviderDiscoveryAttemptId,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto> {
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let current = load_discovery_attempt(transaction, attempt_id)?;
            if current.state() != ProviderDiscoveryAttemptStateDto::Prepared {
                return Err(discovery_attempt_state_conflict());
            }
            transaction
                .execute(
                    "UPDATE provider_discovery_attempts SET state='started', started_at=?2 WHERE id=?1",
                    sqlite::params![attempt_id.to_string(), occurred_at],
                )
                .map_err(storage_error)?;
            let attempt = load_discovery_attempt(transaction, attempt_id)?;
            self.fault(FaultPoint::DiscoveryAttempt)?;
            Ok(attempt)
        })
    }

    fn complete_discovery_attempt(
        &self,
        attempt_id: ProviderDiscoveryAttemptId,
        records: Vec<ProviderModelRecordDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryResultDto> {
        validate_discovery_records(&records)?;
        let result = ProviderDiscoveryResultDto::new(attempt_id, records)?;
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let current = load_discovery_attempt(transaction, attempt_id)?;
            if current.state() != ProviderDiscoveryAttemptStateDto::Started {
                return Err(discovery_attempt_state_conflict());
            }
            for (ordinal, record) in result.records().iter().enumerate() {
                transaction
                    .execute(
                        "INSERT INTO provider_discovery_result_records(attempt_id, ordinal, model_id, display_name) \
                         VALUES (?1, ?2, ?3, ?4)",
                        sqlite::params![
                            attempt_id.to_string(),
                            i64::try_from(ordinal).map_err(codec_error)?,
                            record.model_id(),
                            record.display_name()
                        ],
                    )
                    .map_err(storage_error)?;
            }
            transaction
                .execute(
                    "UPDATE provider_discovery_attempts SET state='completed', terminated_at=?2 WHERE id=?1",
                    sqlite::params![attempt_id.to_string(), occurred_at],
                )
                .map_err(storage_error)?;
            self.fault(FaultPoint::DiscoveryAttempt)?;
            Ok(result)
        })
    }

    fn fail_discovery_attempt(
        &self,
        attempt_id: ProviderDiscoveryAttemptId,
        failure: ProviderDiscoveryFailureDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto> {
        let occurred_at = occurred_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let current = load_discovery_attempt(transaction, attempt_id)?;
            if current.state().is_terminal() {
                return Err(discovery_attempt_state_conflict());
            }
            transaction
                .execute(
                    "UPDATE provider_discovery_attempts \
                     SET state='failed', terminated_at=?2, failure_code=?3, failure_message=?4 \
                     WHERE id=?1",
                    sqlite::params![
                        attempt_id.to_string(),
                        occurred_at,
                        failure.code(),
                        failure.message()
                    ],
                )
                .map_err(storage_error)?;
            let attempt = load_discovery_attempt(transaction, attempt_id)?;
            self.fault(FaultPoint::DiscoveryAttempt)?;
            Ok(attempt)
        })
    }

    fn recover_unfinished_discovery_attempts(
        &self,
        recovered_at: TimestampDto,
    ) -> DtoResult<Vec<ProviderDiscoveryAttemptDto>> {
        let recovered_at = recovered_at.unix_seconds();
        self.with_immediate_transaction(|transaction| {
            let mut statement = transaction
                .prepare(&format!(
                    "SELECT id FROM provider_discovery_attempts \
                     WHERE state NOT IN ({TERMINAL_DISCOVERY_STATES}) ORDER BY id"
                ))
                .map_err(storage_error)?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(storage_error)?;
            let unfinished = rows
                .map(|row| row.map_err(storage_error))
                .collect::<DtoResult<Vec<_>>>()?;
            drop(statement);
            transaction
                .execute(
                    &format!(
                        "UPDATE provider_discovery_attempts SET state='interrupted', terminated_at=?1 \
                         WHERE state NOT IN ({TERMINAL_DISCOVERY_STATES})"
                    ),
                    [recovered_at],
                )
                .map_err(storage_error)?;
            let mut recovered = Vec::with_capacity(unfinished.len());
            for attempt in unfinished {
                recovered.push(load_discovery_attempt(
                    transaction,
                    ProviderDiscoveryAttemptId::parse(&attempt).map_err(codec_error)?,
                )?);
            }
            self.fault(FaultPoint::DiscoveryRecovery)?;
            Ok(recovered)
        })
    }

    fn load_profile_usage(
        &self,
        profile_id: ProviderProfileId,
    ) -> DtoResult<Vec<ProfileUsageAggregateDto>> {
        let connection = self.connection()?;
        let usage = load_profile_usage(&connection, profile_id.as_str())?;
        drop(connection);
        Ok(usage)
    }

    fn load_reasoning_history_source(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Vec<ReasoningHistorySourceStepDto>> {
        let connection = self.connection()?;
        Self::require_session(&connection, session_id)?;
        let source = load_reasoning_history_source(&connection, session_id)?;
        drop(connection);
        Ok(source)
    }
}

/// The run provider selection columns in their canonical decode order.
const SELECTION_COLUMNS: &str = "selection_contract_revision, profile_id, provider_profile_revision_id, \
     kind_id, kind_descriptor_revision_id, model_id, normalized_effective_endpoint, \
     credential_transport_mode, credential_transport_safe_header_name, \
     declared_model_capability_subset, resolved_reasoning_policy, effective_execution_policy, \
     effective_loopback_policy, provider_driver_contract_revision, selection_source";

/// Encodes one closed typed value through the storage-owned canonical codec.
///
/// Every nested selection, revision, and policy value persists in its own
/// column through this one canonical text codec, exactly as configuration
/// revisions persist through theirs; no caller-supplied encoding crosses the
/// boundary.
fn encode_canonical<T>(value: &T) -> DtoResult<String>
where
    T: serde::Serialize + ?Sized,
{
    serde_json::to_string(value).map_err(codec_error)
}

/// Decodes one closed typed value written by [`encode_canonical`].
fn decode_canonical<T>(value: &str) -> DtoResult<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(value)
        .map_err(|_| codec_error("the durable value is not a declared typed value"))
}

/// Rejects one cross-turn manifest whose aggregate exceeds the fixed bound.
fn validate_reasoning_history(manifest: &ReasoningHistoryManifestDto) -> DtoResult<()> {
    if manifest.aggregate_size_bytes() > MAX_REASONING_HISTORY_AGGREGATE_BYTES {
        return Err(ErrorDto::validation(
            "reasoning_history_too_large",
            "the cross-turn reasoning history exceeds the fixed aggregate bound",
        ));
    }
    Ok(())
}

/// Rejects one discovery record set whose content exceeds one transport envelope.
fn validate_discovery_records(records: &[ProviderModelRecordDto]) -> DtoResult<()> {
    let total = records.iter().fold(0_usize, |total, record| {
        total
            .saturating_add(record.model_id().len())
            .saturating_add(record.display_name().map_or(0, str::len))
    });
    if total > MAX_DISCOVERY_RESULT_BYTES {
        return Err(ErrorDto::validation(
            "discovery_result_too_large",
            "the discovery result exceeds the durable result bound",
        ));
    }
    Ok(())
}

/// Inserts one credential-free resolved selection for a turn's proposed run.
///
/// The nested closed values persist through the storage-owned canonical codec
/// in their own columns; identities, scalars, and the endpoint stay plain typed
/// columns. Credentials never reach this row by construction.
fn insert_selection(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
    selection: &ResolvedRunProviderSelectionDto,
    created_at: i64,
) -> DtoResult<()> {
    connection
        .execute(
            "INSERT INTO run_provider_selections(run_id, session_id, selection_contract_revision, \
             profile_id, provider_profile_revision_id, kind_id, kind_descriptor_revision_id, model_id, \
             normalized_effective_endpoint, credential_transport_mode, credential_transport_safe_header_name, \
             declared_model_capability_subset, resolved_reasoning_policy, effective_execution_policy, \
             effective_loopback_policy, provider_driver_contract_revision, selection_source, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            sqlite::params![
                run_id.to_string(),
                session_id.to_string(),
                i64::from(selection.selection_contract_revision()),
                selection.profile_id().as_str(),
                selection.provider_profile_revision_id().to_string(),
                selection.kind_id().as_str(),
                selection.kind_descriptor_revision_id().to_string(),
                selection.model_id(),
                selection.normalized_effective_endpoint(),
                selection.credential_transport_mode().as_str(),
                selection.credential_transport_safe_header_name(),
                encode_canonical(selection.declared_model_capability_subset())?,
                encode_canonical(selection.resolved_reasoning_policy())?,
                encode_canonical(&selection.effective_execution_policy())?,
                encode_canonical(&selection.effective_loopback_policy())?,
                encode_canonical(selection.provider_driver_contract_revision())?,
                encode_canonical(&selection.selection_source())?,
                created_at,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

/// The committed columns of one run provider selection in canonical decode order.
type RawSelectionRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    String,
    String,
);

/// Decodes the canonical selection columns into a committed resolved selection.
fn raw_selection_row(row: &sqlite::Row<'_>) -> sqlite::Result<RawSelectionRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
    ))
}

/// Decodes one committed selection row through its typed validating constructor.
fn decode_selection(row: RawSelectionRow) -> DtoResult<ResolvedRunProviderSelectionDto> {
    let selection = ResolvedRunProviderSelectionDto::new(
        u16::try_from(row.0).map_err(codec_error)?,
        ProviderProfileId::parse(&row.1).map_err(codec_error)?,
        ProviderProfileRevisionId::parse(&row.2).map_err(codec_error)?,
        ProviderKindId::parse(&row.3).map_err(codec_error)?,
        ProviderKindDescriptorRevisionId::parse(&row.4).map_err(codec_error)?,
        row.5,
        row.6,
        CredentialTransportModeDto::parse(&row.7).map_err(codec_error)?,
        row.8,
        decode_canonical(&row.9)?,
        decode_canonical(&row.10)?,
        decode_canonical(&row.11)?,
        decode_canonical(&row.12)?,
        decode_canonical(&row.13)?,
        decode_canonical(&row.14)?,
    );
    selection.map_err(|_| {
        codec_error("the committed provider selection is not a declared resolved selection")
    })
}

/// Inserts one cross-turn reasoning history manifest with its entries and bound
/// audit record.
///
/// The manifest is keyed by the run it belongs to, carries no duplicate
/// reasoning text, and commits in the same transaction as that run's start. A
/// source response with no reasoning records one typed empty reference.
fn insert_reasoning_history(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
    manifest: &ReasoningHistoryManifestDto,
    created_at: i64,
) -> DtoResult<()> {
    let manifest_id = manifest.manifest_id().to_string();
    let transfer = encode_canonical(manifest.transfer())?;
    let aggregate_size_bytes =
        i64::try_from(manifest.aggregate_size_bytes()).map_err(codec_error)?;
    connection
        .execute(
            "INSERT INTO reasoning_history_manifests(id, session_id, run_id, transfer, compatibility_id, \
             aggregate_size_bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            sqlite::params![
                manifest_id,
                session_id.to_string(),
                run_id.to_string(),
                transfer,
                manifest.compatibility_id(),
                aggregate_size_bytes,
                created_at,
            ],
        )
        .map_err(storage_error)?;
    for (source_ordinal, source) in manifest.sources().iter().enumerate() {
        let source_ordinal = i64::try_from(source_ordinal).map_err(codec_error)?;
        if source.records().is_empty() {
            insert_manifest_entry(connection, &manifest_id, source_ordinal, 0, source, None)?;
            continue;
        }
        for (record_ordinal, record) in source.records().iter().enumerate() {
            insert_manifest_entry(
                connection,
                &manifest_id,
                source_ordinal,
                i64::try_from(record_ordinal).map_err(codec_error)?,
                source,
                Some(record),
            )?;
        }
    }
    connection
        .execute(
            "INSERT INTO reasoning_history_bounds(manifest_id, session_id, run_id, transfer, \
             compatibility_id, source_entry_count, aggregate_size_bytes, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            sqlite::params![
                manifest_id,
                session_id.to_string(),
                run_id.to_string(),
                encode_canonical(manifest.transfer())?,
                manifest.compatibility_id(),
                i64::from(u32::try_from(manifest.sources().len()).map_err(codec_error)?),
                aggregate_size_bytes,
                created_at,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

/// Inserts one manifest entry row, empty when the source records no reasoning.
fn insert_manifest_entry(
    connection: &sqlite::Connection,
    manifest_id: &str,
    source_ordinal: i64,
    record_ordinal: i64,
    source: &intention_proto::provider::ReasoningHistorySourceEntryDto,
    record: Option<&intention_proto::provider::ReasoningHistoryRecordReferenceDto>,
) -> DtoResult<()> {
    let category = record
        .map(|record| encode_canonical(&record.category()))
        .transpose()?;
    let size_bytes = record
        .map(|record| i64::try_from(record.size_bytes()).map_err(codec_error))
        .transpose()?;
    connection
        .execute(
            "INSERT INTO reasoning_history_manifest_entries(manifest_id, source_ordinal, record_ordinal, \
             source_session_id, source_run_id, final_assistant_message_id, category, size_bytes) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            sqlite::params![
                manifest_id,
                source_ordinal,
                record_ordinal,
                source.session_id().to_string(),
                source.run_id().to_string(),
                source.final_assistant_message_id(),
                category,
                size_bytes,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

/// Loads the current accepted and activated catalog pointers.
fn load_catalog_state(connection: &sqlite::Connection) -> DtoResult<ProviderCatalogStateDto> {
    let row = connection
        .query_row(
            "SELECT accepted_catalog_revision_id, activated_catalog_revision_id \
             FROM provider_catalog_state WHERE id=1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let Some((accepted, activated)) = row else {
        return ProviderCatalogStateDto::new(None, None);
    };
    ProviderCatalogStateDto::new(
        accepted
            .as_deref()
            .map(CatalogRevisionId::parse)
            .transpose()
            .map_err(codec_error)?,
        activated
            .as_deref()
            .map(CatalogRevisionId::parse)
            .transpose()
            .map_err(codec_error)?,
    )
}

/// Writes the current accepted and activated catalog pointers.
fn save_catalog_state(
    connection: &sqlite::Connection,
    accepted: Option<CatalogRevisionId>,
    activated: Option<CatalogRevisionId>,
    updated_at: i64,
) -> DtoResult<ProviderCatalogStateDto> {
    connection
        .execute(
            "INSERT INTO provider_catalog_state(id, accepted_catalog_revision_id, activated_catalog_revision_id, updated_at) \
             VALUES (1, ?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET accepted_catalog_revision_id=excluded.accepted_catalog_revision_id, \
             activated_catalog_revision_id=excluded.activated_catalog_revision_id, updated_at=excluded.updated_at",
            sqlite::params![
                accepted.map(|id| id.to_string()),
                activated.map(|id| id.to_string()),
                updated_at
            ],
        )
        .map_err(storage_error)?;
    ProviderCatalogStateDto::new(accepted, activated)
}

/// Appends one catalog audit record.
fn insert_catalog_audit(
    connection: &sqlite::Connection,
    record_kind: ProviderCatalogAuditRecordKindDto,
    catalog_revision_id: Option<CatalogRevisionId>,
    recorded_at: i64,
) -> DtoResult<()> {
    connection
        .execute(
            "INSERT INTO provider_catalog_audit(record_kind, catalog_revision_id, recorded_at) \
             VALUES (?1, ?2, ?3)",
            sqlite::params![
                record_kind.as_str(),
                catalog_revision_id.map(|id| id.to_string()),
                recorded_at
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

/// Requires that a kind identity keeps the declaration it was first accepted with.
fn require_kind_declaration_is_immutable(
    connection: &sqlite::Connection,
    descriptor: &ProviderKindDescriptorRevisionV1,
) -> DtoResult<()> {
    let mut statement = connection
        .prepare("SELECT body FROM provider_kind_descriptors WHERE kind_id=?1")
        .map_err(storage_error)?;
    let rows = statement
        .query_map([descriptor.kind_id().as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(storage_error)?;
    for row in rows {
        let body = row.map_err(storage_error)?;
        let stored: ProviderKindDescriptorRevisionV1 = decode_canonical(&body)?;
        if !kind_declarations_are_equivalent(&stored, descriptor) {
            return Err(kind_immutable_mismatch());
        }
    }
    Ok(())
}

/// Compares two kind declarations without their revision identity.
fn kind_declarations_are_equivalent(
    stored: &ProviderKindDescriptorRevisionV1,
    candidate: &ProviderKindDescriptorRevisionV1,
) -> bool {
    stored.kind_id() == candidate.kind_id()
        && stored.descriptor_family() == candidate.descriptor_family()
        && stored.ordered_protocol_part_revisions() == candidate.ordered_protocol_part_revisions()
        && stored.endpoint_policy() == candidate.endpoint_policy()
        && stored.credential_transport_contract() == candidate.credential_transport_contract()
        && stored.model_capability_envelope() == candidate.model_capability_envelope()
        && stored.driver_contract_family() == candidate.driver_contract_family()
}

/// Stores one kind declaration and its revision identity as append-only history.
fn store_kind_declaration(
    connection: &sqlite::Connection,
    descriptor: &ProviderKindDescriptorRevisionV1,
    created_at: i64,
) -> DtoResult<()> {
    let body = encode_canonical(descriptor)?;
    let descriptor_revision_id = descriptor.descriptor_revision_id().to_string();
    connection
        .execute(
            "INSERT OR IGNORE INTO provider_kind_revisions(kind_id, descriptor_revision_id, created_at) \
             VALUES (?1, ?2, ?3)",
            sqlite::params![
                descriptor.kind_id().as_str(),
                descriptor_revision_id,
                created_at
            ],
        )
        .map_err(storage_error)?;
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO provider_kind_descriptors(descriptor_revision_id, kind_id, body, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            sqlite::params![
                descriptor_revision_id,
                descriptor.kind_id().as_str(),
                body,
                created_at
            ],
        )
        .map_err(storage_error)?;
    if inserted == 0 {
        let (kind_id, stored): (String, String) = connection
            .query_row(
                "SELECT kind_id, body FROM provider_kind_descriptors WHERE descriptor_revision_id=?1",
                [descriptor_revision_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(not_found_or_storage)?;
        if kind_id != descriptor.kind_id().as_str() || stored != body {
            return Err(kind_immutable_mismatch());
        }
    }
    Ok(())
}

/// Stores one profile revision as append-only history.
fn store_profile_revision(
    connection: &sqlite::Connection,
    profile: &ProviderProfileRevisionV1,
    created_at: i64,
) -> DtoResult<()> {
    let body = encode_canonical(profile)?;
    let revision_id = profile.revision_id().to_string();
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO provider_profile_revisions(profile_id, revision_id, body, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            sqlite::params![profile.profile_id().as_str(), revision_id, body, created_at],
        )
        .map_err(storage_error)?;
    if inserted == 0 {
        let stored: String = connection
            .query_row(
                "SELECT body FROM provider_profile_revisions WHERE profile_id=?1 AND revision_id=?2",
                sqlite::params![profile.profile_id().as_str(), revision_id],
                |row| row.get(0),
            )
            .map_err(not_found_or_storage)?;
        if stored != body {
            return Err(profile_revision_mismatch());
        }
    }
    Ok(())
}

/// Upserts the current display, enabled, and pricing policy of one profile.
fn store_profile_policy_row(
    connection: &sqlite::Connection,
    profile_id: &ProviderProfileId,
    policy: &ProviderProfilePolicyDto,
    updated_at: i64,
) -> DtoResult<()> {
    let pricing = policy.pricing();
    connection
        .execute(
            "INSERT INTO provider_profile_policies(profile_id, display_name, enabled, pricing_declared, \
             input_per_million_tokens, output_per_million_tokens, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(profile_id) DO UPDATE SET display_name=excluded.display_name, \
             enabled=excluded.enabled, pricing_declared=excluded.pricing_declared, \
             input_per_million_tokens=excluded.input_per_million_tokens, \
             output_per_million_tokens=excluded.output_per_million_tokens, updated_at=excluded.updated_at",
            sqlite::params![
                profile_id.as_str(),
                policy.display_name(),
                i64::from(policy.enabled()),
                i64::from(pricing.is_some()),
                pricing.and_then(ProviderPricingPolicyDto::input_per_million_tokens),
                pricing.and_then(ProviderPricingPolicyDto::output_per_million_tokens),
                updated_at,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

/// Stores one catalog revision row, keeping a re-accepted revision's membership.
fn store_catalog_revision(
    connection: &sqlite::Connection,
    revision: &ProviderCatalogRevisionDto,
    captured_at: i64,
) -> DtoResult<()> {
    let catalog_revision_id = revision.catalog_revision_id().to_string();
    let default_profile_id = revision
        .default_profile_id()
        .map(|profile| profile.as_str().to_owned());
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO provider_catalog_revisions(id, default_profile_id, captured_at) \
             VALUES (?1, ?2, ?3)",
            sqlite::params![catalog_revision_id, default_profile_id, captured_at],
        )
        .map_err(storage_error)?;
    if inserted == 0 {
        // Re-accepting the same revision identity refreshes current policy and
        // the global default only: its member kinds and profiles are immutable.
        if !catalog_membership_matches(connection, revision)? {
            return Err(catalog_revision_conflict());
        }
        connection
            .execute(
                "UPDATE provider_catalog_revisions SET default_profile_id=?2 WHERE id=?1",
                sqlite::params![catalog_revision_id, default_profile_id],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

/// Returns whether the stored membership of one catalog revision equals a candidate's.
fn catalog_membership_matches(
    connection: &sqlite::Connection,
    revision: &ProviderCatalogRevisionDto,
) -> DtoResult<bool> {
    let stored_kinds = catalog_kind_members(connection, revision.catalog_revision_id())?;
    let mut candidate_kinds = revision
        .kind_descriptors()
        .iter()
        .map(|descriptor| {
            (
                descriptor.kind_id().as_str().to_owned(),
                descriptor.descriptor_revision_id().to_string(),
            )
        })
        .collect::<Vec<_>>();
    candidate_kinds.sort();
    let mut stored_kinds = stored_kinds;
    stored_kinds.sort();
    if stored_kinds != candidate_kinds {
        return Ok(false);
    }
    let stored_profiles = catalog_profile_members(connection, revision.catalog_revision_id())?;
    let mut candidate_profiles = revision
        .profile_revisions()
        .iter()
        .map(|profile| {
            (
                profile.profile_id().as_str().to_owned(),
                profile.revision_id().to_string(),
            )
        })
        .collect::<Vec<_>>();
    candidate_profiles.sort();
    let mut stored_profiles = stored_profiles;
    stored_profiles.sort();
    Ok(stored_profiles == candidate_profiles)
}

/// Returns the stored kind members of one catalog revision.
fn catalog_kind_members(
    connection: &sqlite::Connection,
    catalog_revision_id: CatalogRevisionId,
) -> DtoResult<Vec<(String, String)>> {
    let mut statement = connection
        .prepare(
            "SELECT kind_id, descriptor_revision_id FROM provider_catalog_kinds \
             WHERE catalog_revision_id=?1 ORDER BY ordinal",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([catalog_revision_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?;
    rows.map(|row| row.map_err(storage_error)).collect()
}

/// Returns the stored profile members of one catalog revision.
fn catalog_profile_members(
    connection: &sqlite::Connection,
    catalog_revision_id: CatalogRevisionId,
) -> DtoResult<Vec<(String, String)>> {
    let mut statement = connection
        .prepare(
            "SELECT profile_id, revision_id FROM provider_catalog_profiles \
             WHERE catalog_revision_id=?1 ORDER BY ordinal",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([catalog_revision_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?;
    rows.map(|row| row.map_err(storage_error)).collect()
}

/// Stores the kind and profile membership of one catalog revision.
fn store_catalog_membership(
    connection: &sqlite::Connection,
    revision: &ProviderCatalogRevisionDto,
) -> DtoResult<()> {
    let catalog_revision_id = revision.catalog_revision_id().to_string();
    for (ordinal, descriptor) in revision.kind_descriptors().iter().enumerate() {
        connection
            .execute(
                "INSERT OR IGNORE INTO provider_catalog_kinds(catalog_revision_id, kind_id, \
                 descriptor_revision_id, ordinal) VALUES (?1, ?2, ?3, ?4)",
                sqlite::params![
                    catalog_revision_id,
                    descriptor.kind_id().as_str(),
                    descriptor.descriptor_revision_id().to_string(),
                    i64::try_from(ordinal).map_err(codec_error)?
                ],
            )
            .map_err(storage_error)?;
    }
    for (ordinal, profile) in revision.profile_revisions().iter().enumerate() {
        connection
            .execute(
                "INSERT OR IGNORE INTO provider_catalog_profiles(catalog_revision_id, profile_id, \
                 revision_id, ordinal) VALUES (?1, ?2, ?3, ?4)",
                sqlite::params![
                    catalog_revision_id,
                    profile.profile_id().as_str(),
                    profile.revision_id().to_string(),
                    i64::try_from(ordinal).map_err(codec_error)?
                ],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

/// Appends removal-history tombstones for members the candidate omits and
/// returns whether the candidate removes anything.
///
/// Tombstones are append-only removal events keyed by the accepted catalog
/// revision; admission authority stays the active projection, so an identifier
/// reintroduced by a later accepted catalog is admitted again.
fn append_removal_history(
    connection: &sqlite::Connection,
    revision: &ProviderCatalogRevisionDto,
    previous: &ProviderCatalogStateDto,
    removed_at: i64,
) -> DtoResult<bool> {
    let Some(previous_id) = previous.accepted_catalog_revision_id() else {
        return Ok(false);
    };
    let removed_catalog_revision_id = revision.catalog_revision_id().to_string();
    let candidate_kind_ids = revision
        .kind_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind_id().as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let candidate_profile_ids = revision
        .profile_revisions()
        .iter()
        .map(|profile| profile.profile_id().as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut removed = false;
    for (kind_id, _) in catalog_kind_members(connection, previous_id)? {
        if candidate_kind_ids.contains(&kind_id) {
            continue;
        }
        // A kind can only be removed once the same candidate reassigns or
        // removes every dependent profile.
        let dependent = revision
            .profile_revisions()
            .iter()
            .any(|profile| profile.kind_id().as_str() == kind_id);
        if dependent {
            return Err(kind_has_dependents());
        }
        connection
            .execute(
                "INSERT OR IGNORE INTO provider_kind_tombstones(kind_id, removed_catalog_revision_id, removed_at) \
                 VALUES (?1, ?2, ?3)",
                sqlite::params![kind_id, removed_catalog_revision_id, removed_at],
            )
            .map_err(storage_error)?;
        removed = true;
    }
    for (profile_id, _) in catalog_profile_members(connection, previous_id)? {
        if candidate_profile_ids.contains(&profile_id) {
            continue;
        }
        connection
            .execute(
                "INSERT OR IGNORE INTO provider_profile_tombstones(profile_id, removed_catalog_revision_id, removed_at) \
                 VALUES (?1, ?2, ?3)",
                sqlite::params![profile_id, removed_catalog_revision_id, removed_at],
            )
            .map_err(storage_error)?;
        removed = true;
    }
    Ok(removed)
}

/// Returns whether one accepted catalog revision declares a profile.
fn catalog_declares_profile(
    connection: &sqlite::Connection,
    catalog_revision_id: CatalogRevisionId,
    profile_id: &str,
) -> DtoResult<bool> {
    connection
        .query_row(
            "SELECT 1 FROM provider_catalog_profiles WHERE catalog_revision_id=?1 AND profile_id=?2",
            sqlite::params![catalog_revision_id.to_string(), profile_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(storage_error)
        .map(|found| found.is_some())
}

/// Loads one committed catalog revision with its current member policies.
fn load_catalog_revision(
    connection: &sqlite::Connection,
    catalog_revision_id: CatalogRevisionId,
) -> DtoResult<ProviderCatalogRevisionDto> {
    let (default_profile_id, captured_at) = connection
        .query_row(
            "SELECT default_profile_id, captured_at FROM provider_catalog_revisions WHERE id=?1",
            [catalog_revision_id.to_string()],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(catalog_revision_not_found)?;
    let mut kind_descriptors = Vec::new();
    for (_, descriptor_revision_id) in catalog_kind_members(connection, catalog_revision_id)? {
        let body: String = connection
            .query_row(
                "SELECT body FROM provider_kind_descriptors WHERE descriptor_revision_id=?1",
                [descriptor_revision_id],
                |row| row.get(0),
            )
            .map_err(not_found_or_storage)?;
        kind_descriptors.push(decode_canonical(&body)?);
    }
    let mut profile_revisions = Vec::new();
    let mut profile_policies = Vec::new();
    for (profile_id, revision_id) in catalog_profile_members(connection, catalog_revision_id)? {
        let body: String = connection
            .query_row(
                "SELECT body FROM provider_profile_revisions WHERE profile_id=?1 AND revision_id=?2",
                sqlite::params![profile_id, revision_id],
                |row| row.get(0),
            )
            .map_err(not_found_or_storage)?;
        profile_revisions.push(decode_canonical(&body)?);
        let profile_id = ProviderProfileId::parse(&profile_id).map_err(codec_error)?;
        let policy = load_profile_policy(connection, &profile_id)?;
        profile_policies.push(ProviderProfilePolicyEntryDto::new(profile_id, policy));
    }
    ProviderCatalogRevisionDto::new(
        catalog_revision_id,
        default_profile_id
            .as_deref()
            .map(ProviderProfileId::parse)
            .transpose()
            .map_err(codec_error)?,
        TimestampDto::from_unix_seconds(captured_at).map_err(codec_error)?,
        kind_descriptors,
        profile_revisions,
        profile_policies,
    )
}

/// Loads the current policy of one profile member of a catalog revision.
fn load_profile_policy(
    connection: &sqlite::Connection,
    profile_id: &ProviderProfileId,
) -> DtoResult<ProviderProfilePolicyDto> {
    let (display_name, enabled, pricing_declared, input, output) = connection
        .query_row(
            "SELECT display_name, enabled, pricing_declared, input_per_million_tokens, \
             output_per_million_tokens FROM provider_profile_policies WHERE profile_id=?1",
            [profile_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                    row.get::<_, Option<f64>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(provider_profile_not_found)?;
    let pricing = if pricing_declared == 0 {
        None
    } else {
        Some(ProviderPricingPolicyDto::new(input, output).map_err(codec_error)?)
    };
    ProviderProfilePolicyDto::new(display_name, enabled != 0, pricing).map_err(codec_error)
}

/// Loads one session's durable provider default and projection revision.
fn session_provider_profile_row(
    connection: &sqlite::Connection,
    session_id: SessionId,
) -> DtoResult<(Option<String>, i64)> {
    connection
        .query_row(
            "SELECT provider_profile_id, projection_revision FROM sessions WHERE id=?1",
            [session_id.to_string()],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(not_found_or_storage)
}

/// Loads one durable discovery attempt or a typed not-found error.
fn load_discovery_attempt(
    connection: &sqlite::Connection,
    attempt_id: ProviderDiscoveryAttemptId,
) -> DtoResult<ProviderDiscoveryAttemptDto> {
    let row = connection
        .query_row(
            "SELECT profile_id, state, prepared_at, started_at, terminated_at, failure_code, \
             failure_message FROM provider_discovery_attempts WHERE id=?1",
            [attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(discovery_attempt_not_found)?;
    let prepared_at = TimestampDto::from_unix_seconds(row.2).map_err(codec_error)?;
    let started_at = row
        .3
        .map(TimestampDto::from_unix_seconds)
        .transpose()
        .map_err(codec_error)?;
    let terminated_at = row
        .4
        .map(TimestampDto::from_unix_seconds)
        .transpose()
        .map_err(codec_error)?;
    let failure = match (row.5, row.6) {
        (None, None) => None,
        (Some(code), Some(message)) => Some(ProviderDiscoveryFailureDto::new(code, message)?),
        _ => {
            return Err(codec_error(
                "a discovery attempt carries incomplete failure evidence",
            ));
        }
    };
    ProviderDiscoveryAttemptDto::new(
        attempt_id,
        ProviderProfileId::parse(&row.0).map_err(codec_error)?,
        ProviderDiscoveryAttemptStateDto::parse(&row.1)?,
        prepared_at,
        started_at,
        terminated_at,
        failure,
    )
}

/// Aggregates terminal-run usage by exact profile revision and model identity.
fn load_profile_usage(
    connection: &sqlite::Connection,
    profile_id: &str,
) -> DtoResult<Vec<ProfileUsageAggregateDto>> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT selections.provider_profile_revision_id, selections.model_id, runs.usage_json \
             FROM runs JOIN run_provider_selections AS selections ON selections.run_id=runs.id \
             WHERE selections.profile_id=?1 AND runs.status IN ({TERMINAL_STATUSES}) \
             ORDER BY selections.provider_profile_revision_id, selections.model_id, runs.id"
        ))
        .map_err(storage_error)?;
    let rows = statement
        .query_map([profile_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(storage_error)?;
    let mut aggregates = Vec::new();
    let mut current: Option<UsageAccumulator> = None;
    for row in rows {
        let (revision, model, usage_json) = row.map_err(storage_error)?;
        let usage: Option<UsageDto> = usage_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(codec_error)?;
        let reported = matches!(usage, Some(UsageDto::Reported { .. }));
        let (input, output, total) = match usage {
            Some(UsageDto::Reported {
                input_tokens,
                output_tokens,
                total_tokens,
            }) => (input_tokens, output_tokens, total_tokens),
            _ => (0, 0, 0),
        };
        let accumulate = current.as_mut().is_some_and(|accumulator| {
            accumulator.revision == revision && accumulator.model == model
        });
        if !accumulate {
            if let Some(accumulator) = current.take() {
                aggregates.push(accumulator.finish(profile_id)?);
            }
            current = Some(UsageAccumulator::new(revision, model));
        }
        if let Some(accumulator) = current.as_mut() {
            accumulator.accept(reported, input, output, total);
        }
    }
    if let Some(accumulator) = current {
        aggregates.push(accumulator.finish(profile_id)?);
    }
    Ok(aggregates)
}

/// One accumulating (profile revision, model) usage group.
struct UsageAccumulator {
    revision: String,
    model: String,
    reported_runs: u64,
    unreported_runs: u64,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

impl UsageAccumulator {
    /// Creates one empty usage group.
    const fn new(revision: String, model: String) -> Self {
        Self {
            revision,
            model,
            reported_runs: 0,
            unreported_runs: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
        }
    }

    /// Adds one terminal run's reported usage to the group.
    const fn accept(&mut self, reported: bool, input: u64, output: u64, total: u64) {
        if reported {
            self.reported_runs = self.reported_runs.saturating_add(1);
            self.input_tokens = self.input_tokens.saturating_add(input);
            self.output_tokens = self.output_tokens.saturating_add(output);
            self.total_tokens = self.total_tokens.saturating_add(total);
        } else {
            self.unreported_runs = self.unreported_runs.saturating_add(1);
        }
    }

    /// Freezes the group into its committed aggregate.
    fn finish(self, profile_id: &str) -> DtoResult<ProfileUsageAggregateDto> {
        ProfileUsageAggregateDto::new(
            ProviderProfileId::parse(profile_id).map_err(codec_error)?,
            ProviderProfileRevisionId::parse(&self.revision).map_err(codec_error)?,
            self.model,
            self.reported_runs,
            self.unreported_runs,
            self.input_tokens,
            self.output_tokens,
            self.total_tokens,
        )
    }
}

/// Loads the committed completed reasoning source steps of one session.
fn load_reasoning_history_source(
    connection: &sqlite::Connection,
    session_id: SessionId,
) -> DtoResult<Vec<ReasoningHistorySourceStepDto>> {
    let mut statement = connection
        .prepare(
            "SELECT messages.run_id, messages.id, messages.reasoning \
             FROM messages JOIN runs ON runs.id=messages.run_id \
             WHERE messages.session_id=?1 AND messages.kind='assistant' \
             AND runs.status='completed' ORDER BY messages.id",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([session_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(storage_error)?;
    rows.map(|row| {
        let (run, message, reasoning) = row.map_err(storage_error)?;
        ReasoningHistorySourceStepDto::new(
            session_id,
            RunId::parse(&run).map_err(codec_error)?,
            message,
            reasoning,
        )
    })
    .collect()
}

/// The committed rows of one transcript query.
type RawMessageRow = (
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Decodes the canonical transcript columns into a committed message projection.
fn raw_message_row(row: &sqlite::Row<'_>) -> sqlite::Result<RawMessageRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn decode_message(row: RawMessageRow) -> DtoResult<MessageProjectionDto> {
    MessageProjectionDto::new(
        SessionId::parse(&row.0).map_err(codec_error)?,
        row.1
            .as_deref()
            .map(RunId::parse)
            .transpose()
            .map_err(codec_error)?,
        MessageKindDto::parse(&row.2)?,
        row.3,
        row.4,
        row.5
            .as_deref()
            .map(ToolCallId::parse)
            .transpose()
            .map_err(codec_error)?,
        row.6,
    )
}

/// The raw durable columns of one scoped run: its turn, status, configuration
/// revision, and the committed provider profile identity of its selection.
type RawRunRow = (String, String, String, Option<String>);

/// Loads the raw durable columns of one scoped run together with its committed
/// provider profile identity, or `None` when the run row is genuinely absent. A
/// backend failure keeps its classified storage error instead of collapsing
/// into the caller's absence handling.
fn scoped_run_row(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<Option<RawRunRow>> {
    connection
        .query_row(
            "SELECT runs.turn_id, runs.status, runs.config_revision_id, selections.profile_id \
             FROM runs LEFT JOIN run_provider_selections AS selections ON selections.run_id=runs.id \
             WHERE runs.session_id=?1 AND runs.id=?2",
            sqlite::params![session_id.to_string(), run_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)
}

/// Loads one scoped durable run or a typed not-found error.
fn load_scoped_run(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<RunProjectionDto> {
    let row = scoped_run_row(connection, session_id, run_id)?.ok_or_else(record_not_found)?;
    run_projection(
        session_id,
        &run_id.to_string(),
        &row.0,
        &row.1,
        &row.2,
        row.3.as_deref(),
    )
}

/// Loads one scoped durable run that still accepts further writes.
///
/// A terminal run accepts no further transcript row, pending-turn join, or tool
/// result, so a write that addresses one is a typed conflict instead of an
/// append to a closed run.
fn require_active_run(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<RunProjectionDto> {
    let run = load_scoped_run(connection, session_id, run_id)?;
    if run_status_is_terminal(run.status()) {
        return Err(run_already_terminal());
    }
    Ok(run)
}

/// Loads the committed terminal outcome of one run for an idempotent replay.
///
/// The whole outcome is decoded, so a replay is compared against the recorded
/// evidence and not against its status alone.
fn load_run_outcome(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<RunOutcomeDto> {
    let (status, usage_json, finish_reason, error_code, error_message) = connection
        .query_row(
            "SELECT status, usage_json, finish_reason, error_code, error_message FROM runs \
             WHERE session_id=?1 AND id=?2",
            sqlite::params![session_id.to_string(), run_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .map_err(storage_error)?;
    RunOutcomeDto::new(
        RunStatusDto::parse(&status)?,
        usage_json
            .as_deref()
            .map(serde_json::from_str::<UsageDto>)
            .transpose()
            .map_err(codec_error)?,
        finish_reason
            .as_deref()
            .map(parse_finish_reason)
            .transpose()?,
        error_code,
        error_message,
    )
    .map_err(|_| codec_error("the committed run outcome is not a declared terminal outcome"))
}

/// Loads the committed user message that started one run.
fn first_user_message(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<MessageProjectionDto> {
    let id: Option<i64> = connection
        .query_row(
            "SELECT MIN(id) FROM messages WHERE session_id=?1 AND run_id=?2 AND kind='user'",
            sqlite::params![session_id.to_string(), run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let id = id.ok_or_else(|| codec_error("started turn has no committed user message"))?;
    let row = connection
        .query_row(
            &format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id=?1"),
            [id],
            raw_message_row,
        )
        .map_err(storage_error)?;
    let message = decode_message(row)?;
    if message.session_id() != session_id || message.run_id() != Some(run_id) {
        return Err(codec_error(
            "the committed user message is not scoped to its run",
        ));
    }
    Ok(message)
}

/// Loads every session transcript row up to and including one run's starting user turn.
fn starting_context_messages(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<Vec<MessageProjectionDto>> {
    let last_id: Option<i64> = connection
        .query_row(
            "SELECT MIN(id) FROM messages WHERE session_id=?1 AND run_id=?2 AND kind='user'",
            sqlite::params![session_id.to_string(), run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let Some(last_id) = last_id else {
        return Err(run_model_context_unavailable());
    };
    let mut statement = connection
        .prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE session_id=?1 AND id<=?2 ORDER BY id"
        ))
        .map_err(storage_error)?;
    let rows = statement
        .query_map(
            sqlite::params![session_id.to_string(), last_id],
            raw_message_row,
        )
        .map_err(storage_error)?;
    rows.map(|row| {
        let row = row.map_err(storage_error)?;
        decode_message(row).map_err(|_| run_model_context_unavailable())
    })
    .collect()
}

/// Loads the most recent committed transcript rows of one session in insertion
/// order.
///
/// The session must exist, and the limit must be able to select a row: both are
/// the read's own preconditions, shared by every caller that reads the session
/// transcript.
fn recent_messages(
    connection: &sqlite::Connection,
    session_id: SessionId,
    limit: u32,
) -> DtoResult<Vec<MessageProjectionDto>> {
    validate_message_limit(limit)?;
    SqliteStorageRepository::require_session(connection, session_id)?;
    let mut statement = connection
        .prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE session_id=?1 ORDER BY id DESC LIMIT ?2"
        ))
        .map_err(storage_error)?;
    let rows = statement
        .query_map(
            sqlite::params![session_id.to_string(), i64::from(limit)],
            raw_message_row,
        )
        .map_err(storage_error)?;
    let mut messages = rows
        .map(|row| {
            let row = row.map_err(storage_error)?;
            decode_message(row)
        })
        .collect::<DtoResult<Vec<_>>>()?;
    messages.reverse();
    Ok(messages)
}

/// Loads the persisted credential-free revision of one configuration identity.
fn load_config_snapshot(
    connection: &sqlite::Connection,
    revision_id: ConfigRevisionId,
) -> DtoResult<ConfigSnapshotDto> {
    let encoded: String = connection
        .query_row(
            "SELECT snapshot_json FROM configuration_revisions WHERE id=?1",
            [revision_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(run_model_context_unavailable)?;
    let snapshot: ConfigSnapshotDto =
        serde_json::from_str(&encoded).map_err(|_| run_model_context_unavailable())?;
    snapshot
        .validate_for_persistence()
        .map_err(|_| run_model_context_unavailable())?;
    Ok(snapshot)
}

/// Returns every pending turn of one session in insertion order.
///
/// The admission path joins every queued turn, so this read is deliberately
/// unbounded; the projection read uses [`bounded_pending_turns`] instead.
fn pending_turns(
    connection: &sqlite::Connection,
    session_id: SessionId,
) -> DtoResult<Vec<PendingTurnProjectionDto>> {
    let mut statement = connection
        .prepare(
            "SELECT id, content FROM turns WHERE session_id=?1 AND state='pending' ORDER BY rowid",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([session_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?;
    rows.map(|row| {
        let (turn, content) = row.map_err(storage_error)?;
        PendingTurnProjectionDto::new(
            session_id,
            TurnId::parse(&turn).map_err(codec_error)?,
            content,
        )
    })
    .collect()
}

/// Returns the newest pending turns of one session that fit the projection
/// bound, in insertion order, together with the count of older turns left out.
///
/// The session projection is bounded so a large queued turn can never make the
/// snapshot permanently unrepresentable; the durable rows are untouched, and
/// the omitted count is what tells a reader that older turns are still queued.
fn bounded_pending_turns(
    connection: &sqlite::Connection,
    session_id: SessionId,
) -> DtoResult<(Vec<PendingTurnProjectionDto>, u32)> {
    let mut statement = connection
        .prepare(
            "SELECT id, content FROM turns WHERE session_id=?1 AND state='pending' \
             ORDER BY rowid DESC",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([session_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?;
    let mut kept = Vec::new();
    let mut omitted = 0_u32;
    let mut budget = MAX_PENDING_TURN_PROJECTION_BYTES;
    let mut full = false;
    for row in rows {
        let (turn, content) = row.map_err(storage_error)?;
        if full {
            omitted = omitted.saturating_add(1);
            continue;
        }
        let turn = PendingTurnProjectionDto::new(
            session_id,
            TurnId::parse(&turn).map_err(codec_error)?,
            content,
        )?;
        if turn.content().len() > budget {
            full = true;
            omitted = omitted.saturating_add(1);
            continue;
        }
        budget -= turn.content().len();
        kept.push(turn);
    }
    kept.reverse();
    Ok((kept, omitted))
}

/// Returns the oldest pending turn of one session with its durable run selection.
fn oldest_pending_turn(
    connection: &sqlite::Connection,
    session_id: SessionId,
) -> DtoResult<Option<(TurnId, RunId, ConfigRevisionId, String)>> {
    connection
        .query_row(
            "SELECT id, proposed_run_id, config_revision_id, content FROM turns \
             WHERE session_id=?1 AND state='pending' ORDER BY rowid LIMIT 1",
            [session_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .map(|(turn, run, revision, content)| {
            Ok((
                TurnId::parse(&turn).map_err(codec_error)?,
                RunId::parse(&run).map_err(codec_error)?,
                ConfigRevisionId::parse(&revision).map_err(codec_error)?,
                content,
            ))
        })
        .transpose()
}

/// Rejects a message read limit that cannot select a committed row.
fn validate_message_limit(limit: u32) -> DtoResult<()> {
    if limit == 0 {
        return Err(ErrorDto::validation(
            "invalid_message_limit",
            "a message read limit must select at least one committed row",
        ));
    }
    Ok(())
}

fn run_projection(
    session: SessionId,
    run: &str,
    turn: &str,
    status: &str,
    revision: &str,
    provider_profile_id: Option<&str>,
) -> DtoResult<RunProjectionDto> {
    let projection = RunProjectionDto::new(
        session,
        RunId::parse(run).map_err(codec_error)?,
        TurnId::parse(turn).map_err(codec_error)?,
        RunStatusDto::parse(status)?,
        ConfigRevisionId::parse(revision).map_err(codec_error)?,
    );
    let provider_profile_id = provider_profile_id
        .map(ProviderProfileId::parse)
        .transpose()
        .map_err(codec_error)?;
    Ok(with_run_provider_profile(projection, provider_profile_id))
}

/// Applies the durable session provider default and projection revision to a
/// session projection read from its committed columns.
fn with_session_provider_state(
    projection: SessionProjectionDto,
    provider_profile_id: Option<ProviderProfileId>,
    session_projection_revision: u64,
) -> SessionProjectionDto {
    let projection = projection.with_session_projection_revision(session_projection_revision);
    match provider_profile_id {
        Some(profile_id) => projection.with_session_provider_profile(profile_id),
        None => projection,
    }
}

/// Applies the run's committed provider profile identity to a run projection.
fn with_run_provider_profile(
    projection: RunProjectionDto,
    provider_profile_id: Option<ProviderProfileId>,
) -> RunProjectionDto {
    projection.with_provider_profile_id(provider_profile_id)
}

const fn finish_reason_name(value: FinishReasonDto) -> &'static str {
    match value {
        FinishReasonDto::Stop => "stop",
        FinishReasonDto::Length => "length",
        FinishReasonDto::ToolCalls => "tool_calls",
        FinishReasonDto::ContentFilter => "content_filter",
        FinishReasonDto::Error => "error",
        FinishReasonDto::Unknown => "unknown",
    }
}

/// Parses the canonical durable string representation of a provider finish
/// reason, the inverse of [`finish_reason_name`].
///
/// # Errors
///
/// Returns a decode failure when `value` is not a declared durable finish reason.
fn parse_finish_reason(value: &str) -> DtoResult<FinishReasonDto> {
    match value {
        "stop" => Ok(FinishReasonDto::Stop),
        "length" => Ok(FinishReasonDto::Length),
        "tool_calls" => Ok(FinishReasonDto::ToolCalls),
        "content_filter" => Ok(FinishReasonDto::ContentFilter),
        "error" => Ok(FinishReasonDto::Error),
        "unknown" => Ok(FinishReasonDto::Unknown),
        _ => Err(codec_error("the durable finish reason is not declared")),
    }
}

fn run_configuration_unavailable() -> ErrorDto {
    ErrorDto::new(
        "run_configuration_unavailable",
        ErrorCategoryDto::Unavailable,
        "the durable run configuration is unavailable",
        ErrorRetryDto::Manual,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn run_model_context_unavailable() -> ErrorDto {
    ErrorDto::new(
        "run_model_context_unavailable",
        ErrorCategoryDto::Unavailable,
        "the durable run model context is unavailable",
        ErrorRetryDto::Manual,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn run_configuration_not_found() -> ErrorDto {
    ErrorDto::new(
        "run_configuration_not_found",
        ErrorCategoryDto::NotFound,
        "the requested durable run configuration does not exist",
        ErrorRetryDto::Never,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn config_revision_conflict() -> ErrorDto {
    conflict(
        "config_revision_conflict",
        "the configuration revision is already bound to different safe configuration",
    )
}

fn turn_idempotency_conflict() -> ErrorDto {
    conflict(
        "turn_idempotency_conflict",
        "the turn idempotency key is already bound to different durable content",
    )
}

fn turn_identity_conflict() -> ErrorDto {
    conflict(
        "turn_identity_conflict",
        "the durable run identity is already bound to another turn",
    )
}

fn run_outcome_conflict() -> ErrorDto {
    conflict(
        "run_outcome_conflict",
        "the run already holds a different terminal outcome",
    )
}

fn run_already_terminal() -> ErrorDto {
    conflict(
        "run_already_terminal",
        "the run holds a terminal status and accepts no further writes",
    )
}

fn pending_turn_not_found() -> ErrorDto {
    not_found(
        "pending_turn_not_found",
        "the requested pending turn does not exist",
    )
}

fn record_not_found() -> ErrorDto {
    not_found(
        "storage_record_not_found",
        "the requested durable record does not exist",
    )
}

fn tool_result_conflict() -> ErrorDto {
    conflict(
        "tool_result_conflict",
        "a durable tool result already exists for the call identity",
    )
}

fn tool_result_unavailable() -> ErrorDto {
    ErrorDto::new(
        "tool_result_unavailable",
        ErrorCategoryDto::Unavailable,
        "the durable tool result is unavailable",
        ErrorRetryDto::Manual,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn kind_immutable_mismatch() -> ErrorDto {
    conflict(
        "provider_kind_immutable_mismatch",
        "a provider kind identity cannot change the declaration it was accepted with",
    )
}

fn kind_has_dependents() -> ErrorDto {
    conflict(
        "provider_kind_has_dependents",
        "a provider kind with dependent profiles cannot be removed",
    )
}

fn profile_revision_mismatch() -> ErrorDto {
    conflict(
        "provider_profile_revision_mismatch",
        "a profile revision identity is already bound to different revision meaning",
    )
}

fn catalog_revision_conflict() -> ErrorDto {
    conflict(
        "provider_catalog_revision_conflict",
        "the catalog revision identity is already bound to different membership",
    )
}

fn catalog_revision_not_found() -> ErrorDto {
    not_found(
        "provider_catalog_revision_not_found",
        "the requested catalog revision does not exist",
    )
}

fn provider_catalog_changed() -> ErrorDto {
    conflict(
        "provider_catalog_changed",
        "the addressed catalog revision is not the accepted one",
    )
}

fn provider_profile_not_found() -> ErrorDto {
    not_found(
        "provider_profile_not_found",
        "the requested provider profile is not a member of the accepted catalog",
    )
}

fn session_projection_revision_conflict() -> ErrorDto {
    conflict(
        "session_projection_revision_conflict",
        "the expected session projection revision does not match the durable one",
    )
}

fn run_provider_selection_not_found() -> ErrorDto {
    not_found(
        "run_provider_selection_not_found",
        "the requested run has no committed provider selection",
    )
}

fn discovery_attempt_not_found() -> ErrorDto {
    not_found(
        "discovery_attempt_not_found",
        "the requested discovery attempt does not exist",
    )
}

fn discovery_attempt_conflict(message: &'static str) -> ErrorDto {
    conflict("discovery_attempt_conflict", message)
}

fn discovery_attempt_state_conflict() -> ErrorDto {
    conflict(
        "discovery_attempt_state_conflict",
        "the discovery attempt does not accept this transition",
    )
}

fn unavailable() -> ErrorDto {
    ErrorDto::unavailable(
        "storage_unavailable",
        "the local durable storage is unavailable",
    )
}

/// Returns transient unavailability for a backend failure that cannot be
/// classified more precisely; retrying after a delay may succeed.
fn storage_backend_unavailable() -> ErrorDto {
    ErrorDto::unavailable_delayed(
        "storage_unavailable",
        "the local durable storage is unavailable",
    )
}

/// Classifies one raw SQLite failure into a distinct safe typed error.
///
/// Classes and their retry guidance, by SQLite primary result code:
/// - `busy`/`locked`: another writer holds the database file, so retrying
///   immediately can succeed -> `storage_busy` (Unavailable, Immediate).
/// - `corrupt`/`not a database`: the file image cannot be trusted and retrying
///   cannot repair it -> `storage_corrupt` (Internal, Never).
/// - constraint violation: the write breaks a declared uniqueness,
///   primary-key, or not-null rule that the repository pre-checks for every
///   expected case, so reaching here means durable state disagrees with the
///   request -> `storage_constraint_violation` (Conflict, Never).
/// - type mismatch: a bound value does not fit its stored column, which is
///   always a defect of this backend's own binding rather than caller input ->
///   `storage_type_mismatch` (Internal, Never).
/// - API misuse: the driver was driven incorrectly -> `storage_api_misuse`
///   (Internal, Never).
/// - engine, schema, and limit codes that no retry can change
///   (`internal`, `schema-changed`, `too-big`, `parameter-out-of-range`,
///   `read-only`, `permission-denied`, `not-found`, `abort`,
///   `interrupt`, `protocol`, `no-large-file-support`, `auth`): a permanent
///   backend defect -> `storage_internal_fault` (Internal, Never).
/// - disk-full, I/O, out-of-memory, cannot-open, and every otherwise
///   unclassified engine failure: the underlying resource is not usable now ->
///   `storage_unavailable` (Unavailable, Delayed).
///
/// A missing expected row is `storage_record_not_found`, and any driver-side
/// failure that is not a SQLite result (row decoding, parameter binding,
/// statement usage) is `storage_internal_fault` (Internal, Never). Only the
/// class crosses the boundary: SQL text, connection paths, bound values, row
/// content, and the SQLite message are never carried.
fn storage_error(error: sqlite::Error) -> ErrorDto {
    match error {
        sqlite::Error::SqliteFailure(failure, _) => sqlite_code_error(failure.code),
        sqlite::Error::QueryReturnedNoRows => record_not_found(),
        _ => storage_internal_fault(),
    }
}

/// Maps one SQLite primary result code to its safe typed error.
fn sqlite_code_error(code: sqlite::ErrorCode) -> ErrorDto {
    match code {
        sqlite::ErrorCode::DatabaseBusy | sqlite::ErrorCode::DatabaseLocked => ErrorDto::new(
            "storage_busy",
            ErrorCategoryDto::Unavailable,
            "the local durable storage is busy",
            ErrorRetryDto::Immediate,
            None,
        )
        .unwrap_or_else(|_| unavailable()),
        sqlite::ErrorCode::DatabaseCorrupt | sqlite::ErrorCode::NotADatabase => ErrorDto::new(
            "storage_corrupt",
            ErrorCategoryDto::Internal,
            "the local durable storage is corrupt",
            ErrorRetryDto::Never,
            None,
        )
        .unwrap_or_else(|_| unavailable()),
        sqlite::ErrorCode::ConstraintViolation => ErrorDto::new(
            "storage_constraint_violation",
            ErrorCategoryDto::Conflict,
            "the durable write violates a storage integrity constraint",
            ErrorRetryDto::Never,
            None,
        )
        .unwrap_or_else(|_| unavailable()),
        sqlite::ErrorCode::TypeMismatch => ErrorDto::new(
            "storage_type_mismatch",
            ErrorCategoryDto::Internal,
            "the durable write supplies a value that does not fit its stored type",
            ErrorRetryDto::Never,
            None,
        )
        .unwrap_or_else(|_| unavailable()),
        sqlite::ErrorCode::ApiMisuse => ErrorDto::new(
            "storage_api_misuse",
            ErrorCategoryDto::Internal,
            "the local durable storage was used incorrectly",
            ErrorRetryDto::Never,
            None,
        )
        .unwrap_or_else(|_| unavailable()),
        // Permanent backend defects: every one of these describes a storage
        // engine or schema state that no retry can change, so they must not
        // masquerade as transient unavailability.
        sqlite::ErrorCode::InternalMalfunction
        | sqlite::ErrorCode::SchemaChanged
        | sqlite::ErrorCode::TooBig
        | sqlite::ErrorCode::ParameterOutOfRange
        | sqlite::ErrorCode::ReadOnly
        | sqlite::ErrorCode::PermissionDenied
        | sqlite::ErrorCode::NotFound
        | sqlite::ErrorCode::OperationAborted
        | sqlite::ErrorCode::OperationInterrupted
        | sqlite::ErrorCode::FileLockingProtocolFailed
        | sqlite::ErrorCode::NoLargeFileSupport
        | sqlite::ErrorCode::AuthorizationForStatementDenied => storage_internal_fault(),
        _ => storage_backend_unavailable(),
    }
}

/// Returns the permanent internal error for a driver-side failure that no
/// retry can change.
fn storage_internal_fault() -> ErrorDto {
    ErrorDto::new(
        "storage_internal_fault",
        ErrorCategoryDto::Internal,
        "an unexpected local durable storage failure occurred",
        ErrorRetryDto::Never,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn codec_error(_: impl std::fmt::Display) -> ErrorDto {
    ErrorDto::new(
        "storage_decode_failed",
        ErrorCategoryDto::Internal,
        "the local durable storage contains unsupported data",
        ErrorRetryDto::Never,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn not_found_or_storage(error: sqlite::Error) -> ErrorDto {
    if matches!(error, sqlite::Error::QueryReturnedNoRows) {
        record_not_found()
    } else {
        storage_error(error)
    }
}

fn not_found(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::new(
        code,
        ErrorCategoryDto::NotFound,
        message,
        ErrorRetryDto::Never,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn conflict(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::new(
        code,
        ErrorCategoryDto::Conflict,
        message,
        ErrorRetryDto::Never,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

/// A deterministic test-only write stage whose injected failure must roll back
/// the whole transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultPoint {
    /// The stage that commits an accepted turn, its run, and its first message.
    TurnAcceptance,
    /// The stage that commits one appended transcript row.
    Message,
    /// The stage that commits one terminal run outcome.
    RunOutcome,
    /// The stage that commits one tool result with its transcript row.
    ToolResult,
    /// The stage that commits one configuration revision.
    ConfigRevision,
    /// The stage that commits one accepted provider catalog revision.
    CatalogAcceptance,
    /// The stage that commits one catalog activation.
    CatalogActivation,
    /// The stage that commits one rejected catalog candidate.
    CatalogRejection,
    /// The stage that commits one current profile policy.
    ProfilePolicy,
    /// The stage that commits one session provider-profile change.
    SessionProviderProfile,
    /// The stage that commits one discovery attempt transition.
    DiscoveryAttempt,
    /// The stage that commits discovery-attempt recovery.
    DiscoveryRecovery,
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Focused SQLite fixtures use expect for test diagnostics."
    )]
    use super::*;
    use crate::StorageRepositoryDto;
    use intention_config::ContextWindowPolicyDto;
    use intention_proto::provider::{
        ContextPreservationCapabilityDto, CredentialTransportContractDto, CredentialTransportDto,
        LoopbackPolicyDto, ModelCapabilitySetV1, ModelCapabilityTaxonomyVersionDto,
        ModelInputKindDto, ProviderCapabilityAvailabilityDto, ProviderDriverContractRevisionDto,
        ProviderEndpointPolicyDto, ProviderExecutionPolicyDto, ProviderKindId,
        ProviderProfileRevisionId, ProviderSelectionSourceDto, ReasoningCapabilityDto,
        ReasoningEffortLevelDto, ReasoningFragmentCategoryDto, ReasoningHistoryManifestId,
        ReasoningHistoryRecordReferenceDto, ReasoningHistorySourceEntryDto,
        ReasoningHistoryTransferDto, ResolvedReasoningPolicyDto, ToolExchangeCapabilityDto,
    };
    use intention_proto::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
    use intention_proto::{IdempotencyKey, ProjectId, SchemaVersionDto, UsageDto, WorkspaceId};

    fn fixture_time(value: i64) -> TimestampDto {
        TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
    }

    fn fixture_transfer() -> ReasoningHistoryTransferDto {
        ReasoningHistoryTransferDto::textual_history_v1("fixture-compatibility-v1")
            .expect("fixture transfer contract is valid")
    }

    fn fixture_capability_subset() -> ModelCapabilitySetV1 {
        ModelCapabilitySetV1::new(
            ModelCapabilityTaxonomyVersionDto::current(),
            ModelInputKindDto::TextOnly,
            ProviderCapabilityAvailabilityDto::Enabled,
            ProviderCapabilityAvailabilityDto::Disabled,
            ReasoningCapabilityDto::textual_reasoning_v1(
                vec![ReasoningEffortLevelDto::Medium],
                true,
            )
            .expect("fixture reasoning capability is valid"),
            ToolExchangeCapabilityDto::model_tool_loop_v1("fixture-tool-loop-v1")
                .expect("fixture tool loop is valid"),
            ContextPreservationCapabilityDto::local_durable_history_v1(fixture_transfer()),
        )
        .expect("fixture capability subset is valid")
    }

    fn fixture_reasoning_policy() -> ResolvedReasoningPolicyDto {
        ResolvedReasoningPolicyDto::new(
            Some(ReasoningEffortLevelDto::Medium),
            true,
            fixture_transfer(),
            vec![ReasoningFragmentCategoryDto::Primary],
        )
        .expect("fixture reasoning policy is valid")
    }

    fn fixture_profile_id() -> ProviderProfileId {
        ProviderProfileId::parse("main").expect("fixture profile identity is valid")
    }

    fn fixture_kind_id() -> ProviderKindId {
        ProviderKindId::parse("openrouter").expect("fixture kind identity is valid")
    }

    fn fixture_profile_revision() -> ProviderProfileRevisionV1 {
        ProviderProfileRevisionV1::new(
            fixture_profile_id(),
            ProviderProfileRevisionId::new(),
            fixture_kind_id(),
            ProviderKindDescriptorRevisionId::new(),
            "fixture-model",
            None,
            CredentialTransportDto::bearer(),
            fixture_capability_subset(),
            fixture_reasoning_policy(),
            ProviderExecutionPolicyDto::new(30, 2).expect("fixture execution policy is valid"),
            LoopbackPolicyDto::NotApplicable,
        )
        .expect("fixture profile revision is valid")
    }

    fn fixture_selection() -> ResolvedRunProviderSelectionDto {
        ResolvedRunProviderSelectionDto::from_profile_revision(
            &fixture_profile_revision(),
            ProviderDriverContractRevisionDto::new("fixture-driver", 1, 0)
                .expect("fixture driver contract is valid"),
            ProviderSelectionSourceDto::GlobalDefault,
        )
    }

    fn fixture_kind_descriptor(
        descriptor_revision_id: ProviderKindDescriptorRevisionId,
    ) -> ProviderKindDescriptorRevisionV1 {
        ProviderKindDescriptorRevisionV1::new(
            fixture_kind_id(),
            descriptor_revision_id,
            "fixture-family",
            vec!["fixture-part-v1".to_owned()],
            ProviderEndpointPolicyDto::new(false, true, false),
            CredentialTransportContractDto::new(vec![CredentialTransportModeDto::Bearer], None)
                .expect("fixture credential transport contract is valid"),
            fixture_capability_subset(),
            "fixture-driver",
        )
        .expect("fixture kind descriptor revision is valid")
    }

    fn fixture_profile_revision_with_descriptor(
        descriptor_revision_id: ProviderKindDescriptorRevisionId,
    ) -> ProviderProfileRevisionV1 {
        ProviderProfileRevisionV1::new(
            fixture_profile_id(),
            ProviderProfileRevisionId::new(),
            fixture_kind_id(),
            descriptor_revision_id,
            "fixture-model",
            None,
            CredentialTransportDto::bearer(),
            fixture_capability_subset(),
            fixture_reasoning_policy(),
            ProviderExecutionPolicyDto::new(30, 2).expect("fixture execution policy is valid"),
            LoopbackPolicyDto::NotApplicable,
        )
        .expect("fixture profile revision is valid")
    }

    fn fixture_catalog_revision() -> ProviderCatalogRevisionDto {
        let descriptor_revision_id = ProviderKindDescriptorRevisionId::new();
        ProviderCatalogRevisionDto::new(
            CatalogRevisionId::new(),
            Some(fixture_profile_id()),
            fixture_time(2),
            vec![fixture_kind_descriptor(descriptor_revision_id)],
            vec![fixture_profile_revision_with_descriptor(
                descriptor_revision_id,
            )],
            vec![ProviderProfilePolicyEntryDto::new(
                fixture_profile_id(),
                ProviderProfilePolicyDto::new("Fixture Profile", true, None)
                    .expect("fixture profile policy is valid"),
            )],
        )
        .expect("fixture catalog revision is valid")
    }

    fn fixture_failure() -> ProviderDiscoveryFailureDto {
        ProviderDiscoveryFailureDto::new("provider_unavailable", "safe failure")
            .expect("fixture failure is valid")
    }

    fn fixture_manifest(session_id: SessionId) -> ReasoningHistoryManifestDto {
        ReasoningHistoryManifestDto::new(
            ReasoningHistoryManifestId::new(),
            fixture_transfer(),
            Some("fixture-compatibility-v1".to_owned()),
            vec![ReasoningHistorySourceEntryDto::new(
                session_id,
                RunId::new(),
                Some(1),
                vec![
                    ReasoningHistoryRecordReferenceDto::new(
                        ReasoningFragmentCategoryDto::Primary,
                        5,
                    )
                    .expect("fixture record reference is valid"),
                ],
            )],
            5,
        )
        .expect("fixture manifest is valid")
    }

    fn fixture_snapshot() -> ConfigSnapshotDto {
        ConfigSnapshotDto::new(
            SchemaVersionDto::new(1, 0),
            ConfigRevisionId::new(),
            fixture_time(1),
            ContextWindowPolicyDto::default_policy(),
        )
        .expect("fixture snapshot is valid")
    }

    fn fixture_location() -> SqliteDatabaseLocationDto {
        SqliteDatabaseLocationDto::new(format!(
            "{}/intention-storage-fault-{}.db",
            std::env::temp_dir().display(),
            TurnId::new()
        ))
        .expect("temporary location is absolute")
    }

    fn fixture_workspace_root() -> WorkspaceRootDto {
        WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-storage-unit-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("native fixture workspace is absolute")
    }

    fn create_fixture_session(repository: &SqliteStorageRepository) -> SessionId {
        let session_id = SessionId::new();
        repository
            .create_session(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    session_id,
                    WorkspaceId::new(),
                    fixture_workspace_root(),
                    RunModeDto::Build,
                ),
                fixture_time(1),
            )
            .expect("fixture session creates");
        session_id
    }

    fn accept_fixture_turn(
        repository: &SqliteStorageRepository,
        session_id: SessionId,
        content: &str,
    ) -> RunProjectionDto {
        let run_id = RunId::new();
        match repository
            .accept_user_turn(
                session_id,
                IdempotencyKey::new(),
                content,
                run_id,
                fixture_snapshot(),
                fixture_selection(),
                None,
                fixture_time(2),
            )
            .expect("fixture turn commits")
        {
            AcceptedTurnOutcomeDto::Started { run, .. } => {
                assert_eq!(run.run_id(), run_id);
                run
            }
            AcceptedTurnOutcomeDto::Pending(_) => {
                unreachable!("the first accepted turn starts a run")
            }
        }
    }

    fn raw_count(location: &SqliteDatabaseLocationDto, table: &str) -> i64 {
        let connection =
            sqlite::Connection::open(&location.0).expect("database reopens for inspection");
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count query executes")
    }

    fn raw_discovery_attempt(
        location: &SqliteDatabaseLocationDto,
        attempt_id: ProviderDiscoveryAttemptId,
    ) -> (String, Option<i64>, Option<i64>, Option<String>) {
        let connection =
            sqlite::Connection::open(&location.0).expect("database reopens for inspection");
        connection
            .query_row(
                "SELECT state, started_at, terminated_at, failure_code \
                 FROM provider_discovery_attempts WHERE id=?1",
                sqlite::params![attempt_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("the attempt row reads")
    }

    fn raw_json_columns(location: &SqliteDatabaseLocationDto) -> Vec<String> {
        let connection =
            sqlite::Connection::open(&location.0).expect("database reopens for inspection");
        [
            "SELECT snapshot_json FROM configuration_revisions",
            "SELECT metadata_json FROM tool_results",
            "SELECT COALESCE(usage_json, '') FROM runs",
            "SELECT text FROM messages",
            "SELECT COALESCE(reasoning, '') FROM messages",
            "SELECT body FROM provider_kind_descriptors",
            "SELECT body FROM provider_profile_revisions",
            "SELECT display_name FROM provider_profile_policies",
            "SELECT record_kind FROM provider_catalog_audit",
            "SELECT model_id FROM run_provider_selections",
            "SELECT COALESCE(normalized_effective_endpoint, '') FROM run_provider_selections",
            "SELECT credential_transport_mode FROM run_provider_selections",
            "SELECT COALESCE(credential_transport_safe_header_name, '') FROM run_provider_selections",
            "SELECT declared_model_capability_subset FROM run_provider_selections",
            "SELECT resolved_reasoning_policy FROM run_provider_selections",
            "SELECT effective_execution_policy FROM run_provider_selections",
            "SELECT effective_loopback_policy FROM run_provider_selections",
            "SELECT provider_driver_contract_revision FROM run_provider_selections",
            "SELECT transfer FROM reasoning_history_manifests",
            "SELECT COALESCE(compatibility_id, '') FROM reasoning_history_manifests",
            "SELECT COALESCE(category, '') FROM reasoning_history_manifest_entries",
        ]
        .into_iter()
        .flat_map(|query| {
            let mut statement = connection
                .prepare(query)
                .expect("JSON inspection query prepares");
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("JSON inspection query executes")
                .map(|row| row.expect("JSON inspection row reads"))
                .collect::<Vec<_>>()
        })
        .collect()
    }

    #[test]
    fn tool_result_status_pins_its_canonical_durable_strings() {
        assert_eq!(ToolResultStatusDto::Completed.as_str(), "completed");
        assert_eq!(ToolResultStatusDto::Failed.as_str(), "failed");
        assert_eq!(ToolResultStatusDto::Partial.as_str(), "partial");
        for status in [
            ToolResultStatusDto::Completed,
            ToolResultStatusDto::Failed,
            ToolResultStatusDto::Partial,
        ] {
            assert_eq!(
                ToolResultStatusDto::parse(status.as_str())
                    .expect("known tool result status parses"),
                status
            );
        }
        assert_eq!(
            ToolResultStatusDto::parse("invalid")
                .expect_err("unknown tool result status rejects")
                .code(),
            "invalid_tool_result_status"
        );
        assert_eq!(tool_result_unavailable().code(), "tool_result_unavailable");
    }

    #[test]
    fn sqlite_failures_map_to_distinct_safe_typed_errors() {
        for (code, expected) in [
            (
                sqlite::ErrorCode::DatabaseBusy,
                (
                    "storage_busy",
                    ErrorCategoryDto::Unavailable,
                    ErrorRetryDto::Immediate,
                ),
            ),
            (
                sqlite::ErrorCode::DatabaseLocked,
                (
                    "storage_busy",
                    ErrorCategoryDto::Unavailable,
                    ErrorRetryDto::Immediate,
                ),
            ),
            (
                sqlite::ErrorCode::DatabaseCorrupt,
                (
                    "storage_corrupt",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::NotADatabase,
                (
                    "storage_corrupt",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::ConstraintViolation,
                (
                    "storage_constraint_violation",
                    ErrorCategoryDto::Conflict,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::TypeMismatch,
                (
                    "storage_type_mismatch",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::ApiMisuse,
                (
                    "storage_api_misuse",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::InternalMalfunction,
                (
                    "storage_internal_fault",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::SchemaChanged,
                (
                    "storage_internal_fault",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::TooBig,
                (
                    "storage_internal_fault",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::ParameterOutOfRange,
                (
                    "storage_internal_fault",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::ReadOnly,
                (
                    "storage_internal_fault",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::PermissionDenied,
                (
                    "storage_internal_fault",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::OperationAborted,
                (
                    "storage_internal_fault",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::OperationInterrupted,
                (
                    "storage_internal_fault",
                    ErrorCategoryDto::Internal,
                    ErrorRetryDto::Never,
                ),
            ),
            (
                sqlite::ErrorCode::DiskFull,
                (
                    "storage_unavailable",
                    ErrorCategoryDto::Unavailable,
                    ErrorRetryDto::Delayed,
                ),
            ),
            (
                sqlite::ErrorCode::SystemIoFailure,
                (
                    "storage_unavailable",
                    ErrorCategoryDto::Unavailable,
                    ErrorRetryDto::Delayed,
                ),
            ),
            (
                sqlite::ErrorCode::CannotOpen,
                (
                    "storage_unavailable",
                    ErrorCategoryDto::Unavailable,
                    ErrorRetryDto::Delayed,
                ),
            ),
            (
                sqlite::ErrorCode::Unknown,
                (
                    "storage_unavailable",
                    ErrorCategoryDto::Unavailable,
                    ErrorRetryDto::Delayed,
                ),
            ),
        ] {
            let error = sqlite_code_error(code);
            assert_eq!((error.code(), error.category(), error.retry()), expected);
        }
        assert_eq!(
            storage_error(sqlite::Error::QueryReturnedNoRows).code(),
            "storage_record_not_found"
        );
        assert_eq!(
            storage_error(sqlite::Error::ExecuteReturnedResults).code(),
            "storage_internal_fault"
        );
        // The SQLite message text is classified, never projected.
        let classified = storage_error(sqlite::Error::SqliteFailure(
            sqlite::ffi::Error {
                code: sqlite::ErrorCode::ConstraintViolation,
                extended_code: 2067,
            },
            Some("UNIQUE constraint failed: turns.proposed_run_id".to_owned()),
        ));
        assert_eq!(classified.code(), "storage_constraint_violation");
        assert!(!classified.to_string().contains("UNIQUE"));
        assert!(!classified.to_string().contains("proposed_run_id"));
    }

    /// Pins the durable terminal-status literal to the proto terminal set, so a
    /// status added to one and not the other fails loudly instead of silently
    /// changing which runs the storage predicate treats as finished.
    #[test]
    fn durable_terminal_status_literal_matches_the_proto_terminal_set() {
        let mut literal = TERMINAL_STATUSES
            .split(',')
            .map(|value| value.trim().trim_matches('\''))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        literal.sort_unstable();
        let mut expected = RunStatusDto::TERMINAL
            .iter()
            .map(|status| status.as_str().to_owned())
            .collect::<Vec<_>>();
        expected.sort_unstable();
        assert_eq!(literal, expected);
    }

    #[test]
    fn finish_reason_mapping_round_trips_through_its_durable_strings() {
        for reason in [
            FinishReasonDto::Stop,
            FinishReasonDto::Length,
            FinishReasonDto::ToolCalls,
            FinishReasonDto::ContentFilter,
            FinishReasonDto::Error,
            FinishReasonDto::Unknown,
        ] {
            assert_eq!(
                parse_finish_reason(finish_reason_name(reason)).expect("declared reason parses"),
                reason
            );
        }
        assert_eq!(
            parse_finish_reason("not-a-reason")
                .expect_err("an undeclared finish reason rejects")
                .code(),
            "storage_decode_failed"
        );
    }

    #[test]
    fn location_is_absolute() {
        assert!(SqliteDatabaseLocationDto::new("relative.db").is_err());
        let location = format!(
            "{}/intention-storage-unit-{}.db",
            std::env::temp_dir().display(),
            TurnId::new()
        );
        SqliteStorageRepository::open(
            SqliteDatabaseLocationDto::new(location).expect("temp location is absolute"),
        )
        .expect("database opens");
    }

    #[test]
    fn turn_acceptance_fault_rolls_back_every_durable_row() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        repository.arm_fault(FaultPoint::TurnAcceptance);
        let error = repository
            .accept_user_turn(
                session_id,
                IdempotencyKey::new(),
                "atomic turn",
                RunId::new(),
                fixture_snapshot(),
                fixture_selection(),
                None,
                fixture_time(2),
            )
            .expect_err("injected acceptance fault aborts the transaction");
        assert_eq!(error.code(), "injected_storage_fault");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        let projection = reopened
            .load_session_projection(session_id)
            .expect("baseline projection loads");
        assert!(projection.active_run().is_none());
        assert!(projection.pending_turns().is_empty());
        drop(reopened);
        assert_eq!(raw_count(&location, "turns"), 0);
        assert_eq!(raw_count(&location, "runs"), 0);
        assert_eq!(raw_count(&location, "messages"), 0);
        // The revision recorded by the aborted acceptance rolls back with the
        // rest of the transaction.
        assert_eq!(raw_count(&location, "configuration_revisions"), 0);
    }

    #[test]
    fn queued_turn_acceptance_fault_rolls_back_the_pending_turn() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        let _active = accept_fixture_turn(&repository, session_id, "active");
        repository.arm_fault(FaultPoint::TurnAcceptance);
        let error = repository
            .accept_user_turn(
                session_id,
                IdempotencyKey::new(),
                "queued",
                RunId::new(),
                fixture_snapshot(),
                fixture_selection(),
                None,
                fixture_time(3),
            )
            .expect_err("injected acceptance fault aborts the queued transaction");
        assert_eq!(error.code(), "injected_storage_fault");
        drop(repository);
        assert_eq!(raw_count(&location, "turns"), 1);
        let reopened = SqliteStorageRepository::open(location).expect("database reopens");
        assert!(
            reopened
                .load_session_projection(session_id)
                .expect("baseline projection loads")
                .pending_turns()
                .is_empty()
        );
    }

    #[test]
    fn appended_message_fault_rolls_back_the_transcript_row() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        let run = accept_fixture_turn(&repository, session_id, "active");
        let message = MessageProjectionDto::new(
            session_id,
            Some(run.run_id()),
            MessageKindDto::Assistant,
            "atomic answer",
            Some("reasoning".to_owned()),
            None,
            None,
        )
        .expect("assistant row is valid");
        repository.arm_fault(FaultPoint::Message);
        let error = repository
            .append_message(message, fixture_time(3))
            .expect_err("injected message fault aborts the transaction");
        assert_eq!(error.code(), "injected_storage_fault");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        assert_eq!(raw_count(&location, "messages"), 1);
        assert!(
            reopened
                .load_run_messages(session_id, run.run_id(), 10)
                .expect("transcript loads")
                .iter()
                .all(|row| row.kind() != MessageKindDto::Assistant)
        );
    }

    #[test]
    fn terminal_run_outcome_fault_rolls_back_status_and_evidence() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        let run = accept_fixture_turn(&repository, session_id, "active");
        repository
            .transition_run(
                session_id,
                run.run_id(),
                RunStatusDto::Running,
                fixture_time(3),
            )
            .expect("run starts");
        repository.arm_fault(FaultPoint::RunOutcome);
        let error = repository
            .finish_run(
                session_id,
                run.run_id(),
                RunOutcomeDto::new(
                    RunStatusDto::Completed,
                    Some(UsageDto::reported(1, 2, 3).expect("fixture usage is consistent")),
                    Some(FinishReasonDto::Stop),
                    None,
                    None,
                )
                .expect("fixture outcome is valid"),
                fixture_time(4),
            )
            .expect_err("injected outcome fault aborts the transaction");
        assert_eq!(error.code(), "injected_storage_fault");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        assert_eq!(
            reopened
                .load_run_projection(session_id, run.run_id())
                .expect("run projection loads")
                .status(),
            RunStatusDto::Running
        );
        drop(reopened);
        let connection =
            sqlite::Connection::open(&location.0).expect("database reopens for inspection");
        let (usage, finished): (Option<String>, Option<i64>) = connection
            .query_row("SELECT usage_json, finished_at FROM runs", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("run evidence reads");
        assert!(usage.is_none());
        assert!(finished.is_none());
    }

    #[test]
    fn tool_result_fault_rolls_back_evidence_and_its_transcript_row() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        let run = accept_fixture_turn(&repository, session_id, "active");
        let call_id = ToolCallId::new();
        let evidence = ToolResultEvidenceDto::new(
            session_id,
            run.run_id(),
            call_id,
            "read",
            ToolResultStatusDto::Completed,
            r#"{"result":"read"}"#,
            vec![
                ToolResultMetadataEntryDto::new("truncated", "false")
                    .expect("fixture metadata is valid"),
            ],
            fixture_time(3),
        )
        .expect("fixture evidence is valid");
        let message = MessageProjectionDto::new(
            session_id,
            Some(run.run_id()),
            MessageKindDto::ToolResult,
            r#"{"result":"read"}"#,
            None,
            Some(call_id),
            Some("read".to_owned()),
        )
        .expect("answering row is valid");
        repository.arm_fault(FaultPoint::ToolResult);
        let error = repository
            .write_tool_result(evidence, message)
            .expect_err("injected tool result fault aborts the transaction");
        assert_eq!(error.code(), "injected_storage_fault");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        assert_eq!(raw_count(&location, "tool_results"), 0);
        assert_eq!(raw_count(&location, "messages"), 1);
        assert_eq!(
            reopened
                .load_tool_result(session_id, run.run_id(), call_id)
                .expect_err("evidence rolled back with its transaction")
                .code(),
            "tool_result_not_found"
        );
    }

    #[test]
    fn configuration_revision_fault_rolls_back_the_revision() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        let accepted = fixture_snapshot();
        let run = match repository
            .accept_user_turn(
                session_id,
                IdempotencyKey::new(),
                "active",
                RunId::new(),
                accepted.clone(),
                fixture_selection(),
                None,
                fixture_time(2),
            )
            .expect("fixture turn commits")
        {
            AcceptedTurnOutcomeDto::Started { run, .. } => run,
            AcceptedTurnOutcomeDto::Pending(_) => unreachable!("the first turn starts a run"),
        };
        repository.arm_fault(FaultPoint::ConfigRevision);
        repository
            .accept_configuration_revision(fixture_snapshot())
            .expect_err("injected revision fault aborts the transaction");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        assert_eq!(raw_count(&location, "configuration_revisions"), 1);
        assert_eq!(
            reopened
                .load_run_config_snapshot(session_id, run.run_id())
                .expect("the committed revision remains canonical"),
            accepted
        );
    }

    #[test]
    fn catalog_acceptance_fault_rolls_back_the_revision_audit_and_membership() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        repository.arm_fault(FaultPoint::CatalogAcceptance);
        let error = repository
            .accept_catalog_revision(fixture_catalog_revision())
            .expect_err("injected catalog fault aborts the transaction");
        assert_eq!(error.code(), "injected_storage_fault");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        assert_eq!(
            reopened.load_catalog_state().expect("state loads"),
            ProviderCatalogStateDto::new(None, None).expect("the empty state is coherent")
        );
        for table in [
            "provider_kind_revisions",
            "provider_kind_descriptors",
            "provider_profile_revisions",
            "provider_profile_policies",
            "provider_catalog_revisions",
            "provider_catalog_kinds",
            "provider_catalog_profiles",
            "provider_catalog_audit",
        ] {
            assert_eq!(raw_count(&location, table), 0, "{table} staged no row");
        }

        // A removing acceptance stages tombstones and removal audit records
        // that must roll back with the rest of the same transaction.
        let repository = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        let accepted = fixture_catalog_revision();
        let accepted_id = accepted.catalog_revision_id();
        repository
            .accept_catalog_revision(accepted)
            .expect("the first catalog accepts");
        repository.arm_fault(FaultPoint::CatalogAcceptance);
        repository
            .accept_catalog_revision(
                ProviderCatalogRevisionDto::new(
                    CatalogRevisionId::new(),
                    None,
                    fixture_time(3),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
                .expect("the empty removal candidate is coherent"),
            )
            .expect_err("injected removal fault aborts the transaction");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        assert_eq!(
            reopened
                .load_catalog_state()
                .expect("state loads")
                .accepted_catalog_revision_id(),
            Some(accepted_id)
        );
        assert_eq!(raw_count(&location, "provider_profile_tombstones"), 0);
        assert_eq!(raw_count(&location, "provider_kind_tombstones"), 0);
        assert_eq!(raw_count(&location, "provider_catalog_revisions"), 1);
        assert_eq!(raw_count(&location, "provider_catalog_audit"), 2);
    }

    #[test]
    fn catalog_activation_fault_rolls_back_the_activated_pointer() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let revision = fixture_catalog_revision();
        let catalog_revision_id = revision.catalog_revision_id();
        repository
            .accept_catalog_revision(revision)
            .expect("the fixture catalog accepts");
        repository.arm_fault(FaultPoint::CatalogActivation);
        let error = repository
            .mark_catalog_activated(catalog_revision_id, fixture_time(4))
            .expect_err("injected activation fault aborts the transaction");
        assert_eq!(error.code(), "injected_storage_fault");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        let state = reopened.load_catalog_state().expect("state loads");
        assert_eq!(
            state.accepted_catalog_revision_id(),
            Some(catalog_revision_id)
        );
        assert_eq!(state.activated_catalog_revision_id(), None);
        assert!(state.requires_activation_recovery());
        assert_eq!(
            raw_count(&location, "provider_catalog_audit"),
            2,
            "the activation audit row rolled back with the transaction"
        );
    }

    #[test]
    fn catalog_rejection_fault_rolls_back_the_rejection_audit() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        repository.arm_fault(FaultPoint::CatalogRejection);
        repository
            .record_catalog_candidate_rejected(CatalogRevisionId::new(), fixture_time(3))
            .expect_err("injected rejection fault aborts the transaction");
        drop(repository);
        assert_eq!(raw_count(&location, "provider_catalog_audit"), 0);
    }

    #[test]
    fn profile_policy_fault_rolls_back_the_current_policy() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let revision = fixture_catalog_revision();
        let catalog_revision_id = revision.catalog_revision_id();
        repository
            .accept_catalog_revision(revision)
            .expect("the fixture catalog accepts");
        repository.arm_fault(FaultPoint::ProfilePolicy);
        let error = repository
            .store_profile_policy(
                fixture_profile_id(),
                ProviderProfilePolicyDto::new("Renamed Profile", false, None)
                    .expect("fixture profile policy is valid"),
                fixture_time(4),
            )
            .expect_err("injected policy fault aborts the transaction");
        assert_eq!(error.code(), "injected_storage_fault");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location).expect("database reopens");
        let policy = reopened
            .load_catalog_revision(catalog_revision_id)
            .expect("the committed revision loads")
            .profile_policies()[0]
            .policy()
            .clone();
        assert_eq!(policy.display_name(), "Fixture Profile");
        assert!(policy.enabled());
    }

    #[test]
    fn session_provider_profile_fault_rolls_back_the_default_and_revision() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        repository.arm_fault(FaultPoint::SessionProviderProfile);
        repository
            .set_session_provider_profile(session_id, fixture_profile_id(), 0, fixture_time(3))
            .expect_err("injected session-profile fault aborts the transaction");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location).expect("database reopens");
        assert_eq!(
            reopened
                .load_session_provider_profile(session_id)
                .expect("the durable default loads"),
            SessionProviderProfileDto::new(session_id, None, 0)
        );
        assert_eq!(
            reopened
                .load_session_projection(session_id)
                .expect("the session projection loads")
                .session_projection_revision(),
            0
        );
    }

    #[test]
    fn discovery_attempt_fault_rolls_back_every_transition() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let attempt_id = ProviderDiscoveryAttemptId::new();
        repository
            .begin_discovery_attempt(attempt_id, fixture_profile_id(), fixture_time(2))
            .expect("the attempt begins");

        repository.arm_fault(FaultPoint::DiscoveryAttempt);
        repository
            .mark_discovery_started(attempt_id, fixture_time(3))
            .expect_err("injected attempt fault aborts the start");
        repository.arm_fault(FaultPoint::DiscoveryAttempt);
        repository
            .fail_discovery_attempt(attempt_id, fixture_failure(), fixture_time(4))
            .expect_err("injected attempt fault aborts the failure");
        assert_eq!(
            raw_discovery_attempt(&location, attempt_id),
            ("prepared".to_owned(), None, None, None)
        );

        repository
            .mark_discovery_started(attempt_id, fixture_time(5))
            .expect("the attempt starts");
        repository.arm_fault(FaultPoint::DiscoveryAttempt);
        repository
            .complete_discovery_attempt(
                attempt_id,
                vec![
                    ProviderModelRecordDto::new("fixture-model", Some("Fixture Model".to_owned()))
                        .expect("fixture model record is valid"),
                ],
                fixture_time(6),
            )
            .expect_err("injected attempt fault aborts the completion");
        assert_eq!(
            raw_discovery_attempt(&location, attempt_id),
            ("started".to_owned(), Some(5), None, None)
        );
        assert_eq!(raw_count(&location, "provider_discovery_result_records"), 0);
    }

    #[test]
    fn discovery_recovery_fault_rolls_back_terminalization() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let prepared = ProviderDiscoveryAttemptId::new();
        let started = ProviderDiscoveryAttemptId::new();
        repository
            .begin_discovery_attempt(prepared, fixture_profile_id(), fixture_time(2))
            .expect("the before-start attempt begins");
        repository
            .begin_discovery_attempt(started, fixture_profile_id(), fixture_time(2))
            .expect("the started attempt begins");
        repository
            .mark_discovery_started(started, fixture_time(3))
            .expect("the attempt starts");
        repository.arm_fault(FaultPoint::DiscoveryRecovery);
        repository
            .recover_unfinished_discovery_attempts(fixture_time(4))
            .expect_err("injected recovery fault aborts the transaction");
        assert_eq!(
            raw_discovery_attempt(&location, prepared),
            ("prepared".to_owned(), None, None, None)
        );
        assert_eq!(
            raw_discovery_attempt(&location, started),
            ("started".to_owned(), Some(3), None, None)
        );
    }

    #[test]
    fn reasoning_history_fault_rolls_back_the_manifest_selection_and_run() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        repository.arm_fault(FaultPoint::TurnAcceptance);
        repository
            .accept_user_turn(
                session_id,
                IdempotencyKey::new(),
                "dependent turn",
                RunId::new(),
                fixture_snapshot(),
                fixture_selection(),
                Some(fixture_manifest(session_id)),
                fixture_time(2),
            )
            .expect_err("injected acceptance fault aborts the dependent transaction");
        drop(repository);
        let reopened = SqliteStorageRepository::open(location.clone()).expect("database reopens");
        assert!(
            reopened
                .load_session_projection(session_id)
                .expect("projection loads")
                .active_run()
                .is_none()
        );
        for table in [
            "turns",
            "runs",
            "run_provider_selections",
            "reasoning_history_manifests",
            "reasoning_history_manifest_entries",
            "reasoning_history_bounds",
        ] {
            assert_eq!(raw_count(&location, table), 0, "{table} staged no row");
        }
    }

    #[test]
    fn persisted_json_columns_never_contain_the_fixture_credential() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        repository
            .accept_catalog_revision(fixture_catalog_revision())
            .expect("the fixture catalog accepts");
        let run = match repository
            .accept_user_turn(
                session_id,
                IdempotencyKey::new(),
                "active",
                RunId::new(),
                fixture_snapshot(),
                fixture_selection(),
                Some(fixture_manifest(session_id)),
                fixture_time(2),
            )
            .expect("fixture turn commits")
        {
            AcceptedTurnOutcomeDto::Started { run, .. } => run,
            AcceptedTurnOutcomeDto::Pending(_) => unreachable!("the first turn starts a run"),
        };
        repository
            .append_message(
                MessageProjectionDto::new(
                    session_id,
                    Some(run.run_id()),
                    MessageKindDto::Assistant,
                    "safe answer",
                    Some("safe reasoning".to_owned()),
                    None,
                    None,
                )
                .expect("assistant row is valid"),
                fixture_time(3),
            )
            .expect("assistant row commits");
        let call_id = ToolCallId::new();
        let evidence = ToolResultEvidenceDto::new(
            session_id,
            run.run_id(),
            call_id,
            "read",
            ToolResultStatusDto::Completed,
            r#"{"result":"read"}"#,
            Vec::new(),
            fixture_time(3),
        )
        .expect("fixture evidence is valid");
        repository
            .write_tool_result(
                evidence,
                MessageProjectionDto::new(
                    session_id,
                    Some(run.run_id()),
                    MessageKindDto::ToolResult,
                    r#"{"result":"read"}"#,
                    None,
                    Some(call_id),
                    Some("read".to_owned()),
                )
                .expect("answering row is valid"),
            )
            .expect("tool result commits");
        repository
            .finish_run(
                session_id,
                run.run_id(),
                RunOutcomeDto::new(
                    RunStatusDto::Failed,
                    Some(UsageDto::NotReported),
                    None,
                    Some("provider_failed".to_owned()),
                    Some("safe failure".to_owned()),
                )
                .expect("fixture outcome is valid"),
                fixture_time(4),
            )
            .expect("terminal outcome commits");
        let columns = raw_json_columns(&location);
        assert!(!columns.is_empty());
        assert!(
            columns
                .iter()
                .all(|value| !value.contains("fixture-secret"))
        );
    }
}
