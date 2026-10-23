//! The interactive line REPL over the shared application core.
//!
//! The REPL owns one line loop and no rendering: every line is either a command
//! the loop performs or a turn the shared driver streams through the core. The
//! same driver and the same application state serve the headless command, so
//! session, turn, transcript, and provisional-text behavior is identical in both
//! line front ends.
//!
//! Ctrl+C belongs to this loop rather than to the process: while a run streams,
//! the signal stops that run, and at the prompt it leaves the loop like `/quit`.
//! A signal therefore never kills a process that would leave a run working in
//! the daemon.

use std::io::{BufRead, Write};

use intention_client::IntentionClient;
use intention_proto::{RunStatusDto, SessionId, run_status_is_terminal};
use intention_transport::LocalEndpoint;
use intention_tui::app::{CommandAction, CommandInvocation, resolve_command, short_identifier};

use crate::cli::{ExitStatus, Options};
use crate::headless::{
    Driver, InterruptRequests, PumpFailure, RunOutcome, TextReport, interrupt_requests,
};

/// Runs the line REPL until the input ends or the user quits.
pub fn run<W: Write, E: Write>(
    options: &Options,
    client: IntentionClient,
    endpoint: LocalEndpoint,
    out: &mut W,
    err: &mut E,
) -> ExitStatus {
    let runtime = match crate::session_runtime() {
        Ok(runtime) => runtime,
        Err(error) => return crate::startup_failure(&error, err),
    };
    runtime.block_on(converse(options, client, endpoint, out, err))
}

/// Drives the line loop over one connected driver.
async fn converse<W: Write, E: Write>(
    options: &Options,
    client: IntentionClient,
    endpoint: LocalEndpoint,
    out: &mut W,
    err: &mut E,
) -> ExitStatus {
    let workspace_root = match crate::workspace_root(options.workspace()) {
        Ok(root) => root,
        Err(error) => return crate::startup_failure(&error, err),
    };
    let mut driver = Driver::new(client, endpoint, workspace_root, options.mode());
    // The front end resolves the session it opens once, through the client: a
    // selected session is read by its own snapshot, an explicit continuation
    // resolves the target the daemon's list order names, and a launch that
    // asked for neither opens nothing.
    let opened = match options.session() {
        Some(session) => driver.show(session).await,
        None if options.continue_session() => match driver.continue_session().await {
            Ok(session) => driver.show(session).await,
            Err(error) => Err(error),
        },
        None => driver.bootstrap().await,
    };
    if let Err(error) = opened {
        return crate::startup_failure(&error, err);
    }

    let mut lines = input_lines();
    let mut interrupts = interrupt_requests();
    loop {
        // @todo(hack): every REPL write is best-effort - a failed prompt, error,
        // or notice write is silently dropped and the loop continues as if the
        // user saw the line; one reporter should record the suppressed failure.
        let _ = write!(out, "> ");
        let _ = out.flush();
        let mut interrupted = false;
        let line = tokio::select! {
            biased;
            line = lines.recv() => line,
            _ = interrupts.recv() => {
                interrupted = true;
                None
            }
        };
        if interrupted {
            // Ctrl+C leaves the line loop like `/quit`: the run this front end
            // still observes is stopped first, so the daemon never keeps
            // working for a loop that left.
            let _stopped = driver.request_stop().await;
            drain_active_turn(&mut driver, out, err, &mut interrupts).await;
            break;
        }
        let Some(line) = line else {
            drain_active_turn(&mut driver, out, err, &mut interrupts).await;
            break;
        };
        let input = match line {
            Input::Line(text) => text,
            Input::Failed => {
                let _ = writeln!(err, "error: the input line could not be read");
                break;
            }
        };
        match Line::parse(&input) {
            Line::Blank => {}
            Line::Refused(notice) => {
                let _ = writeln!(err, "error: {notice}");
            }
            Line::Command(invocation) => match invocation.spec().action() {
                CommandAction::Quit => {
                    drain_active_turn(&mut driver, out, err, &mut interrupts).await;
                    break;
                }
                CommandAction::NewSession => match driver.create().await {
                    Ok(()) => report_open_session(&driver, out),
                    Err(error) => {
                        let _ = writeln!(err, "error: {error}");
                    }
                },
                CommandAction::OpenSession => {
                    let word = invocation.word(0).unwrap_or_default();
                    match SessionId::parse(word) {
                        Ok(session) => match driver.show(session).await {
                            Ok(()) => report_open_session(&driver, out),
                            Err(error) => {
                                let _ = writeln!(err, "error: {error}");
                            }
                        },
                        Err(_) => {
                            let _ = writeln!(
                                err,
                                "error: {} requires a canonical session identifier",
                                invocation.spec().typed_name()
                            );
                        }
                    }
                }
                // The two surfaces share one vocabulary; a command that drives
                // the fullscreen surface is known here and has nothing to drive.
                CommandAction::SessionsBrowser | CommandAction::Theme => {
                    let _ = writeln!(
                        err,
                        "error: {} needs the fullscreen front end",
                        invocation.spec().typed_name()
                    );
                }
            },
            Line::Turn(turn) => {
                // A prompt with no session open creates the session it needs
                // and commits the prompt as that session's first turn in one
                // command.
                converse_turn(&mut driver, &turn, out, err, &mut interrupts).await;
            }
        }
    }
    ExitStatus::Completed
}

