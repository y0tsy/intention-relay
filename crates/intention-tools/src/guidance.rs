//! Model-visible guidance for one failed tool call.
//!
//! Guidance is tool-result content, not an `ErrorDto`: it explains the failed
//! call in code-owned language so the model can correct the call and continue
//! the run. Its only dynamic detail is the failure message — the addressed
//! workspace-relative path and the underlying error. It carries no absolute
//! workspace root, no resolved symlink target, no command line, no file
//! content, and no credential, and it stays inside
//! `MAX_TOOL_GUIDANCE_BYTES`.

use intention_proto::{ErrorDto, ToolId};

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
        content.push_str(&suggestion);
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
fn suggestion(tool_id: &str, code: &str) -> Option<String> {
    let suggestion = match code {
        "unknown_tool" => {
            "the tool name is not one of the six exposed tools: read, write, edit, execute, glob, grep"
        }
        "invalid_tool_input_json" => return Some(expected_arguments(tool_id)),
        "invalid_tool_text" => {
            "tool text must be non-blank and free of NUL bytes; inspect binary files with execute instead of read"
        }
        "invalid_tool_pattern" => {
            "the pattern is workspace-relative; remove any leading root or parent component and retry"
        }
        "invalid_tool_env" => {
            "use a plain environment variable name without '=', NUL, or control characters"
        }
        "invalid_tool_timeout" => {
            "timeout_seconds is a whole number of seconds and must be at least 1"
        }
        "edit_target_missing" => {
            "the exact `old` text was not found; read the file again and pass text that appears verbatim"
        }
        "tool_edit_target_too_large" => {
            "the file is too large to edit; split the change or rewrite the file with write"
        }
        "tool_read_failed" => {
            "the path could not be read; check that it exists and stays workspace-relative"
        }
        "tool_search_failed" => {
            "the search target could not be read; check that the path exists and matches the scope kind"
        }
        "tool_write_conflict" | "tool_edit_conflict" => {
            "the file changed before the call; read it again and retry with its current content"
        }
        "tool_write_failed" => {
            "the file could not be written; check that its parent directory exists"
        }
        "tool_shell_unavailable" => {
            "the platform shell is not installed; report this instead of retrying"
        }
        "tool_execute_spawn_failed" => {
            "the shell could not start; verify the workspace and the command text"
        }
        _ => return None,
    };
    Some(suggestion.to_owned())
}

/// Returns the expected arguments of one exposed tool.
///
/// The tool's own input schema document is the single owner of the argument
/// contract, so the guidance renders that document instead of repeating its
/// argument names and required flags.
fn expected_arguments(tool_id: &str) -> String {
    ToolId::parse(tool_id).map_or_else(
        |_| "the arguments must be a JSON object matching the tool's input schema".to_owned(),
        |id| {
            format!(
                "the arguments must match this JSON Schema: {}",
                crate::spec(id).input_schema()
            )
        },
    )
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
        assert!(guidance.contains("JSON Schema"));
        assert!(guidance.contains(&crate::spec(ToolId::Execute).input_schema()));
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
