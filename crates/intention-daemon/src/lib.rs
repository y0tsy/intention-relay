//! Thin daemon process host for the typed local wire.
//!
//! The daemon owns the local listener, typed connection hosting, and the one
//! scheduling path: an accepted turn is admitted by the composition root and
//! executed here, where the per-run registry, the single run cancellation
//! handle, and the one commit observer live. Health, session reads, and command
//! meaning stay in the durable composition root.

mod composition;

pub use composition::DaemonApplicationFacade;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use intention_engine::{
    ApplicationService, ModelRunCommitDto, ModelRunCommitObserver, ModelRunExecutionService,
    ModelSleepFuture, ModelTimePort, RunCancellation, ToolExecutionPort, ToolInvocationRequestDto,
    ToolResultOutcomeDto, fail_starting_run,
};
use intention_proto::{
    ClientRequestDto, DtoResult, ErrorDto, InterruptRunCommandDto, RunId,
    RunSubscriptionSnapshotDto, SendUserTurnOutcomeDto, SessionId, SubscribeRunCommandDto,
    TimestampDto, ToolCallDto,
};
use intention_proto::{
    ProtocolDaemonMessageDto, ProtocolResultDto, RunStreamFrameDto, decode_request_line,
};
use intention_proto::{RunStatusDto, run_status_is_terminal};
use intention_storage::{RunOutcomeDto, StorageRepositoryDto};
use intention_tools::{ToolInput, WorkspaceRoot};
use intention_transport::{AsyncLocalListener, AsyncMessageSender, LocalEndpoint};

const SUBSCRIBER_QUEUE_CAPACITY: usize = 64;
const SUBSCRIBER_WRITE_DEADLINE: Duration = Duration::from_secs(10);

type RunKey = (SessionId, RunId);

struct TokioTime;

