#![allow(
    clippy::expect_used,
    reason = "M4 SQLite outcome fixtures use expect for precise diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, ModelRunFactDto, ModelRunFactEventDto,
    ModelRunFactInputDto, ModelRunProjectionDto, RunEventCursorDto, RunModeDto, RunSnapshotDto,
    RunStatusDto, ToolLifecycleEventDto, ToolLifecycleStatusDto, WorkspaceRootDto,
};
use intention_storage::{
    AcceptUserTurnInputDto, AppendModelRunFactsInputDto, CreateSessionInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{
    EventEnvelopeDto, EventId, EventMetadataDto, ProjectId, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, ToolCallId, TurnId, WorkspaceId,
};
use tempfile::TempDir;

const M3_SCHEMA_SQL: &str = "
CREATE TABLE projects (project_id TEXT PRIMARY KEY);
CREATE TABLE workspace_roots (workspace_id TEXT PRIMARY KEY, workspace_root TEXT NOT NULL UNIQUE);
CREATE TABLE sessions (
  session_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, workspace_id TEXT NOT NULL,
  workspace_root TEXT NOT NULL, mode TEXT NOT NULL, config_revision_id TEXT,
  last_sequence INTEGER NOT NULL, next_queue_ticket INTEGER NOT NULL
);
CREATE TABLE turns (
  session_id TEXT NOT NULL, turn_id TEXT NOT NULL, content TEXT NOT NULL,
  proposed_run_id TEXT NOT NULL, config_revision_id TEXT NOT NULL, outcome TEXT NOT NULL,
  queue_ticket INTEGER, PRIMARY KEY (session_id, turn_id), UNIQUE (proposed_run_id)
);
CREATE TABLE runs (
  run_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, turn_id TEXT NOT NULL,
  status TEXT NOT NULL, config_revision_id TEXT NOT NULL, UNIQUE(session_id, turn_id)
);
CREATE TABLE queued_turns (session_id TEXT NOT NULL, turn_id TEXT NOT NULL, queue_ticket INTEGER NOT NULL, PRIMARY KEY(session_id, turn_id), UNIQUE(session_id, queue_ticket));
CREATE TABLE configuration_revisions (revision_id TEXT PRIMARY KEY, snapshot_json TEXT NOT NULL);
CREATE TABLE domain_events (event_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, sequence INTEGER NOT NULL, envelope_json TEXT NOT NULL, UNIQUE(session_id, sequence));
CREATE TABLE session_snapshots (session_id TEXT PRIMARY KEY, sequence INTEGER NOT NULL, projection_json TEXT NOT NULL);
CREATE TABLE run_snapshots (run_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, sequence INTEGER NOT NULL, projection_json TEXT NOT NULL);
CREATE UNIQUE INDEX one_active_run_per_session ON runs(session_id) WHERE status NOT IN ('completed','cancelled','failed','interrupted');
";

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture time is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe snapshot decodes")
}

fn temporary_repository() -> (TempDir, SqliteStorageRepository) {
    let directory = TempDir::new().expect("temporary directory exists");
    let path = directory.path().join("storage.sqlite");
    let repository = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(path.to_string_lossy().into_owned())
            .expect("database path is absolute"),
    )
    .expect("database opens");
    (directory, repository)
}

fn create_started_run(repository: &SqliteStorageRepository) -> (SessionId, RunId) {
    let session_id = SessionId::new();
    repository
        .create_session(CreateSessionInputDto::new(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(
                    std::env::temp_dir()
                        .join("intention-m4-facts")
                        .to_string_lossy()
                        .into_owned(),
                )
                .expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
            time(1),
        ))
        .expect("session creates");
    let run_id = RunId::new();
    repository
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                TurnId::new(),
                "turn",
                run_id,
                snapshot(),
                time(2),
            )
            .expect("turn input is valid"),
        )
        .expect("turn starts");
    (session_id, run_id)
}

