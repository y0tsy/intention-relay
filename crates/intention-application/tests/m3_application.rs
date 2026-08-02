#![allow(
    clippy::expect_used,
    reason = "Focused application fixtures use expect to provide precise test failures."
)]

use std::cell::RefCell;

use intention_application::{
    ApplicationService, CreateSessionWorkflowInputDto, SendUserTurnWorkflowInputDto,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, GetSessionSnapshotQueryDto,
    RemoveQueuedTurnCommandDto, RunModeDto, RunProjectionDto, RunStatusDto, SendUserTurnCommandDto,
    SessionProjectionDto, WorkspaceRootDto,
};
use intention_protocol::{ProtocolAcceptedResultDto, SendUserTurnOutcomeDto};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, CommittedChangeDto, CreateSessionInputDto,
    RecoverUnfinishedRunsInputDto, RemoveQueuedTurnInputDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, ProjectId, QueuePositionDto, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, TurnId, WorkspaceId,
};

fn fixture_time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-application-test.toml")
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
        fixture_time(),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

fn projection(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
    queued_turns: Vec<intention_domain::QueuedTurnProjectionDto>,
    position: u64,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        WorkspaceRootDto::parse("/workspace").expect("workspace is valid"),
        RunModeDto::Build,
        active_run.map(RunProjectionDto::config_revision_id),
        active_run,
        queued_turns,
        SessionEventSequenceDto::new(position),
    )
    .expect("fixture projection is valid")
}

fn change(
    projection: SessionProjectionDto,
    outcome: Option<AcceptedTurnOutcomeDto>,
) -> CommittedChangeDto {
    CommittedChangeDto::new(
        projection.clone(),
        projection.at_sequence(),
        Vec::new(),
        outcome,
    )
    .expect("fixture change is valid")
}

struct FakeRepository {
    accepted: RefCell<DtoResult<CommittedChangeDto>>,
    accepted_inputs: RefCell<Vec<AcceptUserTurnInputDto>>,
    created: RefCell<Option<CommittedChangeDto>>,
    removed: RefCell<Option<CommittedChangeDto>>,
    transitioned: RefCell<Option<CommittedChangeDto>>,
    loaded_snapshot: RefCell<Option<SessionProjectionDto>>,
}

impl FakeRepository {
    const fn with_accepted(accepted: DtoResult<CommittedChangeDto>) -> Self {
        Self {
            accepted: RefCell::new(accepted),
            accepted_inputs: RefCell::new(Vec::new()),
            created: RefCell::new(None),
            removed: RefCell::new(None),
            transitioned: RefCell::new(None),
            loaded_snapshot: RefCell::new(None),
        }
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        self.created.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn accept_user_turn(&self, input: AcceptUserTurnInputDto) -> DtoResult<CommittedChangeDto> {
        self.accepted_inputs.borrow_mut().push(input);
        self.accepted.borrow().clone()
    }

    fn remove_queued_turn(
        &self,
        _input: RemoveQueuedTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        self.removed.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn transition_run(&self, _input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
        self.transitioned.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
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
        self.loaded_snapshot.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
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
fn send_user_turn_maps_durable_started_outcome_and_retries_without_local_enforcement() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let committed = change(
        projection(
            session_id,
            Some(RunProjectionDto::new(
                session_id,
                run_id,
                turn_id,
                RunStatusDto::Starting,
                config.revision_id(),
            )),
            Vec::new(),
            2,
        ),
        Some(AcceptedTurnOutcomeDto::Started(RunProjectionDto::new(
            session_id,
            run_id,
            turn_id,
            RunStatusDto::Starting,
            config.revision_id(),
        ))),
    );
    let repository = FakeRepository::with_accepted(Ok(committed));
    let application = ApplicationService::new(&repository);
    let command =
        SendUserTurnCommandDto::new(session_id, turn_id, "hello").expect("fixture turn is valid");
    let workflow = SendUserTurnWorkflowInputDto::new(run_id, config, fixture_time());

    for _ in 0..2 {
        let result = application
            .send_user_turn(command.clone(), workflow.clone())
            .expect("repository acceptance maps");
        assert_eq!(
            result,
            ProtocolAcceptedResultDto::SendUserTurn(
                intention_protocol::SendUserTurnAcceptedDto::new(
                    session_id,
                    turn_id,
                    SessionEventSequenceDto::new(2),
                    SendUserTurnOutcomeDto::Started {
                        run_id,
                        config_revision_id: workflow.config_snapshot().revision_id(),
                    },
                ),
            )
        );
    }
    assert_eq!(repository.accepted_inputs.borrow().len(), 2);
}

#[test]
fn send_user_turn_propagates_repository_idempotency_conflict() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::validation(
        "turn_idempotency_conflict",
        "the accepted turn identity has different durable content",
    )));
    let application = ApplicationService::new(&repository);

