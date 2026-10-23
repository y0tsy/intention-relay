//! Private platform shell adapter for the `execute` tool.
//!
//! The shell executable, its flags, and its operand shape are code-owned tool
//! semantics: the model supplies one command text and never selects the
//! interpreter (architecture 15). The adapter keeps the platform shell — bash
//! on Unix and cmd on Windows — behind this module, so no public DTO carries
//! an executable path or any other platform resource.

use std::process::Command;

/// Builds the platform shell invocation for one command text.
pub fn command(command_text: &str) -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(command_text);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new("bash");
        command.arg("-c").arg(command_text);
        command
    }
}

/// Classifies one failed shell spawn safely.
///
/// A missing platform shell is an availability outcome the model can act on;
/// every other spawn failure keeps the generic safe failure. Both report the
/// underlying OS error verbatim and never the command text.
pub fn spawn_failure(error: &std::io::Error) -> intention_proto::ErrorDto {
    if error.kind() == std::io::ErrorKind::NotFound {
        intention_proto::ErrorDto::owned_message(
            "tool_shell_unavailable",
            intention_proto::ErrorCategoryDto::Unavailable,
            format!("the platform shell interpreter is unavailable: {error}"),
        )
    } else {
        intention_proto::ErrorDto::owned_message(
            "tool_execute_spawn_failed",
            intention_proto::ErrorCategoryDto::Validation,
            format!("unable to spawn the workspace command: {error}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_shell_is_an_availability_outcome() {
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "fixture shell missing");
        let failure = spawn_failure(&missing);
        assert_eq!(failure.code(), "tool_shell_unavailable");
        assert!(failure.message().contains("fixture shell missing"));
        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "fixture denial");
        let failure = spawn_failure(&denied);
        assert_eq!(failure.code(), "tool_execute_spawn_failed");
        assert!(failure.message().contains("fixture denial"));
    }
}
