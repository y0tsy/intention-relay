#![allow(
    clippy::expect_used,
    reason = "Focused runtime fixtures use expect to provide precise test failures."
)]

use std::cell::RefCell;

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    DomainEventDto, ModelRunFactInputDto, ModelRunProjectionDto, RunEventCursorDto, RunModeDto,
    RunProjectionDto, RunSnapshotDto, RunStatusDto, SessionProjectionDto, WorkspaceRootDto,
};
use intention_runtime::fail_starting_run;
use intention_storage::{
    AcceptUserTurnInputDto, AppendModelRunFactsInputDto, AppendModelRunFactsOutcomeDto,
    CommittedChangeDto, CreateSessionInputDto, RecoverUnfinishedRunsInputDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, ProjectId, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, TurnId, WorkspaceId,
};

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-runtime-test.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    );
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-secret\"",
        source,
    ))
    .expect("fixture config resolves");
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(1),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-runtime-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

fn projection(
    session_id: SessionId,
    active: Option<RunProjectionDto>,
    position: u64,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        active.map(RunProjectionDto::config_revision_id),
        active,
        Vec::new(),
        SessionEventSequenceDto::new(position),
    )
    .expect("fixture projection is valid")
}

/// Records the exact durable scheduling-failure append issued by the runtime.
struct FakeRepository {
    snapshot: RefCell<SessionProjectionDto>,
    appends: RefCell<Vec<AppendModelRunFactsInputDto>>,
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "create is not used by this fixture",
        ))
    }

    fn accept_user_turn(&self, _input: AcceptUserTurnInputDto) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "accept is not used by this fixture",
        ))
    }

    fn remove_turn(
        &self,
        _input: intention_storage::RemoveTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "remove is not used by this fixture",
        ))
    }

    fn transition_run(&self, _input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "transition is not used by this fixture",
        ))
    }

    fn recover_unfinished_runs(
        &self,
        _input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<CommittedChangeDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "recovery is not used by this fixture",
        ))
    }

    fn load_session_snapshot(&self, _session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        Ok(self.snapshot.borrow().clone())
    }

    fn load_current_run_snapshot(
        &self,
        _session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunSnapshotDto> {
        let projection = self.snapshot.borrow();
        let run = projection
            .active_run()
            .filter(|run| run.run_id() == run_id)
            .ok_or_else(|| {
                ErrorDto::validation(
                    "run_replay_not_found",
                    "the requested durable run replay does not exist",
                )
            })?;
        Ok(RunSnapshotDto::new(
            run.session_id(),
            run.run_id(),
            projection.at_sequence(),
            ModelRunProjectionDto::new(run, RunEventCursorDto::new(0), None, "", None, None, None)
                .expect("fixture model projection is valid"),
        )
        .expect("fixture snapshot is valid"))
    }

    fn append_model_run_facts(
        &self,
        input: AppendModelRunFactsInputDto,
    ) -> DtoResult<AppendModelRunFactsOutcomeDto> {
        let run = self
            .snapshot
            .borrow()
            .active_run()
            .expect("fixture has an active run");
        let expected = self.appends.borrow().len() as u64;
        let cursor = RunEventCursorDto::new(expected + 1);
        let facts = input
            .facts()
            .iter()
            .map(|fact| {
                intention_domain::ModelRunFactDto::new(cursor, fact.clone())
                    .expect("fixture fact is valid")
            })
            .collect::<Vec<_>>();
        let outcome = AppendModelRunFactsOutcomeDto::new(
            cursor,
            RunSnapshotDto::new(
                input.session_id(),
                input.run_id(),
                self.snapshot.borrow().at_sequence(),
                ModelRunProjectionDto::new(run, cursor, None, "", None, None, None)
                    .expect("fixture model projection is valid"),
            )
            .expect("fixture snapshot is valid"),
            facts,
        )
        .expect("fixture append outcome is valid");
        self.appends.borrow_mut().push(input);
        Ok(outcome)
    }

    fn load_tail(
        &self,
        _session_id: SessionId,
        _after_sequence: SessionEventSequenceDto,
    ) -> DtoResult<Vec<intention_types::EventEnvelopeDto<DomainEventDto>>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "tail is not used by this fixture",
        ))
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "config is not used by this fixture",
        ))
    }
}

fn repository_with_status(session_id: SessionId, status: RunStatusDto) -> FakeRepository {
    let run = RunProjectionDto::new(
        session_id,
        RunId::new(),
        TurnId::new(),
        status,
        snapshot().revision_id(),
    );
    FakeRepository {
        snapshot: RefCell::new(projection(session_id, Some(run), 2)),
        appends: RefCell::new(Vec::new()),
    }
}

#[test]
fn fail_starting_run_records_a_manual_failure_for_the_exact_starting_run() {
    let session_id = SessionId::new();
    let repository = repository_with_status(session_id, RunStatusDto::Starting);
    let run_id = repository
        .snapshot
        .borrow()
        .active_run()
        .expect("fixture has active run")
        .run_id();

    let outcome = fail_starting_run(
        &repository,
        session_id,
        run_id,
        "model_context_unavailable",
        time(72),
    )
    .expect("starting mutation failure commits");

    assert_eq!(outcome.cursor().value(), 1);
    assert_eq!(outcome.facts().len(), 1);
    match outcome.facts()[0].input() {
        ModelRunFactInputDto::Failed { failure } => {
            assert_eq!(failure.code(), "model_context_unavailable");
            assert_eq!(failure.retry(), intention_types::ErrorRetryDto::Manual);
        }
        _ => unreachable!("the terminal failure fact is recorded"),
    }
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 1);
    assert_eq!(appends[0].session_id(), session_id);
    assert_eq!(appends[0].run_id(), run_id);
    assert_eq!(appends[0].expected_cursor().value(), 0);
    assert_eq!(appends[0].status(), Some(RunStatusDto::Failed));
    assert_eq!(appends[0].occurred_at(), time(72));
}

#[test]
fn fail_starting_run_rejects_a_run_that_is_no_longer_starting() {
    let session_id = SessionId::new();
    let repository = repository_with_status(session_id, RunStatusDto::Running);
    let run_id = repository
        .snapshot
        .borrow()
        .active_run()
        .expect("fixture has active run")
        .run_id();

    assert_eq!(
        fail_starting_run(
            &repository,
            session_id,
            run_id,
            "model_context_unavailable",
            time(72),
        )
        .expect_err("a running run is not a scheduling failure")
        .code(),
        "invalid_starting_run_failure_state"
    );
    assert!(repository.appends.borrow().is_empty());
}

#[test]
fn fail_starting_run_propagates_an_absent_run_without_writing() {
    let session_id = SessionId::new();
    let repository = repository_with_status(session_id, RunStatusDto::Starting);
    assert_eq!(
        fail_starting_run(
            &repository,
            session_id,
            RunId::new(),
            "model_context_unavailable",
            time(72),
        )
        .expect_err("an unknown run rejects")
        .code(),
        "run_replay_not_found"
    );
    assert!(repository.appends.borrow().is_empty());
}
