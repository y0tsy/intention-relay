#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "Integration tests use expect and unwrap only for deterministic fixture setup; failures indicate a broken test fixture."
)]

//! Regression coverage for PR24-022 and PR24-023: tool text validates on
//! Deserialize at the JSON boundary, and file-processing tools stay
//! memory-bounded.

use intention_proto::WorkspaceRootDto;
use intention_proto::{ToolCallId, WorkspaceRelativePathDto};
use intention_tools::{
    BoundedText, CancellationSignal, EditInput, GlobInput, GrepInput, GrepMatch, GrepScope,
    ToolDispatchOutcome, ToolInput, ToolResult, ToolService, WriteInput,
};
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

/// Returns the serialized cost the shared search window charges for one
/// retained match: its JSON bytes plus the one-byte list separator.
fn serialized_match_bytes(matched: &GrepMatch) -> usize {
    serde_json::to_string(matched)
        .expect("a retained match serializes")
        .len()
        + 1
}

/// Returns the serialized cost the shared search window charges for one
/// retained path: its JSON string bytes plus the one-byte list separator.
fn serialized_path_bytes(path: &WorkspaceRelativePathDto) -> usize {
    path.as_str().len() + 3
}

/// Test adapter: unwraps one completed dispatch and fails loudly on any
/// interruption, so fixtures that expect a final typed result stay direct.
trait DispatchCompleted {
    fn dispatch_completed(
        &self,
        call: ToolCallId,
        input: ToolInput,
        cancellation: CancellationSignal,
    ) -> ToolResult;
}

impl DispatchCompleted for ToolService {
    fn dispatch_completed(
        &self,
        call: ToolCallId,
        input: ToolInput,
        cancellation: CancellationSignal,
    ) -> ToolResult {
        match self
            .dispatch_with_cancellation(call, input, cancellation)
            .expect("completed dispatch succeeds")
        {
            ToolDispatchOutcome::Completed(result) => result,
            ToolDispatchOutcome::Interrupted { cause, partial } => {
                unreachable!("unexpected interrupted dispatch: {cause:?} {partial:?}")
            }
        }
    }
}

#[test]
fn bounded_text_deserialize_rejects_nul_text() {
    let error = serde_json::from_value::<BoundedText>(json!("nul\0inside")).expect_err("NUL fails");
    assert!(error.to_string().contains("invalid_tool_text"));
    assert_eq!(
        serde_json::from_value::<BoundedText>(json!("valid")).expect("valid text decodes"),
        BoundedText::new("valid").expect("fixture")
    );
}

#[test]
fn tool_input_execute_deserialize_decodes_any_argument_count() {
    // No argument-count or aggregate-byte contract cap exists: a shape beyond
    // the removed 128-argument bound decodes like any other invocation.
    let input = json!({
        "tool": "execute",
        "input": {
            "program": "echo",
            "args": (0..129).map(|index| format!("arg-{index}")).collect::<Vec<_>>(),
        },
    });
    let decoded = serde_json::from_value::<ToolInput>(input).expect("execute input decodes");
    let ToolInput::Execute(execute) = decoded else {
        panic!("execute input expected");
    };
    assert_eq!(execute.args.len(), 129);
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
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle-in-the-tail").expect("pattern"),
            scope: Some(GrepScope::Directory {
                path: relative("haystack"),
            }),
            path: None,
        }),
        CancellationSignal::new(),
    );
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
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("prefix-").expect("pattern"),
            scope: Some(GrepScope::Directory {
                path: relative("haystack"),
            }),
            path: None,
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Grep(grep) = result else {
        unreachable!("grep returns a grep result")
    };
    assert!(grep.truncated);
    let retained: usize = grep.matches.iter().map(serialized_match_bytes).sum();
    assert!(
        retained <= 128 * 1024,
        "the serialized match aggregate must stay within the search window ({retained})"
    );
    assert!(
        grep.matches.len() < 200,
        "the aggregate bound must clamp the retained match set"
    );
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
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").expect("pattern"),
            scope: None,
            path: Some(relative("needles.txt")),
        }),
        CancellationSignal::new(),
    );
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
        matches!(missing, Err(error) if error.code() == "tool_search_failed"),
        "a missing pattern-only target fails with a typed search failure"
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
fn single_file_grep_truncates_at_the_serialized_search_window() {
    // C-04: no count cap drops matches, but every retained match is charged
    // its serialized bytes against the shared search window, so a match set
    // far beyond the window truncates honestly instead of growing until the
    // durable fact bound rejects the run.
    let root_dir = fixture_dir("grep-window");
    let mut haystack = String::new();
    for _ in 0..(10_000 + 1) {
        haystack.push_str("a\n");
    }
    std::fs::write(root_dir.path().join("many.txt"), haystack).expect("seed many matches");
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("a").expect("pattern"),
            scope: None,
            path: Some(relative("many.txt")),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Grep(grep) = result else {
        unreachable!("grep returns a grep result")
    };
    assert!(
        grep.truncated,
        "a match set beyond the serialized window must report truncation"
    );
    assert!(
        grep.matches.len() < 10_001,
        "the serialized window must clamp the retained match set"
    );
    assert!(
        grep.matches.len() > 1_000,
        "the window, not a small count cap, bounds the retained match set ({})",
        grep.matches.len()
    );
    let retained: usize = grep.matches.iter().map(serialized_match_bytes).sum();
    assert!(
        retained <= 128 * 1024,
        "the serialized match aggregate must stay within the search window ({retained})"
    );
}

