#![allow(
    clippy::expect_used,
    reason = "Execution fixtures use expect to provide precise failures."
)]

use std::{cell::RefCell, future, time::Duration};

use futures_util::{StreamExt, stream};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_proto::{
    ConfigRevisionId, DtoResult, ErrorDto, ProjectId, RunId, SchemaVersionDto, SessionId,
    TimestampDto, TurnId, WorkspaceId,
};
use intention_proto::{
    MessageKindDto, MessageProjectionDto, RunModeDto, RunProjectionDto, RunStatusDto,
    SessionProjectionDto, WorkspaceRootDto,
};
use intention_providers::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto, ToolCallDto, UsageDto,
};
use intention_runtime::{
    ModelRunCommitDto, ModelRunCommitObserver, ModelRunExecutionInputDto,
    ModelRunExecutionOutcomeDto, ModelRunExecutionService, ModelSleepFuture, ModelTimePort,
    ToolExecutionPort, ToolResultOutcomeDto,
};
use intention_storage::{
    AppendMessageInputDto, ConsumePendingUserTurnsInputDto, CreateSessionInputDto,
    FinishRunInputDto, RemoveTurnInputDto, StorageRepositoryDto, TransitionRunInputDto,
};

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
}

fn snapshot(model: &str) -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-runtime-execution.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    );
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        format!("schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"{model}\"\ncredential = \"fixture-secret\""),
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

struct ImmediateTime {
    sleeps: RefCell<Vec<Duration>>,
}

impl ImmediateTime {
    const fn new() -> Self {
        Self {
            sleeps: RefCell::new(Vec::new()),
        }
    }
}

impl ModelTimePort for ImmediateTime {
    fn now(&self) -> TimestampDto {
        time(2)
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        self.sleeps.borrow_mut().push(duration);
        Box::pin(future::ready(()))
    }
}

fn request(run_id: RunId, model: &str) -> ModelRequestDto {
    ModelRequestDto::new(
        run_id,
        model,
        vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
        None,
        None,
    )
    .expect("request is valid")
}

/// Records every committed transcript row, run outcome, and status transition.
struct FakeRepository {
    session_id: SessionId,
    run_id: RunId,
    turn_id: TurnId,
    config: ConfigSnapshotDto,
    status: RefCell<RunStatusDto>,
    messages: RefCell<Vec<MessageProjectionDto>>,
    finishes: RefCell<Vec<FinishRunInputDto>>,
    transitions: RefCell<Vec<TransitionRunInputDto>>,
    config_error: RefCell<Option<ErrorDto>>,
    append_failure: RefCell<Option<ErrorDto>>,
    /// Fails exactly the append whose one-based insertion index matches.
    append_failure_at: RefCell<Option<(usize, ErrorDto)>>,
    append_count: RefCell<usize>,
    transition_failure: RefCell<Option<(RunStatusDto, ErrorDto)>>,
}

impl FakeRepository {
    fn new(session_id: SessionId, run_id: RunId, config: ConfigSnapshotDto) -> Self {
        Self {
            session_id,
            run_id,
            turn_id: TurnId::new(),
            config,
            status: RefCell::new(RunStatusDto::Starting),
            messages: RefCell::new(Vec::new()),
            finishes: RefCell::new(Vec::new()),
            transitions: RefCell::new(Vec::new()),
            config_error: RefCell::new(None),
            append_failure: RefCell::new(None),
            append_failure_at: RefCell::new(None),
            append_count: RefCell::new(0),
            transition_failure: RefCell::new(None),
        }
    }

    fn projection(&self) -> RunProjectionDto {
        RunProjectionDto::new(
            self.session_id,
            self.run_id,
            self.turn_id,
            *self.status.borrow(),
            self.config.revision_id(),
        )
    }

