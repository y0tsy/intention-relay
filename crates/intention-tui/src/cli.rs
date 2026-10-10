//! Command-line grammar and process status of the terminal client binary.
//!
//! One grammar serves the three terminal front ends: the bare command line and
//! `tui` select the fullscreen UI, `repl` selects the interactive line loop, and
//! `run <PROMPT>` selects the headless command. The module owns the complete
//! user-facing text (help, usage, rejections) and the closed exit-status set, so
//! no other terminal module decides what a rejection or a run end reports.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use intention_proto::{ErrorCategoryDto, ErrorDto, RunModeDto, RunStatusDto, SessionId};

/// The one-line usage every command-line rejection prints.
pub const USAGE: &str = "usage: intention-tui [tui | repl | run <PROMPT>] [OPTIONS]";

/// The rejection text the fullscreen front end prints without a usable terminal.
pub const TUI_REQUIRES_TERMINAL: &str =
    "the fullscreen front end requires a terminal on standard input and output";

/// The complete help text `-h` and `--help` print.
pub const HELP: &str = "\
Intention Relay terminal client

usage:
  intention-tui [tui] [OPTIONS]         fullscreen terminal UI (requires a terminal)
  intention-tui repl [OPTIONS]          interactive line REPL
  intention-tui run <PROMPT> [OPTIONS]  one headless prompt

options:
  --workspace PATH    workspace root for a new session (default: current directory)
  --session ID        open an existing session by its durable identifier
  --continue          continue the most recently updated session
  --mode plan|build   run policy of a created session (default: build)
  --timeout SECS      interrupt the run when it does not finish within SECS
  --format text|json  run output format: progressive text or NDJSON records
  -h, --help          print this help
";

/// The script output format of the headless command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// Progressive human text on standard output.
    Text,
    /// Newline-delimited typed JSON records on standard output.
    Json,
}

/// The front-end options every command shares.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    workspace: Option<PathBuf>,
    session: Option<SessionId>,
    continue_session: bool,
    mode: RunModeDto,
    timeout: Option<Duration>,
    format: Format,
}

impl Options {
    /// Returns the workspace root the caller configured, if any.
    #[must_use]
    pub fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    /// Returns the session the caller selected, if any.
    #[must_use]
    pub const fn session(&self) -> Option<SessionId> {
        self.session
    }

    /// Returns whether the caller asked to continue the most recent session.
    #[must_use]
    pub const fn continue_session(&self) -> bool {
        self.continue_session
    }

    /// Returns the run policy a created session starts with.
    #[must_use]
    pub const fn mode(&self) -> RunModeDto {
        self.mode
    }

    /// Returns the run wait deadline the caller configured, if any.
    #[must_use]
    pub const fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Returns the script output format of the headless command.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }
}

/// One parsed command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invocation {
    /// Run the fullscreen terminal UI.
    Tui(Options),
    /// Run the interactive line REPL.
    Repl(Options),
    /// Run one headless prompt.
    Run {
        /// The prompt text the caller supplied.
        prompt: String,
        /// The front-end options the caller supplied.
        options: Options,
    },
    /// Print the help text and end without running a front end.
    Help,
}

/// The closed set of process statuses the terminal client returns.
///
/// The codes are the slice contract: `0` completed, `1` usage, `2` daemon or
/// transport, `3` typed rejection or failed run, `4` timeout after a
/// best-effort interrupt, and `5` interrupted run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitStatus {
    /// The run completed.
    Completed,
    /// The command line was rejected before any client work.
    Usage,
    /// The daemon or its transport could not deliver the operation.
    Daemon,
    /// The daemon rejected the operation or the run ended failed.
    Rejected,
    /// The run wait deadline expired after a best-effort interrupt.
    Timeout,
    /// The run ended interrupted.
    Interrupted,
}

impl ExitStatus {
    /// Returns the process exit code one status ends with.
    #[must_use]
    pub fn code(self) -> ExitCode {
        ExitCode::from(match self {
            Self::Completed => 0,
            Self::Usage => 1,
            Self::Daemon => 2,
            Self::Rejected => 3,
            Self::Timeout => 4,
            Self::Interrupted => 5,
        })
    }

