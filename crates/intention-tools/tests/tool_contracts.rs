#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Integration tests use expect and unwrap only for deterministic fixture setup; failures indicate a broken test fixture."
)]

mod common;

use common::{DispatchCompleted, fixture_dir, service};
use intention_proto::{ToolCallId, WorkspaceRelativePathDto};
use intention_tools::{
    BoundedText, CancellationSignal, EditInput, ExecuteInput, GlobInput, GrepInput, GrepMatch,
    GrepResult, GrepScope, InterruptCause, PathsResult, REDACTED_WORKSPACE_CWD, ReadInput,
    TOOL_DESCRIPTOR_REVISION, TOOL_SCHEMA_VERSION, TextResult, ToolDispatchOutcome, ToolId,
    ToolInput, ToolProcessStatus, ToolProjectedContent, ToolResult, ToolResultProjection,
    WriteInput, WriteResult, model_visible_descriptors, registry,
};

#[test]
fn execute_uses_workspace_cwd_and_returns_typed_result() {
    let root_dir = fixture_dir("execute");
    let root = root_dir.path().to_owned();
    let service = service(&root_dir);
    let program = if cfg!(windows) { "cmd" } else { "pwd" };
    let args = if cfg!(windows) {
        vec!["/C", "cd"]
    } else {
        vec![]
    };
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new(program).expect("program"),
            args: args
                .into_iter()
                .map(|value| BoundedText::new(value).expect("argument"))
                .collect(),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned a non-execute result")
    };
    let expected_root = std::fs::canonicalize(&root)
        .expect("fixture root canonicalizes")
        .to_string_lossy()
        .into_owned();
    if cfg!(windows) {
        assert!(result.text.as_str().contains("stdout:"));
    } else {
        assert!(result.text.as_str().contains(&expected_root));
    }
}

#[test]
fn write_expected_content_accepts_match_and_rejects_mismatch() {
    let root_dir = fixture_dir("write-expected-content");
    let path = root_dir.path().join("file.txt");
    std::fs::write(&path, "before").expect("seed");
    let service = service(&root_dir);
    let relative = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Write(WriteInput {
            path: relative.clone(),
            content: BoundedText::new("after").expect("content"),
            expected_content: Some(BoundedText::new("before").expect("expected")),
        }),
        CancellationSignal::new(),
    );
    assert!(result.is_ok());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "after");

    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: relative,
                content: BoundedText::new("final").expect("content"),
                expected_content: Some(BoundedText::new("stale").expect("expected")),
            }),
            CancellationSignal::new(),
        )
        .expect_err("mismatched expected content");
    assert_eq!(error.code(), "tool_write_conflict");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "after");
}

#[test]
fn edit_expected_content_accepts_match_and_rejects_mismatch() {
    let root_dir = fixture_dir("edit-expected-content");
    let path = root_dir.path().join("file.txt");
    std::fs::write(&path, "before needle").expect("seed");
    let service = service(&root_dir);
    let relative = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    let result = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Edit(EditInput {
            path: relative.clone(),
            old: BoundedText::new("needle").expect("old"),
            new: BoundedText::new("changed").expect("new"),
            expected_content: Some(BoundedText::new("before needle").expect("expected")),
        }),
        CancellationSignal::new(),
    );
    assert!(result.is_ok());
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "before changed"
    );

    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: relative,
                old: BoundedText::new("changed").expect("old"),
                new: BoundedText::new("final").expect("new"),
                expected_content: Some(BoundedText::new("stale").expect("expected")),
            }),
            CancellationSignal::new(),
        )
        .expect_err("mismatched expected content");
    assert_eq!(error.code(), "tool_edit_conflict");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "before changed"
    );
}

#[test]
fn tool_service_covers_nonzero_execute_as_normalized_result() {
    let root_dir = fixture_dir("execute-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "content").expect("seed");
    let service = service(&root_dir);
    let nonzero_input = || {
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new(if cfg!(windows) { "cmd" } else { "sh" }).expect("program"),
            args: if cfg!(windows) {
                vec![
                    BoundedText::new("/C").expect("arg"),
                    BoundedText::new("exit 2").expect("arg"),
                ]
            } else {
                vec![
                    BoundedText::new("-c").expect("arg"),
                    BoundedText::new("exit 2").expect("arg"),
                ]
            },
        })
    };
    // A known non-zero exit is a normalized program result on the typed
    // output path, not a transport-level error.
    let result = service.dispatch_completed(
        ToolCallId::new(),
        nonzero_input(),
        CancellationSignal::new(),
    );
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned a non-execute result")
    };
    assert!(result.text.as_str().contains("exit_code:2"));

    // The real path renders the typed classification into the durable
    // projection: the non-zero exit stays a normalized result whose projected
    // text carries the stable status.
    let call_id = ToolCallId::new();
    let projection = service
        .dispatch_completed(call_id, nonzero_input(), CancellationSignal::new())
        .projection();
    assert_eq!(projection.tool, ToolId::Execute);
    assert!(matches!(
        projection.content,
        ToolProjectedContent::Text {
            text,
            truncated: false
        } if text.as_str().contains("exit_code:2")
    ));

    let encoded = serde_json::to_string(&ToolProcessStatus::NonZero { code: 2 }).unwrap();
    assert_eq!(encoded, r#"{"kind":"non_zero","code":2}"#);
    assert_eq!(
        serde_json::from_str::<ToolProcessStatus>(&encoded).unwrap(),
        ToolProcessStatus::NonZero { code: 2 }
    );
}

#[test]
fn execute_cancellation_is_classified_as_a_stopped_interruption() {
    let root_dir = fixture_dir("timeout-");
    let service = service(&root_dir);
    let cancellation = CancellationSignal::new();
    let canceller = cancellation.clone();
    let cancellation_helper = std::thread::spawn(move || {
        // Wait for a confirmed child spawn instead of racing a fixed sleep:
        // the cancellation then provably lands while the child is running,
        // so the interruption cause is an observed stop. The short fixture
        // stays alive long enough on both Unix and Windows, and its trap
        // ignores termination signals.
        assert!(
            canceller.wait_until_spawn_observed(std::time::Duration::from_secs(10)),
            "execute child was never observed after spawn"
        );
        canceller.cancel();
    });
    let outcome = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(if cfg!(windows) { "ping" } else { "sh" })
                    .expect("program"),
                args: if cfg!(windows) {
                    vec![
                        BoundedText::new("-n").unwrap(),
                        BoundedText::new("2").unwrap(),
                        BoundedText::new("127.0.0.1").unwrap(),
                    ]
                } else {
                    vec![
                        BoundedText::new("-c").unwrap(),
                        BoundedText::new("trap '' TERM; sleep 2").unwrap(),
                    ]
                },
            }),
            cancellation,
        )
        .expect("an interrupted dispatch is an outcome, not an error");
    cancellation_helper
        .join()
        .expect("cancellation helper completes");
    let ToolDispatchOutcome::Interrupted { cause, partial } = outcome else {
        unreachable!("cancellation must interrupt the dispatch");
    };
    assert_eq!(cause, InterruptCause::Stopped);
    let Some(ToolResult::Execute(value)) = partial else {
        unreachable!("the stopped execute captures the output of its collected pipes")
    };
    // Captured output keeps the completed rendering without the exit status
    // line that only a finished program has.
    assert!(value.text.as_str().starts_with("stdout:\n"));
    assert!(!value.text.as_str().contains("exit_code:"));
}

