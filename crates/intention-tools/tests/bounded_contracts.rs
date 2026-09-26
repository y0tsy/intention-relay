#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "Integration tests use expect and unwrap only for deterministic fixture setup; failures indicate a broken test fixture."
)]

//! Regression coverage for PR24-022 and PR24-023: tool text validates on
//! Deserialize at the JSON boundary, execute invocations carry argument count
//! and aggregate caps, and file-processing tools stay memory-bounded.

use intention_domain::WorkspaceRootDto;
use intention_tools::{
    BoundedText, CancellationSignal, EditInput, ExecuteInput, GrepInput, GrepScope, ReadInput,
    ToolInput, ToolResult, ToolService, WriteInput,
};
use intention_types::{ToolCallId, WorkspaceRelativePathDto};
use serde_json::json;
use tempfile::TempDir;

fn fixture_dir(label: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(&format!("intention-tools-bounded-{label}-"))
        .tempdir()
        .expect("temporary workspace")
}

fn workspace(root: &TempDir) -> intention_workspace::WorkspaceRoot {
    intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.path().to_string_lossy().into_owned()).expect("root dto"),
    )
    .expect("workspace")
}

fn relative(path: &str) -> WorkspaceRelativePathDto {
    WorkspaceRelativePathDto::parse(path).expect("relative path")
}

#[test]
fn bounded_text_deserialize_rejects_oversized_and_nul_text() {
    let oversized = "x".repeat(1024 * 1024 + 1);
    let error = serde_json::from_value::<BoundedText>(json!(oversized))
        .expect_err("oversized text must fail deserialization");
    assert!(
        error
            .to_string()
            .contains("invalid tool text (invalid_tool_text)"),
        "the deserialize error must surface the validation code: {error}"
    );
    let error = serde_json::from_value::<BoundedText>(json!("nul\0inside")).expect_err("NUL fails");
    assert!(error.to_string().contains("invalid_tool_text"));
    assert_eq!(
        serde_json::from_value::<BoundedText>(json!("valid")).expect("valid text decodes"),
        BoundedText::new("valid").expect("fixture")
    );
}

#[test]
fn tool_input_execute_deserialize_rejects_excessive_argument_shapes() {
    let too_many = json!({
        "tool": "execute",
        "input": {
            "program": "echo",
            "args": (0..129).map(|index| format!("arg-{index}")).collect::<Vec<_>>(),
        },
    });
    let error = serde_json::from_value::<ToolInput>(too_many)
        .expect_err("129 arguments must fail deserialization");
    assert!(error.to_string().contains("invalid execute input"));
    let oversized = json!({
        "tool": "execute",
        "input": {
            "program": "echo",
            "args": vec!["x".repeat(256 * 1024 + 1)],
        },
    });
    assert!(
        serde_json::from_value::<ToolInput>(oversized).is_err(),
        "an aggregate argument payload beyond 256 KiB must fail deserialization"
    );
}

#[test]
fn execute_dispatch_rejects_in_memory_argument_cap_violations() {
    let service = ToolService::new(workspace(&fixture_dir("execute-bounds")));
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("echo").expect("program"),
            args: (0..129)
                .map(|index| BoundedText::new(format!("arg-{index}")).expect("argument"))
                .collect(),
        }),
        CancellationSignal::new(),
    );
    let Err(error) = result else {
        panic!("argument cap violation must be rejected");
    };
    assert_eq!(error.code(), "invalid_tool_execute_arguments");
}

