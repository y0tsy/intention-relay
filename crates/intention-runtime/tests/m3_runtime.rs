#![allow(
    clippy::expect_used,
    reason = "Focused runtime fixtures use expect to provide precise test failures."
)]

use std::cell::RefCell;

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    DomainEventDto, QueuedTurnProjectionDto, RunModeDto, RunProjectionDto, RunStatusDto,
    SessionProjectionDto, WorkspaceRootDto,
};
use intention_runtime::{RuntimeService, RuntimeValuesDto};
use intention_storage::{
    AcceptUserTurnInputDto, CommittedChangeDto, CreateSessionInputDto,
    RecoverUnfinishedRunsInputDto, StorageRepositoryDto, TransitionRunInputDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, ProjectId, QueuePositionDto, RunId, SchemaVersionDto,
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

fn projection_with_position(
    session_id: SessionId,
    active: Option<RunProjectionDto>,
    queued: Vec<QueuedTurnProjectionDto>,
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
        queued,
        SessionEventSequenceDto::new(position),
    )
    .expect("fixture projection is valid")
}

fn projection(
    session_id: SessionId,
    active: Option<RunProjectionDto>,
    queued: Vec<QueuedTurnProjectionDto>,
) -> SessionProjectionDto {
    projection_with_position(session_id, active, queued, 2)
}

fn change(projection: SessionProjectionDto) -> CommittedChangeDto {
    CommittedChangeDto::new(
        projection.clone(),
        projection.at_sequence(),
        Vec::new(),
        None,
    )
    .expect("fixture change is valid")
}