/// One prompt read the input thread produced.
#[derive(Debug, Eq, PartialEq)]
enum Input {
    /// The thread read one line, its newline included.
    Line(String),
    /// The thread could not read the console, so no further line follows.
    Failed,
}

/// Reads prompt lines on their own thread.
///
/// A blocking read of the console cannot notice a signal, and the line loop
/// must wait for a line and for SIGINT at the same time: the thread owns the
/// read and the console lock for the life of the front end - it is the only
/// reader in the process - and sends every line it reads, so a signal reaches
/// the loop while it waits.
fn input_lines() -> tokio::sync::mpsc::UnboundedReceiver<Input> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut buffer = String::new();
        loop {
            buffer.clear();
            let read = std::io::stdin().lock().read_line(&mut buffer);
            match read {
                Ok(0) => break,
                Ok(_) => {
                    if sender
                        .send(Input::Line(std::mem::take(&mut buffer)))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => {
                    let _ = sender.send(Input::Failed);
                    break;
                }
            }
        }
    });
    receiver
}

/// Sends one line as a user turn and streams the run to its end.
async fn converse_turn<W: Write, E: Write>(
    driver: &mut Driver,
    input: &str,
    out: &mut W,
    err: &mut E,
    interrupts: &mut InterruptRequests,
) {
    if let Some(error) = driver.send_turn(input).await {
        let _ = writeln!(err, "error: {error}");
        return;
    }
    stream_active_turn(driver, out, err, interrupts).await;
}

/// Streams the run the core currently owns to its terminal status.
async fn stream_active_turn<W: Write, E: Write>(
    driver: &mut Driver,
    out: &mut W,
    err: &mut E,
    interrupts: &mut InterruptRequests,
) {
    let mut report = TextReport::new(out);
    match driver.pump(&mut report, None, interrupts).await {
        Ok(outcome) => {
            let _ = report.finish(outcome);
            if outcome == RunOutcome::Terminal(RunStatusDto::Failed)
                && let Some(error) = driver.state().error()
            {
                let _ = writeln!(err, "error: {error}");
            }
        }
        Err(PumpFailure::Client(error)) => {
            let _ = writeln!(err, "error: {error}");
        }
        Err(PumpFailure::Output(_error)) => {
            let _ = writeln!(err, "error: the run output could not be written");
        }
    }
}