impl ModelTimePort for TokioTime {
    fn now(&self) -> TimestampDto {
        composition::now().unwrap_or_else(|_| {
            TimestampDto::from_unix_seconds(0)
                .unwrap_or_else(|_| unreachable!("zero timestamp is valid"))
        })
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// One live subscription of one connection.
///
/// The bounded sender and the close signal identify the subscriber; a
/// connection holds at most one subscription, and the host data lock is the
/// one critical section covering registration, the correlated reply, and
/// publication, so no separate identity or gate is needed.
struct Subscriber {
    sender: tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
    close: tokio::sync::watch::Sender<bool>,
}

#[derive(Default)]
struct HostData {
    tasks: HashMap<RunKey, RunCancellation>,
    subscribers: HashMap<RunKey, Vec<Subscriber>>,
}

/// The one execution-recording seam of the daemon host.
///
/// Every admitted run registers its completion signal and hands its spawned
/// execution task to this seam. A production build records nothing: the durable
/// run registry and the one commit observer are all a production execution
/// needs, and a detached task needs no owner. A test or test-support build
/// records the per-run completion senders and the spawned task handles so a
/// fixture can wait for the exact admitted execution and can abort and join
/// every task it created. Only the seam implementation is conditional, so no
/// production shape carries fixture state.
trait ExecutionRecorder: Send + Sync {
    /// Registers the completion signal of one newly admitted run.
    fn register(&self, key: RunKey);

    /// Takes ownership of one spawned execution task.
    fn track(&self, task: tokio::task::JoinHandle<()>);

    /// Reports that the exact registered execution has returned.
    ///
    /// The recorded sender stays in the recorder so the unified terminalizer
    /// can report completion for runs it terminalizes after the executor task
    /// has already returned (PR24-013).
    fn signal(&self, key: RunKey);
}

/// Fixture-only inspection of the execution recorder.
///
/// It exists only in a test or test-support build, where the recorder keeps the
/// state a fixture waits on and aborts; a production recorder has nothing to
/// inspect.
#[cfg(any(test, feature = "test-support"))]
trait RecordedExecution: ExecutionRecorder {
    /// Returns a receiver of the exact run's completion signal.
    fn completion(&self, key: RunKey) -> Option<tokio::sync::watch::Receiver<bool>>;

    /// Returns every execution task this recorder still owns, leaving none.
    fn take_tasks(&self) -> Vec<tokio::task::JoinHandle<()>>;
}

/// The production execution recorder: a zero-sized no-op.
#[cfg(not(any(test, feature = "test-support")))]
#[derive(Default)]
struct NoopExecutionRecorder;

#[cfg(not(any(test, feature = "test-support")))]
impl ExecutionRecorder for NoopExecutionRecorder {
    fn register(&self, _key: RunKey) {}

    fn track(&self, _task: tokio::task::JoinHandle<()>) {}

    fn signal(&self, _key: RunKey) {}
}

/// The fixture execution recorder: keeps the completion sender of every
/// admitted run and the handle of every spawned execution task.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
struct RecordingExecutionRecorder {
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    completions: Mutex<HashMap<RunKey, tokio::sync::watch::Sender<bool>>>,
}

#[cfg(any(test, feature = "test-support"))]
impl ExecutionRecorder for RecordingExecutionRecorder {
    fn register(&self, key: RunKey) {
        if let Ok(mut completions) = self.completions.lock() {
            let (completion, _) = tokio::sync::watch::channel(false);
            completions.insert(key, completion);
        }
    }

    fn track(&self, task: tokio::task::JoinHandle<()>) {
        if let Ok(mut tasks) = self.tasks.lock() {
            tasks.push(task);
        } else {
            task.abort();
        }
    }

    fn signal(&self, key: RunKey) {
        if let Ok(completions) = self.completions.lock()
            && let Some(completion) = completions.get(&key)
        {
            let _ = completion.send_replace(true);
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl RecordedExecution for RecordingExecutionRecorder {
    fn completion(&self, key: RunKey) -> Option<tokio::sync::watch::Receiver<bool>> {
        self.completions
            .lock()
            .ok()
            .and_then(|completions| completions.get(&key).cloned())
            .map(|sender| sender.subscribe())
    }

    fn take_tasks(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let Ok(mut tasks) = self.tasks.lock() else {
            return Vec::new();
        };
        std::mem::take(&mut tasks)
    }
}

/// The execution recorder one host build uses.
#[cfg(not(any(test, feature = "test-support")))]
type HostRecorder = NoopExecutionRecorder;

#[cfg(any(test, feature = "test-support"))]
type HostRecorder = RecordingExecutionRecorder;

struct HostState {
    facade: DaemonApplicationFacade,
    data: Mutex<HostData>,
    recorder: HostRecorder,
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
        // The admitted run's cancellation handle is created here and embedded
        // in the execution input, so an interrupt and the executor share it.
        let cancellation = RunCancellation::new();
        let input = match ApplicationService::new(self.facade.repository()).schedule_starting_run(
            session_id,
            run_id,
            cancellation.clone(),
        ) {
            Ok(input) => input,
            Err(_) => {
                drop(data);
                self.fail_unadmitted_starting_run(session_id, run_id);
                return;
            }
        };
        let std::collections::hash_map::Entry::Vacant(entry) = data.tasks.entry(key) else {
            return;
        };
        entry.insert(cancellation.clone());
        drop(data);
        self.recorder.register(key);
        let host = Arc::clone(self);
        let task = tokio::spawn(async move {
            let observer = HostCommitObserver {
                host: Arc::clone(&host),
            };
            // The model-run executor and the tool-invocation path share this
            // one commit sink and this one per-run cancellation handle.
            let executor = DaemonToolExecutor::with_publication(
                host.facade.clone(),
                observer.clone(),
                cancellation,
            );
            let result = ModelRunExecutionService::new(
                host.facade.repository(),
                host.facade.driver(),
                &TokioTime,
                &observer,
                &executor,
            )
            .execute(input)
            .await;
            // An executor error must never leave a non-terminal durable run
            // without an owner (PR24-012/013): a still-active run is
            // terminalized as `Failed` with the executor's stable error code.
            if let Err(error) = result {
                let active = host
                    .facade
                    .repository()
                    .load_run_projection(key.0, key.1)
                    .map_or(true, |run| !run_status_is_terminal(run.status()));
                if active && host.fail_active_run(key.0, key.1, error.code()).is_ok() {
                    host.publish_current(key.0, key.1);
                    host.on_terminal(key.0);
                }
            }
            if let Ok(mut data) = host.data.lock() {
                data.tasks.remove(&key);
            }
            host.recorder.signal(key);
        });
        self.recorder.track(task);
    }

    /// Interrupts the current operation of one active run.
    ///
    /// The interrupt is accepted only for an exact active run; the run stays
    /// `Running`. The one registered run signal reaches the in-flight provider
    /// stream or tool call, which ends with a partial result and a context
    /// notice, and the model continues with the next step.
    fn interrupt_run(
        self: &Arc<Self>,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ProtocolResultDto> {
        let accepted = self
            .facade
            .interrupt_run(InterruptRunCommandDto::new(session_id, run_id))?;
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

    fn fail_unadmitted_starting_run(self: &Arc<Self>, session_id: SessionId, run_id: RunId) {
        if self
            .fail_starting_run(session_id, run_id, "model_scheduling_unavailable")
            .is_ok()
        {
            self.publish_current(session_id, run_id);
            self.on_terminal(session_id);
        }
    }

    /// Records a safe terminal scheduling failure for an exact unadmitted run.
    ///
    /// This preserves the already accepted user turn when durable context
    /// reconstruction cannot produce executable work.
    fn fail_starting_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        failure_code: &'static str,
    ) -> DtoResult<()> {
        self.facade.command_gate().run(|| {
            fail_starting_run(
                self.facade.repository(),
                session_id,
                run_id,
                failure_code,
                composition::now()?,
            )?;
            Ok(())
        })
    }

    /// Terminalizes one still-active run as durably `Failed`.
    ///
    /// An executor error must never leave a `Starting`/`Running` run without an
    /// owner: this commits the terminal `Failed` run row with the executor
    /// error's stable code, so deterministic bound and semantic failures (for
    /// example `reasoning_output_limit_exceeded`) become the durable failed
    /// outcome (PR24-012). Runs already terminal are a no-op.
    ///
    /// # Errors
    ///
    /// Returns the repository's typed error when the terminal run row cannot
    /// commit.
    fn fail_active_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        failure_code: &str,
    ) -> DtoResult<()> {
        self.facade.command_gate().run(|| {
            let repository = self.facade.repository();
            let run = repository.load_run_projection(session_id, run_id)?;
            if run_status_is_terminal(run.status()) {
                return Ok(());
            }
            let outcome = RunOutcomeDto::new(
                RunStatusDto::Failed,
                None,
                None,
                Some(failure_code.to_owned()),
                Some("the scheduled run execution failed".to_owned()),
            )?;
            repository.finish_run(session_id, run_id, outcome, composition::now()?)?;
            Ok(())
        })
    }

    /// Runs the terminal side effect for one run that just reached a durable
    /// terminal state: schedule a current `Starting` successor exactly once.
    ///
    /// The terminal status frame was already published from the committed
    /// projection, so no publication retry is required.
    fn on_terminal(self: &Arc<Self>, session_id: SessionId) {
        if let Ok(Some(promoted)) = self.current_starting_run(session_id) {
            self.schedule_if_starting(session_id, promoted);
        }
    }

    /// Returns the currently active durable run when it is eligible for admission.
    fn current_starting_run(&self, session_id: SessionId) -> DtoResult<Option<RunId>> {
        Ok(self
            .facade
            .repository()
            .load_session_projection(session_id)?
            .active_run()
            .filter(|run| run.status() == RunStatusDto::Starting)
            .map(|run| run.run_id()))
    }

    /// Loads one coherent current-state run snapshot.
    ///
    /// The snapshot carries the current run projection and the byte-bounded
    /// recent transcript rows of that run; a re-subscribing client receives
    /// current state and continues from live frames.
    fn load_run_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunSubscriptionSnapshotDto> {
        let repository = self.facade.repository();
        let run = repository.load_run_projection(session_id, run_id)?;
        let messages = composition::bounded_snapshot_messages(repository.load_run_messages(
            session_id,
            run_id,
            composition::SESSION_SNAPSHOT_MESSAGES,
        )?)?;
        RunSubscriptionSnapshotDto::new(run, messages)
    }

    /// Publishes the current committed run projection to live subscribers.
    ///
    /// A status commit carries only the status value, so the frame is built
    /// from the committed projection the transition just wrote. A repeated
    /// publication is a harmless duplicate: the subscriber replaces its state
    /// with the same committed value.
    fn publish_current(&self, session_id: SessionId, run_id: RunId) {
        if let Ok(run) = self
            .facade
            .repository()
            .load_run_projection(session_id, run_id)
        {
            self.broadcast(
                (session_id, run_id),
                ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Status(run)),
            );
        }
    }

    /// Publishes one committed transcript row to live subscribers.
    fn publish_content(&self, message: &intention_proto::MessageProjectionDto) {
        let Some(run_id) = message.run_id() else {
            return;
        };
        self.broadcast(
            (message.session_id(), run_id),
            ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Content(message.clone())),
        );
    }

    /// Queues one committed value for every subscriber of one run.
    ///
    /// Registration and publication share the host data lock, so a live frame
    /// can never enter a subscriber's queue before that subscription's
    /// correlated reply. A subscriber whose bounded queue overflowed is closed
    /// by position under the same lock instead of being resynchronized: it
    /// re-reads current state on reconnect.
    fn broadcast(&self, key: RunKey, message: ProtocolDaemonMessageDto) {
        let Ok(mut data) = self.data.lock() else {
            return;
        };
        let Some(subscribers) = data.subscribers.get_mut(&key) else {
            return;
        };
        subscribers.retain(|subscriber| {
            if subscriber.sender.try_send(message.clone()).is_err() {
                subscriber.close.send_replace(true);
                return false;
            }
            true
        });
        let empty = subscribers.is_empty();
        if empty {
            data.subscribers.remove(&key);
        }
    }

    /// Removes one connection's subscription from one run.
    ///
    /// Removal is by the connection's own channel identity: the guard that owns
    /// the registration passes the exact channel it registered, so no other
    /// subscriber can be removed. The removed subscriber is dropped only after
    /// the host data lock is released, so dropping its channel handles never
    /// runs under the registry lock.
    fn remove_subscriber(
        &self,
        key: RunKey,
        sender: &tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
    ) {
        let removed = {
            let Ok(mut data) = self.data.lock() else {
                return;
            };
            let Some(subscribers) = data.subscribers.get_mut(&key) else {
                return;
            };
            let mut removed: Option<Subscriber> = None;
            let mut index = subscribers.len();
            while index > 0 {
                index -= 1;
                if subscribers[index].sender.same_channel(sender) {
                    // One connection registers at most one subscription, so at
                    // most one entry can match this channel.
                    debug_assert!(
                        removed.is_none(),
                        "one subscriber channel is registered at most once"
                    );
                    removed = Some(subscribers.remove(index));
                }
            }
            if subscribers.is_empty() {
                data.subscribers.remove(&key);
            }
            removed
        };
        drop(removed);
    }

    /// Registers one connection's run subscription and queues its correlated reply.
    ///
    /// Registration, the reply, and every later publication share the host data
    /// lock, so a live frame can never enter the queue before the reply. The
    /// reply is queued first and the subscriber is registered after it under
    /// the same lock; when the bounded queue rejects the reply, the connection
    /// is told to end instead of being left to wait silently. A run whose
    /// current snapshot cannot be read is refused with its correlated typed
    /// rejection and registers nothing.
    ///
    /// Returns whether the connection is registered for the run.
    fn register_subscriber(
        self: &Arc<Self>,
        subscription: SubscribeRunCommandDto,
        sender: tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
        close: tokio::sync::watch::Sender<bool>,
        request_id: u64,
    ) -> bool {
        let session_id = subscription.session_id();
        let run_id = subscription.run_id();
        let key = (session_id, run_id);
        let Ok(mut data) = self.data.lock() else {
            let _ = sender.try_send(subscriber_unavailable(request_id));
            return false;
        };
        let reply = match self.load_run_snapshot(session_id, run_id) {
            Ok(snapshot) => ProtocolDaemonMessageDto::reply(
                request_id,
                ProtocolResultDto::RunSubscribed(snapshot),
            ),
            Err(error) => {
                // An unknown or unreadable run is refused with its correlated
                // typed error and registers nothing: a subscription that can
                // never receive a frame must not occupy the registry. A queue
                // that rejects even the typed rejection still ends the
                // connection instead of leaving the peer waiting silently.
                if sender
                    .try_send(ProtocolDaemonMessageDto::rejection(Some(request_id), error))
                    .is_err()
                {
                    close.send_replace(true);
                }
                return false;
            }
        };
        if sender.try_send(reply).is_err() {
            // The per-connection queue cannot deliver the correlated reply, so
            // the peer must not be left waiting silently: trip the close signal
            // exactly as the slow-subscriber path does and register nothing.
            close.send_replace(true);
            return false;
        }
        data.subscribers
            .entry(key)
            .or_default()
            .push(Subscriber { sender, close });
        true
    }
}

/// The typed rejection one connection receives when it cannot be registered.
fn subscriber_unavailable(request_id: u64) -> ProtocolDaemonMessageDto {
    ProtocolDaemonMessageDto::rejection(
        Some(request_id),
        ErrorDto::unavailable(
            "daemon_subscriber_unavailable",
            "the daemon subscriber is unavailable",
        ),
    )
}

/// Publishes committed transcript rows and run statuses to live subscribers.
///
/// The one commit sink serves both the model-run executor and the
/// tool-invocation path: a content frame carries the row the commit returned,
/// and a status frame carries the committed run projection the transition just
/// wrote, so no frame ever carries an uncommitted value.
#[derive(Clone)]
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
                self.host.publish_current(*session_id, *run_id);
                if run_status_is_terminal(*status) {
                    self.host.on_terminal(*session_id);
                }
            }
        }
    }
}

