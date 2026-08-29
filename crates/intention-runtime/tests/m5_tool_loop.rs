#![allow(
    clippy::expect_used,
    reason = "Execution fixtures use expect to provide precise failures."
)]

use std::{cell::RefCell, collections::VecDeque, future, sync::mpsc, time::Duration};

use futures_util::{StreamExt, stream};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    ModelRunFactDto, ModelRunFactInputDto, ModelRunProjectionDto, RunEventCursorDto,
    RunEventTailPageDto, RunFailureDto, RunModeDto, RunProjectionDto, RunReplayDto, RunSnapshotDto,
    RunStatusDto, SessionProjectionDto, ToolResultOutcomeDto, WorkspaceRootDto,
};
use intention_model::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto, ToolCallDto,
};
use intention_runtime::{
    ModelRunCommitDto, ModelRunCommitObserver, ModelRunExecutionInputDto,
    ModelRunExecutionOutcomeDto, ModelRunExecutionService, ModelSleepFuture, ModelTimePort,
    ToolExecutionPort,
};
use intention_storage::{
    AppendModelRunFactsInputDto, AppendModelRunFactsOutcomeDto, CommittedChangeDto,
    CreateSessionInputDto, RecoverUnfinishedRunsInputDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, ErrorRetryDto, ProjectId, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, ToolCallId, TurnId, WorkspaceId,
};

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
}

fn snapshot(model: &str) -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-runtime-tool-loop.toml")
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

struct FakeRepository {
    session_id: SessionId,
    run_id: RunId,
    config: ConfigSnapshotDto,
    status: RefCell<RunStatusDto>,
    cursor: RefCell<RunEventCursorDto>,
    appends: RefCell<Vec<AppendModelRunFactsInputDto>>,
    transitions: RefCell<Vec<TransitionRunInputDto>>,
    config_error: RefCell<Option<ErrorDto>>,
    append_failure: RefCell<Option<ErrorDto>>,
    fail_append_at: RefCell<Option<usize>>,
    append_count: RefCell<usize>,
    cancel_before_first_append: RefCell<bool>,
    cancel_after_append: RefCell<Option<(usize, ModelCancellationSignal)>>,
    transition_failure: RefCell<Option<(RunStatusDto, ErrorDto)>>,
}

impl FakeRepository {
    const fn new(session_id: SessionId, run_id: RunId, config: ConfigSnapshotDto) -> Self {
        Self {
            session_id,
            run_id,
            config,
            status: RefCell::new(RunStatusDto::Starting),
            cursor: RefCell::new(RunEventCursorDto::new(0)),
            appends: RefCell::new(Vec::new()),
            transitions: RefCell::new(Vec::new()),
            config_error: RefCell::new(None),
            append_failure: RefCell::new(None),
            fail_append_at: RefCell::new(None),
            append_count: RefCell::new(0),
            cancel_before_first_append: RefCell::new(false),
            cancel_after_append: RefCell::new(None),
            transition_failure: RefCell::new(None),
        }
    }