    fn next_append_index(&self) -> usize {
        let mut count = self.append_count.borrow_mut();
        *count += 1;
        *count
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<SessionProjectionDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn accept_user_turn(
        &self,
        _input: intention_storage::AcceptUserTurnInputDto,
    ) -> DtoResult<intention_storage::AcceptedTurnOutcomeDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn remove_turn(
        &self,
        _input: RemoveTurnInputDto,
    ) -> DtoResult<intention_proto::PendingTurnProjectionDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn consume_pending_user_turns(
        &self,
        input: ConsumePendingUserTurnsInputDto,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        assert_eq!(input.session_id(), self.session_id);
        assert_eq!(input.run_id(), self.run_id);
        Ok(Vec::new())
    }

    fn transition_run(&self, input: TransitionRunInputDto) -> DtoResult<RunProjectionDto> {
        assert_eq!(input.session_id(), self.session_id);
        assert_eq!(input.run_id(), self.run_id);
        if self
            .transition_failure
            .borrow()
            .as_ref()
            .is_some_and(|(status, _)| *status == input.status())
        {
            return Err(self
                .transition_failure
                .borrow_mut()
                .take()
                .expect("configured transition failure exists")
                .1);
        }
        *self.status.borrow_mut() = input.status();
        self.transitions.borrow_mut().push(input);
        Ok(self.projection())
    }

    fn finish_run(&self, input: FinishRunInputDto) -> DtoResult<RunProjectionDto> {
        assert_eq!(input.session_id(), self.session_id);
        assert_eq!(input.run_id(), self.run_id);
        *self.status.borrow_mut() = input.status();
        self.finishes.borrow_mut().push(input);
        Ok(self.projection())
    }

    fn append_message(&self, input: AppendMessageInputDto) -> DtoResult<MessageProjectionDto> {
        let index = self.next_append_index();
        if let Some(error) = self.append_failure.borrow_mut().take() {
            return Err(error);
        }
        if self
            .append_failure_at
            .borrow()
            .as_ref()
            .is_some_and(|(at, _)| *at == index)
        {
            return Err(self
                .append_failure_at
                .borrow_mut()
                .take()
                .expect("configured append failure exists")
                .1);
        }
        assert_eq!(input.message().session_id(), self.session_id);
        assert_eq!(input.message().run_id(), Some(self.run_id));
        let message = input.message().clone();
        self.messages.borrow_mut().push(message.clone());
        Ok(message)
    }

    fn write_tool_result(
        &self,
        _input: intention_storage::WriteToolResultInputDto,
    ) -> DtoResult<intention_storage::ToolResultEvidenceDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn load_tool_result(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _call_id: intention_proto::ToolCallId,
    ) -> DtoResult<intention_storage::ToolResultEvidenceDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn load_run_config_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        assert_eq!((session_id, run_id), (self.session_id, self.run_id));
        if let Some(error) = self.config_error.borrow_mut().take() {
            return Err(error);
        }
        Ok(self.config.clone())
    }