/// The one blocking tool seam.
///
/// The engine's model loop calls it; each call is decoded into the typed local
/// tool input and executed through the engine's durable local-tool lifecycle,
/// which hands every committed transcript row to the configured publication
/// boundary. The blocking effect runs on a spawned worker so the async
/// execution loop is never stalled.
#[derive(Clone)]
struct DaemonToolExecutor<P> {
    facade: DaemonApplicationFacade,
    publisher: P,
    cancellation: RunCancellation,
}

impl<P: ModelRunCommitObserver + Clone + Send + Sync + 'static> DaemonToolExecutor<P> {
    /// Binds the durable composition, the host publication boundary, and the
    /// exact run's single cancellation handle.
    #[must_use]
    const fn with_publication(
        facade: DaemonApplicationFacade,
        publisher: P,
        cancellation: RunCancellation,
    ) -> Self {
        Self {
            facade,
            publisher,
            cancellation,
        }
    }
}

impl<P: ModelRunCommitObserver + Clone + Send + Sync + 'static> ToolExecutionPort
    for DaemonToolExecutor<P>
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
        let cancellation = self.cancellation.clone();
        Box::pin(async move {
            let tool_id = call.name().to_owned();
            let call_id = call.call_id();
            let arguments = call.arguments_json().to_owned();
            let input = ToolInput::from_arguments_json(&tool_id, &arguments)?;
            tokio::task::spawn_blocking(move || {
                let repository = facade.repository();
                let workspace = WorkspaceRoot::resolve(
                    repository
                        .load_session_projection(session_id)?
                        .workspace_root(),
                )?;
                ApplicationService::new(repository).invoke_local_tool_with_publication(
                    ToolInvocationRequestDto::new(
                        workspace,
                        session_id,
                        run_id,
                        call_id,
                        tool_id,
                        input,
                        composition::now()?,
                    )
                    .with_arguments_json(arguments)
                    .with_cancellation(cancellation),
                    &publisher,
                )
            })
            .await
            .map_err(|_| {
                ErrorDto::unavailable(
                    "tool_execution_task_failed",
                    "the tool execution task failed",
                )
            })?
        })
    }
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
        recorder: HostRecorder::default(),
    })
}

