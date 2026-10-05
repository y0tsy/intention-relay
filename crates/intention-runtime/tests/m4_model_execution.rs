#![allow(
    clippy::expect_used,
    reason = "Execution fixtures use expect to provide precise failures."
)]

use std::{cell::RefCell, future, time::Duration};

use futures_util::{StreamExt, stream};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    ModelRunFactDto, ModelRunFactInputDto, ModelRunProjectionDto, RunEventCursorDto, RunModeDto,
    RunProjectionDto, RunSnapshotDto, RunStatusDto, SessionProjectionDto, ToolResultOutcomeDto,
    WorkspaceRootDto,
};
use intention_model::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto, UsageDto,
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
    ConfigRevisionId, DtoResult, ErrorDto, ProjectId, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, TurnId, WorkspaceId,
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
    /// Fails exactly the append whose expected cursor matches this entry.
    append_failure_at: RefCell<Option<(u64, ErrorDto)>>,
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
            append_failure_at: RefCell::new(None),
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
        if self
            .append_failure_at
            .borrow()
            .as_ref()
            .is_some_and(|(cursor, _)| *cursor == input.expected_cursor().value())
        {
            return Err(self
                .append_failure_at
                .borrow_mut()
                .take()
                .expect("configured append failure exists")
                .1);
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
        intention_storage::AppendPendingUserTurnsOutcomeDto::new(*self.cursor.borrow(), Vec::new())
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
        _session_id: intention_types::SessionId,
        _run_id: intention_types::RunId,
        _call: intention_model::ToolCallDto,
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
}

impl ModelRunCommitObserver for RecordingCommitObserver {
    fn observe_model_run_commit(&self, committed: ModelRunCommitDto) {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .push(committed);
    }
}

#[test]
fn observer_receives_only_successful_fact_appends_and_completion_transition() {
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

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto { .. }
        }
    ));
    let commits = observer
        .commits
        .lock()
        .expect("observer recorder remains available");
    assert_eq!(commits.len(), 4, "attempt, content, finishing, completion");
    assert!(
        commits
            .windows(2)
            .all(|pair| pair[0].cursor() <= pair[1].cursor())
    );
    assert!(commits.iter().all(|commit| {
        commit.session_id() == session_id
            && commit.run_id() == run_id
            && commit.snapshot().run_id() == run_id
    }));
    assert_eq!(
        commits
            .last()
            .expect("completion is observed")
            .snapshot()
            .run_projection()
            .status(),
        RunStatusDto::Completed
    );
    drop(commits);
}

#[test]
fn observer_is_not_called_when_a_durable_append_fails() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    *repository.append_failure.borrow_mut() = Some(ErrorDto::unavailable(
        "append_failed",
        "append fails before commit",
    ));
    let driver = ScriptedDriver::new(vec![Ok(ModelEventDto::started())]);
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
            .commits
            .lock()
            .expect("observer recorder remains available")
            .is_empty()
    );
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
    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(1)
        }
    );
    assert_eq!(*driver.executions.borrow(), 0);
    let append = repository.appends.borrow();
    assert_eq!(append.len(), 1);
    assert_eq!(append[0].status(), Some(RunStatusDto::Failed));
    assert!(matches!(
        append[0].facts(),
        [ModelRunFactInputDto::Failed { .. }]
    ));
}

