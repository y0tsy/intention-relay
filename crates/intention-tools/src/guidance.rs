//! Model-visible guidance for one failed tool call.
//!
//! Guidance is tool-result content, not an `ErrorDto`: it explains the failed
//! call in code-owned language so the model can correct the call and continue
//! the run. Its only dynamic detail is the failure message — the addressed
//! workspace-relative path and the underlying error. It carries no absolute
//! workspace root, no resolved symlink target, no command line, no file
//! content, and no credential, and it stays inside
//! `MAX_TOOL_GUIDANCE_BYTES`.

use intention_proto::ErrorDto;

use crate::MAX_TOOL_GUIDANCE_BYTES;

/// Renders the bounded, model-visible guidance for one failed call.
#[must_use]
pub fn tool_failure_guidance(tool_id: &str, error: &ErrorDto) -> String {
    let mut content = format!(
        "the tool call failed\ntool: {}\n{error}\n",
        safe_name(tool_id)
    );
    if let Some(suggestion) = suggestion(tool_id, error.code()) {
        content.push_str("suggestion: ");
        content.push_str(suggestion);
        content.push('\n');
    }
    truncate(content)
}

/// Normalizes the model-requested tool name for the guidance text.
///
/// The name comes from the model, so only printable ASCII stays and the
/// rendering is capped; an empty result keeps a visible placeholder.
fn safe_name(tool_id: &str) -> String {
    let name: String = tool_id
        .chars()
        .filter(|character| character.is_ascii_graphic())
        .take(64)
        .collect();
    if name.is_empty() {
        "?".to_owned()
    } else {
        name
    }
}

/// Returns the code-owned suggestion for one failure classification.
fn suggestion(tool_id: &str, code: &str) -> Option<&'static str> {
    match code {
        "unknown_tool" => Some(
            "the tool name is not one of the six exposed tools: read, write, edit, execute, glob, grep",
        ),
        "invalid_tool_input_json" => Some(expected_arguments(tool_id)),
        "invalid_tool_text" => Some(
            "tool text must be non-blank and free of NUL bytes; inspect binary files with execute instead of read",
        ),
        "invalid_tool_pattern" => Some(
            "the pattern is workspace-relative; remove any leading root or parent component and retry",
        ),
        "invalid_tool_env" => {
            Some("use a plain environment variable name without '=', NUL, or control characters")
        }
        "invalid_tool_timeout" => {
            Some("timeout_seconds is a whole number of seconds and must be at least 1")
        }
        "edit_target_missing" => Some(
            "the exact `old` text was not found; read the file again and pass text that appears verbatim",
        ),
        "tool_edit_target_too_large" => {
            Some("the file is too large to edit; split the change or rewrite the file with write")
        }
        "tool_read_failed" => {
            Some("the path could not be read; check that it exists and stays workspace-relative")
        }
        "tool_search_failed" => Some(
            "the search target could not be read; check that the path exists and matches the scope kind",
        ),
        "tool_write_conflict" | "tool_edit_conflict" => Some(
            "the file changed before the call; read it again and retry with its current content",
        ),
        "tool_write_failed" => {
            Some("the file could not be written; check that its parent directory exists")
        }
        "tool_shell_unavailable" => {
            Some("the platform shell is not installed; report this instead of retrying")
        }
        "tool_execute_spawn_failed" => {
            Some("the shell could not start; verify the workspace and the command text")
        }
        _ => None,
    }
}

/// Returns the expected arguments of one exposed tool.
fn expected_arguments(tool_id: &str) -> &'static str {
    match tool_id {
        "read" => "expected arguments: path (required)",
        "write" => {
            "expected arguments: path (required), content (required), expected_content (optional)"
        }
        "edit" => {
            "expected arguments: path (required), old (required), new (required), expected_content (optional)"
        }
        "execute" => {
            "expected arguments: command (required), cwd (optional), env (optional), timeout_seconds (optional)"
        }
        "glob" => "expected arguments: pattern (required)",
        "grep" => {
            "expected arguments: pattern (required), path (optional), scope (optional; kind file|directory|workspace with its path)"
        }
        _ => "the arguments must be a JSON object matching the tool's input schema",
    }
}

/// Keeps the guidance inside its byte window with an honest marker.
fn truncate(mut content: String) -> String {
    if content.len() <= MAX_TOOL_GUIDANCE_BYTES {
        return content;
    }
    let mut end = MAX_TOOL_GUIDANCE_BYTES;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    content.truncate(end);
    content.push_str("\n[truncated]");
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_guidance_names_the_exposed_tools() {
        let guidance = tool_failure_guidance(
            "frobnicate",
            &ErrorDto::validation(
                "unknown_tool",
                "tool is not registered for model invocation",
            ),
        );
        assert!(guidance.contains("frobnicate"));
        assert!(guidance.contains("read, write, edit, execute, glob, grep"));
    }

    #[test]
    fn guidance_carries_the_expected_arguments_of_an_undecodable_call() {
        let guidance = tool_failure_guidance(
            "execute",
            &ErrorDto::validation(
                "invalid_tool_input_json",
                "tool arguments are not valid typed input",
            ),
        );
        assert!(guidance.contains("command (required)"));
        assert!(guidance.contains("timeout_seconds (optional)"));
    }

    #[test]
    fn control_characters_stay_out_of_the_guidance() {
        let guidance = tool_failure_guidance(
            "bad\nname\u{7}",
            &ErrorDto::validation(
                "unknown_tool",
                "tool is not registered for model invocation",
            ),
        );
        assert!(!guidance.contains('\u{7}'));
        assert!(guidance.contains("badname"));
    }

    #[test]
    fn guidance_stays_bounded_and_non_blank() {
        let guidance = tool_failure_guidance(
            &"x".repeat(4096),
            &ErrorDto::validation(
                "unknown_tool",
                "tool is not registered for model invocation",
            ),
        );
        assert!(!guidance.trim().is_empty());
        assert!(guidance.len() <= MAX_TOOL_GUIDANCE_BYTES + "\n[truncated]".len());
    }
}
