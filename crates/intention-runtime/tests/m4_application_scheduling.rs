#![allow(
    clippy::expect_used,
    reason = "Focused runtime scheduling fixtures use expect for precise diagnostics."
)]

use std::cell::RefCell;

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    DomainEventDto, ModelRunFactDto, ModelRunFactInputDto, ModelRunProjectionDto,
    RunEventCursorDto, RunEventTailPageDto, RunProjectionDto, RunReplayDto, RunSnapshotDto,
    RunStatusDto, SessionProjectionDto,
};
use intention_runtime::fail_starting_run;
use intention_storage::{
    AcceptUserTurnInputDto, AppendModelRunFactsInputDto, AppendModelRunFactsOutcomeDto,
    CommittedChangeDto, CreateSessionInputDto, RecoverUnfinishedRunsInputDto,
    RemoveQueuedTurnInputDto, StorageRepositoryDto, TransitionRunInputDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, ErrorRetryDto, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, TurnId,
};

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-runtime-scheduling-test.toml")
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
        time(),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

struct FakeRepository {
    session_id: SessionId,
    run_id: RunId,
    config: ConfigSnapshotDto,
    status: RefCell<RunStatusDto>,
    cursor: RefCell<RunEventCursorDto>,
    appends: RefCell<Vec<AppendModelRunFactsInputDto>>,
}

impl FakeRepository {
    fn new(status: RunStatusDto) -> Self {
        Self {
            session_id: SessionId::new(),
            run_id: RunId::new(),
            config: snapshot(),
            status: RefCell::new(status),
            cursor: RefCell::new(RunEventCursorDto::new(4)),
            appends: RefCell::new(Vec::new()),
        }
    }

    fn replay(&self) -> DtoResult<RunReplayDto> {
        let run = RunProjectionDto::new(
            self.session_id,
            self.run_id,
            TurnId::new(),
            *self.status.borrow(),
            self.config.revision_id(),
        );
        let cursor = *self.cursor.borrow();
        let projection = ModelRunProjectionDto::new(run, cursor, None, "", None, None, None)?;
        let snapshot = RunSnapshotDto::new(
            self.session_id,
            self.run_id,
            SessionEventSequenceDto::new(cursor.value()),
            projection,
        )?;
        RunReplayDto::new(
            snapshot,
            RunEventTailPageDto::empty(self.session_id, self.run_id, cursor),
        )
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        Err(unused())
    }

    fn accept_user_turn(&self, _input: AcceptUserTurnInputDto) -> DtoResult<CommittedChangeDto> {
        Err(unused())
    }

    fn remove_queued_turn(
        &self,
        _input: RemoveQueuedTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        Err(unused())
    }

    fn transition_run(&self, _input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
        Err(unused())
    }

    fn append_model_run_facts(
        &self,
        input: AppendModelRunFactsInputDto,
    ) -> DtoResult<AppendModelRunFactsOutcomeDto> {
        assert_eq!(input.expected_cursor(), *self.cursor.borrow());
        assert_eq!(input.status(), Some(RunStatusDto::Failed));
        let facts = input
            .facts()
            .iter()
            .cloned()
            .enumerate()
            .map(|(offset, fact)| {
                ModelRunFactDto::new(
                    RunEventCursorDto::new(input.expected_cursor().value() + offset as u64 + 1),
                    fact,
                )
            })
            .collect::<DtoResult<Vec<_>>>()?;
        let cursor = facts
            .last()
            .map_or_else(|| input.expected_cursor(), ModelRunFactDto::cursor);
        *self.cursor.borrow_mut() = cursor;
        *self.status.borrow_mut() = RunStatusDto::Failed;
        self.appends.borrow_mut().push(input);
        let replay = self.replay()?;
        AppendModelRunFactsOutcomeDto::new(cursor, replay.snapshot().clone(), facts)
    }

    fn load_current_run_replay(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        assert_eq!((session_id, run_id), (self.session_id, self.run_id));
        self.replay()
    }

    fn recover_unfinished_runs(
        &self,
        _input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<CommittedChangeDto>> {
        Err(unused())
    }

    fn load_session_snapshot(&self, _session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        Err(unused())
    }

    fn load_tail(
        &self,
        _session_id: SessionId,
        _after_sequence: SessionEventSequenceDto,
    ) -> DtoResult<Vec<intention_types::EventEnvelopeDto<DomainEventDto>>> {
        Err(unused())
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(unused())
    }
}

#[test]
fn failure_helper_appends_one_manual_failure_only_for_the_exact_starting_run() {
    let repository = FakeRepository::new(RunStatusDto::Starting);
    let outcome = fail_starting_run(
        &repository,
        repository.session_id,
        repository.run_id,
        "model_scheduling_unavailable",
        time(),
    )
    .expect("starting run can fail atomically");

    assert_eq!(outcome.cursor(), RunEventCursorDto::new(5));
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 1);
    assert!(matches!(
        appends[0].facts(),
        [ModelRunFactInputDto::Failed { failure }]
            if failure.code() == "model_scheduling_unavailable"
                && failure.retry() == ErrorRetryDto::Manual
    ));
    assert_eq!(appends[0].status(), Some(RunStatusDto::Failed));

    let wrong_state = FakeRepository::new(RunStatusDto::Running);
    let error = fail_starting_run(
        &wrong_state,
        wrong_state.session_id,
        wrong_state.run_id,
        "model_context_unavailable",
        time(),
    )
    .expect_err("running run must not be failed by scheduling recovery");
    assert_eq!(error.code(), "invalid_starting_run_failure_state");
    assert!(wrong_state.appends.borrow().is_empty());
}

fn unused() -> ErrorDto {
    ErrorDto::unavailable("fixture_unused", "unused")
}