struct FakeRepository {
    snapshot: RefCell<SessionProjectionDto>,
    transitions: RefCell<Vec<TransitionRunInputDto>>,
    recovery_inputs: RefCell<Vec<RecoverUnfinishedRunsInputDto>>,
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "create is not used by this fixture",
        ))
    }

    fn accept_user_turn(
        &self,
        _input: intention_storage::AcceptUserTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "accept is not used by this fixture",
        ))
    }

    fn remove_queued_turn(
        &self,
        _input: intention_storage::RemoveQueuedTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "remove is not used by this fixture",
        ))
    }

    fn transition_run(&self, input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
        let mut current_projection = self.snapshot.borrow_mut();
        if input.status() == RunStatusDto::Cancelling {
            let active = current_projection
                .active_run()
                .expect("fixture has active run");
            *current_projection = projection(
                input.session_id(),
                Some(RunProjectionDto::new(
                    active.session_id(),
                    active.run_id(),
                    active.turn_id(),
                    RunStatusDto::Cancelling,
                    active.config_revision_id(),
                )),
                current_projection.queued_turns().to_vec(),
            );
        }
        self.transitions.borrow_mut().push(input);
        Ok(change(current_projection.clone()))
    }

    fn recover_unfinished_runs(
        &self,
        input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<CommittedChangeDto>> {
        self.recovery_inputs.borrow_mut().push(input);
        Ok(Vec::new())
    }

    fn load_session_snapshot(&self, _session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        Ok(self.snapshot.borrow().clone())
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

struct DurablePromotionRepository {
    snapshot: RefCell<SessionProjectionDto>,
    queued_run_id: RunId,
    queued_revision_id: ConfigRevisionId,
}

impl StorageRepositoryDto for DurablePromotionRepository {
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

    fn remove_queued_turn(
        &self,
        _input: intention_storage::RemoveQueuedTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "remove is not used by this fixture",
        ))
    }

    fn transition_run(&self, input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
        let queued_turn = self
            .snapshot
            .borrow()
            .queued_turns()
            .first()
            .expect("terminal fixture has queued turn")
            .turn_id();
        let active = self
            .snapshot
            .borrow()
            .active_run()
            .expect("fixture has active run");
        let next = projection_with_position(
            input.session_id(),
            Some(RunProjectionDto::new(
                input.session_id(),
                self.queued_run_id,
                queued_turn,
                RunStatusDto::Starting,
                self.queued_revision_id,
            )),
            Vec::new(),
            4,
        );
        assert_eq!(active.run_id(), input.run_id());
        *self.snapshot.borrow_mut() = next.clone();
        Ok(change(next))
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

#[test]
fn status_graph_allows_declared_edges_and_rejects_forbidden_edges() {
    assert!(RuntimeService::<FakeRepository>::can_transition(
        RunStatusDto::Starting,
        RunStatusDto::Cancelling
    ));
    assert!(RuntimeService::<FakeRepository>::can_transition(
        RunStatusDto::Cancelling,
        RunStatusDto::Cancelled
    ));
    assert!(!RuntimeService::<FakeRepository>::can_transition(
        RunStatusDto::Starting,
        RunStatusDto::Completed
    ));
    assert!(!RuntimeService::<FakeRepository>::can_transition(
        RunStatusDto::Completed,
        RunStatusDto::Starting
    ));
}

#[test]
fn stopping_starting_run_cancels_then_atomically_promotes_next_turn() {
    let session_id = SessionId::new();
    let active_id = RunId::new();
    let next_turn = TurnId::new();
    let config = snapshot();
    let repository = FakeRepository {
        snapshot: RefCell::new(projection(
            session_id,
            Some(RunProjectionDto::new(
                session_id,
                active_id,
                TurnId::new(),
                RunStatusDto::Starting,
                config.revision_id(),
            )),
            vec![
                QueuedTurnProjectionDto::new(
                    session_id,
                    next_turn,
                    "next",
                    QueuePositionDto::new(0),
                )
                .expect("queued fixture is valid"),
            ],
        )),
        transitions: RefCell::new(Vec::new()),
        recovery_inputs: RefCell::new(Vec::new()),
    };
    let runtime = RuntimeService::new(
        &repository,
        RuntimeValuesDto::new(RunId::new(), config, time(10)),
    );

    runtime
        .stop_run(session_id, active_id)
        .expect("starting run cancels");
    runtime
        .complete_terminal(session_id, active_id, RunStatusDto::Cancelled)
        .expect("terminal transition promotes queue atomically");

    let transitions = repository.transitions.borrow();
    assert_eq!(transitions.len(), 2);
    assert_eq!(transitions[0].status(), RunStatusDto::Cancelling);
    assert_eq!(transitions[1].status(), RunStatusDto::Cancelled);
}

#[test]
fn terminal_promotion_uses_queued_snapshot_after_runtime_configuration_changes() {
    let session_id = SessionId::new();
    let active_run = RunId::new();
    let queued_turn = TurnId::new();
    let queued_run = RunId::new();
    let config_a = snapshot();
    let repository = DurablePromotionRepository {
        snapshot: RefCell::new(projection(
            session_id,
            Some(RunProjectionDto::new(
                session_id,
                active_run,
                TurnId::new(),
                RunStatusDto::Starting,
                config_a.revision_id(),
            )),
            vec![
                QueuedTurnProjectionDto::new(
                    session_id,
                    queued_turn,
                    "queued",
                    QueuePositionDto::new(0),
                )
                .expect("queued fixture is valid"),
            ],
        )),
        queued_run_id: queued_run,
        queued_revision_id: config_a.revision_id(),
    };
    let config_b = snapshot();
    RuntimeService::new(
        &repository,
        RuntimeValuesDto::new(RunId::new(), config_b, time(4)),
    )
    .complete_terminal(session_id, active_run, RunStatusDto::Failed)
    .expect("runtime promotes queued selection atomically");
    let active = repository
        .load_session_snapshot(session_id)
        .expect("snapshot loads")
        .active_run()
        .expect("queued run is active");
    assert_eq!(active.run_id(), queued_run);
    assert_eq!(active.config_revision_id(), config_a.revision_id());
    assert!(
        repository
            .load_session_snapshot(session_id)
            .expect("snapshot loads")
            .queued_turns()
            .is_empty()
    );
}

#[test]
fn runtime_values_expose_the_selected_durable_values() {
    let run_id = RunId::new();
    let config = snapshot();
    let values = RuntimeValuesDto::new(run_id, config.clone(), time(9));
    assert_eq!(values.next_run_id(), run_id);
    assert_eq!(values.config_snapshot(), &config);
    assert_eq!(values.occurred_at(), time(9));
}

#[test]
fn stop_and_completion_reject_missing_or_mismatched_runs() {
    let session_id = SessionId::new();
    let values = RuntimeValuesDto::new(RunId::new(), snapshot(), time(9));
    let absent_repository = FakeRepository {
        snapshot: RefCell::new(projection(session_id, None, Vec::new())),
        transitions: RefCell::new(Vec::new()),
        recovery_inputs: RefCell::new(Vec::new()),
    };
    let absent = RuntimeService::new(&absent_repository, values.clone());
    assert_eq!(
        absent
            .stop_run(session_id, RunId::new())
            .expect_err("missing active run rejects")
            .code(),
        "active_run_not_found"
    );
    assert_eq!(
        absent
            .complete_terminal(session_id, RunId::new(), RunStatusDto::Completed)
            .expect_err("missing active run rejects")
            .code(),
        "active_run_not_found"
    );

    let active_id = RunId::new();
    let mismatched_repository = FakeRepository {
        snapshot: RefCell::new(projection(
            session_id,
            Some(RunProjectionDto::new(
                session_id,
                active_id,
                TurnId::new(),
                RunStatusDto::Starting,
                values.config_snapshot().revision_id(),
            )),
            Vec::new(),
        )),
        transitions: RefCell::new(Vec::new()),
        recovery_inputs: RefCell::new(Vec::new()),
    };
    let mismatched = RuntimeService::new(&mismatched_repository, values);
    assert_eq!(
        mismatched
            .stop_run(session_id, RunId::new())
            .expect_err("different run rejects")
            .code(),
        "active_run_not_found"
    );
    assert_eq!(
        mismatched
            .complete_terminal(session_id, RunId::new(), RunStatusDto::Completed)
            .expect_err("different run rejects")
            .code(),
        "active_run_not_found"
    );
}

#[test]
fn completion_rejects_non_terminal_status() {
    let repository = FakeRepository {
        snapshot: RefCell::new(projection(SessionId::new(), None, Vec::new())),
        transitions: RefCell::new(Vec::new()),
        recovery_inputs: RefCell::new(Vec::new()),
    };
    let error = RuntimeService::new(
        &repository,
        RuntimeValuesDto::new(RunId::new(), snapshot(), time(9)),
    )
    .complete_terminal(SessionId::new(), RunId::new(), RunStatusDto::Running)
    .expect_err("non-terminal completion rejects");
    assert_eq!(error.code(), "invalid_terminal_run_status");
}

#[test]
fn recovery_requests_deterministic_interruption_before_facade_readiness() {
    let session_id = SessionId::new();
    let repository = FakeRepository {
        snapshot: RefCell::new(projection(session_id, None, Vec::new())),
        transitions: RefCell::new(Vec::new()),
        recovery_inputs: RefCell::new(Vec::new()),
    };
    let runtime = RuntimeService::new(
        &repository,
        RuntimeValuesDto::new(RunId::new(), snapshot(), time(42)),
    );

    runtime.recover_before_ready().expect("recovery completes");
    assert_eq!(
        repository.recovery_inputs.borrow().as_slice(),
        &[RecoverUnfinishedRunsInputDto::new(time(42))]
    );
}