#[test]
fn tool_service_rejects_invalid_patterns_and_unreadable_files() {
    let root_dir = fixture_dir("invalid-");
    let service = service(&root_dir);
    assert!(
        service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Glob(GlobInput {
                    pattern: BoundedText::new("[").expect("pattern")
                }),
                CancellationSignal::new()
            )
            .is_err()
    );
    assert!(
        service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Read(ReadInput {
                    path: WorkspaceRelativePathDto::parse("missing").expect("path")
                }),
                CancellationSignal::new()
            )
            .is_err()
    );
}

#[test]
fn grep_reports_no_matches_for_a_valid_file_scope() {
    let root_dir = fixture_dir("search-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "content").expect("seed");
    let service = service(&root_dir);
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("x").expect("pattern"),
            path: None,
            scope: Some(GrepScope::File {
                path: WorkspaceRelativePathDto::parse("file.txt").unwrap(),
            }),
        }),
        CancellationSignal::new(),
    );
    assert!(matches!(result, ToolResult::Grep(value) if value.matches.is_empty()));
}

#[test]
fn search_rejects_unsafe_patterns_and_reports_utf8_columns() {
    let dir = fixture_dir("search-validation");
    std::fs::write(dir.path().join("file.txt"), "é needle\n").unwrap();
    let service = service(&dir);
    for pattern in [
        "",
        "../*",
        "..\\*",
        "/tmp/*",
        // Windows drive-letter, UNC, and rooted forms must fail closed on
        // every host so validation never depends on the running platform.
        "C:/tmp/*",
        "C:\\tmp\\*",
        "\\\\server\\share\\*",
        "\\Users\\*",
    ] {
        let pattern = BoundedText::new(pattern).unwrap();
        let result = service.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput { pattern }),
            CancellationSignal::new(),
        );
        assert_eq!(result.unwrap_err().code(), "invalid_tool_pattern");
    }
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").unwrap(),
            path: Some(WorkspaceRelativePathDto::parse("file.txt").unwrap()),
            scope: Some(GrepScope::File {
                path: WorkspaceRelativePathDto::parse("file.txt").unwrap(),
            }),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Grep(result) = result else {
        unreachable!()
    };
    assert_eq!(result.matches[0].column, 3);
}

#[test]
fn glob_matches_are_sorted_deduplicated_and_deterministic() {
    let dir = fixture_dir("glob-determinism");
    std::fs::create_dir(dir.path().join("real")).unwrap();
    std::fs::write(dir.path().join("target.txt"), "x").unwrap();
    std::fs::write(dir.path().join("real/deep.txt"), "x").unwrap();
    let service = service(&dir);
    for pattern in ["*.txt", "**/*.txt", "real/*.txt", "{target,deep}*"] {
        let result = service.dispatch_completed(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new(pattern).unwrap(),
            }),
            CancellationSignal::new(),
        );
        let ToolResult::Glob(result) = result else {
            unreachable!("non-glob result")
        };
        // The reported set is sorted and duplicate-free on every replay.
        let mut sorted = result.paths.clone();
        sorted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        sorted.dedup_by(|a, b| a.as_str() == b.as_str());
        assert_eq!(
            result.paths, sorted,
            "nondeterministic subset for: {pattern}"
        );
    }
    // `**/` recursion reaches nested directories, not only the workspace root.
    let recursive = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Glob(GlobInput {
            pattern: BoundedText::new("**/*.txt").unwrap(),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Glob(recursive) = recursive else {
        unreachable!("non-glob result")
    };
    assert!(
        recursive
            .paths
            .iter()
            .any(|path| path.as_str() == "real/deep.txt"),
        "**/*.txt must reach nested directories"
    );
}

#[test]
fn bounded_sources_report_truncation_only_past_the_output_bound() {
    let dir = fixture_dir("bounded-source");
    std::fs::write(dir.path().join("exact.bin"), vec![b'a'; 65_536]).unwrap();
    std::fs::write(dir.path().join("over.bin"), vec![b'b'; 65_537]).unwrap();
    let service = service(&dir);
    for (name, truncated, length) in [("exact.bin", false, 65_536), ("over.bin", true, 65_536)] {
        let result = service.dispatch_completed(
            ToolCallId::new(),
            ToolInput::Read(ReadInput {
                path: WorkspaceRelativePathDto::parse(name).unwrap(),
            }),
            CancellationSignal::new(),
        );
        let ToolResult::Read(result) = result else {
            unreachable!("non-read result")
        };
        assert_eq!(result.text.as_str().len(), length, "bound cut: {name}");
        assert_eq!(result.truncated, truncated, "truncation flag: {name}");
    }
}

#[test]
fn grep_file_scope_rejects_directories_and_follows_file_links() {
    let dir = fixture_dir("search-links");
    std::fs::write(dir.path().join("target.txt"), "needle").unwrap();
    std::fs::create_dir(dir.path().join("folder")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt")).unwrap();
    let service = service(&dir);
    // A directory is not a valid explicit file scope.
    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("folder").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("folder").unwrap(),
                }),
            }),
            CancellationSignal::new(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "tool_search_failed");
    // An explicitly addressed file link is followed like any other path.
    #[cfg(unix)]
    {
        let result = service.dispatch_completed(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("link.txt").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("link.txt").unwrap(),
                }),
            }),
            CancellationSignal::new(),
        );
        let ToolResult::Grep(result) = result else {
            unreachable!("non-grep result")
        };
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].path.as_str(), "link.txt");
        assert!(!result.truncated);
    }
}

