//! Typed, bounded contracts for workspace tools and the workspace addressing
//! anchor.

use intention_proto::{DtoResult, WorkspaceRelativePathDto};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, PoisonError,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

mod workspace;

pub use workspace::WorkspaceRoot;

#[cfg(test)]
#[allow(clippy::expect_used, reason = "deterministic timeout fixture setup")]
mod timeout_tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn timeout_is_classified_as_a_lost_interruption_without_waiting_thirty_seconds() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "3", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("1");
            command
        };
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = command.spawn().expect("spawn timeout fixture");
        let started = Instant::now();
        let result = bounded_output_with_timeout(
            child,
            CancellationSignal::new(),
            Duration::from_millis(25),
        );
        assert!(matches!(
            result,
            Err(ExecuteFailure::Interrupted {
                cause: InterruptCause::Lost,
                ..
            })
        ));
        // The deadline bound is the controlling invariant: the tool must
        // return promptly (far below the thirty-second execute default) even
        // when the pipe readers are descheduled by a loaded machine; the
        // assertion is deliberately looser than the two-second drain grace.
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn background_descendant_holding_pipes_cannot_exceed_the_deadline() {
        use std::os::unix::process::CommandExt;
        let marker = std::env::temp_dir().join(format!(
            "intention-relay-exec-descendant-{}.pid",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&marker);
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!(
                "sleep 300 & echo $! > '{}'",
                marker.to_string_lossy()
            ))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.process_group(0);
        let child = command.spawn().expect("spawn descendant fixture");
        let started = Instant::now();
        let result = bounded_output_with_timeout(
            child,
            CancellationSignal::new(),
            Duration::from_millis(50),
        );
        assert!(matches!(
            result,
            Err(ExecuteFailure::Interrupted {
                cause: InterruptCause::Lost,
                ..
            })
        ));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "a descendant holding the pipes open must not extend the deadline"
        );
        // The process-group termination must also kill the background
        // descendant, not only the direct child.
        let pid_text = std::fs::read_to_string(&marker).unwrap_or_default();
        let _ = std::fs::remove_file(&marker);
        if let Ok(descendant) = pid_text.trim().parse::<i32>()
            && let Some(pid) = rustix::process::Pid::from_raw(descendant)
        {
            let mut alive = true;
            for _ in 0..40 {
                alive = rustix::process::test_kill_process(pid).is_ok();
                if !alive {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            assert!(
                !alive,
                "the background descendant must be terminated with its process group"
            );
        }
    }
}

const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound on the serialized bytes of one search result, shared by glob
/// and grep. Per-line fragments and per-entry path lists alone still allow a
/// very large aggregate result; every retained entry is charged against this
/// one window and the cut is reported through the result's own truncation
/// flag, so durable, normalized content stays bounded (PR24-022, C-04).
const MAX_GREP_AGGREGATE_BYTES: usize = 128 * 1024;
/// Upper bound on one edit target or write expected-content source file.
/// Larger files can be read (truncated) but never edited or equality-checked,
/// which keeps edit reads and write preflights bounded (PR24-022).
const MAX_EDIT_TARGET_BYTES: usize = 1024 * 1024;

/// Typed cancellation signal for one tool invocation.
#[derive(Clone, Debug, Default)]
pub struct CancellationSignal {
    cancelled: Arc<AtomicBool>,
}

impl CancellationSignal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    #[must_use]
    pub fn cancelled() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(true)),
        }
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    /// Requests cancellation of this invocation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    /// Clears the request so the same run-scoped signal observes the next
    /// interruption with fresh state.
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::Release);
    }
}

pub const TOOL_SCHEMA_VERSION: u16 = 1;

/// Typed terminal classification for one executed program.
///
/// Per the external-attempt taxonomy, a known non-zero exit or known signal
/// termination is a normalized program result carried on the typed output, not
/// a transport-level tool failure. Lost terminal evidence (cancellation,
/// timeout) classifies as an interrupted dispatch whose captured output, when
/// any, stays partial.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolProcessStatus {
    Success,
    NonZero { code: i32 },
    Signal { signal: i32 },
}

impl ToolProcessStatus {
    /// Classifies a finished child-process status.
    #[must_use]
    pub fn classify(status: std::process::ExitStatus) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                return Self::Signal { signal };
            }
        }
        match status.code() {
            Some(0) => Self::Success,
            Some(code) => Self::NonZero { code },
            // A reaped child always reports either a recorded signal (checked
            // above on Unix) or a numeric exit code (Windows always reports
            // one); a status with neither cannot occur.
            None => unreachable!("child status has neither signal nor exit code"),
        }
    }
}

fn bounded_lossy(bytes: &[u8]) -> (String, bool) {
    if bytes.len() <= MAX_TOOL_OUTPUT_BYTES {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let mut end = MAX_TOOL_OUTPUT_BYTES;
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    (
        format!("{}\n[truncated]", String::from_utf8_lossy(&bytes[..end])),
        true,
    )
}

fn bounded_text(value: String) -> DtoResult<BoundedText> {
    BoundedText::new(value)
}

#[derive(Debug)]
struct BoundedOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn bounded_output(
    child: Child,
    cancellation: CancellationSignal,
) -> Result<BoundedOutput, ExecuteFailure> {
    bounded_output_with_timeout(child, cancellation, EXECUTE_TIMEOUT)
}

/// How long pipe readers may keep draining without observed progress after the
/// direct child has been reaped or terminated. Descendants holding the pipes
/// open are killed with the child's process group on Unix; on every platform
/// the collection is deadline-bounded so Execute can never wait forever
/// (PR24-011). The window tolerates reader-thread descheduling on loaded,
/// instrumented machines; a reader that keeps consuming bytes is descheduled
/// rather than stalled, so observed progress extends the window up to the
/// thirty-second execute deadline instead of failing the command.
const READER_DRAIN_GRACE: Duration = Duration::from_secs(5);

/// Shared progress state between the pipe readers and the drain loop.
///
/// The readers publish every consumed byte and record their own exit, and the
/// drain waits on the paired condvar for the remaining stall window, so an
/// idle drain blocks until the window runs out and a busy one wakes on the
/// progress signal instead of on a timer.
#[derive(Default)]
struct ReaderProgress {
    state: Mutex<ReaderState>,
    wake: Condvar,
}

/// The counter state guarded by the [`ReaderProgress`] mutex.
#[derive(Default)]
struct ReaderState {
    /// Total bytes every reader has consumed.
    consumed: u64,
    /// Readers that stopped, whether they returned or panicked.
    stopped: usize,
}

impl ReaderProgress {
    /// Locks the shared state, ignoring poisoning: the state is a plain
    /// counter, so a panicking thread cannot leave it inconsistent.
    fn lock(&self) -> MutexGuard<'_, ReaderState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Total bytes consumed so far.
    fn consumed(&self) -> u64 {
        self.lock().consumed
    }

    /// Number of readers that stopped.
    fn stopped(&self) -> usize {
        self.lock().stopped
    }

    /// Publishes consumed bytes and wakes a waiting drain, saturating on the
    /// platforms where one read can exceed the counter's range.
    ///
    /// The counter is updated under the lock and the notification follows the
    /// release: a drain that checked the counter before the update is already
    /// registered as a waiter, so the wakeup cannot be lost.
    fn publish(&self, count: usize) {
        let mut state = self.lock();
        state.consumed = state
            .consumed
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        drop(state);
        self.wake.notify_all();
    }

    /// Records that one reader stopped and wakes a waiting drain.
    fn record_stop(&self) {
        let mut state = self.lock();
        state.stopped += 1;
        drop(state);
        self.wake.notify_all();
    }

    /// Blocks until progress is published, a reader stops, or `window`
    /// elapses, and returns the consumed total observed on return.
    ///
    /// The progress check and the wait hold the same lock, so progress
    /// published since `observed` returns immediately instead of after the
    /// window; the window bounds the wait, so an idle drain blocks for exactly
    /// the remaining stall window rather than waking on a timer.
    fn wait_for_activity(&self, observed: u64, window: Duration) -> u64 {
        let mut state = self.lock();
        if state.consumed <= observed {
            let (waited, _) = self
                .wake
                .wait_timeout(state, window)
                .unwrap_or_else(PoisonError::into_inner);
            state = waited;
        }
        state.consumed
    }
}

/// Wakes the drain when its reader thread stops.
///
/// The drain may collect a reader only once that reader stopped, and the OS
/// reports the stop through [`thread::JoinHandle::is_finished`] only after the
/// closure returned, so this guard publishes the exit from inside the reader
/// (a panic unwind included) and closes that window.
struct ReaderStop(Arc<ReaderProgress>);

impl Drop for ReaderStop {
    fn drop(&mut self) {
        self.0.record_stop();
    }
}

/// Counts the bytes a pipe reader has consumed.
///
/// The drain loop compares consecutive readings to separate a slow reader from
/// a stalled one: a descendant that inherited the pipes but writes nothing
/// produces no progress, while a reader on a loaded machine keeps advancing.
struct ProgressReader<R> {
    inner: R,
    progress: Arc<ReaderProgress>,
}

impl<R: std::io::Read> std::io::Read for ProgressReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.progress.publish(count);
        Ok(count)
    }
}

