//! The Intention Relay line client binary.
//!
//! One binary serves the two line front ends: the interactive REPL and the
//! headless prompt command. The fullscreen terminal UI is a separate
//! application that speaks the same protocol, so this binary neither names nor
//! proxies it. The binary consults the process-level seams both front ends
//! share — the local endpoint and the daemon launcher the client declares, the
//! workspace root, and the session runtime — and dispatches exactly one front
//! end. Command-line rejections and startup failures go through explicit
//! writers and the closed exit-status set of [`cli`], and the process status is
//! the value `main` returns.

mod app;
mod cli;
mod client_task;
mod headless;
mod repl;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use intention_client::{IntentionClient, ProcessDaemonLauncher, endpoint_discovery};
use intention_proto::{DtoResult, ErrorDto, WorkspaceRootDto};
use intention_transport::LocalEndpoint;

use cli::{ExitStatus, Invocation};

fn main() -> ExitCode {
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let invocation = match cli::parse(&mut lexopt::Parser::from_env()) {
        Ok(invocation) => invocation,
        Err(message) => return usage_failure(&message, &mut stderr),
    };
    if invocation == Invocation::Help {
        let _ = stdout.write_all(cli::help().as_bytes());
        return ExitStatus::Completed.code();
    }
    dispatch(invocation, &mut stdout, &mut stderr)
}

/// Runs the front end one parsed invocation selects.
fn dispatch(invocation: Invocation, out: &mut impl Write, err: &mut impl Write) -> ExitCode {
    let (client, endpoint) = match open_client(err) {
        Ok(connected) => connected,
        Err(status) => return status.code(),
    };
    match invocation {
        Invocation::Help => ExitStatus::Completed.code(),
        Invocation::Repl(options) => repl::run(&options, client, endpoint, out, err).code(),
        Invocation::Run { prompt, options } => {
            headless::run(&prompt, &options, client, endpoint, out, err).code()
        }
    }
}

/// Builds the shared client from the surfaces the client declares.
///
/// The local endpoint instance and the daemon program source are the client's
/// declared inputs; the binary carries no environment read and no installation
/// path inference.
fn open_client(err: &mut impl Write) -> Result<(IntentionClient, LocalEndpoint), ExitStatus> {
    let endpoint = match endpoint_discovery() {
        Ok(endpoint) => endpoint,
        Err(error) => return Err(startup_failure(&error, err)),
    };
    let launcher = match ProcessDaemonLauncher::platform_default() {
        Ok(launcher) => launcher,
        Err(error) => return Err(startup_failure(&error, err)),
    };
    Ok((
        IntentionClient::new(endpoint.clone(), Box::new(launcher)),
        endpoint,
    ))
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

/// Reports one safe typed startup failure and returns its process status.
pub(crate) fn startup_failure(error: &ErrorDto, err: &mut impl Write) -> ExitStatus {
    let _ = writeln!(err, "error: {error}");
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
