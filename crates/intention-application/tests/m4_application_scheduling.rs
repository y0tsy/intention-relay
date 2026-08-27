#![allow(
    clippy::expect_used,
    reason = "Focused application scheduling fixtures use expect for precise diagnostics."
)]

use std::cell::RefCell;

use intention_application::{
    ApplicationService, ModelRunDispatchPort, ScheduleModelRunDto, SendUserTurnWorkflowInputDto,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    DomainEventDto, ModelRunFactDto, ModelRunFactInputDto, ModelRunProjectionDto,
    RunEventCursorDto, RunEventTailPageDto, RunModeDto, RunProjectionDto, RunReplayDto,
    RunSnapshotDto, RunStartedEventDto, RunStatusDto, SendUserTurnCommandDto, SessionProjectionDto,
    UserTurnAcceptedEventDto, WorkspaceRootDto,
};
use intention_protocol::{ProtocolAcceptedResultDto, SendUserTurnOutcomeDto};
use intention_runtime::{ModelMessageDto, ModelRequestDto, ModelRoleDto};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendModelRunFactsInputDto,
    AppendModelRunFactsOutcomeDto, CommittedChangeDto, CreateSessionInputDto,
    ModelContextMessageDto, ModelContextRoleDto, RecoverUnfinishedRunsInputDto,
    RemoveQueuedTurnInputDto, StartingRunModelContextDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, ErrorRetryDto, EventEnvelopeDto, EventId,
    EventMetadataDto, ProjectId, QueuePositionDto, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, TurnId, WorkspaceId,
};

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-application-scheduling-test.toml")
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

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
        .expect("fixture workspace is valid")
}

fn projection(session_id: SessionId, run: RunProjectionDto, position: u64) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        Some(run.config_revision_id()),
        Some(run),
        Vec::new(),
        SessionEventSequenceDto::new(position),
    )
    .expect("fixture projection is valid")
}

fn change(run: RunProjectionDto, position: u64) -> CommittedChangeDto {
    let projection = projection(run.session_id(), run, position);
    CommittedChangeDto::new(
        projection.clone(),
        projection.at_sequence(),
        vec![
            EventEnvelopeDto::new(
                EventMetadataDto::new(
                    SchemaVersionDto::new(1, 0),
                    EventId::new(),
                    projection.session_id(),
                    None,
                    Some(run.turn_id()),
                    SessionEventSequenceDto::new(position - 1),
                    time(),
                ),
                DomainEventDto::UserTurnAccepted(
                    UserTurnAcceptedEventDto::new(
                        projection.session_id(),
                        run.turn_id(),
                        "latest",
                        time(),
                    )
                    .expect("fixture acceptance event is valid"),
                ),
            ),
            EventEnvelopeDto::new(
                EventMetadataDto::new(
                    SchemaVersionDto::new(1, 0),
                    EventId::new(),
                    projection.session_id(),
                    Some(run.run_id()),
                    Some(run.turn_id()),
                    SessionEventSequenceDto::new(position),
                    time(),
                ),
                DomainEventDto::RunStarted(RunStartedEventDto::new(
                    projection.session_id(),
                    run.run_id(),
                    run.turn_id(),
                    run.config_revision_id(),
                    time(),
                )),
            ),
        ],
        Some(AcceptedTurnOutcomeDto::Started(run)),
    )
    .expect("fixture accepted change is valid")
}

fn idempotent_change(run: RunProjectionDto, position: u64) -> CommittedChangeDto {
    let projection = projection(run.session_id(), run, position);
    CommittedChangeDto::new(
        projection.clone(),
        projection.at_sequence(),
        Vec::new(),
        Some(AcceptedTurnOutcomeDto::Started(run)),
    )
    .expect("fixture idempotent accepted change is valid")
}

struct FakeRepository {
    accepted: RefCell<Vec<DtoResult<CommittedChangeDto>>>,
    accepted_calls: RefCell<usize>,
    context: RefCell<DtoResult<StartingRunModelContextDto>>,
    status: RefCell<RunStatusDto>,
    cursor: RefCell<RunEventCursorDto>,
    appends: RefCell<Vec<AppendModelRunFactsInputDto>>,
}

impl FakeRepository {
    fn new(
        accepted: DtoResult<CommittedChangeDto>,
        context: DtoResult<StartingRunModelContextDto>,
        status: RunStatusDto,
    ) -> Self {
        Self {
            accepted: RefCell::new(vec![accepted]),
            accepted_calls: RefCell::new(0),
            context: RefCell::new(context),
            status: RefCell::new(status),
            cursor: RefCell::new(RunEventCursorDto::new(0)),
            appends: RefCell::new(Vec::new()),
        }
    }

