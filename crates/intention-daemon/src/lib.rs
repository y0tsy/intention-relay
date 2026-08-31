//! Thin daemon process host for the local protocol facade.
//!
//! The daemon owns the local listener and typed connection hosting. It delegates
//! health, query, command, and replay-only subscription meaning to the durable
//! composition facade.

#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use intention::DaemonApplicationFacade;
use intention_domain::{RunEventCursorDto, RunFailureDto, RunStatusDto, ToolResultOutcomeDto};
use intention_model::ModelCancellationSignal;
use intention_protocol::{
    ProtocolAcceptedDto, ProtocolCapabilityDto, ProtocolCommandDto, ProtocolCommandResultDto,
    ProtocolDaemonFrameDto, ProtocolHelloDto, ProtocolMessageDto, ProtocolRequestPayloadDto,
    ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto, RunLiveBatchDto, RunResyncDto,
    RunResyncReasonDto, RunSnapshotFrameDto, RunStreamFrameDto, RunSubscriptionResponseDto,
};
#[cfg(any(test, feature = "test-support"))]
use intention_runtime::ModelRunFirstAppendGate;
use intention_runtime::{
    ModelRunCommitDto, ModelRunCommitObserver, ModelSleepFuture, ModelTimePort,
};
use intention_tools::{
    EditInput, ExecuteInput, GlobInput, GrepInput, ReadInput, ToolInput, ToolProjectedContent,
    ToolResult, WriteInput,
};
#[cfg(test)]
use intention_transport::LocalListener;
use intention_transport::{
    AsyncDaemonConnectionRoles, AsyncDaemonFrameSender, AsyncLocalListener, AsyncRequestReceiver,
    LocalEndpoint, local_protocol_version,
};
#[cfg(any(test, feature = "test-support"))]
use intention_transport::{LocalConnection, negotiate_daemon};
use intention_types::{
    CorrelationIdDto, DtoResult, ErrorDto, RunId, SessionId, TimestampDto, ToolCallDto,
};
#[cfg(test)]
use std::thread;

const SUBSCRIBER_QUEUE_CAPACITY: usize = 64;
const SUBSCRIBER_WRITE_DEADLINE: Duration = Duration::from_secs(10);
const CANCELLATION_TERMINALIZER_RETRY_DELAY: Duration = Duration::from_millis(25);

type RunKey = (SessionId, RunId);

struct TokioTime;

impl ModelTimePort for TokioTime {
    fn now(&self) -> TimestampDto {
        unix_timestamp().unwrap_or_else(|_| {
            TimestampDto::from_unix_seconds(0)
                .unwrap_or_else(|_| unreachable!("zero timestamp is valid"))
        })
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        Box::pin(tokio::time::sleep(duration))
    }
}

struct Subscriber {
    id: u64,
    sender: tokio::sync::mpsc::Sender<ProtocolDaemonFrameDto>,
    close: tokio::sync::watch::Sender<bool>,
}

#[derive(Clone, Copy)]
struct PublishedRun {
    cursor: RunEventCursorDto,
    status: RunStatusDto,
}

struct HostData {
    tasks: HashMap<RunKey, ModelCancellationSignal>,
    #[cfg(any(test, feature = "test-support"))]
    execution_tasks: Vec<tokio::task::JoinHandle<()>>,
    #[cfg(any(test, feature = "test-support"))]
    execution_completion: HashMap<RunKey, tokio::sync::watch::Receiver<bool>>,
    subscribers: HashMap<RunKey, Vec<Subscriber>>,
    published: HashMap<RunKey, PublishedRun>,
    next_subscriber_id: u64,
}

impl Default for HostData {
    fn default() -> Self {
        Self {
            tasks: HashMap::new(),
            #[cfg(any(test, feature = "test-support"))]
            execution_tasks: Vec::new(),
            #[cfg(any(test, feature = "test-support"))]
            execution_completion: HashMap::new(),
            subscribers: HashMap::new(),
            published: HashMap::new(),
            next_subscriber_id: 1,
        }
    }
}

struct HostState {
    facade: DaemonApplicationFacade,
    data: Mutex<HostData>,
    publication_gate: Mutex<()>,
    #[cfg(any(test, feature = "test-support"))]
    first_append_gate: Option<Arc<dyn ModelRunFirstAppendGate>>,
    #[cfg(any(test, feature = "test-support"))]
    terminalizer_failures: AtomicUsize,
    #[cfg(any(test, feature = "test-support"))]
    terminalizer_attempts: AtomicUsize,
    #[cfg(any(test, feature = "test-support"))]
    terminalizer_retry_paused: AtomicBool,
    #[cfg(any(test, feature = "test-support"))]
    terminalizer_failure_entered: tokio::sync::Notify,
    #[cfg(any(test, feature = "test-support"))]
    terminalizer_failure_release: tokio::sync::Notify,
    #[cfg(any(test, feature = "test-support"))]
    terminalizer_completed: tokio::sync::Notify,
    #[cfg(any(test, feature = "test-support"))]
    task_completed: tokio::sync::Notify,
}

#[cfg(test)]
fn host_for_test(facade: DaemonApplicationFacade) -> Arc<HostState> {
    Arc::new(HostState {
        facade,
        data: Mutex::new(HostData::default()),
        publication_gate: Mutex::new(()),
        #[cfg(any(test, feature = "test-support"))]
        first_append_gate: None,
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failures: AtomicUsize::new(0),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_attempts: AtomicUsize::new(0),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_retry_paused: AtomicBool::new(false),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failure_entered: tokio::sync::Notify::new(),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failure_release: tokio::sync::Notify::new(),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_completed: tokio::sync::Notify::new(),
        #[cfg(any(test, feature = "test-support"))]
        task_completed: tokio::sync::Notify::new(),
    })
}

impl HostState {
    fn schedule_if_starting(self: &Arc<Self>, session_id: SessionId, run_id: RunId) {
        let key = (session_id, run_id);
        // Admission and StopRun share this registry lock. Once admission begins,
        // it either registers an executor before StopRun can persist Cancelling,
        // or StopRun installs its own terminalization task before a later
        // admission can observe the no-longer-Starting run.
        let mut data = match self.data.lock() {
            Ok(data) => data,
            Err(_) => return,
        };
        if data.tasks.contains_key(&key) {
            return;
        }
        let schedule = match self
            .facade
            .schedule_starting_run_for_daemon(session_id, run_id)
        {
            Ok(schedule) => schedule,
            Err(_) => {
                drop(data);
                self.fail_unadmitted_starting_run(session_id, run_id);
                return;
            }
        };
        let cancellation = ModelCancellationSignal::new();
        let std::collections::hash_map::Entry::Vacant(entry) = data.tasks.entry(key) else {
            return;
        };
        entry.insert(cancellation.clone());
        #[cfg(any(test, feature = "test-support"))]
        let (execution_completion, completion) = tokio::sync::watch::channel(false);
        #[cfg(any(test, feature = "test-support"))]
        data.execution_completion.insert(key, completion);
        drop(data);
        let host = Arc::clone(self);
        let task = tokio::spawn(async move {
            let observer = HostCommitObserver {
                host: Arc::clone(&host),
            };
            let executor = DaemonToolExecutor::new(host.facade.clone());
            #[cfg(any(test, feature = "test-support"))]
            let result = if let Some(first_append_gate) = host.first_append_gate.as_deref() {
                host.facade
                    .execute_scheduled_model_run_for_daemon_with_first_append_gate(
                        schedule.clone(),
                        cancellation.clone(),
                        &TokioTime,
                        &observer,
                        first_append_gate,
                    )
                    .await
            } else {
                host.facade
                    .execute_scheduled_model_run_for_daemon_with_tool_executor(
                        schedule.clone(),
                        cancellation.clone(),
                        &TokioTime,
                        &observer,
                        &executor,
                    )
                    .await
            };
            #[cfg(not(any(test, feature = "test-support")))]
            let result = host
                .facade
                .execute_scheduled_model_run_for_daemon_with_tool_executor(
                    schedule.clone(),
                    cancellation.clone(),
                    &TokioTime,
                    &observer,
                    &executor,
                )
                .await;
            // An executor error cannot leave a StopRun's durable Cancelling
            // state stranded. Re-entering the task-owned executor against the
            // exact current scope performs only its cancellation terminal
            // transition; it cannot append a late initial fact.
            if result.is_err()
                && host
                    .facade
                    .load_current_run_replay_for_daemon(key.0, key.1)
                    .is_ok_and(|replay| {
                        replay.snapshot().run_projection().status() == RunStatusDto::Cancelling
                    })
            {
                let _ = host
                    .facade
                    .execute_scheduled_model_run_for_daemon(
                        schedule,
                        cancellation,
                        &TokioTime,
                        &observer,
                    )
                    .await;
            }
            if let Ok(mut data) = host.data.lock() {
                data.tasks.remove(&key);
            }
            #[cfg(any(test, feature = "test-support"))]
            {
                execution_completion.send_replace(true);
                host.task_completed.notify_one();
            }
        });
        #[cfg(any(test, feature = "test-support"))]
        self.track_test_execution_task(task);
        #[cfg(not(any(test, feature = "test-support")))]
        std::mem::drop(task);
    }

