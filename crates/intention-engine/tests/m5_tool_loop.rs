#![allow(
    clippy::expect_used,
    reason = "Execution fixtures use expect to provide precise failures."
)]

mod common;

use std::{collections::VecDeque, future, sync::mpsc, time::Duration};

use common::{
    FakeRepository, ImmediateTime, RecordedTransition, RecordingCommitObserver, ScriptedPort, time,
};
use futures_util::{StreamExt, stream};
use intention_config::ConfigSnapshotDto;
use intention_engine::{
    ModelRunCommitDto, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, ModelSleepFuture, ModelTimePort, RunCancellation, ToolExecutionPort,
    ToolResultOutcomeDto,
};
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId, TimestampDto, ToolCallId};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunStatusDto};
use intention_providers::{
    AssistantReasoningDto, FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto,
    ModelEventDto, ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto,
    ModelRoleDto, ModelToolDefinitionDto, ProviderErrorDto, ToolCallDto,
};
use intention_test_support::{
    ScriptedDriver, fixture_snapshot_with_context_window, fixture_snapshot_with_model, run_ready,
};

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

/// Emits one started event, signals the test, then blocks; later rounds finish.
///
/// Used to hold the first provider round mid-stream so the test can interrupt
/// the run while the round's select is waiting on the provider.
struct PendingAfterStartedDriver {
    entered: std::sync::Mutex<Option<mpsc::Sender<()>>>,
    executions: std::sync::Mutex<usize>,
}

impl ModelExecutionDriver for PendingAfterStartedDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }

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
            Ok(ToolResultOutcomeDto::completed("tool output").expect("tool output is valid"))
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
    signal: RunCancellation,
) -> DtoResult<ModelRunExecutionOutcomeDto> {
    let clock = ImmediateTime::new();
    run_ready(
        ModelRunExecutionService::new(
            repository,
            driver,
            &clock,
            &RecordingCommitObserver::new(),
            port,
        )
        .execute(ModelRunExecutionInputDto::new(
            repository.session_id,
            repository.run_id,
            request,
            config,
            signal,
        )),
    )
}

#[test]
fn tool_call_executes_tool_and_completes() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
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
        ToolResultOutcomeDto::completed("hello world").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("tool loop completes");

    let ModelRunExecutionOutcomeDto::Completed { run } = outcome else {
        unreachable!("a stop reason completes the run");
    };
    assert_eq!(run.status(), RunStatusDto::Completed);
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        port.calls().as_slice(),
        &[(session_id, run_id, call.clone())]
    );
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![
            (MessageKindDto::Assistant, "before "),
            (MessageKindDto::Assistant, "after"),
        ],
        "each completed model step commits one assistant row"
    );
    drop(messages);
    assert!(
        repository.tool_results.borrow().is_empty(),
        "the dispatched tool path owns the tool-result transaction"
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(RecordedTransition::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
    let requests = driver.requests();
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
    let config = fixture_snapshot_with_context_window("fixture", Some(60));
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
        ToolResultOutcomeDto::completed(large.clone()).expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("windowed tool loop completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    let requests = driver.requests();
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
    assert!(
        repository.messages.borrow().is_empty(),
        "a tool round without assistant text commits no transcript row"
    );
}

#[test]
fn partial_tool_result_continues_the_loop_without_terminalizing() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
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
        RunCancellation::new(),
    )
    .expect("the loop continues after a partial tool result");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 2);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(
        finishes[0].status(),
        RunStatusDto::Completed,
        "a partial tool result never terminalizes the run"
    );
    drop(finishes);
    let requests = driver.requests();
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
    let config = fixture_snapshot_with_model("fixture");
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
        Ok(ToolResultOutcomeDto::completed("one").expect("content is valid")),
        Ok(ToolResultOutcomeDto::completed("two").expect("content is valid")),
    ]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("sequential tool loop completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        port.calls().as_slice(),
        &[
            (session_id, run_id, first.clone()),
            (session_id, run_id, second.clone()),
        ]
    );
    let requests = driver.requests();
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
    let config = fixture_snapshot_with_model("fixture");
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
        Ok(ToolResultOutcomeDto::completed("one").expect("content is valid")),
        Ok(ToolResultOutcomeDto::completed("two").expect("content is valid")),
    ]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("repeated tool rounds complete");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 3);
    assert_eq!(
        port.calls().as_slice(),
        &[
            (session_id, run_id, first.clone()),
            (session_id, run_id, second.clone()),
        ]
    );
    let requests = driver.requests();
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
    let config = fixture_snapshot_with_model("fixture");
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
        Ok(ToolResultOutcomeDto::completed("one").expect("content is valid")),
        Ok(ToolResultOutcomeDto::completed("two").expect("content is valid")),
    ]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("tool loop completes");
    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));

    let first_reasoning = AssistantReasoningDto::new(vec![first.call_id()], "think ")
        .expect("fixture reasoning is valid");
    let second_reasoning = AssistantReasoningDto::new(vec![second.call_id()], "second round")
        .expect("fixture reasoning is valid");
    let requests = driver.requests();
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

    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text(), message.reasoning()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Assistant, "after", None)],
        "a textless reasoning round never becomes a blank assistant row"
    );
}

