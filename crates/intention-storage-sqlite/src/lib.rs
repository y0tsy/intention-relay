//! SQLite-backed M3 durable storage implementation.
//!
//! The public boundary is DTO-only. SQLite connections, SQL rows, paths, and
//! JSON codecs remain private implementation details of this crate.

use std::path::Path;
use std::sync::{LazyLock, Mutex};

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    DomainEventDto, ModelRunFactDto, ModelRunFactEventDto, ModelRunFactInputDto,
    ModelRunProjectionDto, QueuedTurnProjectionDto, QueuedTurnRemovedEventDto, RunEventCursorDto,
    RunEventTailPageDto, RunProjectionDto, RunReplayDto, RunSnapshotDto, RunStartedEventDto,
    RunStatusChangedEventDto, RunStatusDto, SessionCreatedEventDto, SessionProjectionDto,
    UserTurnAcceptedEventDto, UserTurnQueuedEventDto, validate_run_status_transition,
};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendModelRunFactsInputDto,
    AppendModelRunFactsOutcomeDto, CommittedChangeDto, CreateSessionInputDto,
    RecoverUnfinishedRunsInputDto, RemoveQueuedTurnInputDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, EventEnvelopeDto,
    EventId, EventMetadataDto, ProjectId, QueuePositionDto, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, TurnId, WorkspaceId,
};
use rusqlite_migration::{M, Migrations};
use sqlite::OptionalExtension;

const CURRENT_STORAGE_SCHEMA: i64 = 2;
const MAX_CANONICAL_FACT_BYTES: usize = 512 * 1024;
const MAX_TAIL_CANONICAL_BYTES: usize = 512 * 1024;
const MAX_TAIL_FACTS: usize = 256;
const TERMINAL_STATUSES: &str = "'completed','cancelled','failed','interrupted'";

