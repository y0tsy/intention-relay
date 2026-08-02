//! SQLite-backed M3 durable storage implementation.
//!
//! The public boundary is DTO-only. SQLite connections, SQL rows, paths, and
//! JSON codecs remain private implementation details of this crate.

use std::path::Path;
use std::sync::{LazyLock, Mutex};

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    DomainEventDto, QueuedTurnProjectionDto, QueuedTurnRemovedEventDto, RunProjectionDto,
    RunStartedEventDto, RunStatusChangedEventDto, RunStatusDto, SessionCreatedEventDto,
    SessionProjectionDto, UserTurnAcceptedEventDto, UserTurnQueuedEventDto,
    validate_run_status_transition,
};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, CommittedChangeDto, CreateSessionInputDto,
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

const CURRENT_STORAGE_SCHEMA: i64 = 1;
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
)])
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
                sqlite::params![event.event_id().to_string(), session_id.to_string(), position as i64, encoded],
            ).map_err(storage_error)?;
            events.push(event);
        }
        tx.execute(
            "UPDATE sessions SET last_sequence=?2 WHERE session_id=?1",
            sqlite::params![session_id.to_string(), position as i64],
        )
        .map_err(storage_error)?;
        Ok(events)
    }

    fn snapshot(tx: &sqlite::Transaction<'_>, projection: &SessionProjectionDto) -> DtoResult<()> {
        let encoded = serde_json::to_string(projection).map_err(codec_error)?;
        tx.execute(
            "INSERT INTO session_snapshots(session_id, sequence, projection_json) VALUES (?1, ?2, ?3) ON CONFLICT(session_id) DO UPDATE SET sequence=excluded.sequence, projection_json=excluded.projection_json",
            sqlite::params![projection.session_id().to_string(), projection.at_sequence().value() as i64, encoded],
        ).map_err(storage_error)?;
        if let Some(run) = projection.active_run() {
            let encoded = serde_json::to_string(&run).map_err(codec_error)?;
            tx.execute(
                "INSERT INTO run_snapshots(run_id, session_id, sequence, projection_json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(run_id) DO UPDATE SET sequence=excluded.sequence, projection_json=excluded.projection_json",
                sqlite::params![run.run_id().to_string(), projection.session_id().to_string(), projection.at_sequence().value() as i64, encoded],
            ).map_err(storage_error)?;
        }
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
            tx.execute("INSERT INTO sessions(session_id,project_id,workspace_id,workspace_root,mode,config_revision_id,last_sequence,next_queue_ticket) VALUES (?1,?2,?3,?4,?5,NULL,0,0)", sqlite::params![session_id.to_string(), command.project_id().to_string(), command.workspace_id().to_string(), command.workspace_root().as_str(), mode_name(command.mode())]).map_err(storage_error)?;
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
            if let Some(promoted) = input.promoted_turn() {
                if !input.status().is_terminal() {
                    return Err(conflict(
                        "invalid_queue_promotion",
                        "a queued turn may only be promoted with a terminal run transition",
                    ));
                }
                let promoted_turn_id = promoted.turn_id();
                let queued_selection = tx
                    .query_row(
                        "SELECT turns.proposed_run_id, turns.config_revision_id FROM queued_turns JOIN turns ON turns.session_id=queued_turns.session_id AND turns.turn_id=queued_turns.turn_id WHERE queued_turns.session_id=?1 AND queued_turns.turn_id=?2",
                        sqlite::params![session_id.to_string(), promoted_turn_id.to_string()],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()
                    .map_err(storage_error)?
                    .ok_or_else(|| {
                        not_found(
                            "queued_turn_not_found",
                            "the promoted queued turn does not exist",
                        )
                    })?;
                let promoted_run_id = RunId::parse(&queued_selection.0).map_err(codec_error)?;
                let config_revision_id =
                    ConfigRevisionId::parse(&queued_selection.1).map_err(codec_error)?;
                tx.execute(
                    "DELETE FROM queued_turns WHERE session_id=?1 AND turn_id=?2",
                    sqlite::params![session_id.to_string(), promoted_turn_id.to_string()],
                )
                .map_err(storage_error)?;
                tx.execute("UPDATE turns SET outcome='started',queue_ticket=NULL WHERE session_id=?1 AND turn_id=?2",sqlite::params![session_id.to_string(),promoted_turn_id.to_string()]).map_err(storage_error)?;
                tx.execute("INSERT INTO runs(run_id,session_id,turn_id,status,config_revision_id) VALUES (?1,?2,?3,'starting',?4)",sqlite::params![promoted_run_id.to_string(),session_id.to_string(),promoted_turn_id.to_string(),config_revision_id.to_string()]).map_err(storage_error)?;
                drafts.push(EventDraft::new(
                    Some(promoted_run_id),
                    Some(promoted_turn_id),
                    input.occurred_at(),
                    DomainEventDto::RunStarted(RunStartedEventDto::new(
                        session_id,
                        promoted_run_id,
                        promoted_turn_id,
                        config_revision_id,
                        input.occurred_at(),
                    )),
                ));
            }
            let events = Self::append(&tx, session_id, position, drafts)?;
            self.finish(tx, session_id, events, None)
        })
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
            let change = immediate_transaction!(self, |tx| {
                let position = sequence(&tx, session_id)?;
                tx.execute(
                    "UPDATE runs SET status='interrupted' WHERE session_id=?1 AND run_id=?2",
                    sqlite::params![session, run],
                )
                .map_err(storage_error)?;
                let events = Self::append(
                    &tx,
                    session_id,
                    position,
                    vec![EventDraft::new(
                        Some(run_id),
                        None,
                        input.recovered_at(),
                        DomainEventDto::RunStatusChanged(RunStatusChangedEventDto::new(
                            session_id,
                            run_id,
                            RunStatusDto::Interrupted,
                            input.recovered_at(),
                        )),
                    )],
                )?;
                self.finish(tx, session_id, events, None)
            })?;
            changes.push(change);
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
            let mut statement = connection
                .prepare("SELECT envelope_json FROM domain_events WHERE session_id=?1 AND sequence>?2 ORDER BY sequence")
                .map_err(storage_error)?;
            let events = statement
                .query_map(
                    sqlite::params![session_id.to_string(), after.value() as i64],
                    |row| row.get::<_, String>(0),
                )
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
    use intention_domain::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
    use intention_storage::{
        AcceptUserTurnInputDto, CreateSessionInputDto, PromotedQueuedTurnInputDto,
        StorageRepositoryDto, TransitionRunInputDto,
    };
    use intention_types::{
        ConfigRevisionId, ProjectId, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId,
        TimestampDto, TurnId, WorkspaceId,
    };

    fn fixture_time(value: i64) -> TimestampDto {
        TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
    }

    fn fixture_snapshot() -> ConfigSnapshotDto {
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse("/tmp/intention-storage-sqlite-unit.toml")
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

    fn create_fixture_session(repository: &SqliteStorageRepository) -> SessionId {
        let session_id = SessionId::new();
        repository
            .create_session(CreateSessionInputDto::new(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    session_id,
                    WorkspaceId::new(),
                    WorkspaceRootDto::parse("/workspace/storage-sqlite-unit")
                        .expect("fixture workspace is absolute"),
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
            let reopened = SqliteStorageRepository::open(location).expect("database reopens");
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
            repository.arm_fault(point);
            let error = repository
                .transition_run(TransitionRunInputDto::new(
                    session_id,
                    active_run,
                    RunStatusDto::Failed,
                    fixture_time(3),
                    Some(PromotedQueuedTurnInputDto::new(queued_turn)),
                ))
                .expect_err("injected promotion fault aborts transaction");
            assert_eq!(error.code(), "injected_storage_fault");
            drop(repository);
            let reopened = SqliteStorageRepository::open(location).expect("database reopens");
            assert_eq!(
                reopened
                    .load_session_snapshot(session_id)
                    .expect("reopened baseline snapshot loads"),
                baseline
            );
            assert_eq!(
                reopened
                    .load_tail(session_id, SessionEventSequenceDto::new(0))
                    .expect("reopened tail loads"),
                baseline_tail
            );
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