/// The one RAII registration of one connection's run subscription.
///
/// The guard owns the exact run key the host registered this connection's
/// channel under, and it removes that registration when it is dropped, so every
/// connection exit path (closed channel, parse failure, close signal, over-size
/// frame, write failure) unregisters exactly once without repeating the removal
/// at each branch. Removal is by the connection's own channel identity under
/// the key the host confirmed, so the guard can never remove a subscriber it
/// does not own; `clear` is idempotent, and the removed subscriber's channel
/// handles are dropped only after the host data lock has been released.
struct SubscriptionGuard<'host> {
    host: &'host Arc<HostState>,
    sender: &'host tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
    registered: Option<RunKey>,
}

impl<'host> SubscriptionGuard<'host> {
    const fn new(
        host: &'host Arc<HostState>,
        sender: &'host tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
    ) -> Self {
        Self {
            host,
            sender,
            registered: None,
        }
    }

    /// Registers this connection's run subscription, replacing any previous one.
    ///
    /// The previous registration is removed first, so the host's registry never
    /// holds two entries for one connection and the guard always owns exactly
    /// the registration the host just confirmed.
    fn subscribe(
        &mut self,
        subscription: SubscribeRunCommandDto,
        close: tokio::sync::watch::Sender<bool>,
        request_id: u64,
    ) {
        let key = (subscription.session_id(), subscription.run_id());
        self.clear();
        if self
            .host
            .register_subscriber(subscription, self.sender.clone(), close, request_id)
        {
            self.registered = Some(key);
        }
    }