#[test]
fn empty_reasoning_channel_round_attaches_presence_without_blank_facts() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
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
        ToolResultOutcomeDto::completed("one").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("textless reasoning round completes");
    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));

    let requests = driver.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].assistant_reasoning().is_empty());
    assert_eq!(requests[1].assistant_reasoning().len(), 1);
    assert_eq!(
        requests[1].assistant_reasoning()[0].tool_call_ids(),
        &[call.call_id()]
    );
    assert!(requests[1].assistant_reasoning()[0].text().is_empty());
    drop(requests);

    assert!(
        repository.messages.borrow().is_empty(),
        "a textless reasoning channel must not become a durable assistant row"
    );
}

#[test]
fn reasoning_echo_beyond_attachment_bound_terminalizes_as_typed_failed_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    // Each fragment stays inside the transient per-round bound; only the
    // accumulated echo crosses the attachment's representable bound.
    let first = "a".repeat(300 * 1024);
    let second = "b".repeat(300 * 1024);
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::reasoning_delta(first).expect("reasoning is valid")),
        Ok(ModelEventDto::reasoning_delta(second).expect("reasoning is valid")),
        Ok(ModelEventDto::tool_call(call)),
    ]);
    let port = ScriptedPort::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("an unrepresentable attachment terminalizes as a typed failed run");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("an unrepresentable echo fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "reasoning_attachment_unrepresentable");
    assert_eq!(driver.executions(), 1);
    assert!(
        port.calls().is_empty(),
        "the failed round never executes its tool call"
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(
        finishes[0].error_code(),
        Some("reasoning_attachment_unrepresentable")
    );
    assert!(repository.messages.borrow().is_empty());
}

#[test]
fn control_character_reasoning_echo_terminalizes_as_typed_failed_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    // The transient attachment DTO rejects the control character.
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::reasoning_delta("thinking\u{7}").expect("reasoning is valid")),
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
        RunCancellation::new(),
    )
    .expect("an unrepresentable attachment terminalizes as a typed failed run");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("an unrepresentable echo fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "reasoning_attachment_unrepresentable");
    assert_eq!(driver.executions(), 1);
    assert!(
        port.calls().is_empty(),
        "the failed round never executes its tool call"
    );
    assert_eq!(repository.finishes.borrow().len(), 1);
    assert!(repository.messages.borrow().is_empty());
}

#[test]
fn tool_failure_terminalizes_without_retry() {
    // A typed failed outcome from the port — including the daemon's
    // `invalid_tool_input_json` decoding failure — fails the run without
    // scheduling any retry or re-invoking the call.
    for (code, message) in [
        ("tool_denied", "the workspace denied the call"),
        ("invalid_tool_input_json", "tool arguments are invalid"),
    ] {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let config = fixture_snapshot_with_model("fixture");
        let repository = FakeRepository::new(session_id, run_id, config.clone());
        let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
        let driver = ScriptedDriver::new(vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(call)),
        ]);
        let port = ScriptedPort::new(vec![Ok(ToolResultOutcomeDto::failed(
            ErrorDto::validation(code, message),
        ))]);

        let outcome = execute(
            &repository,
            &driver,
            &port,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )
        .expect("a typed tool failure terminalizes safely");

        let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
            unreachable!("a failed tool call fails the run");
        };
        assert_eq!(run.status(), RunStatusDto::Failed);
        assert_eq!(error.code(), code);
        assert_eq!(driver.executions(), 1, "{code} must not retry");
        assert_eq!(
            port.calls().len(),
            1,
            "{code} executes the tool exactly once, never re-invoking it"
        );
        let finishes = repository.finishes.borrow();
        assert_eq!(finishes.len(), 1, "a tool failure never retries");
        assert_eq!(finishes[0].error_code(), Some(code));
        assert!(
            repository.tool_results.borrow().is_empty(),
            "the tool path committed the failed result row"
        );
    }
}

#[test]
fn port_infrastructure_error_terminalizes_with_the_safe_error() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::tool_call(call)),
    ]);
    let port = ScriptedPort::new(vec![Err(ErrorDto::unavailable(
        "tool_execution_failed",
        "the local tool executor is unavailable",
    ))]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("port failure terminalizes safely");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("a port failure fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "tool_execution_failed");
    assert_eq!(error.message(), "the local tool executor is unavailable");
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("tool_execution_failed"));
    assert_eq!(
        finishes[0].error_message(),
        Some("the local tool executor is unavailable"),
        "the runtime persists the port's safe error verbatim"
    );
    assert!(
        repository.tool_results.borrow().is_empty(),
        "the port owns the durable failed result row"
    );
}