    fn load_starting_run_model_context(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<intention_storage::StartingRunModelContextDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn load_run_projection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunProjectionDto> {
        assert_eq!((session_id, run_id), (self.session_id, self.run_id));
        Ok(self.projection())
    }

    fn load_session_projection(&self, session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        assert_eq!(session_id, self.session_id);
        SessionProjectionDto::new(
            ProjectId::new(),
            self.session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
                .expect("workspace is valid"),
            RunModeDto::Build,
            Some(self.config.revision_id()),
            Some(self.projection()),
            Vec::new(),
        )
    }

    fn load_recent_messages(
        &self,
        _session_id: SessionId,
        _limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn load_run_messages(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn recover_unfinished_runs(
        &self,
        _input: intention_storage::RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<RunProjectionDto>> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }
}

struct ScriptedDriver {
    preflight_error: Option<ErrorDto>,
    events: RefCell<Vec<Vec<Result<ModelEventDto, ProviderErrorDto>>>>,
    executions: RefCell<usize>,
    requests: RefCell<Vec<ModelRequestDto>>,
    /// Cancels the shared signal after the given event index of the first round.
    cancel_during_stream: Option<(usize, ModelCancellationSignal)>,
    pending_stream: bool,
}

impl ScriptedDriver {
    fn new(events: Vec<Result<ModelEventDto, ProviderErrorDto>>) -> Self {
        Self {
            preflight_error: None,
            events: RefCell::new(vec![events]),
            executions: RefCell::new(0),
            requests: RefCell::new(Vec::new()),
            cancel_during_stream: None,
            pending_stream: false,
        }
    }

    fn with_rounds(rounds: Vec<Vec<Result<ModelEventDto, ProviderErrorDto>>>) -> Self {
        Self {
            events: RefCell::new(rounds),
            ..Self::new(Vec::new())
        }
    }
}

impl ModelDriver for ScriptedDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }

    fn preflight(&self, _request: &ModelRequestDto) -> DtoResult<()> {
        self.preflight_error.clone().map_or(Ok(()), Err)
    }
}

impl ModelExecutionDriver for ScriptedDriver {
    fn execute(
        &self,
        request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        let execution = {
            let mut count = self.executions.borrow_mut();
            *count += 1;
            *count
        };
        self.requests.borrow_mut().push(request);
        if self.pending_stream {
            return Box::pin(stream::pending());
        }
        let events = self.events.borrow_mut().remove(0);
        if execution == 1
            && let Some((index, signal)) = &self.cancel_during_stream
        {
            let signal = signal.clone();
            let index = *index;
            let mut seen = 0usize;
            return Box::pin(stream::iter(events).inspect(move |_| {
                if seen == index {
                    signal.cancel();
                }
                seen += 1;
            }));
        }
        Box::pin(stream::iter(events))
    }
}

/// A tool executor that the M4 execution fixtures never invoke.
///
/// The tool executor is mandatory on every executor; these fixtures exercise
/// provider rounds that never emit a tool call.
struct NeverInvokedToolExecutor;

impl ToolExecutionPort for NeverInvokedToolExecutor {
    fn execute_tool(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _call: ToolCallDto,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = DtoResult<ToolResultOutcomeDto>> + Send + '_>,
    > {
        Box::pin(async {
            Err(ErrorDto::validation(
                "fixture_tool_unexpected",
                "m4 execution fixtures never invoke the tool executor",
            ))
        })
    }
}

fn execute(
    repository: &FakeRepository,
    driver: &ScriptedDriver,
    request: ModelRequestDto,
    config: ConfigSnapshotDto,
    signal: ModelCancellationSignal,
) -> DtoResult<ModelRunExecutionOutcomeDto> {
    let clock = ImmediateTime::new();
    let tool_executor = NeverInvokedToolExecutor;
    futures_executor::block_on(
        ModelRunExecutionService::new(repository, driver, &clock, &tool_executor).execute(
            ModelRunExecutionInputDto::new(
                repository.session_id,
                repository.run_id,
                request,
                config,
                signal,
            ),
        ),
    )
}

struct RecordingCommitObserver {
    commits: std::sync::Mutex<Vec<ModelRunCommitDto>>,
}

impl RecordingCommitObserver {
    const fn new() -> Self {
        Self {
            commits: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn commits(&self) -> Vec<ModelRunCommitDto> {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .clone()
    }
}

impl ModelRunCommitObserver for RecordingCommitObserver {
    fn observe_model_run_commit(&self, committed: &ModelRunCommitDto) {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .push(committed.clone());
    }
}

#[test]
fn observer_receives_only_committed_transcript_rows_and_statuses() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta("complete").expect("text is valid")),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let clock = ImmediateTime::new();
    let observer = RecordingCommitObserver::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::with_commit_observer(
            &repository,
            &driver,
            &clock,
            &observer,
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            ModelCancellationSignal::new(),
        )),
    )
    .expect("execution completes");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            run: RunProjectionDto::new(
                session_id,
                run_id,
                repository.projection().turn_id(),
                RunStatusDto::Completed,
                repository.config.revision_id(),
            )
        }
    );
    let commits = observer.commits();
    assert_eq!(
        commits.len(),
        3,
        "running status, step row, completed status"
    );
    assert_eq!(
        commits[0],
        ModelRunCommitDto::Status {
            session_id,
            run_id,
            status: RunStatusDto::Running,
        }
    );
    assert_eq!(
        commits[1],
        ModelRunCommitDto::Content(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::Assistant,
                "complete",
                None,
                None,
                None,
            )
            .expect("fixture message is valid")
        )
    );
    assert_eq!(
        commits[2],
        ModelRunCommitDto::Status {
            session_id,
            run_id,
            status: RunStatusDto::Completed,
        }
    );
    assert_eq!(repository.messages.borrow().len(), 1);
}

#[test]
fn observer_receives_no_content_when_a_message_commit_fails() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    *repository.append_failure.borrow_mut() = Some(ErrorDto::unavailable(
        "append_failed",
        "append fails before commit",
    ));
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta("lost").expect("text is valid")),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let clock = ImmediateTime::new();
    let observer = RecordingCommitObserver::new();

    let error = futures_executor::block_on(
        ModelRunExecutionService::with_commit_observer(
            &repository,
            &driver,
            &clock,
            &observer,
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            ModelCancellationSignal::new(),
        )),
    )
    .expect_err("failed append must abort execution");

    assert_eq!(error.code(), "append_failed");
    assert!(
        observer
            .commits()
            .iter()
            .all(|commit| !matches!(commit, ModelRunCommitDto::Content(_))),
        "an uncommitted row is never published"
    );
    assert!(repository.messages.borrow().is_empty());
    assert!(repository.finishes.borrow().is_empty());
}