/// Raw output captured before a program was stopped, with its truncation flag.
struct PartialOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    truncated: bool,
}

impl PartialOutput {
    /// Keeps whatever the pipe drain collected; a stalled drain has no bytes.
    fn from_drain(drain: PipeDrain) -> Option<Self> {
        let PipeDrain::Complete { stdout, stderr } = drain else {
            return None;
        };
        let (stdout, stdout_truncated) = stdout.unwrap_or_default();
        let (stderr, stderr_truncated) = stderr.unwrap_or_default();
        Some(Self {
            stdout,
            stderr,
            truncated: stdout_truncated || stderr_truncated,
        })
    }

    /// Renders the captured bytes exactly like a completed execute result,
    /// without the exit status line that only a finished program has.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the rendered text violates the bounded
    /// text contract.
    fn into_result(self) -> DtoResult<ToolResult> {
        let (stdout, _) = bounded_lossy(&self.stdout);
        let (stderr, _) = bounded_lossy(&self.stderr);
        let text = format!(
            "stdout:\n{stdout}\nstderr:\n{stderr}{}",
            if self.truncated { "\n[truncated]" } else { "" }
        );
        Ok(ToolResult::Execute(TextResult {
            text: BoundedText::new(text)?,
            truncated: self.truncated,
        }))
    }
}

/// Why one bounded program collection stopped without a final result.
enum ExecuteFailure {
    /// A pipe reader failed or panicked.
    ReadFailed,
    /// The program was interrupted; captured bytes are kept when available.
    Interrupted {
        cause: InterruptCause,
        partial: Option<PartialOutput>,
    },
}

/// Classifies one observed interruption: an explicit cancellation is a stop,
/// anything else (deadline, stalled drain, failed wait) is lost evidence.
fn interruption_cause(cancellation: &CancellationSignal) -> InterruptCause {
    if cancellation.is_cancelled() {
        InterruptCause::Stopped
    } else {
        InterruptCause::Lost
    }
}

fn bounded_output_with_timeout(
    mut child: Child,
    cancellation: CancellationSignal,
    timeout: Duration,
) -> Result<BoundedOutput, ExecuteFailure> {
    let child_id = child.id();
    let progress = Arc::new(ReaderProgress::default());
    let stdout = child.stdout.take().map(|pipe| {
        let progress = Arc::clone(&progress);
        thread::spawn(move || {
            let _stop = ReaderStop(Arc::clone(&progress));
            let mut output = Vec::new();
            let mut reader = std::io::BufReader::new(ProgressReader {
                inner: pipe,
                progress,
            });
            read_bounded(&mut reader, &mut output).map(|truncated| (output, truncated))
        })
    });
    let stderr = child.stderr.take().map(|pipe| {
        let progress = Arc::clone(&progress);
        thread::spawn(move || {
            let _stop = ReaderStop(Arc::clone(&progress));
            let mut output = Vec::new();
            let mut reader = std::io::BufReader::new(ProgressReader {
                inner: pipe,
                progress,
            });
            read_bounded(&mut reader, &mut output).map(|truncated| (output, truncated))
        })
    });
    let deadline = Instant::now() + timeout;
    loop {
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            // The direct child is killed and, on Unix, its whole process
            // group, so a backgrounded descendant that inherited the pipes
            // cannot keep the reader threads alive after the tool returns.
            let cause = interruption_cause(&cancellation);
            terminate_process_tree(&mut child, child_id);
            let _ = child.wait();
            // The interruption is already observed: the drain joins the
            // killed readers within its existing bound instead of bailing on
            // the signal, so their captured output survives as the partial
            // result whenever the pipes were collected.
            let drained = drain_pipes(
                stdout,
                stderr,
                &progress,
                READER_DRAIN_GRACE,
                deadline,
                None,
            );
            return Err(ExecuteFailure::Interrupted {
                cause,
                partial: PartialOutput::from_drain(drained),
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // The direct child is reaped, but a descendant may still hold
                // the output pipes open. Collect while the readers keep making
                // progress, within the grace bound; a drain that stalls is
                // killed and classified as a lost interruption so Execute
                // still returns within its deadline.
                match drain_pipes(
                    stdout,
                    stderr,
                    &progress,
                    READER_DRAIN_GRACE,
                    deadline,
                    Some(&cancellation),
                ) {
                    PipeDrain::Complete { stdout, stderr } => {
                        let (stdout, stdout_was_truncated) =
                            stdout.map_err(|_| ExecuteFailure::ReadFailed)?;
                        let (stderr, stderr_was_truncated) =
                            stderr.map_err(|_| ExecuteFailure::ReadFailed)?;
                        return Ok(BoundedOutput {
                            status,
                            stdout,
                            stderr,
                            stdout_truncated: stdout_was_truncated,
                            stderr_truncated: stderr_was_truncated,
                        });
                    }
                    PipeDrain::Stalled => {
                        let cause = interruption_cause(&cancellation);
                        terminate_process_tree(&mut child, child_id);
                        let _ = child.wait();
                        return Err(ExecuteFailure::Interrupted {
                            cause,
                            partial: None,
                        });
                    }
                }
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let cause = interruption_cause(&cancellation);
                terminate_process_tree(&mut child, child_id);
                let _ = child.wait();
                let drained = drain_pipes(
                    stdout,
                    stderr,
                    &progress,
                    READER_DRAIN_GRACE,
                    deadline,
                    None,
                );
                return Err(ExecuteFailure::Interrupted {
                    cause,
                    partial: PartialOutput::from_drain(drained),
                });
            }
        }
    }
}

