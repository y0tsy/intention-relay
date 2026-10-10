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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use intention_engine::{
    ApplicationService, ModelRunCommitDto, ModelRunCommitObserver, ModelRunExecutionService,
    ModelSleepFuture, ModelTextDeltaPort, ModelTimePort, RunCancellation, ToolExecutionPort,
    ToolInvocationRequestDto, ToolResultOutcomeDto, fail_starting_run,
};
use intention_proto::{
    ClientRequestDto, DtoResult, ErrorDto, InterruptRunCommandDto, RunId,
    RunSubscriptionSnapshotDto, SendUserTurnOutcomeDto, SessionId, SubscribeRunCommandDto,
    TimestampDto, ToolCallDto,
};
use intention_proto::{
    ProtocolDaemonMessageDto, ProtocolResultDto, RunStreamFrameDto, TextDeltaFrameDto,
    decode_request_line,
};
use intention_proto::{RunStatusDto, run_status_is_terminal};
use intention_storage::{RunOutcomeDto, StorageRepositoryDto};
use intention_tools::{ToolInput, WorkspaceRoot};
use intention_transport::{AsyncLocalListener, AsyncMessageSender, LocalEndpoint};

const SUBSCRIBER_QUEUE_CAPACITY: usize = 64;
const SUBSCRIBER_WRITE_DEADLINE: Duration = Duration::from_secs(10);
const PUBLICATION_RETRY_ATTEMPTS: usize = 6;
const PUBLICATION_RETRY_DELAY: Duration = Duration::from_millis(100);
/// The bounded wait for every registered run to return after a stop request.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

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
/// The bounded sender, the close signal, and the snapshot watermark identify
/// the subscriber; a connection holds at most one subscription, and the host
/// data lock is the one critical section covering registration, the correlated
/// reply, and publication, so no separate identity or gate is needed.
///
/// The watermark is the newest committed row the correlated snapshot carried.
/// It suppresses the one content frame that can repeat that row: a durable
/// commit and its publication are not one step, so a subscription that
/// registers between them receives the row in its snapshot and again as a live
/// frame. The watermark is consumed by the subscription's first content frame.
struct Subscriber {
    sender: tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
    close: tokio::sync::watch::Sender<bool>,
    watermark: Option<intention_proto::MessageProjectionDto>,
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
    ///
    /// The teardown boundary also releases every recorded completion signal, so
    /// the recorder holds no per-run state after a fixture takes its tasks.
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
        // The teardown boundary releases every recorded execution: the tasks
        // are taken, and the completion senders are dropped with them, so a
        // long-lived fixture host holds no per-run state past its teardown.
        if let Ok(mut completions) = self.completions.lock() {
            completions.clear();
        }
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
    /// Wakes a shutdown drain whenever one execution task deregisters.
    executions: tokio::sync::Notify,
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
        // The workspace root is immutable for the session, so it is resolved
        // once here and carried into every tool invocation of the run instead of
        // re-reading the whole session projection for each tool call.
        let workspace = self
            .facade
            .repository()
            .load_session_projection(session_id)
            .and_then(|projection| WorkspaceRoot::resolve(projection.workspace_root()));
        let Ok(workspace) = workspace else {
            drop(data);
            self.fail_unadmitted_starting_run(session_id, run_id);
            return;
        };
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
            // The run's provisional text has one window, one cadence task, and
            // one commit observer, and all three are dropped with the run.
            let deltas = Arc::new(RunTextDeltas::new(Arc::clone(&host), key));
            host.recorder
                .track(tokio::spawn(publish_text_deltas_at_cadence(Arc::clone(
                    &deltas,
                ))));
            let observer = HostCommitObserver {
                host: Arc::clone(&host),
                deltas: Arc::clone(&deltas),
            };
            // The model-run executor and the tool-invocation path share this
            // one commit sink, this one per-run cancellation handle, and the
            // session's resolved workspace root.
            let executor = DaemonToolExecutor::with_publication(
                host.facade.clone(),
                observer.clone(),
                cancellation,
                workspace,
            );
            let result = ModelRunExecutionService::new(
                host.facade.repository(),
                host.facade.driver(),
                &TokioTime,
                &observer,
                &executor,
            )
            .with_text_delta_port(deltas.as_ref())
            .execute(input)
            .await;
            deltas.finish();
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
                    host.on_terminal(key.0, key.1);
                }
            }
            if let Ok(mut data) = host.data.lock() {
                data.tasks.remove(&key);
            }
            host.executions.notify_waiters();
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
            self.on_terminal(session_id, run_id);
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

    /// Runs the terminal side effects for one run that just reached a durable
    /// terminal state: publish its terminal status frame, retrying a transient
    /// read failure boundedly, and schedule a current `Starting` successor
    /// exactly once.
    ///
    /// A status commit carries only the status value, so the terminal frame is
    /// built from a second durable read. A terminal commit has no guaranteed
    /// successor, so a transient failure of that read cannot be left to a later
    /// commit to fix: a live subscriber of the run would wait forever. The
    /// retry is bounded because a shutdown must never hang, and a reconnecting
    /// subscriber remains the ultimate fallback.
    fn on_terminal(self: &Arc<Self>, session_id: SessionId, run_id: RunId) {
        if !self.publish_current(session_id, run_id) {
            self.spawn_bounded_publication_retry(session_id, run_id);
        }
        if let Ok(Some(promoted)) = self.current_starting_run(session_id) {
            self.schedule_if_starting(session_id, promoted);
        }
    }

    /// Spawns one bounded publication retry worker for one terminal run.
    ///
    /// The worker republishes the exact terminal frame a bounded number of
    /// times, because no later commit of a terminal run exists to catch up
    /// from.
    fn spawn_bounded_publication_retry(self: &Arc<Self>, session_id: SessionId, run_id: RunId) {
        let host = Arc::clone(self);
        let task = tokio::spawn(async move {
            retry_bounded(PUBLICATION_RETRY_ATTEMPTS, PUBLICATION_RETRY_DELAY, || {
                host.publish_current(session_id, run_id)
            })
            .await;
        });
        self.recorder.track(task);
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
        let messages = composition::bounded_run_snapshot_messages(
            &run,
            repository.load_run_messages(
                session_id,
                run_id,
                composition::SESSION_SNAPSHOT_MESSAGES,
            )?,
        )?;
        RunSubscriptionSnapshotDto::new(run, messages)
    }

    /// Publishes the current committed run projection to live subscribers.
    ///
    /// A status commit carries only the status value, so the frame is built
    /// from the committed projection the transition just wrote. A repeated
    /// publication is a harmless duplicate: the subscriber replaces its state
    /// with the same committed value. Returns whether the projection was read
    /// and queued, so the terminal publication path can retry a transient read
    /// failure.
    fn publish_current(&self, session_id: SessionId, run_id: RunId) -> bool {
        let Ok(run) = self
            .facade
            .repository()
            .load_run_projection(session_id, run_id)
        else {
            return false;
        };
        self.broadcast(
            (session_id, run_id),
            ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Status(run)),
        );
        true
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
    /// re-reads current state on reconnect, and its channel handles are dropped
    /// only after the registry lock is released.
    ///
    /// A subscription's first content frame is suppressed when it repeats the
    /// newest committed row its correlated snapshot already carried, because
    /// the durable commit and its publication are not one step. Later rows are
    /// delivered unchanged: publication follows commit order, so at most that
    /// one row can be repeated.
    fn broadcast(&self, key: RunKey, message: ProtocolDaemonMessageDto) {
        let mut removed: Vec<Subscriber> = Vec::new();
        {
            let Ok(mut data) = self.data.lock() else {
                return;
            };
            let Some(subscribers) = data.subscribers.get_mut(&key) else {
                return;
            };
            let mut index = 0;
            while index < subscribers.len() {
                let remove = {
                    let subscriber = &mut subscribers[index];
                    let duplicate = match &message {
                        ProtocolDaemonMessageDto::Frame(RunStreamFrameDto::Content(row)) => {
                            subscriber.watermark.take().as_ref() == Some(row)
                        }
                        _ => false,
                    };
                    if duplicate {
                        false
                    } else if subscriber.sender.try_send(message.clone()).is_err() {
                        subscriber.close.send_replace(true);
                        true
                    } else {
                        false
                    }
                };
                if remove {
                    removed.push(subscribers.swap_remove(index));
                } else {
                    index += 1;
                }
            }
            if subscribers.is_empty() {
                data.subscribers.remove(&key);
            }
        }
        drop(removed);
    }

    /// Returns whether any live subscriber is registered for one run.
    ///
    /// Transient publication asks this before it retains anything, so a run
    /// nobody follows keeps no provisional text at all.
    fn has_subscribers(&self, key: RunKey) -> bool {
        self.data
            .lock()
            .is_ok_and(|data| data.subscribers.contains_key(&key))
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
            take_registration(&mut data, key, sender)
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
    /// The registration carries the snapshot's newest committed row as its
    /// watermark, so the one content frame that can repeat a row the snapshot
    /// already delivered is suppressed. The connection's previous registration
    /// is removed only after the new one is queued, so a refused re-subscribe
    /// leaves the run the connection was following untouched.
    ///
    /// Returns whether the connection is registered for the run.
    fn register_subscriber(
        self: &Arc<Self>,
        subscription: SubscribeRunCommandDto,
        sender: tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
        close: tokio::sync::watch::Sender<bool>,
        request_id: u64,
        previous: Option<RunKey>,
    ) -> bool {
        let session_id = subscription.session_id();
        let run_id = subscription.run_id();
        let key = (session_id, run_id);
        let Ok(mut data) = self.data.lock() else {
            // The registry is unavailable, so the peer must not be left waiting
            // for a reply that cannot arrive: an unqueueable rejection ends the
            // connection exactly like every other refused registration.
            if sender.try_send(subscriber_unavailable(request_id)).is_err() {
                close.send_replace(true);
            }
            return false;
        };
        let (reply, watermark) = match self.load_run_snapshot(session_id, run_id) {
            Ok(snapshot) => {
                let watermark = snapshot.messages().last().cloned();
                (
                    ProtocolDaemonMessageDto::reply(
                        request_id,
                        ProtocolResultDto::RunSubscribed(snapshot),
                    ),
                    watermark,
                )
            }
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
        let removed = previous.and_then(|previous| take_registration(&mut data, previous, &sender));
        data.subscribers.entry(key).or_default().push(Subscriber {
            sender,
            close,
            watermark,
        });
        drop(data);
        drop(removed);
        true
    }

    /// Stops every registered run and waits, boundedly, for each one to return.
    ///
    /// A stop request takes the same path as a user interrupt: every admitted
    /// run's cancellation handle is signalled, the engine commits the partial
    /// model answer it had produced together with the durable interruption
    /// notice, and the host then returns so the process can exit normally. The
    /// wait is bounded because a shutdown must never hang: a run that ignores
    /// its cancellation stays unfinished and keeps only the rows it managed to
    /// commit, which recovery reads as `Interrupted` on the next start.
    async fn shutdown_executions(&self) {
        let cancellations: Vec<RunCancellation> = match self.data.lock() {
            Ok(data) => data.tasks.values().cloned().collect(),
            Err(_) => return,
        };
        if cancellations.is_empty() {
            return;
        }
        for cancellation in &cancellations {
            cancellation.cancel();
        }
        let drain = async {
            loop {
                // The notification future is created before the registry is
                // read, so a run that deregisters in between cannot be missed.
                let deregistered = self.executions.notified();
                let pending = self.data.lock().map_or(0, |data| data.tasks.len());
                if pending == 0 {
                    return;
                }
                deregistered.await;
            }
        };
        let _drained = tokio::time::timeout(SHUTDOWN_DRAIN_TIMEOUT, drain).await;
    }
}

/// The publication cadence of one run's coalesced provisional text.
const RUN_TEXT_DELTA_INTERVAL: Duration = Duration::from_millis(50);

/// The retained bound of one run's pending provisional text.
///
/// The pending window is uncommitted, best-effort display state, so an over-long
/// step loses the rest of its provisional text instead of growing without
/// limit, and its committed row still carries the whole step. The bound stays
/// far below the single transport envelope cap, so a coalesced frame is always
/// writable.
const RUN_TEXT_DELTA_MAX_BYTES: usize = 8 * 1024;

/// The pending provisional text window of one run.
///
/// One window belongs to one model step: the first chunk of another step
/// replaces it, so a window never mixes two steps' text.
struct PendingText {
    step: u32,
    text: String,
    /// The current step crossed [`RUN_TEXT_DELTA_MAX_BYTES`].
    dropped: bool,
}

/// The transient text publication of one admitted run.
///
/// The engine's text-delta port fills one bounded window, this run's cadence
/// task publishes it at [`RUN_TEXT_DELTA_INTERVAL`], and this run's commit
/// observer drains it before it publishes the committed row that supersedes it,
/// so a subscriber never receives committed text behind its own provisional
/// text. Every step of this path is best-effort: text is never persisted, a run
/// without subscribers retains nothing, a slow subscriber's bounded queue drops
/// what it cannot hold, and no failure here can reach the run's durable state.
struct RunTextDeltas {
    host: Arc<HostState>,
    key: RunKey,
    /// The one window; its lock also orders a cadence publication against the
    /// flush that precedes one commit's publication.
    pending: Mutex<PendingText>,
    /// Set when the run's execution returned, ending its cadence task.
    finished: AtomicBool,
}

impl RunTextDeltas {
    /// Creates the empty transient publication of one run.
    const fn new(host: Arc<HostState>, key: RunKey) -> Self {
        Self {
            host,
            key,
            pending: Mutex::new(PendingText::new()),
            finished: AtomicBool::new(false),
        }
    }

    /// Publishes the pending text of the run, leaving the window empty.
    ///
    /// The window is drained and published under one lock acquisition, so the
    /// cadence task and this flush can never interleave: whatever the cadence
    /// task published is published before the flush that follows it.
    fn flush(&self) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        if pending.text.trim().is_empty() {
            // A step that produced only whitespace never becomes provisional
            // text, because a text delta must carry text.
            pending.text.clear();
            return;
        }
        let text = std::mem::take(&mut pending.text);
        if let Ok(delta) = TextDeltaFrameDto::new(self.key.0, self.key.1, pending.step, text) {
            self.host.broadcast(
                self.key,
                ProtocolDaemonMessageDto::frame(RunStreamFrameDto::TextDelta(delta)),
            );
        }
    }

    /// Reports that the run's execution returned, ending its cadence task.
    ///
    /// Whatever the run's last step had not published is discarded with it, so
    /// no transient frame follows the run's committed terminal state.
    fn finish(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.text.clear();
            pending.dropped = false;
        }
        self.finished.store(true, Ordering::Release);
    }

    /// Returns whether the run's execution returned.
    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