#[test]
fn streams_ordered_facts_splits_utf8_content_and_completes_in_two_stages() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let content = format!("{}€tail", "a".repeat(4094));
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta(content).expect("text is valid")),
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
    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(6)
        }
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[0].facts(),
        [ModelRunFactInputDto::ProviderAttemptStarted { attempt: 1 }]
    ));
    let content_batches = appends
        .iter()
        .filter_map(|input| input.facts().first())
        .filter_map(|fact| match fact {
            ModelRunFactInputDto::AssistantContentAppended { content, .. } => Some(content),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        content_batches
            .iter()
            .map(|content| content.len())
            .collect::<Vec<_>>(),
        vec![4094, 7]
    );
    assert_eq!(
        content_batches
            .iter()
            .map(|content| content.as_str())
            .collect::<String>(),
        format!("{}€tail", "a".repeat(4094))
    );
    assert!(appends.iter().any(|input| matches!(
        input.facts(),
        [ModelRunFactInputDto::ReasoningDeltaRecorded { .. }]
    )));
    assert!(
        appends
            .iter()
            .any(|input| matches!(input.facts(), [ModelRunFactInputDto::UsageRecorded { .. }]))
    );
    let finished = appends.last().expect("finished append exists");
    assert!(matches!(
        finished.facts(),
        [ModelRunFactInputDto::Finished { .. }]
    ));
    assert_eq!(finished.status(), Some(RunStatusDto::Completing));
    assert_eq!(repository.transitions.borrow().as_slice().len(), 1);
    assert_eq!(
        repository.transitions.borrow()[0].status(),
        RunStatusDto::Completed
    );
}

#[test]
fn malformed_provider_and_eof_streams_safely_fail_without_invalid_facts() {
    for events in [
        vec![Ok(
            ModelEventDto::text_delta("invalid").expect("text is valid")
        )],
        vec![Ok(ModelEventDto::started())],
        vec![
            Ok(ModelEventDto::started()),
            Err(ProviderErrorDto::unavailable("provider_down", false, None)
                .expect("error is valid")),
        ],
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
        assert!(matches!(
            outcome,
            ModelRunExecutionOutcomeDto::Failed { .. }
        ));
        let appends = repository.appends.borrow();
        assert!(matches!(
            appends.last().expect("failure append").facts(),
            [
                ModelRunFactInputDto::ProviderAttemptFailed { .. },
                ModelRunFactInputDto::Failed { .. }
            ]
        ));
        assert!(
            !appends
                .iter()
                .flat_map(|input| input.facts())
                .any(|fact| matches!(fact, ModelRunFactInputDto::AssistantContentAppended { .. }))
        );
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
    let appends = repository.appends.borrow();
    let facts = appends
        .iter()
        .flat_map(|input| input.facts())
        .collect::<Vec<_>>();
    assert!(facts.iter().any(|fact| matches!(
        fact,
        ModelRunFactInputDto::AssistantContentAppended { content, .. }
            if content == "partial answer"
    )));
    assert!(facts.iter().any(|fact| matches!(
        fact,
        ModelRunFactInputDto::InterruptNoticeRecorded { content }
            if content == intention_runtime::INTERRUPT_NOTICE
    )));
    // The continuation request carries the notice, so the model knows its
    // previous call was stopped before a final result.
    let requests = driver.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::Notice
            && message.content() == intention_runtime::INTERRUPT_NOTICE
    }));
    let transitions = repository.transitions.borrow();
    assert_eq!(
        transitions
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Completed]
    );
    drop(transitions);
    assert_eq!(appends[0].status(), Some(RunStatusDto::Running));
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
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[1].facts(),
        [ModelRunFactInputDto::InterruptNoticeRecorded { .. }]
    ));
    assert!(matches!(
        appends.last().expect("finished append").facts(),
        [ModelRunFactInputDto::Finished { .. }]
    ));
}