#[test]
fn edit_rejects_targets_larger_than_the_edit_bound() {
    let root_dir = fixture_dir("edit-large");
    std::fs::write(
        root_dir.path().join("huge.txt"),
        vec![b'a'; 1024 * 1024 + 8],
    )
    .expect("seed oversized file");
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Edit(EditInput {
            path: relative("huge.txt"),
            old: BoundedText::new("needle").expect("old"),
            new: BoundedText::new("replacement").expect("new"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    let Err(error) = result else {
        panic!("oversized edit target must be rejected");
    };
    assert_eq!(error.code(), "tool_edit_target_too_large");
}

#[test]
fn write_expected_content_never_reads_files_beyond_the_bounded_check() {
    let root_dir = fixture_dir("write-expected-large");
    std::fs::write(
        root_dir.path().join("huge.txt"),
        vec![b'x'; 1024 * 1024 + 8],
    )
    .expect("seed oversized file");
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Write(WriteInput {
            path: relative("huge.txt"),
            content: BoundedText::new("after").expect("content"),
            expected_content: Some(BoundedText::new("before").expect("expected")),
        }),
        CancellationSignal::new(),
    );
    let Err(error) = result else {
        panic!("a file larger than any bounded expected content must conflict");
    };
    assert_eq!(error.code(), "tool_write_conflict");
}

#[test]
fn directory_grep_caps_scanned_file_content_and_retained_aggregate() {
    let root_dir = fixture_dir("grep-bounds");
    let haystack = root_dir.path().join("haystack");
    std::fs::create_dir(&haystack).expect("haystack directory");
    let service = ToolService::new(workspace(&root_dir));
    // The match lives beyond the bounded per-file read window.
    let mut large = vec![b'\n'; 70 * 1024];
    large.extend_from_slice(b"needle-in-the-tail\n");
    std::fs::write(haystack.join("tail.txt"), large).expect("seed tail file");
    let result = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle-in-the-tail").expect("pattern"),
                scope: Some(GrepScope::Directory {
                    path: relative("haystack"),
                }),
                path: None,
            }),
            CancellationSignal::new(),
        )
        .expect("directory grep dispatches");
    let ToolResult::Grep(grep) = result else {
        unreachable!("grep returns a grep result")
    };
    assert!(
        grep.truncated,
        "a file larger than the bounded read window must be reported truncated"
    );
    assert_eq!(
        grep.matches.len(),
        0,
        "matches beyond the bounded window must not be reported"
    );

    // Many long matching lines exceed the aggregate retained-fragment bound.
    for index in 0..200 {
        std::fs::write(
            haystack.join(format!("match-{index}.txt")),
            format!("prefix-{index} {}", "y".repeat(700)),
        )
        .expect("seed matching file");
    }
    let result = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("prefix-").expect("pattern"),
                scope: Some(GrepScope::Directory {
                    path: relative("haystack"),
                }),
                path: None,
            }),
            CancellationSignal::new(),
        )
        .expect("directory grep dispatches");
    let ToolResult::Grep(grep) = result else {
        unreachable!("grep returns a grep result")
    };
    assert!(grep.truncated);
    let retained: usize = grep
        .matches
        .iter()
        .map(|matched| matched.fragment.as_str().len())
        .sum();
    assert!(
        retained <= 128 * 1024,
        "the retained fragment aggregate must stay within its bound ({retained})"
    );
    assert!(
        grep.matches.len() < 200,
        "the aggregate bound must clamp the retained match set"
    );
}

#[test]
fn read_and_grep_dispatches_remain_bounded_after_all_changes() {
    let root_dir = fixture_dir("read-bounds");
    std::fs::write(root_dir.path().join("file.txt"), "content").expect("seed");
    let service = ToolService::new(workspace(&root_dir));
    assert!(matches!(
        service.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Read(ReadInput {
                path: relative("file.txt"),
            }),
            CancellationSignal::new(),
        ),
        Ok(ToolResult::Read(_))
    ));
}

