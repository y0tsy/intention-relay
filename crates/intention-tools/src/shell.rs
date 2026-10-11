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
/// every other spawn failure keeps the generic safe failure.
pub fn spawn_failure(error: &std::io::Error) -> intention_proto::ErrorDto {
    if error.kind() == std::io::ErrorKind::NotFound {
        intention_proto::ErrorDto::unavailable(
            "tool_shell_unavailable",
            "the platform shell interpreter is unavailable",
        )
    } else {
        intention_proto::ErrorDto::validation(
            "tool_execute_spawn_failed",
            "unable to spawn workspace command",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_shell_is_an_availability_outcome() {
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(spawn_failure(&missing).code(), "tool_shell_unavailable");
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(spawn_failure(&denied).code(), "tool_execute_spawn_failed");
    }
}