/// Kills the direct child and, on Unix, its whole process group.
///
/// The execute path spawns the child as the leader of its own process group,
/// so this also terminates background descendants that inherited the output
/// pipes. A missing group (ESRCH) is harmless: the direct kill covers the
/// child. The kill is performed through the safe `rustix` process API, never
/// through raw `unsafe` syscall blocks, so the workspace `unsafe_code` deny
/// stays intact (PR24-011).
///
/// The child identifier is consumed only by the Unix process-group kill, so
/// the parameter keeps an underscore prefix for the platforms where it is
/// unused.
fn terminate_process_tree(child: &mut Child, _child_id: u32) {
    let _ = child.kill();
    #[cfg(unix)]
    if let Some(pid) = rustix::process::Pid::from_raw(i32::try_from(_child_id).unwrap_or(0)) {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
}

/// Collected reader results once both pipe readers have finished.
enum PipeDrain {
    /// Both readers completed; an `Err` records a reader failure.
    Complete {
        stdout: Result<(Vec<u8>, bool), &'static str>,
        stderr: Result<(Vec<u8>, bool), &'static str>,
    },
    /// At least one reader was still pending at the deadline.
    Stalled,
}

/// Joins both pipe readers until the stall window or `deadline`.
///
/// A reader that keeps consuming bytes is descheduled rather than stalled, so
/// observed progress re-arms the stall window up to `deadline`; a reader that
/// stops making progress for the whole window is treated as a descendant
/// holding the pipes open. The wait is woken by the readers' progress signal
/// instead of a timer, and the remaining stall window bounds it. The success
/// path passes the invocation's signal so a cancellation ends the drain early;
/// an interrupted collection passes `None`, because the interruption is
/// already observed and the readers killed with their process tree must be
/// joined so their captured output survives as the partial result.
fn drain_pipes(
    mut stdout: Option<ReaderHandle>,
    mut stderr: Option<ReaderHandle>,
    progress: &ReaderProgress,
    stall: Duration,
    deadline: Instant,
    cancellation: Option<&CancellationSignal>,
) -> PipeDrain {
    // An absent reader is already collected: the joined pair must complete even
    // when a caller pipes only one of the two streams.
    let mut stdout_result = stdout.is_none().then_some(Ok((Vec::new(), false)));
    let mut stderr_result = stderr.is_none().then_some(Ok((Vec::new(), false)));
    // Every spawned reader records its own stop, so the drain can collect the
    // handles the moment the last reader stopped rather than when the OS
    // observes the thread as finished.
    let readers = usize::from(stdout.is_some()) + usize::from(stderr.is_some());
    let mut observed = progress.consumed();
    let mut until = (Instant::now() + stall).min(deadline);
    loop {
        let stopped = progress.stopped() >= readers;
        if let Some(handle) = stdout.take() {
            if stopped || handle.is_finished() {
                stdout_result = Some(join_reader(Some(handle)));
            } else {
                stdout = Some(handle);
            }
        }
        if let Some(handle) = stderr.take() {
            if stopped || handle.is_finished() {
                stderr_result = Some(join_reader(Some(handle)));
            } else {
                stderr = Some(handle);
            }
        }
        match (stdout_result.take(), stderr_result.take()) {
            (Some(stdout), Some(stderr)) => return PipeDrain::Complete { stdout, stderr },
            partial => {
                // Both readers must be collected before the drain completes, so
                // a partially collected pair keeps its gathered side.
                (stdout_result, stderr_result) = partial;
            }
        }
        if cancellation.is_some_and(CancellationSignal::is_cancelled) || Instant::now() >= until {
            return PipeDrain::Stalled;
        }
        // Progress or a reader stop wakes the wait at once; with neither, the
        // drain blocks for the remaining stall window instead of polling.
        let current =
            progress.wait_for_activity(observed, until.saturating_duration_since(Instant::now()));
        if current > observed {
            observed = current;
            until = (Instant::now() + stall).min(deadline);
        }
    }
}

#[cfg(test)]
mod drain_progress_tests {
    use super::*;

    #[test]
    fn reader_progress_extends_the_drain_beyond_the_stall_window() {
        let progress = Arc::new(ReaderProgress::default());
        let writer = Arc::clone(&progress);
        let handle = thread::spawn(move || -> Result<(Vec<u8>, bool), &'static str> {
            let _stop = ReaderStop(Arc::clone(&writer));
            // Bytes keep arriving well past the stall window: the loaded-machine
            // descheduling case that must not be classified as a stalled drain.
            for _ in 0..8 {
                thread::sleep(Duration::from_millis(20));
                writer.publish(64);
            }
            Ok((vec![b'x'; 64], false))
        });
        let drain = drain_pipes(
            Some(handle),
            None,
            &progress,
            Duration::from_millis(50),
            Instant::now() + Duration::from_secs(5),
            None,
        );
        assert!(matches!(drain, PipeDrain::Complete { .. }));
    }

    #[test]
    fn reader_stop_wakes_the_drain_before_the_stall_window() {
        let progress = Arc::new(ReaderProgress::default());
        let reader = Arc::clone(&progress);
        let handle = thread::spawn(move || -> Result<(Vec<u8>, bool), &'static str> {
            let _stop = ReaderStop(reader);
            // The reader stops long before the stall window expires: the drain
            // must collect it on the wakeup, not by outwaiting the window.
            thread::sleep(Duration::from_millis(50));
            Ok((Vec::new(), false))
        });
        let started = Instant::now();
        let drain = drain_pipes(
            Some(handle),
            None,
            &progress,
            Duration::from_secs(5),
            started + Duration::from_secs(30),
            None,
        );
        assert!(matches!(drain, PipeDrain::Complete { .. }));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn reader_without_progress_stalls_at_the_window() {
        let progress = Arc::new(ReaderProgress::default());
        let handle = thread::spawn(move || -> Result<(Vec<u8>, bool), &'static str> {
            // Alive and silent, like a descendant that inherited the pipes.
            thread::sleep(Duration::from_secs(30));
            Ok((Vec::new(), false))
        });
        let started = Instant::now();
        let drain = drain_pipes(
            Some(handle),
            None,
            &progress,
            Duration::from_millis(50),
            started + Duration::from_secs(30),
            None,
        );
        assert!(matches!(drain, PipeDrain::Stalled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

#[cfg(test)]
mod process_failure_tests {
    use super::*;
    use std::io::{self, Read};

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected read failure"))
        }
    }

    struct PanicReader;

    impl Read for PanicReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            std::panic::resume_unwind(Box::new("injected reader panic"))
        }
    }

    #[test]
    fn read_failure_is_classified_as_read_failure() {
        let mut output = Vec::new();
        assert_eq!(
            read_bounded(&mut FailingReader, &mut output),
            Err("tool_execute_read_failed")
        );
    }

    #[test]
    fn reader_join_failure_is_classified_as_read_failure() {
        let reader = thread::spawn(|| -> Result<(Vec<u8>, bool), &'static str> {
            Err("tool_execute_read_failed")
        });
        assert_eq!(join_reader(Some(reader)), Err("tool_execute_read_failed"));
    }

    #[test]
    fn reader_panic_is_classified_as_read_failure() {
        let reader = thread::spawn(|| {
            let mut reader = PanicReader;
            let mut output = Vec::new();
            read_bounded(&mut reader, &mut output).map(|truncated| (output, truncated))
        });
        assert_eq!(join_reader(Some(reader)), Err("tool_execute_read_failed"));
    }
}

type ReaderHandle = thread::JoinHandle<Result<(Vec<u8>, bool), &'static str>>;

fn join_reader(reader: Option<ReaderHandle>) -> Result<(Vec<u8>, bool), &'static str> {
    reader
        .map(|reader| reader.join().map_err(|_| "tool_execute_read_failed")?)
        .transpose()
        .map(|value| value.unwrap_or_default())
}

/// Outcome of a bounded whole-file read.
enum LimitedReadOutcome {
    /// The file fit within the bound and was read completely.
    Content(Vec<u8>),
    /// The file exceeds the bound; no content is retained.
    TooLarge,
    /// The file could not be opened or read.
    Unreadable,
}

/// Reads a whole file only when it fits within `limit` bytes.
///
/// Oversized files are detected without allocating or slurping their content,
/// which keeps edit and expected-content comparisons bounded (PR24-022).
fn read_limited(path: &std::path::Path, limit: usize) -> LimitedReadOutcome {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return LimitedReadOutcome::Unreadable,
    };
    let mut bytes = Vec::new();
    if file.take(limit as u64 + 1).read_to_end(&mut bytes).is_err() {
        return LimitedReadOutcome::Unreadable;
    }
    if bytes.len() > limit {
        return LimitedReadOutcome::TooLarge;
    }
    LimitedReadOutcome::Content(bytes)
}