    fn run_snapshot(&self, status: RunStatusDto, cursor: RunEventCursorDto) -> RunSnapshotDto {
        let projection = ModelRunProjectionDto::new(
            RunProjectionDto::new(
                self.session_id,
                self.run_id,
                TurnId::new(),
                status,
                self.config.revision_id(),
            ),
            cursor,
            None,
            "",
            None,
            None,
            None,
        )
        .expect("fixture projection is valid");
        RunSnapshotDto::new(
            self.session_id,
            self.run_id,
            SessionEventSequenceDto::new(cursor.value()),
            projection,
        )
        .expect("fixture snapshot is valid")
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn accept_user_turn(
        &self,
        _input: intention_storage::AcceptUserTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn remove_queued_turn(
        &self,
        _input: intention_storage::RemoveQueuedTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn transition_run(&self, input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
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
        let projection = SessionProjectionDto::new(
            ProjectId::new(),
            self.session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
                .expect("workspace is valid"),
            RunModeDto::Build,
            Some(self.config.revision_id()),
            None,
            Vec::new(),
            SessionEventSequenceDto::new(self.cursor.borrow().value()),
        )
        .expect("fixture projection is valid");
        CommittedChangeDto::new(
            projection,
            SessionEventSequenceDto::new(self.cursor.borrow().value()),
            Vec::new(),
            None,
        )
    }

    fn append_model_run_facts(
        &self,
        input: AppendModelRunFactsInputDto,
    ) -> DtoResult<AppendModelRunFactsOutcomeDto> {
        if self.cancel_before_first_append.replace(false) {
            // This is the deterministic scheduling barrier: execution has read
            // Starting, then the host's StopRun commit wins immediately before
            // the first transition-to-Running append.
            *self.status.borrow_mut() = RunStatusDto::Cancelling;
            self.transitions
                .borrow_mut()
                .push(TransitionRunInputDto::new(
                    self.session_id,
                    self.run_id,
                    RunStatusDto::Cancelling,
                    time(2),
                ));
            return Err(ErrorDto::unavailable(
                "run_status_changed",
                "the run changed while execution was being admitted",
            ));
        }
        if let Some(error) = self.append_failure.borrow_mut().take() {
            return Err(error);
        }
        let append_index = {
            *self.append_count.borrow_mut() += 1;
            *self.append_count.borrow()
        };
        if self.fail_append_at.borrow().as_ref() == Some(&append_index) {
            return Err(ErrorDto::unavailable(
                "fixture_append_failure",
                "the scripted append failed",
            ));
        }
        if let Some((index, signal)) = self.cancel_after_append.borrow().as_ref()
            && index == &append_index
        {
            signal.cancel();
        }
        assert_eq!(input.session_id(), self.session_id);
        assert_eq!(input.run_id(), self.run_id);
        assert_eq!(input.expected_cursor(), *self.cursor.borrow());
        let mut next = input.expected_cursor().value();
        let facts = input
            .facts()
            .iter()
            .cloned()
            .map(|fact| {
                next += 1;
                ModelRunFactDto::new(RunEventCursorDto::new(next), fact)
                    .expect("fixture fact is valid")
            })
            .collect::<Vec<_>>();
        let cursor = RunEventCursorDto::new(next);
        let status = input.status().unwrap_or_else(|| *self.status.borrow());
        *self.status.borrow_mut() = status;
        *self.cursor.borrow_mut() = cursor;
        self.appends.borrow_mut().push(input);
        AppendModelRunFactsOutcomeDto::new(cursor, self.run_snapshot(status, cursor), facts)
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

    fn load_current_run_replay(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        assert_eq!((session_id, run_id), (self.session_id, self.run_id));
        let cursor = *self.cursor.borrow();
        let snapshot = self.run_snapshot(*self.status.borrow(), cursor);
        RunReplayDto::new(
            snapshot,
            RunEventTailPageDto::empty(session_id, run_id, cursor),
        )
    }

    fn recover_unfinished_runs(
        &self,
        _input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<CommittedChangeDto>> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn load_session_snapshot(&self, _session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        SessionProjectionDto::new(
            ProjectId::new(),
            self.session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
                .expect("workspace is valid"),
            RunModeDto::Build,
            Some(self.config.revision_id()),
            None,
            Vec::new(),
            SessionEventSequenceDto::new(0),
        )
    }

    fn load_tail(
        &self,
        _session_id: SessionId,
        _after_sequence: SessionEventSequenceDto,
    ) -> DtoResult<Vec<intention_types::EventEnvelopeDto<intention_domain::DomainEventDto>>> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(ErrorDto::unavailable("fixture_unused", "unused"))
    }
}

struct ScriptedDriver {
    events: RefCell<Vec<Vec<Result<ModelEventDto, ProviderErrorDto>>>>,
    executions: RefCell<usize>,
    requests: RefCell<Vec<ModelRequestDto>>,
}

impl ScriptedDriver {
    fn new(events: Vec<Result<ModelEventDto, ProviderErrorDto>>) -> Self {
        Self::with_rounds(vec![events])
    }

    const fn with_rounds(rounds: Vec<Vec<Result<ModelEventDto, ProviderErrorDto>>>) -> Self {
        Self {
            events: RefCell::new(rounds),
            executions: RefCell::new(0),
            requests: RefCell::new(Vec::new()),
        }
    }
}

impl ModelDriver for ScriptedDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }
}

impl ModelExecutionDriver for ScriptedDriver {
    fn execute(
        &self,
        request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        *self.executions.borrow_mut() += 1;
        self.requests.borrow_mut().push(request);
        let events = self.events.borrow_mut().remove(0);
        Box::pin(stream::iter(events))
    }
}

/// Emits one started event, then cancels the run signal from inside the stream.
///
/// The round's next iteration observes the already-cancelled signal at its
/// pre-stream check and commits the cancellation deterministically, without
/// racing the provider timeout.
struct SelfCancellingDriver {
    signal: ModelCancellationSignal,
    executions: std::sync::Mutex<usize>,
}

impl ModelDriver for SelfCancellingDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }
}

