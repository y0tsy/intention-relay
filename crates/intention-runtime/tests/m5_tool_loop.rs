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
    ModelRunFactDto, ModelRunFactInputDto, ModelRunProjectionDto, RunEventCursorDto, RunFailureDto,
    RunModeDto, RunProjectionDto, RunSnapshotDto, RunStatusDto, SessionProjectionDto,
    ToolResultOutcomeDto, WorkspaceRootDto,
};
use intention_model::{
    AssistantReasoningDto, FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto,
    ModelDriver, ModelEventDto, ModelEventStream, ModelExecutionDriver, ModelMessageDto,
    ModelRequestDto, ModelRoleDto, ModelToolDefinitionDto, ProviderErrorDto, ToolCallDto,
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
    snapshot_with_context_window(model, None)
}

/// Builds one frozen run configuration, optionally overriding the context
/// window policy so a test can drive the sliding window deterministically.
fn snapshot_with_context_window(
    model: &str,
    context_window: Option<(u64, u64)>,
) -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-runtime-tool-loop.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    );
    let context_window = context_window.map_or_else(String::new, |(window, capacity)| {
        format!("context_window_tokens = {window}\ncontext_capacity_tokens = {capacity}\n")
    });
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        format!("schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"{model}\"\ncredential = \"fixture-secret\"\n{context_window}"),
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

fn tool_definition() -> ModelToolDefinitionDto {
    ModelToolDefinitionDto::new(
        "read",
        "Read bounded text from a workspace file.",
        r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
    )
    .expect("fixture tool definition is valid")
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
    .with_tools(vec![tool_definition()])
    .expect("request tool definitions are valid")
}