#[test]
fn interruption_during_tool_execution_records_the_notice_and_continues() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
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
    let signal = RunCancellation::new();
    let (called_tx, called_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let port = GatedPort::new(called_tx, release_rx);
    let clock = ImmediateTime::new();
    let execution_signal = signal.clone();

    let execution = std::thread::spawn(move || {
        let outcome = run_ready(
            ModelRunExecutionService::new(
                &repository,
                &driver,
                &clock,
                &RecordingCommitObserver::new(),
                &port,
            )
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
    let outcome = outcome.expect("the interrupted run continues and completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .len(),
        1,
        "the in-flight tool ran exactly once"
    );
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Notice, intention_engine::INTERRUPT_NOTICE)],
        "the interruption commits its durable notice"
    );
    drop(messages);
    let requests = driver.requests();
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::Notice
            && message.content() == intention_engine::INTERRUPT_NOTICE
    }));
}

#[test]
fn provider_failure_after_tool_round_is_terminal_without_retry() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
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
        ToolResultOutcomeDto::completed("hello world").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("provider failure after a tool round terminalizes");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("a provider failure after a tool round fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_broken");
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        port.calls().len(),
        1,
        "a provider failure after a tool result never re-executes the tool"
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("provider_broken"));
}

#[test]
fn interruption_before_port_invocation_answers_the_call_with_a_partial_result() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("planning").expect("text is valid")),
            Ok(ModelEventDto::tool_call(call.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::completed("never used").expect("content is valid")
    )]);
    let signal = RunCancellation::new();
    // The interrupt lands on the assistant step commit, before the port
    // invocation of the round's call.
    repository
        .cancel_after_append
        .borrow_mut()
        .replace((1, signal.clone()));
    let clock = ImmediateTime::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &RecordingCommitObserver::new(),
            &port,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            signal,
        )),
    )
    .expect("the interrupted batch continues and completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert!(
        port.calls().is_empty(),
        "the interrupt never starts the tool effect"
    );
    let requests = driver.requests();
    let stopped_notice = intention_tools::partial_tool_result_content(true, None)
        .expect("the stopped-call notice renders");
    let answered = requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::Tool
            && message.tool_call_id() == Some(call.call_id())
            && message.content() == stopped_notice
    });
    assert!(
        answered,
        "the stopped call is answered with its partial notice"
    );
}

#[test]
fn tool_loop_with_commit_observer_executes_and_observes() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("before").expect("text is valid")),
            Ok(ModelEventDto::tool_call(call)),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("after").expect("text is valid")),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let port = ScriptedPort::new(vec![Ok(
        ToolResultOutcomeDto::completed("hello world").expect("content is valid")
    )]);
    let observer = RecordingCommitObserver::new();
    let clock = ImmediateTime::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(&repository, &driver, &clock, &observer, &port).execute(
            ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                RunCancellation::new(),
            ),
        ),
    )
    .expect("the tool loop with an observer completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    let commits = observer.commits();
    assert_eq!(
        commits
            .iter()
            .filter(|commit| matches!(commit, ModelRunCommitDto::Status { .. }))
            .count(),
        2,
        "the running and completed statuses are observed"
    );
    assert_eq!(
        commits
            .iter()
            .filter(|commit| matches!(commit, ModelRunCommitDto::Content(_)))
            .count(),
        2,
        "each committed assistant step is observed exactly once"
    );
    assert_eq!(
        commits.last(),
        Some(&ModelRunCommitDto::Status {
            session_id,
            run_id,
            status: RunStatusDto::Completed,
        })
    );
}