impl ModelExecutionDriver for SelfCancellingDriver {
    fn execute(
        &self,
        _request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        *self
            .executions
            .lock()
            .expect("driver recorder is available") += 1;
        let signal = self.signal.clone();
        Box::pin(
            stream::once(async move {
                signal.cancel();
                Ok(ModelEventDto::started())
            })
            .chain(stream::pending::<Result<ModelEventDto, ProviderErrorDto>>()),
        )
    }
}

/// Emits one started event, signals the test, then blocks forever.
///
/// Used to hold the provider round mid-stream so the test can cancel the run
/// while the round's select is waiting on the provider and its timeout.
struct PendingAfterStartedDriver {
    entered_tx: mpsc::Sender<()>,
    executions: std::sync::Mutex<usize>,
}

impl ModelDriver for PendingAfterStartedDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }
}

impl ModelExecutionDriver for PendingAfterStartedDriver {
    fn execute(
        &self,
        _request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        *self
            .executions
            .lock()
            .expect("driver recorder is available") += 1;
        let entered_tx = self.entered_tx.clone();
        Box::pin(
            stream::once(async move {
                entered_tx
                    .send(())
                    .expect("the test observes the started event");
                Ok(ModelEventDto::started())
            })
            .chain(stream::pending::<Result<ModelEventDto, ProviderErrorDto>>()),
        )
    }
}

/// A time port whose sleeps stay pending until the test sets the release flag.
///
/// Poll-based, so it never blocks the executor thread; the round timeout only
/// becomes ready when the test decides, which keeps the round's cancellation
/// race deterministic.
struct FlagTime {
    release: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl FlagTime {
    const fn new(release: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self { release }
    }
}

impl ModelTimePort for FlagTime {
    fn now(&self) -> TimestampDto {
        time(2)
    }

