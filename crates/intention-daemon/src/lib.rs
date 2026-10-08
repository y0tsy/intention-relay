//! Thin daemon process host for the local protocol facade.
//!
//! The daemon owns the local listener and typed connection hosting. It delegates
//! health, query, command, and current-state subscription meaning to the durable
//! composition facade.

mod composition;

pub use composition::DaemonApplicationFacade;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use intention_domain::run_status_is_terminal;
use intention_engine::{
    LocalToolInvocationOutcomeDto, ModelRunCommitDto, ModelRunCommitObserver, ModelSleepFuture,
    ModelTimePort, ToolResultOutcomeDto,
};
use intention_proto::RunStatusDto;
use intention_proto::{
    CorrelationIdDto, DtoResult, ErrorDto, RunId, SessionId, TimestampDto, ToolCallDto,
};
use intention_proto::{
    JsonRpcResponseDto, ProtocolAcceptedDto, ProtocolCommandDto, ProtocolCommandResultDto,
    ProtocolDaemonMessageDto, ProtocolHelloDto, ProtocolRequestPayloadDto,
    ProtocolResponsePayloadDto, RunStatusFrameDto, RunStreamFrameDto, RunSubscriptionResponseDto,
    decode_request_line, encode_response, is_notification_line,
};
use intention_providers::ModelCancellationSignal;
use intention_tools::{GrepResult, PathsResult, ToolInput, ToolProjectedContent, ToolResult};
#[cfg(test)]
use intention_transport::LocalListener;
use intention_transport::{
    AsyncLocalListener, AsyncMessageSender, LocalEndpoint, local_protocol_version,
};

const SUBSCRIBER_QUEUE_CAPACITY: usize = 64;
const SUBSCRIBER_WRITE_DEADLINE: Duration = Duration::from_secs(10);

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
    sender: tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
    close: tokio::sync::watch::Sender<bool>,
}

#[derive(Clone, Copy)]
struct PublishedRun {
    status: RunStatusDto,
}

struct HostData {
    tasks: HashMap<RunKey, ModelCancellationSignal>,
    #[cfg(any(test, feature = "test-support"))]
    execution_tasks: Vec<tokio::task::JoinHandle<()>>,
    #[cfg(any(test, feature = "test-support"))]
    execution_completion: HashMap<RunKey, tokio::sync::watch::Sender<bool>>,
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
}

#[cfg(test)]
fn host_for_test(facade: DaemonApplicationFacade) -> Arc<HostState> {
    Arc::new(HostState {
        facade,
        data: Mutex::new(HostData::default()),
        publication_gate: Mutex::new(()),
    })
}

impl HostState {
    fn schedule_if_starting(self: &Arc<Self>, session_id: SessionId, run_id: RunId) {
        let key = (session_id, run_id);
        // Admission and interruption share this registry lock: an interrupt
        // either reaches the registered execution signal, or arrives before
        // admission and finds no in-flight operation to stop.
        let mut data = match self.data.lock() {
            Ok(data) => data,
            Err(_) => return,
        };
        if data.tasks.contains_key(&key) {
            return;
        }
        // The admitted run's cancellation signal is created here and embedded
        // in the execution input, so an interrupt and the executor share it.
        let cancellation = ModelCancellationSignal::new();
        let schedule = match self.facade.schedule_starting_run_for_daemon(
            session_id,
            run_id,
            cancellation.clone(),
        ) {
            Ok(schedule) => schedule,
            Err(_) => {
                drop(data);
                self.fail_unadmitted_starting_run(session_id, run_id);
                return;
            }
        };
        let std::collections::hash_map::Entry::Vacant(entry) = data.tasks.entry(key) else {
            return;
        };
        entry.insert(cancellation);
        #[cfg(any(test, feature = "test-support"))]
        {
            let (completion, _) = tokio::sync::watch::channel(false);
            data.execution_completion.insert(key, completion);
        }
        drop(data);
        let host = Arc::clone(self);
        let task = tokio::spawn(async move {
            let observer = HostCommitObserver {
                host: Arc::clone(&host),
            };
            let executor = DaemonToolExecutor::with_publication(
                host.facade.clone(),
                HostTranscriptPublisher {
                    facade: host.facade.clone(),
                    host: Arc::clone(&host),
                },
            );
            let result = host
                .facade
                .execute_scheduled_model_run_for_daemon_with_tool_executor(
                    schedule.clone(),
                    &TokioTime,
                    &observer,
                    &executor,
                )
                .await;
            // An executor error must never leave a non-terminal durable run
            // without an owner (PR24-012/013): a still-active run is
            // terminalized as `Failed` with the executor's stable error code.
            if let Err(error) = result {
                let active = host
                    .facade
                    .load_run_projection_for_daemon(key.0, key.1)
                    .map_or(true, |run| !run_status_is_terminal(run.status()));
                if active
                    && host
                        .facade
                        .fail_active_run_for_daemon(key.0, key.1, error.code())
                        .is_ok()
                {
                    host.publish_current(key.0, key.1);
                    host.on_terminal(key.0);
                }
            }
            if let Ok(mut data) = host.data.lock() {
                data.tasks.remove(&key);
            }
            #[cfg(any(test, feature = "test-support"))]
            {
                host.signal_execution_completion(key);
            }
        });
        #[cfg(any(test, feature = "test-support"))]
        self.track_test_execution_task(task);
        #[cfg(not(any(test, feature = "test-support")))]
        std::mem::drop(task);
    }