    fn replay(
        &self,
        session_id: SessionId,
        run_id: RunId,
        revision: ConfigRevisionId,
    ) -> DtoResult<RunReplayDto> {
        let cursor = *self.cursor.borrow();
        let run = RunProjectionDto::new(
            session_id,
            run_id,
            TurnId::new(),
            *self.status.borrow(),
            revision,
        );
        let projection = ModelRunProjectionDto::new(run, cursor, None, "", None, None, None)?;
        let snapshot = RunSnapshotDto::new(
            session_id,
            run_id,
            SessionEventSequenceDto::new(cursor.value()),
            projection,
        )?;
        RunReplayDto::new(
            snapshot,
            RunEventTailPageDto::empty(session_id, run_id, cursor),
        )
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        Err(unused())
    }

    fn accept_user_turn(&self, _input: AcceptUserTurnInputDto) -> DtoResult<CommittedChangeDto> {
        let call = *self.accepted_calls.borrow();
        *self.accepted_calls.borrow_mut() += 1;
        let accepted = self.accepted.borrow();
        accepted
            .get(call)
            .or_else(|| accepted.last())
            .cloned()
            .unwrap_or_else(|| Err(unused()))
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
        *self.status.borrow_mut() = input.status().unwrap_or_else(|| *self.status.borrow());
        self.appends.borrow_mut().push(input.clone());
        let revision = self
            .context
            .borrow()
            .as_ref()
            .map(StartingRunModelContextDto::safe_config)
            .map(ConfigSnapshotDto::revision_id)
            .unwrap_or_else(|_| ConfigRevisionId::new());
        let replay = self.replay(input.session_id(), input.run_id(), revision)?;
        AppendModelRunFactsOutcomeDto::new(cursor, replay.snapshot().clone(), facts)
    }

    fn load_starting_run_model_context(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto> {
        self.context.borrow().clone()
    }

    fn load_current_run_replay(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        let revision = self
            .context
            .borrow()
            .as_ref()
            .map(StartingRunModelContextDto::safe_config)
            .map(ConfigSnapshotDto::revision_id)
            .unwrap_or_else(|_| ConfigRevisionId::new());
        self.replay(session_id, run_id, revision)
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

#[derive(Default)]
struct RecordingDispatchPort {
    inputs: RefCell<Vec<ScheduleModelRunDto>>,
    failure: RefCell<Option<ErrorDto>>,
}

impl ModelRunDispatchPort for RecordingDispatchPort {
    fn dispatch_model_run(&self, input: ScheduleModelRunDto) -> DtoResult<()> {
        self.inputs.borrow_mut().push(input);
        self.failure.borrow_mut().take().map_or(Ok(()), Err)
    }
}

fn context(
    session_id: SessionId,
    run_id: RunId,
    config: ConfigSnapshotDto,
) -> StartingRunModelContextDto {
    StartingRunModelContextDto::new(
        session_id,
        run_id,
        config,
        vec![
            ModelContextMessageDto::new(ModelContextRoleDto::User, "first")
                .expect("fixture context message is valid"),
            ModelContextMessageDto::new(ModelContextRoleDto::Assistant, "answer")
                .expect("fixture context message is valid"),
            ModelContextMessageDto::new(ModelContextRoleDto::User, "latest")
                .expect("fixture context message is valid"),
        ],
    )
    .expect("fixture context is valid")
}

fn command(session_id: SessionId, turn_id: TurnId) -> SendUserTurnCommandDto {
    SendUserTurnCommandDto::new(session_id, turn_id, "latest").expect("fixture command is valid")
}

#[test]
fn queued_acceptance_never_loads_context_or_dispatches() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let config = snapshot();
    let run = RunProjectionDto::new(
        session_id,
        RunId::new(),
        turn_id,
        RunStatusDto::Starting,
        config.revision_id(),
    );
    let accepted = CommittedChangeDto::new(
        projection(session_id, run, 3),
        SessionEventSequenceDto::new(3),
        Vec::new(),
        Some(AcceptedTurnOutcomeDto::Queued(QueuePositionDto::new(0))),
    )
    .expect("queued change is valid");
    let repository = FakeRepository::new(Ok(accepted), Err(unused()), RunStatusDto::Starting);
    let dispatch = RecordingDispatchPort::default();

    let result = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            command(session_id, turn_id),
            SendUserTurnWorkflowInputDto::new(RunId::new(), config, time()),
            &dispatch,
        )
        .expect("queued result remains accepted");

    assert!(matches!(
        result,
        ProtocolAcceptedResultDto::SendUserTurn(value)
            if value.outcome() == SendUserTurnOutcomeDto::Queued { queue_position: QueuePositionDto::new(0) }
    ));
    assert!(dispatch.inputs.borrow().is_empty());
    assert!(repository.appends.borrow().is_empty());
}

#[test]
fn started_acceptance_dispatches_exact_dto_after_commit() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let run = RunProjectionDto::new(
        session_id,
        run_id,
        turn_id,
        RunStatusDto::Starting,
        config.revision_id(),
    );
    let repository = FakeRepository::new(
        Ok(change(run, 2)),
        Ok(context(session_id, run_id, config.clone())),
        RunStatusDto::Starting,
    );
    let dispatch = RecordingDispatchPort::default();