#[test]
fn preflight_failure_does_not_execute_and_fails_starting_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let mut driver = ScriptedDriver::new(Vec::new());
    driver.preflight_error = Some(ErrorDto::validation(
        "unsupported_model_capability",
        "unsupported",
    ));
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("failure is committed");
    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("preflight failure terminalizes the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "unsupported_model_capability");
    assert_eq!(*driver.executions.borrow(), 0);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Failed);
    assert_eq!(
        finishes[0].error_code(),
        Some("unsupported_model_capability")
    );
    assert!(repository.messages.borrow().is_empty());
}

#[test]
fn streams_commit_one_assistant_step_with_reasoning_and_complete() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let content = format!("{}€tail", "a".repeat(4094));
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta(content.clone()).expect("text is valid")),
        Ok(ModelEventDto::reasoning_delta("why").expect("reasoning is valid")),
        Ok(ModelEventDto::usage(
            UsageDto::reported(1, 2, 3).expect("usage is valid"),
        )),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("run completes");
    let ModelRunExecutionOutcomeDto::Completed { run } = outcome else {
        unreachable!("a stop reason completes the run");
    };
    assert_eq!(run.status(), RunStatusDto::Completed);
    let messages = repository.messages.borrow();
    assert_eq!(
        messages.as_slice(),
        &[MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            content,
            Some("why".to_owned()),
            None,
            None,
        )
        .expect("fixture message is valid")],
        "the whole step commits as one row with its reasoning"
    );
    drop(messages);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert_eq!(finishes[0].finish_reason(), Some(FinishReasonDto::Stop));
    assert_eq!(
        finishes[0].usage(),
        Some(&UsageDto::reported(1, 2, 3).expect("usage is valid"))
    );
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
}

#[test]
fn malformed_provider_and_eof_streams_safely_fail_without_committing_content() {
    for (events, expected_code) in [
        (
            vec![Ok(
                ModelEventDto::text_delta("invalid").expect("text is valid")
            )],
            "invalid_model_stream_order",
        ),
        (vec![Ok(ModelEventDto::started())], "provider_stream_ended"),
        (
            vec![
                Ok(ModelEventDto::started()),
                Err(ProviderErrorDto::unavailable("provider_down", false, None)
                    .expect("error is valid")),
            ],
            "provider_down",
        ),
    ] {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let config = snapshot("fixture");
        let repository = FakeRepository::new(session_id, run_id, config.clone());
        let driver = ScriptedDriver::new(events);
        let outcome = execute(
            &repository,
            &driver,
            request(run_id, "fixture"),
            config,
            ModelCancellationSignal::new(),
        )
        .expect("safe failure commits");
        let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
            unreachable!("a malformed stream fails the run");
        };
        assert_eq!(error.code(), expected_code);
        assert_eq!(run.status(), RunStatusDto::Failed);
        assert!(
            repository.messages.borrow().is_empty(),
            "an invalid stream never commits assistant content"
        );
        let finishes = repository.finishes.borrow();
        assert_eq!(finishes.len(), 1);
        assert_eq!(finishes[0].error_code(), Some(expected_code));
    }
}

#[test]
fn interruption_records_a_notice_and_continues_the_same_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    let driver = ScriptedDriver {
        preflight_error: None,
        events: RefCell::new(vec![
            vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("partial answer").expect("text is valid")),
            ],
            vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("final answer").expect("text is valid")),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ],
        ]),
        executions: RefCell::new(0),
        requests: RefCell::new(Vec::new()),
        // The interrupt is requested while the first provider stream is live,
        // after its partial text was delivered.
        cancel_during_stream: Some((1, signal.clone())),
        pending_stream: false,
    };
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        signal,
    )
    .expect("the interrupted run continues and completes");
    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(*driver.executions.borrow(), 2);
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![
            (MessageKindDto::Assistant, "partial answer"),
            (MessageKindDto::Notice, intention_runtime::INTERRUPT_NOTICE),
            (MessageKindDto::Assistant, "final answer"),
        ],
        "the stopped step commits its text and the durable notice"
    );
    assert_eq!(messages[1].run_id(), Some(run_id));
    drop(messages);
    // The continuation request carries the notice, so the model knows its
    // previous call was stopped before a final result.
    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::Notice
            && message.content() == intention_runtime::INTERRUPT_NOTICE
    }));
    drop(requests);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1, "one terminal outcome commits");
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
}