#[test]
fn dispatch_reports_precise_errors_and_process_output_paths() {
    let root_dir = fixture_dir("dispatch-errors");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "needle\nother").expect("seed");
    let service = service(&root_dir);
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");

    let cancelled = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Read(ReadInput { path: path.clone() }),
            CancellationSignal::cancelled(),
        )
        .expect("pre-start cancellation is an interrupted outcome");
    assert_eq!(
        cancelled,
        ToolDispatchOutcome::Interrupted {
            cause: InterruptCause::Stopped,
            partial: None,
        }
    );

    let missing_parent = WorkspaceRelativePathDto::parse("missing/new.txt").expect("path");
    let write_error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: missing_parent,
                content: BoundedText::new("x").expect("content"),
                expected_content: None,
            }),
            CancellationSignal::new(),
        )
        .expect_err("write failure");
    assert_eq!(write_error.code(), "tool_write_failed");

    let edit_missing = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: path.clone(),
                old: BoundedText::new("absent").expect("old"),
                new: BoundedText::new("new").expect("new"),
                expected_content: None,
            }),
            CancellationSignal::new(),
        )
        .expect_err("missing edit target");
    assert_eq!(edit_missing.code(), "edit_target_missing");

    let grep = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").expect("pattern"),
            path: Some(path.clone()),
            scope: Some(GrepScope::File { path }),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Grep(result) = grep else {
        unreachable!("dispatch returned non-grep result")
    };
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].fragment.as_str(), "needle");
    assert!(!result.truncated);
}

#[test]
fn a_stopped_tool_never_starts_its_effect_and_keeps_partial_results() {
    let root_dir = fixture_dir("stop-effects");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "original").expect("seed");
    let service = service(&root_dir);
    let stopped = || ToolDispatchOutcome::Interrupted {
        cause: InterruptCause::Stopped,
        partial: None,
    };

    // Write and edit report the stop instead of touching the file.
    let write = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: WorkspaceRelativePathDto::parse("file.txt").expect("path"),
                content: BoundedText::new("replacement").expect("content"),
                expected_content: None,
            }),
            CancellationSignal::cancelled(),
        )
        .expect("a stopped write is an outcome, not an error");
    assert_eq!(write, stopped());
    let edit = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: WorkspaceRelativePathDto::parse("file.txt").expect("path"),
                old: BoundedText::new("original").expect("old"),
                new: BoundedText::new("replacement").expect("new"),
                expected_content: None,
            }),
            CancellationSignal::cancelled(),
        )
        .expect("a stopped edit is an outcome, not an error");
    assert_eq!(edit, stopped());
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).expect("file survives untouched"),
        "original"
    );

    // The search tools report the stop without collecting results.
    let glob = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("**/*.txt").expect("pattern"),
            }),
            CancellationSignal::cancelled(),
        )
        .expect("a stopped glob is an outcome, not an error");
    assert_eq!(glob, stopped());
    let grep = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("original").expect("pattern"),
                path: None,
                scope: None,
            }),
            CancellationSignal::cancelled(),
        )
        .expect("a stopped grep is an outcome, not an error");
    assert_eq!(grep, stopped());
}

#[test]
fn execute_returns_stdout_stderr_and_truncation_metadata() {
    let root_dir = fixture_dir("execute-output");
    let service = service(&root_dir);
    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/C", "echo out & echo err 1>&2"])
    } else {
        ("sh", vec!["-c", "printf out; printf err >&2"])
    };
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new(program).expect("program"),
            args: args
                .into_iter()
                .map(|arg| BoundedText::new(arg).expect("arg"))
                .collect(),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned non-execute result")
    };
    assert!(result.text.as_str().contains("stdout:\nout"));
    assert!(result.text.as_str().contains("stderr:\nerr"));
    assert!(result.text.as_str().contains("exit_code:0"));
    assert!(!result.truncated);
}

#[test]
fn public_tool_errors_redact_secret_paths_commands_and_os_text() {
    let root_dir = fixture_dir("redaction");
    let root = root_dir.path();
    let service = service(&root_dir);
    // Assembled at runtime: recognizably fake, yet never a literal
    // secret-shaped assignment that docs-check rejects.
    let secret = format!("credential{}", "-leak-probe");
    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: WorkspaceRelativePathDto::parse("missing/new.txt").expect("path"),
                content: BoundedText::new(secret.as_str()).expect("content"),
                expected_content: None,
            }),
            CancellationSignal::new(),
        )
        .expect_err("write must fail");
    let rendered = format!("{error:?}");
    assert!(!rendered.contains(secret.as_str()));
    assert!(!rendered.contains(&root.to_string_lossy().to_string()));
    assert!(!rendered.contains("No such file or directory"));
}

#[test]
fn tool_service_covers_read_write_and_edit_error_variants() {
    let root_dir = fixture_dir("errors-2");
    let root = root_dir.path();
    std::fs::create_dir(root.join("directory")).expect("directory");
    let service = service(&root_dir);
    let directory = WorkspaceRelativePathDto::parse("directory").expect("path");
    assert!(
        service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Read(ReadInput {
                    path: directory.clone()
                }),
                CancellationSignal::new()
            )
            .is_err()
    );
    assert!(
        service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Write(WriteInput {
                    path: directory.clone(),
                    content: BoundedText::new("x").expect("content"),
                    expected_content: None
                }),
                CancellationSignal::new()
            )
            .is_err()
    );
    assert!(
        service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Edit(EditInput {
                    path: directory,
                    old: BoundedText::new("x").expect("old"),
                    new: BoundedText::new("y").expect("new"),
                    expected_content: None
                }),
                CancellationSignal::new()
            )
            .is_err()
    );
}

#[test]
fn tool_service_returns_search_matches_and_sorted_glob_paths() {
    let root_dir = fixture_dir("search-2");
    let root = root_dir.path();
    std::fs::write(root.join("z.txt"), "first\nneedle\nneedle two").expect("seed");
    std::fs::write(root.join("a.txt"), "needle").expect("seed");
    let service = service(&root_dir);
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").expect("pattern"),
            path: Some(WorkspaceRelativePathDto::parse("z.txt").expect("path")),
            scope: Some(GrepScope::File {
                path: WorkspaceRelativePathDto::parse("z.txt").expect("path"),
            }),
        }),
        CancellationSignal::new(),
    );
    assert!(
        matches!(result, ToolResult::Grep(value) if value.matches.iter().map(|m| m.fragment.as_str()).collect::<Vec<_>>() == vec!["needle", "needle two"])
    );
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Glob(GlobInput {
            pattern: BoundedText::new("*.txt").expect("pattern"),
        }),
        CancellationSignal::new(),
    );
    assert!(
        matches!(result, ToolResult::Glob(value) if value.paths.iter().map(WorkspaceRelativePathDto::as_str).collect::<Vec<_>>() == vec!["a.txt", "z.txt"])
    );
}