    /// Interrupts the current operation of one active run.
    ///
    /// The interrupt is accepted only for an exact active run; the run stays
    /// `Running`. The registered execution task observes the shared signal,
    /// ends its current provider stream or tool call with a partial result and
    /// a context notice, and the model continues with the next step.
    fn interrupt_run(
        self: &Arc<Self>,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<intention_proto::ProtocolAcceptedResultDto> {
        let accepted = self
            .facade
            .interrupt_run_for_daemon_host(session_id, run_id)?;
        let data = self.data.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_task_registry_unavailable",
                "the daemon task registry is unavailable",
            )
        })?;
        if let Some(cancellation) = data.tasks.get(&(session_id, run_id)) {
            cancellation.cancel();
        }
        drop(data);
        Ok(accepted)
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
    async fn wait_for_execution_completion(&self, key: RunKey) -> bool {
        let Some(mut completion) = self
            .data
            .lock()
            .ok()
            .and_then(|data| data.execution_completion.get(&key).cloned())
            .map(|sender| sender.subscribe())
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

    /// Marks the exact registered execution complete.
    ///
    /// The sender stays in the host registry so the unified terminalizer can
    /// report completion for runs it terminalizes after the executor task has
    /// already returned (PR24-013).
    #[cfg(any(test, feature = "test-support"))]
    fn signal_execution_completion(&self, key: RunKey) {
        if let Ok(data) = self.data.lock()
            && let Some(completion) = data.execution_completion.get(&key)
        {
            let _ = completion.send_replace(true);
        }
    }

    fn fail_unadmitted_starting_run(self: &Arc<Self>, session_id: SessionId, run_id: RunId) {
        if self
            .facade
            .fail_starting_run_for_daemon(session_id, run_id, "model_scheduling_unavailable")
            .is_ok()
        {
            self.publish_current(session_id, run_id);
            self.on_terminal(session_id);
        }
    }

    /// Runs the terminal side effect for one run that just reached a durable
    /// terminal state: schedule a current `Starting` successor exactly once.
    ///
    /// The terminal status frame is published directly from the committed
    /// transition value, so no publication retry is required.
    fn on_terminal(self: &Arc<Self>, session_id: SessionId) {
        if let Ok(Some(promoted)) = self.facade.current_starting_run_for_daemon(session_id) {
            self.schedule_if_starting(session_id, promoted);
        }
    }

    /// Publishes the current durable run status to live subscribers when it
    /// differs from the last published status, returning whether the read
    /// succeeded.
    fn publish_current(&self, session_id: SessionId, run_id: RunId) -> bool {
        let Ok(run) = self
            .facade
            .load_run_projection_for_daemon(session_id, run_id)
        else {
            return false;
        };
        self.publish_status(session_id, run_id, run.status());
        true
    }

    /// Publishes one committed run status to live subscribers.
    ///
    /// The frame carries the committed value; a repeated status is not
    /// re-published.
    fn publish_status(&self, session_id: SessionId, run_id: RunId, status: RunStatusDto) {
        let key = (session_id, run_id);
        let previous = self
            .data
            .lock()
            .ok()
            .and_then(|data| data.published.get(&key).copied());
        if previous.is_some_and(|value| value.status == status) {
            return;
        }
        self.broadcast(
            key,
            ProtocolDaemonMessageDto::run_frame(RunStreamFrameDto::Status(RunStatusFrameDto::new(
                session_id, run_id, status,
            ))),
        );
        if let Ok(mut data) = self.data.lock() {
            data.published.insert(key, PublishedRun { status });
        }
    }

    /// Publishes one committed transcript row to live subscribers.
    fn publish_content(&self, message: &intention_proto::MessageProjectionDto) {
        let Some(run_id) = message.run_id() else {
            return;
        };
        self.broadcast(
            (message.session_id(), run_id),
            ProtocolDaemonMessageDto::run_frame(RunStreamFrameDto::Content(message.clone())),
        );
    }

    fn broadcast(&self, key: RunKey, message: ProtocolDaemonMessageDto) {
        // Publication shares the registration gate, so a live frame can never
        // enter a subscriber's queue before that subscription's correlated
        // snapshot reply, which registration queues while holding this gate.
        let Ok(_publication_gate) = self.publication_gate.lock() else {
            return;
        };
        let mut slow = Vec::new();
        if let Ok(data) = self.data.lock()
            && let Some(subscribers) = data.subscribers.get(&key)
        {
            for subscriber in subscribers {
                if subscriber.sender.try_send(message.clone()).is_err() {
                    slow.push(subscriber.id);
                }
            }
        }
        for id in slow {
            // A subscriber whose queue overflowed is closed instead of
            // resynchronized: it re-reads current state on reconnect.
            self.close_subscriber(key, id);
        }
    }

    fn close_subscriber(&self, key: RunKey, id: u64) {
        if let Ok(data) = self.data.lock()
            && let Some(subscribers) = data.subscribers.get(&key)
            && let Some(subscriber) = subscribers.iter().find(|subscriber| subscriber.id == id)
        {
            subscriber.close.send_replace(true);
        }
        self.remove_subscriber(key, id);
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
        sender: tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
        close: tokio::sync::watch::Sender<bool>,
        request_id: u64,
    ) -> Option<u64> {
        let key = (session_id, run_id);
        let Ok(_publication_gate) = self.publication_gate.lock() else {
            let _ = sender.try_send(run_subscription_response(
                request_id,
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
                    request_id,
                    RunSubscriptionResponseDto::Error(ErrorDto::unavailable(
                        "daemon_subscriber_unavailable",
                        "the daemon subscriber is unavailable",
                    )),
                ));
                return None;
            }
        };
        if let Err(error) = self
            .facade
            .load_run_projection_for_daemon(session_id, run_id)
        {
            let _ = sender.try_send(run_subscription_response(
                request_id,
                RunSubscriptionResponseDto::Error(error),
            ));
            return None;
        }
        let id = data.next_subscriber_id;
        data.next_subscriber_id = data.next_subscriber_id.saturating_add(1);
        data.subscribers.entry(key).or_default().push(Subscriber {
            id,
            sender: sender.clone(),
            close: close.clone(),
        });
        drop(data);
        // The subscriber is registered before this second durable read, and
        // publication shares the registration gate: no later live frame can
        // place itself in this subscriber's FIFO queue before this response
        // enters it.
        let response = match self.facade.load_run_snapshot_for_daemon(session_id, run_id) {
            Ok(snapshot) => RunSubscriptionResponseDto::Snapshot(snapshot),
            Err(error) => {
                // The run became unreadable between the admission read and this
                // snapshot read, so the registration is withdrawn before the
                // typed error is reported.
                self.remove_subscriber(key, id);
                RunSubscriptionResponseDto::Error(error)
            }
        };
        if sender
            .try_send(run_subscription_response(request_id, response))
            .is_err()
        {
            // The per-connection queue is full, so the correlated reply cannot
            // be delivered. The subscriber is removed and the failure is
            // reported through the same typed error every other registration
            // failure uses.
            self.remove_subscriber(key, id);
            if sender
                .try_send(run_subscription_response(
                    request_id,
                    RunSubscriptionResponseDto::Error(ErrorDto::unavailable(
                        "daemon_subscriber_unavailable",
                        "the daemon subscriber is unavailable",
                    )),
                ))
                .is_err()
            {
                // The queue that rejected the reply rejected the typed error
                // too, so the peer must not be left waiting silently: trip the
                // per-connection close signal exactly as the slow-subscriber
                // path does, so the serve loop ends and the peer observes a
                // closed stream instead of hanging.
                close.send_replace(true);
            }
            return None;
        }
        Some(id)
    }
}

