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
    AcceptedTurnOutcomeDto, StartingRunModelContextDto, StorageRepositoryDto, ToolResultEvidenceDto,
};
use intention_config::ConfigSnapshotDto;
use intention_domain::{
    ToolResultMetadataEntryDto, ToolResultStatusDto, run_status_is_terminal,
    validate_run_status_transition,
};
use intention_proto::{
    ConfigRevisionId, CreateSessionCommandDto, DtoResult, ErrorCategoryDto, ErrorDto,
    ErrorRetryDto, FinishReasonDto, IdempotencyKey, ProjectId, RemoveTurnCommandDto, RunId,
    SessionId, TimestampDto, ToolCallId, TurnId, UsageDto, WorkspaceId,
};
use intention_proto::{
    MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto, RunModeDto, RunProjectionDto,
    RunStatusDto, SessionProjectionDto, WorkspaceRootDto,
};
use sqlite::OptionalExtension;

/// The durable run statuses that accept no further transitions.
const TERMINAL_STATUSES: &str = "'completed','failed','interrupted'";

/// The transcript projection columns in their canonical decode order.
const MESSAGE_COLUMNS: &str = "session_id, run_id, kind, text, reasoning, tool_call_id, tool_id";

/// The complete current storage schema (logical version 1): the eight
/// current-state tables created directly on open. There is no migration chain,
/// and no event log, snapshot, cursor, or journal table exists under the single
/// live schema. `SCHEMA_STAMP` records the version this text implements.
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
CREATE UNIQUE INDEX IF NOT EXISTS one_active_run_per_session ON runs(session_id)
  WHERE status NOT IN ('completed','failed','interrupted');
";

/// The single schema stamp of the current storage schema, written to the
/// `user_version` header field of every database this module creates.
///
/// Bump this integer whenever `SCHEMA_SQL` or the shape of any persisted JSON
/// column changes: the next open discards the whole database file and its
/// rollback journal and recreates the current schema from scratch. That discard
/// is the only version gate; there is no migration path.
const SCHEMA_STAMP: i32 = 1;