fn read_bounded(
    reader: &mut impl std::io::Read,
    output: &mut Vec<u8>,
) -> Result<bool, &'static str> {
    let mut buffer = [0_u8; 4096];
    let mut total = 0;
    // Truncation means bytes were actually dropped: a source ending exactly at
    // the bound stays untruncated.
    let mut truncated = false;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "tool_execute_read_failed")?;
        if count == 0 {
            return Ok(truncated);
        }
        if truncated {
            // Drain the remainder to EOF so oversized sources still finish
            // cleanly instead of stalling writers mid-stream.
            continue;
        }
        let remaining = MAX_TOOL_OUTPUT_BYTES.saturating_sub(total);
        let kept = count.min(remaining);
        output.extend_from_slice(&buffer[..kept]);
        total += kept;
        truncated = count > kept;
    }
}

/// The fixed set of tools exposed by the product boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolId {
    Read,
    Write,
    Edit,
    Execute,
    Glob,
    Grep,
}

impl ToolId {
    /// Returns the stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::Execute => "execute",
        }
    }
}
impl Display for ToolId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Static specification of one tool.
///
/// The specification carries the three strings the model boundary consumes:
/// the tool identity, its description, and the JSON Schema document text for
/// its typed model arguments. The typed input DTO is the decode authority and
/// the typed result DTO is the result contract, so no result schema is kept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolSpec {
    id: ToolId,
    description: &'static str,
    input_schema: &'static str,
}
impl ToolSpec {
    /// Returns the tool this specification describes.
    #[must_use]
    pub const fn id(self) -> ToolId {
        self.id
    }
    #[must_use]
    pub const fn description(self) -> &'static str {
        self.description
    }
    /// Returns the JSON Schema document text for this tool's typed model
    /// arguments.
    #[must_use]
    pub const fn input_schema(self) -> &'static str {
        self.input_schema
    }
}

/// JSON Schema for the `read` tool's typed model arguments.
pub const READ_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Workspace-relative path of the file to read."
    }
  },
  "required": ["path"]
}"#;

/// JSON Schema for the `write` tool's typed model arguments.
pub const WRITE_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Workspace-relative path of the file to write."
    },
    "content": {
      "type": "string",
      "description": "Bounded text to write to the file."
    },
    "expected_content": {
      "type": "string",
      "description": "Optional guard: the write applies only while the file holds exactly this text."
    }
  },
  "required": ["path", "content"]
}"#;

/// JSON Schema for the `edit` tool's typed model arguments.
pub const EDIT_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Workspace-relative path of the file to edit."
    },
    "old": {
      "type": "string",
      "description": "Existing text to replace."
    },
    "new": {
      "type": "string",
      "description": "Replacement text."
    },
    "expected_content": {
      "type": "string",
      "description": "Optional guard: the edit applies only while the file holds exactly this text."
    }
  },
  "required": ["path", "old", "new"]
}"#;

/// JSON Schema for the `execute` tool's typed model arguments.
pub const EXECUTE_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "program": {
      "type": "string",
      "description": "Program to run directly, without shell interpretation."
    },
    "args": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "Arguments passed verbatim to the program."
    }
  },
  "required": ["program", "args"]
}"#;

/// JSON Schema for the `glob` tool's typed model arguments.
pub const GLOB_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "pattern": {
      "type": "string",
      "description": "Glob pattern matched against workspace-relative paths."
    }
  },
  "required": ["pattern"]
}"#;

/// JSON Schema for the `grep` tool's typed model arguments.
pub const GREP_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "pattern": {
      "type": "string",
      "description": "Text pattern to search for inside workspace files."
    },
    "scope": {
      "type": "object",
      "properties": {
        "kind": {
          "type": "string",
          "enum": ["file", "directory", "workspace"],
          "description": "Search scope kind."
        },
        "path": {
          "type": "string",
          "description": "Workspace-relative path required by the file and directory scope kinds."
        }
      },
      "required": ["kind"]
    },
    "path": {
      "type": "string",
      "description": "Optional workspace-relative path narrowing the search."
    }
  },
  "required": ["pattern"]
}"#;

/// Returns the static specification of one tool.
#[must_use]
pub const fn spec(id: ToolId) -> ToolSpec {
    match id {
        ToolId::Read => ToolSpec {
            id,
            description: "Read bounded text from a workspace file.",
            input_schema: READ_INPUT_SCHEMA,
        },
        ToolId::Write => ToolSpec {
            id,
            description: "Write bounded text to a workspace file.",
            input_schema: WRITE_INPUT_SCHEMA,
        },
        ToolId::Edit => ToolSpec {
            id,
            description: "Apply a bounded text replacement.",
            input_schema: EDIT_INPUT_SCHEMA,
        },
        ToolId::Execute => ToolSpec {
            id,
            description: "Execute an explicitly bounded command.",
            input_schema: EXECUTE_INPUT_SCHEMA,
        },
        ToolId::Glob => ToolSpec {
            id,
            description: "List workspace paths matching a pattern.",
            input_schema: GLOB_INPUT_SCHEMA,
        },
        ToolId::Grep => ToolSpec {
            id,
            description: "Search bounded workspace text.",
            input_schema: GREP_INPUT_SCHEMA,
        },
    }
}

/// Returns the model-visible tool specifications in advertisement order.
#[must_use]
pub fn model_visible_descriptors() -> Vec<ToolSpec> {
    vec![
        spec(ToolId::Read),
        spec(ToolId::Write),
        spec(ToolId::Edit),
        spec(ToolId::Execute),
        spec(ToolId::Glob),
        spec(ToolId::Grep),
    ]
}