/// Marks one expected message as the cache breakpoint the runtime recomputes
/// for the stable window prefix before every continuation request.
const fn cache_breakpoint(mut message: ModelMessageDto) -> ModelMessageDto {
    message.set_cache_control(true);
    message
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
    cancel_after_append: RefCell<Option<(usize, ModelCancellationSignal)>>,
    transition_failure: RefCell<Option<(RunStatusDto, ErrorDto)>>,
    /// Pending user messages consumed by the next context boundary.
    pending: RefCell<VecDeque<(TurnId, String)>>,
    /// Facts committed by every pending-message boundary consume.
    pending_facts: RefCell<Vec<ModelRunFactDto>>,
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
            cancel_after_append: RefCell::new(None),
            transition_failure: RefCell::new(None),
            pending: RefCell::new(VecDeque::new()),
            pending_facts: RefCell::new(Vec::new()),
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

    fn remove_turn(
        &self,
        _input: intention_storage::RemoveTurnInputDto,
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

    fn append_pending_user_turns(
        &self,
        input: intention_storage::AppendPendingUserTurnsInputDto,
    ) -> DtoResult<intention_storage::AppendPendingUserTurnsOutcomeDto> {
        assert_eq!(input.session_id(), self.session_id);
        assert_eq!(input.run_id(), self.run_id);
        assert_eq!(input.expected_cursor(), *self.cursor.borrow());
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        if pending.is_empty() {
            return intention_storage::AppendPendingUserTurnsOutcomeDto::new(
                *self.cursor.borrow(),
                Vec::new(),
            );
        }
        let mut next = self.cursor.borrow().value();
        let facts = pending
            .into_iter()
            .map(|(turn_id, content)| {
                next += 1;
                ModelRunFactDto::new(
                    RunEventCursorDto::new(next),
                    ModelRunFactInputDto::user_message_appended(turn_id, content)
                        .expect("fixture pending message is valid"),
                )
                .expect("fixture fact is valid")
            })
            .collect::<Vec<_>>();
        let cursor = RunEventCursorDto::new(next);
        *self.cursor.borrow_mut() = cursor;
        self.pending_facts
            .borrow_mut()
            .extend(facts.iter().cloned());
        intention_storage::AppendPendingUserTurnsOutcomeDto::new(cursor, facts)
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

    fn load_current_run_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunSnapshotDto> {
        assert_eq!((session_id, run_id), (self.session_id, self.run_id));
        let cursor = *self.cursor.borrow();
        let snapshot = self.run_snapshot(*self.status.borrow(), cursor);
        Ok(snapshot)
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
    /// Cancels the shared signal after the given event index of the first round.
    cancel_during_stream: RefCell<Option<(usize, ModelCancellationSignal)>>,
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
            cancel_during_stream: RefCell::new(None),
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
        let execution = {
            let mut executions = self.executions.borrow_mut();
            *executions += 1;
            *executions
        };
        self.requests.borrow_mut().push(request);
        let events = self.events.borrow_mut().remove(0);
        if execution == 1
            && let Some((index, signal)) = self.cancel_during_stream.borrow().as_ref()
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

/// Emits one started event, signals the test, then blocks; later rounds finish.
///
/// Used to hold the first provider round mid-stream so the test can interrupt
/// the run while the round's select is waiting on the provider.
struct PendingAfterStartedDriver {
    entered: std::sync::Mutex<Option<mpsc::Sender<()>>>,
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
        let execution = {
            let mut executions = self
                .executions
                .lock()
                .expect("driver recorder is available");
            *executions += 1;
            *executions
        };
        if execution == 1 {
            let entered = self
                .entered
                .lock()
                .expect("driver channel is available")
                .take();
            return Box::pin(
                stream::once(async move {
                    if let Some(entered) = entered {
                        entered
                            .send(())
                            .expect("the test observes the started event");
                    }
                    Ok(ModelEventDto::started())
                })
                .chain(stream::pending::<Result<ModelEventDto, ProviderErrorDto>>()),
            );
        }
        Box::pin(stream::iter(vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ]))
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

/// A time port whose retry-delay sleep signals that the delay was entered and
/// then stays pending, so the test can cancel the run inside the wait.
///
/// The attempt timeout is a seconds-scale duration while the runtime's retry
/// delay is sub-second, so only the retry delay is held pending.
struct RetryDelayTime {
    entered: mpsc::Sender<()>,
}

impl ModelTimePort for RetryDelayTime {
    fn now(&self) -> TimestampDto {
        time(2)
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        if duration < Duration::from_secs(1) {
            let entered = self.entered.clone();
            Box::pin(async move {
                entered.send(()).expect("the test observes the retry delay");
                future::pending::<()>().await;
            })
        } else {
            Box::pin(future::ready(()))
        }
    }
}

/// A time port that signals when the provider round's timeout sleep is first
/// polled, proving the round's select is suspended on the provider stream, and
/// then stays pending until the run is cancelled.
///
/// The attempt timeout is a seconds-scale duration while the runtime's retry
/// delay is sub-second, so only the round timeout is held pending.
struct RoundSelectTime {
    entered: std::sync::Mutex<Option<mpsc::Sender<()>>>,
}

impl ModelTimePort for RoundSelectTime {
    fn now(&self) -> TimestampDto {
        time(2)
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        if duration >= Duration::from_secs(1) {
            let entered = self
                .entered
                .lock()
                .expect("time channel is available")
                .take();
            Box::pin(async move {
                if let Some(entered) = entered {
                    entered
                        .send(())
                        .expect("the test observes the round select");
                }
                future::pending::<()>().await;
            })
        } else {
            Box::pin(future::ready(()))
        }
    }
}

/// A time port that answers immediately and records every requested delay.
struct RecordingTime {
    sleeps: std::sync::Mutex<Vec<Duration>>,
}

impl RecordingTime {
    const fn new() -> Self {
        Self {
            sleeps: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl ModelTimePort for RecordingTime {
    fn now(&self) -> TimestampDto {
        time(2)
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        self.sleeps
            .lock()
            .expect("sleep recorder is available")
            .push(duration);
        Box::pin(future::ready(()))
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

/// Gates only the first port invocation until the test releases it; later
/// invocations answer immediately from the scripted outcome queue.
///
/// Used to hold the first tool call mid-execution so the test can assert that
/// the loop never starts the second call before the first finishes.
struct GateFirstCallPort {
    calls: std::sync::Mutex<Vec<(SessionId, RunId, ToolCallDto)>>,
    called: mpsc::Sender<()>,
    release: std::sync::Arc<std::sync::Mutex<Option<mpsc::Receiver<()>>>>,
    outcomes: std::sync::Mutex<VecDeque<DtoResult<ToolResultOutcomeDto>>>,
}

impl GateFirstCallPort {
    fn new(
        called: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        outcomes: Vec<DtoResult<ToolResultOutcomeDto>>,
    ) -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            called,
            release: std::sync::Arc::new(std::sync::Mutex::new(Some(release))),
            outcomes: std::sync::Mutex::new(outcomes.into()),
        }
    }
}

impl ToolExecutionPort for GateFirstCallPort {
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
            .take();
        let outcome = self
            .outcomes
            .lock()
            .expect("scripted outcomes are available")
            .pop_front()
            .expect("scripted tool outcome exists");
        Box::pin(async move {
            if let Some(release) = release {
                called.send(()).expect("test observes the gated call");
                release.recv().expect("test releases the gated call");
            }
            outcome
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
        ModelRunExecutionService::new(repository, driver, &clock, port).execute(
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
        requests[1].tools(),
        requests[0].tools(),
        "the follow-up provider round preserves the advertised tool definitions"
    );
    assert_eq!(
        requests[1].messages(),
        vec![
            ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                .expect("message is valid"),
            cache_breakpoint(
                ModelMessageDto::tool_result(call.call_id(), "hello world")
                    .expect("message is valid"),
            ),
        ]
    );
}

#[test]
fn the_window_pass_compresses_a_large_tool_result_before_the_continuation_request() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot_with_context_window("fixture", Some((60, 1_000_000)));
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(call.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let large = "x".repeat(400);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::succeeded(large.clone()).expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("windowed tool loop completes");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(4)
        }
    );
    let requests = driver.requests.borrow();
    let messages = requests[1].messages();
    assert_eq!(messages.len(), 3, "no message is ever removed");
    assert_eq!(messages[1].role(), ModelRoleDto::Assistant);
    assert_eq!(
        messages[1]
            .tool_calls()
            .expect("the assistant tool call is preserved")
            .first()
            .expect("one call is preserved")
            .call_id(),
        call.call_id()
    );
    let content = messages[2].content();
    assert_eq!(messages[2].role(), ModelRoleDto::Tool);
    assert_eq!(messages[2].tool_call_id(), Some(call.call_id()));
    assert!(
        content.chars().count() < large.chars().count(),
        "the continuation request carries the compressed result"
    );
    assert!(content.contains("[compressed]"));
    assert!(
        messages[2].cache_control(),
        "the recomputed breakpoint closes the trimmed stable prefix"
    );
    drop(requests);
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ToolResultRecorded {
            outcome: ToolResultOutcomeDto::Succeeded { content },
            ..
        }] if content == &large
    ));
}

#[test]
fn partial_tool_result_continues_the_loop_without_terminalizing() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "execute", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(call.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("recovered").expect("text is valid")),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let partial_content = "captured output\n[The tool call did not receive a final result; the output above is partial.]";
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::partial(partial_content).expect("partial content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("the loop continues after a partial tool result");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(5)
        }
    );
    assert_eq!(*driver.executions.borrow(), 2);
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Partial { content },
        }] if *call_id == call.call_id() && content == partial_content
    ));
    assert!(
        appends
            .iter()
            .all(|append| !matches!(append.facts(), [ModelRunFactInputDto::Failed { .. }])),
        "a partial tool result never records a failed fact"
    );
    drop(appends);
    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].messages(),
        vec![
            ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                .expect("message is valid"),
            cache_breakpoint(
                ModelMessageDto::tool_result(call.call_id(), partial_content)
                    .expect("message is valid"),
            ),
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
            cache_breakpoint(
                ModelMessageDto::tool_result(second.call_id(), "two").expect("message is valid"),
            ),
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
            cache_breakpoint(
                ModelMessageDto::tool_result(second.call_id(), "two").expect("message is valid"),
            ),
        ]
    );
}