    fn stop_run(
        self: &Arc<Self>,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<intention_protocol::ProtocolAcceptedResultDto> {
        let key = (session_id, run_id);
        // Keep the registry locked through the durable transition so an
        // unregistered executor cannot observe Starting, lose this StopRun,
        // and leave the durable run stranded in Cancelling.
        let mut data = self.data.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_task_registry_unavailable",
                "the daemon task registry is unavailable",
            )
        })?;
        let cancellation = data.tasks.get(&key).cloned();
        let accepted = self.facade.stop_run_for_daemon_host(session_id, run_id)?;
        let terminalize = cancellation.is_none();
        let cancellation = cancellation.unwrap_or_else(|| {
            let cancellation = ModelCancellationSignal::new();
            data.tasks.insert(key, cancellation.clone());
            cancellation
        });
        drop(data);
        self.publish_current(session_id, run_id);
        cancellation.cancel();
        if terminalize {
            self.spawn_cancellation_terminalizer(key);
        }
        Ok(accepted)
    }

    fn spawn_cancellation_terminalizer(self: &Arc<Self>, key: RunKey) {
        let host = Arc::clone(self);
        let task = tokio::spawn(async move {
            let mut retry_immediately = true;
            loop {
                let result = host.terminalize_cancelling_run(key);
                host.publish_current(key.0, key.1);
                // The registry entry remains task-owned until an independent
                // durable reread proves this exact run is terminal. A failed
                // SQLite completion therefore cannot strand Cancelling after
                // the terminalizer has relinquished its only ownership.
                if host.cancellation_terminalizer_is_terminal(key) {
                    if let Ok(mut data) = host.data.lock() {
                        data.tasks.remove(&key);
                    }
                    #[cfg(any(test, feature = "test-support"))]
                    {
                        host.terminalizer_completed.notify_one();
                        host.task_completed.notify_one();
                    }
                    return;
                }
                if result.is_err() {
                    host.wait_for_injected_terminalizer_retry().await;
                }
                // Permit one immediate retry for a transient one-shot failure,
                // then rate-limit every later retry. This keeps the exact task
                // alive without an unbounded busy loop or a state conversion.
                if retry_immediately {
                    retry_immediately = false;
                    tokio::task::yield_now().await;
                } else {
                    tokio::time::sleep(CANCELLATION_TERMINALIZER_RETRY_DELAY).await;
                }
            }
        });
        #[cfg(any(test, feature = "test-support"))]
        self.track_test_execution_task(task);
        #[cfg(not(any(test, feature = "test-support")))]
        std::mem::drop(task);
    }

    fn terminalize_cancelling_run(&self, key: RunKey) -> DtoResult<()> {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.terminalizer_attempts.fetch_add(1, Ordering::Relaxed);
            if self
                .terminalizer_failures
                .try_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                self.terminalizer_retry_paused
                    .store(true, Ordering::Release);
                self.terminalizer_failure_entered.notify_one();
                return Err(ErrorDto::unavailable(
                    "injected_terminalizer_failure",
                    "a deterministic terminalizer failure was injected",
                ));
            }
        }
        self.facade
            .terminalize_cancelling_run_for_daemon(key.0, key.1)
    }

    fn cancellation_terminalizer_is_terminal(&self, key: RunKey) -> bool {
        self.facade
            .load_current_run_replay_for_daemon(key.0, key.1)
            .is_ok_and(|replay| replay.snapshot().run_projection().status().is_terminal())
    }

    async fn wait_for_injected_terminalizer_retry(&self) {
        #[cfg(any(test, feature = "test-support"))]
        if self.terminalizer_retry_paused.swap(false, Ordering::AcqRel) {
            self.terminalizer_failure_release.notified().await;
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn track_test_execution_task(&self, task: tokio::task::JoinHandle<()>) {
        if let Ok(mut data) = self.data.lock() {
            data.execution_tasks.push(task);
        } else {
            task.abort();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn abort_test_execution_tasks(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let Ok(mut data) = self.data.lock() else {
            return Vec::new();
        };
        std::mem::take(&mut data.execution_tasks)
    }

    #[cfg(any(test, feature = "test-support"))]
    fn inject_terminalizer_failure_once(&self) {
        self.terminalizer_failures.store(1, Ordering::Release);
    }

    #[cfg(any(test, feature = "test-support"))]
    fn terminalizer_attempts(&self) -> usize {
        self.terminalizer_attempts.load(Ordering::Acquire)
    }

    #[cfg(any(test, feature = "test-support"))]
    async fn wait_for_terminalizer_failure(&self) {
        self.terminalizer_failure_entered.notified().await;
    }

    #[cfg(any(test, feature = "test-support"))]
    fn release_terminalizer_retry(&self) {
        self.terminalizer_failure_release.notify_one();
    }

    #[cfg(any(test, feature = "test-support"))]
    async fn wait_for_terminalizer_completion(&self) {
        self.terminalizer_completed.notified().await;
    }

    #[cfg(any(test, feature = "test-support"))]
    async fn wait_for_task_cleanup(&self) {
        loop {
            if self.data.lock().is_ok_and(|data| data.tasks.is_empty()) {
                return;
            }
            self.task_completed.notified().await;
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    async fn wait_for_execution_completion(&self, key: RunKey) -> bool {
        let Some(mut completion) = self
            .data
            .lock()
            .ok()
            .and_then(|data| data.execution_completion.get(&key).cloned())
        else {
            return false;
        };
        loop {
            if *completion.borrow_and_update() {
                return true;
            }
            if completion.changed().await.is_err() {
                return false;
            }
        }
    }

    fn fail_unadmitted_starting_run(&self, session_id: SessionId, run_id: RunId) {
        if self
            .facade
            .fail_starting_run_for_daemon(session_id, run_id, "model_scheduling_unavailable")
            .is_ok()
        {
            self.publish_current(session_id, run_id);
        }
    }

    fn publish_current(&self, session_id: SessionId, run_id: RunId) {
        let Ok(_publication_gate) = self.publication_gate.lock() else {
            return;
        };
        let replay = match self
            .facade
            .load_current_run_replay_for_daemon(session_id, run_id)
        {
            Ok(replay) => replay,
            Err(_) => return,
        };
        let key = (session_id, run_id);
        let snapshot = replay.snapshot().clone();
        let current = PublishedRun {
            cursor: snapshot.cursor(),
            status: snapshot.run_projection().status(),
        };
        let previous = self
            .data
            .lock()
            .ok()
            .and_then(|data| data.published.get(&key).copied());
        let mut after = previous.map_or(RunEventCursorDto::new(0), |value| value.cursor);
        while after < current.cursor {
            let Ok(tail) = self
                .facade
                .load_run_tail_for_daemon(session_id, run_id, after)
            else {
                return;
            };
            if tail.facts().is_empty() {
                return;
            }
            let next_after = tail.next_after_cursor();
            let Ok(batch) = RunLiveBatchDto::new(
                session_id,
                run_id,
                tail.after_cursor(),
                tail.facts().to_vec(),
                next_after,
            ) else {
                return;
            };
            self.broadcast(
                key,
                ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::LiveBatch(batch)),
            );
            if next_after <= after {
                return;
            }
            after = next_after;
        }
        if previous.is_none_or(|value| value.status != current.status) {
            self.broadcast(
                key,
                ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Snapshot(
                    RunSnapshotFrameDto::new(snapshot),
                )),
            );
        }
        if let Ok(mut data) = self.data.lock() {
            data.published.insert(key, current);
        }
    }

    fn broadcast(&self, key: RunKey, frame: ProtocolDaemonFrameDto) {
        let mut slow = Vec::new();
        if let Ok(data) = self.data.lock()
            && let Some(subscribers) = data.subscribers.get(&key)
        {
            for subscriber in subscribers {
                if subscriber.sender.try_send(frame.clone()).is_err() {
                    slow.push(subscriber.id);
                }
            }
        }
        for id in slow {
            self.send_slow_resync(key, id);
            self.remove_subscriber(key, id);
        }
    }

    fn send_slow_resync(&self, key: RunKey, id: u64) {
        if let Ok(data) = self.data.lock()
            && let Some(subscribers) = data.subscribers.get(&key)
            && let Some(subscriber) = subscribers.iter().find(|subscriber| subscriber.id == id)
        {
            let _ = subscriber
                .sender
                .try_send(ProtocolDaemonFrameDto::RunStream(
                    RunStreamFrameDto::Resync(RunResyncDto::new(
                        key.0,
                        key.1,
                        RunResyncReasonDto::SubscriberTooSlow,
                    )),
                ));
            subscriber.close.send_replace(true);
        }
    }

    fn remove_subscriber(&self, key: RunKey, id: u64) {
        if let Ok(mut data) = self.data.lock()
            && let Some(subscribers) = data.subscribers.get_mut(&key)
        {
            subscribers.retain(|subscriber| subscriber.id != id);
        }
    }

    fn register_subscriber(
        self: &Arc<Self>,
        session_id: SessionId,
        run_id: RunId,
        after_cursor: Option<RunEventCursorDto>,
        sender: tokio::sync::mpsc::Sender<ProtocolDaemonFrameDto>,
        close: tokio::sync::watch::Sender<bool>,
        correlation_id: CorrelationIdDto,
    ) -> Option<u64> {
        let key = (session_id, run_id);
        let Ok(_publication_gate) = self.publication_gate.lock() else {
            let _ = sender.try_send(run_subscription_response(
                correlation_id,
                RunSubscriptionResponseDto::Error(ErrorDto::unavailable(
                    "daemon_subscriber_unavailable",
                    "the daemon subscriber is unavailable",
                )),
            ));
            return None;
        };
        let mut data = match self.data.lock() {
            Ok(data) => data,
            Err(_) => {
                let _ = sender.try_send(run_subscription_response(
                    correlation_id,
                    RunSubscriptionResponseDto::Error(ErrorDto::unavailable(
                        "daemon_subscriber_unavailable",
                        "the daemon subscriber is unavailable",
                    )),
                ));
                return None;
            }
        };
        let replay = match self
            .facade
            .load_current_run_replay_for_daemon(session_id, run_id)
        {
            Ok(replay) => replay,
            Err(error) if error.code() == "run_replay_not_found" => {
                let _ = sender.try_send(run_subscription_response(
                    correlation_id,
                    RunSubscriptionResponseDto::Error(error),
                ));
                return None;
            }
            Err(_) => {
                let _ = sender.try_send(run_subscription_response(
                    correlation_id,
                    RunSubscriptionResponseDto::Resync(RunResyncDto::new(
                        session_id,
                        run_id,
                        RunResyncReasonDto::HistoryUnavailable,
                    )),
                ));
                return None;
            }
        };
        if after_cursor.is_some_and(|cursor| cursor > replay.snapshot().cursor()) {
            let _ = sender.try_send(run_subscription_response(
                correlation_id,
                RunSubscriptionResponseDto::Resync(RunResyncDto::new(
                    session_id,
                    run_id,
                    RunResyncReasonDto::InvalidCursor,
                )),
            ));
            return None;
        }
        let id = data.next_subscriber_id;
        data.next_subscriber_id = data.next_subscriber_id.saturating_add(1);
        data.subscribers.entry(key).or_default().push(Subscriber {
            id,
            sender: sender.clone(),
            close,
        });
        drop(data);
        // The subscriber is registered before this second durable read. The
        // serialized publisher cannot place a later live frame before this
        // response enters this subscriber's FIFO queue.
        let response = match self
            .facade
            .load_current_run_replay_for_daemon(session_id, run_id)
        {
            Ok(replay) => RunSubscriptionResponseDto::Replay(replay),
            Err(error) if error.code() == "run_replay_not_found" => {
                RunSubscriptionResponseDto::Error(error)
            }
            Err(_) => RunSubscriptionResponseDto::Resync(RunResyncDto::new(
                session_id,
                run_id,
                RunResyncReasonDto::HistoryUnavailable,
            )),
        };
        if sender
            .try_send(run_subscription_response(correlation_id, response))
            .is_err()
        {
            self.remove_subscriber(key, id);
            return None;
        }
        Some(id)
    }
}

const fn run_subscription_response(
    correlation_id: CorrelationIdDto,
    response: RunSubscriptionResponseDto,
) -> ProtocolDaemonFrameDto {
    ProtocolDaemonFrameDto::Response(ProtocolResponseEnvelopeDto::new(
        local_protocol_version(),
        correlation_id,
        ProtocolMessageDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            ProtocolResponsePayloadDto::RunSubscription(response),
        ),
    ))
}

struct HostCommitObserver {
    host: Arc<HostState>,
}

impl ModelRunCommitObserver for HostCommitObserver {
    fn observe_model_run_commit(&self, committed: ModelRunCommitDto) {
        self.host
            .publish_current(committed.session_id(), committed.run_id());
        if committed.snapshot().run_projection().status().is_terminal()
            && let Ok(Some(promoted)) = self
                .host
                .facade
                .current_starting_run_for_daemon(committed.session_id())
        {
            self.host
                .schedule_if_starting(committed.session_id(), promoted);
        }
    }
}

/// Executes provider-normalized tool calls through the durable daemon-owned tool path.
///
/// Each call is decoded into the typed daemon tool input and executed through
/// the facade's durable local-tool lifecycle, which publishes only independently
/// reread committed evidence. The blocking tool effect runs on a spawned worker
/// so the async execution loop is never stalled.
#[doc(hidden)]
#[derive(Clone)]
pub struct DaemonToolExecutor {
    facade: DaemonApplicationFacade,
}

impl DaemonToolExecutor {
    /// Binds one durable facade to the daemon tool-execution path.
    #[must_use]
    pub const fn new(facade: DaemonApplicationFacade) -> Self {
        Self { facade }
    }
}

impl intention_runtime::ToolExecutionPort for DaemonToolExecutor {
    fn execute_tool(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call: ToolCallDto,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = DtoResult<ToolResultOutcomeDto>> + Send + '_>,
    > {
        let facade = self.facade.clone();
        Box::pin(async move {
            let tool_id = call.name().to_owned();
            let call_id = call.call_id();
            let arguments = call.arguments_json().to_owned();
            let input = parse_tool_input(&tool_id, &arguments)?;
            let result = tokio::task::spawn_blocking(move || {
                let workspace = facade.resolve_workspace_root_for_daemon(session_id)?;
                facade.invoke_local_tool_for_daemon(
                    session_id, run_id, call_id, tool_id, input, workspace,
                )
            })
            .await
            .map_err(|_| {
                ErrorDto::unavailable(
                    "tool_execution_task_failed",
                    "the tool execution task failed",
                )
            })?;
            // Tool-level failures are typed outcomes, not port errors; only a
            // lost execution task is a port-level infrastructure failure.
            match result {
                Ok(result) => normalize_tool_result(result),
                Err(error) => Ok(ToolResultOutcomeDto::failed(RunFailureDto::new(
                    error.code(),
                    error.retry(),
                    error.correlation_id(),
                )?)),
            }
        })
    }
}

/// Decodes provider-normalized tool arguments into the typed daemon tool input.
///
/// # Errors
///
/// Returns a validation error for an unknown tool id or arguments that are not
/// valid typed input for that tool.
fn parse_tool_input(tool_id: &str, arguments_json: &str) -> DtoResult<ToolInput> {
    let input = match tool_id {
        "read" => serde_json::from_str::<ReadInput>(arguments_json).map(ToolInput::Read),
        "write" => serde_json::from_str::<WriteInput>(arguments_json).map(ToolInput::Write),
        "edit" => serde_json::from_str::<EditInput>(arguments_json).map(ToolInput::Edit),
        "execute" => serde_json::from_str::<ExecuteInput>(arguments_json).map(ToolInput::Execute),
        "glob" => serde_json::from_str::<GlobInput>(arguments_json).map(ToolInput::Glob),
        "grep" => serde_json::from_str::<GrepInput>(arguments_json).map(ToolInput::Grep),
        _ => {
            return Err(ErrorDto::validation(
                "unknown_tool",
                "tool is not supported by the daemon",
            ));
        }
    };
    input.map_err(|_| {
        ErrorDto::validation(
            "invalid_tool_input_json",
            "tool arguments are not valid typed input",
        )
    })
}

/// Normalizes one typed tool result into bounded durable outcome content.
///
/// The projection is redacted and workspace-relative by construction, and
/// `ToolResultOutcomeDto::succeeded` keeps the durable outcome within its own
/// content bound.
fn normalize_tool_result(result: ToolResult) -> DtoResult<ToolResultOutcomeDto> {
    let content = match result.projection().content {
        ToolProjectedContent::Text { text, truncated } => {
            if truncated {
                format!("{}\n[truncated]", text.as_str())
            } else {
                text.as_str().to_owned()
            }
        }
        ToolProjectedContent::Paths { paths, .. } => {
            serde_json::to_string(&paths).map_err(|_| {
                ErrorDto::validation(
                    "invalid_tool_result_content",
                    "tool result content could not be normalized",
                )
            })?
        }
        ToolProjectedContent::Matches { matches, .. } => {
            serde_json::to_string(&matches).map_err(|_| {
                ErrorDto::validation(
                    "invalid_tool_result_content",
                    "tool result content could not be normalized",
                )
            })?
        }
        ToolProjectedContent::Mutation { bytes } => format!("{bytes} bytes"),
    };
    ToolResultOutcomeDto::succeeded(content)
}

/// Runs the local daemon host until its process is terminated.
///
/// Production startup loads and validates the platform-standard TOML configuration,
/// creates a new credential-free snapshot for this daemon epoch, opens AppData
/// SQLite storage, and completes recovery before the host begins accepting peers.
///
/// # Errors
///
/// Returns a safe typed error if configuration, durable startup, or endpoint
/// binding cannot complete. Per-connection failures are isolated to that
/// connection so a malformed or disconnected client cannot stop the host.
pub fn run(endpoint: LocalEndpoint) -> DtoResult<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| {
            ErrorDto::unavailable(
                "daemon_runtime_unavailable",
                "the daemon runtime is unavailable",
            )
        })?;
    // The async listener binds inside the runtime: interprocess requires a
    // Tokio reactor context when it wraps the Unix socket listener.
    let listener = runtime.block_on(async { AsyncLocalListener::bind(endpoint) })?;
    let facade = DaemonApplicationFacade::open_platform()?;
    runtime.block_on(serve_async_listener(listener, facade))
}

async fn serve_async_listener(
    listener: AsyncLocalListener,
    facade: DaemonApplicationFacade,
) -> DtoResult<()> {
    let host = Arc::new(HostState {
        facade,
        data: Mutex::new(HostData::default()),
        publication_gate: Mutex::new(()),
        #[cfg(any(test, feature = "test-support"))]
        first_append_gate: None,
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failures: AtomicUsize::new(0),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_attempts: AtomicUsize::new(0),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_retry_paused: AtomicBool::new(false),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failure_entered: tokio::sync::Notify::new(),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failure_release: tokio::sync::Notify::new(),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_completed: tokio::sync::Notify::new(),
        #[cfg(any(test, feature = "test-support"))]
        task_completed: tokio::sync::Notify::new(),
    });
    loop {
        let connection = listener.accept().await?;
        let host = Arc::clone(&host);
        tokio::spawn(async move {
            serve_async_connection(connection, host).await;
        });
    }
}

async fn serve_async_connection(
    connection: intention_transport::AsyncLocalDaemonConnection,
    host: Arc<HostState>,
) {
    let hello = match daemon_hello() {
        Ok(hello) => hello,
        Err(_) => return,
    };
    let (_, roles) = match connection.negotiate_by_capability(hello).await {
        Ok(roles) => roles,
        Err(_) => return,
    };
    match roles {
        AsyncDaemonConnectionRoles::Ordinary(requests, responses) => {
            serve_async_ordinary(requests, responses, host).await;
        }
        AsyncDaemonConnectionRoles::RunStream(requests, frames) => {
            serve_async_run_stream(requests, frames, host).await;
        }
    }
}

async fn serve_async_ordinary(
    mut requests: AsyncRequestReceiver,
    mut responses: intention_transport::AsyncResponseSender,
    host: Arc<HostState>,
) {
    while let Ok(request) = requests.receive().await {
        let payload = match request.message().payload() {
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(
                subscription,
            )) => ProtocolResponsePayloadDto::Subscription(host.facade.subscribe(*subscription)),
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::StopRun(command)) => {
                let result = host
                    .stop_run(command.session_id(), command.run_id())
                    .map(|result| {
                        ProtocolCommandResultDto::Accepted(ProtocolAcceptedDto::with_result(
                            CorrelationIdDto::new(),
                            result,
                        ))
                    })
                    .unwrap_or_else(ProtocolCommandResultDto::Rejected);
                ProtocolResponsePayloadDto::CommandResult(result)
            }
            ProtocolRequestPayloadDto::Command(command) => {
                let result = host.facade.command(command.clone());
                if let ProtocolCommandDto::SendUserTurn(_) = command
                    && let ProtocolCommandResultDto::Accepted(accepted) = &result
                    && let Some(intention_protocol::ProtocolAcceptedResultDto::SendUserTurn(turn)) =
                        accepted.result()
                    && let intention_protocol::SendUserTurnOutcomeDto::Started { run_id, .. } =
                        turn.outcome()
                {
                    host.schedule_if_starting(turn.session_id(), run_id);
                }
                ProtocolResponsePayloadDto::CommandResult(result)
            }
            ProtocolRequestPayloadDto::Query(query) => {
                ProtocolResponsePayloadDto::QueryResult(host.facade.query(*query))
            }
        };
        let response = ProtocolResponseEnvelopeDto::new(
            local_protocol_version(),
            request.correlation_id(),
            ProtocolMessageDto::new(intention_protocol::CURRENT_DTO_SCHEMA_VERSION, payload),
        );
        if responses.send(&response).await.is_err() {
            return;
        }
    }
}