    fn sleep(&self, _duration: Duration) -> ModelSleepFuture<'_> {
        let release = self.release.clone();
        Box::pin(futures_util::future::poll_fn(move |context| {
            if release.load(std::sync::atomic::Ordering::Relaxed) {
                std::task::Poll::Ready(())
            } else {
                context.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }))
    }
}

/// Records every committed model-run snapshot.
struct RecordingCommitObserver {
    commits: std::sync::Mutex<Vec<ModelRunCommitDto>>,
}

impl RecordingCommitObserver {
    const fn new() -> Self {
        Self {
            commits: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl ModelRunCommitObserver for RecordingCommitObserver {
    fn observe_model_run_commit(&self, committed: ModelRunCommitDto) {
        self.commits
            .lock()
            .expect("observer recorder is available")
            .push(committed);
    }
}

/// Executes scripted tool outcomes and records every port invocation.
struct ScriptedPort {
    calls: std::sync::Mutex<Vec<(SessionId, RunId, ToolCallDto)>>,
    outcomes: std::sync::Mutex<VecDeque<DtoResult<ToolResultOutcomeDto>>>,
}

impl ScriptedPort {
    fn new(outcomes: Vec<DtoResult<ToolResultOutcomeDto>>) -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            outcomes: std::sync::Mutex::new(outcomes.into()),
        }
    }
}

impl ToolExecutionPort for ScriptedPort {
    fn execute_tool(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call: ToolCallDto,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = DtoResult<ToolResultOutcomeDto>> + Send + '_>,
    > {
        self.calls
            .lock()
            .expect("port call recorder is available")
            .push((session_id, run_id, call));
        let outcome = self
            .outcomes
            .lock()
            .expect("scripted outcomes are available")
            .pop_front()
            .expect("scripted tool outcome exists");
        Box::pin(future::ready(outcome))
    }
}

/// Blocks each port invocation until the test observes it and releases it.
struct GatedPort {
    calls: std::sync::Mutex<Vec<(SessionId, RunId, ToolCallDto)>>,
    called: mpsc::Sender<()>,
    release: std::sync::Arc<std::sync::Mutex<Option<mpsc::Receiver<()>>>>,
}

impl GatedPort {
    fn new(called: mpsc::Sender<()>, release: mpsc::Receiver<()>) -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            called,
            release: std::sync::Arc::new(std::sync::Mutex::new(Some(release))),
        }
    }
}

impl ToolExecutionPort for GatedPort {
    fn execute_tool(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call: ToolCallDto,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = DtoResult<ToolResultOutcomeDto>> + Send + '_>,
    > {
        self.calls
            .lock()
            .expect("port call recorder is available")
            .push((session_id, run_id, call));
        let called = self.called.clone();
        let release = self
            .release
            .lock()
            .expect("release receiver is available")
            .take()
            .expect("one gated tool call is scripted");
        Box::pin(async move {
            called.send(()).expect("test observes the gated call");
            release.recv().expect("test releases the gated call");
            Ok(ToolResultOutcomeDto::succeeded("tool output").expect("tool output is valid"))
        })
    }
}

fn execute(
    repository: &FakeRepository,
    driver: &ScriptedDriver,
    port: &ScriptedPort,
    request: ModelRequestDto,
    config: ConfigSnapshotDto,
    signal: ModelCancellationSignal,
) -> DtoResult<ModelRunExecutionOutcomeDto> {
    let clock = ImmediateTime::new();
    futures_executor::block_on(
        ModelRunExecutionService::with_tool_executor(repository, driver, &clock, port).execute(
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

#[test]
fn tool_call_executes_tool_records_result_and_completes() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("before ").expect("text is valid")),
            Ok(ModelEventDto::tool_call(call.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("after").expect("text is valid")),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::succeeded("hello world").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("tool loop completes");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(6)
        }
    );
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .as_slice(),
        &[(session_id, run_id, call.clone())]
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[0].facts(),
        [ModelRunFactInputDto::ProviderAttemptStarted { attempt: 1 }]
    ));
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::AssistantContentAppended { content, .. }]
            if content == "before "
    ));
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ToolCallRecorded { call: recorded }]
            if *recorded == call
    ));
    assert!(matches!(
        appends[3].facts(),
        [ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Succeeded { content },
        }] if *call_id == call.call_id() && content == "hello world"
    ));
    assert!(matches!(
        appends[4].facts(),
        [ModelRunFactInputDto::AssistantContentAppended { content, .. }]
            if content == "after"
    ));
    assert!(matches!(
        appends[5].facts(),
        [ModelRunFactInputDto::Finished { .. }]
    ));
    assert_eq!(appends[5].status(), Some(RunStatusDto::Completing));
    drop(appends);
    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].messages(),
        vec![
            ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                .expect("message is valid"),
            ModelMessageDto::tool_result(call.call_id(), "hello world").expect("message is valid"),
        ]
    );
}

#[test]
fn multiple_tool_calls_execute_sequentially_in_provider_order() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let first = ToolCallDto::new(ToolCallId::new(), "first", "{}").expect("call is valid");
    let second = ToolCallDto::new(ToolCallId::new(), "second", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(first.clone())),
            Ok(ModelEventDto::tool_call(second.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(vec![
        Ok(ToolResultOutcomeDto::succeeded("one").expect("content is valid")),
        Ok(ToolResultOutcomeDto::succeeded("two").expect("content is valid")),
    ]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("sequential tool loop completes");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(6)
        }
    );
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .as_slice(),
        &[
            (session_id, run_id, first.clone()),
            (session_id, run_id, second.clone()),
        ]
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Succeeded { content },
        }] if *call_id == first.call_id() && content == "one"
    ));
    assert!(matches!(
        appends[4].facts(),
        [ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Succeeded { content },
        }] if *call_id == second.call_id() && content == "two"
    ));
    drop(appends);
    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].messages(),
        vec![
            ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![first.clone(), second.clone()])
                .expect("message is valid"),
            ModelMessageDto::tool_result(first.call_id(), "one").expect("message is valid"),
            ModelMessageDto::tool_result(second.call_id(), "two").expect("message is valid"),
        ]
    );
}