static MIGRATIONS: LazyLock<Migrations<'static>> = LazyLock::new(|| {
    Migrations::new(vec![M::up(
    "
CREATE TABLE projects (project_id TEXT PRIMARY KEY);
CREATE TABLE workspace_roots (workspace_id TEXT PRIMARY KEY, workspace_root TEXT NOT NULL UNIQUE);
CREATE TABLE sessions (
  session_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(project_id),
  workspace_id TEXT NOT NULL REFERENCES workspace_roots(workspace_id),
  workspace_root TEXT NOT NULL,
  mode TEXT NOT NULL,
  config_revision_id TEXT,
  last_sequence INTEGER NOT NULL CHECK(last_sequence >= 0),
  next_queue_ticket INTEGER NOT NULL CHECK(next_queue_ticket >= 0)
);
CREATE TABLE turns (
  session_id TEXT NOT NULL REFERENCES sessions(session_id), turn_id TEXT NOT NULL,
  content TEXT NOT NULL CHECK(length(trim(content)) > 0), proposed_run_id TEXT NOT NULL,
  config_revision_id TEXT NOT NULL, outcome TEXT NOT NULL CHECK(outcome IN ('started','queued')),
  queue_ticket INTEGER, PRIMARY KEY (session_id, turn_id), UNIQUE (proposed_run_id)
);
CREATE TABLE runs (
  run_id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(session_id),
  turn_id TEXT NOT NULL, status TEXT NOT NULL, config_revision_id TEXT NOT NULL,
  UNIQUE(session_id, turn_id)
);
CREATE UNIQUE INDEX one_active_run_per_session ON runs(session_id)
  WHERE status NOT IN ('completed','cancelled','failed','interrupted');
CREATE TABLE queued_turns (
  session_id TEXT NOT NULL REFERENCES sessions(session_id), turn_id TEXT NOT NULL,
  queue_ticket INTEGER NOT NULL, PRIMARY KEY(session_id, turn_id), UNIQUE(session_id, queue_ticket)
);
CREATE TABLE configuration_revisions (revision_id TEXT PRIMARY KEY, snapshot_json TEXT NOT NULL);
CREATE TABLE domain_events (
  event_id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(session_id),
  sequence INTEGER NOT NULL CHECK(sequence > 0), envelope_json TEXT NOT NULL,
  UNIQUE(session_id, sequence)
);
CREATE TABLE session_snapshots (
  session_id TEXT PRIMARY KEY REFERENCES sessions(session_id), sequence INTEGER NOT NULL,
  projection_json TEXT NOT NULL
);
CREATE TABLE run_snapshots (
  run_id TEXT PRIMARY KEY REFERENCES runs(run_id), session_id TEXT NOT NULL REFERENCES sessions(session_id),
  sequence INTEGER NOT NULL, projection_json TEXT NOT NULL
);
",
), M::up(
    "
CREATE TABLE run_cursors (
  run_id TEXT PRIMARY KEY REFERENCES runs(run_id), session_id TEXT NOT NULL REFERENCES sessions(session_id),
  cursor INTEGER NOT NULL CHECK(cursor >= 0)
);
CREATE TABLE model_run_facts (
  run_id TEXT NOT NULL REFERENCES runs(run_id), cursor INTEGER NOT NULL CHECK(cursor > 0),
  event_id TEXT NOT NULL REFERENCES domain_events(event_id),
  PRIMARY KEY(run_id, cursor), UNIQUE(event_id)
);
CREATE TABLE model_run_snapshots (
  run_id TEXT PRIMARY KEY REFERENCES runs(run_id), session_id TEXT NOT NULL REFERENCES sessions(session_id),
  sequence INTEGER NOT NULL, cursor INTEGER NOT NULL CHECK(cursor >= 0), snapshot_json TEXT NOT NULL
);
INSERT INTO run_cursors(run_id, session_id, cursor)
  SELECT run_id, session_id, 0 FROM runs;
", )])
});

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
    /// Opens or creates a migrated local database at an explicitly supplied absolute location.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error when the database cannot be opened or migrated.
    pub fn open(location: SqliteDatabaseLocationDto) -> DtoResult<Self> {
        let mut connection = sqlite::Connection::open(location.0).map_err(storage_error)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(storage_error)?;
        let stored_version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(storage_error)?;
        if stored_version > CURRENT_STORAGE_SCHEMA {
            return Err(ErrorDto::unavailable(
                "unsupported_storage_schema",
                "the local storage schema is newer than this application supports",
            ));
        }
        MIGRATIONS
            .to_latest(&mut connection)
            .map_err(|_| unavailable())?;
        Self::hydrate_model_run_snapshots(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            #[cfg(test)]
            fault: Mutex::new(None),
        })
    }

    fn hydrate_model_run_snapshots(connection: &mut sqlite::Connection) -> DtoResult<()> {
        let tx = connection
            .transaction_with_behavior(sqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let session_ids = {
            let mut statement = tx
                .prepare("SELECT session_id FROM sessions ORDER BY session_id")
                .map_err(storage_error)?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(storage_error)?;
            rows.map(|row| row.map_err(storage_error))
                .collect::<DtoResult<Vec<_>>>()?
        };
        for encoded_session in session_ids {
            let session_id = SessionId::parse(&encoded_session).map_err(codec_error)?;
            let projection = Self::project(&tx, session_id)?;
            Self::ensure_run_cursors(&tx, session_id)?;
            Self::snapshot_model_runs(&tx, &projection)?;
        }
        tx.commit().map_err(storage_error)
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

    fn store_config(tx: &sqlite::Transaction<'_>, snapshot: &ConfigSnapshotDto) -> DtoResult<()> {
        snapshot.validate_for_persistence()?;
        let encoded = serde_json::to_string(snapshot).map_err(codec_error)?;
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO configuration_revisions(revision_id, snapshot_json) VALUES (?1, ?2)",
                sqlite::params![snapshot.revision_id().to_string(), encoded],
            )
            .map_err(storage_error)?;
        if inserted == 0 {
            let existing: String = tx
                .query_row(
                    "SELECT snapshot_json FROM configuration_revisions WHERE revision_id=?1",
                    [snapshot.revision_id().to_string()],
                    |row| row.get(0),
                )
                .map_err(not_found_or_storage)?;
            let existing: ConfigSnapshotDto =
                serde_json::from_str(&existing).map_err(codec_error)?;
            if existing != *snapshot {
                return Err(conflict(
                    "config_revision_conflict",
                    "the configuration revision is already bound to different safe configuration",
                ));
            }
        }
        Ok(())
    }

    fn project(
        tx: &sqlite::Transaction<'_>,
        session_id: SessionId,
    ) -> DtoResult<SessionProjectionDto> {
        let session = tx
            .query_row(
                "SELECT project_id, workspace_id, workspace_root, mode, config_revision_id, last_sequence FROM sessions WHERE session_id=?1",
                [session_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, i64>(5)?,
                    ))
                },
            )
            .map_err(not_found_or_storage)?;
        let active = tx
            .query_row(
                &format!("SELECT run_id, turn_id, status, config_revision_id FROM runs WHERE session_id=?1 AND status NOT IN ({TERMINAL_STATUSES})"),
                [session_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)),
            )
            .optional()
            .map_err(storage_error)?
            .map(|(run, turn, status, revision)| run_projection(session_id, &run, &turn, &status, &revision))
            .transpose()?;
        let mut statement = tx.prepare(
            "SELECT queued_turns.turn_id, turns.content, queued_turns.queue_ticket FROM queued_turns JOIN turns ON turns.session_id=queued_turns.session_id AND turns.turn_id=queued_turns.turn_id WHERE queued_turns.session_id=?1 ORDER BY queued_turns.queue_ticket",
        ).map_err(storage_error)?;
        let queue_rows = statement
            .query_map([session_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(storage_error)?;
        let queued = queue_rows
            .map(|item| {
                let (turn, content, ticket) = item.map_err(storage_error)?;
                QueuedTurnProjectionDto::new(
                    session_id,
                    TurnId::parse(&turn).map_err(codec_error)?,
                    content,
                    QueuePositionDto::new(
                        u64::try_from(ticket).map_err(|_| codec_error("invalid queue ticket"))?,
                    ),
                )
            })
            .collect::<DtoResult<Vec<_>>>()?;
        SessionProjectionDto::new(
            ProjectId::parse(&session.0).map_err(codec_error)?,
            session_id,
            WorkspaceId::parse(&session.1).map_err(codec_error)?,
            intention_domain::WorkspaceRootDto::parse(session.2).map_err(codec_error)?,
            parse_mode(&session.3)?,
            session
                .4
                .map(|id| ConfigRevisionId::parse(&id).map_err(codec_error))
                .transpose()?,
            active,
            queued,
            SessionEventSequenceDto::new(
                u64::try_from(session.5).map_err(|_| codec_error("invalid event sequence"))?,
            ),
        )
    }

    fn append(
        tx: &sqlite::Transaction<'_>,
        session_id: SessionId,
        sequence: u64,
        drafts: Vec<EventDraft>,
    ) -> DtoResult<Vec<EventEnvelopeDto<DomainEventDto>>> {
        let mut position = sequence;
        let mut events = Vec::with_capacity(drafts.len());
        for draft in drafts {
            position = position
                .checked_add(1)
                .ok_or_else(|| codec_error("event sequence overflow"))?;
            let position_sql =
                sqlite_integer(position, "event sequence is outside the SQLite range")?;
            let event = EventEnvelopeDto::new(
                EventMetadataDto::new(
                    SchemaVersionDto::new(1, 0),
                    EventId::new(),
                    session_id,
                    draft.run_id,
                    draft.turn_id,
                    SessionEventSequenceDto::new(position),
                    draft.occurred_at,
                ),
                draft.payload,
            );
            let encoded = serde_json::to_string(&event).map_err(codec_error)?;
            tx.execute(
                "INSERT INTO domain_events(event_id, session_id, sequence, envelope_json) VALUES (?1, ?2, ?3, ?4)",
                sqlite::params![event.event_id().to_string(), session_id.to_string(), position_sql, encoded],
            ).map_err(storage_error)?;
            events.push(event);
        }
        tx.execute(
            "UPDATE sessions SET last_sequence=?2 WHERE session_id=?1",
            sqlite::params![
                session_id.to_string(),
                sqlite_integer(position, "event sequence is outside the SQLite range")?
            ],
        )
        .map_err(storage_error)?;
        Ok(events)
    }

    fn snapshot(tx: &sqlite::Transaction<'_>, projection: &SessionProjectionDto) -> DtoResult<()> {
        let encoded = serde_json::to_string(projection).map_err(codec_error)?;
        let sequence = sqlite_integer(
            projection.at_sequence().value(),
            "snapshot sequence is outside the SQLite range",
        )?;
        tx.execute(
            "INSERT INTO session_snapshots(session_id, sequence, projection_json) VALUES (?1, ?2, ?3) ON CONFLICT(session_id) DO UPDATE SET sequence=excluded.sequence, projection_json=excluded.projection_json",
            sqlite::params![projection.session_id().to_string(), sequence, encoded],
        ).map_err(storage_error)?;
        let runs = {
            let mut statement = tx
                .prepare(
                    "SELECT run_id, turn_id, status, config_revision_id FROM runs WHERE session_id=?1",
                )
                .map_err(storage_error)?;
            let rows = statement
                .query_map([projection.session_id().to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(storage_error)?;
            rows.map(|row| row.map_err(storage_error))
                .collect::<DtoResult<Vec<_>>>()?
        };
        for (run_id, turn_id, status, revision) in runs {
            let run = run_projection(
                projection.session_id(),
                &run_id,
                &turn_id,
                &status,
                &revision,
            )?;
            let encoded = serde_json::to_string(&run).map_err(codec_error)?;
            tx.execute(
                "INSERT INTO run_snapshots(run_id, session_id, sequence, projection_json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(run_id) DO UPDATE SET sequence=excluded.sequence, projection_json=excluded.projection_json",
                sqlite::params![run.run_id().to_string(), projection.session_id().to_string(), sequence, encoded],
            )
            .map_err(storage_error)?;
        }
        Self::snapshot_model_runs(tx, projection)?;
        Ok(())
    }

    fn snapshot_model_runs(
        tx: &sqlite::Transaction<'_>,
        projection: &SessionProjectionDto,
    ) -> DtoResult<()> {
        let runs = {
            let mut statement = tx
                .prepare(
                    "SELECT run_id, turn_id, status, config_revision_id FROM runs WHERE session_id=?1",
                )
                .map_err(storage_error)?;
            let rows = statement
                .query_map([projection.session_id().to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(storage_error)?;
            rows.map(|row| row.map_err(storage_error))
                .collect::<DtoResult<Vec<_>>>()?
        };
        for (run_id, turn_id, status, revision) in runs {
            let run = run_projection(
                projection.session_id(),
                &run_id,
                &turn_id,
                &status,
                &revision,
            )?;
            let cursor = current_run_cursor(tx, run.run_id())?;
            let model = model_projection(tx, run, cursor)?;
            let snapshot = RunSnapshotDto::new(
                projection.session_id(),
                run.run_id(),
                projection.at_sequence(),
                model,
            )?;
            let encoded = serde_json::to_string(&snapshot).map_err(codec_error)?;
            tx.execute(
                "INSERT INTO model_run_snapshots(run_id, session_id, sequence, cursor, snapshot_json) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(run_id) DO UPDATE SET sequence=excluded.sequence, cursor=excluded.cursor, snapshot_json=excluded.snapshot_json",
                sqlite::params![
                    run.run_id().to_string(),
                    projection.session_id().to_string(),
                    sqlite_integer(projection.at_sequence().value(), "snapshot sequence is outside the SQLite range")?,
                    sqlite_integer(cursor.value(), "run event cursor is outside the SQLite range")?,
                    encoded,
                ],
            )
            .map_err(storage_error)?;
        }
        Ok(())
    }

    fn promote_oldest_queued_turn(
        tx: &sqlite::Transaction<'_>,
        session_id: SessionId,
        occurred_at: TimestampDto,
    ) -> DtoResult<Vec<EventDraft>> {
        let queued_selection = tx
            .query_row(
                "SELECT queued_turns.turn_id, turns.proposed_run_id, turns.config_revision_id FROM queued_turns JOIN turns ON turns.session_id=queued_turns.session_id AND turns.turn_id=queued_turns.turn_id WHERE queued_turns.session_id=?1 ORDER BY queued_turns.queue_ticket ASC LIMIT 1",
                [session_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
        let Some((turn_id, run_id, revision_id)) = queued_selection else {
            return Ok(Vec::new());
        };
        let promoted_turn_id = TurnId::parse(&turn_id).map_err(codec_error)?;
        let promoted_run_id = RunId::parse(&run_id).map_err(codec_error)?;
        let config_revision_id = ConfigRevisionId::parse(&revision_id).map_err(codec_error)?;
        tx.execute(
            "DELETE FROM queued_turns WHERE session_id=?1 AND turn_id=?2",
            sqlite::params![session_id.to_string(), promoted_turn_id.to_string()],
        )
        .map_err(storage_error)?;
        tx.execute("UPDATE turns SET outcome='started',queue_ticket=NULL WHERE session_id=?1 AND turn_id=?2",sqlite::params![session_id.to_string(),promoted_turn_id.to_string()]).map_err(storage_error)?;
        tx.execute("INSERT INTO runs(run_id,session_id,turn_id,status,config_revision_id) VALUES (?1,?2,?3,'starting',?4)",sqlite::params![promoted_run_id.to_string(),session_id.to_string(),promoted_turn_id.to_string(),config_revision_id.to_string()]).map_err(storage_error)?;
        Ok(vec![EventDraft::new(
            Some(promoted_run_id),
            Some(promoted_turn_id),
            occurred_at,
            DomainEventDto::RunStarted(RunStartedEventDto::new(
                session_id,
                promoted_run_id,
                promoted_turn_id,
                config_revision_id,
                occurred_at,
            )),
        )])
    }

    fn ensure_run_cursors(tx: &sqlite::Transaction<'_>, session_id: SessionId) -> DtoResult<()> {
        tx.execute(
            "INSERT INTO run_cursors(run_id, session_id, cursor) SELECT run_id, session_id, 0 FROM runs WHERE session_id=?1 ON CONFLICT(run_id) DO NOTHING",
            [session_id.to_string()],
        )
        .map_err(storage_error)?;
        Ok(())
    }

    fn finish(
        &self,
        tx: sqlite::Transaction<'_>,
        session_id: SessionId,
        events: Vec<EventEnvelopeDto<DomainEventDto>>,
        outcome: Option<AcceptedTurnOutcomeDto>,
    ) -> DtoResult<CommittedChangeDto> {
        self.fault(FaultPoint::Events)?;
        let projection = Self::project(&tx, session_id)?;
        self.fault(FaultPoint::Projection)?;
        Self::snapshot(&tx, &projection)?;
        self.fault(FaultPoint::Snapshot)?;
        let position = projection.at_sequence();
        let change = CommittedChangeDto::new(projection, position, events, outcome)?;
        tx.commit().map_err(storage_error)?;
        Ok(change)
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
    fn create_session(&self, input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        let command = input.command();
        let session_id = command.session_id();
        immediate_transaction!(self, |tx| {
            if tx
                .query_row(
                    "SELECT 1 FROM sessions WHERE session_id=?1",
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
            tx.execute(
                "INSERT INTO projects(project_id) VALUES (?1) ON CONFLICT(project_id) DO NOTHING",
                [command.project_id().to_string()],
            )
            .map_err(storage_error)?;
            tx.execute("INSERT INTO workspace_roots(workspace_id, workspace_root) VALUES (?1, ?2) ON CONFLICT(workspace_id) DO NOTHING", sqlite::params![command.workspace_id().to_string(), command.workspace_root().as_str()]).map_err(storage_error)?;
            let stored_workspace_root: String = tx
                .query_row(
                    "SELECT workspace_root FROM workspace_roots WHERE workspace_id=?1",
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
            tx.execute("INSERT INTO sessions(session_id,project_id,workspace_id,workspace_root,mode,config_revision_id,last_sequence,next_queue_ticket) VALUES (?1,?2,?3,?4,?5,NULL,0,0)", sqlite::params![session_id.to_string(), command.project_id().to_string(), command.workspace_id().to_string(), stored_workspace_root, mode_name(command.mode())]).map_err(storage_error)?;
            let events = Self::append(
                &tx,
                session_id,
                0,
                vec![EventDraft::new(
                    None,
                    None,
                    input.occurred_at(),
                    DomainEventDto::SessionCreated(SessionCreatedEventDto::new(
                        command.project_id(),
                        session_id,
                        command.workspace_id(),
                        command.workspace_root().clone(),
                        command.mode(),
                        input.occurred_at(),
                    )),
                )],
            )?;
            Self::ensure_run_cursors(&tx, session_id)?;
            self.finish(tx, session_id, events, None)
        })
    }

    fn accept_user_turn(&self, input: AcceptUserTurnInputDto) -> DtoResult<CommittedChangeDto> {
        let session_id = input.session_id();
        immediate_transaction!(self, |tx| {
            let existing = tx.query_row(
            "SELECT content,outcome,queue_ticket,proposed_run_id,config_revision_id FROM turns WHERE session_id=?1 AND turn_id=?2",
            sqlite::params![session_id.to_string(), input.turn_id().to_string()],
            |row| Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?,row.get::<_, Option<i64>>(2)?,row.get::<_, String>(3)?,row.get::<_, String>(4)?)),
        ).optional().map_err(storage_error)?;
            if let Some((content, outcome, ticket, run, revision)) = existing {
                if content != input.content()
                    || run != input.proposed_run_id().to_string()
                    || revision != input.config_revision_id().to_string()
                {
                    return Err(conflict(
                        "turn_idempotency_conflict",
                        "the accepted turn identity has different durable content",
                    ));
                }
                let projection = Self::project(&tx, session_id)?;
                let outcome = if outcome == "started" {
                    Some(AcceptedTurnOutcomeDto::Started(run_projection(
                        session_id,
                        &run,
                        &input.turn_id().to_string(),
                        "starting",
                        &revision,
                    )?))
                } else {
                    Some(AcceptedTurnOutcomeDto::Queued(QueuePositionDto::new(
                        u64::try_from(ticket.ok_or_else(|| codec_error("missing queue ticket"))?)
                            .map_err(|_| codec_error("invalid queue ticket"))?,
                    )))
                };
                let result = CommittedChangeDto::new(
                    projection.clone(),
                    projection.at_sequence(),
                    Vec::new(),
                    outcome,
                )?;
                tx.commit().map_err(storage_error)?;
                return Ok(result);
            }
            let sequence = sequence(&tx, session_id)?;
            Self::store_config(&tx, input.config_snapshot())?;
            let active = tx
            .query_row(
                &format!(
                    "SELECT 1 FROM runs WHERE session_id=?1 AND status NOT IN ({TERMINAL_STATUSES})"
                ),
                [session_id.to_string()],
                |_| Ok(()),
            )
            .optional()
            .map_err(storage_error)?;
            let queued_exists = tx
                .query_row(
                    "SELECT 1 FROM queued_turns WHERE session_id=?1 LIMIT 1",
                    [session_id.to_string()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(storage_error)?
                .is_some();
            if active.is_none() && queued_exists {
                return Err(conflict(
                    "queue_promotion_required",
                    "durable queued turns must promote before a new run can start",
                ));
            }
            let (outcome, drafts) = if active.is_none() {
                tx.execute("INSERT INTO turns(session_id,turn_id,content,proposed_run_id,config_revision_id,outcome,queue_ticket) VALUES (?1,?2,?3,?4,?5,'started',NULL)", sqlite::params![session_id.to_string(), input.turn_id().to_string(), input.content(), input.proposed_run_id().to_string(), input.config_revision_id().to_string()]).map_err(storage_error)?;
                tx.execute("INSERT INTO runs(run_id,session_id,turn_id,status,config_revision_id) VALUES (?1,?2,?3,'starting',?4)", sqlite::params![input.proposed_run_id().to_string(),session_id.to_string(),input.turn_id().to_string(),input.config_revision_id().to_string()]).map_err(storage_error)?;
                (
                    AcceptedTurnOutcomeDto::Started(RunProjectionDto::new(
                        session_id,
                        input.proposed_run_id(),
                        input.turn_id(),
                        RunStatusDto::Starting,
                        input.config_revision_id(),
                    )),
                    vec![
                        EventDraft::new(
                            None,
                            Some(input.turn_id()),
                            input.occurred_at(),
                            DomainEventDto::UserTurnAccepted(UserTurnAcceptedEventDto::new(
                                session_id,
                                input.turn_id(),
                                input.content(),
                                input.occurred_at(),
                            )?),
                        ),
                        EventDraft::new(
                            Some(input.proposed_run_id()),
                            Some(input.turn_id()),
                            input.occurred_at(),
                            DomainEventDto::RunStarted(RunStartedEventDto::new(
                                session_id,
                                input.proposed_run_id(),
                                input.turn_id(),
                                input.config_revision_id(),
                                input.occurred_at(),
                            )),
                        ),
                    ],
                )
            } else {
                let ticket: i64 = tx
                    .query_row(
                        "SELECT next_queue_ticket FROM sessions WHERE session_id=?1",
                        [session_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(not_found_or_storage)?;
                tx.execute(
                    "UPDATE sessions SET next_queue_ticket=next_queue_ticket+1 WHERE session_id=?1",
                    [session_id.to_string()],
                )
                .map_err(storage_error)?;
                tx.execute("INSERT INTO turns(session_id,turn_id,content,proposed_run_id,config_revision_id,outcome,queue_ticket) VALUES (?1,?2,?3,?4,?5,'queued',?6)", sqlite::params![session_id.to_string(), input.turn_id().to_string(),input.content(),input.proposed_run_id().to_string(),input.config_revision_id().to_string(),ticket]).map_err(storage_error)?;
                tx.execute(
                    "INSERT INTO queued_turns(session_id,turn_id,queue_ticket) VALUES (?1,?2,?3)",
                    sqlite::params![session_id.to_string(), input.turn_id().to_string(), ticket],
                )
                .map_err(storage_error)?;
                let position = QueuePositionDto::new(
                    u64::try_from(ticket).map_err(|_| codec_error("invalid queue ticket"))?,
                );
                (
                    AcceptedTurnOutcomeDto::Queued(position),
                    vec![
                        EventDraft::new(
                            None,
                            Some(input.turn_id()),
                            input.occurred_at(),
                            DomainEventDto::UserTurnAccepted(UserTurnAcceptedEventDto::new(
                                session_id,
                                input.turn_id(),
                                input.content(),
                                input.occurred_at(),
                            )?),
                        ),
                        EventDraft::new(
                            None,
                            Some(input.turn_id()),
                            input.occurred_at(),
                            DomainEventDto::UserTurnQueued(UserTurnQueuedEventDto::new(
                                session_id,
                                input.turn_id(),
                                position,
                                input.occurred_at(),
                            )),
                        ),
                    ],
                )
            };
            let events = Self::append(&tx, session_id, sequence, drafts)?;
            Self::ensure_run_cursors(&tx, session_id)?;
            self.finish(tx, session_id, events, Some(outcome))
        })
    }

    fn remove_queued_turn(&self, input: RemoveQueuedTurnInputDto) -> DtoResult<CommittedChangeDto> {
        let command = input.command();
        let session_id = command.session_id();
        immediate_transaction!(self, |tx| {
            let position = sequence(&tx, session_id)?;
            if tx
                .execute(
                    "DELETE FROM queued_turns WHERE session_id=?1 AND turn_id=?2",
                    sqlite::params![session_id.to_string(), command.turn_id().to_string()],
                )
                .map_err(storage_error)?
                != 1
            {
                return Err(not_found(
                    "queued_turn_not_found",
                    "the requested queued turn does not exist",
                ));
            }
            let events = Self::append(
                &tx,
                session_id,
                position,
                vec![EventDraft::new(
                    None,
                    Some(command.turn_id()),
                    input.occurred_at(),
                    DomainEventDto::QueuedTurnRemoved(QueuedTurnRemovedEventDto::new(
                        session_id,
                        command.turn_id(),
                        input.occurred_at(),
                    )),
                )],
            )?;
            self.finish(tx, session_id, events, None)
        })
    }

    fn transition_run(&self, input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
        let session_id = input.session_id();
        immediate_transaction!(self, |tx| {
            let position = sequence(&tx, session_id)?;
            let current: String = tx
                .query_row(
                    "SELECT status FROM runs WHERE session_id=?1 AND run_id=?2",
                    sqlite::params![session_id.to_string(), input.run_id().to_string()],
                    |row| row.get(0),
                )
                .map_err(not_found_or_storage)?;
            validate_run_status_transition(parse_status(&current)?, input.status())?;
            tx.execute(
                "UPDATE runs SET status=?3 WHERE session_id=?1 AND run_id=?2",
                sqlite::params![
                    session_id.to_string(),
                    input.run_id().to_string(),
                    status_name(input.status())
                ],
            )
            .map_err(storage_error)?;
            let mut drafts = vec![EventDraft::new(
                Some(input.run_id()),
                None,
                input.occurred_at(),
                DomainEventDto::RunStatusChanged(RunStatusChangedEventDto::new(
                    session_id,
                    input.run_id(),
                    input.status(),
                    input.occurred_at(),
                )),
            )];
            if input.status().is_terminal() {
                drafts.extend(Self::promote_oldest_queued_turn(
                    &tx,
                    session_id,
                    input.occurred_at(),
                )?);
            }
            let events = Self::append(&tx, session_id, position, drafts)?;
            Self::ensure_run_cursors(&tx, session_id)?;
            self.finish(tx, session_id, events, None)
        })
    }

    fn append_model_run_facts(
        &self,
        input: AppendModelRunFactsInputDto,
    ) -> DtoResult<AppendModelRunFactsOutcomeDto> {
        let session_id = input.session_id();
        let run_id = input.run_id();
        immediate_transaction!(self, |tx| {
            let run = load_scoped_run(&tx, session_id, run_id)?;
            if run.status().is_terminal() {
                return Err(invalid_run_cursor());
            }
            let cursor = current_run_cursor(&tx, run_id)?;
            if cursor != input.expected_cursor() {
                return Err(cursor_conflict());
            }
            let mut next_cursor = cursor.value();
            let mut drafts = Vec::with_capacity(input.facts().len());
            let mut facts = Vec::with_capacity(input.facts().len());
            for input_fact in input.facts() {
                next_cursor = next_cursor.checked_add(1).ok_or_else(invalid_run_cursor)?;
                let fact =
                    ModelRunFactDto::new(RunEventCursorDto::new(next_cursor), input_fact.clone())?;
                let encoded = serde_json::to_vec(&fact).map_err(codec_error)?;
                if encoded.len() > MAX_CANONICAL_FACT_BYTES {
                    return Err(fact_too_large());
                }
                let payload =
                    model_fact_event(session_id, run_id, fact.clone(), input.occurred_at());
                drafts.push(EventDraft::new(
                    Some(run_id),
                    Some(run.turn_id()),
                    input.occurred_at(),
                    payload,
                ));
                facts.push(fact);
            }
            if let Some(status) = input.status() {
                validate_run_status_transition(run.status(), status)?;
                tx.execute(
                    "UPDATE runs SET status=?3 WHERE session_id=?1 AND run_id=?2",
                    sqlite::params![
                        session_id.to_string(),
                        run_id.to_string(),
                        status_name(status),
                    ],
                )
                .map_err(storage_error)?;
                drafts.push(EventDraft::new(
                    Some(run_id),
                    Some(run.turn_id()),
                    input.occurred_at(),
                    DomainEventDto::RunStatusChanged(RunStatusChangedEventDto::new(
                        session_id,
                        run_id,
                        status,
                        input.occurred_at(),
                    )),
                ));
                if status.is_terminal() {
                    drafts.extend(Self::promote_oldest_queued_turn(
                        &tx,
                        session_id,
                        input.occurred_at(),
                    )?);
                }
            }
            let position = sequence(&tx, session_id)?;
            let events = Self::append(&tx, session_id, position, drafts)?;
            Self::ensure_run_cursors(&tx, session_id)?;
            for (fact, event) in facts.iter().zip(events.iter()) {
                tx.execute(
                    "INSERT INTO model_run_facts(run_id, cursor, event_id) VALUES (?1, ?2, ?3)",
                    sqlite::params![
                        run_id.to_string(),
                        sqlite_integer(
                            fact.cursor().value(),
                            "run event cursor is outside the SQLite range"
                        )?,
                        event.event_id().to_string(),
                    ],
                )
                .map_err(storage_error)?;
            }
            tx.execute(
                "UPDATE run_cursors SET cursor=?2 WHERE run_id=?1 AND session_id=?3",
                sqlite::params![
                    run_id.to_string(),
                    sqlite_integer(next_cursor, "run event cursor is outside the SQLite range")?,
                    session_id.to_string(),
                ],
            )
            .map_err(storage_error)?;
            self.fault(FaultPoint::ModelFacts)?;
            let projection = Self::project(&tx, session_id)?;
            self.fault(FaultPoint::Projection)?;
            Self::snapshot(&tx, &projection)?;
            self.fault(FaultPoint::Snapshot)?;
            let snapshot = load_model_run_snapshot(&tx, session_id, run_id)?;
            let outcome = AppendModelRunFactsOutcomeDto::new(
                RunEventCursorDto::new(next_cursor),
                snapshot,
                facts,
            )?;
            tx.commit().map_err(storage_error)?;
            Ok(outcome)
        })
    }

    fn load_run_config_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        let connection = self.connection()?;
        let revision_id: String = connection
            .query_row(
                "SELECT config_revision_id FROM runs WHERE session_id=?1 AND run_id=?2",
                sqlite::params![session_id.to_string(), run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| run_configuration_not_found())?;
        let snapshot: String = connection
            .query_row(
                "SELECT snapshot_json FROM configuration_revisions WHERE revision_id=?1",
                [revision_id],
                |row| row.get(0),
            )
            .map_err(|_| run_configuration_unavailable())?;
        drop(connection);
        serde_json::from_str(&snapshot).map_err(|_| run_configuration_unavailable())
    }

    fn load_current_run_replay(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        let connection = self.connection()?;
        let snapshot = load_model_run_snapshot(&connection, session_id, run_id)?;
        drop(connection);
        RunReplayDto::new(
            snapshot.clone(),
            RunEventTailPageDto::empty(session_id, run_id, snapshot.cursor()),
        )
    }

    fn load_run_tail(
        &self,
        session_id: SessionId,
        run_id: RunId,
        after_cursor: RunEventCursorDto,
    ) -> DtoResult<RunEventTailPageDto> {
        let connection = self.connection()?;
        let _run = load_scoped_run(&connection, session_id, run_id)?;
        let current_cursor = current_run_cursor(&connection, run_id)?;
        if after_cursor > current_cursor {
            return Err(invalid_run_cursor());
        }
        let after = sqlite_integer(
            after_cursor.value(),
            "run event cursor is outside the SQLite range",
        )
        .map_err(|_| invalid_run_cursor())?;
        let mut statement = connection
            .prepare(
                "SELECT domain_events.envelope_json FROM model_run_facts JOIN domain_events ON domain_events.event_id=model_run_facts.event_id WHERE model_run_facts.run_id=?1 AND model_run_facts.cursor>?2 ORDER BY model_run_facts.cursor LIMIT ?3",
            )
            .map_err(|_| run_history_unavailable())?;
        let rows = statement
            .query_map(
                sqlite::params![
                    run_id.to_string(),
                    after,
                    i64::try_from(MAX_TAIL_FACTS + 1).map_err(|_| invalid_run_cursor())?
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|_| run_history_unavailable())?;
        let mut facts = Vec::new();
        let mut byte_count = 0_usize;
        let mut has_more = false;
        for row in rows {
            let encoded = row.map_err(|_| run_history_unavailable())?;
            let event: EventEnvelopeDto<DomainEventDto> =
                serde_json::from_str(&encoded).map_err(|_| run_history_unavailable())?;
            let fact = domain_model_fact(event.payload()).ok_or_else(run_history_unavailable)?;
            if event.session_id() != session_id
                || event.run_id() != Some(run_id)
                || fact.fact().cursor().value() <= after_cursor.value()
            {
                return Err(run_history_unavailable());
            }
            let canonical =
                serde_json::to_vec(fact.fact()).map_err(|_| run_history_unavailable())?;
            if facts.len() == MAX_TAIL_FACTS
                || byte_count.saturating_add(canonical.len()) > MAX_TAIL_CANONICAL_BYTES
            {
                has_more = true;
                break;
            }
            byte_count += canonical.len();
            facts.push(fact.fact().clone());
        }
        let next = facts.last().map_or(after_cursor, ModelRunFactDto::cursor);
        drop(statement);
        drop(connection);
        RunEventTailPageDto::new(session_id, run_id, after_cursor, facts, next, has_more)
    }

    fn recover_unfinished_runs(
        &self,
        input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<CommittedChangeDto>> {
        let unfinished = {
            let connection = self.connection()?;
            let mut statement = connection
                .prepare(&format!(
                    "SELECT session_id,run_id FROM runs WHERE status NOT IN ({TERMINAL_STATUSES}) ORDER BY session_id,run_id"
                ))
                .map_err(storage_error)?;
            let unfinished = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(storage_error)?
                .map(|row| row.map_err(storage_error))
                .collect::<DtoResult<Vec<_>>>()?;
            drop(statement);
            drop(connection);
            unfinished
        };
        let mut changes = Vec::with_capacity(unfinished.len());
        for (session, run) in unfinished {
            let session_id = SessionId::parse(&session).map_err(codec_error)?;
            let run_id = RunId::parse(&run).map_err(codec_error)?;
            changes.push(self.transition_run(TransitionRunInputDto::new(
                session_id,
                run_id,
                RunStatusDto::Interrupted,
                input.recovered_at(),
            ))?);
        }
        Ok(changes)
    }

    fn load_session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        let encoded = {
            let connection = self.connection()?;
            let encoded: String = connection
                .query_row(
                    "SELECT projection_json FROM session_snapshots WHERE session_id=?1",
                    [session_id.to_string()],
                    |row| row.get(0),
                )
                .map_err(not_found_or_storage)?;
            drop(connection);
            encoded
        };
        serde_json::from_str(&encoded).map_err(codec_error)
    }
    fn load_tail(
        &self,
        session_id: SessionId,
        after: SessionEventSequenceDto,
    ) -> DtoResult<Vec<EventEnvelopeDto<DomainEventDto>>> {
        let events = {
            let connection = self.connection()?;
            let known_session = connection
                .query_row(
                    "SELECT 1 FROM sessions WHERE session_id=?1",
                    [session_id.to_string()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(storage_error)?;
            if known_session.is_none() {
                return Err(not_found(
                    "storage_record_not_found",
                    "the requested durable record does not exist",
                ));
            }
            let last_sequence: i64 = connection
                .query_row(
                    "SELECT last_sequence FROM sessions WHERE session_id=?1",
                    [session_id.to_string()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let last_sequence = u64::try_from(last_sequence)
                .map_err(|_| codec_error("invalid durable event sequence"))?;
            if after.value() > last_sequence {
                return Err(ErrorDto::validation(
                    "invalid_event_tail_position",
                    "event tail position is beyond the durable session sequence",
                ));
            }
            let after = i64::try_from(after.value()).map_err(|_| {
                ErrorDto::validation(
                    "invalid_event_tail_position",
                    "event tail position is outside the supported durable range",
                )
            })?;
            let mut statement = connection
                .prepare("SELECT envelope_json FROM domain_events WHERE session_id=?1 AND sequence>?2 ORDER BY sequence")
                .map_err(storage_error)?;
            let events = statement
                .query_map(sqlite::params![session_id.to_string(), after], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(storage_error)?
                .map(|row| {
                    let encoded = row.map_err(storage_error)?;
                    let event: EventEnvelopeDto<DomainEventDto> =
                        serde_json::from_str(&encoded).map_err(codec_error)?;
                    if event.session_id() != session_id {
                        Err(codec_error("event session mismatch"))
                    } else {
                        Ok(event)
                    }
                })
                .collect::<DtoResult<Vec<_>>>()?;
            drop(statement);
            drop(connection);
            events
        };
        Ok(events)
    }
    fn accept_configuration_revision(&self, snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            Self::store_config(&tx, &snapshot)?;
            tx.commit().map_err(storage_error)
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultPoint {
    Events,
    ModelFacts,
    Projection,
    Snapshot,
}

struct EventDraft {
    run_id: Option<RunId>,
    turn_id: Option<TurnId>,
    occurred_at: TimestampDto,
    payload: DomainEventDto,
}
impl EventDraft {
    const fn new(
        run_id: Option<RunId>,
        turn_id: Option<TurnId>,
        occurred_at: TimestampDto,
        payload: DomainEventDto,
    ) -> Self {
        Self {
            run_id,
            turn_id,
            occurred_at,
            payload,
        }
    }
}
fn current_run_cursor(
    connection: &sqlite::Connection,
    run_id: RunId,
) -> DtoResult<RunEventCursorDto> {
    let cursor: i64 = connection
        .query_row(
            "SELECT cursor FROM run_cursors WHERE run_id=?1",
            [run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|_| run_history_unavailable())?;
    Ok(RunEventCursorDto::new(
        u64::try_from(cursor).map_err(|_| run_history_unavailable())?,
    ))
}

fn load_scoped_run(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<RunProjectionDto> {
    let row = connection
        .query_row(
            "SELECT turn_id, status, config_revision_id FROM runs WHERE session_id=?1 AND run_id=?2",
            sqlite::params![session_id.to_string(), run_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|_| run_replay_not_found())?;
    run_projection(session_id, &run_id.to_string(), &row.0, &row.1, &row.2)
        .map_err(|_| run_history_unavailable())
}

const fn domain_model_fact(event: &DomainEventDto) -> Option<&ModelRunFactEventDto> {
    match event {
        DomainEventDto::ProviderAttemptStarted(value)
        | DomainEventDto::ProviderAttemptFailed(value)
        | DomainEventDto::RetryScheduled(value)
        | DomainEventDto::AssistantContentAppended(value)
        | DomainEventDto::ReasoningDeltaRecorded(value)
        | DomainEventDto::UsageRecorded(value)
        | DomainEventDto::ToolCallRecorded(value)
        | DomainEventDto::Finished(value)
        | DomainEventDto::Failed(value) => Some(value),
        _ => None,
    }
}

const fn model_fact_event(
    session_id: SessionId,
    run_id: RunId,
    fact: ModelRunFactDto,
    occurred_at: TimestampDto,
) -> DomainEventDto {
    let event = ModelRunFactEventDto::new(session_id, run_id, fact, occurred_at);
    match event.fact().kind() {
        intention_domain::ModelRunFactKindDto::ProviderAttemptStarted => {
            DomainEventDto::ProviderAttemptStarted(event)
        }
        intention_domain::ModelRunFactKindDto::ProviderAttemptFailed => {
            DomainEventDto::ProviderAttemptFailed(event)
        }
        intention_domain::ModelRunFactKindDto::RetryScheduled => {
            DomainEventDto::RetryScheduled(event)
        }
        intention_domain::ModelRunFactKindDto::AssistantContentAppended => {
            DomainEventDto::AssistantContentAppended(event)
        }
        intention_domain::ModelRunFactKindDto::ReasoningDeltaRecorded => {
            DomainEventDto::ReasoningDeltaRecorded(event)
        }
        intention_domain::ModelRunFactKindDto::UsageRecorded => {
            DomainEventDto::UsageRecorded(event)
        }
        intention_domain::ModelRunFactKindDto::ToolCallRecorded => {
            DomainEventDto::ToolCallRecorded(event)
        }
        intention_domain::ModelRunFactKindDto::Finished => DomainEventDto::Finished(event),
        intention_domain::ModelRunFactKindDto::Failed => DomainEventDto::Failed(event),
    }
}

fn model_projection(
    connection: &sqlite::Connection,
    run: RunProjectionDto,
    cursor: RunEventCursorDto,
) -> DtoResult<ModelRunProjectionDto> {
    let mut statement = connection
        .prepare(
            "SELECT domain_events.envelope_json FROM model_run_facts JOIN domain_events ON domain_events.event_id=model_run_facts.event_id WHERE model_run_facts.run_id=?1 ORDER BY model_run_facts.cursor",
        )
        .map_err(|_| run_history_unavailable())?;
    let rows = statement
        .query_map([run.run_id().to_string()], |row| row.get::<_, String>(0))
        .map_err(|_| run_history_unavailable())?;
    let mut assistant_turn_id = None;
    let mut assistant_content = String::new();
    let mut last_assistant_turn = None;
    let mut usage = None;
    let mut finish_reason = None;
    let mut failure = None;
    for row in rows {
        let encoded = row.map_err(|_| run_history_unavailable())?;
        let event: EventEnvelopeDto<DomainEventDto> =
            serde_json::from_str(&encoded).map_err(|_| run_history_unavailable())?;
        let fact = domain_model_fact(event.payload()).ok_or_else(run_history_unavailable)?;
        match fact.fact().input() {
            ModelRunFactInputDto::AssistantContentAppended {
                assistant_turn_id: turn,
                content,
            } => {
                if last_assistant_turn != Some(*turn) {
                    assistant_content.clear();
                    last_assistant_turn = Some(*turn);
                }
                assistant_turn_id = Some(*turn);
                assistant_content.push_str(content);
            }
            ModelRunFactInputDto::UsageRecorded { usage: value } => usage = Some(*value),
            ModelRunFactInputDto::Finished { reason } => finish_reason = Some(*reason),
            ModelRunFactInputDto::Failed { failure: value } => failure = Some(value.clone()),
            _ => {}
        }
    }
    ModelRunProjectionDto::new(
        run,
        cursor,
        assistant_turn_id,
        assistant_content,
        usage,
        finish_reason,
        failure,
    )
    .map_err(|_| run_history_unavailable())
}

fn load_model_run_snapshot(
    connection: &sqlite::Connection,
    session_id: SessionId,
    run_id: RunId,
) -> DtoResult<RunSnapshotDto> {
    let encoded: String = connection
        .query_row(
            "SELECT snapshot_json FROM model_run_snapshots WHERE session_id=?1 AND run_id=?2",
            sqlite::params![session_id.to_string(), run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|_| run_replay_not_found())?;
    serde_json::from_str(&encoded).map_err(|_| run_history_unavailable())
}

fn sequence(tx: &sqlite::Transaction<'_>, session: SessionId) -> DtoResult<u64> {
    let value: i64 = tx
        .query_row(
            "SELECT last_sequence FROM sessions WHERE session_id=?1",
            [session.to_string()],
            |row| row.get(0),
        )
        .map_err(not_found_or_storage)?;
    u64::try_from(value).map_err(|_| codec_error("invalid event sequence"))
}
fn sqlite_integer(value: u64, message: &'static str) -> DtoResult<i64> {
    i64::try_from(value).map_err(|_| codec_error(message))
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
const fn mode_name(value: intention_domain::RunModeDto) -> &'static str {
    match value {
        intention_domain::RunModeDto::Plan => "plan",
        intention_domain::RunModeDto::Build => "build",
    }
}
fn parse_mode(value: &str) -> DtoResult<intention_domain::RunModeDto> {
    match value {
        "plan" => Ok(intention_domain::RunModeDto::Plan),
        "build" => Ok(intention_domain::RunModeDto::Build),
        _ => Err(codec_error("invalid durable mode")),
    }
}
const fn status_name(value: RunStatusDto) -> &'static str {
    match value {
        RunStatusDto::Queued => "queued",
        RunStatusDto::Starting => "starting",
        RunStatusDto::Running => "running",
        RunStatusDto::WaitingInput => "waiting_input",
        RunStatusDto::Completing => "completing",
        RunStatusDto::Cancelling => "cancelling",
        RunStatusDto::Completed => "completed",
        RunStatusDto::Cancelled => "cancelled",
        RunStatusDto::Failed => "failed",
        RunStatusDto::Interrupted => "interrupted",
    }
}
fn parse_status(value: &str) -> DtoResult<RunStatusDto> {
    match value {
        "queued" => Ok(RunStatusDto::Queued),
        "starting" => Ok(RunStatusDto::Starting),
        "running" => Ok(RunStatusDto::Running),
        "waiting_input" => Ok(RunStatusDto::WaitingInput),
        "completing" => Ok(RunStatusDto::Completing),
        "cancelling" => Ok(RunStatusDto::Cancelling),
        "completed" => Ok(RunStatusDto::Completed),
        "cancelled" => Ok(RunStatusDto::Cancelled),
        "failed" => Ok(RunStatusDto::Failed),
        "interrupted" => Ok(RunStatusDto::Interrupted),
        _ => Err(codec_error("invalid durable status")),
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

fn run_history_unavailable() -> ErrorDto {
    ErrorDto::new(
        "run_history_unavailable",
        ErrorCategoryDto::Unavailable,
        "the durable run history is unavailable",
        ErrorRetryDto::Manual,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn run_replay_not_found() -> ErrorDto {
    ErrorDto::new(
        "run_replay_not_found",
        ErrorCategoryDto::NotFound,
        "the requested durable run replay does not exist",
        ErrorRetryDto::Never,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn invalid_run_cursor() -> ErrorDto {
    ErrorDto::new(
        "invalid_run_event_cursor",
        ErrorCategoryDto::Validation,
        "run event cursor is not valid for the requested durable history",
        ErrorRetryDto::Never,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn cursor_conflict() -> ErrorDto {
    ErrorDto::new(
        "run_event_cursor_conflict",
        ErrorCategoryDto::Conflict,
        "run event cursor no longer matches durable history",
        ErrorRetryDto::Immediate,
        None,
    )
    .unwrap_or_else(|_| unavailable())
}

fn fact_too_large() -> ErrorDto {
    ErrorDto::new(
        "run_fact_too_large",
        ErrorCategoryDto::Validation,
        "individual durable model fact exceeds the canonical size limit",
        ErrorRetryDto::Never,
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
        not_found(
            "storage_record_not_found",
            "the requested durable record does not exist",
        )
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Focused SQLite fixtures use expect for test diagnostics."
    )]
    use super::*;
    use intention_config::{ConfigPathDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto};
    use intention_domain::{
        CreateSessionCommandDto, RunEventCursorDto, RunModeDto, WorkspaceRootDto,
    };
    use intention_storage::{
        AcceptUserTurnInputDto, AppendModelRunFactsInputDto, CreateSessionInputDto,
        StorageRepositoryDto, TransitionRunInputDto,
    };
    use intention_types::{
        ConfigRevisionId, ProjectId, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId,
        TimestampDto, TurnId, UsageDto, WorkspaceId,
    };

    fn fixture_time(value: i64) -> TimestampDto {
        TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
    }

    fn fixture_snapshot() -> ConfigSnapshotDto {
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-storage-sqlite-unit.toml")
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
            "{}/intention-storage-sqlite-fault-{}.db",
            std::env::temp_dir().display(),
            EventId::new()
        ))
        .expect("temporary location is absolute")
    }

    fn raw_snapshot_rows(location: &SqliteDatabaseLocationDto) -> Vec<(String, i64, String)> {
        let connection =
            sqlite::Connection::open(&location.0).expect("database reopens for inspection");
        let mut statement = connection
            .prepare("SELECT run_id, sequence, projection_json FROM run_snapshots ORDER BY run_id")
            .expect("run snapshot query prepares");
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("run snapshot query executes")
            .map(|row| row.expect("run snapshot row reads"))
            .collect()
    }

    fn raw_json_columns(location: &SqliteDatabaseLocationDto) -> Vec<String> {
        let connection =
            sqlite::Connection::open(&location.0).expect("database reopens for inspection");
        [
            "SELECT snapshot_json FROM configuration_revisions",
            "SELECT projection_json FROM session_snapshots",
            "SELECT projection_json FROM run_snapshots",
            "SELECT envelope_json FROM domain_events",
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

    fn raw_model_snapshot_rows(
        location: &SqliteDatabaseLocationDto,
    ) -> Vec<(String, i64, i64, String)> {
        let connection =
            sqlite::Connection::open(&location.0).expect("database reopens for inspection");
        let mut statement = connection
            .prepare(
                "SELECT run_id, sequence, cursor, snapshot_json FROM model_run_snapshots ORDER BY run_id",
            )
            .expect("model snapshot query prepares");
        statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .expect("model snapshot query executes")
            .map(|row| row.expect("model snapshot row reads"))
            .collect()
    }

    fn fixture_workspace_root() -> WorkspaceRootDto {
        WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-storage-sqlite-unit-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("native fixture workspace is absolute")
    }

    fn create_fixture_session(repository: &SqliteStorageRepository) -> SessionId {
        let session_id = SessionId::new();
        repository
            .create_session(CreateSessionInputDto::new(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    session_id,
                    WorkspaceId::new(),
                    fixture_workspace_root(),
                    RunModeDto::Build,
                ),
                fixture_time(1),
            ))
            .expect("fixture session creates");
        session_id
    }

    fn accept_fixture_turn(
        repository: &SqliteStorageRepository,
        session_id: SessionId,
        turn_id: TurnId,
        run_id: RunId,
        content: &str,
    ) {
        repository
            .accept_user_turn(
                AcceptUserTurnInputDto::new(
                    session_id,
                    turn_id,
                    content,
                    run_id,
                    fixture_snapshot(),
                    fixture_time(2),
                )
                .expect("fixture turn input is valid"),
            )
            .expect("fixture turn commits");
    }
    #[test]
    fn durable_codec_helpers_cover_declared_values_and_safe_errors() {
        for mode in [
            intention_domain::RunModeDto::Plan,
            intention_domain::RunModeDto::Build,
        ] {
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
            RunStatusDto::Queued,
            RunStatusDto::Starting,
            RunStatusDto::Running,
            RunStatusDto::WaitingInput,
            RunStatusDto::Completing,
            RunStatusDto::Cancelling,
            RunStatusDto::Completed,
            RunStatusDto::Cancelled,
            RunStatusDto::Failed,
            RunStatusDto::Interrupted,
        ] {
            assert_eq!(
                parse_status(status_name(status)).expect("known status parses"),
                status
            );
        }
        assert_eq!(
            parse_status("invalid")
                .expect_err("unknown status rejects")
                .code(),
            "storage_decode_failed"
        );
        assert_eq!(unavailable().code(), "storage_unavailable");
        assert_eq!(
            storage_error(sqlite::Error::InvalidQuery).code(),
            "storage_unavailable"
        );
        assert_eq!(
            not_found_or_storage(sqlite::Error::QueryReturnedNoRows).code(),
            "storage_record_not_found"
        );
        assert_eq!(
            not_found("fixture_missing", "missing").code(),
            "fixture_missing"
        );
        assert_eq!(
            conflict("fixture_conflict", "conflict").code(),
            "fixture_conflict"
        );
    }

    #[test]
    fn terminal_and_recovery_snapshots_record_every_affected_run() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        let active_run = RunId::new();
        accept_fixture_turn(&repository, session_id, TurnId::new(), active_run, "active");
        let queued_run = RunId::new();
        accept_fixture_turn(&repository, session_id, TurnId::new(), queued_run, "queued");
        let terminal = repository
            .transition_run(TransitionRunInputDto::new(
                session_id,
                active_run,
                RunStatusDto::Failed,
                fixture_time(3),
            ))
            .expect("terminal transition commits");
        let rows = raw_snapshot_rows(&location);
        assert_eq!(rows.len(), 2);
        let terminal_sequence =
            i64::try_from(terminal.position().value()).expect("terminal sequence fits SQLite");
        assert!(
            rows.iter()
                .all(|(_, sequence, _)| *sequence == terminal_sequence)
        );
        assert!(rows.iter().any(|(run_id, _, projection)| {
            run_id == &active_run.to_string() && projection.contains("failed")
        }));
        assert!(rows.iter().any(|(run_id, _, projection)| {
            run_id == &queued_run.to_string() && projection.contains("starting")
        }));

        let recovery = repository
            .recover_unfinished_runs(RecoverUnfinishedRunsInputDto::new(fixture_time(4)))
            .expect("recovery commits");
        assert_eq!(recovery.len(), 1);
        let recovery_active = RunId::new();
        accept_fixture_turn(
            &repository,
            session_id,
            TurnId::new(),
            recovery_active,
            "active after terminal promotion",
        );
        let recovery_successor = RunId::new();
        accept_fixture_turn(
            &repository,
            session_id,
            TurnId::new(),
            recovery_successor,
            "queued after terminal promotion",
        );
        let recovery = repository
            .recover_unfinished_runs(RecoverUnfinishedRunsInputDto::new(fixture_time(5)))
            .expect("recovery promotes queued successor");
        assert_eq!(recovery.len(), 1);
        let rows = raw_snapshot_rows(&location);
        assert_eq!(rows.len(), 4);
        let recovery_sequence =
            i64::try_from(recovery[0].position().value()).expect("recovery sequence fits SQLite");
        assert!(
            rows.iter()
                .all(|(_, sequence, _)| { *sequence == recovery_sequence })
        );
        assert!(rows.iter().any(|(run_id, _, projection)| {
            run_id == &recovery_active.to_string() && projection.contains("interrupted")
        }));
        assert!(rows.iter().any(|(run_id, _, projection)| {
            run_id == &recovery_successor.to_string() && projection.contains("starting")
        }));
    }

    #[test]
    fn persisted_m3_json_never_contains_the_fixture_credential() {
        let location = fixture_location();
        let repository = SqliteStorageRepository::open(location.clone()).expect("database opens");
        let session_id = create_fixture_session(&repository);
        accept_fixture_turn(
            &repository,
            session_id,
            TurnId::new(),
            RunId::new(),
            "active",
        );
        let columns = raw_json_columns(&location);
        assert!(!columns.is_empty());
        assert!(
            columns
                .iter()
                .all(|value| !value.contains("fixture-secret"))
        );
    }

    #[test]
    fn every_fault_phase_rolls_back_turn_acceptance_durably() {
        for point in [
            FaultPoint::Events,
            FaultPoint::Projection,
            FaultPoint::Snapshot,
        ] {
            let location = fixture_location();
            let repository =
                SqliteStorageRepository::open(location.clone()).expect("database opens");
            let session_id = create_fixture_session(&repository);
            let baseline = repository
                .load_session_snapshot(session_id)
                .expect("baseline snapshot loads");
            repository.arm_fault(point);
            let error = repository
                .accept_user_turn(
                    AcceptUserTurnInputDto::new(
                        session_id,
                        TurnId::new(),
                        "atomic turn",
                        RunId::new(),
                        fixture_snapshot(),
                        fixture_time(2),
                    )
                    .expect("fixture turn input is valid"),
                )
                .expect_err("injected mutation fault aborts transaction");
            assert_eq!(error.code(), "injected_storage_fault");
            drop(repository);
            let reopened =
                SqliteStorageRepository::open(location.clone()).expect("database reopens");
            assert_eq!(
                reopened
                    .load_session_snapshot(session_id)
                    .expect("reopened baseline snapshot loads"),
                baseline
            );
            assert!(
                reopened
                    .load_tail(session_id, SessionEventSequenceDto::new(0))
                    .expect("tail loads")
                    .iter()
                    .all(|event| !matches!(event.payload(), DomainEventDto::UserTurnAccepted(_)))
            );
        }
    }

    #[test]
    fn every_fault_phase_rolls_back_terminal_promotion_durably() {
        for point in [
            FaultPoint::Events,
            FaultPoint::Projection,
            FaultPoint::Snapshot,
        ] {
            let location = fixture_location();
            let repository =
                SqliteStorageRepository::open(location.clone()).expect("database opens");
            let session_id = create_fixture_session(&repository);
            let active_run = RunId::new();
            accept_fixture_turn(&repository, session_id, TurnId::new(), active_run, "active");
            let queued_turn = TurnId::new();
            let queued_run = RunId::new();
            accept_fixture_turn(&repository, session_id, queued_turn, queued_run, "queued");
            let baseline = repository
                .load_session_snapshot(session_id)
                .expect("baseline snapshot loads");
            let baseline_tail = repository
                .load_tail(session_id, SessionEventSequenceDto::new(0))
                .expect("baseline tail loads");
            let baseline_rows = raw_snapshot_rows(&location);
            repository.arm_fault(point);
            let error = repository
                .transition_run(TransitionRunInputDto::new(
                    session_id,
                    active_run,
                    RunStatusDto::Failed,
                    fixture_time(3),
                ))
                .expect_err("injected promotion fault aborts transaction");
            assert_eq!(error.code(), "injected_storage_fault");
            drop(repository);
            let reopened =
                SqliteStorageRepository::open(location.clone()).expect("database reopens");
            assert_eq!(
                reopened
                    .load_session_snapshot(session_id)
                    .expect("reopened baseline snapshot loads"),
                baseline
            );
            assert_eq!(raw_snapshot_rows(&location), baseline_rows);
            assert_eq!(
                reopened
                    .load_tail(session_id, SessionEventSequenceDto::new(0))
                    .expect("reopened tail loads"),
                baseline_tail
            );
        }
    }
    #[test]
    fn model_fact_fault_stages_roll_back_envelope_index_projection_and_snapshots() {
        for point in [
            FaultPoint::ModelFacts,
            FaultPoint::Projection,
            FaultPoint::Snapshot,
        ] {
            let location = fixture_location();
            let repository =
                SqliteStorageRepository::open(location.clone()).expect("database opens");
            let session_id = create_fixture_session(&repository);
            let run_id = RunId::new();
            accept_fixture_turn(&repository, session_id, TurnId::new(), run_id, "active");
            repository
                .append_model_run_facts(
                    AppendModelRunFactsInputDto::new(
                        session_id,
                        run_id,
                        RunEventCursorDto::new(0),
                        vec![
                            ModelRunFactInputDto::provider_attempt_started(1)
                                .expect("attempt is valid"),
                        ],
                        None,
                        fixture_time(3),
                    )
                    .expect("initial fact input is valid"),
                )
                .expect("initial fact appends");
            let baseline_replay = repository
                .load_current_run_replay(session_id, run_id)
                .expect("baseline replay loads");
            let baseline_model_snapshots = raw_model_snapshot_rows(&location);
            let baseline_facts: i64 = {
                let connection =
                    sqlite::Connection::open(&location.0).expect("database reopens for inspection");
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM model_run_facts WHERE run_id=?1",
                        [run_id.to_string()],
                        |row| row.get(0),
                    )
                    .expect("fact count loads")
            };
            repository.arm_fault(point);
            assert_eq!(
                repository
                    .append_model_run_facts(
                        AppendModelRunFactsInputDto::new(
                            session_id,
                            run_id,
                            RunEventCursorDto::new(1),
                            vec![ModelRunFactInputDto::usage_recorded(UsageDto::NotReported)],
                            Some(RunStatusDto::Failed),
                            fixture_time(4),
                        )
                        .expect("fault append input is valid"),
                    )
                    .expect_err("injected model fact stage rolls back")
                    .code(),
                "injected_storage_fault"
            );
            drop(repository);
            let reopened =
                SqliteStorageRepository::open(location.clone()).expect("database reopens");
            assert_eq!(
                reopened
                    .load_current_run_replay(session_id, run_id)
                    .expect("reopened replay loads"),
                baseline_replay
            );
            assert_eq!(
                reopened
                    .load_session_snapshot(session_id)
                    .expect("reopened session snapshot loads")
                    .active_run()
                    .expect("run remains active after rollback")
                    .status(),
                RunStatusDto::Starting
            );
            drop(reopened);
            assert_eq!(raw_model_snapshot_rows(&location), baseline_model_snapshots);
            let connection =
                sqlite::Connection::open(&location.0).expect("database reopens for inspection");
            let fact_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM model_run_facts WHERE run_id=?1",
                    [run_id.to_string()],
                    |row| row.get(0),
                )
                .expect("fact count reloads");
            assert_eq!(fact_count, baseline_facts);
        }
    }

    #[test]
    fn location_is_absolute_and_faults_are_single_use() {
        assert!(SqliteDatabaseLocationDto::new("relative.db").is_err());
        let location = format!(
            "{}/intention-storage-sqlite-unit-{}.db",
            std::env::temp_dir().display(),
            EventId::new()
        );
        let repository = SqliteStorageRepository::open(
            SqliteDatabaseLocationDto::new(location).expect("temp location is absolute"),
        )
        .expect("database opens");
        repository.arm_fault(FaultPoint::Snapshot);
        assert!(repository.fault(FaultPoint::Snapshot).is_err());
        assert!(repository.fault(FaultPoint::Snapshot).is_ok());
    }
}