#[test]
fn an_interrupt_before_the_first_round_still_records_a_notice_and_continues() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    signal.cancel();
    let driver = ScriptedDriver::with_rounds(vec![
        Vec::new(),
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        signal,
    )
    .expect("a pre-requested interrupt is handled and the run continues");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        *driver.executions.borrow(),
        2,
        "the interrupted round is followed by the next provider step"
    );
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Notice, intention_runtime::INTERRUPT_NOTICE)],
        "an interrupted step without text commits only its notice"
    );
    drop(messages);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
}

#[test]
fn an_interrupt_whose_notice_cannot_commit_surfaces_the_storage_error() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    // The stopped step commits its text first; the interruption notice append
    // then fails as the second committed row.
    *repository.append_failure_at.borrow_mut() = Some((
        2,
        ErrorDto::unavailable(
            "fixture_notice_unavailable",
            "the interrupt notice could not be stored",
        ),
    ));
    let driver = ScriptedDriver {
        preflight_error: None,
        events: RefCell::new(vec![vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("late").expect("text is valid")),
        ]]),
        executions: RefCell::new(0),
        requests: RefCell::new(Vec::new()),
        cancel_during_stream: Some((1, signal.clone())),
        pending_stream: false,
    };

    let error = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        signal,
    )
    .expect_err("an uncommittable notice is a typed error, never a silent stop");
    assert_eq!(error.code(), "fixture_notice_unavailable");
    assert_eq!(*driver.executions.borrow(), 1);
    assert_eq!(repository.messages.borrow().len(), 1);
    assert!(repository.finishes.borrow().is_empty());
}

#[test]
fn retry_is_ordered_once_and_waits_exactly_250_milliseconds() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![Err(ProviderErrorDto::unavailable(
            "provider_down",
            true,
            None,
        )
        .expect("error is valid"))],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let clock = ImmediateTime::new();
    let outcome = futures_executor::block_on(
        ModelRunExecutionService::new(&repository, &driver, &clock, &NeverInvokedToolExecutor)
            .execute(ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                ModelCancellationSignal::new(),
            )),
    )
    .expect("retry completes");
    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        clock
            .sleeps
            .borrow()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        1
    );
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert!(
        repository.messages.borrow().is_empty(),
        "a failed attempt commits no assistant row"
    );
}

#[test]
fn configuration_mismatch_fails_without_provider_calls() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let persisted = snapshot("persisted");
    let repository = FakeRepository::new(session_id, run_id, persisted);
    let driver = ScriptedDriver::new(Vec::new());
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "current"),
        snapshot("current"),
        ModelCancellationSignal::new(),
    )
    .expect("mismatch safely fails");
    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("a configuration mismatch fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(*driver.executions.borrow(), 0);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(
        finishes[0].error_code(),
        Some("provider_configuration_unavailable")
    );
}

#[test]
fn unavailable_persisted_configuration_fails_without_provider_calls() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    *repository.config_error.borrow_mut() = Some(ErrorDto::unavailable(
        "configuration_not_found",
        "the persisted configuration is unavailable",
    ));
    let driver = ScriptedDriver::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("configuration absence becomes durable safe failure");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("configuration absence fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(*driver.executions.borrow(), 0);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(
        finishes[0].error_code(),
        Some("provider_configuration_unavailable")
    );
}

#[test]
fn starting_run_with_wrong_request_identity_fails_without_provider_calls() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        request(RunId::new(), "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("wrong request identity becomes durable safe failure");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("a wrong request identity fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(*driver.executions.borrow(), 0);
    assert_eq!(repository.finishes.borrow().len(), 1);
}

#[test]
fn execution_rejects_non_starting_run_before_configuration_or_provider_work() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    *repository.status.borrow_mut() = RunStatusDto::Running;
    let driver = ScriptedDriver::new(Vec::new());

    let error = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect_err("a non-starting run cannot execute");

    assert_eq!(error.code(), "invalid_model_run_execution_state");
    assert_eq!(*driver.executions.borrow(), 0);
    assert!(repository.messages.borrow().is_empty());
    assert!(repository.finishes.borrow().is_empty());
    assert!(repository.transitions.borrow().is_empty());
}