/// Waits for a live run before the line loop leaves.
///
/// A piped script ends with `/quit` or end-of-input while the turn it sent may
/// still be streaming. The front end owns one run at a time, so the loop waits
/// for that run's terminal status before it returns, and a script's output
/// stays complete instead of racing the process exit.
// @todo(hack): the line loop must block on the live run before it returns so a
// piped script's output is not truncated by the process exit; the run's
// completion should own the process lifetime instead of the loop draining it.
async fn drain_active_turn<W: Write, E: Write>(
    driver: &mut Driver,
    out: &mut W,
    err: &mut E,
    interrupts: &mut InterruptRequests,
) {
    let live = driver
        .state()
        .run_status()
        .is_some_and(|status| !run_status_is_terminal(status));
    if live {
        stream_active_turn(driver, out, err, interrupts).await;
    }
}

/// Reports the session one command left open.
///
/// A command that opened no session already wrote the typed failure the client
/// reported, so this writes nothing of its own.
fn report_open_session(driver: &Driver, out: &mut impl Write) {
    if let Some(session) = driver.state().session_id() {
        let _ = writeln!(out, "session {} open", short_identifier(session));
    }
}

/// One recognized REPL input line.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Line {
    /// The input was blank.
    Blank,
    /// The line names a registered command and satisfies its declaration.
    Command(CommandInvocation),
    /// The line does not satisfy the one registry: it names no registered
    /// command, or its words do not fill the arguments its command declares.
    Refused(String),
    /// The user typed one turn.
    Turn(String),
}

impl Line {
    /// Parses one raw input line through the one command registry.
    ///
    /// The names, the argument rules, and the notices are the fullscreen
    /// surface's own: a line resolves here exactly as it does in the input
    /// band, so no command exists in one front end and not in the other.
    fn parse(raw: &str) -> Self {
        let input = raw.trim();
        if input.is_empty() {
            return Self::Blank;
        }
        let Some(command) = input.strip_prefix('/') else {
            return Self::Turn(input.to_owned());
        };
        match resolve_command(command).into_invocation() {
            Ok(invocation) => Self::Command(invocation),
            Err(notice) => Self::Refused(notice),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Returns the invocation one raw line resolved to, or the notice it
    /// answered with.
    fn resolved(raw: &str) -> Result<CommandInvocation, String> {
        match Line::parse(raw) {
            Line::Command(invocation) => Ok(invocation),
            Line::Refused(notice) => Err(notice),
            line => panic!("{line:?} carries no command"),
        }
    }

    #[test]
    fn every_line_kind_parses_from_its_input() {
        assert_eq!(Line::parse("   \n"), Line::Blank);
        assert_eq!(
            Line::parse("  hello world  "),
            Line::Turn("hello world".to_owned())
        );
        assert_eq!(
            resolved("/quit\n")
                .expect("quit is a command")
                .spec()
                .action(),
            CommandAction::Quit
        );
        assert_eq!(
            resolved("/new").expect("new is a command").spec().action(),
            CommandAction::NewSession
        );
        assert_eq!(
            resolved("/session 0b1a2c3d-0000-4000-8000-000000000000\n")
                .expect("session is a command")
                .word(0),
            Some("0b1a2c3d-0000-4000-8000-000000000000"),
            "the free-form identifier reaches the transition as it was typed"
        );
    }

    #[test]
    fn the_repl_parses_the_one_vocabulary_exactly_as_the_fullscreen_surface_does() {
        assert_eq!(
            resolved("/theme DARK")
                .expect("a declared value resolves")
                .value(0),
            Some("dark"),
            "the value vocabulary and its case rule are the registry's"
        );
        assert_eq!(
            resolved("/session").expect_err("the identifier is required"),
            "missing argument \"session\""
        );
        assert_eq!(
            resolved("/quit now").expect_err("the exit takes no argument"),
            "unexpected argument \"now\""
        );
        assert_eq!(
            resolved("/watch").expect_err("watch is registered nowhere"),
            "unknown command /watch; known commands: /new /sessions /theme /session /quit",
            "the REPL's notice lists the same set the hint band shows"
        );
    }
}