fn append(
    repository: &SqliteStorageRepository,
    session_id: SessionId,
    run_id: RunId,
    expected: u64,
    facts: Vec<ModelRunFactInputDto>,
) -> intention_storage::AppendModelRunFactsOutcomeDto {
    repository
        .append_model_run_facts(
            AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                RunEventCursorDto::new(expected),
                facts,
                None,
                time(3),
            )
            .expect("append input is valid"),
        )
        .expect("facts append")
}

#[test]
fn m3_database_migrates_to_cursor_zero_snapshots_without_synthetic_facts() {
    let directory = TempDir::new().expect("temporary directory exists");
    let path = directory.path().join("legacy-m3.sqlite");
    let connection = sqlite::Connection::open(&path).expect("legacy database opens");
    connection
        .execute_batch(M3_SCHEMA_SQL)
        .expect("legacy M3 schema creates");
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let revision_id = intention_types::ConfigRevisionId::new();
    connection
        .execute(
            "INSERT INTO projects(project_id) VALUES (?1)",
            [ProjectId::new().to_string()],
        )
        .expect("project inserts");
    connection
        .execute(
            "INSERT INTO workspace_roots(workspace_id, workspace_root) VALUES (?1, ?2)",
            sqlite::params![WorkspaceId::new().to_string(), workspace_root_path()],
        )
        .expect("workspace inserts");
    let workspace_id: String = connection
        .query_row("SELECT workspace_id FROM workspace_roots", [], |row| {
            row.get(0)
        })
        .expect("workspace identity loads");
    let project_id: String = connection
        .query_row("SELECT project_id FROM projects", [], |row| row.get(0))
        .expect("project identity loads");
    let run_projection = intention_domain::RunProjectionDto::new(
        session_id,
        run_id,
        turn_id,
        RunStatusDto::Failed,
        revision_id,
    );
    let session_projection = intention_domain::SessionProjectionDto::new(
        ProjectId::parse(&project_id).expect("project identity parses"),
        session_id,
        WorkspaceId::parse(&workspace_id).expect("workspace identity parses"),
        WorkspaceRootDto::parse(workspace_root_path()).expect("workspace root is absolute"),
        RunModeDto::Build,
        Some(revision_id),
        None,
        Vec::new(),
        intention_types::SessionEventSequenceDto::new(7),
    )
    .expect("legacy session projection is valid");
    connection
        .execute(
            "INSERT INTO sessions(session_id, project_id, workspace_id, workspace_root, mode, config_revision_id, last_sequence, next_queue_ticket) VALUES (?1, ?2, ?3, ?4, 'build', ?5, 7, 0)",
            sqlite::params![session_id.to_string(), project_id, workspace_id, workspace_root_path(), revision_id.to_string()],
        )
        .expect("session inserts");
    connection
        .execute(
            "INSERT INTO runs(run_id, session_id, turn_id, status, config_revision_id) VALUES (?1, ?2, ?3, 'failed', ?4)",
            sqlite::params![run_id.to_string(), session_id.to_string(), turn_id.to_string(), revision_id.to_string()],
        )
        .expect("run inserts");
    connection
        .execute(
            "INSERT INTO session_snapshots(session_id, sequence, projection_json) VALUES (?1, 7, ?2)",
            sqlite::params![session_id.to_string(), serde_json::to_string(&session_projection).expect("session projection serializes")],
        )
        .expect("session snapshot inserts");
    connection
        .execute(
            "INSERT INTO run_snapshots(run_id, session_id, sequence, projection_json) VALUES (?1, ?2, 7, ?3)",
            sqlite::params![run_id.to_string(), session_id.to_string(), serde_json::to_string(&run_projection).expect("run projection serializes")],
        )
        .expect("run snapshot inserts");
    connection
        .pragma_update(None, "user_version", 1_i64)
        .expect("legacy version sets");
    drop(connection);

    let repository = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(path.to_string_lossy().into_owned())
            .expect("database path is absolute"),
    )
    .expect("M3 database migrates");
    let replay = repository
        .load_current_run_replay(session_id, run_id)
        .expect("migrated run replay loads");
    assert_eq!(replay.snapshot().cursor().value(), 0);
    assert_eq!(replay.snapshot().at_sequence().value(), 7);
    assert_eq!(replay.snapshot().run_projection(), run_projection);
    assert!(replay.tail().facts().is_empty());
    let connection = sqlite::Connection::open(&path).expect("migrated database reopens");
    let fact_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM model_run_facts", [], |row| row.get(0))
        .expect("fact count loads");
    assert_eq!(fact_count, 0);
}

