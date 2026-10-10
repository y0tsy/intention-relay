//! The interactive line REPL over the shared application core.
//!
//! The REPL owns one line loop and no rendering: every line is either a command
//! the loop performs or a turn the shared driver streams through the core. The
//! same driver and the same application state serve the headless command, so
//! session, turn, transcript, and provisional-text behavior is identical in both
//! line front ends.

use std::io::{BufRead, Write};

use intention_client::IntentionClient;
use intention_proto::{RunStatusDto, SessionId};
use intention_transport::LocalEndpoint;
use intention_tui::app::short_identifier;

use crate::cli::{ExitStatus, Options};
use crate::headless::{Driver, PumpFailure, RunOutcome, TextReport};

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
    if let Err(error) = driver
        .select(options.session(), options.continue_session())
        .await
    {
        return crate::startup_failure(&error, err);
    }
    if options.session().is_some() && driver.state().session_id().is_none() {
        return crate::startup_failure(&driver.session_failure(), err);
    }

    let mut line = String::new();
    loop {
        let _ = write!(out, "> ");
        let _ = out.flush();
        line.clear();
        // The lock lives no longer than the read: the line loop awaits client
        // work between two lines, and a held console lock must not travel there.
        let read = std::io::stdin().lock().read_line(&mut line);
        match read {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => {
                let _ = writeln!(err, "error: the input line could not be read");
                break;
            }
        }
        match Line::parse(&line) {
            Line::Blank => {}
            Line::Quit => break,
            Line::New => {
                driver.create().await;
                report_session(&driver, out, err);
            }
            Line::Session(argument) => match SessionId::parse(&argument) {
                Ok(session) => {
                    driver.show(session).await;
                    report_session(&driver, out, err);
                }
                Err(_) => {
                    let _ = writeln!(
                        err,
                        "error: /session requires a canonical session identifier"
                    );
                }
            },
            Line::Unknown(name) => {
                let _ = writeln!(err, "error: unknown command /{name}");
            }
            Line::Turn(turn) => {
                // A prompt with no session open starts the session it needs:
                // the core creates and opens one, then sends the prompt as its
                // first turn.
                converse_turn(&mut driver, &turn, out, err).await;
            }
        }
    }
    ExitStatus::Completed
}

/// Sends one line as a user turn and streams the run to its end.
async fn converse_turn<W: Write, E: Write>(
    driver: &mut Driver,
    input: &str,
    out: &mut W,
    err: &mut E,
) {
    driver.send_turn(input).await;
    if let Some(error) = driver.last_failure().cloned() {
        let _ = writeln!(err, "error: {} ({})", error.message(), error.code());
        return;
    }
    let mut report = TextReport::new(out);
    match driver.pump(&mut report, None).await {
        Ok(outcome) => {
            let _ = report.finish(outcome);
            if outcome == RunOutcome::Terminal(RunStatusDto::Failed)
                && let Some(error) = driver.state().error()
            {
                let _ = writeln!(err, "error: {error}");
            }
        }
        Err(PumpFailure::Client(error)) => {
            let _ = writeln!(err, "error: {} ({})", error.message(), error.code());
        }
        Err(PumpFailure::Output(_error)) => {
            let _ = writeln!(err, "error: the run output could not be written");
        }
    }
}

/// Reports the session one command left open.
fn report_session(driver: &Driver, out: &mut impl Write, err: &mut impl Write) {
    match driver.state().session_id() {
        Some(session) => {
            let _ = writeln!(out, "session {} open", short_identifier(session));
        }
        None => {
            let error = driver.session_failure();
            let _ = writeln!(err, "error: {} ({})", error.message(), error.code());
        }
    }
}

/// One recognized REPL input line.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Line {
    /// The input was blank.
    Blank,
    /// The user asked to leave.
    Quit,
    /// The user asked for a new session.
    New,
    /// The user asked to open the named session.
    Session(String),
    /// The user typed one turn.
    Turn(String),
    /// The user typed an undeclared command.
    Unknown(String),
}

impl Line {
    /// Parses one raw input line.
    fn parse(raw: &str) -> Self {
        let input = raw.trim();
        if input.is_empty() {
            return Self::Blank;
        }
        let Some(command) = input.strip_prefix('/') else {
            return Self::Turn(input.to_owned());
        };
        let (name, argument) = command
            .split_once(' ')
            .map_or((command, ""), |(name, argument)| (name, argument.trim()));
        match name {
            "quit" => Self::Quit,
            "new" => Self::New,
            "session" => Self::Session(argument.to_owned()),
            _ => Self::Unknown(name.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::panic,
        reason = "Line tests assert the parsed step of typed input directly."
    )]

    use super::*;

    #[test]
    fn every_line_kind_parses_from_its_input() {
        assert_eq!(Line::parse("   \n"), Line::Blank);
        assert_eq!(Line::parse("/quit\n"), Line::Quit);
        assert_eq!(Line::parse("/new"), Line::New);
        assert_eq!(
            Line::parse("/session 0b1a2c3d-0000-4000-8000-000000000000\n"),
            Line::Session("0b1a2c3d-0000-4000-8000-000000000000".to_owned())
        );
        assert_eq!(
            Line::parse("  hello world  "),
            Line::Turn("hello world".to_owned())
        );
        assert_eq!(Line::parse("/watch"), Line::Unknown("watch".to_owned()));
    }

    #[test]
    fn a_command_without_an_argument_keeps_an_empty_argument() {
        assert_eq!(Line::parse("/session"), Line::Session(String::new()));
        assert_eq!(Line::parse("/quit now"), Line::Quit);
    }
}