#[test]
fn repeated_tool_rounds_continue_until_finished() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let first = ToolCallDto::new(ToolCallId::new(), "first", "{}").expect("call is valid");
    let second = ToolCallDto::new(ToolCallId::new(), "second", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(first.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(second.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("after").expect("text is valid")),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(vec![
        Ok(ToolResultOutcomeDto::succeeded("one").expect("content is valid")),
        Ok(ToolResultOutcomeDto::succeeded("two").expect("content is valid")),
    ]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("repeated tool rounds complete");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(7)
        }
    );
    assert_eq!(*driver.executions.borrow(), 3);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .as_slice(),
        &[
            (session_id, run_id, first.clone()),
            (session_id, run_id, second.clone()),
        ]
    );
    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[2].messages(),
        vec![
            ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![first.clone()])
                .expect("message is valid"),
            ModelMessageDto::tool_result(first.call_id(), "one").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![second.clone()])
                .expect("message is valid"),
            ModelMessageDto::tool_result(second.call_id(), "two").expect("message is valid"),
        ]
    );
}

#[test]
fn tool_failure_records_result_and_terminalizes_without_retry() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::tool_call(call.clone())),
    ]);
    let failure =
        RunFailureDto::new("tool_denied", ErrorRetryDto::Never, None).expect("failure is valid");
    let port = ScriptedPort::new(vec![Ok(ToolResultOutcomeDto::failed(failure))]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("tool denial terminalizes safely");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(4)
        }
    );
    assert_eq!(*driver.executions.borrow(), 1);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        1
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Failed { failure: recorded },
        }] if *call_id == call.call_id() && recorded.code() == "tool_denied"
    ));
    assert!(matches!(
        appends[3].facts(),
        [ModelRunFactInputDto::Failed { failure: terminal }]
            if terminal.code() == "tool_denied"
                && terminal.retry() == ErrorRetryDto::Never
    ));
    assert_eq!(appends[3].status(), Some(RunStatusDto::Failed));
}

#[test]
fn port_infrastructure_error_terminalizes_without_leaking_text() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::tool_call(call)),
    ]);
    let port = ScriptedPort::new(vec![Err(ErrorDto::unavailable(
        "tool_execution_failed",
        "sensitive provider detail",
    ))]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("port failure terminalizes safely");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(3)
        }
    );
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 3);
    assert!(matches!(
        appends[0].facts(),
        [ModelRunFactInputDto::ProviderAttemptStarted { attempt: 1 }]
    ));
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::ToolCallRecorded { .. }]
    ));
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::Failed { failure }]
            if failure.code() == "tool_execution_failed"
                && failure.retry() == ErrorRetryDto::Manual
    ));
}

#[test]
fn cancellation_during_tool_execution_suppresses_continuation() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::tool_call(call)),
    ]);
    let signal = ModelCancellationSignal::new();
    let (called_tx, called_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let port = GatedPort::new(called_tx, release_rx);
    let clock = ImmediateTime::new();
    let execution_signal = signal.clone();

    let execution = std::thread::spawn(move || {
        let outcome = futures_executor::block_on(
            ModelRunExecutionService::with_tool_executor(&repository, &driver, &clock, &port)
                .execute(ModelRunExecutionInputDto::new(
                    session_id,
                    run_id,
                    request(run_id, "fixture"),
                    config,
                    execution_signal,
                )),
        );
        (outcome, repository, driver, port)
    });
    called_rx.recv().expect("the gated port call is observed");
    signal.cancel();
    release_tx
        .send(())
        .expect("the gated port call is released");
    let (outcome, repository, driver, port) = execution.join().expect("execution thread completes");
    let outcome = outcome.expect("cancellation commits");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Cancelled {
            cursor: RunEventCursorDto::new(2)
        }
    );
    assert_eq!(*driver.executions.borrow(), 1);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        1
    );
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 2);
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::ToolCallRecorded { .. }]
    ));
    assert!(appends.iter().all(|input| {
        !input
            .facts()
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::ToolResultRecorded { .. }))
    }));
    drop(appends);
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Cancelling, RunStatusDto::Cancelled]
    );
}

#[test]
fn no_port_preserves_m4_denial() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::tool_call(call)),
    ]);
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::new(&repository, &driver, &clock).execute(
            ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                ModelCancellationSignal::new(),
            ),
        ),
    )
    .expect("tool denial commits");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(3)
        }
    );
    assert_eq!(*driver.executions.borrow(), 1);
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[1].facts(),
        [
            ModelRunFactInputDto::ToolCallRecorded { .. },
            ModelRunFactInputDto::Failed { failure },
        ] if failure.code() == "tool_execution_unavailable"
    ));
    assert_eq!(appends[1].status(), Some(RunStatusDto::Failed));
}