#[test]
fn glob_empty_and_grep_read_failure_are_typed() {
    let root_dir = fixture_dir("search-extra-");
    let root = root_dir.path();
    std::fs::create_dir_all(root).unwrap();
    let service = service(&root_dir);
    let glob = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Glob(GlobInput {
            pattern: BoundedText::new("*.none").unwrap(),
        }),
        CancellationSignal::new(),
    );
    assert!(matches!(glob, ToolResult::Glob(value) if value.paths.is_empty()));
    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("x").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("missing").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("missing").unwrap(),
                }),
            }),
            CancellationSignal::new(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "tool_search_failed");
}

#[test]
fn dto_metadata_and_policy_round_trip_all_variants() {
    use intention_tools::{MutationKind, ToolCapability, ToolPolicy};
    for value in [
        MutationKind::ReadOnly,
        MutationKind::Mutating,
        MutationKind::Process,
    ] {
        let json = serde_json::to_string(&value).expect("mutation json");
        assert_eq!(
            serde_json::from_str::<MutationKind>(&json).expect("mutation"),
            value
        );
    }
    for value in [
        ToolCapability::Read,
        ToolCapability::Search,
        ToolCapability::Write,
        ToolCapability::Edit,
        ToolCapability::Execute,
    ] {
        let json = serde_json::to_string(&value).expect("capability json");
        assert_eq!(
            serde_json::from_str::<ToolCapability>(&json).expect("capability"),
            value
        );
    }
    for value in [ToolPolicy::Allowed, ToolPolicy::Denied] {
        let json = serde_json::to_string(&value).expect("policy json");
        assert_eq!(
            serde_json::from_str::<ToolPolicy>(&json).expect("policy"),
            value
        );
    }
}

#[test]
fn cancelled_dispatch_is_interrupted_before_any_tool_effect() {
    let root_dir = fixture_dir("cancelled-before-dispatch");
    let outcome = service(&root_dir)
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: WorkspaceRelativePathDto::parse("created.txt").unwrap(),
                content: BoundedText::new("must not write").unwrap(),
                expected_content: None,
            }),
            CancellationSignal::cancelled(),
        )
        .expect("pre-start cancellation is an interrupted outcome, not an error");
    assert_eq!(
        outcome,
        ToolDispatchOutcome::Interrupted {
            cause: InterruptCause::Stopped,
            partial: None,
        }
    );
    assert!(!root_dir.path().join("created.txt").exists());
}

#[test]
fn cancellation_signal_transitions_and_tool_id_formats_are_stable() {
    let signal = CancellationSignal::new();
    assert!(!signal.is_cancelled());
    signal.cancel();
    assert!(signal.is_cancelled());
    assert!(CancellationSignal::cancelled().is_cancelled());
    let ids = [
        (ToolId::Read, "read"),
        (ToolId::Glob, "glob"),
        (ToolId::Grep, "grep"),
        (ToolId::Write, "write"),
        (ToolId::Edit, "edit"),
        (ToolId::Execute, "execute"),
    ];
    for (id, name) in ids {
        assert_eq!(id.as_str(), name);
        assert_eq!(id.to_string(), name);
    }
}

#[test]
fn tool_service_read_and_grep_report_truncation_for_invalid_utf8() {
    let dir = fixture_dir("invalid-utf8");
    let bytes = vec![0xff; 70_000];
    std::fs::write(dir.path().join("bytes.bin"), bytes).unwrap();
    let service = service(&dir);
    let path = WorkspaceRelativePathDto::parse("bytes.bin").unwrap();
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Read(ReadInput { path: path.clone() }),
        CancellationSignal::new(),
    );
    assert!(matches!(
        result,
        ToolResult::Read(TextResult {
            truncated: true,
            ..
        })
    ));
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("x").unwrap(),
            path: Some(path.clone()),
            scope: Some(GrepScope::File { path }),
        }),
        CancellationSignal::new(),
    );
    assert!(matches!(
        result,
        ToolResult::Grep(intention_tools::GrepResult {
            truncated: true,
            ..
        })
    ));
}

#[test]
fn execute_success_reports_stderr_and_typed_success_status() {
    let dir = fixture_dir("execute-stderr");
    let service = service(&dir);
    let input = || {
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new(if cfg!(windows) { "cmd" } else { "sh" }).unwrap(),
            args: if cfg!(windows) {
                vec![
                    BoundedText::new("/C").unwrap(),
                    BoundedText::new("echo err 1>&2").unwrap(),
                ]
            } else {
                vec![
                    BoundedText::new("-c").unwrap(),
                    BoundedText::new("printf err >&2").unwrap(),
                ]
            },
        })
    };
    let result = service.dispatch_completed(ToolCallId::new(), input(), CancellationSignal::new());
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned a non-execute result")
    };
    assert!(result.text.as_str().contains("stderr:\nerr"));
    // The typed `success` classification renders into the result text on the
    // real path, so the text and the classification can never disagree.
    assert!(result.text.as_str().contains("exit_code:0"));
}

#[test]
fn execute_inherits_the_invoking_environment() {
    let dir = fixture_dir("execute-env");
    let service = service(&dir);
    // Edition 2024 makes `std::env::set_var` unsafe and the workspace denies
    // `unsafe_code`, so the probe reads a variable that is present in the
    // invoking process on every supported platform. A child that does not
    // inherit the caller environment exits non-zero, and the rendered result
    // records that exit code.
    let (program, args): (&str, Vec<String>) = if cfg!(windows) {
        // `cmd` rejects the parenthesized `if defined … (…) else (…)` form on
        // the Windows runners (`" was unexpected at this time.`), so the probe
        // uses the single-line negated guard: it exits non-zero only when
        // `PATH` is absent from the inherited environment.
        ("cmd", vec!["/C if not defined PATH exit 1".to_owned()])
    } else {
        ("sh", vec!["-c".to_owned(), "test -n \"$PATH\"".to_owned()])
    };
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new(program).unwrap(),
            args: args
                .into_iter()
                .map(|arg| BoundedText::new(arg).unwrap())
                .collect(),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned a non-execute result")
    };
    assert!(
        result.text.as_str().contains("exit_code:0"),
        "environment inheritance check failed: {}",
        result.text.as_str()
    );
}