async fn serve_async_run_stream(
    mut requests: AsyncRequestReceiver,
    mut frames: AsyncDaemonFrameSender,
    host: Arc<HostState>,
) {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
    let (close_sender, mut close_receiver) = tokio::sync::watch::channel(false);
    let mut registered: Option<(RunKey, u64)> = None;
    loop {
        tokio::select! {
            changed = close_receiver.changed() => {
                if changed.is_err() || *close_receiver.borrow() {
                    if let Some((key, id)) = registered {
                        host.remove_subscriber(key, id);
                    }
                    return;
                }
            }
            request = requests.receive_run_subscription() => {
                let Ok(request) = request else {
                    if let Some((key, id)) = registered {
                        host.remove_subscriber(key, id);
                    }
                    return;
                };
                let command = request.message().payload();
                if let Some((key, id)) = registered.take() {
                    host.remove_subscriber(key, id);
                }
                let subscriber_id = host.register_subscriber(
                    command.session_id(),
                    command.run_id(),
                    command.after_cursor(),
                    sender.clone(),
                    close_sender.clone(),
                    request.correlation_id(),
                );
                if let Some(subscriber_id) = subscriber_id {
                    registered = Some(((command.session_id(), command.run_id()), subscriber_id));
                }
            }
            frame = receiver.recv() => {
                let Some(frame) = frame else { return; };
                if write_frame_with_deadline(&mut frames, frame).await.is_err() {
                    // A queued resync frame is best effort: a timed-out OS write
                    // cannot be recovered, but it is never allowed to stall
                    // persistence or any other subscriber.
                    if let Some((key, id)) = registered {
                        host.remove_subscriber(key, id);
                    }
                    return;
                }
            }
        }
    }
}