/// Bounded text accepted by tool contracts.
///
/// Deserialization is validating: JSON arriving at the daemon tool boundary
/// cannot bypass the constructor's NUL check (PR24-023).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BoundedText(String);
impl BoundedText {
    /// # Errors
    ///
    /// Returns a validation error when the text contains a NUL byte.
    pub fn new(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        if value.contains('\0') {
            Err(intention_proto::ErrorDto::validation(
                "invalid_tool_text",
                "tool text contains NUL",
            ))
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BoundedText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(|error| {
            serde::de::Error::custom(format!("invalid tool text ({})", error.code()))
        })
    }
}

/// Typed tool input family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", content = "input", rename_all = "snake_case")]
pub enum ToolInput {
    Read(ReadInput),
    Glob(GlobInput),
    Grep(GrepInput),
    Write(WriteInput),
    Edit(EditInput),
    Execute(ExecuteInput),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReadInput {
    pub path: WorkspaceRelativePathDto,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GlobInput {
    pub pattern: BoundedText,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepInput {
    pub pattern: BoundedText,
    #[serde(default)]
    pub scope: Option<GrepScope>,
    #[serde(default)]
    pub path: Option<WorkspaceRelativePathDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrepScope {
    File { path: WorkspaceRelativePathDto },
    Directory { path: WorkspaceRelativePathDto },
    Workspace,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteInput {
    pub path: WorkspaceRelativePathDto,
    pub content: BoundedText,
    #[serde(default)]
    pub expected_content: Option<BoundedText>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditInput {
    pub path: WorkspaceRelativePathDto,
    pub old: BoundedText,
    pub new: BoundedText,
    #[serde(default)]
    pub expected_content: Option<BoundedText>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecuteInput {
    pub program: BoundedText,
    pub args: Vec<BoundedText>,
}

impl ToolInput {
    /// Decodes raw model arguments into the typed input of one exposed tool.
    ///
    /// The wire name is the decoding authority: only the six exposed tools
    /// decode, each through its own typed input DTO; every other name is
    /// rejected as unknown, so a tool the product does not expose is never
    /// reachable through raw model arguments.
    ///
    /// # Errors
    ///
    /// Returns `unknown_tool` when the name is not an exposed tool, and
    /// `invalid_tool_input_json` when the arguments are malformed or do not
    /// match the tool's typed input.
    pub fn from_arguments_json(tool_id: &str, arguments_json: &str) -> DtoResult<Self> {
        let input = match tool_id {
            "read" => serde_json::from_str::<ReadInput>(arguments_json).map(Self::Read),
            "write" => serde_json::from_str::<WriteInput>(arguments_json).map(Self::Write),
            "edit" => serde_json::from_str::<EditInput>(arguments_json).map(Self::Edit),
            "execute" => serde_json::from_str::<ExecuteInput>(arguments_json).map(Self::Execute),
            "glob" => serde_json::from_str::<GlobInput>(arguments_json).map(Self::Glob),
            "grep" => serde_json::from_str::<GrepInput>(arguments_json).map(Self::Grep),
            _ => return Err(unknown_tool()),
        };
        input.map_err(|_| {
            intention_proto::ErrorDto::validation(
                "invalid_tool_input_json",
                "tool arguments are not valid typed input",
            )
        })
    }

    /// Returns the concrete tool this input belongs to.
    #[must_use]
    pub const fn tool_id(&self) -> ToolId {
        match self {
            Self::Read(_) => ToolId::Read,
            Self::Glob(_) => ToolId::Glob,
            Self::Grep(_) => ToolId::Grep,
            Self::Write(_) => ToolId::Write,
            Self::Edit(_) => ToolId::Edit,
            Self::Execute(_) => ToolId::Execute,
        }
    }
}

/// Returns the stable error for a name the product does not expose as a tool.
fn unknown_tool() -> intention_proto::ErrorDto {
    intention_proto::ErrorDto::validation(
        "unknown_tool",
        "tool is not registered for model invocation",
    )
}

/// Typed tool result family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
pub enum ToolResult {
    Read(TextResult),
    Glob(PathsResult),
    Grep(GrepResult),
    Write(WriteResult),
    Edit(WriteResult),
    Execute(TextResult),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextResult {
    pub text: BoundedText,
    pub truncated: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepResult {
    pub matches: Vec<GrepMatch>,
    /// Whether a dropped read window, an oversized line, or the retained
    /// serialized-match window cut content; the byte window, not a match
    /// count, bounds the result.
    #[serde(default)]
    pub truncated: bool,
}
/// One workspace-relative match; its serialized size is charged against the
/// shared search-result window.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepMatch {
    pub path: WorkspaceRelativePathDto,
    pub line: u64,
    pub column: u64,
    pub fragment: BoundedText,
}
/// Workspace-relative path list produced by a glob search.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathsResult {
    /// Paths retained inside the shared search-result window.
    pub paths: Vec<WorkspaceRelativePathDto>,
    /// Whether the byte window cut further matching paths.
    #[serde(default)]
    pub truncated: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteResult {
    pub bytes: u64,
}

/// Renders one typed tool result into its bounded model-visible content.
///
/// The typed result is redacted and workspace-relative by construction: text
/// and search payloads keep their own bounds, truncated content keeps its
/// explicit marker, and mutations report their byte count.
///
/// # Errors
///
/// Returns a validation error when the rendered content is blank, because a
/// tool result must always answer its call with readable content.
pub fn render_tool_result_content(result: &ToolResult) -> DtoResult<String> {
    let content = match result {
        ToolResult::Read(value) | ToolResult::Execute(value) => {
            if value.truncated {
                format!("{}\n[truncated]", value.text.as_str())
            } else {
                value.text.as_str().to_owned()
            }
        }
        ToolResult::Glob(value) => {
            let mut content = value
                .paths
                .iter()
                .map(WorkspaceRelativePathDto::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            append_truncation_marker(&mut content, value.truncated);
            content
        }
        ToolResult::Grep(value) => {
            let mut content = value
                .matches
                .iter()
                .map(|matched| {
                    format!(
                        "{}:{}:{}: {}",
                        matched.path.as_str(),
                        matched.line,
                        matched.column,
                        matched.fragment.as_str()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            append_truncation_marker(&mut content, value.truncated);
            content
        }
        ToolResult::Write(value) | ToolResult::Edit(value) => format!("{} bytes", value.bytes),
    };
    if content.trim().is_empty() {
        return Err(intention_proto::ErrorDto::validation(
            "invalid_tool_result_content",
            "tool result content must not be empty",
        ));
    }
    Ok(content)
}

/// Appends the honest truncation marker to one bounded list projection.
fn append_truncation_marker(content: &mut String, truncated: bool) {
    if truncated {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str("[truncated]");
    }
}

/// Renders the durable partial result of one interrupted dispatch.
///
/// The captured output precedes the exact interruption notice selected by doc
/// 15 for the stopped/lost and captured/uncaptured cases.
///
/// # Errors
///
/// Returns a validation error when the captured output cannot render.
pub fn partial_tool_result_content(
    stopped: bool,
    result: Option<&ToolResult>,
) -> DtoResult<String> {
    let notice = match (stopped, result.is_some()) {
        (true, true) => {
            "[The tool call was stopped before a final result; the output above is partial.]"
        }
        (false, true) => {
            "[The tool call did not receive a final result; the output above is partial.]"
        }
        (true, false) => "[The tool call was stopped before a final result.]",
        (false, false) => "[The tool call did not receive a final result.]",
    };
    match result {
        Some(result) => Ok(format!("{}\n{notice}", render_tool_result_content(result)?)),
        None => Ok(notice.to_owned()),
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Renderer fixtures use expect to provide precise failures."
)]
mod tool_result_renderer_tests {
    use super::*;

    fn bounded(value: &str) -> BoundedText {
        BoundedText::new(value).unwrap_or_else(|_| unreachable!("fixture tool text is bounded"))
    }

    fn relative(value: &str) -> WorkspaceRelativePathDto {
        WorkspaceRelativePathDto::parse(value)
            .unwrap_or_else(|_| unreachable!("fixture relative path is valid"))
    }

    #[test]
    fn tool_result_content_covers_each_typed_result_family() {
        let read = ToolResult::Read(TextResult {
            text: bounded("hello"),
            truncated: false,
        });
        assert_eq!(
            render_tool_result_content(&read).expect("read content renders"),
            "hello"
        );
        let truncated = ToolResult::Execute(TextResult {
            text: bounded("done"),
            truncated: true,
        });
        assert_eq!(
            render_tool_result_content(&truncated).expect("execute content renders"),
            "done\n[truncated]"
        );
        let glob = ToolResult::Glob(PathsResult {
            paths: vec![relative("src/a.rs"), relative("src/b.rs")],
            truncated: true,
        });
        assert_eq!(
            render_tool_result_content(&glob).expect("glob content renders"),
            "src/a.rs\nsrc/b.rs\n[truncated]"
        );
        let grep = ToolResult::Grep(GrepResult {
            matches: vec![GrepMatch {
                path: relative("src/a.rs"),
                line: 3,
                column: 5,
                fragment: bounded("needle"),
            }],
            truncated: false,
        });
        assert_eq!(
            render_tool_result_content(&grep).expect("grep content renders"),
            "src/a.rs:3:5: needle"
        );
        let write = ToolResult::Write(WriteResult { bytes: 17 });
        assert_eq!(
            render_tool_result_content(&write).expect("write content renders"),
            "17 bytes"
        );
        let edit = ToolResult::Edit(WriteResult { bytes: 2 });
        assert_eq!(
            render_tool_result_content(&edit).expect("edit content renders"),
            "2 bytes"
        );
    }

    #[test]
    fn blank_tool_result_content_is_rejected() {
        let read = ToolResult::Read(TextResult {
            text: bounded(""),
            truncated: false,
        });
        let error = render_tool_result_content(&read).expect_err("blank content is rejected");
        assert_eq!(error.code(), "invalid_tool_result_content");
    }

    #[test]
    fn partial_content_carries_the_exact_interruption_notice() {
        let captured = ToolResult::Read(TextResult {
            text: bounded("half a line"),
            truncated: false,
        });
        assert_eq!(
            partial_tool_result_content(true, Some(&captured)).expect("partial content renders"),
            "half a line\n[The tool call was stopped before a final result; the output above is partial.]"
        );
        assert_eq!(
            partial_tool_result_content(false, Some(&captured)).expect("partial content renders"),
            "half a line\n[The tool call did not receive a final result; the output above is partial.]"
        );
        assert_eq!(
            partial_tool_result_content(true, None).expect("partial content renders"),
            "[The tool call was stopped before a final result.]"
        );
        assert_eq!(
            partial_tool_result_content(false, None).expect("partial content renders"),
            "[The tool call did not receive a final result.]"
        );
    }
}

/// Why one tool dispatch stopped before producing a final result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptCause {
    /// Cancellation was observed while the tool was running.
    Stopped,
    /// The tool lost its process evidence: deadline, stalled pipes, or a
    /// failed wait probe.
    Lost,
}

/// Outcome of one admitted tool dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolDispatchOutcome {
    /// The tool produced its final typed result.
    Completed(ToolResult),
    /// The tool stopped before a final result; `partial` carries the output
    /// captured before the interruption when the pipes were collected.
    Interrupted {
        cause: InterruptCause,
        partial: Option<ToolResult>,
    },
}

/// Returns the interrupted outcome for one cooperatively observed stop.
///
/// Every workspace tool checks its invocation signal between I/O steps, so a
/// stop that arrives while the tool runs produces a partial result instead of
/// an unbounded effect or a lost observation.
const fn stopped_outcome(partial: Option<ToolResult>) -> ToolDispatchOutcome {
    ToolDispatchOutcome::Interrupted {
        cause: InterruptCause::Stopped,
        partial,
    }
}

/// Local execution service rooted at an authorized workspace.
pub struct ToolService {
    root: WorkspaceRoot,
}
impl ToolService {
    /// Creates a service.
    #[must_use]
    pub const fn new(root: WorkspaceRoot) -> Self {
        Self { root }
    }
    /// Dispatches a typed tool call with cooperative cancellation.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when validation, workspace resolution, or execution fails.
    pub fn dispatch_with_cancellation(
        &self,
        input: ToolInput,
        cancellation: CancellationSignal,
    ) -> DtoResult<ToolDispatchOutcome> {
        if cancellation.is_cancelled() {
            return Ok(stopped_outcome(None));
        }
        Ok(match input {
            ToolInput::Read(i) => read_tool(&self.root, i, &cancellation)?,
            ToolInput::Write(i) => write_tool(&self.root, i, &cancellation)?,
            ToolInput::Edit(i) => edit_tool(&self.root, i, &cancellation)?,
            ToolInput::Glob(i) => glob_tool(&self.root, i, &cancellation)?,
            ToolInput::Grep(i) => grep_tool(&self.root, i, &cancellation)?,
            ToolInput::Execute(i) => execute_tool(&self.root, i, cancellation)?,
        })
    }
}

fn read_tool(
    root: &WorkspaceRoot,
    input: ReadInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ToolDispatchOutcome> {
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(None));
    }
    let mut file = std::fs::File::open(root.resolve_path(&input.path)).map_err(|_| {
        intention_proto::ErrorDto::validation("tool_read_failed", "unable to read workspace file")
    })?;
    let mut bytes = Vec::new();
    let source_truncated = read_bounded(&mut file, &mut bytes).map_err(|_| {
        intention_proto::ErrorDto::validation("tool_read_failed", "unable to read workspace file")
    })?;
    let (text, truncated) = bounded_lossy(&bytes);
    let result = ToolResult::Read(TextResult {
        text: bounded_text(text)?,
        truncated: truncated || source_truncated,
    });
    // A stop observed after the bounded read keeps the captured bytes as the
    // call's partial output instead of discarding them.
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(Some(result)));
    }
    Ok(ToolDispatchOutcome::Completed(result))
}

fn execute_tool(
    root: &WorkspaceRoot,
    input: ExecuteInput,
    cancellation: CancellationSignal,
) -> DtoResult<ToolDispatchOutcome> {
    let mut command = Command::new(input.program.as_str());
    command.args(input.args.iter().map(BoundedText::as_str));
    command.current_dir(root.execute_cwd());
    // The child inherits the caller's environment by default. WorkspaceRoot
    // scopes filesystem path resolution and the child CWD, not the process
    // environment.
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // The child leads its own process group so timeout and cancellation
        // can terminate background descendants that inherited the pipes
        // (PR24-011).
        command.process_group(0);
    }
    let child = command.spawn().map_err(|_| {
        intention_proto::ErrorDto::validation(
            "tool_execute_spawn_failed",
            "unable to spawn workspace command",
        )
    })?;
    let output = match bounded_output(child, cancellation) {
        Ok(output) => output,
        Err(ExecuteFailure::ReadFailed) => {
            return Err(intention_proto::ErrorDto::validation(
                "tool_execute_read_failed",
                "workspace command execution failed",
            ));
        }
        Err(ExecuteFailure::Interrupted { cause, partial }) => {
            let partial = partial.map(PartialOutput::into_result).transpose()?;
            return Ok(ToolDispatchOutcome::Interrupted { cause, partial });
        }
    };
    let process_status = ToolProcessStatus::classify(output.status);
    let (stdout, _) = bounded_lossy(&output.stdout);
    let (stderr, _) = bounded_lossy(&output.stderr);
    let truncated = output.stdout_truncated || output.stderr_truncated;
    // Render the terminal status from the typed classification so the text and
    // the typed `ToolProcessStatus` can never disagree.
    let status_text = match process_status {
        ToolProcessStatus::Success => "exit_code:0".to_owned(),
        ToolProcessStatus::NonZero { code } => format!("exit_code:{code}"),
        ToolProcessStatus::Signal { signal } => format!("signal:{signal}"),
    };
    let text = format!(
        "stdout:\n{stdout}\nstderr:\n{stderr}\n{status_text}{}",
        if truncated { "\n[truncated]" } else { "" }
    );
    Ok(ToolDispatchOutcome::Completed(ToolResult::Execute(
        TextResult {
            text: BoundedText::new(text)?,
            truncated,
        },
    )))
}

fn write_tool(
    root: &WorkspaceRoot,
    input: WriteInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ToolDispatchOutcome> {
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(None));
    }
    let bytes = input.content.as_str().len() as u64;
    let path = root.resolve_path(&input.path);
    if let Some(expected) = input.expected_content.as_ref() {
        // Expected-content equality is checked against a bounded read: a
        // larger file can never equal the bounded expected content and is
        // reported as changed instead of being slurped (PR24-022).
        let current = match read_limited(&path, MAX_EDIT_TARGET_BYTES) {
            LimitedReadOutcome::Content(bytes) => String::from_utf8(bytes).map_err(|_| {
                intention_proto::ErrorDto::validation(
                    "tool_write_conflict",
                    "workspace file changed before write",
                )
            })?,
            LimitedReadOutcome::TooLarge => {
                return Err(intention_proto::ErrorDto::validation(
                    "tool_write_conflict",
                    "workspace file changed before write",
                ));
            }
            LimitedReadOutcome::Unreadable => {
                return Err(intention_proto::ErrorDto::validation(
                    "tool_write_conflict",
                    "workspace file changed before write",
                ));
            }
        };
        if current != expected.as_str() {
            return Err(intention_proto::ErrorDto::validation(
                "tool_write_conflict",
                "workspace file changed before write",
            ));
        }
        // The preflight read is the last I/O step before the effect.
        if cancellation.is_cancelled() {
            return Ok(stopped_outcome(None));
        }
    }
    std::fs::write(path, input.content.as_str()).map_err(|_| {
        intention_proto::ErrorDto::validation("tool_write_failed", "unable to write workspace file")
    })?;
    Ok(ToolDispatchOutcome::Completed(ToolResult::Write(
        WriteResult { bytes },
    )))
}

fn edit_tool(
    root: &WorkspaceRoot,
    input: EditInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ToolDispatchOutcome> {
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(None));
    }
    let path = root.resolve_path(&input.path);
    // Edit reads the complete target to apply one replacement; the target is
    // therefore size-bounded so a huge file cannot allocate unboundedly
    // (PR24-022).
    let text = match read_limited(&path, MAX_EDIT_TARGET_BYTES) {
        LimitedReadOutcome::Content(bytes) => String::from_utf8(bytes).map_err(|_| {
            intention_proto::ErrorDto::validation(
                "tool_read_failed",
                "unable to read workspace file",
            )
        })?,
        LimitedReadOutcome::TooLarge => {
            return Err(intention_proto::ErrorDto::validation(
                "tool_edit_target_too_large",
                "edit target exceeds the workspace file size bound",
            ));
        }
        LimitedReadOutcome::Unreadable => {
            return Err(intention_proto::ErrorDto::validation(
                "tool_read_failed",
                "unable to read workspace file",
            ));
        }
    };
    if !text.contains(input.old.as_str()) {
        return Err(intention_proto::ErrorDto::validation(
            "edit_target_missing",
            "edit target was not found",
        ));
    }
    if let Some(expected) = input.expected_content.as_ref()
        && text != expected.as_str()
    {
        return Err(intention_proto::ErrorDto::validation(
            "tool_edit_conflict",
            "workspace file changed before edit",
        ));
    }
    // The bounded read is the last I/O step before the replacement write.
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(None));
    }
    let replacement = text.replacen(input.old.as_str(), input.new.as_str(), 1);
    std::fs::write(path, &replacement).map_err(|_| {
        intention_proto::ErrorDto::validation("tool_write_failed", "unable to write workspace file")
    })?;
    Ok(ToolDispatchOutcome::Completed(ToolResult::Edit(
        WriteResult {
            bytes: replacement.len() as u64,
        },
    )))
}

fn glob_tool(
    root: &WorkspaceRoot,
    input: GlobInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ToolDispatchOutcome> {
    validate_search_pattern(input.pattern.as_str())?;
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(None));
    }
    let base = root.root();
    let pattern = base
        .join(input.pattern.as_str())
        .to_str()
        .ok_or_else(|| {
            intention_proto::ErrorDto::validation("invalid_tool_pattern", "tool pattern is invalid")
        })?
        .to_owned();
    let mut paths = Vec::new();
    for entry in glob::glob(&pattern).map_err(|_| {
        intention_proto::ErrorDto::validation("invalid_tool_pattern", "tool pattern is invalid")
    })? {
        // The traversal is the tool's I/O step: a stop ends it and the entries
        // collected so far are returned as an honestly truncated partial list.
        if cancellation.is_cancelled() {
            break;
        }
        // One unreadable or raced-away entry skips itself instead of aborting
        // the whole search; listing what is safely listable keeps repeated
        // traversals deterministic.
        let Ok(path) = entry else { continue };
        let Some(relative) = path.strip_prefix(base).ok().and_then(|p| p.to_str()) else {
            continue;
        };
        let Ok(value) = WorkspaceRelativePathDto::parse(relative.replace('\\', "/")) else {
            continue;
        };
        paths.push(value);
    }
    paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    paths.dedup();
    let (retained, window_truncated) = window_paths(paths);
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(Some(ToolResult::Glob(PathsResult {
            paths: retained,
            truncated: true,
        }))));
    }
    Ok(ToolDispatchOutcome::Completed(ToolResult::Glob(
        PathsResult {
            paths: retained,
            truncated: window_truncated,
        },
    )))
}