    /// Removes the current registration, if any; a second call does nothing.
    fn clear(&mut self) {
        if let Some(key) = self.registered.take() {
            self.host.remove_subscriber(key, self.sender);
        }
    }
}

impl Drop for SubscriptionGuard<'_> {
    fn drop(&mut self) {
        self.clear();
    }
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
    let (mut requests, mut messages) = connection.split();
    // One connection carries one role: typed requests arrive on the same NDJSON
    // stream that carries replies, rejections, and `run.frame` messages back.
    // The per-connection queue carries every subscriber message, so a
    // registration reply keeps its ordering ahead of live frames (PR24-014).
    let (sender, mut receiver) = tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
    let (close_sender, mut close_receiver) = tokio::sync::watch::channel(false);
    let mut registration = SubscriptionGuard::new(&host, &sender);
    loop {
        tokio::select! {
            changed = close_receiver.changed() => {
                if changed.is_err() || *close_receiver.borrow() {
                    return;
                }
            }
            line = requests.receive_line() => {
                let Ok(line) = line else {
                    return;
                };
                let request = match decode_request_line(&line) {
                    Ok(request) => request,
                    Err(error) => {
                        // A malformed or unsupported request is answered with a
                        // typed identity-less rejection, and the connection
                        // keeps serving.
                        let rejection = ProtocolDaemonMessageDto::rejection(None, error);
                        if !write_message_or_rejection(&mut messages, rejection).await {
                            return;
                        }
                        continue;
                    }
                };
                let request_id = request.id();
                let message = match request.into_request() {
                    ClientRequestDto::SubscribeRun(subscription) => {
                        registration.subscribe(subscription, close_sender.clone(), request_id);
                        continue;
                    }
                    request => match dispatch_request(&host, request) {
                        Ok(result) => ProtocolDaemonMessageDto::reply(request_id, result),
                        Err(error) => ProtocolDaemonMessageDto::rejection(Some(request_id), error),
                    },
                };
                if !write_message_or_rejection(&mut messages, message).await {
                    return;
                }
            }
            message = receiver.recv() => {
                let Some(message) = message else {
                    return;
                };
                if !write_message_or_rejection(&mut messages, message).await {
                    return;
                }
            }
        }
    }
}