async fn write_frame_with_deadline(
    sender: &mut AsyncDaemonFrameSender,
    frame: ProtocolDaemonFrameDto,
) -> DtoResult<()> {
    write_with_deadline(sender.send(&frame)).await
}

async fn write_with_deadline<T>(
    write: impl std::future::Future<Output = DtoResult<T>>,
) -> DtoResult<T> {
    tokio::time::timeout(SUBSCRIBER_WRITE_DEADLINE, write)
        .await
        .map_err(|_| {
            ErrorDto::unavailable(
                "subscriber_write_timed_out",
                "the stream subscriber is too slow",
            )
        })?
}

#[cfg(test)]
mod deadline_tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "The paused-clock deadline fixture uses direct assertions for exact diagnostics."
    )]

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn subscriber_write_deadline_is_exactly_ten_seconds() {
        let write = write_with_deadline(std::future::pending::<DtoResult<()>>());
        tokio::pin!(write);
        tokio::select! {
            result = &mut write => panic!("pending write completed: {result:?}"),
            () = tokio::task::yield_now() => {}
        }
        tokio::time::advance(Duration::from_secs(9)).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(1), &mut write)
                .await
                .is_err()
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(
            write.await.expect_err("ten-second deadline expires").code(),
            "subscriber_write_timed_out"
        );
    }
}