#[test]
fn an_interrupt_whose_notice_cannot_commit_surfaces_the_storage_error() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = ModelCancellationSignal::new();
    // The attempt-start append commits at cursor 0; the interruption notice
    // append then fails at cursor 1.
    *repository.append_failure_at.borrow_mut() = Some((
        1,
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
        cancel_during_stream: Some((0, signal.clone())),
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
}

#[test]
fn retry_is_ordered_once_and_waits_exactly_250_milliseconds() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
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
        cancel_during_stream: None,
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
    let facts = repository.appends.borrow();
    assert!(matches!(
        facts[1].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 1, .. },
            ModelRunFactInputDto::RetryScheduled {
                failed_attempt: 1,
                next_attempt: 2
            },
        ]
    ));
    assert!(matches!(
        facts[2].facts(),
        [ModelRunFactInputDto::ProviderAttemptStarted { attempt: 2 }]
    ));
    assert_eq!(facts[0].status(), Some(RunStatusDto::Running));
    assert_eq!(facts[1].status(), None);
    assert_eq!(facts[2].status(), None);
    assert!(matches!(
        facts[3].facts(),
        [ModelRunFactInputDto::Finished { .. }]
    ));
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
    assert!(
        matches!(outcome, ModelRunExecutionOutcomeDto::Failed { cursor } if cursor == RunEventCursorDto::new(1))
    );
    assert_eq!(*driver.executions.borrow(), 0);
    assert!(
        matches!(repository.appends.borrow()[0].facts(), [ModelRunFactInputDto::Failed { failure }] if failure.code() == "provider_configuration_unavailable")
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

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(1)
        }
    );
    assert_eq!(*driver.executions.borrow(), 0);
    assert!(matches!(
        repository.appends.borrow()[0].facts(),
        [ModelRunFactInputDto::Failed { failure }]
            if failure.code() == "provider_configuration_unavailable"
    ));
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

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(1)
        }
    );
    assert_eq!(*driver.executions.borrow(), 0);
    assert!(matches!(
        repository.appends.borrow()[0].facts(),
        [ModelRunFactInputDto::Failed { failure }]
            if failure.code() == "provider_configuration_unavailable"
    ));
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
    assert!(repository.appends.borrow().is_empty());
    assert!(repository.transitions.borrow().is_empty());
}

#[test]
fn retryable_failure_after_durable_reasoning_does_not_retry() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::reasoning_delta("why").expect("reasoning is valid")),
        Err(ProviderErrorDto::unavailable("provider_down", true, None).expect("error is valid")),
    ]);

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("durable reasoning prevents a retry");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed { .. }
    ));
    assert_eq!(*driver.executions.borrow(), 1);
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[2].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 1, .. },
            ModelRunFactInputDto::Failed { .. }
        ]
    ));
    assert!(appends.iter().all(|input| !matches!(
        input.facts(),
        [ModelRunFactInputDto::RetryScheduled { .. }, ..]
    )));
}

#[test]
fn exhausted_retryable_failure_stops_after_second_attempt() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver {
        preflight_error: None,
        events: RefCell::new(vec![
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
        ]),
        executions: RefCell::new(0),
        requests: RefCell::new(Vec::new()),
        cancel_during_stream: None,
        pending_stream: false,
    };

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        ModelCancellationSignal::new(),
    )
    .expect("second retryable failure becomes terminal");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed { .. }
    ));
    assert_eq!(*driver.executions.borrow(), 2);
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends
            .last()
            .expect("terminal failure was appended")
            .facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 2, .. },
            ModelRunFactInputDto::Failed { .. }
        ]
    ));
    assert_eq!(
        appends
            .iter()
            .filter(|input| matches!(
                input.facts(),
                [
                    ModelRunFactInputDto::ProviderAttemptFailed { .. },
                    ModelRunFactInputDto::RetryScheduled { .. }
                ]
            ))
            .count(),
        1
    );
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

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(6)
        }
    );
    assert_eq!(*driver.executions.borrow(), 2);
    assert_eq!(
        clock.sleeps.borrow().as_slice(),
        &[
            Duration::from_secs(30),
            Duration::from_millis(250),
            Duration::from_secs(30)
        ]
    );
    let appends = repository.appends.borrow();
    assert!(matches!(
        appends[1].facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 1, failure },
            ModelRunFactInputDto::RetryScheduled { .. }
        ] if failure.code() == "provider_attempt_timed_out"
    ));
    assert!(matches!(
        appends.last().expect("terminal timeout was appended").facts(),
        [
            ModelRunFactInputDto::ProviderAttemptFailed { attempt: 2, failure },
            ModelRunFactInputDto::Failed { failure: terminal_failure }
        ] if failure.code() == "provider_attempt_timed_out"
            && terminal_failure.code() == "provider_attempt_timed_out"
    ));
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
    let requests = driver.requests.borrow();
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::Notice
            && message.content() == intention_runtime::INTERRUPT_NOTICE
    }));
    drop(requests);
    let transitions = repository.transitions.borrow();
    assert_eq!(
        transitions
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Completed]
    );
}