#[test]
fn slice1_schema_three_reopen_preserves_all_m3_m4_bytes() {
    // Stamps PRAGMA user_version = 3 on the exact production schema-3 surface
    // (built from the crate's migration source of truth), so the reopen
    // exercises the schema-3 path without a migration while proving every
    // pre-existing M3/M4 row in all fourteen tables is byte-identical. The
    // fixture enables foreign keys so the seeded rows prove the production
    // constraints (FKs, UNIQUEs, CHECKs, and the partial active-run index)
    // directly against SQLite.
    let directory = TempDir::new().expect("temporary directory exists");
    let path = directory.path().join("legacy-v3.sqlite");
    let connection = sqlite::Connection::open(&path).expect("legacy database opens");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("foreign keys enable");
    connection
        .execute_batch(intention_storage_sqlite::TEST_SCHEMA_3_SQL)
        .expect("production schema three creates");
    let project_id = ProjectId::new();
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let revision_id = intention_types::ConfigRevisionId::new();
    let turn_id = TurnId::new();
    let queued_turn_id = TurnId::new();
    let run_id = RunId::new();
    let queued_run_id = RunId::new();
    let fact_event_id = EventId::new();
    let tool_event_id = EventId::new();
    let call_id = ToolCallId::new();

    connection
        .execute(
            "INSERT INTO projects(project_id) VALUES (?1)",
            [project_id.to_string()],
        )
        .expect("project inserts");
    connection
        .execute(
            "INSERT INTO workspace_roots(workspace_id, workspace_root) VALUES (?1, ?2)",
            sqlite::params![workspace_id.to_string(), workspace_root_path()],
        )
        .expect("workspace inserts");
    connection
        .execute(
            "INSERT INTO configuration_revisions(revision_id, snapshot_json) VALUES (?1, ?2)",
            sqlite::params![
                revision_id.to_string(),
                serde_json::to_string(&snapshot()).expect("safe snapshot serializes")
            ],
        )
        .expect("configuration revision inserts");
    connection
        .execute(
            "INSERT INTO sessions(session_id, project_id, workspace_id, workspace_root, mode, config_revision_id, last_sequence, next_queue_ticket) VALUES (?1, ?2, ?3, ?4, 'build', ?5, 7, 1)",
            sqlite::params![session_id.to_string(), project_id.to_string(), workspace_id.to_string(), workspace_root_path(), revision_id.to_string()],
        )
        .expect("session inserts");
    connection
        .execute(
            "INSERT INTO turns(session_id, turn_id, content, proposed_run_id, config_revision_id, outcome, queue_ticket) VALUES (?1, ?2, 'turn', ?3, ?4, 'started', NULL)",
            sqlite::params![session_id.to_string(), turn_id.to_string(), run_id.to_string(), revision_id.to_string()],
        )
        .expect("started turn inserts");
    connection
        .execute(
            "INSERT INTO turns(session_id, turn_id, content, proposed_run_id, config_revision_id, outcome, queue_ticket) VALUES (?1, ?2, 'queued turn', ?3, ?4, 'queued', 0)",
            sqlite::params![session_id.to_string(), queued_turn_id.to_string(), queued_run_id.to_string(), revision_id.to_string()],
        )
        .expect("queued turn inserts");
    // The run is terminal ('completed') so the queued-turn rows coexist with
    // the partial unique index one_active_run_per_session.
    connection
        .execute(
            "INSERT INTO runs(run_id, session_id, turn_id, status, config_revision_id) VALUES (?1, ?2, ?3, 'completed', ?4)",
            sqlite::params![run_id.to_string(), session_id.to_string(), turn_id.to_string(), revision_id.to_string()],
        )
        .expect("run inserts");
    connection
        .execute(
            "INSERT INTO queued_turns(session_id, turn_id, queue_ticket) VALUES (?1, ?2, 0)",
            sqlite::params![session_id.to_string(), queued_turn_id.to_string()],
        )
        .expect("queued turn entry inserts");
    connection
        .execute(
            "INSERT INTO domain_events(event_id, session_id, sequence, envelope_json) VALUES (?1, ?2, 1, ?3)",
            sqlite::params![fact_event_id.to_string(), session_id.to_string(), serde_json::to_string(&fact_envelope(session_id, run_id, turn_id, fact_event_id, 1)).expect("fact envelope serializes")],
        )
        .expect("fact event inserts");
    connection
        .execute(
            "INSERT INTO domain_events(event_id, session_id, sequence, envelope_json) VALUES (?1, ?2, 2, ?3)",
            sqlite::params![tool_event_id.to_string(), session_id.to_string(), serde_json::to_string(&tool_lifecycle_envelope(session_id, run_id, turn_id, tool_event_id, call_id, 2)).expect("tool lifecycle envelope serializes")],
        )
        .expect("tool event inserts");
    let run_projection = intention_domain::RunProjectionDto::new(
        session_id,
        run_id,
        turn_id,
        RunStatusDto::Completed,
        revision_id,
    );
    let session_projection = intention_domain::SessionProjectionDto::new(
        project_id,
        session_id,
        workspace_id,
        WorkspaceRootDto::parse(workspace_root_path()).expect("workspace root is absolute"),
        RunModeDto::Build,
        Some(revision_id),
        None,
        Vec::new(),
        SessionEventSequenceDto::new(7),
    )
    .expect("legacy session projection is valid");
    connection
        .execute(
            "INSERT INTO session_snapshots(session_id, sequence, projection_json) VALUES (?1, 7, ?2)",
            sqlite::params![session_id.to_string(), serde_json::to_string(&session_projection).expect("session projection serializes")],
        )
        .expect("session snapshot inserts");
    connection
        .execute(
            "INSERT INTO run_snapshots(run_id, session_id, sequence, projection_json) VALUES (?1, ?2, 7, ?3)",
            sqlite::params![run_id.to_string(), session_id.to_string(), serde_json::to_string(&run_projection).expect("run projection serializes")],
        )
        .expect("run snapshot inserts");
    connection
        .execute(
            "INSERT INTO run_cursors(run_id, session_id, cursor) VALUES (?1, ?2, 0)",
            sqlite::params![run_id.to_string(), session_id.to_string()],
        )
        .expect("run cursor inserts");
    connection
        .execute(
            "INSERT INTO model_run_facts(run_id, cursor, event_id) VALUES (?1, 1, ?2)",
            sqlite::params![run_id.to_string(), fact_event_id.to_string()],
        )
        .expect("model fact inserts");
    let model_snapshot = RunSnapshotDto::new(
        session_id,
        run_id,
        SessionEventSequenceDto::new(7),
        ModelRunProjectionDto::new(
            run_projection,
            RunEventCursorDto::new(0),
            None,
            "",
            None,
            None,
            None,
        )
        .expect("model projection is valid"),
    )
    .expect("model run snapshot is valid");
    connection
        .execute(
            "INSERT INTO model_run_snapshots(run_id, session_id, sequence, cursor, snapshot_json) VALUES (?1, ?2, 7, 0, ?3)",
            sqlite::params![run_id.to_string(), session_id.to_string(), serde_json::to_string(&model_snapshot).expect("model snapshot serializes")],
        )
        .expect("model run snapshot inserts");
    connection
        .execute(
            "INSERT INTO tool_results(run_id, session_id, call_id, event_id, kind, content, occurred_at) VALUES (?1, ?2, ?3, ?4, 'read', 'file content', 3)",
            sqlite::params![run_id.to_string(), session_id.to_string(), call_id.to_string(), tool_event_id.to_string()],
        )
        .expect("tool result inserts");
    connection
        .pragma_update(None, "user_version", 3_i64)
        .expect("schema three version sets");

    // Capture every pre-existing M3/M4 row byte-for-byte before reopen, in
    // foreign-key order across all fourteen schema-3 tables.
    let tables = [
        "projects",
        "workspace_roots",
        "configuration_revisions",
        "sessions",
        "turns",
        "runs",
        "queued_turns",
        "domain_events",
        "session_snapshots",
        "run_snapshots",
        "run_cursors",
        "model_run_facts",
        "model_run_snapshots",
        "tool_results",
    ];
    let mut before = Vec::new();
    for table in tables {
        before.extend(capture_rows(&connection, table));
    }
    drop(connection);

    let repository = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(path.to_string_lossy().into_owned())
            .expect("database path is absolute"),
    )
    .expect("schema three database reopens");
    let replay = repository
        .load_current_run_replay(session_id, run_id)
        .expect("reopened run replay loads");
    assert_eq!(replay.snapshot().cursor().value(), 0);
    assert_eq!(replay.snapshot().at_sequence().value(), 7);
    assert_eq!(replay.snapshot().run_projection(), run_projection);
    assert!(replay.tail().facts().is_empty());

    let connection = sqlite::Connection::open(&path).expect("reopened database reopens raw");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version reads");
    assert_eq!(version, 3);
    let mut after = Vec::new();
    for table in tables {
        after.extend(capture_rows(&connection, table));
    }
    assert_eq!(
        before, after,
        "every pre-existing M3/M4 row is byte-identical after reopen"
    );
}