/// Dispatches one decoded request against the shared composition and host registry.
fn dispatch_request(
    host: &Arc<HostState>,
    request: ClientRequestDto,
) -> DtoResult<ProtocolResultDto> {
    match request {
        ClientRequestDto::GetDaemonHealth => {
            Ok(ProtocolResultDto::DaemonHealth(host.facade.health()))
        }
        ClientRequestDto::GetSessionSnapshot(query) => host
            .facade
            .session_snapshot(query.session_id())
            .map(ProtocolResultDto::SessionSnapshot),
        ClientRequestDto::CreateSession(command) => host.facade.create_session(command),
        ClientRequestDto::SendUserTurn(command) => {
            let result = host.facade.send_user_turn(command)?;
            if let ProtocolResultDto::TurnAccepted(accepted) = &result
                && let SendUserTurnOutcomeDto::Started { run_id, .. } = accepted.outcome()
            {
                host.schedule_if_starting(accepted.session_id(), run_id);
            }
            Ok(result)
        }
        ClientRequestDto::RemoveTurn(command) => host.facade.remove_turn(command),
        ClientRequestDto::InterruptRun(command) => {
            host.interrupt_run(command.session_id(), command.run_id())
        }
        // A subscription is answered by the connection registration path, which
        // queues the correlated reply under the same lock that publishes.
        ClientRequestDto::SubscribeRun(_) => Err(ErrorDto::validation(
            "daemon_subscription_route_unavailable",
            "run subscriptions are answered by the connection registration path",
        )),
    }
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

/// Writes one daemon message, answering an over-size correlated reply with a
/// typed rejection.
///
/// An over-size reply fails to encode before any byte is written, so the
/// connection still carries the request correlation: the peer receives the
/// typed `local_protocol_message_too_large` failure instead of an unexplained
/// close. Any other write failure leaves the stream in an unknown state and
/// still ends the connection, because a queued frame is best effort and can
/// never stall persistence or another subscriber.
///
/// Returns whether the connection can continue serving.
async fn write_message_or_rejection(
    messages: &mut AsyncMessageSender,
    message: ProtocolDaemonMessageDto,
) -> bool {
    let correlated = match &message {
        ProtocolDaemonMessageDto::Reply(reply) => Some(reply.id()),
        ProtocolDaemonMessageDto::Rejection(_) | ProtocolDaemonMessageDto::Frame(_) => None,
    };
    match write_message_with_deadline(messages, message).await {
        Ok(()) => true,
        Err(failure) if failure.code() == "local_protocol_message_too_large" => {
            let Some(id) = correlated else {
                return false;
            };
            write_message_with_deadline(
                messages,
                ProtocolDaemonMessageDto::rejection(Some(id), failure),
            )
            .await
            .is_ok()
        }
        Err(_) => false,
    }
}

/// The one fixture host seam: owns a bounded host and every task it creates.
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
        let Some(mut completion) = self.host.recorder.completion((session_id, run_id)) else {
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
        let execution_tasks = self.host.recorder.take_tasks();
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
        ClientRequestDto, ConfigRevisionId, CreateSessionCommandDto, DaemonReadinessDto,
        GetSessionSnapshotQueryDto, IdempotencyKey, MessageKindDto, MessageProjectionDto,
        ProtocolDaemonMessageDto, ProtocolResultDto, RunModeDto, SendUserTurnCommandDto,
        SendUserTurnOutcomeDto, SessionId, SubscribeRunCommandDto, WorkspaceId, WorkspaceRootDto,
        decode_response, encode_request,
    };
    use intention_proto::{ProjectId, RunId, SchemaVersionDto, TimestampDto};
    use intention_providers::{
        FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelEventDto,
        ModelEventStream, ModelExecutionDriver,
    };
    use intention_transport::{
        AsyncLocalClientConnection, AsyncLocalListener, AsyncMessageReceiver, AsyncMessageSender,
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

    impl ModelExecutionDriver for EmptyDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }

        fn execute(
            &self,
            _request: intention_providers::ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> ModelEventStream {
            Box::pin(futures_util::stream::empty())
        }
    }

    struct CompletedDriver;

    impl ModelExecutionDriver for CompletedDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }

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

    impl ModelExecutionDriver for PendingDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }

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
        let created = facade
            .create_session(CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
                    .expect("fixture workspace is absolute"),
                RunModeDto::Build,
            ))
            .expect("fixture session creates");
        assert!(matches!(created, ProtocolResultDto::SessionCreated(_)));
        let accepted = facade
            .send_user_turn(
                SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "fixture turn")
                    .expect("fixture turn is valid"),
            )
            .expect("fixture turn is accepted");
        let ProtocolResultDto::TurnAccepted(turn) = accepted else {
            unreachable!("fixture result is a turn")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("fixture first turn starts")
        };
        (session_id, run_id)
    }

    /// Serves exactly one fixture peer through a host-owned connection task.
    async fn serve_one_test_connection(
        listener: AsyncLocalListener,
        facade: DaemonApplicationFacade,
    ) {
        let host = new_host(facade);
        let Ok(connection) = listener.accept().await else {
            return;
        };
        serve_async_connection(connection, host).await;
    }

    /// Connects one scripted client link and returns its two typed directions.
    async fn connect_fixture(
        endpoint: &LocalEndpoint,
    ) -> (AsyncMessageSender, AsyncMessageReceiver) {
        let connection = AsyncLocalClientConnection::connect(endpoint)
            .await
            .expect("fixture client connects");
        connection.split()
    }

    #[tokio::test]
    async fn one_connection_serves_requests_and_run_frames_together() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(CompletedDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(serve_one_test_connection(listener, facade));

        let (mut requests, mut messages) = connect_fixture(&endpoint).await;
        requests
            .send_message(&encode_request(
                1,
                ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, run_id)),
            ))
            .await
            .expect("subscription sends");
        let line = messages
            .receive_line()
            .await
            .expect("subscription reply arrives");
        // The correlated reply is the current durable state of this run.
        let ProtocolResultDto::RunSubscribed(snapshot) =
            decode_response(&line, 1).expect("subscription reply decodes")
        else {
            panic!("a run subscription answers with the current run snapshot")
        };
        assert_eq!(snapshot.run().session_id(), session_id);
        assert_eq!(snapshot.run().run_id(), run_id);

        // The same connection still serves an ordinary request while the
        // subscription is registered.
        requests
            .send_message(&encode_request(2, ClientRequestDto::GetDaemonHealth))
            .await
            .expect("health request sends");
        let line = messages.receive_line().await.expect("health reply arrives");
        assert!(matches!(
            decode_response(&line, 2),
            Ok(ProtocolResultDto::DaemonHealth(health))
                if health.readiness() == DaemonReadinessDto::Ready
        ));
        // The peer releases its socket so the host's serving loop observes
        // end-of-stream; only then does the awaited connection task finish.
        drop((requests, messages));
        server.await.expect("host serves the merged connection");
    }

    #[tokio::test]
    async fn a_closed_connection_leaves_no_subscriber_registered() {
        // The guard owns the registration: the graceful close path must remove
        // the subscriber exactly once, without any explicit removal in the loop.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let host = new_host(facade);
        let server = {
            let host = Arc::clone(&host);
            tokio::spawn(async move {
                let Ok(connection) = listener.accept().await else {
                    return;
                };
                serve_async_connection(connection, host).await;
            })
        };

        let (mut requests, mut messages) = connect_fixture(&endpoint).await;
        requests
            .send_message(&encode_request(
                1,
                ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, run_id)),
            ))
            .await
            .expect("subscription sends");
        let line = messages
            .receive_line()
            .await
            .expect("subscription reply arrives");
        assert!(matches!(
            decode_response(&line, 1),
            Ok(ProtocolResultDto::RunSubscribed(_))
        ));
        assert_eq!(
            host.data
                .lock()
                .expect("host registry remains available")
                .subscribers
                .get(&(session_id, run_id))
                .map(Vec::len),
            Some(1),
            "the connection registers exactly one subscriber"
        );

        drop((requests, messages));
        server.await.expect("host serves the closed peer");
        assert_eq!(
            host.data
                .lock()
                .expect("host registry remains available")
                .subscribers
                .get(&(session_id, run_id))
                .map_or(0, Vec::len),
            0,
            "a closed connection leaves no subscriber registered"
        );
    }

    #[tokio::test]
    async fn an_over_size_response_answers_with_a_typed_error_instead_of_closing() {
        let (_directory, facade) = fixture_facade();
        let (session_id, _run_id) = create_and_start(&facade);
        // Queued user input stays user input, so a snapshot projection can
        // legitimately grow past the envelope cap: that response must carry the
        // typed transport failure instead of closing the connection silently.
        for index in 0..6 {
            let accepted = facade
                .send_user_turn(
                    SendUserTurnCommandDto::new(
                        session_id,
                        IdempotencyKey::new(),
                        format!("pending {index} {}", "x".repeat(256 * 1024)),
                    )
                    .expect("fixture pending turn is valid"),
                )
                .expect("fixture pending turn is accepted");
            assert!(
                matches!(accepted, ProtocolResultDto::TurnAccepted(_)),
                "the fixture turn is durably accepted"
            );
        }
        let snapshot = facade
            .session_snapshot(session_id)
            .expect("the over-size snapshot reads in process");
        assert!(
            serde_json::to_vec(&snapshot)
                .expect("the over-size snapshot serializes")
                .len()
                > intention_transport::MAX_MESSAGE_BYTES,
            "the fixture must force one over-size response"
        );

        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(serve_one_test_connection(listener, facade));
        let (mut requests, mut messages) = connect_fixture(&endpoint).await;
        requests
            .send_message(&encode_request(
                1,
                ClientRequestDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id)),
            ))
            .await
            .expect("snapshot query sends");
        let line = messages
            .receive_line()
            .await
            .expect("the correlated typed rejection still arrives");
        assert_eq!(
            decode_response(&line, 1)
                .expect_err("the over-size reply is a typed rejection")
                .code(),
            "local_protocol_message_too_large"
        );
        // The refused peer releases its socket before the host task is awaited.
        drop((requests, messages));
        server.await.expect("host serves the refused peer");
    }

    #[tokio::test]
    async fn a_malformed_request_is_answered_with_a_typed_rejection_and_the_connection_continues() {
        // The typed wire keeps the one error contract: a request line that is
        // not a current typed request is answered with a typed rejection, and
        // the connection keeps serving instead of closing.
        let (_directory, facade) = fixture_facade();
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(serve_one_test_connection(listener, facade));
        let (mut requests, mut messages) = connect_fixture(&endpoint).await;

        requests
            .send_message(&"{\"id\":9,\"request\":{\"kind\":\"bogus\",\"data\":{}}}")
            .await
            .expect("unsupported request sends");
        let line = messages.receive_line().await.expect("rejection arrives");
        assert_eq!(
            decode_response(&line, 9)
                .expect_err("an unsupported request is rejected")
                .code(),
            "invalid_local_protocol_message"
        );

        requests
            .send_message(&"not a typed request".to_owned())
            .await
            .expect("malformed request sends");
        let line = messages.receive_line().await.expect("rejection arrives");
        assert_eq!(
            decode_response(&line, 10)
                .expect_err("a malformed request is rejected")
                .code(),
            "invalid_local_protocol_message"
        );

        // The same connection still serves the next valid request.
        requests
            .send_message(&encode_request(11, ClientRequestDto::GetDaemonHealth))
            .await
            .expect("health request sends");
        let line = messages.receive_line().await.expect("health reply arrives");
        assert!(matches!(
            decode_response(&line, 11),
            Ok(ProtocolResultDto::DaemonHealth(_))
        ));
        drop((requests, messages));
        server.await.expect("host serves the rejected peer");
    }

    #[test]
    fn duplicate_or_unknown_admission_never_creates_an_extra_task() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);
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
            .insert((session_id, run_id), RunCancellation::new());
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
        let host = new_host(facade);
        assert!(matches!(
            host.interrupt_run(session_id, run_id),
            Ok(ProtocolResultDto::RunInterrupted(value))
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
                .repository()
                .load_run_projection(session_id, run_id)
                .expect("starting run replay reads")
                .status(),
            RunStatusDto::Starting
        );
    }

    #[tokio::test]
    async fn host_interrupt_signals_the_registered_task_and_the_run_stays_active() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(PendingDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);
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
            if host
                .facade
                .repository()
                .load_run_projection(session_id, run_id)
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
            host.facade
                .repository()
                .load_run_projection(session_id, run_id)
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
        for task in host.recorder.take_tasks() {
            task.abort();
            let _ = task.await;
        }
    }

    #[tokio::test]
    async fn subscriber_admission_scope_errors_and_capacity_are_isolated() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);

        // An unknown run is refused with its typed error and is never registered.
        let (unknown_sender, mut unknown_receiver) =
            tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (unknown_close, _unknown_closed) = tokio::sync::watch::channel(false);
        assert!(!host.register_subscriber(
            SubscribeRunCommandDto::new(session_id, RunId::new()),
            unknown_sender,
            unknown_close,
            1,
        ));
        assert!(matches!(
            unknown_receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Rejection(rejection))
                if rejection.id() == Some(1)
                    && rejection.error().code() == "storage_record_not_found"
        ));

        // A current run admits one subscriber and answers with its snapshot.
        let (scoped_sender, mut scoped_receiver) =
            tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (scoped_close, _scoped_closed) = tokio::sync::watch::channel(false);
        assert!(host.register_subscriber(
            SubscribeRunCommandDto::new(session_id, run_id),
            scoped_sender.clone(),
            scoped_close,
            2,
        ));
        assert!(matches!(
            scoped_receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Reply(reply))
                if reply.id() == 2
                    && matches!(
                        reply.result(),
                        ProtocolResultDto::RunSubscribed(snapshot)
                            if snapshot.run().run_id() == run_id
                    )
        ));
        host.remove_subscriber((session_id, run_id), &scoped_sender);

        // A slow subscriber is closed on overflow while a healthy sibling keeps
        // receiving frames and the run keeps exactly one registration.
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
                        sender: slow_sender,
                        close: slow_close,
                    },
                    Subscriber {
                        sender: healthy_sender,
                        close: healthy_close,
                    },
                ],
            );
        }
        let run = host
            .facade
            .repository()
            .load_run_projection(session_id, run_id)
            .expect("fixture run reads");
        let frame = ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Status(run));
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
        let host = new_host(facade);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let (close, mut closed) = tokio::sync::watch::channel(false);
        let run = host
            .facade
            .repository()
            .load_run_projection(session_id, run_id)
            .expect("fixture run reads");
        sender
            .try_send(ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Status(
                run,
            )))
            .expect("the single-slot queue accepts one frame");

        assert!(
            !host.register_subscriber(
                SubscribeRunCommandDto::new(session_id, run_id),
                sender,
                close,
                7,
            ),
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
                .map_or(0, Vec::len),
            0,
            "the failed registration leaves no subscriber registered"
        );
        assert!(matches!(
            receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Frame(_))
        ));
    }

    #[tokio::test]
    async fn a_correlated_subscription_reply_precedes_every_live_frame() {
        // The subscription reply is the correlated current-state snapshot, so
        // it is the first message a new subscriber receives even while the run
        // is publishing committed frames concurrently. Registration and
        // publication therefore share the one host data lock.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);
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
            assert!(host.register_subscriber(
                SubscribeRunCommandDto::new(session_id, run_id),
                sender.clone(),
                close,
                request_id,
            ));
            let first = receiver
                .recv()
                .await
                .expect("the correlated reply reaches the subscriber");
            assert!(
                matches!(
                    first,
                    ProtocolDaemonMessageDto::Reply(reply)
                        if reply.id() == request_id
                            && matches!(reply.result(), ProtocolResultDto::RunSubscribed(_))
                ),
                "the correlated snapshot reply must precede every live frame"
            );
            host.remove_subscriber((session_id, run_id), &sender);
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        publisher.join().expect("the publisher thread joins");
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
        for name in [
            "fetch_url",
            "ask_user",
            "todo",
            "retrieve",
            "plan_submit",
            "sub_agent",
            "expand",
            "mcp",
        ] {
            let error = ToolInput::from_arguments_json(name, "{}")
                .expect_err("unexposed tools are not decodable");
            assert_eq!(
                error.code(),
                "unknown_tool",
                "the daemon decodes the unexposed tool {name}"
            );
        }
    }
}