fn run_subscription_response(
    request_id: u64,
    response: RunSubscriptionResponseDto,
) -> ProtocolDaemonMessageDto {
    ProtocolDaemonMessageDto::Response(encode_response(
        request_id,
        ProtocolResponsePayloadDto::RunSubscription(response),
    ))
}

struct HostCommitObserver {
    host: Arc<HostState>,
}

impl ModelRunCommitObserver for HostCommitObserver {
    fn observe_model_run_commit(&self, committed: &ModelRunCommitDto) {
        match committed {
            ModelRunCommitDto::Content(message) => self.host.publish_content(message),
            ModelRunCommitDto::Status {
                session_id,
                run_id,
                status,
            } => {
                self.host.publish_status(*session_id, *run_id, *status);
                if run_status_is_terminal(*status) {
                    self.host.on_terminal(*session_id);
                }
            }
        }
    }
}

/// Publishes committed tool transcript rows to the host's live subscribers.
#[derive(Clone)]
struct HostTranscriptPublisher {
    facade: DaemonApplicationFacade,
    host: Arc<HostState>,
}

impl intention_engine::ToolResultPublicationPort for HostTranscriptPublisher {
    fn publish_committed_message(
        &self,
        message: &intention_proto::MessageProjectionDto,
    ) -> DtoResult<()> {
        // A committed tool-result row is verified against the durable structured
        // evidence of its own call before its frame is broadcast; a tool-call
        // row has no structured result yet and is published as committed.
        if message.kind() == intention_proto::MessageKindDto::ToolResult {
            let (Some(run_id), Some(call_id)) = (message.run_id(), message.tool_call_id()) else {
                return Err(ErrorDto::unavailable(
                    "tool_result_evidence_unavailable",
                    "committed tool result evidence is unavailable",
                ));
            };
            let evidence =
                self.facade
                    .load_tool_result_for_daemon(message.session_id(), run_id, call_id)?;
            if evidence.call_id() != call_id || evidence.run_id() != run_id {
                return Err(ErrorDto::unavailable(
                    "tool_result_evidence_unavailable",
                    "committed tool result evidence is unavailable",
                ));
            }
        }
        self.host.publish_content(message);
        Ok(())
    }
}