/// A byte-exact captured cell typed by SQLite storage class: text stays raw
/// bytes, blobs stay raw bytes, and reals keep their exact bit pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
enum CapturedValue {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

/// Captures every row of a table as typed values in rowid order, tagged with
/// the table name, so pre-reopen and post-reopen bytes compare exactly.
fn capture_rows(connection: &sqlite::Connection, table: &str) -> Vec<(String, Vec<CapturedValue>)> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .expect("capture statement prepares");
    let column_count = statement.column_count();
    let mut rows = statement.query([]).expect("capture query runs");
    let mut captured = Vec::new();
    while let Some(row) = rows.next().expect("capture row reads") {
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            let value = match row.get_ref(index).expect("capture value reads") {
                sqlite::types::ValueRef::Null => CapturedValue::Null,
                sqlite::types::ValueRef::Integer(value) => CapturedValue::Integer(value),
                sqlite::types::ValueRef::Real(value) => CapturedValue::Real(value.to_bits()),
                sqlite::types::ValueRef::Text(text) => CapturedValue::Text(text.to_vec()),
                sqlite::types::ValueRef::Blob(blob) => CapturedValue::Blob(blob.to_vec()),
            };
            values.push(value);
        }
        captured.push((table.to_string(), values));
    }
    captured
}