#[test]
fn tool_round_reasoning_is_attached_to_later_requests_in_round_order() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let first = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let second = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::reasoning_delta("think ").expect("reasoning is valid")),
            Ok(ModelEventDto::tool_call(first.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::reasoning_delta("second round").expect("reasoning is valid")),
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
    .expect("tool loop completes");
    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(9)
        }
    );

    let first_reasoning = AssistantReasoningDto::new(vec![first.call_id()], "think ")
        .expect("fixture reasoning is valid");
    let second_reasoning = AssistantReasoningDto::new(vec![second.call_id()], "second round")
        .expect("fixture reasoning is valid");
    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].assistant_reasoning().is_empty());
    assert_eq!(
        requests[1].assistant_reasoning(),
        std::slice::from_ref(&first_reasoning)
    );
    assert_eq!(
        requests[2].assistant_reasoning(),
        [first_reasoning, second_reasoning].as_slice()
    );
    drop(requests);

    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::ReasoningDeltaRecorded { content }] if content == "think "
    ));
    assert!(matches!(
        appends[4].facts(),
        [ModelRunFactInputDto::ReasoningDeltaRecorded { content, .. }]
            if content == "second round"
    ));
}

#[test]
fn empty_reasoning_channel_round_attaches_presence_without_blank_facts() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::reasoning_presence()),
            Ok(ModelEventDto::tool_call(call.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::succeeded("one").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("textless reasoning round completes");
    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(4)
        }
    );

    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].assistant_reasoning().is_empty());
    assert_eq!(requests[1].assistant_reasoning().len(), 1);
    assert_eq!(
        requests[1].assistant_reasoning()[0].tool_call_ids(),
        &[call.call_id()]
    );
    assert!(requests[1].assistant_reasoning()[0].text().is_empty());
    drop(requests);

    let appends = repository.appends.borrow();
    assert!(
        appends.iter().all(|append| append
            .facts()
            .iter()
            .all(|fact| !matches!(fact, ModelRunFactInputDto::ReasoningDeltaRecorded { .. }))),
        "a textless reasoning channel must not become a blank durable reasoning fact"
    );
}