impl PendingText {
    /// Creates the empty pending window of one run.
    const fn new() -> Self {
        Self {
            step: 0,
            text: String::new(),
            dropped: false,
        }
    }
}

impl ModelTextDeltaPort for RunTextDeltas {
    fn text_delta(&self, step: u32, text: &str) {
        // Provisional text nobody follows is dropped instead of retained, so a
        // run keeps exactly the durable path it would have without this port.
        if text.is_empty() || !self.host.has_subscribers(self.key) {
            return;
        }
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        if pending.step != step {
            pending.step = step;
            pending.text.clear();
            pending.dropped = false;
        }
        if pending.dropped {
            return;
        }
        if pending.text.len() + text.len() > RUN_TEXT_DELTA_MAX_BYTES {
            // The over-long step loses its pending window and the rest of its
            // provisional text; its committed row still carries the whole step.
            pending.text.clear();
            pending.dropped = true;
            return;
        }
        pending.text.push_str(text);
    }
}

/// Publishes one run's coalesced provisional text at its cadence.
///
/// The task ends on the first tick after the run's execution returned, so it
/// never outlives its run by more than one cadence.
async fn publish_text_deltas_at_cadence(run: Arc<RunTextDeltas>) {
    while !run.is_finished() {
        tokio::time::sleep(RUN_TEXT_DELTA_INTERVAL).await;
        run.flush();
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

/// Removes one connection's registration of one run from the host registry.
///
/// Removal is by the connection's own channel identity, so no other subscriber
/// can be removed, and one connection registers at most one subscription, so at
/// most one entry can match. The removed subscriber is returned instead of being
/// dropped here, so its channel handles are released only after the registry
/// lock is gone.
fn take_registration(
    data: &mut HostData,
    key: RunKey,
    sender: &tokio::sync::mpsc::Sender<ProtocolDaemonMessageDto>,
) -> Option<Subscriber> {
    let subscribers = data.subscribers.get_mut(&key)?;
    let mut removed: Option<Subscriber> = None;
    let mut index = subscribers.len();
    while index > 0 {
        index -= 1;
        if subscribers[index].sender.same_channel(sender) {
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
}

/// Runs one bounded retry loop, stopping at the first success.
///
/// The attempt closure is called at most `attempts` times with `delay` between
/// the attempts, and the delay is awaited, so a retry never blocks a runtime
/// worker. Returns whether an attempt succeeded.
async fn retry_bounded(
    attempts: usize,
    delay: Duration,
    mut attempt: impl FnMut() -> bool,
) -> bool {
    for index in 0..attempts {
        if attempt() {
            return true;
        }
        if index + 1 < attempts {
            tokio::time::sleep(delay).await;
        }
    }
    false
}

/// Publishes committed transcript rows and run statuses to live subscribers.
///
/// The one commit sink serves both the model-run executor and the
/// tool-invocation path: a content frame carries the row the commit returned,
/// and a status frame carries the committed run projection the transition just
/// wrote, so no frame ever carries an uncommitted value. A content frame is
/// preceded by the provisional text of the run, which that committed row
/// supersedes.
#[derive(Clone)]
struct HostCommitObserver {
    host: Arc<HostState>,
    /// The transient text of the one run this observer commits for.
    deltas: Arc<RunTextDeltas>,
}

impl ModelRunCommitObserver for HostCommitObserver {
    fn observe_model_run_commit(&self, committed: &ModelRunCommitDto) {
        match committed {
            ModelRunCommitDto::Content(message) => {
                // Provisional text is published strictly before the committed
                // row of its step, so a subscriber replaces its provisional
                // text with the committed value and never the reverse.
                self.deltas.flush();
                self.host.publish_content(message);
            }
            ModelRunCommitDto::Status {
                session_id,
                run_id,
                status,
            } => {
                if run_status_is_terminal(*status) {
                    // A terminal commit has no guaranteed successor, so the
                    // terminal publication owns its own bounded retry.
                    self.host.on_terminal(*session_id, *run_id);
                } else {
                    self.host.publish_current(*session_id, *run_id);
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
    /// The session's workspace root, resolved once when the run was admitted.
    workspace: WorkspaceRoot,
}

impl<P: ModelRunCommitObserver + Clone + Send + Sync + 'static> DaemonToolExecutor<P> {
    /// Binds the durable composition, the host publication boundary, the exact
    /// run's single cancellation handle, and the session's resolved workspace
    /// root.
    #[must_use]
    const fn with_publication(
        facade: DaemonApplicationFacade,
        publisher: P,
        cancellation: RunCancellation,
        workspace: WorkspaceRoot,
    ) -> Self {
        Self {
            facade,
            publisher,
            cancellation,
            workspace,
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
        let workspace = self.workspace.clone();
        Box::pin(async move {
            let tool_id = call.name().to_owned();
            let call_id = call.call_id();
            let arguments = call.arguments_json().to_owned();
            let input = ToolInput::from_arguments_json(&tool_id, &arguments)?;
            tokio::task::spawn_blocking(move || {
                let repository = facade.repository();
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
        executions: tokio::sync::Notify::new(),
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

    /// Records the host's confirmation of one subscription attempt.
    ///
    /// The host replaces the connection's previous registration only as part of
    /// a successful registration, so a refused attempt leaves the registration
    /// the guard owns untouched and this records the new key only when the host
    /// confirmed it.
    const fn confirm(&mut self, key: RunKey, registered: bool) {
        if registered {
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

/// Serves the local wire until the environment asks the process to stop.
///
/// A stop request ends the accept loop, stops every registered run so the model
/// answer it had produced becomes durable, drains those runs under a bounded
/// wait, and then returns so the process exits normally.
async fn serve_async_listener(
    listener: AsyncLocalListener,
    facade: DaemonApplicationFacade,
) -> DtoResult<()> {
    let host = new_host(facade);
    // The stop future is created once and pinned across iterations: a signal
    // handler that is re-registered per accept could lose a request in the gap.
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let connection = accepted?;
                let host = Arc::clone(&host);
                tokio::spawn(async move {
                    serve_async_connection(connection, host).await;
                });
            }
            () = &mut shutdown => {
                host.shutdown_executions().await;
                return Ok(());
            }
        }
    }
}

/// Resolves when the environment asks the daemon process to stop.
///
/// The request is `SIGTERM` or `Ctrl-C` on Unix and `Ctrl-C` on every other
/// platform; the future never resolves otherwise, so the accept loop keeps
/// serving. A platform without a `SIGTERM` handler is not covered, which is why
/// the daemon also recovers unfinished runs when it next starts.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                // Without a `SIGTERM` handler the process still stops on the
                // interrupt signal, so the graceful path stays available.
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    return;
                }
            };
        tokio::select! {
            _ = terminate.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
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
                        let key = (subscription.session_id(), subscription.run_id());
                        let previous = registration.registered;
                        let registering_host = Arc::clone(&host);
                        let registering_sender = sender.clone();
                        let registering_close = close_sender.clone();
                        let unavailable_sender = sender.clone();
                        // Registration reads the run's durable snapshot, so it
                        // runs on a blocking worker instead of the connection's
                        // async task; the reply is still queued under the
                        // registry lock that orders it ahead of live frames.
                        let registered = tokio::task::spawn_blocking(move || {
                            registering_host.register_subscriber(
                                subscription,
                                registering_sender,
                                registering_close,
                                request_id,
                                previous,
                            )
                        })
                        .await
                        .unwrap_or_else(|_| {
                            // A registration task that failed cannot answer the
                            // peer, so the peer is answered exactly like an
                            // unavailable registry instead of being left to wait
                            // for a reply that cannot arrive.
                            if unavailable_sender
                                .try_send(subscriber_unavailable(request_id))
                                .is_err()
                            {
                                close_sender.send_replace(true);
                            }
                            false
                        });
                        registration.confirm(key, registered);
                        continue;
                    }
                    request => {
                        // A command reaches durable storage, so it runs on a
                        // blocking worker: a slow disk must not stall the
                        // connection's async task, and the per-connection queue
                        // still preserves reply-before-frame ordering.
                        let dispatched_host = Arc::clone(&host);
                        let dispatched = tokio::task::spawn_blocking(move || {
                            dispatch_request(&dispatched_host, request)
                        })
                        .await
                        .unwrap_or_else(|_| {
                            Err(ErrorDto::unavailable(
                                "daemon_command_unavailable",
                                "the daemon command task failed",
                            ))
                        });
                        match dispatched {
                            Ok(result) => ProtocolDaemonMessageDto::reply(request_id, result),
                            Err(error) => {
                                ProtocolDaemonMessageDto::rejection(Some(request_id), error)
                            }
                        }
                    }
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
        ClientRequestDto::ListSessions => host
            .facade
            .list_sessions(composition::SESSION_LIST_ROWS)
            .map(ProtocolResultDto::SessionsListed),
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

    /// Runs the production graceful-shutdown drain for this fixture host.
    ///
    /// A fixture keeps the same host its connection tasks serve, so this is
    /// exactly the production stop path: every registered run is cancelled and
    /// drained before it returns.
    #[doc(hidden)]
    pub async fn shutdown_gracefully(&self) {
        self.host.shutdown_executions().await;
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
        let (session_id, run_id) = create_and_start(&facade);
        // The bounded snapshot always keeps the newest committed row, so a row
        // larger than the transcript budget still makes the encoded reply exceed
        // the envelope cap: that response must carry the typed transport failure
        // instead of closing the connection silently. (Queued pending input can
        // no longer force this: the projection read is byte-bounded and reports
        // its omitted turns.)
        facade
            .repository()
            .append_message(
                MessageProjectionDto::new(
                    session_id,
                    Some(run_id),
                    MessageKindDto::Assistant,
                    "x".repeat(intention_transport::MAX_MESSAGE_BYTES),
                    None,
                    None,
                    None,
                )
                .expect("fixture assistant row is valid"),
                TimestampDto::from_unix_seconds(3).expect("fixture timestamp is valid"),
            )
            .expect("the large transcript row commits");
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
            None,
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
            None,
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
                        watermark: None,
                    },
                    Subscriber {
                        sender: healthy_sender,
                        close: healthy_close,
                        watermark: None,
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
                None,
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

        // The publisher thread publishes one frame per test tick instead of
        // spinning: an unyielding loop would starve the test runtime and burn
        // durable reads without adding ordering coverage.
        let (ticks, tick_receiver) = std::sync::mpsc::channel::<()>();
        let publisher_host = Arc::clone(&host);
        let publisher = std::thread::spawn(move || {
            while tick_receiver.recv().is_ok() {
                publisher_host.publish_content(&message);
            }
        });

        for request_id in 1..=32 {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
            let (close, _closed) = tokio::sync::watch::channel(false);
            let _ = ticks.send(());
            assert!(host.register_subscriber(
                SubscribeRunCommandDto::new(session_id, run_id),
                sender.clone(),
                close,
                request_id,
                None,
            ));
            let _ = ticks.send(());
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
        drop(ticks);
        publisher.join().expect("the publisher thread joins");
    }

    #[tokio::test(start_paused = true)]
    async fn retry_bounded_stops_at_the_first_success_and_gives_up_after_its_budget() {
        let attempts = std::cell::Cell::new(0usize);
        assert!(
            retry_bounded(3, Duration::from_millis(100), || {
                attempts.set(attempts.get() + 1);
                attempts.get() >= 3
            })
            .await,
            "the third attempt succeeds"
        );
        assert_eq!(attempts.get(), 3);

        let exhausted = std::cell::Cell::new(0usize);
        assert!(
            !retry_bounded(3, Duration::from_millis(100), || {
                exhausted.set(exhausted.get() + 1);
                false
            })
            .await,
            "an always-failing attempt closure exhausts its budget"
        );
        assert_eq!(exhausted.get(), 3);
    }

    #[tokio::test]
    async fn a_content_frame_duplicating_the_snapshot_row_is_suppressed_once() {
        // A durable commit and its publication are not one step, so a
        // subscription that registers between them receives the newest snapshot
        // row again as its first content frame; the watermark suppresses that
        // one frame while every later distinct row is delivered.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);
        let snapshot_row = MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "snapshot row",
            None,
            None,
            None,
        )
        .expect("fixture transcript row is valid");
        host.facade
            .repository()
            .append_message(
                snapshot_row.clone(),
                TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid"),
            )
            .expect("fixture snapshot row commits");

        let (sender, mut receiver) = tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (close, _closed) = tokio::sync::watch::channel(false);
        assert!(host.register_subscriber(
            SubscribeRunCommandDto::new(session_id, run_id),
            sender.clone(),
            close,
            1,
            None,
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Reply(reply))
                if matches!(
                    reply.result(),
                    ProtocolResultDto::RunSubscribed(snapshot)
                        if snapshot.messages().last() == Some(&snapshot_row)
                )
        ));

        host.publish_content(&snapshot_row);
        let distinct = MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "distinct row",
            None,
            None,
            None,
        )
        .expect("fixture transcript row is valid");
        host.publish_content(&distinct);
        let delivered = receiver
            .recv()
            .await
            .expect("the distinct row is delivered");
        assert!(matches!(
            delivered,
            ProtocolDaemonMessageDto::Frame(RunStreamFrameDto::Content(row))
                if row.text() == "distinct row"
        ));
        assert!(
            receiver.try_recv().is_err(),
            "the row the snapshot already carried is never queued"
        );
    }

    #[tokio::test]
    async fn a_terminal_status_frame_reaches_a_live_subscriber() {
        // A status commit carries only the status value, so the frame is built
        // from a second durable read; the terminal path owns that publication,
        // so a live subscriber of the run receives it without waiting for a
        // later commit that a terminal run can never produce.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (close, _closed) = tokio::sync::watch::channel(false);
        assert!(host.register_subscriber(
            SubscribeRunCommandDto::new(session_id, run_id),
            sender.clone(),
            close,
            1,
            None,
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Reply(_))
        ));

        host.fail_active_run(session_id, run_id, "fixture_terminal_failure")
            .expect("the fixture run terminalizes");
        host.on_terminal(session_id, run_id);
        let delivered = receiver
            .recv()
            .await
            .expect("the terminal status frame arrives");
        assert!(matches!(
            delivered,
            ProtocolDaemonMessageDto::Frame(RunStreamFrameDto::Status(run))
                if run.run_id() == run_id && run.status() == RunStatusDto::Failed
        ));
    }

    #[tokio::test]
    async fn a_rejected_resubscribe_keeps_the_previous_subscription() {
        // The previous registration is removed only after the new one is
        // queued, so a re-subscribe to a run that cannot be read leaves the run
        // the connection was following untouched.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let (close, _closed) = tokio::sync::watch::channel(false);
        assert!(host.register_subscriber(
            SubscribeRunCommandDto::new(session_id, run_id),
            sender.clone(),
            close.clone(),
            1,
            None,
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Reply(_))
        ));

        assert!(!host.register_subscriber(
            SubscribeRunCommandDto::new(session_id, RunId::new()),
            sender.clone(),
            close,
            2,
            Some((session_id, run_id)),
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Rejection(rejection))
                if rejection.id() == Some(2)
        ));
        assert_eq!(
            host.data
                .lock()
                .expect("host registry remains available")
                .subscribers
                .get(&(session_id, run_id))
                .map(Vec::len),
            Some(1),
            "the refused re-subscribe keeps the previous registration"
        );
        let run = host
            .facade
            .repository()
            .load_run_projection(session_id, run_id)
            .expect("fixture run reads");
        host.broadcast(
            (session_id, run_id),
            ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Status(run)),
        );
        assert!(matches!(
            receiver.recv().await,
            Some(ProtocolDaemonMessageDto::Frame(_))
        ));
    }

    #[tokio::test]
    async fn a_poisoned_registry_trips_the_close_signal_when_the_queue_rejects_the_rejection() {
        // The unavailable-registration path must not leave the peer waiting for
        // a reply the queue cannot carry.
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);
        let run = host
            .facade
            .repository()
            .load_run_projection(session_id, run_id)
            .expect("fixture run reads");
        let (sender, _receiver) = tokio::sync::mpsc::channel(1);
        let (close, mut closed) = tokio::sync::watch::channel(false);
        sender
            .try_send(ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Status(
                run,
            )))
            .expect("the single-slot queue accepts one frame");
        let _poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = host.data.lock().expect("host data locks");
            panic!("poison the registry lock");
        }));
        assert!(host.data.is_poisoned());

        assert!(
            !host.register_subscriber(
                SubscribeRunCommandDto::new(session_id, run_id),
                sender,
                close,
                3,
                None,
            ),
            "an unavailable registry refuses the subscription"
        );
        assert!(closed.changed().await.is_ok());
        assert!(
            *closed.borrow(),
            "the connection must be told to end instead of waiting silently"
        );
    }

    #[tokio::test]
    async fn take_tasks_releases_the_recorded_completions() {
        let (_directory, facade) = fixture_facade_with_driver(Arc::new(EmptyDriver));
        let (session_id, run_id) = create_and_start(&facade);
        let host = new_host(facade);
        host.recorder.register((session_id, run_id));
        assert!(host.recorder.completion((session_id, run_id)).is_some());

        let tasks = host.recorder.take_tasks();
        assert!(tasks.is_empty());
        assert!(
            host.recorder.completion((session_id, run_id)).is_none(),
            "the teardown boundary releases every recorded completion signal"
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