#[test]
fn dispatch_covers_empty_read_and_successful_empty_edit() {
    let root_dir = fixture_dir("empty-read-edit");
    std::fs::write(root_dir.path().join("empty.txt"), "").expect("seed");
    let service = service(&root_dir);
    let path = WorkspaceRelativePathDto::parse("empty.txt").expect("path");
    let read = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Read(ReadInput { path: path.clone() }),
        CancellationSignal::new(),
    );
    assert!(
        matches!(read, ToolResult::Read(TextResult { truncated: false, text }) if text.as_str().is_empty())
    );
    let edit = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Edit(EditInput {
            path,
            old: BoundedText::new("").expect("old"),
            new: BoundedText::new("replacement").expect("new"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    assert!(matches!(edit, ToolResult::Edit(_)));
}

#[test]
fn execute_reports_signal_termination_as_known_terminal_result() {
    if cfg!(windows) {
        return;
    }
    let root_dir = fixture_dir("signal-exit");
    let service = service(&root_dir);
    let signal_input = || {
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("sh").unwrap(),
            args: vec![
                BoundedText::new("-c").unwrap(),
                BoundedText::new("kill -TERM $$").unwrap(),
            ],
        })
    };
    let result =
        service.dispatch_completed(ToolCallId::new(), signal_input(), CancellationSignal::new());
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned a non-execute result")
    };
    // Signal termination renders from the typed status; there is no invented
    // numeric exit code for a signal.
    assert!(result.text.as_str().contains("signal:15"));
}

#[test]
fn grep_truncates_long_multibyte_fragments_on_character_boundary() {
    let root_dir = fixture_dir("large-multibyte-grep");
    let line = format!("needle{}", "界".repeat(30_000));
    std::fs::write(root_dir.path().join("large.txt"), &line).unwrap();
    let result = service(&root_dir).dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").unwrap(),
            path: Some(WorkspaceRelativePathDto::parse("large.txt").unwrap()),
            scope: Some(GrepScope::File {
                path: WorkspaceRelativePathDto::parse("large.txt").unwrap(),
            }),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Grep(result) = result else {
        unreachable!("dispatch returned non-grep result")
    };
    assert_eq!(result.matches.len(), 1);
    assert!(result.truncated);
    let fragment = result.matches[0].fragment.as_str();
    assert!(fragment.is_char_boundary(fragment.len()));
    assert!(fragment.len() <= 65_536);
    assert!(fragment.starts_with("needle"));
}

#[test]
fn execute_formats_success_and_truncates_both_streams() {
    let root_dir = fixture_dir("execute-output");
    let service = service(&root_dir);
    let result = service
        .dispatch_completed(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(if cfg!(windows) { "cmd" } else { "sh" }).unwrap(),
                args: if cfg!(windows) {
                    vec![
                        // `for /L` and `echo` are built into cmd.exe, so this
                        // fixture does not depend on Python or another tool.
                        // Each iteration writes one 100-byte line, so 2000
                        // iterations deterministically exceed the 64 KiB
                        // output bound without `& exit /b` parsing.
                        BoundedText::new("/C").unwrap(),
                        BoundedText::new(
                            "for /L %i in (1,1,2000) do @echo 0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789",
                        )
                        .unwrap(),
                    ]
                } else {
                    let script = "python3 -c 'import sys; sys.stdout.write(\"x\" * 200000); sys.stderr.write(\"y\" * 200000)'";
                    vec![
                        BoundedText::new("-c").unwrap(),
                        BoundedText::new(script).unwrap(),
                    ]
                },
            }),
            CancellationSignal::new(),
        );
    let ToolResult::Execute(value) = result else {
        unreachable!()
    };
    assert!(value.text.as_str().contains("stdout:"));
    assert!(value.text.as_str().contains("stderr:"));
    assert!(value.text.as_str().contains("exit_code:0"));
    // The fixture writes more than the output bound (to both streams on Unix,
    // to stdout on Windows), so the truncation flag is set and the rendered
    // result carries the marker on every platform.
    assert!(value.truncated);
    assert!(value.text.as_str().contains("[truncated]"));
}

#[test]
fn exact_typed_errors_cover_search_edit_and_spawn_failures() {
    let root_dir = fixture_dir("exact-errors");
    std::fs::write(root_dir.path().join("file.txt"), "content").unwrap();
    let service = service(&root_dir);
    let path = WorkspaceRelativePathDto::parse("file.txt").unwrap();
    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path,
                old: BoundedText::new("missing").unwrap(),
                new: BoundedText::new("x").unwrap(),
                expected_content: None,
            }),
            CancellationSignal::new(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "edit_target_missing");
    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("x").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("missing").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("missing").unwrap(),
                }),
            }),
            CancellationSignal::new(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "tool_search_failed");
    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new("not-a-real-program").unwrap(),
                args: vec![],
            }),
            CancellationSignal::new(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "tool_execute_spawn_failed");
}

#[test]
fn registry_exposes_all_fourteen_slots_in_canonical_order() {
    let expected = [
        (ToolId::Read, "read"),
        (ToolId::Write, "write"),
        (ToolId::Edit, "edit"),
        (ToolId::Execute, "execute"),
        (ToolId::Glob, "glob"),
        (ToolId::Grep, "grep"),
        (ToolId::FetchUrl, "fetch_url"),
        (ToolId::AskUser, "ask_user"),
        (ToolId::Todo, "todo"),
        (ToolId::Retrieve, "retrieve"),
        (ToolId::PlanSubmit, "plan_submit"),
        (ToolId::SubAgent, "sub_agent"),
        (ToolId::Expand, "expand"),
        (ToolId::Mcp, "mcp"),
    ];
    let descriptors = registry();
    assert_eq!(descriptors.len(), expected.len());
    for (descriptor, (id, name)) in descriptors.into_iter().zip(expected) {
        assert_eq!(descriptor.id(), id);
        assert_eq!(descriptor.id().as_str(), name);
    }
    let mut sorted_ids = expected.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    sorted_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    sorted_ids.dedup_by(|a, b| a.as_str() == b.as_str());
    assert_eq!(sorted_ids.len(), expected.len());
}