#[test]
fn reasoning_echo_beyond_attachment_bound_terminalizes_as_typed_failed_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    // Each fragment stays inside the durable 512 KiB per-fact bound, so the
    // durable path accepts both; only the accumulated per-round echo crosses
    // the attachment's representable bound.
    let first = "a".repeat(300 * 1024);
    let second = "b".repeat(300 * 1024);
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::reasoning_delta(first.clone()).expect("reasoning is valid")),
        Ok(ModelEventDto::reasoning_delta(second.clone()).expect("reasoning is valid")),
        Ok(ModelEventDto::tool_call(call)),
    ]);
    let port = ScriptedPort::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("an unrepresentable attachment terminalizes as a typed failed run");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(5)
        }
    );
    assert_eq!(*driver.executions.borrow(), 1);
    assert!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .is_empty(),
        "the failed round never executes its tool call"
    );
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 4);
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::ReasoningDeltaRecorded { content, .. }] if content == &first
    ));
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ReasoningDeltaRecorded { content, .. }] if content == &second
    ));
    assert!(matches!(
        appends[3].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 1, failure },
            ModelRunFactInputDto::Failed { .. },
        ] if failure.code() == "reasoning_attachment_unrepresentable"
            && failure.retry() == ErrorRetryDto::Never
    ));
    assert!(matches!(
        appends[3].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { .. },
            ModelRunFactInputDto::Failed { failure },
        ] if failure.code() == "reasoning_attachment_unrepresentable"
    ));
    assert_eq!(appends[3].status(), Some(RunStatusDto::Failed));
}

#[test]
fn control_character_reasoning_echo_terminalizes_as_typed_failed_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    // The durable fact path accepts the control character; the transient
    // attachment DTO rejects it.
    let content = "thinking\u{7}".to_owned();
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::reasoning_delta(content.clone()).expect("reasoning is valid")),
        Ok(ModelEventDto::tool_call(call)),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let port = ScriptedPort::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("an unrepresentable attachment terminalizes as a typed failed run");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(4)
        }
    );
    assert_eq!(*driver.executions.borrow(), 1);
    assert!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .is_empty(),
        "the failed round never executes its tool call"
    );
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 3);
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::ReasoningDeltaRecorded { content: recorded, .. }]
            if recorded == &content
    ));
    assert!(matches!(
        appends[2].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 1, failure },
            ModelRunFactInputDto::Failed { .. },
        ] if failure.code() == "reasoning_attachment_unrepresentable"
            && failure.retry() == ErrorRetryDto::Never
    ));
    assert!(matches!(
        appends[2].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { .. },
            ModelRunFactInputDto::Failed { failure },
        ] if failure.code() == "reasoning_attachment_unrepresentable"
    ));
    assert_eq!(appends[2].status(), Some(RunStatusDto::Failed));
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
        Ok(ModelEventDto::tool_call(call.clone())),
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
            cursor: RunEventCursorDto::new(4)
        }
    );
    let appends = repository.appends.borrow();
    assert_eq!(appends.len(), 4);
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
        [ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Failed { failure: recorded },
        }] if *call_id == call.call_id()
            && recorded.code() == "tool_execution_failed"
            && recorded.retry() == ErrorRetryDto::Manual
    ));
    assert_eq!(appends[2].status(), None);
    assert!(matches!(
        appends[3].facts(),
        [ModelRunFactInputDto::Failed { failure }]
            if failure.code() == "tool_execution_failed"
                && failure.retry() == ErrorRetryDto::Manual
    ));
    assert_eq!(appends[3].status(), Some(RunStatusDto::Failed));
    // The injected provider diagnostic must never be recorded in the durable
    // appends; only the safe code and retry classification cross the boundary.
    let rendered_appends = format!("{appends:?}");
    assert!(!rendered_appends.contains("sensitive provider detail"));
}

