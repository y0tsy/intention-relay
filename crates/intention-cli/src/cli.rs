//! Command-line grammar and process status of the terminal client binary.
//!
//! One grammar serves the two line front ends: the bare command line and `repl`
//! select the interactive line loop, and `run <PROMPT>` selects the headless
//! command. The fullscreen terminal UI is a separate application with its own
//! entry point, so this grammar neither names nor proxies it. The module owns
//! the complete user-facing text (help, usage, rejections) and the closed
//! exit-status set, so no other terminal module decides what a rejection or a
//! run end reports.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use intention_proto::{ErrorCategoryDto, ErrorDto, RunModeDto, RunStatusDto, SessionId};

/// The one-line usage every command-line rejection prints.
pub const USAGE: &str = "usage: intention-cli [repl | run <PROMPT>] [OPTIONS]";

/// The complete help text `-h` and `--help` print.
///
/// The run-mode line renders the set `RunModeDto` declares and the default it
/// declares, so the help cannot drift from the wire vocabulary.
#[must_use]
pub fn help() -> String {
    let modes = RunModeDto::ALL.map(RunModeDto::as_str).join("|");
    let default = RunModeDto::DEFAULT.as_str();
    format!(
        "Intention Relay terminal client

usage:
  intention-cli [repl] [OPTIONS]         interactive line REPL
  intention-cli run <PROMPT> [OPTIONS]   one headless prompt

options:
  --workspace PATH    workspace root for a new session (default: current directory)
  --session ID        open an existing session by its durable identifier
  --continue          continue the most recently updated session
  --mode {modes}   run policy of a created session (default: {default})
  --timeout SECS      stop the run when it does not finish within SECS
  --format text|json  run output format: progressive text or NDJSON records
  -h, --help          print this help

The fullscreen terminal UI is a separate application; see apps/tui.
"
    )
}

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
    mode: Option<RunModeDto>,
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

    /// Returns the run policy a created session starts with, if the caller
    /// declared one.
    ///
    /// The grammar carries no default: an absent `--mode` leaves the durable
    /// policy of a created session to the core.
    #[must_use]
    pub const fn mode(&self) -> Option<RunModeDto> {
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
/// The codes are the process contract: `0` completed, `1` usage, `2` daemon or
/// transport, `3` typed rejection or failed run, `4` the wait deadline expired
/// with no terminal status observed, and `5` interrupted run.
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
    /// The run wait deadline expired and no terminal status arrived within the
    /// grace.
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
    // The flag carries only a mode the caller spelled: the durable default of a
    // created session is the core's, so the grammar substitutes none.
    let mut mode = None;
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
            Long("mode") => mode = Some(mode_value(&text(parser)?)?),
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

    let command = command.unwrap_or(Command::Repl);
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
    RunModeDto::parse(value).map_err(|_| {
        let modes = RunModeDto::ALL.map(RunModeDto::as_str).join(" or ");
        format!("--mode must be {modes}, got '{value}'")
    })
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

    use super::*;

    /// Parses one argument list the way the binary parses its environment.
    fn parsed(arguments: &[&str]) -> Result<Invocation, String> {
        parse(&mut lexopt::Parser::from_args(arguments.iter().copied()))
    }

    /// Returns the options of one parsed front-end invocation.
    fn options_of(invocation: Invocation) -> Options {
        match invocation {
            Invocation::Repl(options) => options,
            Invocation::Run { options, .. } => options,
            Invocation::Help => panic!("help carries no front-end options"),
        }
    }

    #[test]
    fn a_bare_command_line_selects_the_repl_with_defaults() {
        let invocation = parsed(&[]).expect("a bare command line is valid");
        assert!(matches!(invocation, Invocation::Repl(_)));
        let default = options_of(invocation);
        assert_eq!(default.workspace(), None);
        assert_eq!(default.session(), None);
        assert!(!default.continue_session());
        assert_eq!(default.mode(), None);
        assert_eq!(default.timeout(), None);
        assert_eq!(default.format(), Format::Text);
    }

    #[test]
    fn the_fullscreen_front_end_is_not_part_of_this_grammar() {
        let rejected = parsed(&["tui"]).expect_err("the grammar declares no tui command");
        assert_eq!(rejected, "unknown command 'tui'");
        let theme = parsed(&["--theme", "dark"]).expect_err("the grammar declares no theme flag");
        assert!(
            theme.contains("--theme"),
            "the rejection names the flag the grammar does not take"
        );
        assert!(
            !help().contains("--theme"),
            "the help never offers a flag this binary does not parse"
        );
    }

    #[test]
    fn an_absent_mode_leaves_the_default_to_the_core() {
        let absent = options_of(parsed(&["repl"]).expect("repl parses"));
        assert_eq!(
            absent.mode(),
            None,
            "the grammar carries no run policy of its own"
        );
        let spelled = options_of(parsed(&["repl", "--mode", "plan"]).expect("repl parses"));
        assert_eq!(
            spelled.mode(),
            Some(RunModeDto::Plan),
            "the flag carries only the mode the caller spelled"
        );
    }

    #[test]
    fn the_usage_and_rejection_spell_the_declared_mode_set() {
        let declared = RunModeDto::ALL.map(RunModeDto::as_str).join("|");
        let help = help();
        assert!(
            help.contains(&format!("--mode {declared} ")),
            "the help renders the declared run-mode set"
        );
        assert!(
            help.contains(RunModeDto::DEFAULT.as_str()),
            "the help renders the declared durable default"
        );
        let rejection =
            mode_value("fast").expect_err("a mode the vocabulary does not declare is rejected");
        let spelled = RunModeDto::ALL.map(RunModeDto::as_str).join(" or ");
        assert!(
            rejection.contains(&spelled),
            "the rejection names exactly the declared run-mode set"
        );
    }

    #[test]
    fn every_command_name_parses_with_its_prompt() {
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
        assert_eq!(options.mode(), Some(RunModeDto::Plan));
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
        assert_eq!(continued.mode(), Some(RunModeDto::Plan));
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
        assert!(parsed(&["--timeout", "2"]).is_err());
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
            parsed(&["repl", "--renderer", "ratatui"]).is_err(),
            "an option the grammar never declared is rejected"
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
        let declared = RunModeDto::ALL.map(RunModeDto::as_str).join(" or ");
        assert_eq!(
            parsed(&["run", "hello", "--mode", "fast"]),
            Err(format!("--mode must be {declared}, got 'fast'")),
        );
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