/// A valid opaque envelope for one durable reasoning-delta domain event.
fn fact_envelope(
    session_id: SessionId,
    run_id: RunId,
    turn_id: TurnId,
    event_id: EventId,
    sequence: u64,
) -> EventEnvelopeDto<DomainEventDto> {
    let fact = ModelRunFactDto::new(
        RunEventCursorDto::new(sequence),
        ModelRunFactInputDto::reasoning_delta_recorded("tail-only reasoning")
            .expect("reasoning fact is valid"),
    )
    .expect("model fact is valid");
    let payload = ModelRunFactEventDto::new(session_id, run_id, fact, time(3));
    EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 0),
            event_id,
            session_id,
            Some(run_id),
            Some(turn_id),
            SessionEventSequenceDto::new(sequence),
            time(3),
        ),
        DomainEventDto::ReasoningDeltaRecorded(payload),
    )
}

/// A valid opaque envelope for one durable tool-lifecycle domain event.
fn tool_lifecycle_envelope(
    session_id: SessionId,
    run_id: RunId,
    turn_id: TurnId,
    event_id: EventId,
    call_id: ToolCallId,
    sequence: u64,
) -> EventEnvelopeDto<DomainEventDto> {
    let lifecycle = ToolLifecycleEventDto::new(
        session_id,
        run_id,
        call_id,
        "read",
        ToolLifecycleStatusDto::Admitted,
        "local tool invocation admitted",
        time(3),
    )
    .expect("tool lifecycle event is valid");
    EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 0),
            event_id,
            session_id,
            Some(run_id),
            Some(turn_id),
            SessionEventSequenceDto::new(sequence),
            time(3),
        ),
        DomainEventDto::ToolLifecycle(lifecycle),
    )
}

