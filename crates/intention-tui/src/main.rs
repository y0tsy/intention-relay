//! The Intention Relay terminal client binary.
//!
//! One binary serves the three terminal front ends the slice declares: the
//! fullscreen UI, the interactive REPL, and the headless prompt command. The
//! binary resolves the process-level seams every front end shares — the local
//! endpoint, the daemon launcher, the workspace root, and the session runtime —
//! and dispatches exactly one front end. Command-line rejections and startup
//! failures go through explicit writers and the closed exit-status set of
//! [`cli`], and the process status is the value `main` returns.

mod cli;
mod headless;
mod repl;

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use intention_client::{IntentionClient, ProcessDaemonLauncher};
use intention_proto::{DtoResult, ErrorDto, WorkspaceRootDto};
use intention_transport::LocalEndpoint;
use mimalloc::MiMalloc;

use cli::{ExitStatus, Invocation, Options};

/// The environment variable that selects one local endpoint instance.
///
/// The value is a safe endpoint instance identifier; an installation that shares
/// a machine with another daemon addresses its own endpoint with it.
const ENDPOINT_VARIABLE: &str = "INTENTION_ENDPOINT";

/// The binary's process-wide allocator.
///
/// A terminal frame allocates many short-lived small strings - one per wrapped
/// row, one per styled span, one per status line - so the binary takes
/// mimalloc's small-allocation path instead of the system allocator's. The
/// allocator is the crate's own type and the workspace's `unsafe_code` ban
/// covers every line of ours: no pointer is dereferenced here.
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> ExitCode {
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let invocation = match cli::parse(&mut lexopt::Parser::from_env()) {
        Ok(invocation) => invocation,
        Err(message) => return usage_failure(&message, &mut stderr),
    };
    if invocation == Invocation::Help {
        let _ = stdout.write_all(cli::HELP.as_bytes());
        return ExitStatus::Completed.code();
    }
    dispatch(invocation, &mut stdout, &mut stderr)
}

/// Runs the front end one parsed invocation selects.
fn dispatch(invocation: Invocation, out: &mut impl Write, err: &mut impl Write) -> ExitCode {
    if matches!(invocation, Invocation::Tui(_)) && !console_attached() {
        return usage_failure(cli::TUI_REQUIRES_TERMINAL, err);
    }
    let (client, endpoint) = match open_client(err) {
        Ok(connected) => connected,
        Err(status) => return status.code(),
    };
    match invocation {
        Invocation::Help => ExitStatus::Completed.code(),
        Invocation::Tui(options) => run_tui(&options, client, endpoint, err),
        Invocation::Repl(options) => repl::run(&options, client, endpoint, out, err).code(),
        Invocation::Run { prompt, options } => {
            headless::run(&prompt, &options, client, endpoint, out, err).code()
        }
    }
}

/// Runs the fullscreen terminal front end over one connected client.
fn run_tui(
    options: &Options,
    client: IntentionClient,
    endpoint: LocalEndpoint,
    err: &mut impl Write,
) -> ExitCode {
    let workspace_root = match workspace_root(options.workspace()) {
        Ok(root) => root,
        Err(error) => return startup_failure(&error, err).code(),
    };
    let tui_options =
        intention_tui::tui::TuiOptions::new(workspace_root, options.mode(), options.session());
    match intention_tui::tui::run_blocking(tui_options, client, endpoint) {
        Ok(()) => ExitStatus::Completed.code(),
        Err(error) => startup_failure(&error, err).code(),
    }
}

/// Resolves the local endpoint and builds the shared client with its launcher.
fn open_client(err: &mut impl Write) -> Result<(IntentionClient, LocalEndpoint), ExitStatus> {
    let endpoint = match endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return Err(startup_failure(&error, err)),
    };
    let program = match daemon_program() {
        Ok(program) => program,
        Err(error) => return Err(startup_failure(&error, err)),
    };
    let launcher = match ProcessDaemonLauncher::new(program) {
        Ok(launcher) => launcher,
        Err(error) => return Err(startup_failure(&error, err)),
    };
    Ok((
        IntentionClient::new(endpoint.clone(), Box::new(launcher)),
        endpoint,
    ))
}