/// Applies the shared search-result window to one sorted, deduplicated path list.
///
/// The window is applied after the deterministic sort and dedup so the
/// retained set never depends on traversal order. Each retained entry costs
/// its JSON string bytes plus the one-byte list separator, which keeps the
/// serialized path list inside the shared search-result window (C-04).
fn window_paths(paths: Vec<WorkspaceRelativePathDto>) -> (Vec<WorkspaceRelativePathDto>, bool) {
    let mut retained = Vec::new();
    let mut retained_bytes = 0usize;
    let mut truncated = false;
    for path in paths {
        let cost = path.as_str().len() + 3;
        if cost > MAX_GREP_AGGREGATE_BYTES.saturating_sub(retained_bytes) {
            truncated = true;
            break;
        }
        retained_bytes += cost;
        retained.push(path);
    }
    (retained, truncated)
}

fn grep_tool(
    root: &WorkspaceRoot,
    input: GrepInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ToolDispatchOutcome> {
    validate_search_pattern(input.pattern.as_str())?;
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(None));
    }
    if input.scope.is_some() {
        return grep_scoped(root, input, cancellation);
    }
    let path = input
        .path
        .as_ref()
        .map(|path| root.resolve_path(path))
        .ok_or_else(|| {
            intention_proto::ErrorDto::validation(
                "invalid_tool_path",
                "grep requires a workspace path",
            )
        })?;
    // An explicit file path is addressed as given: `metadata` follows a
    // symbolic link like any other filesystem path, and only the type
    // decision happens here.
    let metadata = std::fs::metadata(&path).map_err(|_| {
        intention_proto::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    if !metadata.is_file() {
        return Err(intention_proto::ErrorDto::validation(
            "tool_search_failed",
            "workspace search failed",
        ));
    }
    let mut file = std::fs::File::open(path).map_err(|_| {
        intention_proto::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    let mut bytes = Vec::new();
    let source_truncated = read_bounded(&mut file, &mut bytes).map_err(|_| {
        intention_proto::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let lossy_truncated = std::str::from_utf8(&bytes).is_err();
    let logical = input
        .path
        .as_ref()
        .ok_or_else(|| {
            intention_proto::ErrorDto::validation(
                "invalid_tool_path",
                "grep requires a workspace path",
            )
        })?
        .clone();
    let mut matches = Vec::new();
    let mut retained_bytes = 0usize;
    let mut truncated = source_truncated || lossy_truncated;
    for (line_index, line) in text.lines().enumerate() {
        // A stop observed between scanned lines keeps the matches found so far
        // as an honestly truncated partial result.
        if cancellation.is_cancelled() {
            return Ok(stopped_outcome(Some(ToolResult::Grep(GrepResult {
                matches,
                truncated: true,
            }))));
        }
        let Some(column) = line.find(input.pattern.as_str()) else {
            continue;
        };
        let fragment = if line.len() > MAX_TOOL_OUTPUT_BYTES {
            truncated = true;
            let end = line
                .char_indices()
                .take_while(|(index, _)| *index < MAX_TOOL_OUTPUT_BYTES)
                .map(|(index, _)| index)
                .last()
                .unwrap_or(0);
            line[..end].to_owned()
        } else {
            line.to_owned()
        };
        if !record_grep_match(
            &mut matches,
            &mut retained_bytes,
            &mut truncated,
            logical.clone(),
            line_index as u64 + 1,
            line[..column].chars().count() as u64 + 1,
            fragment,
        )? {
            break;
        }
    }
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(Some(ToolResult::Grep(GrepResult {
            matches,
            truncated: true,
        }))));
    }
    Ok(ToolDispatchOutcome::Completed(ToolResult::Grep(
        GrepResult { matches, truncated },
    )))
}

fn grep_scoped(
    root: &WorkspaceRoot,
    input: GrepInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ToolDispatchOutcome> {
    let scope = input.scope.ok_or_else(|| {
        intention_proto::ErrorDto::validation("invalid_tool_path", "grep requires a workspace path")
    })?;
    let (base, single) = match scope {
        GrepScope::File { path } => (root.resolve_path(&path), Some(path)),
        GrepScope::Directory { path } => (root.resolve_path(&path), None),
        GrepScope::Workspace => (root.root().to_path_buf(), None),
    };
    // An explicit scope is addressed as given: `metadata` follows a symbolic
    // link like any other filesystem path, and only the type decision happens
    // here.
    let metadata = std::fs::metadata(&base).map_err(|_| {
        intention_proto::ErrorDto::validation("tool_search_failed", "workspace search failed")
    })?;
    if single.is_some() && !metadata.is_file() {
        return Err(intention_proto::ErrorDto::validation(
            "tool_search_failed",
            "workspace search failed",
        ));
    }
    let mut files = Vec::new();
    if metadata.is_file() {
        files.push(base);
    } else if metadata.is_dir() {
        let mut pending = vec![base];
        while let Some(directory) = pending.pop() {
            // The directory walk is one bounded I/O step of the scoped search.
            if cancellation.is_cancelled() {
                return Ok(stopped_outcome(Some(ToolResult::Grep(GrepResult {
                    matches: Vec::new(),
                    truncated: true,
                }))));
            }
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            let mut children = entries.filter_map(Result::ok).collect::<Vec<_>>();
            children.sort_by_key(|entry| entry.file_name());
            for entry in children.into_iter().rev() {
                let path = entry.path();
                let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                // Traversal does not follow symbolic links, for files and
                // directories alike: a link is ordinary filesystem material,
                // and descending through one could alias content outside the
                // addressed scope or cycle. Only a link addressed directly as
                // the search target is followed.
                if metadata.file_type().is_symlink() {
                    continue;
                }
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() {
                    files.push(path);
                } else {
                    continue;
                }
            }
        }
    } else {
        return Err(intention_proto::ErrorDto::validation(
            "tool_search_failed",
            "workspace search failed",
        ));
    }
    files.sort();
    let mut matches = Vec::new();
    let mut retained_bytes = 0usize;
    let mut truncated = false;
    for path in files {
        // A stop observed between searched files keeps the matches found so
        // far as an honestly truncated partial result.
        if cancellation.is_cancelled() {
            return Ok(stopped_outcome(Some(ToolResult::Grep(GrepResult {
                matches,
                truncated: true,
            }))));
        }
        let mut file = std::fs::File::open(&path).map_err(|_| {
            intention_proto::ErrorDto::validation("tool_search_failed", "workspace search failed")
        })?;
        let mut bytes = Vec::new();
        // Every file is read through the bounded reader, so an oversized file
        // cannot allocate unbounded memory during a directory search; the
        // truncation flag reports the dropped tail (PR24-022).
        let source_truncated = read_bounded(&mut file, &mut bytes).map_err(|_| {
            intention_proto::ErrorDto::validation("tool_search_failed", "workspace search failed")
        })?;
        let text = String::from_utf8_lossy(&bytes);
        let lossy_truncated = std::str::from_utf8(&bytes).is_err();
        if source_truncated || lossy_truncated {
            truncated = true;
        }
        let logical = single.clone().or_else(|| {
            path.strip_prefix(root.root()).ok().and_then(|p| {
                WorkspaceRelativePathDto::parse(p.to_string_lossy().replace('\\', "/")).ok()
            })
        });
        let Some(logical) = logical else { continue };
        for (line_index, line) in text.lines().enumerate() {
            // A stop observed between scanned lines ends the search with the
            // matches found so far.
            if cancellation.is_cancelled() {
                return Ok(stopped_outcome(Some(ToolResult::Grep(GrepResult {
                    matches,
                    truncated: true,
                }))));
            }
            let Some(column) = line.find(input.pattern.as_str()) else {
                continue;
            };
            let fragment = if line.len() > MAX_TOOL_OUTPUT_BYTES {
                truncated = true;
                let end = line
                    .char_indices()
                    .take_while(|(index, _)| *index < MAX_TOOL_OUTPUT_BYTES)
                    .map(|(index, _)| index)
                    .last()
                    .unwrap_or(0);
                line[..end].to_owned()
            } else {
                line.to_owned()
            };
            if !record_grep_match(
                &mut matches,
                &mut retained_bytes,
                &mut truncated,
                logical.clone(),
                line_index as u64 + 1,
                line[..column].chars().count() as u64 + 1,
                fragment,
            )? {
                return Ok(ToolDispatchOutcome::Completed(ToolResult::Grep(
                    GrepResult { matches, truncated },
                )));
            }
        }
    }
    if cancellation.is_cancelled() {
        return Ok(stopped_outcome(Some(ToolResult::Grep(GrepResult {
            matches,
            truncated: true,
        }))));
    }
    Ok(ToolDispatchOutcome::Completed(ToolResult::Grep(
        GrepResult { matches, truncated },
    )))
}

/// Records one grep match when its serialized cost fits the aggregate window.
///
/// Per-line fragment caps alone allow a very large aggregate result; every
/// retained match is charged its serialized bytes plus the one-byte list
/// separator, so the serialized list itself stays inside the shared
/// search-result window and the truncation flag reports the cut (PR24-022,
/// C-04).
///
/// # Errors
///
/// Returns a validation error when the fragment violates the bounded-text
/// contract.
fn record_grep_match(
    matches: &mut Vec<GrepMatch>,
    retained_bytes: &mut usize,
    truncated: &mut bool,
    path: WorkspaceRelativePathDto,
    line: u64,
    column: u64,
    fragment: String,
) -> DtoResult<bool> {
    let matched = GrepMatch {
        path,
        line,
        column,
        fragment: bounded_text(fragment)?,
    };
    let cost = serde_json::to_string(&matched)
        .map_err(|_| {
            intention_proto::ErrorDto::validation(
                "invalid_tool_result_content",
                "tool result content could not be normalized",
            )
        })?
        .len()
        .saturating_add(1);
    if cost > MAX_GREP_AGGREGATE_BYTES.saturating_sub(*retained_bytes) {
        *truncated = true;
        return Ok(false);
    }
    *retained_bytes += cost;
    matches.push(matched);
    Ok(true)
}

fn validate_search_pattern(pattern: &str) -> DtoResult<()> {
    // Absoluteness must not depend on the host platform: Windows drive-letter
    // and UNC roots are rejected everywhere so search patterns stay strictly
    // workspace-relative on every supported target.
    let windows_rooted = (pattern.len() >= 2
        && pattern.as_bytes()[1] == b':'
        && pattern.as_bytes()[0].is_ascii_alphabetic())
        || pattern.starts_with("\\\\");
    if pattern.is_empty()
        || pattern.contains('\0')
        || std::path::Path::new(pattern).is_absolute()
        || pattern.starts_with('/')
        || pattern.starts_with('\\')
        || windows_rooted
        || pattern.split(['/', '\\']).any(|part| part == "..")
    {
        return Err(intention_proto::ErrorDto::validation(
            "invalid_tool_pattern",
            "tool pattern must be relative and stay within the workspace",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod coverage_helpers {
    use super::*;

    #[test]
    fn bounded_lossy_handles_invalid_utf8_at_boundary() {
        let mut bytes = vec![b'a'; MAX_TOOL_OUTPUT_BYTES];
        bytes[MAX_TOOL_OUTPUT_BYTES - 1] = 0xff;
        let (text, truncated) = bounded_lossy(&[bytes, vec![b'b']].concat());
        assert!(truncated);
        assert!(text.ends_with("\n[truncated]"));
        assert!(!text.contains('\u{fffd}'));
    }
}