fn daemon_hello() -> DtoResult<ProtocolHelloDto> {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
            ProtocolCapabilityDto::RunStreamSubscriptions,
        ],
        "intention-daemon",
    )
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn serve_test_connection(connection: LocalConnection, facade: DaemonApplicationFacade) {
    serve_connection(connection, facade);
}

/// Serves one injected asynchronous connection for a bounded integration fixture.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub async fn serve_test_async_connection(
    connection: intention_transport::AsyncLocalDaemonConnection,
    facade: DaemonApplicationFacade,
) {
    let host = Arc::new(HostState {
        facade,
        data: Mutex::new(HostData::default()),
        publication_gate: Mutex::new(()),
        #[cfg(any(test, feature = "test-support"))]
        first_append_gate: None,
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failures: AtomicUsize::new(0),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_attempts: AtomicUsize::new(0),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_retry_paused: AtomicBool::new(false),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failure_entered: tokio::sync::Notify::new(),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_failure_release: tokio::sync::Notify::new(),
        #[cfg(any(test, feature = "test-support"))]
        terminalizer_completed: tokio::sync::Notify::new(),
        task_completed: tokio::sync::Notify::new(),
    });
    serve_async_connection(connection, host).await;
}

/// Serves a bounded number of fixture connections through one shared host.
///
/// This exists only for outcome tests that must exercise ordinary commands and
/// persistent run-stream peers against the same task and subscriber registry.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub async fn serve_test_async_listener(
    listener: AsyncLocalListener,
    facade: DaemonApplicationFacade,
    connection_count: usize,
) {
    let host = Arc::new(HostState {
        facade,
        data: Mutex::new(HostData::default()),
        publication_gate: Mutex::new(()),
        first_append_gate: None,
        terminalizer_failures: AtomicUsize::new(0),
        terminalizer_attempts: AtomicUsize::new(0),
        terminalizer_retry_paused: AtomicBool::new(false),
        terminalizer_failure_entered: tokio::sync::Notify::new(),
        terminalizer_failure_release: tokio::sync::Notify::new(),
        terminalizer_completed: tokio::sync::Notify::new(),
        task_completed: tokio::sync::Notify::new(),
    });
    for _ in 0..connection_count {
        let Ok(connection) = listener.accept().await else {
            return;
        };
        let host = Arc::clone(&host);
        tokio::spawn(async move {
            serve_async_connection(connection, host).await;
        });
    }
}

/// Serves bounded fixture connections with a deterministic first-append gate.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub async fn serve_test_async_listener_with_first_append_gate(
    listener: AsyncLocalListener,
    facade: DaemonApplicationFacade,
    connection_count: usize,
    first_append_gate: Arc<dyn ModelRunFirstAppendGate>,
) {
    let host = Arc::new(HostState {
        facade,
        data: Mutex::new(HostData::default()),
        publication_gate: Mutex::new(()),
        first_append_gate: Some(first_append_gate),
        terminalizer_failures: AtomicUsize::new(0),
        terminalizer_attempts: AtomicUsize::new(0),
        terminalizer_retry_paused: AtomicBool::new(false),
        terminalizer_failure_entered: tokio::sync::Notify::new(),
        terminalizer_failure_release: tokio::sync::Notify::new(),
        terminalizer_completed: tokio::sync::Notify::new(),
        task_completed: tokio::sync::Notify::new(),
    });
    for _ in 0..connection_count {
        let Ok(connection) = listener.accept().await else {
            return;
        };
        let host = Arc::clone(&host);
        tokio::spawn(async move {
            serve_async_connection(connection, host).await;
        });
    }
}

/// Owns one bounded fixture host and every task it creates.
///
/// This deterministic test-only lifecycle is not production process shutdown:
/// it aborts fixture connection and execution tasks so a subsequent facade open
/// observes the same durable state a fresh host would recover.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[derive(Clone)]
pub struct TestHostLifecycle {
    host: Arc<HostState>,
    connection_tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

/// Creates a deterministic bounded lifecycle for daemon-host outcome fixtures.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[must_use]
pub fn test_host_lifecycle(facade: DaemonApplicationFacade) -> TestHostLifecycle {
    TestHostLifecycle {
        host: Arc::new(HostState {
            facade,
            data: Mutex::new(HostData::default()),
            publication_gate: Mutex::new(()),
            first_append_gate: None,
            terminalizer_failures: AtomicUsize::new(0),
            terminalizer_attempts: AtomicUsize::new(0),
            terminalizer_retry_paused: AtomicBool::new(false),
            terminalizer_failure_entered: tokio::sync::Notify::new(),
            terminalizer_failure_release: tokio::sync::Notify::new(),
            terminalizer_completed: tokio::sync::Notify::new(),
            task_completed: tokio::sync::Notify::new(),
        }),
        connection_tasks: Arc::new(Mutex::new(Vec::new())),
    }
}

#[cfg(any(test, feature = "test-support"))]
impl TestHostLifecycle {
    /// Attempts exact durable admission through this fixture host.
    pub fn admit_starting_run(&self, session_id: SessionId, run_id: RunId) {
        self.host.schedule_if_starting(session_id, run_id);
    }