#[test]
fn all_descriptor_metadata_values_are_verified() {
    use intention_tools::{MutationKind, ToolCapability, ToolPolicy, ToolRegistrationStatus};
    let expected = [
        (
            ToolId::Read,
            MutationKind::ReadOnly,
            &[ToolCapability::Read][..],
        ),
        (
            ToolId::Write,
            MutationKind::Mutating,
            &[ToolCapability::Write][..],
        ),
        (
            ToolId::Edit,
            MutationKind::Mutating,
            &[ToolCapability::Edit][..],
        ),
        (
            ToolId::Execute,
            MutationKind::Process,
            &[ToolCapability::Execute][..],
        ),
        (
            ToolId::Glob,
            MutationKind::ReadOnly,
            &[ToolCapability::Search][..],
        ),
        (
            ToolId::Grep,
            MutationKind::ReadOnly,
            &[ToolCapability::Search][..],
        ),
    ];
    for (descriptor, (id, mutation, capabilities)) in
        registry().into_iter().take(expected.len()).zip(expected)
    {
        assert_eq!(descriptor.id(), id);
        assert_eq!(descriptor.mutation(), mutation);
        assert_eq!(descriptor.capabilities(), capabilities);
        assert_eq!(descriptor.schema_version(), TOOL_SCHEMA_VERSION);
        assert_eq!(descriptor.descriptor_revision(), TOOL_DESCRIPTOR_REVISION);
        assert!(descriptor.input_schema().is_some());
        assert!(descriptor.output_schema().is_some());
        let input_schema_json: serde_json::Value =
            serde_json::from_str(descriptor.input_schema().expect("active input schema"))
                .expect("input schema json");
        assert!(input_schema_json.is_object());
        assert_eq!(input_schema_json["type"], "object");
        let output_schema_json: serde_json::Value =
            serde_json::from_str(descriptor.output_schema().expect("active output schema"))
                .expect("output schema json");
        assert!(output_schema_json.is_object());
        assert_eq!(output_schema_json["type"], "object");
        assert_eq!(descriptor.status(), ToolRegistrationStatus::Active);
        assert_eq!(descriptor.observability_policy(), ToolPolicy::Allowed);
        assert!(!descriptor.display_name().is_empty());
        assert!(!descriptor.description().is_empty());
    }
}

#[test]
fn model_visible_descriptors_are_the_six_active_tools_in_registry_order() {
    use intention_tools::ToolRegistrationStatus;
    let expected = [
        ToolId::Read,
        ToolId::Write,
        ToolId::Edit,
        ToolId::Execute,
        ToolId::Glob,
        ToolId::Grep,
    ];
    let visible = model_visible_descriptors();
    assert_eq!(visible.len(), expected.len());
    for (descriptor, id) in visible.into_iter().zip(expected) {
        assert_eq!(descriptor.id(), id);
        assert_eq!(descriptor.status(), ToolRegistrationStatus::Active);
        assert!(!descriptor.description().is_empty());
        let schema = descriptor.input_schema().expect("model input schema");
        assert!(!schema.is_empty());
    }
}

#[test]
fn wire_names_parse_back_into_their_typed_identifiers() {
    for descriptor in registry() {
        let id = descriptor.id();
        assert_eq!(
            ToolId::from_wire_name(id.as_str()),
            Some(id),
            "every registered wire name parses back to its identifier"
        );
    }
    assert_eq!(ToolId::from_wire_name("read_workspace_file"), None);
    assert_eq!(ToolId::from_wire_name("Read"), None);
    assert_eq!(ToolId::from_wire_name(""), None);
}

#[test]
fn reserved_slots_have_no_schemas_or_revision() {
    use intention_tools::ToolRegistrationStatus;
    let reserved_in_documented_order = [
        ToolId::FetchUrl,
        ToolId::AskUser,
        ToolId::Todo,
        ToolId::Retrieve,
        ToolId::PlanSubmit,
        ToolId::SubAgent,
        ToolId::Expand,
        ToolId::Mcp,
    ];
    for (descriptor, id) in registry()
        .into_iter()
        .skip(6)
        .zip(reserved_in_documented_order)
    {
        assert_eq!(descriptor.id(), id);
        assert_eq!(descriptor.status(), ToolRegistrationStatus::Reserved);
        assert_eq!(descriptor.descriptor_revision(), 0);
        assert_eq!(descriptor.schema_version(), 0);
        assert_eq!(descriptor.input_schema(), None);
        assert_eq!(descriptor.output_schema(), None);
        assert!(descriptor.capabilities().is_empty());
    }
}

/// Asserts one descriptor schema document is a JSON object whose properties
/// match the keys of the serialized typed payload, and that every required
/// property is present and non-null in that payload.
fn assert_schema_agrees_with_payload(
    id: ToolId,
    schema_json: &str,
    expected_properties: &[&str],
    expected_required: &[&str],
    payload: &serde_json::Value,
) {
    let schema: serde_json::Value = serde_json::from_str(schema_json).expect("schema json");
    assert_eq!(schema["type"], "object");
    let properties = schema["properties"].as_object().expect("schema properties");
    let mut schema_properties = properties.keys().map(String::as_str).collect::<Vec<_>>();
    schema_properties.sort_unstable();
    let object = payload.as_object().expect("serialized payload object");
    let mut serialized_keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    serialized_keys.sort_unstable();
    let mut expected = expected_properties.to_vec();
    expected.sort_unstable();
    assert_eq!(schema_properties, expected);
    assert_eq!(serialized_keys, expected);
    let required = schema["required"]
        .as_array()
        .expect("schema required list")
        .iter()
        .map(|name| name.as_str().expect("required name"))
        .collect::<Vec<_>>();
    assert_eq!(required, expected_required.to_vec());
    for name in required {
        assert!(
            object.get(name).is_some_and(|value| !value.is_null()),
            "required property {name} is absent or null in the serialized {} payload",
            id.as_str()
        );
    }
}