/// Resolves the local endpoint the front end talks to.
///
/// # Errors
///
/// Returns the typed endpoint failure when the configured instance identifier is
/// unsafe or the platform runtime directory cannot be determined.
fn endpoint() -> DtoResult<LocalEndpoint> {
    match std::env::var(ENDPOINT_VARIABLE) {
        Ok(instance_id) if !instance_id.trim().is_empty() => {
            LocalEndpoint::from_instance_id(instance_id)
        }
        _ => LocalEndpoint::platform_default(),
    }
}

/// Resolves the daemon program the client launches when none is running.
///
/// The daemon binary is the terminal binary's sibling in one installation
/// directory, so a development build and an installed pair both resolve without
/// configuration.
///
/// # Errors
///
/// Returns an unavailable error when the running executable has no resolvable
/// sibling daemon program.
fn daemon_program() -> DtoResult<String> {
    let unavailable = || {
        ErrorDto::unavailable(
            "daemon_program_unavailable",
            "the daemon program could not be resolved",
        )
    };
    let executable = std::env::current_exe().map_err(|_| unavailable())?;
    let directory = executable.parent().ok_or_else(unavailable)?;
    let program = directory.join(format!("intention-daemon{}", std::env::consts::EXE_SUFFIX));
    program.to_str().map(str::to_owned).ok_or_else(unavailable)
}

/// Resolves the workspace root one front end creates sessions with.
///
/// The default is the process working directory and a relative `--workspace`
/// value is resolved against it, so every accepted form addresses an absolute
/// native path.
///
/// # Errors
///
/// Returns a validation error when the path cannot be made absolute or is not
/// valid UTF-8 text.
fn workspace_root(workspace: Option<&Path>) -> DtoResult<WorkspaceRootDto> {
    let absolute =
        std::path::absolute(workspace.unwrap_or_else(|| Path::new("."))).map_err(|_| {
            ErrorDto::validation(
                "invalid_workspace_root",
                "the workspace root could not be resolved",
            )
        })?;
    let text = absolute.to_str().ok_or_else(|| {
        ErrorDto::validation(
            "invalid_workspace_root",
            "the workspace root must be valid UTF-8 text",
        )
    })?;
    WorkspaceRootDto::parse(text)
}

/// Creates the current-thread runtime the line front ends drive.
///
/// # Errors
///
/// Returns an unavailable error when the runtime cannot be created.
pub(crate) fn session_runtime() -> DtoResult<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| {
            ErrorDto::unavailable(
                "terminal_runtime_unavailable",
                "the terminal runtime could not be created",
            )
        })
}

/// Returns whether the fullscreen front end has the terminal it needs.
fn console_attached() -> bool {
    cli::terminal_available(
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )
}

/// Reports one safe typed startup failure and returns its process status.
pub(crate) fn startup_failure(error: &ErrorDto, err: &mut impl Write) -> ExitStatus {
    let _ = writeln!(err, "error: {} ({})", error.message(), error.code());
    ExitStatus::for_error(error)
}

/// Reports one command-line rejection and returns the usage process status.
fn usage_failure(message: &str, err: &mut impl Write) -> ExitCode {
    let _ = writeln!(err, "error: {message}");
    let _ = writeln!(err, "{}", cli::USAGE);
    ExitStatus::Usage.code()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Workspace resolution tests assert one resolved native root."
    )]

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn an_absolute_workspace_root_is_carried_through() {
        let temporary = std::env::temp_dir();
        let root = workspace_root(Some(&temporary)).expect("the temporary directory is absolute");
        assert_eq!(PathBuf::from(root.as_str()), temporary);
    }

    #[test]
    fn no_workspace_option_defaults_to_the_working_directory() {
        let default = workspace_root(None).expect("the working directory is a root");
        let working = std::path::absolute(Path::new(".")).expect("the working directory resolves");
        assert_eq!(PathBuf::from(default.as_str()), working);
        assert!(Path::new(default.as_str()).is_absolute());
    }

    #[test]
    fn a_relative_workspace_root_resolves_against_the_working_directory() {
        let root = workspace_root(Some(Path::new("relative/root"))).expect("the path resolves");
        let resolved = PathBuf::from(root.as_str());
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with(Path::new("relative").join("root")));
    }
}