    /// Returns the currently registered fixture execution count.
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.host.data.lock().map_or(0, |data| data.tasks.len())
    }

    /// Makes the next exact cancellation terminalization fail before it is retried.
    #[doc(hidden)]
    pub fn inject_terminalizer_failure_once(&self) {
        self.host.inject_terminalizer_failure_once();
    }

    /// Returns terminalizer attempts made by this bounded fixture host.
    #[doc(hidden)]
    #[must_use]
    pub fn terminalizer_attempts(&self) -> usize {
        self.host.terminalizer_attempts()
    }

    /// Waits until every currently registered fixture task has removed its entry.
    #[doc(hidden)]
    pub async fn wait_for_task_cleanup(&self) {
        self.host.wait_for_task_cleanup().await;
    }

    /// Waits for the admitted executor of this exact run to return.
    #[doc(hidden)]
    #[must_use]
    pub async fn wait_for_execution_completion(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> bool {
        self.host
            .wait_for_execution_completion((session_id, run_id))
            .await
    }

    /// Waits until the injected terminalizer failure has been observed.
    #[doc(hidden)]
    pub async fn wait_for_terminalizer_failure(&self) {
        self.host.wait_for_terminalizer_failure().await;
    }

    /// Releases the deterministic retry after an injected terminalizer failure.
    #[doc(hidden)]
    pub fn release_terminalizer_retry(&self) {
        self.host.release_terminalizer_retry();
    }

    /// Waits for a terminalizer to prove a terminal reread and clean its registry entry.
    #[doc(hidden)]
    pub async fn wait_for_terminalizer_completion(&self) {
        self.host.wait_for_terminalizer_completion().await;
    }

    /// Serves exactly `connection_count` fixture peers through this host.
    pub async fn serve_connections(&self, listener: AsyncLocalListener, connection_count: usize) {
        for _ in 0..connection_count {
            let Ok(connection) = listener.accept().await else {
                return;
            };
            let host = Arc::clone(&self.host);
            let task = tokio::spawn(async move {
                serve_async_connection(connection, host).await;
            });
            let Ok(mut tasks) = self.connection_tasks.lock() else {
                task.abort();
                return;
            };
            tasks.push(task);
        }
    }

    /// Aborts and joins every fixture connection/execution task before dropping the host.
    pub async fn shutdown(self) {
        let connection_tasks = {
            let mut tasks = self
                .connection_tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *tasks)
        };
        for task in &connection_tasks {
            task.abort();
        }
        let execution_tasks = self.host.abort_test_execution_tasks();
        for task in &execution_tasks {
            task.abort();
        }
        for task in connection_tasks {
            let _ = task.await;
        }
        for task in execution_tasks {
            let _ = task.await;
        }
    }
}

#[cfg(test)]
fn serve_next_connection(
    listener: &LocalListener,
    facade: &DaemonApplicationFacade,
) -> DtoResult<()> {
    listener.accept().map_or_else(
        |_| {
            Err(ErrorDto::unavailable(
                "local_daemon_listener_unavailable",
                "the local daemon listener is unavailable",
            ))
        },
        |connection| {
            let connection_facade = facade.clone();
            let _ = thread::Builder::new()
                .name("intention-daemon-connection".to_owned())
                .spawn(move || serve_connection(connection, connection_facade));
            Ok(())
        },
    )
}

#[cfg(any(test, feature = "test-support"))]
fn serve_connection(mut connection: LocalConnection, facade: DaemonApplicationFacade) {
    let hello = match daemon_hello() {
        Ok(hello) => hello,
        Err(_) => return,
    };
    if negotiate_daemon(&mut connection, hello).is_err() {
        return;
    }
    let request = match connection.receive_request() {
        Ok(request) => request,
        Err(_) => return,
    };
    let payload = match request.message().payload() {
        ProtocolRequestPayloadDto::Command(command) => match command {
            intention_protocol::ProtocolCommandDto::SubscribeSession(subscription) => {
                ProtocolResponsePayloadDto::Subscription(facade.subscribe(*subscription))
            }
            _ => ProtocolResponsePayloadDto::CommandResult(facade.command(command.clone())),
        },
        ProtocolRequestPayloadDto::Query(query) => {
            ProtocolResponsePayloadDto::QueryResult(facade.query(*query))
        }
    };
    let response = ProtocolResponseEnvelopeDto::new(
        local_protocol_version(),
        request.correlation_id(),
        ProtocolMessageDto::new(intention_protocol::CURRENT_DTO_SCHEMA_VERSION, payload),
    );
    let _ = connection.send_response(&response);
}