    /// Returns the status of one safe typed failure.
    ///
    /// An unavailable failure is a daemon or transport failure; every other
    /// category is the daemon's typed rejection.
    #[must_use]
    pub const fn for_error(error: &ErrorDto) -> Self {
        match error.category() {
            ErrorCategoryDto::Unavailable => Self::Daemon,
            _ => Self::Rejected,
        }
    }

    /// Returns the status of one terminal run status, or `None` while it is live.
    #[must_use]
    pub const fn for_run_status(status: RunStatusDto) -> Option<Self> {
        match status {
            RunStatusDto::Completed => Some(Self::Completed),
            RunStatusDto::Failed => Some(Self::Rejected),
            RunStatusDto::Interrupted => Some(Self::Interrupted),
            RunStatusDto::Starting | RunStatusDto::Running => None,
        }
    }
}

/// Returns whether the fullscreen front end has the terminal it needs.
#[must_use]
pub const fn terminal_available(stdin_is_terminal: bool, stdout_is_terminal: bool) -> bool {
    stdin_is_terminal && stdout_is_terminal
}

/// Parses one command line into the invocation it selects.
///
/// # Errors
///
/// Returns the usage message for an unknown command or option, a missing or
/// malformed value, an argument a command does not take, or an option
/// combination no front end can honour.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Invocation, String> {
    use lexopt::prelude::*;

    let mut command: Option<Command> = None;
    let mut prompt = None;
    let mut workspace = None;
    let mut session = None;
    let mut continue_session = false;
    let mut mode = RunModeDto::Build;
    let mut timeout = None;
    let mut format = None;

    while let Some(argument) = parser.next().map_err(|error| error.to_string())? {
        match argument {
            Short('h') | Long("help") => return Ok(Invocation::Help),
            Long("workspace") => workspace = Some(PathBuf::from(value(parser)?)),
            Long("session") => {
                let text = text(parser)?;
                let parsed = SessionId::parse(&text)
                    .map_err(|error| format!("--session: {}", error.message()))?;
                session = Some(parsed);
            }
            Long("continue") => continue_session = true,
            Long("mode") => mode = mode_value(&text(parser)?)?,
            Long("timeout") => timeout = Some(seconds(&text(parser)?)?),
            Long("format") => format = Some(format_value(&text(parser)?)?),
            Value(value) => {
                let value = value.string().map_err(|error| error.to_string())?;
                match command {
                    None => command = Some(Command::parse(&value)?),
                    Some(Command::Run) if prompt.is_none() => prompt = Some(value),
                    Some(_) => return Err(format!("unexpected argument '{value}'")),
                }
            }
            other => return Err(other.unexpected().to_string()),
        }
    }

    let command = command.unwrap_or(Command::Tui);
    if session.is_some() && continue_session {
        return Err("--session and --continue are mutually exclusive".to_owned());
    }
    if command != Command::Run {
        if timeout.is_some() {
            return Err("--timeout applies to the run command".to_owned());
        }
        if format.is_some() {
            return Err("--format applies to the run command".to_owned());
        }
    }
    let options = Options {
        workspace,
        session,
        continue_session,
        mode,
        timeout,
        format: format.unwrap_or(Format::Text),
    };
    match command {
        Command::Tui => Ok(Invocation::Tui(options)),
        Command::Repl => Ok(Invocation::Repl(options)),
        Command::Run => {
            let Some(prompt) = prompt else {
                return Err("run requires a prompt argument".to_owned());
            };
            if prompt.trim().is_empty() {
                return Err("the prompt must not be empty".to_owned());
            }
            Ok(Invocation::Run { prompt, options })
        }
    }
}

/// The command one positional argument selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    /// The fullscreen terminal UI.
    Tui,
    /// The interactive line REPL.
    Repl,
    /// One headless prompt.
    Run,
}

impl Command {
    /// Parses one command name.
    ///
    /// # Errors
    ///
    /// Returns the usage message for a name the grammar does not declare.
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "tui" => Ok(Self::Tui),
            "repl" => Ok(Self::Repl),
            "run" => Ok(Self::Run),
            _ => Err(format!("unknown command '{value}'")),
        }
    }
}

/// Reads the value of one option.
///
/// # Errors
///
/// Returns the usage message when the value is missing.
fn value(parser: &mut lexopt::Parser) -> Result<OsString, String> {
    parser.value().map_err(|error| error.to_string())
}