/// The argument side asserts each descriptor's input schema (the former model
/// parameter schema) against the serialized typed inputs; the result side
/// mirrors it against the serialized typed result payloads.
#[test]
fn model_parameter_schemas_agree_with_serialized_inputs() {
    let path = WorkspaceRelativePathDto::parse("src/main.rs").expect("path");
    let text = |value: &str| BoundedText::new(value).expect("text");
    let input_fixtures = [
        (
            ToolId::Read,
            &["path"][..],
            &["path"][..],
            serde_json::to_value(ReadInput { path: path.clone() }).expect("read fixture"),
        ),
        (
            ToolId::Write,
            &["path", "content", "expected_content"][..],
            &["path", "content"][..],
            serde_json::to_value(WriteInput {
                path: path.clone(),
                content: text("content"),
                expected_content: Some(text("previous")),
            })
            .expect("write fixture"),
        ),
        (
            ToolId::Edit,
            &["path", "old", "new", "expected_content"][..],
            &["path", "old", "new"][..],
            serde_json::to_value(EditInput {
                path: path.clone(),
                old: text("old"),
                new: text("new"),
                expected_content: Some(text("old")),
            })
            .expect("edit fixture"),
        ),
        (
            ToolId::Execute,
            &["program", "args"][..],
            &["program", "args"][..],
            serde_json::to_value(ExecuteInput {
                program: text("cargo"),
                args: vec![text("test")],
            })
            .expect("execute fixture"),
        ),
        (
            ToolId::Glob,
            &["pattern"][..],
            &["pattern"][..],
            serde_json::to_value(GlobInput {
                pattern: text("**/*.rs"),
            })
            .expect("glob fixture"),
        ),
        (
            ToolId::Grep,
            &["pattern", "scope", "path"][..],
            &["pattern"][..],
            serde_json::to_value(GrepInput {
                pattern: text("needle"),
                scope: Some(GrepScope::Directory { path: path.clone() }),
                path: Some(path.clone()),
            })
            .expect("grep fixture"),
        ),
    ];
    let result_fixtures = [
        (
            ToolId::Read,
            &["text", "truncated"][..],
            &["text", "truncated"][..],
            serde_json::to_value(TextResult {
                text: text("read output"),
                truncated: false,
            })
            .expect("read result fixture"),
        ),
        (
            ToolId::Write,
            &["bytes"][..],
            &["bytes"][..],
            serde_json::to_value(WriteResult { bytes: 5 }).expect("write result fixture"),
        ),
        (
            ToolId::Edit,
            &["bytes"][..],
            &["bytes"][..],
            serde_json::to_value(WriteResult { bytes: 7 }).expect("edit result fixture"),
        ),
        (
            ToolId::Execute,
            &["text", "truncated"][..],
            &["text", "truncated"][..],
            serde_json::to_value(TextResult {
                text: text("execute output"),
                truncated: true,
            })
            .expect("execute result fixture"),
        ),
        (
            ToolId::Glob,
            &["paths", "truncated"][..],
            &["paths", "truncated"][..],
            serde_json::to_value(PathsResult {
                paths: vec![path.clone()],
                truncated: false,
            })
            .expect("glob result fixture"),
        ),
        (
            ToolId::Grep,
            &["matches", "truncated"][..],
            &["matches", "truncated"][..],
            serde_json::to_value(GrepResult {
                matches: vec![GrepMatch {
                    path,
                    line: 1,
                    column: 1,
                    fragment: text("needle"),
                }],
                truncated: false,
            })
            .expect("grep result fixture"),
        ),
    ];
    let visible = model_visible_descriptors();
    assert_eq!(visible.len(), input_fixtures.len());
    assert_eq!(visible.len(), result_fixtures.len());
    for descriptor in visible {
        let input_fixture = input_fixtures
            .iter()
            .find(|entry| entry.0 == descriptor.id())
            .expect("input fixture for every model-visible tool");
        assert_schema_agrees_with_payload(
            descriptor.id(),
            descriptor.input_schema().expect("input schema"),
            input_fixture.1,
            input_fixture.2,
            &input_fixture.3,
        );
        let result_fixture = result_fixtures
            .iter()
            .find(|entry| entry.0 == descriptor.id())
            .expect("result fixture for every model-visible tool");
        assert_schema_agrees_with_payload(
            descriptor.id(),
            descriptor.output_schema().expect("result schema"),
            result_fixture.1,
            result_fixture.2,
            &result_fixture.3,
        );
    }
}

#[test]
fn dispatch_covers_each_tool_input_variant() {
    let dir = fixture_dir("dispatch-variants");
    std::fs::write(dir.path().join("a.txt"), "needle").unwrap();
    let service = service(&dir);
    let path = WorkspaceRelativePathDto::parse("a.txt").unwrap();
    let calls = [
        ToolInput::Read(ReadInput { path: path.clone() }),
        ToolInput::Glob(GlobInput {
            pattern: BoundedText::new("*.txt").unwrap(),
        }),
        ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").unwrap(),
            path: Some(path.clone()),
            scope: Some(GrepScope::File { path: path.clone() }),
        }),
        ToolInput::Write(WriteInput {
            path: WorkspaceRelativePathDto::parse("b.txt").unwrap(),
            content: BoundedText::new("b").unwrap(),
            expected_content: None,
        }),
        ToolInput::Edit(EditInput {
            path,
            old: BoundedText::new("needle").unwrap(),
            new: BoundedText::new("changed").unwrap(),
            expected_content: None,
        }),
    ];
    for input in calls {
        assert!(
            service
                .dispatch_with_cancellation(ToolCallId::new(), input, CancellationSignal::new())
                .is_ok()
        );
    }
}

#[test]
fn glob_dispatch_projects_redacted_execution_metadata() {
    let dir = fixture_dir("projection-metadata");

    let outcome = service(&dir)
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*.txt").unwrap(),
            }),
            CancellationSignal::new(),
        )
        .expect("a glob dispatch succeeds");
    let ToolDispatchOutcome::Completed(result) = outcome else {
        unreachable!("a bare glob completes");
    };
    assert!(matches!(result, ToolResult::Glob(_)));
    let projection = result.projection();
    assert_eq!(projection.tool, ToolId::Glob);
    assert_eq!(
        projection.execution.policy,
        intention_tools::ToolPolicy::Allowed
    );
    // Durable metadata identifies the workspace root only through the stable
    // redacted marker; the absolute location is never recorded.
    assert_eq!(projection.execution.cwd, REDACTED_WORKSPACE_CWD);
    assert_eq!(projection.execution.path, None);
}