#[test]
fn scoped_directory_grep_truncates_at_the_serialized_search_window() {
    // The scoped grep path reports every match of a directory entry until the
    // serialized search window is full; only that window clamps the result set.
    let root_dir = fixture_dir("scoped-grep-window");
    let haystack = root_dir.path().join("haystack");
    std::fs::create_dir(&haystack).expect("haystack directory");
    let mut content = String::new();
    for _ in 0..(10_000 + 1) {
        content.push_str("a\n");
    }
    std::fs::write(haystack.join("many.txt"), content).expect("seed many matches");
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("a").expect("pattern"),
            scope: Some(GrepScope::Directory {
                path: relative("haystack"),
            }),
            path: None,
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Grep(grep) = result else {
        unreachable!("grep returns a grep result")
    };
    assert!(
        grep.truncated,
        "a match set beyond the serialized window must report truncation"
    );
    assert!(
        grep.matches.len() < 10_001,
        "the serialized window must clamp the retained match set"
    );
    assert!(
        grep.matches.len() > 1_000,
        "the window, not a small count cap, bounds the retained match set ({})",
        grep.matches.len()
    );
    let retained: usize = grep.matches.iter().map(serialized_match_bytes).sum();
    assert!(
        retained <= 128 * 1024,
        "the serialized match aggregate must stay within the search window ({retained})"
    );
}

#[test]
fn glob_truncates_at_the_serialized_search_window() {
    // C-04: a path list larger than the shared search window is cut at the
    // first entry that no longer fits, and the result says so.
    let root_dir = fixture_dir("glob-window");
    let many = root_dir.path().join("many");
    std::fs::create_dir(&many).expect("many directory");
    for index in 0..1_214 {
        std::fs::write(many.join(format!("{:0>100}", index)), "x").expect("seed path");
    }
    let service = ToolService::new(workspace(&root_dir));
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Glob(GlobInput {
            pattern: BoundedText::new("many/*").expect("pattern"),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Glob(glob) = result else {
        unreachable!("glob returns a paths result")
    };
    assert!(
        glob.truncated,
        "a path list beyond the serialized window must report truncation"
    );
    assert!(
        glob.paths.len() < 1_214,
        "the serialized window must clamp the retained path list"
    );
    assert!(
        glob.paths.len() > 1_000,
        "the window, not a small count cap, bounds the retained path list ({})",
        glob.paths.len()
    );
    let retained: usize = glob.paths.iter().map(serialized_path_bytes).sum();
    assert!(
        retained <= 128 * 1024,
        "the serialized path aggregate must stay within the search window ({retained})"
    );
    let next_entry = glob.paths[0].as_str().len() + 3;
    assert!(
        retained + next_entry > 128 * 1024,
        "the window must stop at the first entry that no longer fits"
    );
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