#[test]
fn interruption_during_tool_execution_records_the_notice_and_continues() {
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
    let signal = ModelCancellationSignal::new();
    let (called_tx, called_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let port = GatedPort::new(called_tx, release_rx);
    let clock = ImmediateTime::new();
    let execution_signal = signal.clone();

    let execution = std::thread::spawn(move || {
        let outcome = futures_executor::block_on(
            ModelRunExecutionService::new(&repository, &driver, &clock, &port).execute(
                ModelRunExecutionInputDto::new(
                    session_id,
                    run_id,
                    request(run_id, "fixture"),
                    config,
                    execution_signal,
                ),
            ),
        );
        (outcome, repository, driver, port)
    });
    called_rx.recv().expect("the gated port call is observed");
    signal.cancel();
    release_tx
        .send(())
        .expect("the gated port call is released");
    let (outcome, repository, driver, port) = execution.join().expect("execution thread completes");
    let outcome = outcome.expect("the interrupted run continues and completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        1,
        "the in-flight tool ran exactly once"
    );
    let appends = repository.appends.borrow();
    let facts = appends
        .iter()
        .flat_map(|input| input.facts())
        .collect::<Vec<_>>();
    assert!(facts.iter().any(|fact| matches!(
        fact,
        ModelRunFactInputDto::ToolResultRecorded {
            outcome: ToolResultOutcomeDto::Succeeded { .. },
            ..
        }
    )));
    assert!(facts.iter().any(|fact| matches!(
        fact,
        ModelRunFactInputDto::InterruptNoticeRecorded { content }
            if content == intention_runtime::INTERRUPT_NOTICE
    )));
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Completed]
    );
    let requests = driver.requests.borrow();
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::Notice
            && message.content() == intention_runtime::INTERRUPT_NOTICE
    }));
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
fn interruption_during_a_provider_round_records_a_notice_and_continues() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("partial").expect("text is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    driver
        .cancel_during_stream
        .borrow_mut()
        .replace((1, signal.clone()));
    let port = ScriptedPort::new(Vec::new());
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::new(&repository, &driver, &clock, &port).execute(
            ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                signal,
            ),
        ),
    )
    .expect("the interrupted round continues and completes");

    assert!(
        matches!(outcome, ModelRunExecutionOutcomeDto::Completed { .. }),
        "the interruption continues the run instead of terminalizing it, got {outcome:?}"
    );
    assert_eq!(*driver.executions.borrow(), 2);
    let appends = repository.appends.borrow();
    let facts = appends
        .iter()
        .flat_map(|input| input.facts())
        .collect::<Vec<_>>();
    assert!(facts.iter().any(|fact| matches!(
        fact,
        ModelRunFactInputDto::AssistantContentAppended { content, .. } if content == "partial"
    )));
    assert!(
        facts
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::InterruptNoticeRecorded { .. }))
    );
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Completed]
    );
}

#[test]
fn interruption_before_port_invocation_answers_the_call_with_a_partial_result() {
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
        ToolResultOutcomeDto::succeeded("never used").expect("content is valid")
    )]);
    let signal = ModelCancellationSignal::new();
    // The interrupt lands on the tool-call append, before the port invocation.
    repository
        .cancel_after_append
        .borrow_mut()
        .replace((2, signal.clone()));
    let clock = ImmediateTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::new(&repository, &driver, &clock, &port).execute(
            ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                signal,
            ),
        ),
    )
    .expect("the interrupted batch continues and completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        0,
        "the interrupt never starts the tool effect"
    );
    let appends = repository.appends.borrow();
    let facts = appends
        .iter()
        .flat_map(|input| input.facts())
        .collect::<Vec<_>>();
    assert!(facts.iter().any(|fact| matches!(
        fact,
        ModelRunFactInputDto::ToolResultRecorded { outcome: ToolResultOutcomeDto::Partial { content }, .. }
            if content == intention_runtime::TOOL_INTERRUPT_NOTICE
    )));
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
        ModelRunExecutionService::with_commit_observer(
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
fn interruption_while_the_round_is_waiting_records_a_notice_and_continues() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    let (entered_tx, entered_rx) = mpsc::channel();
    let driver = PendingAfterStartedDriver {
        entered: std::sync::Mutex::new(Some(entered_tx)),
        executions: std::sync::Mutex::new(0),
    };
    let port = ScriptedPort::new(Vec::new());
    let release = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clock = FlagTime::new(std::sync::Arc::clone(&release));
    let execution_signal = signal.clone();

    let execution = std::thread::spawn(move || {
        let outcome = futures_executor::block_on(
            ModelRunExecutionService::new(&repository, &driver, &clock, &port).execute(
                ModelRunExecutionInputDto::new(
                    session_id,
                    run_id,
                    request(run_id, "fixture"),
                    config,
                    execution_signal,
                ),
            ),
        );
        (outcome, repository)
    });
    entered_rx.recv().expect("the provider round started");
    signal.cancel();
    let (outcome, repository) = execution.join().expect("execution thread completes");
    let outcome = outcome.expect("the interrupted round continues and completes");

    assert!(
        matches!(outcome, ModelRunExecutionOutcomeDto::Completed { .. }),
        "the interruption continues the run instead of terminalizing it, got {outcome:?}"
    );
    let appends = repository.appends.borrow();
    let facts = appends
        .iter()
        .flat_map(|input| input.facts())
        .collect::<Vec<_>>();
    assert!(
        facts
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::InterruptNoticeRecorded { .. }))
    );
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Completed]
    );
}