fn workspace_root_path() -> String {
    std::env::temp_dir()
        .join("intention-m4-legacy-migration")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn durable_model_facts_are_replayed_with_cursor_boundaries_and_no_cross_session_leak() {
    let (_directory, repository) = temporary_repository();
    let (session_id, run_id) = create_started_run(&repository);
    let outcome = append(
        &repository,
        session_id,
        run_id,
        0,
        vec![
            ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid"),
            ModelRunFactInputDto::assistant_content_appended(
                intention_types::AssistantTurnId::new(),
                "hello",
            )
            .expect("content is valid"),
        ],
    );
    assert_eq!(outcome.cursor().value(), 2);
    let replay = repository
        .load_current_run_replay(session_id, run_id)
        .expect("current replay loads");
    assert_eq!(replay.snapshot().cursor().value(), 2);
    assert!(replay.tail().facts().is_empty());
    let tail = repository
        .load_run_tail(session_id, run_id, RunEventCursorDto::new(0))
        .expect("run tail loads");
    assert_eq!(tail.facts().len(), 2);
    assert_eq!(tail.next_after_cursor().value(), 2);
    for (count, expected_more) in [(257_usize, true), (256_usize, false)] {
        let (_directory, bounded_repository) = temporary_repository();
        let (bounded_session_id, bounded_run_id) = create_started_run(&bounded_repository);
        let facts = (0..count)
            .map(|_| ModelRunFactInputDto::reasoning_delta_recorded("x").expect("fact is valid"))
            .collect();
        append(
            &bounded_repository,
            bounded_session_id,
            bounded_run_id,
            0,
            facts,
        );
        let page = bounded_repository
            .load_run_tail(
                bounded_session_id,
                bounded_run_id,
                RunEventCursorDto::new(0),
            )
            .expect("bounded tail loads");
        assert_eq!(page.facts().len(), 256);
        assert_eq!(page.has_more(), expected_more);
    }
    assert_eq!(
        repository
            .load_run_tail(session_id, run_id, RunEventCursorDto::new(u64::MAX))
            .expect_err("overflow future cursor fails before history query")
            .code(),
        "invalid_run_event_cursor"
    );
    assert_eq!(
        repository
            .load_current_run_replay(SessionId::new(), run_id)
            .expect_err("cross-session replay fails")
            .code(),
        "run_replay_not_found"
    );
}

#[test]
fn durable_model_fact_failures_are_safe_and_terminal_runs_reject_new_facts() {
    let (_directory, repository) = temporary_repository();
    let (session_id, run_id) = create_started_run(&repository);
    append(
        &repository,
        session_id,
        run_id,
        0,
        vec![ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid")],
    );
    assert_eq!(
        repository
            .append_model_run_facts(
                AppendModelRunFactsInputDto::new(
                    session_id,
                    run_id,
                    RunEventCursorDto::new(0),
                    vec![ModelRunFactInputDto::usage_recorded(
                        intention_types::UsageDto::NotReported,
                    )],
                    None,
                    time(3),
                )
                .expect("input is valid"),
            )
            .expect_err("stale cursor conflicts")
            .code(),
        "run_event_cursor_conflict"
    );
    let too_large = AppendModelRunFactsInputDto::new(
        session_id,
        run_id,
        RunEventCursorDto::new(1),
        vec![
            ModelRunFactInputDto::reasoning_delta_recorded("x".repeat(512 * 1024))
                .expect("individual domain fact accepts large text before storage bound"),
        ],
        None,
        time(3),
    )
    .expect("input construction defers canonical size validation");
    assert_eq!(
        repository
            .append_model_run_facts(too_large)
            .expect_err("canonical fact cap rejects")
            .code(),
        "run_fact_too_large"
    );
    repository
        .transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Failed,
            time(4),
        ))
        .expect("run fails");
    assert_eq!(
        repository
            .append_model_run_facts(
                AppendModelRunFactsInputDto::new(
                    session_id,
                    run_id,
                    RunEventCursorDto::new(1),
                    vec![ModelRunFactInputDto::usage_recorded(
                        intention_types::UsageDto::NotReported,
                    )],
                    None,
                    time(5),
                )
                .expect("input is valid"),
            )
            .expect_err("terminal run rejects facts")
            .code(),
        "invalid_run_event_cursor"
    );
}