    let result = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            command(session_id, turn_id),
            SendUserTurnWorkflowInputDto::new(run_id, config.clone(), time()),
            &dispatch,
        )
        .expect("started result remains accepted");

    assert!(matches!(result, ProtocolAcceptedResultDto::SendUserTurn(_)));
    let dispatched = dispatch.inputs.borrow();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].session_id(), session_id);
    assert_eq!(dispatched[0].run_id(), run_id);
    assert_eq!(dispatched[0].safe_config(), &config);
    assert_eq!(
        dispatched[0].request(),
        &ModelRequestDto::new(
            run_id,
            "fixture",
            vec![
                ModelMessageDto::new(ModelRoleDto::User, "first").expect("message is valid"),
                ModelMessageDto::new(ModelRoleDto::Assistant, "answer").expect("message is valid"),
                ModelMessageDto::new(ModelRoleDto::User, "latest").expect("message is valid"),
            ],
            None,
            None,
        )
        .expect("request is valid")
    );
}

#[test]
fn identical_started_retry_preserves_acceptance_and_dispatches_once() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let run = RunProjectionDto::new(
        session_id,
        run_id,
        turn_id,
        RunStatusDto::Starting,
        config.revision_id(),
    );
    let repository = FakeRepository {
        accepted: RefCell::new(vec![Ok(change(run, 2)), Ok(idempotent_change(run, 2))]),
        accepted_calls: RefCell::new(0),
        context: RefCell::new(Ok(context(session_id, run_id, config.clone()))),
        status: RefCell::new(RunStatusDto::Starting),
        cursor: RefCell::new(RunEventCursorDto::new(0)),
        appends: RefCell::new(Vec::new()),
    };
    let dispatch = RecordingDispatchPort::default();
    let application = ApplicationService::new(&repository);
    let command = command(session_id, turn_id);
    let input = SendUserTurnWorkflowInputDto::new(run_id, config, time());

    let initial = application
        .send_user_turn_and_schedule(command.clone(), input.clone(), &dispatch)
        .expect("initial started acceptance remains accepted");
    let retry = application
        .send_user_turn_and_schedule(command, input, &dispatch)
        .expect("identical started retry remains accepted");

    assert_eq!(retry, initial);
    assert_eq!(*repository.accepted_calls.borrow(), 2);
    assert_eq!(dispatch.inputs.borrow().len(), 1);
}

#[test]
fn mismatched_context_identity_fails_accepted_run_without_dispatching() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let run = RunProjectionDto::new(
        session_id,
        run_id,
        turn_id,
        RunStatusDto::Starting,
        config.revision_id(),
    );
    let repository = FakeRepository::new(
        Ok(change(run, 2)),
        Ok(context(SessionId::new(), RunId::new(), config.clone())),
        RunStatusDto::Starting,
    );
    let dispatch = RecordingDispatchPort::default();

    let result = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            command(session_id, turn_id),
            SendUserTurnWorkflowInputDto::new(run_id, config, time()),
            &dispatch,
        )
        .expect("identity mismatch preserves accepted result");

    assert!(matches!(result, ProtocolAcceptedResultDto::SendUserTurn(_)));
    assert!(dispatch.inputs.borrow().is_empty());
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 1);
    assert_eq!(appends[0].session_id(), session_id);
    assert_eq!(appends[0].run_id(), run_id);
    assert!(matches!(
        appends[0].facts(),
        [ModelRunFactInputDto::Failed { failure }]
            if failure.code() == "model_context_unavailable" && failure.retry() == ErrorRetryDto::Manual
    ));
}