    let error = application
        .send_user_turn(
            SendUserTurnCommandDto::new(SessionId::new(), TurnId::new(), "different")
                .expect("fixture turn is valid"),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
        )
        .expect_err("repository conflict must remain typed");
    assert_eq!(error.code(), "turn_idempotency_conflict");
}

#[test]
fn workflows_expose_their_explicit_durable_inputs() {
    let command = CreateSessionCommandDto::new(
        ProjectId::new(),
        SessionId::new(),
        WorkspaceId::new(),
        WorkspaceRootDto::parse("/workspace").expect("workspace is valid"),
        RunModeDto::Build,
    );
    let create = CreateSessionWorkflowInputDto::new(command.clone(), fixture_time());
    assert_eq!(create.command(), &command);
    assert_eq!(create.occurred_at(), fixture_time());

    let run_id = RunId::new();
    let config = snapshot();
    let send = SendUserTurnWorkflowInputDto::new(run_id, config.clone(), fixture_time());
    assert_eq!(send.proposed_run_id(), run_id);
    assert_eq!(send.config_snapshot(), &config);
    assert_eq!(send.occurred_at(), fixture_time());
}

#[test]
fn send_user_turn_rejects_missing_durable_outcome() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Ok(change(
        projection(session_id, None, Vec::new(), 1),
        None,
    )));
    let error = ApplicationService::new(&repository)
        .send_user_turn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "hello")
                .expect("fixture turn is valid"),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
        )
        .expect_err("accepted commits require a durable outcome");
    assert_eq!(error.code(), "missing_accepted_turn_outcome");
}

#[test]
fn stop_and_snapshot_workflows_map_durable_results() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let state = projection(
        session_id,
        Some(RunProjectionDto::new(
            session_id,
            run_id,
            TurnId::new(),
            RunStatusDto::Starting,
            config.revision_id(),
        )),
        Vec::new(),
        5,
    );
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "fixture_unused",
        "accept is not used by this fixture",
    )));
    *repository.loaded_snapshot.borrow_mut() = Some(state.clone());
    *repository.transitioned.borrow_mut() = Some(change(state.clone(), None));
    let application = ApplicationService::new(&repository);

    let stopped = application
        .stop_run(
            intention_domain::StopRunCommandDto::new(session_id, run_id),
            intention_runtime::RuntimeValuesDto::new(RunId::new(), config, fixture_time()),
        )
        .expect("stop maps");
    assert!(matches!(
        stopped,
        ProtocolAcceptedResultDto::StopRun(value)
            if value.session_id() == session_id && value.run_id() == run_id
    ));

    let snapshot = application
        .get_session_snapshot(GetSessionSnapshotQueryDto::new(session_id))
        .expect("snapshot maps");
    assert_eq!(snapshot.session_id(), session_id);
    assert_eq!(snapshot.projection(), Some(&state));
}

#[test]
fn create_and_remove_workflows_map_committed_queue_results() {
    let session_id = SessionId::new();
    let queued_turn = TurnId::new();
    let repository = FakeRepository::with_accepted(Ok(change(
        projection(session_id, None, Vec::new(), 3),
        Some(AcceptedTurnOutcomeDto::Queued(QueuePositionDto::new(4))),
    )));
    *repository.created.borrow_mut() =
        Some(change(projection(session_id, None, Vec::new(), 1), None));
    *repository.removed.borrow_mut() =
        Some(change(projection(session_id, None, Vec::new(), 4), None));
    let application = ApplicationService::new(&repository);
    let create = CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        WorkspaceRootDto::parse("/workspace").expect("workspace is valid"),
        RunModeDto::Build,
    );

    let created = application
        .create_session(CreateSessionWorkflowInputDto::new(create, fixture_time()))
        .expect("create maps");
    assert!(matches!(
        created,
        ProtocolAcceptedResultDto::CreateSession(_)
    ));
    let queued = application
        .send_user_turn(
            SendUserTurnCommandDto::new(session_id, queued_turn, "queued")
                .expect("fixture turn is valid"),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
        )
        .expect("queue maps");
    assert!(matches!(
        queued,
        ProtocolAcceptedResultDto::SendUserTurn(value)
            if value.outcome() == SendUserTurnOutcomeDto::Queued { queue_position: QueuePositionDto::new(4) }
    ));
    let removed = application
        .remove_queued_turn(
            RemoveQueuedTurnCommandDto::new(session_id, queued_turn),
            fixture_time(),
        )
        .expect("removal maps");
    assert!(matches!(
        removed,
        ProtocolAcceptedResultDto::RemoveQueuedTurn(_)
    ));

    let _ = GetSessionSnapshotQueryDto::new(session_id);
}