#[test]
fn invalid_tool_input_json_records_failed_result_and_terminalizes() {
    // The daemon's parse_tool_input (crates/intention-daemon/src/lib.rs)
    // rejects arguments that are not valid typed input with the
    // `invalid_tool_input_json` failure; non-JSON arguments cannot even be
    // represented in ToolCallDto (rejected at construction). The runtime
    // contract is that a port answering with that typed failed outcome
    // durably records a failed ToolResultRecorded fact and then terminalizes
    // Failed without scheduling any retry — the ADR's invalid-tool-input claim.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::tool_call(call.clone())),
    ]);
    let failure = RunFailureDto::new("invalid_tool_input_json", ErrorRetryDto::Never, None)
        .expect("failure is valid");
    let port = ScriptedPort::new(vec![Ok(ToolResultOutcomeDto::failed(failure))]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("invalid tool input terminalizes safely");

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
        1,
        "invalid input executes the tool exactly once, never re-invoking it"
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Failed { failure: recorded },
        }] if *call_id == call.call_id()
            && recorded.code() == "invalid_tool_input_json"
            && recorded.retry() == ErrorRetryDto::Never
    ));
    assert_eq!(appends[2].status(), None);
    assert!(matches!(
        appends[3].facts(),
        [ModelRunFactInputDto::Failed { failure: terminal }]
            if terminal.code() == "invalid_tool_input_json"
                && terminal.retry() == ErrorRetryDto::Never
    ));
    assert_eq!(appends[3].status(), Some(RunStatusDto::Failed));
    assert!(
        appends.iter().all(|input| {
            !input
                .facts()
                .iter()
                .any(|fact| matches!(fact, ModelRunFactInputDto::RetryScheduled { .. }))
        }),
        "invalid tool input never schedules a retry"
    );
}

#[test]
fn second_tool_call_does_not_start_until_first_finishes() {
    // Tool calls from one provider round must execute strictly sequentially:
    // while the first call's port future is pending, the second call must not
    // be invoked. After the first finishes, both results record durably in
    // call order and the run completes.
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
    let (called_tx, called_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let port = std::sync::Arc::new(GateFirstCallPort::new(
        called_tx,
        release_rx,
        vec![
            Ok(ToolResultOutcomeDto::succeeded("one").expect("content is valid")),
            Ok(ToolResultOutcomeDto::succeeded("two").expect("content is valid")),
        ],
    ));
    let clock = ImmediateTime::new();
    let execution_port = std::sync::Arc::clone(&port);

    let execution = std::thread::spawn(move || {
        let outcome = futures_executor::block_on(
            ModelRunExecutionService::new(&repository, &driver, &clock, execution_port.as_ref())
                .execute(ModelRunExecutionInputDto::new(
                    session_id,
                    run_id,
                    request(run_id, "fixture"),
                    config,
                    ModelCancellationSignal::new(),
                )),
        );
        (outcome, repository, driver)
    });
    called_rx.recv().expect("the first gated call is observed");
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        1,
        "the second tool call must not start while the first is still running"
    );
    release_tx
        .send(())
        .expect("the first gated call is released");
    let (outcome, repository, driver) = execution.join().expect("execution thread completes");
    let outcome = outcome.expect("sequential tool loop completes");

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
    assert!(matches!(
        appends[5].facts(),
        [ModelRunFactInputDto::Finished { .. }]
    ));
    assert_eq!(appends[5].status(), Some(RunStatusDto::Completing));
}

#[test]
fn provider_failure_before_first_tool_round_is_retryable_within_attempt_budget() {
    // A retryable provider failure before any tool round (tool_round == 0)
    // schedules a retry while attempts remain; the second attempt then
    // completes the run. After a tool round the same failure is terminal
    // instead — that boundary is covered by
    // `provider_failure_after_tool_round_is_terminal_without_retry`.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![Err(ProviderErrorDto::unavailable(
            "provider_busy",
            true,
            None,
        )
        .expect("fixture provider error is valid"))],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("retryable pre-tool failure retries then completes");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            // The retry append carries two facts (ProviderAttemptFailed and
            // RetryScheduled), so the final cursor advances by five facts.
            cursor: RunEventCursorDto::new(5)
        }
    );
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        0,
        "no tool round occurs before the retry"
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[0].facts(),
        [ModelRunFactInputDto::ProviderAttemptStarted { attempt: 1 }]
    ));
    assert_eq!(appends[0].status(), Some(RunStatusDto::Running));
    assert!(matches!(
        appends[1].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 1, failure },
            ModelRunFactInputDto::RetryScheduled {
                failed_attempt: 1,
                next_attempt: 2
            },
        ] if failure.code() == "provider_busy" && failure.retry() == ErrorRetryDto::Delayed
    ));
    assert_eq!(appends[1].status(), None);
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ProviderAttemptStarted { attempt: 2 }]
    ));
    assert_eq!(appends[2].status(), None);
    assert!(matches!(
        appends[3].facts(),
        [ModelRunFactInputDto::Finished { .. }]
    ));
    assert_eq!(appends[3].status(), Some(RunStatusDto::Completing));
    assert!(
        appends.iter().all(|input| !input
            .facts()
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::Failed { .. }))),
        "the retried attempt completes without any terminal failure"
    );
}