#[test]
fn retryable_failure_before_a_committed_step_retries_within_the_attempt_budget() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::reasoning_delta("why").expect("reasoning is valid")),
            Err(ProviderErrorDto::unavailable("provider_down", true, None).expect("error is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("an uncommitted attempt stays retryable");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(*driver.executions.borrow(), 2);
    assert!(
        repository.messages.borrow().is_empty(),
        "an uncommitted reasoning echo never becomes a transcript row"
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
}

#[test]
fn retryable_failure_after_a_committed_step_does_not_retry() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    let driver = ScriptedDriver {
        preflight_error: None,
        events: RefCell::new(vec![
            vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("partial answer").expect("text is valid")),
            ],
            vec![Err(ProviderErrorDto::unavailable(
                "provider_down",
                true,
                None,
            )
            .expect("error is valid"))],
        ]),
        executions: RefCell::new(0),
        requests: RefCell::new(Vec::new()),
        cancel_during_stream: Some((1, signal.clone())),
        pending_stream: false,
    };
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::new(&repository, &driver, &clock, &NeverInvokedToolExecutor)
            .execute(ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                signal,
            )),
    )
    .expect("a committed step suppresses the retry");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("the retryable failure terminalizes the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_down");
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        clock
            .sleeps
            .borrow()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        0,
        "no retry delay runs after committed content"
    );
    assert_eq!(repository.messages.borrow().len(), 2);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Failed);
    assert_eq!(finishes[0].error_code(), Some("provider_down"));
}

#[test]
fn exhausted_retryable_failure_stops_after_second_attempt() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![Err(ProviderErrorDto::unavailable(
            "provider_down",
            true,
            None,
        )
        .expect("error is valid"))],
        vec![Err(ProviderErrorDto::unavailable(
            "provider_down",
            true,
            None,
        )
        .expect("error is valid"))],
    ]);
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::new(&repository, &driver, &clock, &NeverInvokedToolExecutor)
            .execute(ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                ModelCancellationSignal::new(),
            )),
    )
    .expect("second retryable failure becomes terminal");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("an exhausted attempt budget fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_down");
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        clock
            .sleeps
            .borrow()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        1,
        "exactly one retry delay runs"
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("provider_down"));
}

#[test]
fn provider_timeout_retries_then_records_a_terminal_timeout_failure() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver {
        preflight_error: None,
        events: RefCell::new(Vec::new()),
        executions: RefCell::new(0),
        requests: RefCell::new(Vec::new()),
        cancel_during_stream: None,
        pending_stream: true,
    };
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::new(&repository, &driver, &clock, &NeverInvokedToolExecutor)
            .execute(ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                ModelCancellationSignal::new(),
            )),
    )
    .expect("exhausted timeouts become a durable failure");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("exhausted timeouts fail the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_attempt_timed_out");
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        clock.sleeps.borrow().as_slice(),
        &[
            Duration::from_secs(30),
            Duration::from_millis(250),
            Duration::from_secs(30)
        ]
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("provider_attempt_timed_out"));
    assert!(
        repository.messages.borrow().is_empty(),
        "a timed-out attempt commits no transcript row"
    );
}

#[test]
fn an_interrupt_during_a_retry_wait_records_a_notice_and_starts_the_next_attempt() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    let driver = ScriptedDriver {
        preflight_error: None,
        events: RefCell::new(vec![
            vec![Err(ProviderErrorDto::unavailable(
                "provider_down",
                true,
                None,
            )
            .expect("error is valid"))],
            vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ],
        ]),
        executions: RefCell::new(0),
        requests: RefCell::new(Vec::new()),
        // The interrupt lands while the first attempt's retry delay is waiting.
        cancel_during_stream: Some((0, signal.clone())),
        pending_stream: false,
    };

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        signal,
    )
    .expect("the interruption during the retry wait continues the run");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(*driver.executions.borrow(), 2);
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Notice, intention_runtime::INTERRUPT_NOTICE)]
    );
    drop(messages);
    let requests = driver.requests.borrow();
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::Notice
            && message.content() == intention_runtime::INTERRUPT_NOTICE
    }));
    drop(requests);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
}