#[test]
fn pattern_only_file_grep_matches_bounded_lines_and_rejects_invalid_targets() {
    // PR24-022: the pattern-only grep path (no scope) reads one workspace
    // file with the same bounded window and aggregate caps as scoped greps,
    // and fails closed for missing, non-file, and absent targets.
    let root_dir = fixture_dir("pattern-grep");
    let haystack = format!("plain line\nprefix-{} needle\n{}", "x".repeat(700), "tail");
    std::fs::write(root_dir.path().join("needles.txt"), haystack).expect("seed");
    let service = ToolService::new(workspace(&root_dir));
    let result = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").expect("pattern"),
                scope: None,
                path: Some(relative("needles.txt")),
            }),
            CancellationSignal::new(),
        )
        .expect("pattern-only file grep dispatches");
    let ToolResult::Grep(grep) = result else {
        unreachable!("grep returns a grep result")
    };
    assert!(
        grep.matches.iter().any(|matched| matched.line == 2),
        "the bounded file window retains the matching line"
    );
    let retained: usize = grep
        .matches
        .iter()
        .map(|matched| matched.fragment.as_str().len())
        .sum();
    assert!(retained <= 128 * 1024);

    let missing = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").expect("pattern"),
            scope: None,
            path: Some(relative("missing.txt")),
        }),
        CancellationSignal::new(),
    );
    assert!(
        matches!(missing, Err(error) if error.code() == "workspace_path_unavailable"),
        "a missing pattern-only target fails closed at workspace resolution"
    );

    let no_path = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").expect("pattern"),
            scope: None,
            path: None,
        }),
        CancellationSignal::new(),
    );
    assert!(matches!(no_path, Err(error) if error.code() == "invalid_tool_path"));

    std::fs::create_dir(root_dir.path().join("sub")).expect("directory seeds");
    let directory = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").expect("pattern"),
            scope: None,
            path: Some(relative("sub")),
        }),
        CancellationSignal::new(),
    );
    assert!(matches!(directory, Err(error) if error.code() == "tool_search_failed"));
}

#[test]
fn write_expected_content_conflicts_when_target_is_missing() {
    // PR24-022: an expected-content write preflights the existing file with a
    // bounded read. A target that does not exist cannot carry the expected
    // content, so the preflight fails closed as a conflict and nothing is
    // created.
    let root_dir = fixture_dir("write-missing-expected");
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Write(WriteInput {
            path: relative("new.txt"),
            content: BoundedText::new("created").expect("content"),
            expected_content: Some(BoundedText::new("before").expect("expected")),
        }),
        CancellationSignal::new(),
    );
    let Err(error) = result else {
        panic!("expected-content write to a missing target must conflict");
    };
    assert_eq!(error.code(), "tool_write_conflict");
    assert!(
        !root_dir.path().join("new.txt").exists(),
        "a conflicting preflight must not create the target"
    );
}

#[test]
fn write_expected_content_conflicts_on_invalid_utf8_existing_file() {
    // PR24-022: expected-content equality compares the bounded read as valid
    // UTF-8. A file that is not valid UTF-8 can never equal the expected
    // text, so the preflight fails closed instead of comparing lossy bytes.
    let root_dir = fixture_dir("write-expected-invalid-utf8");
    let bytes = vec![0xff, 0xfe, 0x00, 0x80];
    std::fs::write(root_dir.path().join("binary.dat"), &bytes).expect("seed binary file");
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Write(WriteInput {
            path: relative("binary.dat"),
            content: BoundedText::new("after").expect("content"),
            expected_content: Some(BoundedText::new("before").expect("expected")),
        }),
        CancellationSignal::new(),
    );
    let Err(error) = result else {
        panic!("expected-content write over a non-UTF-8 file must conflict");
    };
    assert_eq!(error.code(), "tool_write_conflict");
    assert_eq!(
        std::fs::read(root_dir.path().join("binary.dat")).expect("re-read binary file"),
        bytes,
        "a conflicting preflight must not mutate the target"
    );
}