#[test]
fn interruption_while_the_round_select_waits_records_a_notice_and_continues() {
    // The time port signals when the round's timeout sleep is first polled,
    // which proves the round's select is already suspended on the provider
    // stream. The interruption then must be observed by that select rather
    // than by a later pre-stream check, and the run continues.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    let (started_tx, _started_rx) = mpsc::channel();
    let driver = PendingAfterStartedDriver {
        entered: std::sync::Mutex::new(Some(started_tx)),
        executions: std::sync::Mutex::new(0),
    };
    let port = ScriptedPort::new(Vec::new());
    let (select_tx, select_rx) = mpsc::channel();
    let clock = RoundSelectTime {
        entered: std::sync::Mutex::new(Some(select_tx)),
    };
    let execution_signal = signal.clone();

    let execution = std::thread::spawn(move || {
        let outcome = futures_executor::block_on(
            ModelRunExecutionService::new(&repository, &driver, &clock, &port).execute(
                ModelRunExecutionInputDto::new(
                    session_id,
                    run_id,
                    request(run_id, "fixture"),
                    config,
                    execution_signal,
                ),
            ),
        );
        (outcome, repository, driver)
    });
    select_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the round select waits on the provider stream");
    signal.cancel();
    let (outcome, repository, driver) = execution.join().expect("execution thread completes");
    let outcome = outcome.expect("the interrupted round continues and completes");

    assert!(
        matches!(outcome, ModelRunExecutionOutcomeDto::Completed { .. }),
        "the interruption continues the run, got {outcome:?}"
    );
    assert_eq!(
        *driver
            .executions
            .lock()
            .expect("driver recorder is available"),
        2,
        "the run re-enters the provider after the interruption"
    );
    let appends = repository.appends.borrow();
    let facts = appends
        .iter()
        .flat_map(|input| input.facts())
        .collect::<Vec<_>>();
    assert!(
        facts
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::InterruptNoticeRecorded { .. }))
    );
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Completed]
    );
}

#[test]
fn interruption_during_the_retry_delay_starts_the_next_attempt() {
    // A retryable pre-tool provider failure schedules a retry and the run then
    // waits out the retry delay. An interruption inside that wait appends its
    // notice, clears the signal, and starts the second attempt immediately.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![Err(ProviderErrorDto::unavailable(
            "provider_busy",
            true,
            None,
        )
        .expect("fixture provider error is valid"))],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(Vec::new());
    let signal = ModelCancellationSignal::new();
    let (entered_tx, entered_rx) = mpsc::channel();
    let clock = RetryDelayTime {
        entered: entered_tx,
    };
    let execution_signal = signal.clone();

    let execution = std::thread::spawn(move || {
        let outcome = futures_executor::block_on(
            ModelRunExecutionService::new(&repository, &driver, &clock, &port).execute(
                ModelRunExecutionInputDto::new(
                    session_id,
                    run_id,
                    request(run_id, "fixture"),
                    config,
                    execution_signal,
                ),
            ),
        );
        (outcome, repository, driver, port)
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the run enters the retry delay");
    signal.cancel();
    let (outcome, repository, driver, port) = execution.join().expect("execution thread completes");
    let outcome = outcome.expect("the interrupted retry continues");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        *driver.executions.borrow(),
        2,
        "the interrupted wait starts the second attempt"
    );
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        0
    );
    let appends = repository.appends.borrow();
    let facts = appends
        .iter()
        .flat_map(|input| input.facts())
        .collect::<Vec<_>>();
    assert!(matches!(
        appends[1].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 1, failure },
            ModelRunFactInputDto::RetryScheduled {
                failed_attempt: 1,
                next_attempt: 2
            },
        ] if failure.code() == "provider_busy"
            && failure.retry() == ErrorRetryDto::Delayed
    ));
    assert!(
        facts
            .iter()
            .any(|fact| matches!(fact, ModelRunFactInputDto::InterruptNoticeRecorded { .. }))
    );
    assert!(facts.iter().any(|fact| matches!(
        fact,
        ModelRunFactInputDto::ProviderAttemptStarted { attempt: 2 }
    )));
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Completed]
    );
}