#[test]
fn terminal_model_fact_append_promotes_the_oldest_queued_turn() {
    let (_directory, repository) = temporary_repository();
    let (session_id, run_id) = create_started_run(&repository);
    let queued_turn_id = TurnId::new();
    let queued_run_id = RunId::new();
    let queued_snapshot = snapshot();
    repository
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                queued_turn_id,
                "queued turn",
                queued_run_id,
                queued_snapshot.clone(),
                time(3),
            )
            .expect("queued turn input is valid"),
        )
        .expect("turn queues behind active run");

    let outcome = repository
        .append_model_run_facts(
            AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                RunEventCursorDto::new(0),
                vec![ModelRunFactInputDto::usage_recorded(
                    intention_types::UsageDto::NotReported,
                )],
                Some(RunStatusDto::Failed),
                time(4),
            )
            .expect("terminal append input is valid"),
        )
        .expect("terminal facts append and promote");
    assert_eq!(
        outcome.snapshot().run_projection().status(),
        RunStatusDto::Failed
    );
    let session = repository
        .load_session_snapshot(session_id)
        .expect("session snapshot loads");
    assert_eq!(
        session.active_run().expect("queued run promoted").run_id(),
        queued_run_id
    );
    assert_eq!(
        session
            .active_run()
            .expect("queued run remains active")
            .turn_id(),
        queued_turn_id
    );
    let promoted = repository
        .load_current_run_replay(session_id, queued_run_id)
        .expect("promoted run snapshot loads");
    assert_eq!(promoted.snapshot().cursor().value(), 0);
    assert_eq!(
        promoted.snapshot().run_projection().status(),
        RunStatusDto::Starting
    );
    assert_eq!(
        promoted.snapshot().run_projection().config_revision_id(),
        queued_snapshot.revision_id()
    );
    let events = repository
        .load_tail(session_id, intention_types::SessionEventSequenceDto::new(0))
        .expect("session events load");
    let terminal_index = events
        .iter()
        .position(|event| {
            matches!(
                event.payload(),
                DomainEventDto::RunStatusChanged(status)
                    if event.run_id() == Some(run_id) && status.status() == RunStatusDto::Failed
            )
        })
        .expect("terminal status event is present");
    let promoted_index = events
        .iter()
        .position(|event| {
            matches!(
                event.payload(),
                DomainEventDto::RunStarted(started) if started.run_id() == queued_run_id
            )
        })
        .expect("promoted run event is present");
    assert!(terminal_index < promoted_index);
}