#[test]
fn context_or_dispatch_failure_marks_starting_run_failed_and_keeps_original_acceptance() {
    for (context_result, dispatch_error, expected_code) in [
        (
            Err(ErrorDto::unavailable(
                "run_model_context_unavailable",
                "unavailable",
            )),
            None,
            "model_context_unavailable",
        ),
        (
            Ok(()),
            Some(ErrorDto::unavailable(
                "fixture_dispatch_failure",
                "unavailable",
            )),
            "model_scheduling_unavailable",
        ),
    ] {
        let session_id = SessionId::new();
        let turn_id = TurnId::new();
        let run_id = RunId::new();
        let config = snapshot();
        let run = RunProjectionDto::new(
            session_id,
            run_id,
            turn_id,
            RunStatusDto::Starting,
            config.revision_id(),
        );
        let actual_context = context_result.map(|()| context(session_id, run_id, config.clone()));
        let repository =
            FakeRepository::new(Ok(change(run, 2)), actual_context, RunStatusDto::Starting);
        let dispatch = RecordingDispatchPort::default();
        *dispatch.failure.borrow_mut() = dispatch_error;

        let result = ApplicationService::new(&repository)
            .send_user_turn_and_schedule(
                command(session_id, turn_id),
                SendUserTurnWorkflowInputDto::new(run_id, config, time()),
                &dispatch,
            )
            .expect("post-commit scheduling failure preserves acceptance");

        assert!(matches!(result, ProtocolAcceptedResultDto::SendUserTurn(_)));
        let appends = repository.appends.borrow();
        assert_eq!(appends.len(), 1);
        assert!(matches!(
            appends[0].facts(),
            [ModelRunFactInputDto::Failed { failure }]
                if failure.code() == expected_code && failure.retry() == ErrorRetryDto::Manual
        ));
        assert_eq!(appends[0].status(), Some(RunStatusDto::Failed));
        assert_eq!(
            dispatch.inputs.borrow().len(),
            usize::from(expected_code == "model_scheduling_unavailable")
        );
    }
}

#[test]
fn admission_failure_returns_its_typed_error_without_dispatch_or_failure_append() {
    let repository = FakeRepository::new(
        Err(ErrorDto::validation("turn_admission_denied", "denied")),
        Err(unused()),
        RunStatusDto::Starting,
    );
    let dispatch = RecordingDispatchPort::default();
    let error = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            command(SessionId::new(), TurnId::new()),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), time()),
            &dispatch,
        )
        .expect_err("admission rejection remains typed");
    assert_eq!(error.code(), "turn_admission_denied");
    assert!(dispatch.inputs.borrow().is_empty());
    assert!(repository.appends.borrow().is_empty());
}

#[test]
fn schedule_starting_run_maps_context_into_dispatch_dto() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let repository = FakeRepository::new(
        Err(unused()),
        Ok(context(session_id, run_id, config.clone())),
        RunStatusDto::Starting,
    );

    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, run_id)
        .expect("starting context schedules");
    assert_eq!(scheduled.session_id(), session_id);
    assert_eq!(scheduled.run_id(), run_id);
    assert_eq!(scheduled.safe_config(), &config);
    assert_eq!(scheduled.request().messages().len(), 3);
}

#[test]
fn schedule_starting_run_propagates_context_load_error() {
    let repository = FakeRepository::new(
        Err(unused()),
        Err(ErrorDto::unavailable("context_down", "context unavailable")),
        RunStatusDto::Starting,
    );
    let error = ApplicationService::new(&repository)
        .schedule_starting_run(SessionId::new(), RunId::new())
        .expect_err("context error is propagated");
    assert_eq!(error.code(), "context_down");
}

#[test]
fn schedule_starting_run_rejects_context_that_cannot_form_a_request() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let context = context(session_id, run_id, config.clone());
    let repository = FakeRepository::new(Err(unused()), Ok(context), RunStatusDto::Starting);

    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, run_id)
        .expect("context schedules");
    assert_eq!(scheduled.session_id(), session_id);
    assert_eq!(scheduled.run_id(), run_id);
    assert_eq!(scheduled.safe_config(), &config);
    assert_eq!(scheduled.request().messages().len(), 3);
}

fn unused() -> ErrorDto {
    ErrorDto::unavailable("fixture_unused", "unused")
}