#[test]
fn provider_failure_after_tool_round_is_terminal_without_retry() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(call)),
        ],
        vec![Err(ProviderErrorDto::unavailable(
            "provider_broken",
            false,
            None,
        )
        .expect("fixture provider error is valid"))],
    ]);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::succeeded("hello world").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("provider failure after a tool round terminalizes");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(4)
        }
    );
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        1,
        "a provider failure after a tool result never re-executes the tool"
    );
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 4);
    assert!(matches!(
        appends[3].facts(),
        [ModelRunFactInputDto::Failed { failure }]
            if failure.code() == "provider_broken"
    ));
    assert_eq!(appends[3].status(), Some(RunStatusDto::Failed));
}

#[test]
fn cancellation_during_provider_round_cancels_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    let driver = SelfCancellingDriver {
        signal: signal.clone(),
        executions: std::sync::Mutex::new(0),
    };
    let port = ScriptedPort::new(Vec::new());
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::with_tool_executor(&repository, &driver, &clock, &port).execute(
            ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                signal,
            ),
        ),
    )
    .expect("round cancellation commits");

    assert!(
        matches!(outcome, ModelRunExecutionOutcomeDto::Cancelled { .. }),
        "the round cancellation commits as a cancelled run, got {outcome:?}"
    );
    let appends = repository.appends.borrow();
    assert!(appends.iter().all(|input| {
        !input
            .facts()
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::ToolCallRecorded { .. }))
    }));
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Cancelling, RunStatusDto::Cancelled]
    );
}

#[test]
fn cancellation_before_port_invocation_suppresses_tool() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::tool_call(call)),
    ]);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::succeeded("never used").expect("content is valid")
    )]);
    let signal = ModelCancellationSignal::new();
    repository
        .cancel_after_append
        .borrow_mut()
        .replace((2, signal.clone()));
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::with_tool_executor(&repository, &driver, &clock, &port).execute(
            ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                signal,
            ),
        ),
    )
    .expect("cancellation before the port commits");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Cancelled {
            cursor: RunEventCursorDto::new(2)
        }
    );
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        0,
        "a cancelled run never invokes the tool"
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::ToolCallRecorded { .. }]
    ));
    assert!(appends.iter().all(|input| {
        !input
            .facts()
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::ToolResultRecorded { .. }))
    }));
}

#[test]
fn tool_loop_with_commit_observer_executes_and_observes() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(call)),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::succeeded("hello world").expect("content is valid")
    )]);
    let observer = RecordingCommitObserver::new();
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::with_commit_observer_and_tool_executor(
            &repository,
            &driver,
            &clock,
            &observer,
            &port,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            ModelCancellationSignal::new(),
        )),
    )
    .expect("the tool loop with an observer completes");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(4)
        }
    );
    assert!(
        observer
            .commits
            .lock()
            .expect("observer recorder is available")
            .len()
            >= 5
    );
    assert!(matches!(
        observer
            .commits
            .lock()
            .expect("observer recorder is available")
            .last()
            .expect("a terminal commit is observed")
            .snapshot()
            .run_projection()
            .status(),
        RunStatusDto::Completed
    ));
}

#[test]
fn cancellation_while_round_is_waiting_cancels_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    let (entered_tx, entered_rx) = mpsc::channel();
    let driver = PendingAfterStartedDriver {
        entered_tx,
        executions: std::sync::Mutex::new(0),
    };
    let port = ScriptedPort::new(Vec::new());
    let release = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clock = FlagTime::new(std::sync::Arc::clone(&release));
    let execution_signal = signal.clone();

    let execution = std::thread::spawn(move || {
        let outcome = futures_executor::block_on(
            ModelRunExecutionService::with_tool_executor(&repository, &driver, &clock, &port)
                .execute(ModelRunExecutionInputDto::new(
                    session_id,
                    run_id,
                    request(run_id, "fixture"),
                    config,
                    execution_signal,
                )),
        );
        (outcome, repository)
    });
    entered_rx.recv().expect("the provider round started");
    signal.cancel();
    let (outcome, repository) = execution.join().expect("execution thread completes");
    let outcome = outcome.expect("round cancellation commits");

    assert!(
        matches!(outcome, ModelRunExecutionOutcomeDto::Cancelled { .. }),
        "the round cancellation commits as a cancelled run, got {outcome:?}"
    );
    let appends = repository.appends.borrow();
    assert!(appends.iter().all(|input| {
        !input
            .facts()
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::ToolCallRecorded { .. }))
    }));
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Cancelling, RunStatusDto::Cancelled]
    );
}