/// Reads one option value as valid UTF-8 text.
///
/// # Errors
///
/// Returns the usage message when the value is missing or not valid UTF-8.
fn text(parser: &mut lexopt::Parser) -> Result<String, String> {
    value(parser)?
        .into_string()
        .map_err(|_| "the argument value must be valid UTF-8 text".to_owned())
}

/// Parses one `--mode` value.
///
/// # Errors
///
/// Returns the usage message for a mode the grammar does not declare.
fn mode_value(value: &str) -> Result<RunModeDto, String> {
    match value {
        "plan" => Ok(RunModeDto::Plan),
        "build" => Ok(RunModeDto::Build),
        _ => Err(format!("--mode must be plan or build, got '{value}'")),
    }
}

/// Parses one `--format` value.
///
/// # Errors
///
/// Returns the usage message for a format the grammar does not declare.
fn format_value(value: &str) -> Result<Format, String> {
    match value {
        "text" => Ok(Format::Text),
        "json" => Ok(Format::Json),
        _ => Err(format!("--format must be text or json, got '{value}'")),
    }
}

/// Parses one `--timeout` value in whole seconds.
///
/// # Errors
///
/// Returns the usage message when the value is not a whole number of seconds.
fn seconds(value: &str) -> Result<Duration, String> {
    value
        .parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|_| format!("--timeout must be a whole number of seconds, got '{value}'"))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "Grammar tests assert the parsed invocation directly for precise diagnostics."
    )]

    use super::*;

    /// Parses one argument list the way the binary parses its environment.
    fn parsed(arguments: &[&str]) -> Result<Invocation, String> {
        parse(&mut lexopt::Parser::from_args(arguments.iter().copied()))
    }

    /// Returns the options of one parsed front-end invocation.
    fn options_of(invocation: Invocation) -> Options {
        match invocation {
            Invocation::Tui(options) | Invocation::Repl(options) => options,
            Invocation::Run { options, .. } => options,
            Invocation::Help => panic!("help carries no front-end options"),
        }
    }

    #[test]
    fn a_bare_command_line_selects_the_fullscreen_front_end_with_defaults() {
        let invocation = parsed(&[]).expect("a bare command line is valid");
        assert!(matches!(invocation, Invocation::Tui(_)));
        let default = options_of(invocation);
        assert_eq!(default.workspace(), None);
        assert_eq!(default.session(), None);
        assert!(!default.continue_session());
        assert_eq!(default.mode(), RunModeDto::Build);
        assert_eq!(default.timeout(), None);
        assert_eq!(default.format(), Format::Text);
    }

    #[test]
    fn every_command_name_parses_with_its_prompt() {
        assert!(matches!(parsed(&["tui"]), Ok(Invocation::Tui(_))));
        assert!(matches!(parsed(&["repl"]), Ok(Invocation::Repl(_))));
        let Invocation::Run { prompt, .. } =
            parsed(&["run", "read the roadmap"]).expect("run parses")
        else {
            panic!("run selects the headless command");
        };
        assert_eq!(prompt, "read the roadmap");
    }

    #[test]
    fn run_flags_parse_before_and_after_the_prompt() {
        let session = SessionId::new();
        let session_text = session.to_string();
        let invocation = parsed(&[
            "--format=json",
            "run",
            "hello",
            "--mode",
            "plan",
            "--timeout",
            "7",
            "--session",
            session_text.as_str(),
            "--workspace",
            "/workspace",
        ])
        .expect("the full option set parses");
        let Invocation::Run { prompt, options } = invocation else {
            panic!("run selects the headless command");
        };
        assert_eq!(prompt, "hello");
        assert_eq!(options.format(), Format::Json);
        assert_eq!(options.mode(), RunModeDto::Plan);
        assert_eq!(options.timeout(), Some(Duration::from_secs(7)));
        assert_eq!(options.session(), Some(session));
        assert!(!options.continue_session());
        assert_eq!(
            options.workspace(),
            Some(std::path::Path::new("/workspace"))
        );
        let json = options_of(parsed(&["run", "hello", "--format", "json"]).expect("json parses"));
        assert_eq!(json.format(), Format::Json);
        let continued = options_of(
            parsed(&["--continue", "--mode", "plan", "repl"]).expect("continue parses with repl"),
        );
        assert!(continued.continue_session());
        assert_eq!(continued.mode(), RunModeDto::Plan);
    }

    #[test]
    fn a_selected_session_and_a_continuation_are_mutually_exclusive() {
        let session_text = SessionId::new().to_string();
        let rejected = parsed(&[
            "run",
            "hello",
            "--session",
            session_text.as_str(),
            "--continue",
        ]);
        assert!(rejected.is_err(), "the two session selections conflict");
    }

    #[test]
    fn terminal_only_options_are_rejected_for_the_other_commands() {
        assert!(parsed(&["repl", "--format", "json"]).is_err());
        assert!(parsed(&["tui", "--timeout", "2"]).is_err());
        assert!(parsed(&["--format", "json"]).is_err());
    }

    #[test]
    fn malformed_commands_and_values_are_rejected() {
        assert!(
            parsed(&["watch"]).is_err(),
            "an unknown command is rejected"
        );
        assert!(
            parsed(&["--bogus"]).is_err(),
            "an unknown option is rejected"
        );
        assert!(
            parsed(&["tui", "--renderer", "revue"]).is_err(),
            "the removed renderer option is rejected"
        );
        assert!(parsed(&["run"]).is_err(), "a missing prompt is rejected");
        assert!(
            parsed(&["run", "  "]).is_err(),
            "a blank prompt is rejected"
        );
        assert!(
            parsed(&["run", "hello", "extra"]).is_err(),
            "a second positional argument is rejected"
        );
        assert!(parsed(&["repl", "prompt"]).is_err(), "repl takes no prompt");
        assert!(parsed(&["run", "hello", "--mode", "fast"]).is_err());
        assert!(parsed(&["run", "hello", "--format", "yaml"]).is_err());
        assert!(parsed(&["run", "hello", "--timeout", "soon"]).is_err());
        assert!(parsed(&["run", "hello", "--session", "not-an-identifier"]).is_err());
        assert!(parsed(&["run", "hello", "--workspace"]).is_err());
    }

    #[test]
    fn help_is_reported_wherever_it_appears() {
        assert!(matches!(parsed(&["--help"]), Ok(Invocation::Help)));
        assert!(matches!(
            parsed(&["run", "hello", "-h"]),
            Ok(Invocation::Help)
        ));
    }

    #[test]
    fn terminal_availability_requires_both_standard_streams() {
        assert!(terminal_available(true, true));
        assert!(!terminal_available(true, false));
        assert!(!terminal_available(false, true));
        assert!(!terminal_available(false, false));
    }

    #[test]
    fn typed_failures_map_to_their_process_status() {
        let unavailable = ErrorDto::unavailable("daemon_unavailable", "the daemon is unavailable");
        assert_eq!(ExitStatus::for_error(&unavailable), ExitStatus::Daemon);
        let rejected = ErrorDto::validation("invalid_request", "the request is invalid");
        assert_eq!(ExitStatus::for_error(&rejected), ExitStatus::Rejected);
    }

    #[test]
    fn terminal_run_statuses_map_to_their_process_status() {
        assert_eq!(
            ExitStatus::for_run_status(RunStatusDto::Completed),
            Some(ExitStatus::Completed)
        );
        assert_eq!(
            ExitStatus::for_run_status(RunStatusDto::Failed),
            Some(ExitStatus::Rejected)
        );
        assert_eq!(
            ExitStatus::for_run_status(RunStatusDto::Interrupted),
            Some(ExitStatus::Interrupted)
        );
        assert_eq!(ExitStatus::for_run_status(RunStatusDto::Starting), None);
        assert_eq!(ExitStatus::for_run_status(RunStatusDto::Running), None);
    }

    #[test]
    fn the_closed_status_set_uses_the_declared_codes() {
        for (status, code) in [
            (ExitStatus::Completed, 0_u8),
            (ExitStatus::Usage, 1),
            (ExitStatus::Daemon, 2),
            (ExitStatus::Rejected, 3),
            (ExitStatus::Timeout, 4),
            (ExitStatus::Interrupted, 5),
        ] {
            assert_eq!(status.code(), ExitCode::from(code));
        }
    }
}