/// Returns whether the open database already carries the current schema stamp.
/// A database written under any other stamp is discarded by its opener.
fn carries_current_schema_stamp(connection: &sqlite::Connection) -> DtoResult<bool> {
    let stamp: i32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(storage_error)?;
    Ok(stamp == SCHEMA_STAMP)
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
    for suffix in ["", "-journal"] {
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

impl SqliteStorageRepository {
    /// Opens or creates a local database at an explicitly supplied absolute
    /// location. A database that does not carry the current schema stamp is
    /// discarded and recreated from scratch; the current schema is then created
    /// directly on open and stamped with `SCHEMA_STAMP`.
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
            .map_err(|_| unavailable())?;
        connection
            .execute_batch(&format!("PRAGMA user_version = {SCHEMA_STAMP};"))
            .map_err(|_| unavailable())?;
        Ok(Self {
            connection: Mutex::new(connection),
            #[cfg(test)]
            fault: Mutex::new(None),
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

    /// Reads the current committed projection of one session from its rows.
    fn projection_of(
        connection: &sqlite::Connection,
        session_id: SessionId,
    ) -> DtoResult<SessionProjectionDto> {
        let session = connection
            .query_row(
                "SELECT sessions.project_id, sessions.workspace_id, workspace_roots.root, sessions.mode \
                 FROM sessions JOIN workspace_roots ON workspace_roots.id=sessions.workspace_id \
                 WHERE sessions.id=?1",
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
            .map_err(not_found_or_storage)?;
        let active = connection
            .query_row(
                &format!(
                    "SELECT id, turn_id, status, config_revision_id FROM runs \
                     WHERE session_id=?1 AND status NOT IN ({TERMINAL_STATUSES})"
                ),
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
            .map(|(run, turn, status, revision)| {
                run_projection(session_id, &run, &turn, &status, &revision)
            })
            .transpose()?;
        let pending_turns = pending_turns(connection, session_id)?;
        SessionProjectionDto::new(
            ProjectId::parse(&session.0).map_err(codec_error)?,
            session_id,
            WorkspaceId::parse(&session.1).map_err(codec_error)?,
            WorkspaceRootDto::parse(session.2).map_err(codec_error)?,
            parse_mode(&session.3)?,
            None,
            active,
            pending_turns,
        )
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
                    message_kind_name(message.kind()),
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
}

macro_rules! immediate_transaction {
    ($repository:expr, |$transaction:ident| $operation:block) => {{
        let mut connection = $repository.connection()?;
        let result = (|| {
            let $transaction = $repository.begin(&mut connection)?;
            $operation
        })();
        drop(connection);
        result
    }};
}

impl StorageRepositoryDto for SqliteStorageRepository {
    fn create_session(
        &self,
        command: CreateSessionCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<SessionProjectionDto> {
        let session_id = command.session_id();
        let occurred_at = occurred_at.unix_seconds();
        immediate_transaction!(self, |tx| {
            if tx
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
            tx.execute(
                "INSERT INTO projects(id, name, created_at) VALUES (?1, ?1, ?2) \
                 ON CONFLICT(id) DO NOTHING",
                sqlite::params![command.project_id().to_string(), occurred_at],
            )
            .map_err(storage_error)?;
            // The root binding is unique in both directions: a second workspace
            // identity bound to an already-used root must surface as the same
            // typed conflict as the reverse identity/root mismatch, instead of
            // a raw SQLite constraint failure that collapses into
            // `storage_unavailable`.
            let root_bound_to_other_identity = tx
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
            tx.execute(
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
            let stored_workspace_root: String = tx
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
            tx.execute(
                "INSERT INTO sessions(id, project_id, workspace_id, mode, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                sqlite::params![
                    session_id.to_string(),
                    command.project_id().to_string(),
                    command.workspace_id().to_string(),
                    mode_name(command.mode()),
                    occurred_at
                ],
            )
            .map_err(storage_error)?;
            let projection = Self::projection_of(&tx, session_id)?;
            tx.commit().map_err(storage_error)?;
            Ok(projection)
        })
    }

    fn accept_user_turn(
        &self,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: &str,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_turn_content",
                "user turn content must not be empty",
            ));
        }
        let config_revision_id = config_snapshot.revision_id();
        let insert_turn = |connection: &sqlite::Transaction<'_>,
                           turn_id: TurnId,
                           state: &str,
                           created_at: i64|
         -> DtoResult<()> {
            connection
                .execute(
                    "INSERT INTO turns(id, session_id, proposed_run_id, config_revision_id, content, state, idempotency_key, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    sqlite::params![
                        turn_id.to_string(),
                        session_id.to_string(),
                        proposed_run_id.to_string(),
                        config_revision_id.to_string(),
                        content,
                        state,
                        idempotency_key.to_string(),
                        created_at,
                    ],
                )
                .map_err(storage_error)?;
            Ok(())
        };
        immediate_transaction!(self, |tx| {
            Self::require_session(&tx, session_id)?;
            let existing = tx
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
            if let Some((turn, stored_content, stored_run_id, stored_revision_id, state)) = existing
            {
                if stored_content != content
                    || stored_run_id != proposed_run_id.to_string()
                    || stored_revision_id != config_revision_id.to_string()
                {
                    return Err(turn_idempotency_conflict());
                }
                let turn_id = TurnId::parse(&turn).map_err(codec_error)?;
                // A repeated acceptance replays the current durable outcome:
                // a started turn reports its run's actual status together with
                // the user message that started the run, and a queued or joined
                // turn is still the pending projection it was accepted as.
                let outcome = match state.as_str() {
                    "started" => {
                        let run = load_scoped_run(&tx, session_id, proposed_run_id)?;
                        let message = first_user_message(&tx, session_id, proposed_run_id)?;
                        AcceptedTurnOutcomeDto::Started { run, message }
                    }
                    "pending" | "appended" => AcceptedTurnOutcomeDto::Pending(
                        PendingTurnProjectionDto::new(session_id, turn_id, stored_content)?,
                    ),
                    "removed" => return Err(turn_idempotency_conflict()),
                    _ => return Err(codec_error("invalid durable turn state")),
                };
                tx.commit().map_err(storage_error)?;
                return Ok(outcome);
            }
            // Proposed run identities are globally unique while the idempotency
            // check above is scoped to the session, so a caller-supplied run
            // identity already durable anywhere must surface as a typed
            // conflict instead of a raw SQLite constraint failure that
            // collapses into `storage_unavailable`.
            let identity_bound = tx
                .query_row(
                    "SELECT 1 FROM turns WHERE proposed_run_id=?1 \
                     UNION ALL SELECT 1 FROM runs WHERE id=?1 LIMIT 1",
                    [proposed_run_id.to_string()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(storage_error)?
                .is_some();
            if identity_bound {
                return Err(turn_identity_conflict());
            }
            Self::store_config(&tx, &config_snapshot)?;
            let active = tx
                .query_row(
                    &format!(
                        "SELECT 1 FROM runs WHERE session_id=?1 AND status NOT IN ({TERMINAL_STATUSES})"
                    ),
                    [session_id.to_string()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(storage_error)?
                .is_some();
            let occurred_at = occurred_at.unix_seconds();
            let turn_id = TurnId::new();
            if active {
                insert_turn(&tx, turn_id, "pending", occurred_at)?;
                let turn = PendingTurnProjectionDto::new(session_id, turn_id, content)?;
                tx.commit().map_err(storage_error)?;
                return Ok(AcceptedTurnOutcomeDto::Pending(turn));
            }
            // An idle session either starts the oldest pending message (its own
            // stored run identity, configuration revision, and content) or
            // starts the newly accepted turn. The accepted turn stays pending
            // in the first case; it joins the promoted run's context later.
            let (started_turn, started_run, started_revision, started_content) =
                if let Some((pending_turn, proposed_run, revision, content)) =
                    oldest_pending_turn(&tx, session_id)?
                {
                    insert_turn(&tx, turn_id, "pending", occurred_at)?;
                    tx.execute(
                        "UPDATE turns SET state='started' WHERE session_id=?1 AND id=?2",
                        sqlite::params![session_id.to_string(), pending_turn.to_string()],
                    )
                    .map_err(storage_error)?;
                    (pending_turn, proposed_run, revision, content)
                } else {
                    insert_turn(&tx, turn_id, "started", occurred_at)?;
                    (
                        turn_id,
                        proposed_run_id,
                        config_revision_id,
                        content.to_owned(),
                    )
                };
            tx.execute(
                "INSERT INTO runs(id, session_id, turn_id, config_revision_id, status, started_at) \
                 VALUES (?1, ?2, ?3, ?4, 'starting', ?5)",
                sqlite::params![
                    started_run.to_string(),
                    session_id.to_string(),
                    started_turn.to_string(),
                    started_revision.to_string(),
                    occurred_at
                ],
            )
            .map_err(storage_error)?;
            let message = MessageProjectionDto::new(
                session_id,
                Some(started_run),
                MessageKindDto::User,
                started_content,
                None,
                None,
                None,
            )?;
            Self::insert_message(&tx, &message, occurred_at)?;
            self.fault(FaultPoint::TurnAcceptance)?;
            let run = RunProjectionDto::new(
                session_id,
                started_run,
                started_turn,
                RunStatusDto::Starting,
                started_revision,
            );
            tx.commit().map_err(storage_error)?;
            Ok(AcceptedTurnOutcomeDto::Started { run, message })
        })
    }

    fn remove_turn(
        &self,
        command: RemoveTurnCommandDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<PendingTurnProjectionDto> {
        let session_id = command.session_id();
        let turn_id = command.turn_id();
        immediate_transaction!(self, |tx| {
            let content: String = tx
                .query_row(
                    "SELECT content FROM turns WHERE session_id=?1 AND id=?2 AND state='pending'",
                    sqlite::params![session_id.to_string(), turn_id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?
                .ok_or_else(pending_turn_not_found)?;
            tx.execute(
                "UPDATE turns SET state='removed' WHERE session_id=?1 AND id=?2 AND state='pending'",
                sqlite::params![session_id.to_string(), turn_id.to_string()],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)?;
            PendingTurnProjectionDto::new(session_id, turn_id, content)
        })
    }

    fn consume_pending_user_turns(
        &self,
        session_id: SessionId,
        run_id: RunId,
        occurred_at: TimestampDto,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        immediate_transaction!(self, |tx| {
            load_scoped_run(&tx, session_id, run_id)?;
            let pending = pending_turns(&tx, session_id)?;
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
                Self::insert_message(&tx, &message, occurred_at)?;
                tx.execute(
                    "UPDATE turns SET state='appended' WHERE session_id=?1 AND id=?2",
                    sqlite::params![session_id.to_string(), turn.turn_id().to_string()],
                )
                .map_err(storage_error)?;
                messages.push(message);
            }
            tx.commit().map_err(storage_error)?;
            Ok(messages)
        })
    }

    fn transition_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        _occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto> {
        immediate_transaction!(self, |tx| {
            let current = load_scoped_run(&tx, session_id, run_id)?;
            validate_run_status_transition(current.status(), status)?;
            tx.execute(
                "UPDATE runs SET status=?3 WHERE session_id=?1 AND id=?2",
                sqlite::params![
                    session_id.to_string(),
                    run_id.to_string(),
                    status_name(status)
                ],
            )
            .map_err(storage_error)?;
            let run = RunProjectionDto::new(
                session_id,
                run_id,
                current.turn_id(),
                status,
                current.config_revision_id(),
            );
            tx.commit().map_err(storage_error)?;
            Ok(run)
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "One flat terminal outcome keeps the single transaction at one call site."
    )]
    fn finish_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        usage: Option<UsageDto>,
        finish_reason: Option<FinishReasonDto>,
        error_code: Option<String>,
        error_message: Option<String>,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto> {
        if !run_status_is_terminal(status) {
            return Err(ErrorDto::validation(
                "invalid_run_outcome",
                "a terminal run outcome needs a terminal status",
            ));
        }
        let error_valid = match (&error_code, &error_message) {
            (None, None) => true,
            (Some(code), Some(message)) => {
                !code.trim().is_empty()
                    && !message.trim().is_empty()
                    && !code.contains('\0')
                    && !message.contains('\0')
            }
            _ => false,
        };
        if !error_valid {
            return Err(ErrorDto::validation(
                "invalid_run_outcome",
                "a terminal run outcome carries either both error fields or neither",
            ));
        }
        immediate_transaction!(self, |tx| {
            let current = load_scoped_run(&tx, session_id, run_id)?;
            let run = RunProjectionDto::new(
                session_id,
                run_id,
                current.turn_id(),
                status,
                current.config_revision_id(),
            );
            if current.status() == status {
                // The run already holds this exact terminal outcome; the
                // repeated commit is accepted and changes nothing.
                tx.commit().map_err(storage_error)?;
                return Ok(run);
            }
            validate_run_status_transition(current.status(), status)?;
            let usage_json = usage
                .as_ref()
                .map(|usage| serde_json::to_string(usage).map_err(codec_error))
                .transpose()?;
            let finish_reason = finish_reason.map(finish_reason_name);
            tx.execute(
                "UPDATE runs SET status=?3, usage_json=?4, finish_reason=?5, error_code=?6, \
                 error_message=?7, finished_at=?8 WHERE session_id=?1 AND id=?2",
                sqlite::params![
                    session_id.to_string(),
                    run_id.to_string(),
                    status_name(status),
                    usage_json,
                    finish_reason,
                    error_code.as_deref(),
                    error_message.as_deref(),
                    occurred_at.unix_seconds()
                ],
            )
            .map_err(storage_error)?;
            self.fault(FaultPoint::RunOutcome)?;
            tx.commit().map_err(storage_error)?;
            Ok(run)
        })
    }

    fn append_message(
        &self,
        message: MessageProjectionDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<MessageProjectionDto> {
        let session_id = message.session_id();
        immediate_transaction!(self, |tx| {
            Self::require_session(&tx, session_id)?;
            if let Some(run_id) = message.run_id() {
                load_scoped_run(&tx, session_id, run_id)?;
            }
            Self::insert_message(&tx, &message, occurred_at.unix_seconds())?;
            self.fault(FaultPoint::Message)?;
            tx.commit().map_err(storage_error)?;
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
        immediate_transaction!(self, |tx| {
            Self::require_session(&tx, session_id)?;
            load_scoped_run(&tx, session_id, run_id)?;
            let duplicate = tx
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
            tx.execute(
                "INSERT INTO tool_results(tool_call_id, session_id, run_id, tool_id, status, content, metadata_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                sqlite::params![
                    call_id.to_string(),
                    session_id.to_string(),
                    run_id.to_string(),
                    evidence.tool_id(),
                    tool_result_status_name(evidence.status()),
                    evidence.content(),
                    metadata_json,
                    evidence.occurred_at().unix_seconds()
                ],
            )
            .map_err(storage_error)?;
            Self::insert_message(&tx, &message, evidence.occurred_at().unix_seconds())?;
            self.fault(FaultPoint::ToolResult)?;
            tx.commit().map_err(storage_error)?;
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
            let connection = self.connection().map_err(|_| tool_result_unavailable())?;
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
                .map_err(|_| tool_result_unavailable())?
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
            parse_tool_result_status(&status).map_err(|_| tool_result_unavailable())?,
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
        serde_json::from_str(&snapshot).map_err(codec_error)
    }

    fn load_starting_run_model_context(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto> {
        let mut connection = self
            .connection()
            .map_err(|_| run_model_context_unavailable())?;
        let transaction = connection
            .transaction_with_behavior(sqlite::TransactionBehavior::Immediate)
            .map_err(|_| run_model_context_unavailable())?;
        let context = (|| {
            let target = load_scoped_run(&transaction, session_id, run_id)
                .map_err(|_| run_model_context_unavailable())?;
            if target.status() != RunStatusDto::Starting {
                return Err(run_model_context_unavailable());
            }
            let safe_config = load_config_snapshot(&transaction, target.config_revision_id())?;
            let messages = starting_context_messages(&transaction, session_id, run_id)?;
            StartingRunModelContextDto::new(session_id, run_id, safe_config, messages)
                .map_err(|_| run_model_context_unavailable())
        })();
        let result = match context {
            Ok(context) => transaction
                .commit()
                .map_err(|_| run_model_context_unavailable())
                .map(|()| context),
            Err(error) => {
                let _ = transaction.rollback();
                Err(error)
            }
        };
        drop(connection);
        result
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
        validate_message_limit(limit)?;
        let connection = self.connection()?;
        Self::require_session(&connection, session_id)?;
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
        drop(statement);
        drop(connection);
        Ok(messages)
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
        let unfinished = {
            let connection = self.connection()?;
            let mut statement = connection
                .prepare(&format!(
                    "SELECT session_id, id FROM runs WHERE status NOT IN ({TERMINAL_STATUSES}) \
                     ORDER BY session_id, id"
                ))
                .map_err(storage_error)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(storage_error)?;
            let unfinished = rows
                .map(|row| row.map_err(storage_error))
                .collect::<DtoResult<Vec<_>>>()?;
            drop(statement);
            drop(connection);
            unfinished
        };
        let mut recovered = Vec::with_capacity(unfinished.len());
        for (session, run) in unfinished {
            recovered.push(self.transition_run(
                SessionId::parse(&session).map_err(codec_error)?,
                RunId::parse(&run).map_err(codec_error)?,
                RunStatusDto::Interrupted,
                recovered_at,
            )?);
        }
        Ok(recovered)
    }

    fn accept_configuration_revision(&self, snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            Self::store_config(&tx, &snapshot)?;
            self.fault(FaultPoint::ConfigRevision)?;
            tx.commit().map_err(storage_error)
        })
    }
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
        parse_message_kind(&row.2)?,
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

/// Loads one scoped durable run or a typed not-found error.
fn load_scoped_run(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<RunProjectionDto> {
    let row = connection
        .query_row(
            "SELECT turn_id, status, config_revision_id FROM runs WHERE session_id=?1 AND id=?2",
            sqlite::params![session_id.to_string(), run_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(not_found_or_storage)?;
    run_projection(session_id, &run_id.to_string(), &row.0, &row.1, &row.2)
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
        .map_err(|_| run_model_context_unavailable())?;
    let Some(last_id) = last_id else {
        return Err(run_model_context_unavailable());
    };
    let mut statement = connection
        .prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE session_id=?1 AND id<=?2 ORDER BY id"
        ))
        .map_err(|_| run_model_context_unavailable())?;
    let rows = statement
        .query_map(
            sqlite::params![session_id.to_string(), last_id],
            raw_message_row,
        )
        .map_err(|_| run_model_context_unavailable())?;
    rows.map(|row| {
        let row = row.map_err(|_| run_model_context_unavailable())?;
        decode_message(row).map_err(|_| run_model_context_unavailable())
    })
    .collect()
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
        .map_err(|_| run_model_context_unavailable())?;
    let snapshot: ConfigSnapshotDto =
        serde_json::from_str(&encoded).map_err(|_| run_model_context_unavailable())?;
    snapshot
        .validate_for_persistence()
        .map_err(|_| run_model_context_unavailable())?;
    Ok(snapshot)
}

/// Returns every pending turn of one session in insertion order.
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
) -> DtoResult<RunProjectionDto> {
    Ok(RunProjectionDto::new(
        session,
        RunId::parse(run).map_err(codec_error)?,
        TurnId::parse(turn).map_err(codec_error)?,
        parse_status(status)?,
        ConfigRevisionId::parse(revision).map_err(codec_error)?,
    ))
}

const fn mode_name(value: RunModeDto) -> &'static str {
    match value {
        RunModeDto::Plan => "plan",
        RunModeDto::Build => "build",
    }
}

fn parse_mode(value: &str) -> DtoResult<RunModeDto> {
    match value {
        "plan" => Ok(RunModeDto::Plan),
        "build" => Ok(RunModeDto::Build),
        _ => Err(codec_error("invalid durable mode")),
    }
}

const fn status_name(value: RunStatusDto) -> &'static str {
    match value {
        RunStatusDto::Starting => "starting",
        RunStatusDto::Running => "running",
        RunStatusDto::Completed => "completed",
        RunStatusDto::Failed => "failed",
        RunStatusDto::Interrupted => "interrupted",
    }
}

fn parse_status(value: &str) -> DtoResult<RunStatusDto> {
    match value {
        "starting" => Ok(RunStatusDto::Starting),
        "running" => Ok(RunStatusDto::Running),
        "completed" => Ok(RunStatusDto::Completed),
        "failed" => Ok(RunStatusDto::Failed),
        "interrupted" => Ok(RunStatusDto::Interrupted),
        _ => Err(codec_error("invalid durable status")),
    }
}

const fn message_kind_name(value: MessageKindDto) -> &'static str {
    match value {
        MessageKindDto::User => "user",
        MessageKindDto::Assistant => "assistant",
        MessageKindDto::ToolCall => "tool_call",
        MessageKindDto::ToolResult => "tool_result",
        MessageKindDto::Notice => "notice",
    }
}

fn parse_message_kind(value: &str) -> DtoResult<MessageKindDto> {
    match value {
        "user" => Ok(MessageKindDto::User),
        "assistant" => Ok(MessageKindDto::Assistant),
        "tool_call" => Ok(MessageKindDto::ToolCall),
        "tool_result" => Ok(MessageKindDto::ToolResult),
        "notice" => Ok(MessageKindDto::Notice),
        _ => Err(codec_error("invalid durable message kind")),
    }
}

const fn tool_result_status_name(value: ToolResultStatusDto) -> &'static str {
    match value {
        ToolResultStatusDto::Completed => "completed",
        ToolResultStatusDto::Failed => "failed",
        ToolResultStatusDto::Partial => "partial",
    }
}

fn parse_tool_result_status(value: &str) -> DtoResult<ToolResultStatusDto> {
    match value {
        "completed" => Ok(ToolResultStatusDto::Completed),
        "failed" => Ok(ToolResultStatusDto::Failed),
        "partial" => Ok(ToolResultStatusDto::Partial),
        _ => Err(codec_error("invalid durable tool result status")),
    }
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

fn unavailable() -> ErrorDto {
    ErrorDto::unavailable(
        "storage_unavailable",
        "the local durable storage is unavailable",
    )
}

fn storage_error(_: sqlite::Error) -> ErrorDto {
    unavailable()
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
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Focused SQLite fixtures use expect for test diagnostics."
    )]
    use super::*;
    use crate::StorageRepositoryDto;
    use intention_config::{ConfigPathDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto};
    use intention_proto::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
    use intention_proto::{IdempotencyKey, ProjectId, SchemaVersionDto, UsageDto, WorkspaceId};

    fn fixture_time(value: i64) -> TimestampDto {
        TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
    }

    fn fixture_snapshot() -> ConfigSnapshotDto {
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-storage-unit.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        );
        let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-secret\"",
            source,
        ))
        .expect("fixture configuration resolves");
        ConfigSnapshotDto::new(
            SchemaVersionDto::new(1, 0),
            ConfigRevisionId::new(),
            fixture_time(1),
            resolved,
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

    fn raw_json_columns(location: &SqliteDatabaseLocationDto) -> Vec<String> {
        let connection =
            sqlite::Connection::open(&location.0).expect("database reopens for inspection");
        [
            "SELECT snapshot_json FROM configuration_revisions",
            "SELECT metadata_json FROM tool_results",
            "SELECT COALESCE(usage_json, '') FROM runs",
            "SELECT text FROM messages",
            "SELECT COALESCE(reasoning, '') FROM messages",
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
    fn durable_codec_helpers_cover_declared_values_and_safe_errors() {
        for mode in [RunModeDto::Plan, RunModeDto::Build] {
            assert_eq!(
                parse_mode(mode_name(mode)).expect("known mode parses"),
                mode
            );
        }
        assert_eq!(
            parse_mode("invalid")
                .expect_err("unknown mode rejects")
                .code(),
            "storage_decode_failed"
        );
        for status in [
            ToolResultStatusDto::Completed,
            ToolResultStatusDto::Failed,
            ToolResultStatusDto::Partial,
        ] {
            assert_eq!(
                parse_tool_result_status(tool_result_status_name(status))
                    .expect("known tool result status parses"),
                status
            );
        }
        assert_eq!(tool_result_unavailable().code(), "tool_result_unavailable");
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
                RunStatusDto::Completed,
                Some(UsageDto::reported(1, 2, 3).expect("fixture usage is consistent")),
                Some(FinishReasonDto::Stop),
                None,
                None,
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
    fn persisted_json_columns_never_contain_the_fixture_credential() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        let run = accept_fixture_turn(&repository, session_id, "active");
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
                RunStatusDto::Failed,
                Some(UsageDto::NotReported),
                None,
                Some("provider_failed".to_owned()),
                Some("safe failure".to_owned()),
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