/// Executes provider-normalized tool calls through the durable daemon-owned tool path.
///
/// Each call is decoded into the typed daemon tool input and executed through
/// the facade's durable local-tool lifecycle, which hands every committed
/// transcript row to the configured publication boundary. The blocking tool
/// effect runs on a spawned worker so the async execution loop is never stalled.
#[doc(hidden)]
#[derive(Clone)]
pub struct DaemonToolExecutor<P = ()> {
    facade: DaemonApplicationFacade,
    publisher: P,
}

impl DaemonToolExecutor<()> {
    /// Binds one durable facade to the daemon tool-execution path without a
    /// publication boundary, so committed rows stay unpublished.
    #[must_use]
    pub const fn new(facade: DaemonApplicationFacade) -> Self {
        Self {
            facade,
            publisher: (),
        }
    }
}

impl<P: intention_engine::ToolResultPublicationPort + Clone + Send + Sync + 'static>
    DaemonToolExecutor<P>
{
    /// Binds one durable facade and the host publication boundary, so every
    /// committed transcript row of a tool call reaches the live subscribers.
    #[must_use]
    pub const fn with_publication(facade: DaemonApplicationFacade, publisher: P) -> Self {
        Self { facade, publisher }
    }
}

impl<P: intention_engine::ToolResultPublicationPort + Clone + Send + Sync + 'static>
    intention_engine::ToolExecutionPort for DaemonToolExecutor<P>
{
    fn execute_tool(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call: ToolCallDto,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = DtoResult<ToolResultOutcomeDto>> + Send + '_>,
    > {
        let facade = self.facade.clone();
        let publisher = self.publisher.clone();
        Box::pin(async move {
            let tool_id = call.name().to_owned();
            let call_id = call.call_id();
            let arguments = call.arguments_json().to_owned();
            let input = ToolInput::from_arguments_json(&tool_id, &arguments)?;
            let result = tokio::task::spawn_blocking(move || {
                let workspace = facade.resolve_workspace_root_for_daemon(session_id)?;
                facade.invoke_local_tool_for_daemon_with_publication(
                    session_id, run_id, call_id, tool_id, input, workspace, arguments, &publisher,
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
                Ok(LocalToolInvocationOutcomeDto::Completed(result)) => {
                    normalize_tool_result(result)
                }
                Ok(LocalToolInvocationOutcomeDto::Partial { stopped, result }) => {
                    partial_tool_result(stopped, result)
                }
                Err(error) => Ok(ToolResultOutcomeDto::failed(error)),
            }
        })
    }
}

/// Normalizes one typed tool result into bounded durable outcome content.
fn normalize_tool_result(result: ToolResult) -> DtoResult<ToolResultOutcomeDto> {
    ToolResultOutcomeDto::completed(normalize_tool_result_content(result)?)
}

/// Maps one interrupted invocation into bounded, model-visible partial content.
///
/// Captured output is normalized exactly like a completed result; the notice
/// line tells the model that the call never received a final result, and the
/// model decides what the captured output means.
fn partial_tool_result(
    stopped: bool,
    result: Option<ToolResult>,
) -> DtoResult<ToolResultOutcomeDto> {
    let notice = match (stopped, &result) {
        (true, Some(_)) => {
            "[The tool call was stopped before a final result; the output above is partial.]"
        }
        (false, Some(_)) => {
            "[The tool call did not receive a final result; the output above is partial.]"
        }
        (true, None) => "[The tool call was stopped before a final result.]",
        (false, None) => "[The tool call did not receive a final result.]",
    };
    let content = match result {
        Some(result) => format!("{}\n{notice}", normalize_tool_result_content(result)?),
        None => notice.to_owned(),
    };
    ToolResultOutcomeDto::partial(content)
}

/// Normalizes one typed tool result into bounded durable content.
///
/// The projection is redacted and workspace-relative by construction, and
/// `ToolResultOutcomeDto::succeeded` keeps the durable outcome within its own
/// content bound. Search results serialize their own typed result DTO, so the
/// retained window and its truncation flag stay self-describing and identical
/// for glob paths and grep matches (C-04).
fn normalize_tool_result_content(result: ToolResult) -> DtoResult<String> {
    let content = match result.projection().content {
        ToolProjectedContent::Text { text, truncated } => {
            if truncated {
                format!("{}\n[truncated]", text.as_str())
            } else {
                text.as_str().to_owned()
            }
        }
        ToolProjectedContent::Paths { paths, truncated } => {
            serde_json::to_string(&PathsResult { paths, truncated }).map_err(|_| {
                ErrorDto::validation(
                    "invalid_tool_result_content",
                    "tool result content could not be normalized",
                )
            })?
        }
        ToolProjectedContent::Matches { matches, truncated } => {
            serde_json::to_string(&GrepResult { matches, truncated }).map_err(|_| {
                ErrorDto::validation(
                    "invalid_tool_result_content",
                    "tool result content could not be normalized",
                )
            })?
        }
        ToolProjectedContent::Mutation { bytes } => format!("{bytes} bytes"),
    };
    Ok(content)
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

/// Builds one daemon host around a facade without exposing it outside the host.
fn new_host(facade: DaemonApplicationFacade) -> Arc<HostState> {
    Arc::new(HostState {
        facade,
        data: Mutex::new(HostData::default()),
        publication_gate: Mutex::new(()),
    })
}

async fn serve_async_listener(
    listener: AsyncLocalListener,
    facade: DaemonApplicationFacade,
) -> DtoResult<()> {
    let host = new_host(facade);
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
    let (_, mut requests, mut messages) = match connection.negotiate(hello).await {
        Ok(roles) => roles,
        Err(_) => return,
    };
    // One connection carries one role: requests arrive on the same NDJSON
    // stream that carries responses and `run.frame` notifications back. The
    // per-connection queue carries every subscriber message, so a registration
    // response keeps its ordering ahead of live frames (PR24-014).
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
            line = requests.receive_line() => {
                let Ok(line) = line else {
                    if let Some((key, id)) = registered {
                        host.remove_subscriber(key, id);
                    }
                    return;
                };
                let request = match decode_request_line(&line) {
                    Ok(request) => request,
                    Err(failure) => {
                        if is_notification_line(&line) {
                            // A request line without an `id` member is a
                            // JSON-RPC notification, and the server must not
                            // answer one (W-06). An explicit `"id": null` is a
                            // request, so it still receives the correlated
                            // error reply.
                            continue;
                        }
                        let (id, error) = failure.into_parts();
                        let reply = ProtocolDaemonMessageDto::Response(
                            JsonRpcResponseDto::error(id, error),
                        );
                        if write_message_with_deadline(&mut messages, reply).await.is_err() {
                            if let Some((key, id)) = registered {
                                host.remove_subscriber(key, id);
                            }
                            return;
                        }
                        continue;
                    }
                };
                let request_id = request.id();
                match request.payload() {
                    ProtocolRequestPayloadDto::RunSubscription(subscription) => {
                        if let Some((key, id)) = registered.take() {
                            host.remove_subscriber(key, id);
                        }
                        let subscriber_id = host.register_subscriber(
                            subscription.session_id(),
                            subscription.run_id(),
                            sender.clone(),
                            close_sender.clone(),
                            request_id,
                        );
                        if let Some(subscriber_id) = subscriber_id {
                            registered = Some((
                                (subscription.session_id(), subscription.run_id()),
                                subscriber_id,
                            ));
                        }
                    }
                    payload => {
                        let response = ProtocolDaemonMessageDto::Response(encode_response(
                            request_id,
                            dispatch_request(&host, payload),
                        ));
                        if write_message_with_deadline(&mut messages, response)
                            .await
                            .is_err()
                        {
                            // A queued response is best effort: a timed-out OS
                            // write cannot be recovered, but it is never allowed
                            // to stall persistence or any other subscriber.
                            if let Some((key, id)) = registered {
                                host.remove_subscriber(key, id);
                            }
                            return;
                        }
                    }
                }
            }
            message = receiver.recv() => {
                let Some(message) = message else { return; };
                if write_message_with_deadline(&mut messages, message).await.is_err() {
                    // A queued frame is best effort: a timed-out OS write cannot
                    // be recovered, but it is never allowed to stall persistence
                    // or any other subscriber.
                    if let Some((key, id)) = registered {
                        host.remove_subscriber(key, id);
                    }
                    return;
                }
            }
        }
    }
}

/// Dispatches one decoded request against the shared facade and host registry.
fn dispatch_request(
    host: &Arc<HostState>,
    payload: &ProtocolRequestPayloadDto,
) -> ProtocolResponsePayloadDto {
    match payload {
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(subscription)) => {
            ProtocolResponsePayloadDto::Subscription(host.facade.subscribe(*subscription))
        }
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::InterruptRun(command)) => {
            let result = host
                .interrupt_run(command.session_id(), command.run_id())
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
                && let intention_proto::ProtocolAcceptedResultDto::SendUserTurn(turn) =
                    accepted.result()
                && let intention_proto::SendUserTurnOutcomeDto::Started { run_id, .. } =
                    turn.outcome()
            {
                host.schedule_if_starting(turn.session_id(), run_id);
            }
            ProtocolResponsePayloadDto::CommandResult(result)
        }
        ProtocolRequestPayloadDto::Query(query) => {
            ProtocolResponsePayloadDto::QueryResult(host.facade.query(*query))
        }
        ProtocolRequestPayloadDto::RunSubscription(_) => run_subscription_unsupported(),
    }
}