#[test]
fn dispatch_projections_are_redacted_and_normalized_for_every_concrete_tool() {
    let root_dir = fixture_dir("projection");
    let root_path = root_dir.path();
    std::fs::write(root_path.join("data.txt"), "alpha\nneedle\n").unwrap();
    let service = service(&root_dir);
    let path = WorkspaceRelativePathDto::parse("data.txt").unwrap();
    let calls = [
        (
            ToolInput::Read(ReadInput { path: path.clone() }),
            ToolId::Read,
        ),
        (
            ToolInput::Write(WriteInput {
                path: path.clone(),
                content: BoundedText::new("beta needle").unwrap(),
                expected_content: None,
            }),
            ToolId::Write,
        ),
        (
            ToolInput::Edit(EditInput {
                path: path.clone(),
                old: BoundedText::new("beta").unwrap(),
                new: BoundedText::new("gamma").unwrap(),
                expected_content: None,
            }),
            ToolId::Edit,
        ),
        (
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*.txt").unwrap(),
            }),
            ToolId::Glob,
        ),
        (
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").unwrap(),
                path: Some(path.clone()),
                scope: Some(GrepScope::File { path }),
            }),
            ToolId::Grep,
        ),
        (
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(if cfg!(windows) { "cmd" } else { "sh" }).unwrap(),
                args: if cfg!(windows) {
                    vec![
                        BoundedText::new("/C").unwrap(),
                        BoundedText::new("echo ok").unwrap(),
                    ]
                } else {
                    vec![
                        BoundedText::new("-c").unwrap(),
                        BoundedText::new("echo ok").unwrap(),
                    ]
                },
            }),
            ToolId::Execute,
        ),
    ];
    let absolute_root = root_path.to_string_lossy().to_string();
    for (input, tool) in calls {
        let result =
            service.dispatch_completed(ToolCallId::new(), input, CancellationSignal::new());
        let projection = result.projection();
        assert_eq!(projection.schema_version, TOOL_SCHEMA_VERSION, "{tool}");
        assert_eq!(projection.tool, tool);
        assert_eq!(projection.execution.cwd, REDACTED_WORKSPACE_CWD, "{tool}");
        // A bare result carries no invocation timing or logical path, so the
        // projection records the zero default and no path.
        assert_eq!(projection.execution.elapsed_ms, 0, "{tool} timing");
        assert_eq!(
            projection.execution.policy,
            intention_tools::ToolPolicy::Allowed,
            "{tool} policy"
        );
        assert_eq!(projection.execution.path, None, "{tool} metadata path");
        assert_eq!(projection.execution.process_status, None, "{tool}");
        // Neither the projection nor the typed result may carry the absolute
        // workspace root.
        let rendered = serde_json::to_string(&projection).unwrap();
        assert!(
            !rendered.contains(&absolute_root),
            "{tool} projection leaked the absolute root"
        );
        let result_rendered = serde_json::to_string(&result).unwrap();
        assert!(
            !result_rendered.contains(&absolute_root),
            "{tool} result leaked the absolute root"
        );
        match (&projection.content, tool) {
            (ToolProjectedContent::Text { text, truncated }, ToolId::Read) => {
                assert!(text.as_str().starts_with("alpha"));
                assert!(!*truncated);
            }
            (ToolProjectedContent::Mutation { bytes }, ToolId::Write) => {
                assert_eq!(*bytes, "beta needle".len() as u64);
            }
            (ToolProjectedContent::Mutation { bytes }, ToolId::Edit) => {
                assert_eq!(*bytes, "gamma needle".len() as u64);
            }
            (ToolProjectedContent::Paths { paths, truncated }, ToolId::Glob) => {
                let listed = paths
                    .iter()
                    .map(WorkspaceRelativePathDto::as_str)
                    .collect::<Vec<_>>();
                assert_eq!(listed, vec!["data.txt"]);
                assert!(!*truncated);
            }
            (ToolProjectedContent::Matches { matches, truncated }, ToolId::Grep) => {
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].path.as_str(), "data.txt");
                assert_eq!(matches[0].fragment.as_str(), "gamma needle");
                assert_eq!(matches[0].line, 1);
                assert!(!*truncated);
            }
            (ToolProjectedContent::Text { text, truncated }, ToolId::Execute) => {
                assert!(text.as_str().contains("ok"));
                assert!(!*truncated);
            }
            (content, id) => unreachable!("unexpected projection for {id}: {content:?}"),
        }
    }
}

#[test]
fn projections_preserve_collections_and_round_trip() {
    let paths = (0..=10_000)
        .map(|index| WorkspaceRelativePathDto::parse(format!("f{index}.txt")).unwrap())
        .collect::<Vec<_>>();
    let projection = ToolResult::Glob(PathsResult {
        paths,
        truncated: false,
    })
    .projection();
    let ToolProjectedContent::Paths { paths, truncated } = projection.content else {
        unreachable!("glob projection content")
    };
    assert_eq!(paths.len(), 10_001);
    assert!(!truncated);

    let matches = (0..=10_000)
        .map(|index| GrepMatch {
            path: WorkspaceRelativePathDto::parse("f.txt").unwrap(),
            line: index as u64 + 1,
            column: 1,
            fragment: BoundedText::new("needle").unwrap(),
        })
        .collect::<Vec<_>>();
    let projection = ToolResult::Grep(GrepResult {
        matches,
        truncated: true,
    })
    .projection();
    // The projection serializes losslessly for durable persistence.
    let encoded = serde_json::to_string(&projection).unwrap();
    assert_eq!(
        serde_json::from_str::<ToolResultProjection>(&encoded).unwrap(),
        projection
    );
    let ToolProjectedContent::Matches { matches, truncated } = projection.content else {
        unreachable!("grep projection content")
    };
    assert_eq!(matches.len(), 10_001);
    assert!(truncated);
}

#[test]
fn cancelled_execute_surfaces_as_an_interrupted_outcome() {
    let root_dir = fixture_dir("execute-interrupted");
    let service = service(&root_dir);
    let cancellation = CancellationSignal::new();
    let canceller = cancellation.clone();
    let helper = std::thread::spawn(move || {
        assert!(
            canceller.wait_until_spawn_observed(std::time::Duration::from_secs(10)),
            "execute child was never observed after spawn"
        );
        canceller.cancel();
    });
    let outcome = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(if cfg!(windows) { "ping" } else { "sh" }).unwrap(),
                args: if cfg!(windows) {
                    vec![
                        BoundedText::new("-n").unwrap(),
                        BoundedText::new("2").unwrap(),
                        BoundedText::new("127.0.0.1").unwrap(),
                    ]
                } else {
                    vec![
                        BoundedText::new("-c").unwrap(),
                        BoundedText::new("sleep 2").unwrap(),
                    ]
                },
            }),
            cancellation,
        )
        .expect("an interrupted execution is an outcome, not an error");
    helper.join().expect("cancellation helper completes");
    assert!(matches!(
        outcome,
        ToolDispatchOutcome::Interrupted {
            cause: InterruptCause::Stopped,
            ..
        }
    ));
}

#[test]
fn bare_result_projections_stay_bounded_and_redacted() {
    let bare = ToolResult::Edit(WriteResult { bytes: 7 }).projection();
    assert_eq!(bare.schema_version, TOOL_SCHEMA_VERSION);
    assert_eq!(bare.tool, ToolId::Edit);
    assert!(matches!(
        bare.content,
        ToolProjectedContent::Mutation { bytes: 7 }
    ));
    assert_eq!(bare.execution.cwd, REDACTED_WORKSPACE_CWD);
}