#[test]
fn second_tool_call_does_not_start_until_first_finishes() {
    // Tool calls from one provider round must execute strictly sequentially:
    // while the first call's port future is pending, the second call must not
    // be invoked. After the first finishes, both results enter the live
    // context in call order and the run completes.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
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
            Ok(ToolResultOutcomeDto::completed("one").expect("content is valid")),
            Ok(ToolResultOutcomeDto::completed("two").expect("content is valid")),
        ],
    ));
    let clock = ImmediateTime::new();
    let execution_port = std::sync::Arc::clone(&port);

    let execution = std::thread::spawn(move || {
        let outcome = run_ready(
            ModelRunExecutionService::new(
                &repository,
                &driver,
                &clock,
                &RecordingCommitObserver::new(),
                execution_port.as_ref(),
            )
            .execute(ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                RunCancellation::new(),
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

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        port.calls
            .lock()
            .expect("port call recorder is available")
            .as_slice(),
        &[(session_id, run_id, first), (session_id, run_id, second)]
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
}

#[test]
fn interruption_while_the_round_select_waits_records_a_notice_and_continues() {
    // The time port signals when the round's timeout sleep is first polled,
    // which proves the round's select is already suspended on the provider
    // stream. The interruption then must be observed by that select rather
    // than by a later pre-stream check, and the run continues.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = RunCancellation::new();
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
        let outcome = run_ready(
            ModelRunExecutionService::new(
                &repository,
                &driver,
                &clock,
                &RecordingCommitObserver::new(),
                &port,
            )
            .execute(ModelRunExecutionInputDto::new(
                session_id,
                run_id,
                request(run_id, "fixture"),
                config,
                execution_signal,
            )),
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
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Notice, intention_engine::INTERRUPT_NOTICE)]
    );
}

#[test]
fn interruption_during_the_retry_delay_starts_the_next_attempt() {
    // A retryable pre-tool provider failure schedules a retry and the run then
    // waits out the retry delay. An interruption inside that wait commits its
    // notice, clears the signal, and starts the second attempt immediately.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
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
    let signal = RunCancellation::new();
    let (entered_tx, entered_rx) = mpsc::channel();
    let clock = RetryDelayTime {
        entered: entered_tx,
    };
    let execution_signal = signal.clone();

    let execution = std::thread::spawn(move || {
        let outcome = run_ready(
            ModelRunExecutionService::new(
                &repository,
                &driver,
                &clock,
                &RecordingCommitObserver::new(),
                &port,
            )
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
        driver.executions(),
        2,
        "the interrupted wait starts the second attempt"
    );
    assert!(port.calls().is_empty());
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Notice, intention_engine::INTERRUPT_NOTICE)]
    );
}

#[test]
fn interruption_signalled_before_the_retry_wait_still_starts_the_next_attempt() {
    // The provider error cancels the run while the stream yields it, so the
    // signal is already set when the retry wait would begin. The wait commits
    // its notice without arming the sub-second retry delay at all.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
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
    let signal = RunCancellation::new();
    driver.cancel_during_stream(0, signal.model_signal());
    let clock = ImmediateTime::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &RecordingCommitObserver::new(),
            &port,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            signal,
        )),
    )
    .expect("interruption before the retry wait continues the run");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        driver.executions(),
        2,
        "the interrupted retry still starts the second attempt"
    );
    assert!(
        clock
            .sleeps
            .borrow()
            .iter()
            .all(|duration| *duration >= Duration::from_secs(1)),
        "an already-interrupted run never arms the sub-second retry delay"
    );
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Notice, intention_engine::INTERRUPT_NOTICE)]
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
    let config = fixture_snapshot_with_model("fixture");
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
        ToolResultOutcomeDto::completed("hello").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("a finished round with tool calls continues the loop");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        driver.executions(),
        2,
        "the finished-with-calls round drives the continuation round"
    );
    assert_eq!(
        port.calls().as_slice(),
        &[(session_id, run_id, call.clone())]
    );
    let reasoning = AssistantReasoningDto::new(vec![call.call_id()], "plan ")
        .expect("fixture reasoning is valid");
    let requests = driver.requests();
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
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
}

#[test]
fn pending_messages_join_the_live_context_at_a_tool_batch_boundary() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let call = ToolCallDto::new(ToolCallId::new(), "read", "{}").expect("call is valid");
    repository
        .pending
        .borrow_mut()
        .push_back(pending_user_message(session_id, run_id, "pending message"));
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
        ToolResultOutcomeDto::completed("tool output").expect("content is valid")
    )]);

    let outcome = execute(
        &repository,
        &driver,
        &port,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("the run completes after joining the pending message");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        driver.executions(),
        2,
        "the pending message continues the same run"
    );
    let requests = driver.requests();
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::User && message.content() == "pending message"
    }));
    drop(requests);
    assert_eq!(*repository.pending_consumes.borrow(), 2);
    assert!(repository.pending.borrow().is_empty());
}

#[test]
fn a_pending_message_at_the_finish_boundary_continues_instead_of_completing() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    repository
        .pending
        .borrow_mut()
        .push_back(pending_user_message(
            session_id,
            run_id,
            "arrived before completion",
        ));
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
        RunCancellation::new(),
    )
    .expect("the run continues with the joined message and then completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        driver.executions(),
        2,
        "the finished round is followed by another model step"
    );
    let requests = driver.requests();
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::User && message.content() == "arrived before completion"
    }));
    drop(requests);
    let finishes = repository.finishes.borrow();
    assert_eq!(
        finishes.len(),
        1,
        "the run completes exactly once after the joined step"
    );
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
}

fn pending_user_message(
    session_id: SessionId,
    run_id: RunId,
    content: &str,
) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        Some(run_id),
        MessageKindDto::User,
        content,
        None,
        None,
        None,
    )
    .expect("fixture pending message is valid")
}