/// Answers a run subscription that arrived where no subscription can be served.
fn run_subscription_unsupported() -> ProtocolResponsePayloadDto {
    ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Error(
        ErrorDto::validation(
            "run_subscription_unavailable",
            "run subscriptions require an asynchronous daemon connection",
        ),
    ))
}

async fn write_message_with_deadline(
    sender: &mut AsyncMessageSender,
    message: ProtocolDaemonMessageDto,
) -> DtoResult<()> {
    write_with_deadline(sender.send_message(&message)).await
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
    ProtocolHelloDto::new(local_protocol_version(), "intention-daemon")
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
    let host = new_host(facade);
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
        host: new_host(facade),
        connection_tasks: Arc::new(Mutex::new(Vec::new())),
    }
}

#[cfg(any(test, feature = "test-support"))]
impl TestHostLifecycle {
    /// Returns the currently registered fixture execution count.
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.host.data.lock().map_or(0, |data| data.tasks.len())
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use intention_config::{
        ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
    };
    use intention_proto::{
        ConfigRevisionId, IdempotencyKey, ProjectId, SchemaVersionDto, TimestampDto, WorkspaceId,
    };
    use intention_proto::{
        CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, RunModeDto,
        SendUserTurnCommandDto, WorkspaceRootDto,
    };
    use intention_proto::{
        ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, ProtocolHelloDto,
        ProtocolMethodDto, ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestPayloadDto,
        ProtocolResponsePayloadDto, SendUserTurnOutcomeDto, SubscribeRunCommandDto,
        decode_response, encode_request,
    };
    use intention_providers::{
        FinishReasonDto, ModelCapabilitiesDto, ModelDriver, ModelEventDto, ModelEventStream,
        ModelExecutionDriver,
    };
    use intention_transport::{AsyncLocalClientConnection, AsyncLocalListener};
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
            _request: intention_providers::ModelRequestDto,
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
            _request: intention_providers::ModelRequestDto,
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
            _request: intention_providers::ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> ModelEventStream {
            Box::pin(futures_util::stream::pending())
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
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "fixture turn")
                .expect("fixture turn is valid"),
        ));
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            unreachable!("fixture turn starts")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
            unreachable!("fixture result is a turn")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("fixture first turn starts")
        };
        (session_id, run_id)
    }

    fn fixture_hello() -> ProtocolHelloDto {
        ProtocolHelloDto::new(local_protocol_version(), "daemon-host-test")
            .expect("fixture hello is valid")
    }

    #[tokio::test]
    async fn one_connection_serves_requests_and_run_frames_together() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(CompletedDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(serve_test_async_listener(listener, facade, 1));

        let connection = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("merged client connects");
        let (_remote, mut requests, mut messages) = connection
            .negotiate(fixture_hello())
            .await
            .expect("merged client negotiates");
        requests
            .send_message(&encode_request(
                1,
                ProtocolRequestPayloadDto::RunSubscription(SubscribeRunCommandDto::new(
                    intention_proto::CURRENT_DTO_SCHEMA_VERSION,
                    session_id,
                    run_id,
                )),
            ))
            .await
            .expect("subscription sends");
        let line = messages
            .receive_line()
            .await
            .expect("subscription reply arrives");
        // The correlated reply is the current durable state of this run.
        let ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Snapshot(
            snapshot,
        )) = decode_response(&line, ProtocolMethodDto::RunSubscribe, 1)
            .expect("subscription reply decodes")
        else {
            panic!("a run subscription answers with the current run snapshot")
        };
        assert_eq!(snapshot.run().session_id(), session_id);
        assert_eq!(snapshot.run().run_id(), run_id);

        // The same connection still serves an ordinary request while the
        // subscription is registered.
        requests
            .send_message(&encode_request(
                2,
                ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
            ))
            .await
            .expect("health request sends");
        let line = messages.receive_line().await.expect("health reply arrives");
        assert!(matches!(
            decode_response(&line, ProtocolMethodDto::DaemonHealth, 2),
            Ok(ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(health)))
                if health.readiness() == intention_proto::DaemonReadinessDto::Ready
        ));
        server.await.expect("host serves the merged connection");
    }

    #[tokio::test]
    async fn async_host_answers_a_stale_hello_with_a_typed_version_mismatch() {
        let (_directory, facade) = fixture_facade();
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(serve_test_async_listener(listener, facade, 1));

        let connection = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("stale client connects");
        let error = match connection
            .negotiate(
                ProtocolHelloDto::new(
                    intention_proto::ProtocolVersionDto::new(1, 1),
                    "stale-daemon-test",
                )
                .expect("stale hello is valid"),
            )
            .await
        {
            Ok(_) => panic!("a stale protocol version is refused with a typed error"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "incompatible_protocol_version");
        server.await.expect("host serves the refused peer");
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

        // An interrupt that arrives before admission is accepted without a
        // registered task, so it reaches no in-flight operation and the run
        // stays starting and untouched.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade);
        assert!(matches!(
            host.interrupt_run(session_id, run_id),
            Ok(ProtocolAcceptedResultDto::InterruptRun(value))
                if value.session_id() == session_id && value.run_id() == run_id
        ));
        assert!(
            host.data
                .lock()
                .expect("host registry remains available")
                .tasks
                .is_empty()
        );
        assert_eq!(
            host.facade
                .load_run_projection_for_daemon(session_id, run_id)
                .expect("starting run replay reads")
                .status(),
            RunStatusDto::Starting
        );
    }

    #[tokio::test]
    async fn host_interrupt_signals_the_registered_task_and_the_run_stays_active() {
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
        let signal = host
            .data
            .lock()
            .expect("host registry remains available")
            .tasks
            .get(&(session_id, run_id))
            .cloned()
            .expect("the admitted run registers its execution signal");
        // Wait until the pending provider stream is the run's live operation.
        for _ in 0..20 {
            if facade
                .load_run_projection_for_daemon(session_id, run_id)
                .expect("run replay reads")
                .status()
                == RunStatusDto::Running
            {
                break;
            }
            tokio::task::yield_now().await;
        }

        host.interrupt_run(session_id, run_id)
            .expect("host interrupt commits and signals");
        for _ in 0..20 {
            if signal.is_cancelled() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(signal.is_cancelled());

        // The interrupt ends the in-flight round with a notice; the run stays
        // active and its execution task keeps the continuation alive.
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            facade
                .load_run_projection_for_daemon(session_id, run_id)
                .expect("interrupted run replay reads")
                .status(),
            RunStatusDto::Running
        );
        assert!(
            host.data
                .lock()
                .expect("host registry remains available")
                .tasks
                .contains_key(&(session_id, run_id))
        );
        for task in host.abort_test_execution_tasks() {
            task.abort();
            let _ = task.await;
        }
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
            host.register_subscriber(session_id, RunId::new(), unknown_sender, unknown_close, 1)
                .is_none()
        );
        assert!(matches!(
            unknown_receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Response(response))
                if matches!(
                    response.result_value(),
                    Some(ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Error(error)))
                        if error.code() == "storage_record_not_found"
                )
        ));

        let (scoped_sender, mut scoped_receiver) =
            tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (scoped_close, _scoped_closed) = tokio::sync::watch::channel(false);
        let registered = host
            .register_subscriber(session_id, run_id, scoped_sender, scoped_close, 2)
            .expect("a current run admits one subscriber");
        assert!(matches!(
            scoped_receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Response(response))
                if matches!(
                    response.result_value(),
                    Some(ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Snapshot(snapshot)))
                        if snapshot.run().run_id() == run_id
                )
        ));
        host.remove_subscriber((session_id, run_id), registered);

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
        let frame = ProtocolDaemonMessageDto::run_frame(RunStreamFrameDto::Status(
            RunStatusFrameDto::new(session_id, run_id, RunStatusDto::Running),
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

    #[tokio::test]
    async fn a_full_subscriber_queue_fails_closed_instead_of_waiting_silently() {
        // C-03: when the per-connection queue cannot accept the correlated
        // reply, the registration fails closed: no subscriber is left
        // registered and the connection is told to end, so the peer never
        // waits for a reply that cannot arrive.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let (close, mut closed) = tokio::sync::watch::channel(false);
        let queued = ProtocolDaemonMessageDto::run_frame(RunStreamFrameDto::Status(
            RunStatusFrameDto::new(session_id, run_id, RunStatusDto::Running),
        ));
        sender
            .try_send(queued)
            .expect("the single-slot queue accepts one frame");

        assert!(
            host.register_subscriber(session_id, run_id, sender, close, 7)
                .is_none(),
            "a full queue cannot accept the correlated reply"
        );
        assert!(closed.changed().await.is_ok());
        assert!(
            *closed.borrow(),
            "the connection must be told to end instead of waiting silently"
        );
        assert_eq!(
            host.data
                .lock()
                .expect("host data remains available")
                .subscribers
                .get(&(session_id, run_id))
                .map(Vec::len),
            Some(0),
            "the failed registration leaves no subscriber registered"
        );
        assert!(matches!(
            receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Notification(_))
        ));
    }

    #[tokio::test]
    async fn a_correlated_subscription_reply_precedes_every_live_frame() {
        // The subscription reply is the correlated current-state snapshot, so
        // it is the first message a new subscriber receives even while the run
        // is publishing committed frames concurrently. Publication and
        // registration therefore share the same publication gate.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = host_for_test(facade);
        let message = MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "live row",
            None,
            None,
            None,
        )
        .expect("fixture transcript row is valid");

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let publisher_host = Arc::clone(&host);
        let publisher_stop = Arc::clone(&stop);
        let publisher = std::thread::spawn(move || {
            while !publisher_stop.load(std::sync::atomic::Ordering::Relaxed) {
                publisher_host.publish_content(&message);
            }
        });

        for request_id in 1..=32 {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
            let (close, _closed) = tokio::sync::watch::channel(false);
            let id = host
                .register_subscriber(session_id, run_id, sender, close, request_id)
                .expect("a current run admits one subscriber");
            let first = receiver
                .recv()
                .await
                .expect("the correlated reply reaches the subscriber");
            assert!(
                matches!(
                    first,
                    ProtocolDaemonMessageDto::Response(response)
                        if response.id() == Some(request_id)
                            && matches!(
                                response.result_value(),
                                Some(ProtocolResponsePayloadDto::RunSubscription(
                                    RunSubscriptionResponseDto::Snapshot(_)
                                ))
                            )
                ),
                "the correlated snapshot reply must precede every live frame"
            );
            host.remove_subscriber((session_id, run_id), id);
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        publisher.join().expect("the publisher thread joins");
    }

    #[test]
    fn normalize_tool_result_reports_search_truncation_in_durable_content() {
        // C-04: glob and grep durable content carries the same self-describing
        // truncation flag, so an honest byte-window cut survives persistence.
        let glob = ToolResult::Glob(PathsResult {
            paths: vec![
                intention_proto::WorkspaceRelativePathDto::parse("a.txt").expect("fixture path"),
            ],
            truncated: true,
        });
        let ToolResultOutcomeDto::Completed { content, .. } =
            normalize_tool_result(glob).expect("a glob result normalizes")
        else {
            panic!("a glob result succeeds")
        };
        assert_eq!(content, "{\"paths\":[\"a.txt\"],\"truncated\":true}");

        let grep = ToolResult::Grep(GrepResult {
            matches: Vec::new(),
            truncated: true,
        });
        let ToolResultOutcomeDto::Completed { content, .. } =
            normalize_tool_result(grep).expect("a grep result normalizes")
        else {
            panic!("a grep result succeeds")
        };
        assert_eq!(content, "{\"matches\":[],\"truncated\":true}");
    }

    #[test]
    fn partial_tool_result_carries_captured_output_and_an_interruption_notice() {
        let captured = ToolResult::Read(intention_tools::TextResult {
            text: intention_tools::BoundedText::new("half a line").expect("fixture text"),
            truncated: false,
        });
        let ToolResultOutcomeDto::Partial { content, .. } =
            partial_tool_result(true, Some(captured)).expect("a partial outcome normalizes")
        else {
            panic!("an interrupted call yields a partial outcome")
        };
        assert_eq!(
            content,
            "half a line\n[The tool call was stopped before a final result; the output above is partial.]"
        );

        let ToolResultOutcomeDto::Partial { content, .. } =
            partial_tool_result(false, None).expect("a lost partial outcome normalizes")
        else {
            panic!("an interrupted call yields a partial outcome")
        };
        assert_eq!(content, "[The tool call did not receive a final result.]");

        let ToolResultOutcomeDto::Partial { content, .. } =
            partial_tool_result(true, None).expect("a stopped partial outcome normalizes")
        else {
            panic!("an interrupted call yields a partial outcome")
        };
        assert_eq!(
            content,
            "[The tool call was stopped before a final result.]"
        );
    }

    #[test]
    fn daemon_tool_decoder_covers_every_advertised_model_visible_tool() {
        let advertised = intention_tools::model_visible_descriptors();
        for descriptor in &advertised {
            let name = descriptor.id().as_str();
            // Empty arguments may be rejected as invalid typed input, but an
            // advertised name must never be rejected as an unknown tool.
            if let Err(error) = ToolInput::from_arguments_json(name, "{}") {
                assert_ne!(
                    error.code(),
                    "unknown_tool",
                    "the daemon decoder rejects the advertised tool {name}"
                );
            }
        }
        let advertised_names: Vec<&str> = advertised
            .iter()
            .map(|descriptor| descriptor.id().as_str())
            .collect();
        for descriptor in intention_tools::registry() {
            let name = descriptor.id().as_str();
            if advertised_names.contains(&name) {
                continue;
            }
            let error = ToolInput::from_arguments_json(name, "{}")
                .expect_err("registered but unadvertised tools are not decodable");
            assert_eq!(
                error.code(),
                "unknown_tool",
                "the daemon decodes the unadvertised tool {name}"
            );
        }
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
}