#[test]
fn interruption_signalled_by_the_retry_append_still_starts_the_next_attempt() {
    // The repository interrupts the run from inside the retry-scheduling
    // append, so the signal is already set when the wait would begin. The wait
    // appends its notice without arming the sub-second retry delay at all.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![Err(ProviderErrorDto::unavailable(
            "provider_busy",
            true,
            None,
        )
        .expect("fixture provider error is valid"))],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(Vec::new());
    let signal = ModelCancellationSignal::new();
    repository
        .cancel_after_append
        .borrow_mut()
        .replace((2, signal.clone()));
    let clock = RecordingTime::new();

    let outcome = futures_executor::block_on(
        ModelRunExecutionService::new(&repository, &driver, &clock, &port).execute(
            ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                signal,
            ),
        ),
    )
    .expect("interruption during the retry append continues the run");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        *driver.executions.borrow(),
        2,
        "the interrupted retry still starts the second attempt"
    );
    assert!(
        clock
            .sleeps
            .lock()
            .expect("sleep recorder is available")
            .iter()
            .all(|duration| *duration >= Duration::from_secs(1)),
        "an already-interrupted run never arms the sub-second retry delay"
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[1].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { .. },
            ModelRunFactInputDto::RetryScheduled {
                failed_attempt: 1,
                next_attempt: 2
            },
        ]
    ));
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::InterruptNoticeRecorded { .. }]
    ));
    drop(appends);
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Completed]
    );
}

#[test]
fn finished_with_tool_calls_attaches_reasoning_and_continues_the_loop() {
    // A round may finish its stream while already carrying tool calls. The
    // round's reasoning channel must still attach to the assistant tool-call
    // message that continues the loop, exactly like a round whose stream ends
    // without a finish event.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::reasoning_delta("plan ").expect("reasoning is valid")),
            Ok(ModelEventDto::tool_call(call.clone())),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::succeeded("hello").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("a finished round with tool calls continues the loop");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(5)
        }
    );
    assert_eq!(
        *driver.executions.borrow(),
        2,
        "the finished-with-calls round drives the continuation round"
    );
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .as_slice(),
        &[(session_id, run_id, call.clone())]
    );
    let reasoning = AssistantReasoningDto::new(vec![call.call_id()], "plan ")
        .expect("fixture reasoning is valid");
    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].assistant_reasoning(),
        std::slice::from_ref(&reasoning),
        "the finished-with-calls round keeps its reasoning attachment"
    );
    assert_eq!(
        requests[1].messages(),
        vec![
            ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                .expect("message is valid"),
            cache_breakpoint(
                ModelMessageDto::tool_result(call.call_id(), "hello").expect("message is valid"),
            ),
        ]
    );
    drop(requests);
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::ReasoningDeltaRecorded { content, .. }] if content == "plan "
    ));
    assert!(matches!(
        appends[2].facts(),
        [ModelRunFactInputDto::ToolCallRecorded { call: recorded }] if *recorded == call
    ));
    assert!(matches!(
        appends[3].facts(),
        [ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Succeeded { content },
        }] if *call_id == call.call_id() && content == "hello"
    ));
    assert!(matches!(
        appends[4].facts(),
        [ModelRunFactInputDto::Finished { .. }]
    ));
    assert_eq!(appends[4].status(), Some(RunStatusDto::Completing));
}

#[test]
fn pending_messages_join_the_live_context_at_a_tool_batch_boundary() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    repository
        .pending
        .borrow_mut()
        .push_back((TurnId::new(), "pending message".to_owned()));
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
        ToolResultOutcomeDto::succeeded("tool output").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("the run completes after joining the pending message");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        *driver.executions.borrow(),
        2,
        "the pending message continues the same run"
    );
    let requests = driver.requests.borrow();
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::User && message.content() == "pending message"
    }));
    drop(requests);
    assert!(repository.pending_facts.borrow().iter().any(|fact| matches!(
        fact.input(),
        ModelRunFactInputDto::UserMessageAppended { content, .. } if content == "pending message"
    )));
    assert!(repository.pending.borrow().is_empty());
}

#[test]
fn a_pending_message_at_the_finish_boundary_continues_instead_of_completing() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    repository
        .pending
        .borrow_mut()
        .push_back((TurnId::new(), "arrived before completion".to_owned()));
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("the run continues with the joined message and then completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        *driver.executions.borrow(),
        2,
        "the finished round is followed by another model step"
    );
    let requests = driver.requests.borrow();
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::User && message.content() == "arrived before completion"
    }));
    drop(requests);
    assert!(matches!(
        repository.pending_facts.borrow().as_slice(),
        [fact] if matches!(
            fact.input(),
            ModelRunFactInputDto::UserMessageAppended { .. }
        )
    ));
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends.last().expect("finished append").facts(),
        [ModelRunFactInputDto::Finished { .. }]
    ));
}