#[test]
fn edit_rejects_invalid_utf8_target_before_any_mutation() {
    // PR24-022: the edit tool reads its complete target as valid UTF-8 before
    // applying a replacement. A non-UTF-8 target is a typed read failure and
    // is left untouched.
    let root_dir = fixture_dir("edit-invalid-utf8");
    let bytes = vec![0xff, 0xfe, 0x00, 0x80];
    std::fs::write(root_dir.path().join("binary.dat"), &bytes).expect("seed binary file");
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Edit(EditInput {
            path: relative("binary.dat"),
            old: BoundedText::new("needle").expect("old"),
            new: BoundedText::new("replacement").expect("new"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    let Err(error) = result else {
        panic!("edit of a non-UTF-8 target must fail closed");
    };
    assert_eq!(error.code(), "tool_read_failed");
    assert_eq!(
        std::fs::read(root_dir.path().join("binary.dat")).expect("re-read binary file"),
        bytes,
        "a failed edit must not mutate the target"
    );
}

#[test]
fn single_file_grep_stops_at_the_match_cap_and_truncates() {
    // The pattern-only file grep (no scope) clamps its retained result set at
    // the match cap even when the aggregate fragment bound is not reached,
    // reporting the drop through the truncation flag.
    let root_dir = fixture_dir("grep-match-cap");
    let mut haystack = String::new();
    for _ in 0..(10_000 + 1) {
        haystack.push_str("a\n");
    }
    std::fs::write(root_dir.path().join("many.txt"), haystack).expect("seed many matches");
    let service = ToolService::new(workspace(&root_dir));
    let result = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("a").expect("pattern"),
                scope: None,
                path: Some(relative("many.txt")),
            }),
            CancellationSignal::new(),
        )
        .expect("file grep dispatches");
    let ToolResult::Grep(grep) = result else {
        unreachable!("grep returns a grep result")
    };
    assert_eq!(grep.matches.len(), 10_000);
    assert!(grep.truncated, "the dropped matches must set truncation");
}

#[test]
fn scoped_directory_grep_stops_at_the_match_cap_and_truncates() {
    // The scoped grep path applies the same per-file match cap: a single
    // directory entry with more matches than the cap is clamped and reported
    // truncated.
    let root_dir = fixture_dir("scoped-grep-match-cap");
    let haystack = root_dir.path().join("haystack");
    std::fs::create_dir(&haystack).expect("haystack directory");
    let mut content = String::new();
    for _ in 0..(10_000 + 1) {
        content.push_str("a\n");
    }
    std::fs::write(haystack.join("many.txt"), content).expect("seed many matches");
    let service = ToolService::new(workspace(&root_dir));
    let result = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("a").expect("pattern"),
                scope: Some(GrepScope::Directory {
                    path: relative("haystack"),
                }),
                path: None,
            }),
            CancellationSignal::new(),
        )
        .expect("scoped grep dispatches");
    let ToolResult::Grep(grep) = result else {
        unreachable!("grep returns a grep result")
    };
    assert_eq!(grep.matches.len(), 10_000);
    assert!(grep.truncated, "the dropped matches must set truncation");
}

#[cfg(unix)]
#[test]
fn scoped_grep_rejects_a_special_file_target() {
    // A scoped grep whose target is neither a regular file nor a directory
    // (here a Unix socket) fails closed with a typed search failure instead of
    // reading an unbounded stream.
    use std::os::unix::net::UnixListener;

    let root_dir = fixture_dir("scoped-grep-socket");
    let socket_path = root_dir.path().join("listener.sock");
    let _listener = UnixListener::bind(&socket_path).expect("bind socket fixture");
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").expect("pattern"),
            scope: Some(GrepScope::Directory {
                path: relative("listener.sock"),
            }),
            path: None,
        }),
        CancellationSignal::new(),
    );
    assert!(matches!(result, Err(error) if error.code() == "tool_search_failed"));
}

#[test]
fn spawn_observation_wait_rejects_overflowing_duration() {
    // The spawn-observation wait guard treats a duration whose deadline
    // calculation overflows as immediately expired rather than panicking
    // inside `Instant::checked_add`.
    let signal = CancellationSignal::new();
    assert!(
        !signal.wait_until_spawn_observed(std::time::Duration::MAX),
        "an overflowing wait duration must be treated as expired"
    );
}