fn unix_timestamp() -> DtoResult<TimestampDto> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            ErrorDto::unavailable(
                "daemon_clock_unavailable",
                "the daemon clock is unavailable",
            )
        })?
        .as_secs();
    TimestampDto::from_unix_seconds(i64::try_from(seconds).map_err(|_| {
        ErrorDto::unavailable(
            "daemon_clock_unavailable",
            "the daemon clock is unavailable",
        )
    })?)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "Daemon host unit tests use controlled native fixtures for direct protocol diagnostics."
    )]

    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use intention_config::{
        ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
    };
    use intention_domain::{
        CreateSessionCommandDto, RunModeDto, SendUserTurnCommandDto, WorkspaceRootDto,
    };
    use intention_model::{
        FinishReasonDto, ModelCapabilitiesDto, ModelDriver, ModelEventDto, ModelEventStream,
        ModelExecutionDriver,
    };
    use intention_protocol::{
        ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, ProtocolHelloDto,
        ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestEnvelopeDto,
        ProtocolRequestPayloadDto, ProtocolResponsePayloadDto, RunSubscriptionRequestEnvelopeDto,
        SendUserTurnOutcomeDto, SubscribeRunCommandDto,
    };
    use intention_transport::{AsyncLocalClientConnection, AsyncLocalListener, negotiate_client};
    use intention_types::{
        ConfigRevisionId, CorrelationIdDto, ProjectId, SchemaVersionDto, TimestampDto, TurnId,
        WorkspaceId,
    };
    use tempfile::TempDir;

    fn endpoint() -> LocalEndpoint {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after Unix epoch")
            .as_nanos();
        LocalEndpoint::from_instance_id(format!("daemon-library-{nanos}"))
            .expect("fixture endpoint is valid")
    }

    fn fixture_facade() -> (TempDir, DaemonApplicationFacade) {
        fixture_facade_with_driver(Arc::new(EmptyDriver))
    }

    fn fixture_snapshot() -> ConfigSnapshotDto {
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-daemon-unit.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture configuration path is absolute"),
        );
        let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"",
            source,
        ))
        .expect("fixture configuration resolves");
        ConfigSnapshotDto::new(
            SchemaVersionDto::new(1, 0),
            ConfigRevisionId::new(),
            TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid"),
            resolved,
        )
        .expect("fixture snapshot is credential-free")
    }

    fn fixture_facade_with_driver(
        driver: Arc<dyn ModelExecutionDriver + Send + Sync>,
    ) -> (TempDir, DaemonApplicationFacade) {
        let directory = TempDir::new().expect("temporary fixture directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support_with_driver(
            directory.path().join("daemon.sqlite"),
            fixture_snapshot(),
            driver,
        )
        .expect("fixture facade opens");
        (directory, facade)
    }

    struct EmptyDriver;

    impl ModelDriver for EmptyDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }
    }

    impl ModelExecutionDriver for EmptyDriver {
        fn execute(
            &self,
            _request: intention_model::ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> ModelEventStream {
            Box::pin(futures_util::stream::empty())
        }
    }

    struct CompletedDriver;

    impl ModelDriver for CompletedDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }
    }

    impl ModelExecutionDriver for CompletedDriver {
        fn execute(
            &self,
            _request: intention_model::ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> ModelEventStream {
            Box::pin(futures_util::stream::iter(vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("fixture output").expect("fixture text is valid")),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ]))
        }
    }

    struct PendingDriver;

    impl ModelDriver for PendingDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }
    }

    impl ModelExecutionDriver for PendingDriver {
        fn execute(
            &self,
            _request: intention_model::ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> ModelEventStream {
            Box::pin(futures_util::stream::pending())
        }
    }

    struct ImmediateTime;

    impl ModelTimePort for ImmediateTime {
        fn now(&self) -> TimestampDto {
            TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
        }

        fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
            Box::pin(async move {
                let _ = duration;
            })
        }
    }

    fn create_and_start(facade: &DaemonApplicationFacade) -> (SessionId, RunId) {
        let session_id = SessionId::new();
        assert!(matches!(
            facade.command(ProtocolCommandDto::CreateSession(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    session_id,
                    WorkspaceId::new(),
                    WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
                        .expect("fixture workspace is absolute"),
                    RunModeDto::Build,
                )
            )),
            ProtocolCommandResultDto::Accepted(_)
        ));
        let accepted = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "fixture turn")
                .expect("fixture turn is valid"),
        ));
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            unreachable!("fixture turn starts")
        };
        let Some(ProtocolAcceptedResultDto::SendUserTurn(turn)) = accepted.result() else {
            unreachable!("fixture result is a turn")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("fixture first turn starts")
        };
        (session_id, run_id)
    }

    fn ordinary_hello() -> ProtocolHelloDto {
        ProtocolHelloDto::new(
            local_protocol_version(),
            vec![
                ProtocolCapabilityDto::SessionSubscriptions,
                ProtocolCapabilityDto::CorrelatedRequests,
                ProtocolCapabilityDto::DaemonHealth,
            ],
            "daemon-host-test",
        )
        .expect("fixture hello is valid")
    }

    #[tokio::test]
    async fn tokio_time_exposes_a_safe_timestamp_and_sleep_future() {
        let time = TokioTime;
        assert!(time.now().unix_seconds() >= 0);
        time.sleep(Duration::ZERO).await;
        let immediate = ImmediateTime;
        immediate.sleep(Duration::ZERO).await;
    }

    #[tokio::test]
    async fn async_host_keeps_m3_requests_and_run_streams_on_one_listener() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(CompletedDriver));
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let host = host_for_test(facade);
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let connection = listener.accept().await.expect("client connects");
                let host = Arc::clone(&host);
                tokio::spawn(async move { serve_async_connection(connection, host).await });
            }
        });

        let connection = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("ordinary client connects");
        let (_remote, mut requests, mut responses) = connection
            .negotiate(ordinary_hello())
            .await
            .expect("ordinary client negotiates");
        let session_id = SessionId::new();
        let create_correlation = CorrelationIdDto::new();
        requests
            .send(&ProtocolRequestEnvelopeDto::new(
                local_protocol_version(),
                create_correlation,
                ProtocolMessageDto::new(
                    intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                    ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
                        CreateSessionCommandDto::new(
                            ProjectId::new(),
                            session_id,
                            WorkspaceId::new(),
                            WorkspaceRootDto::parse(
                                std::env::temp_dir().to_string_lossy().into_owned(),
                            )
                            .expect("fixture workspace is absolute"),
                            RunModeDto::Build,
                        ),
                    )),
                ),
            ))
            .await
            .expect("create request sends");
        assert_eq!(
            responses
                .receive()
                .await
                .expect("create response arrives")
                .correlation_id(),
            create_correlation
        );

        let turn_correlation = CorrelationIdDto::new();
        requests
            .send(&ProtocolRequestEnvelopeDto::new(
                local_protocol_version(),
                turn_correlation,
                ProtocolMessageDto::new(
                    intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                    ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
                        SendUserTurnCommandDto::new(session_id, TurnId::new(), "streamed turn")
                            .expect("fixture turn is valid"),
                    )),
                ),
            ))
            .await
            .expect("turn request sends");
        let turn_response = responses.receive().await.expect("turn response arrives");
        assert_eq!(turn_response.correlation_id(), turn_correlation);
        let ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(accepted)) =
            turn_response.message().payload()
        else {
            panic!("turn response is accepted")
        };
        let Some(ProtocolAcceptedResultDto::SendUserTurn(turn)) = accepted.result() else {
            panic!("turn response contains a run")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            panic!("turn starts a run")
        };
        drop(requests);
        drop(responses);

        tokio::time::sleep(Duration::from_millis(50)).await;
        let connection = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("stream client connects");
        let (_remote, mut requests, mut frames) = connection
            .negotiate_daemon_frames(
                ProtocolHelloDto::new(
                    local_protocol_version(),
                    vec![ProtocolCapabilityDto::RunStreamSubscriptions],
                    "daemon-stream-test",
                )
                .expect("stream hello is valid"),
            )
            .await
            .expect("stream client negotiates");
        let correlation = CorrelationIdDto::new();
        let subscription = SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
            None,
        );
        requests
            .send_run_subscription(&RunSubscriptionRequestEnvelopeDto::new(
                local_protocol_version(),
                correlation,
                ProtocolMessageDto::new(
                    intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                    subscription,
                ),
            ))
            .await
            .expect("stream subscription sends");
        assert!(matches!(
            frames.receive().await.expect("current replay arrives"),
            ProtocolDaemonFrameDto::Response(response)
                if response.correlation_id() == correlation
        ));
        server.await.expect("host accepted both connections");
    }

    #[tokio::test]
    async fn host_executes_starting_run_once_and_publishes_durable_live_batches() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(CompletedDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade.clone());
        let (sender, mut receiver) = tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (close, _closed) = tokio::sync::watch::channel(false);
        assert!(
            host.register_subscriber(
                session_id,
                run_id,
                None,
                sender,
                close,
                CorrelationIdDto::new(),
            )
            .is_some()
        );
        assert!(matches!(
            receiver.recv().await,
            Some(ProtocolDaemonFrameDto::Response(_))
        ));

        host.schedule_if_starting(session_id, run_id);
        host.schedule_if_starting(session_id, run_id);
        let mut saw_live = false;
        let mut saw_completed = false;
        for _ in 0..6 {
            let frame = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .expect("host publishes promptly")
                .expect("subscriber remains connected");
            match frame {
                ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::LiveBatch(batch)) => {
                    saw_live |= !batch.facts().is_empty();
                }
                ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Snapshot(snapshot)) => {
                    saw_completed |=
                        snapshot.snapshot().run_projection().status() == RunStatusDto::Completed;
                }
                ProtocolDaemonFrameDto::Response(_)
                | ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Resync(_)) => {}
            }
            if saw_live && saw_completed {
                break;
            }
        }
        assert!(saw_live);
        assert!(saw_completed);
        assert_eq!(
            facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("completed run reads")
                .snapshot()
                .run_projection()
                .status(),
            RunStatusDto::Completed
        );
        assert!(
            host.data
                .lock()
                .expect("host registry remains available")
                .tasks
                .is_empty()
        );
    }

    #[test]
    fn duplicate_or_unknown_admission_never_creates_an_extra_task() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade);
        host.schedule_if_starting(session_id, RunId::new());
        assert!(
            host.data
                .lock()
                .expect("host registry remains available")
                .tasks
                .is_empty()
        );
        host.data
            .lock()
            .expect("host registry remains available")
            .tasks
            .insert((session_id, run_id), ModelCancellationSignal::new());
        host.schedule_if_starting(session_id, run_id);
        assert_eq!(
            host.data
                .lock()
                .expect("host registry remains available")
                .tasks
                .len(),
            1
        );

        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade);
        host.facade
            .stop_run_for_daemon_host(session_id, run_id)
            .expect("fixture run becomes cancelling");
        host.schedule_if_starting(session_id, run_id);
    }

    #[tokio::test]
    async fn host_stop_without_an_admitted_task_terminalizes_and_cleans_the_registry() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade.clone());
        host.stop_run(session_id, run_id)
            .expect("host stop commits cancelling without a task");
        for _ in 0..20 {
            if facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("run replay reads")
                .snapshot()
                .run_projection()
                .status()
                == RunStatusDto::Cancelled
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("cancelled run replay reads")
                .snapshot()
                .run_projection()
                .status(),
            RunStatusDto::Cancelled
        );
        assert!(
            host.data
                .lock()
                .expect("host registry remains available")
                .tasks
                .is_empty()
        );
    }

    #[tokio::test]
    async fn host_stop_signals_the_registered_task_and_executor_owns_cancelled_terminal_state() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(PendingDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade.clone());
        host.schedule_if_starting(session_id, run_id);
        for _ in 0..20 {
            if host
                .data
                .lock()
                .expect("host registry remains available")
                .tasks
                .contains_key(&(session_id, run_id))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            host.data
                .lock()
                .expect("host registry remains available")
                .tasks
                .contains_key(&(session_id, run_id))
        );
        host.stop_run(session_id, run_id)
            .expect("host stop commits and signals");
        for _ in 0..20 {
            if facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("run replay reads")
                .snapshot()
                .run_projection()
                .status()
                == RunStatusDto::Cancelled
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("cancelled run replay reads")
                .snapshot()
                .run_projection()
                .status(),
            RunStatusDto::Cancelled
        );
        assert!(
            host.data
                .lock()
                .expect("host registry remains available")
                .tasks
                .is_empty()
        );
    }

    #[tokio::test]
    async fn subscriber_admission_scope_errors_and_capacity_are_isolated() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade);

        let (unknown_sender, mut unknown_receiver) =
            tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (unknown_close, _unknown_closed) = tokio::sync::watch::channel(false);
        assert!(
            host.register_subscriber(
                session_id,
                RunId::new(),
                None,
                unknown_sender,
                unknown_close,
                CorrelationIdDto::new(),
            )
            .is_none()
        );
        assert!(matches!(
            unknown_receiver.recv().await,
            Some(ProtocolDaemonFrameDto::Response(response))
                if matches!(
                    response.message().payload(),
                    ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Error(error))
                        if error.code() == "run_replay_not_found"
                )
        ));

        let (cursor_sender, mut cursor_receiver) =
            tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (cursor_close, _cursor_closed) = tokio::sync::watch::channel(false);
        assert!(
            host.register_subscriber(
                session_id,
                run_id,
                Some(RunEventCursorDto::new(1)),
                cursor_sender,
                cursor_close,
                CorrelationIdDto::new(),
            )
            .is_none()
        );
        assert!(matches!(
            cursor_receiver.recv().await,
            Some(ProtocolDaemonFrameDto::Response(response))
                if matches!(
                    response.message().payload(),
                    ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Resync(resync))
                        if resync.reason() == RunResyncReasonDto::InvalidCursor
                )
        ));

        let (slow_sender, mut slow_receiver) =
            tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (slow_close, mut slow_closed) = tokio::sync::watch::channel(false);
        let (healthy_sender, mut healthy_receiver) =
            tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (healthy_close, _healthy_closed) = tokio::sync::watch::channel(false);
        {
            let mut data = host.data.lock().expect("host data remains available");
            data.subscribers.insert(
                (session_id, run_id),
                vec![
                    Subscriber {
                        id: 1,
                        sender: slow_sender,
                        close: slow_close,
                    },
                    Subscriber {
                        id: 2,
                        sender: healthy_sender,
                        close: healthy_close,
                    },
                ],
            );
        }
        let frame = ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Resync(
            RunResyncDto::new(session_id, run_id, RunResyncReasonDto::CursorGap),
        ));
        for _ in 0..SUBSCRIBER_QUEUE_CAPACITY {
            host.broadcast((session_id, run_id), frame.clone());
            let _ = healthy_receiver.recv().await;
        }
        host.broadcast((session_id, run_id), frame);
        assert!(slow_closed.changed().await.is_ok());
        assert!(*slow_closed.borrow());
        assert!(healthy_receiver.recv().await.is_some());
        assert_eq!(
            host.data
                .lock()
                .expect("host data remains available")
                .subscribers
                .get(&(session_id, run_id))
                .map(Vec::len),
            Some(1)
        );
        assert!(slow_receiver.recv().await.is_some());
    }

    #[test]
    fn daemon_hello_and_subscription_response_are_typed() {
        let hello = daemon_hello().expect("daemon hello is valid");
        assert!(format!("{hello:?}").contains("intention-daemon"));
        let frame = run_subscription_response(
            CorrelationIdDto::new(),
            RunSubscriptionResponseDto::Resync(RunResyncDto::new(
                SessionId::new(),
                RunId::new(),
                RunResyncReasonDto::HistoryUnavailable,
            )),
        );
        assert!(matches!(frame, ProtocolDaemonFrameDto::Response(_)));
    }

    #[tokio::test]
    async fn terminalizer_retries_after_injected_failure() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade.clone());
        host.inject_terminalizer_failure_once();
        host.stop_run(session_id, run_id).expect("stop is accepted");
        host.wait_for_terminalizer_failure().await;
        assert_eq!(host.terminalizer_attempts(), 1);
        host.release_terminalizer_retry();
        host.wait_for_terminalizer_completion().await;
        assert_eq!(host.terminalizer_attempts(), 2);
        assert_eq!(host.data.lock().expect("host data").tasks.len(), 0);
    }

    #[tokio::test]
    async fn lifecycle_shutdown_aborts_fixture_tasks() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let lifecycle = test_host_lifecycle(facade);
        assert_eq!(lifecycle.task_count(), 0);
        lifecycle.shutdown().await;
    }

    #[test]
    fn run_rejects_an_endpoint_already_owned_by_another_host() {
        let endpoint = endpoint();
        let _listener = LocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        assert_eq!(
            run(endpoint)
                .expect_err("daemon must not reclaim an owned endpoint")
                .code(),
            "local_daemon_endpoint_in_use"
        );
    }

    #[test]
    fn listener_accepts_and_dispatches_one_typed_health_query() {
        let endpoint = endpoint();
        let listener = LocalListener::bind(endpoint.clone()).expect("fixture listener binds");
        let mut client = LocalConnection::connect(&endpoint).expect("fixture client connects");
        let server = std::thread::spawn(move || {
            let (directory, facade) = fixture_facade();
            let _directory = directory;
            serve_next_connection(&listener, &facade)
        });
        server
            .join()
            .expect("single accept thread completes")
            .expect("single accept succeeds");
        negotiate_client(
            &mut client,
            ProtocolHelloDto::new(
                local_protocol_version(),
                vec![
                    ProtocolCapabilityDto::SessionSubscriptions,
                    ProtocolCapabilityDto::CorrelatedRequests,
                    ProtocolCapabilityDto::DaemonHealth,
                ],
                "daemon-library-test",
            )
            .expect("fixture hello is valid"),
        )
        .expect("fixture hello negotiates");
        client
            .send_request(&ProtocolRequestEnvelopeDto::new(
                local_protocol_version(),
                CorrelationIdDto::new(),
                ProtocolMessageDto::new(
                    intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                    ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
                ),
            ))
            .expect("health request sends");
        assert!(matches!(
            client
                .receive_response()
                .expect("health response arrives")
                .message()
                .payload(),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(health))
                if health.readiness() == intention_protocol::DaemonReadinessDto::Ready
        ));
        std::thread::sleep(Duration::from_millis(1));
    }
}
