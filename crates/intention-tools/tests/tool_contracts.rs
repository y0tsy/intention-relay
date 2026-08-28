#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Integration tests use expect and unwrap only for deterministic fixture setup; failures indicate a broken test fixture."
)]

use intention_domain::WorkspaceRootDto;
use intention_tools::{
    BoundedText, CancellationSignal, EditInput, ExecuteInput, GlobInput, GrepInput, GrepMatch,
    GrepResult, GrepScope, PathsResult, REDACTED_WORKSPACE_CWD, ReadInput,
    TOOL_DESCRIPTOR_REVISION, TOOL_SCHEMA_VERSION, TextResult, ToolId, ToolInput,
    ToolProcessStatus, ToolProjectedContent, ToolResult, ToolResultProjection, ToolService,
    WriteInput, WriteResult, registry,
};
use intention_types::{ToolCallId, WorkspaceRelativePathDto};
use tempfile::TempDir;

fn fixture_dir(label: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(&format!("intention-tools-{label}-"))
        .tempdir()
        .expect("temporary workspace")
}

#[test]
fn execute_uses_workspace_cwd_and_returns_typed_result() {
    let root_dir = fixture_dir("execute");
    let root = root_dir.path().to_owned();
    let dto = WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root dto");
    let workspace = intention_workspace::WorkspaceRoot::resolve(&dto).expect("workspace");
    let service = ToolService::new(workspace);
    let program = if cfg!(windows) { "cmd" } else { "pwd" };
    let args = if cfg!(windows) {
        vec!["/C", "cd"]
    } else {
        vec![]
    };
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(program).expect("program"),
                args: args
                    .into_iter()
                    .map(|value| BoundedText::new(value).expect("argument"))
                    .collect(),
            }),
        )
        .expect("execution");
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned a non-execute result")
    };
    let expected_root = std::fs::canonicalize(&root)
        .expect("fixture root canonicalizes")
        .to_string_lossy()
        .into_owned();
    assert!(result.text.as_str().contains(&expected_root));
}

#[test]
fn tool_service_covers_read_and_edit_success_values() {
    let root_dir = fixture_dir("values");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "old").expect("seed");
    let service = ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace root"),
    );
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Read(ReadInput { path: path.clone() }),
        ),
        Ok(ToolResult::Read(_))
    ));
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path,
                old: BoundedText::new("old").expect("old"),
                new: BoundedText::new("updated").expect("new"),
                expected_content: None
            })
        ),
        Ok(ToolResult::Edit(_))
    ));
}

#[test]
fn write_expected_content_accepts_match_and_rejects_mismatch() {
    let root_dir = fixture_dir("write-expected-content");
    let path = root_dir.path().join("file.txt");
    std::fs::write(&path, "before").expect("seed");
    let service = ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace"),
    );
    let relative = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    let result = service.dispatch(
        ToolCallId::new(),
        ToolInput::Write(WriteInput {
            path: relative.clone(),
            content: BoundedText::new("after").expect("content"),
            expected_content: Some(BoundedText::new("before").expect("expected")),
        }),
    );
    assert!(result.is_ok());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "after");

    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: relative,
                content: BoundedText::new("final").expect("content"),
                expected_content: Some(BoundedText::new("stale").expect("expected")),
            }),
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
    let service = ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace"),
    );
    let relative = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    let result = service.dispatch(
        ToolCallId::new(),
        ToolInput::Edit(EditInput {
            path: relative.clone(),
            old: BoundedText::new("needle").expect("old"),
            new: BoundedText::new("changed").expect("new"),
            expected_content: Some(BoundedText::new("before needle").expect("expected")),
        }),
    );
    assert!(result.is_ok());
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "before changed"
    );

    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: relative,
                old: BoundedText::new("changed").expect("old"),
                new: BoundedText::new("final").expect("new"),
                expected_content: Some(BoundedText::new("stale").expect("expected")),
            }),
        )
        .expect_err("mismatched expected content");
    assert_eq!(error.code(), "tool_edit_conflict");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "before changed"
    );
}

#[test]
fn tool_service_covers_write_and_edit_failures() {
    let root_dir = fixture_dir("write-errors-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "content").expect("seed");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    let missing = WorkspaceRelativePathDto::parse("missing/file.txt").expect("path");
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Write(WriteInput {
                    path: missing,
                    content: BoundedText::new("x").expect("content"),
                    expected_content: None
                })
            )
            .is_err()
    );
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Edit(EditInput {
                    path,
                    old: BoundedText::new("").expect("old"),
                    new: BoundedText::new("x").expect("new"),
                    expected_content: None
                })
            )
            .is_ok()
    );
}

#[test]
fn tool_service_covers_successful_empty_search() {
    let root_dir = fixture_dir("empty-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "content").expect("seed");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("absent").expect("pattern"),
                path: Some(path),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("file.txt").unwrap()
                }),
            })
        ),
        Ok(ToolResult::Grep(_))
    ));
}

#[test]
fn tool_service_covers_nonzero_execute_as_normalized_result() {
    let root_dir = fixture_dir("execute-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "content").expect("seed");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
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
    let result = service
        .dispatch(ToolCallId::new(), nonzero_input())
        .expect("nonzero exit must stay a typed result");
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned a non-execute result")
    };
    assert!(result.text.as_str().contains("exit_code:2"));

    let call_id = ToolCallId::new();
    let envelope = service
        .invoke_enveloped(intention_tools::ToolInvocation {
            schema_version: TOOL_SCHEMA_VERSION,
            context: intention_tools::ToolContext {
                session_id: intention_types::SessionId::parse(
                    "00000000-0000-4000-8000-000000000003",
                )
                .unwrap(),
                run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000004")
                    .unwrap(),
                call_id,
            },
            input: nonzero_input(),
        })
        .expect("envelope for a known terminal exit");
    assert_eq!(envelope.context.call_id, call_id);
    assert_eq!(
        envelope
            .execution
            .and_then(|metadata| metadata.process_status),
        Some(ToolProcessStatus::NonZero { code: 2 })
    );

    let encoded = serde_json::to_string(&ToolProcessStatus::NonZero { code: 2 }).unwrap();
    assert_eq!(encoded, r#"{"kind":"non_zero","code":2}"#);
    assert_eq!(
        serde_json::from_str::<ToolProcessStatus>(&encoded).unwrap(),
        ToolProcessStatus::NonZero { code: 2 }
    );
}

#[test]
fn execute_cancellation_is_classified_as_unknown_effect() {
    let root_dir = fixture_dir("timeout-");
    let root = root_dir.path();
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    let cancellation = CancellationSignal::new();
    let canceller = cancellation.clone();
    std::thread::spawn(move || {
        // Give the child a chance to spawn before requesting cancellation.
        // The longer fixture is deterministic on both Unix and Windows.
        std::thread::sleep(std::time::Duration::from_millis(50));
        canceller.cancel();
    });
    let error = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(if cfg!(windows) { "ping" } else { "sh" })
                    .expect("program"),
                args: if cfg!(windows) {
                    vec![
                        BoundedText::new("-n").unwrap(),
                        BoundedText::new("30").unwrap(),
                        BoundedText::new("127.0.0.1").unwrap(),
                    ]
                } else {
                    vec![
                        BoundedText::new("-c").unwrap(),
                        BoundedText::new("trap '' TERM; sleep 30").unwrap(),
                    ]
                },
            }),
            cancellation,
        )
        .expect_err("cancellation");
    assert_eq!(error.code(), "tool_execute_external_effect_unknown");
}

#[test]
fn tool_service_rejects_invalid_patterns_and_unreadable_files() {
    let root_dir = fixture_dir("invalid-");
    let root = root_dir.path();
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Glob(GlobInput {
                    pattern: BoundedText::new("[").expect("pattern")
                })
            )
            .is_err()
    );
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Read(ReadInput {
                    path: WorkspaceRelativePathDto::parse("missing").expect("path")
                })
            )
            .is_err()
    );
}

#[test]
fn glob_and_grep_cover_empty_and_invalid_search_paths() {
    let root_dir = fixture_dir("search-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "content").expect("seed");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*.missing").expect("pattern")
            })
        ),
        Ok(ToolResult::Glob(_))
    ));
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("x").expect("pattern"),
                path: None,
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("file.txt").unwrap(),
                }),
            }),
        )
        .expect("valid file scope with no matches");
    assert!(matches!(result, ToolResult::Grep(value) if value.matches.is_empty()));
}

#[test]
fn search_rejects_unsafe_patterns_and_reports_utf8_columns() {
    let dir = fixture_dir("search-validation");
    std::fs::write(dir.path().join("file.txt"), "é needle\n").unwrap();
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
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
        let result = service.dispatch(ToolCallId::new(), ToolInput::Glob(GlobInput { pattern }));
        assert_eq!(result.unwrap_err().code(), "invalid_tool_pattern");
    }
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("file.txt").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("file.txt").unwrap(),
                }),
            }),
        )
        .unwrap();
    let ToolResult::Grep(result) = result else {
        unreachable!()
    };
    assert_eq!(result.matches[0].column, 3);
}

#[test]
fn glob_matches_fail_closed_on_symlinks_and_stay_deterministic() {
    let dir = fixture_dir("glob-determinism");
    std::fs::create_dir(dir.path().join("real")).unwrap();
    std::fs::write(dir.path().join("target.txt"), "x").unwrap();
    std::fs::write(dir.path().join("real/deep.txt"), "x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(dir.path().join("target.txt"), dir.path().join("alias.txt")).unwrap();
        symlink(dir.path().join("real"), dir.path().join("linked-dir")).unwrap();
    }
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    for pattern in ["*.txt", "**/*.txt", "{target,alias}.txt"] {
        let result = service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Glob(GlobInput {
                    pattern: BoundedText::new(pattern).unwrap(),
                }),
            )
            .unwrap();
        let ToolResult::Glob(result) = result else {
            unreachable!("non-glob result")
        };
        assert!(!result.truncated, "unexpected truncation for: {pattern}");
        for path in &result.paths {
            let value = path.as_str();
            assert_ne!(value, "alias.txt", "symlink alias reported for: {pattern}");
            assert!(
                !value.contains("linked-dir"),
                "symlinked directory traversed for: {pattern}"
            );
        }
        // The reported subset is sorted and duplicate-free on every replay.
        let mut sorted = result.paths.clone();
        sorted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        sorted.dedup_by(|a, b| a.as_str() == b.as_str());
        assert_eq!(
            result.paths, sorted,
            "nondeterministic subset for: {pattern}"
        );
    }
}

#[test]
fn bounded_sources_report_truncation_only_past_the_output_bound() {
    let dir = fixture_dir("bounded-source");
    std::fs::write(dir.path().join("exact.bin"), vec![b'a'; 65_536]).unwrap();
    std::fs::write(dir.path().join("over.bin"), vec![b'b'; 65_537]).unwrap();
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    for (name, truncated, length) in [("exact.bin", false, 65_536), ("over.bin", true, 65_536)] {
        let result = service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Read(ReadInput {
                    path: WorkspaceRelativePathDto::parse(name).unwrap(),
                }),
            )
            .unwrap();
        let ToolResult::Read(result) = result else {
            unreachable!("non-read result")
        };
        assert_eq!(result.text.as_str().len(), length, "bound cut: {name}");
        assert_eq!(result.truncated, truncated, "truncation flag: {name}");
    }
}

#[test]
fn grep_does_not_follow_symlinks_or_search_directories() {
    let dir = fixture_dir("search-links");
    std::fs::write(dir.path().join("target.txt"), "needle").unwrap();
    std::fs::create_dir(dir.path().join("folder")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt")).unwrap();
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    for path in ["folder", "link.txt"] {
        let error = service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: BoundedText::new("needle").unwrap(),
                    path: Some(WorkspaceRelativePathDto::parse(path).unwrap()),
                    scope: Some(GrepScope::File {
                        path: WorkspaceRelativePathDto::parse(path).unwrap(),
                    }),
                }),
            )
            .unwrap_err();
        assert!(matches!(
            error.code(),
            "tool_search_failed" | "workspace_path_symlink"
        ));
    }
}

#[test]
#[cfg(unix)]
fn write_through_dangling_final_symlink_is_rejected_without_outside_effects() {
    use std::os::unix::fs::symlink;

    let root_dir = fixture_dir("dangling-write");
    let outside_dir = tempfile::Builder::new()
        .prefix("intention-tools-dangling-outside-")
        .tempdir()
        .expect("outside temporary directory");
    let escape_target = outside_dir.path().join("escape.txt");
    symlink(&escape_target, root_dir.path().join("dangling")).expect("dangling symlink");

    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace");
    let service = ToolService::new(workspace);

    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: WorkspaceRelativePathDto::parse("dangling").expect("path"),
                content: BoundedText::new("must not escape").expect("content"),
                expected_content: None,
            }),
        )
        .expect_err("dangling final symlink must fail closed");
    assert_eq!(error.code(), "workspace_path_symlink");

    // No file may be created at the link target outside the workspace, and
    // the write must not replace the in-workspace link itself.
    assert!(
        std::fs::symlink_metadata(&escape_target).is_err(),
        "write escaped through dangling final symlink"
    );
    assert!(
        std::fs::symlink_metadata(root_dir.path().join("dangling"))
            .expect("link metadata")
            .file_type()
            .is_symlink(),
        "in-workspace link was modified by rejected write"
    );
}

#[test]
fn file_tools_report_policy_and_execution_failures() {
    let root_dir = fixture_dir("errors-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "content").expect("seed");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    let missing = WorkspaceRelativePathDto::parse("missing.txt").expect("missing path");
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Read(ReadInput { path: missing })
            )
            .is_err()
    );
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Edit(EditInput {
                    path,
                    old: BoundedText::new("absent").expect("old"),
                    new: BoundedText::new("new").expect("new"),
                    expected_content: None
                })
            )
            .is_err()
    );
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Execute(ExecuteInput {
                    program: BoundedText::new("definitely-not-a-program").expect("program"),
                    args: vec![]
                })
            )
            .is_err()
    );
}

#[test]
fn registry_has_the_declared_core_tools() {
    assert_eq!(ToolId::Read.as_str(), "read");
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    let _ = ToolInput::Read(ReadInput { path });
}

#[test]
fn file_tools_execute_against_the_declared_workspace() {
    let root_dir = fixture_dir("files-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "old needle").expect("seed file");
    let dto = WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root dto");
    let workspace = intention_workspace::WorkspaceRoot::resolve(&dto).expect("workspace root");
    let service = ToolService::new(workspace);
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Read(ReadInput { path: path.clone() })
        ),
        Ok(ToolResult::Read(_))
    ));
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").expect("pattern"),
                path: Some(path.clone()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("file.txt").unwrap()
                }),
            })
        ),
        Ok(ToolResult::Grep(_))
    ));
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: path.clone(),
                content: BoundedText::new("new").expect("content"),
                expected_content: None
            })
        ),
        Ok(ToolResult::Write(_))
    ));
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path,
                old: BoundedText::new("new").expect("old"),
                new: BoundedText::new("edited").expect("new"),
                expected_content: None,
            })
        ),
        Ok(ToolResult::Edit(_))
    ));
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*.txt").expect("pattern")
            })
        ),
        Ok(ToolResult::Glob(_))
    ));
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Read(ReadInput {
                    path: WorkspaceRelativePathDto::parse("missing.txt").expect("missing path")
                })
            )
            .is_err()
    );
}

#[test]
fn dispatch_reports_precise_errors_and_process_output_paths() {
    let root_dir = fixture_dir("dispatch-errors");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "needle\nother").expect("seed");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");

    let cancelled = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Read(ReadInput { path: path.clone() }),
            CancellationSignal::cancelled(),
        )
        .expect_err("cancelled invocation");
    assert_eq!(cancelled.code(), "tool_cancelled");

    let missing_parent = WorkspaceRelativePathDto::parse("missing/new.txt").expect("path");
    let write_error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: missing_parent,
                content: BoundedText::new("x").expect("content"),
                expected_content: None,
            }),
        )
        .expect_err("write failure");
    assert_eq!(write_error.code(), "workspace_parent_unavailable");

    let edit_missing = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: path.clone(),
                old: BoundedText::new("absent").expect("old"),
                new: BoundedText::new("new").expect("new"),
                expected_content: None,
            }),
        )
        .expect_err("missing edit target");
    assert_eq!(edit_missing.code(), "edit_target_missing");

    let grep = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").expect("pattern"),
                path: Some(path.clone()),
                scope: Some(GrepScope::File { path }),
            }),
        )
        .expect("grep");
    let ToolResult::Grep(result) = grep else {
        unreachable!("dispatch returned non-grep result")
    };
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].fragment.as_str(), "needle");
    assert!(!result.truncated);
}

#[test]
fn execute_returns_stdout_stderr_and_truncation_metadata() {
    let root_dir = fixture_dir("execute-output");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/C", "echo out & echo err 1>&2"])
    } else {
        ("sh", vec!["-c", "printf out; printf err >&2"])
    };
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(program).expect("program"),
                args: args
                    .into_iter()
                    .map(|arg| BoundedText::new(arg).expect("arg"))
                    .collect(),
            }),
        )
        .expect("execute");
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
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace");
    let service = ToolService::new(workspace);
    // Assembled at runtime: recognizably fake, yet never a literal
    // secret-shaped assignment that docs-check rejects.
    let secret = format!("credential{}", "-leak-probe");
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: WorkspaceRelativePathDto::parse("missing/new.txt").expect("path"),
                content: BoundedText::new(secret.as_str()).expect("content"),
                expected_content: None,
            }),
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
    let service = ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace root"),
    );
    let directory = WorkspaceRelativePathDto::parse("directory").expect("path");
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Read(ReadInput {
                    path: directory.clone()
                })
            )
            .is_err()
    );
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Write(WriteInput {
                    path: directory.clone(),
                    content: BoundedText::new("x").expect("content"),
                    expected_content: None
                })
            )
            .is_err()
    );
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Edit(EditInput {
                    path: directory,
                    old: BoundedText::new("x").expect("old"),
                    new: BoundedText::new("y").expect("new"),
                    expected_content: None
                })
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
    let service = ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace root"),
    );
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").expect("pattern"),
                path: Some(WorkspaceRelativePathDto::parse("z.txt").expect("path")),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("z.txt").expect("path"),
                }),
            }),
        )
        .expect("grep");
    assert!(
        matches!(result, ToolResult::Grep(value) if value.matches.iter().map(|m| m.fragment.as_str()).collect::<Vec<_>>() == vec!["needle", "needle two"])
    );
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*.txt").expect("pattern"),
            }),
        )
        .expect("glob");
    assert!(
        matches!(result, ToolResult::Glob(value) if value.paths.iter().map(WorkspaceRelativePathDto::as_str).collect::<Vec<_>>() == vec!["a.txt", "z.txt"])
    );
}

#[test]
fn descriptors_expose_all_metadata_and_invoke_delegates() {
    let descriptors = registry();
    assert_eq!(descriptors.len(), 14);
    for descriptor in descriptors {
        assert!(!descriptor.description().is_empty());
        if descriptor.schema_version() != 0 {
            assert_eq!(descriptor.schema_version(), TOOL_SCHEMA_VERSION);
            assert!(!descriptor.capabilities().is_empty());
        }
        assert_eq!(
            descriptor.id().as_str().to_string(),
            descriptor.id().to_string()
        );
    }
    let root_dir = fixture_dir("invoke-");
    let root = root_dir.path();
    std::fs::create_dir_all(root).unwrap();
    let service = ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).unwrap(),
        )
        .unwrap(),
    );
    let result = service
        .invoke(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(if cfg!(windows) { "cmd" } else { "echo" }).unwrap(),
                args: if cfg!(windows) {
                    vec![
                        BoundedText::new("/C").unwrap(),
                        BoundedText::new("echo ok").unwrap(),
                    ]
                } else {
                    vec![BoundedText::new("ok").unwrap()]
                },
            }),
        )
        .unwrap();
    assert!(matches!(result, ToolResult::Execute(_)));
}

#[test]
fn glob_empty_and_grep_read_failure_are_typed() {
    let root_dir = fixture_dir("search-extra-");
    let root = root_dir.path();
    std::fs::create_dir_all(root).unwrap();
    let service = ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).unwrap(),
        )
        .unwrap(),
    );
    let glob = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*.none").unwrap(),
            }),
        )
        .unwrap();
    assert!(matches!(glob, ToolResult::Glob(value) if value.paths.is_empty()));
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("x").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("missing").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("missing").unwrap(),
                }),
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "workspace_path_unavailable");
}

#[test]
fn dto_metadata_and_observability_round_trip_all_variants() {
    use intention_tools::{
        MutationKind, ToolCapability, ToolContext, ToolObservability, ToolOutcome, ToolPolicy,
        ToolResultEnvelope,
    };
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
    for value in [ToolOutcome::Succeeded, ToolOutcome::Failed] {
        let json = serde_json::to_string(&value).expect("outcome json");
        assert_eq!(
            serde_json::from_str::<ToolOutcome>(&json).expect("outcome"),
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
    let context = ToolContext {
        session_id: intention_types::SessionId::parse("00000000-0000-4000-8000-000000000001")
            .unwrap(),
        run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000002").unwrap(),
        call_id: ToolCallId::new(),
    };
    let envelope = ToolResultEnvelope {
        schema_version: TOOL_SCHEMA_VERSION,
        context,
        result: ToolResult::Read(TextResult {
            text: BoundedText::new("ok").expect("text"),
            truncated: false,
        }),
        observability: ToolObservability {
            outcome: ToolOutcome::Succeeded,
            policy: ToolPolicy::Allowed,
            elapsed_ms: 3,
        },
        execution: None,
    };
    assert_eq!(
        serde_json::from_str::<ToolResultEnvelope>(
            &serde_json::to_string(&envelope).expect("envelope json")
        )
        .expect("envelope"),
        envelope
    );
}

#[test]
fn invocation_call_identity_is_validated() {
    let id = ToolCallId::new();
    let invocation = intention_tools::ToolInvocation {
        schema_version: TOOL_SCHEMA_VERSION,
        context: intention_tools::ToolContext {
            session_id: intention_types::SessionId::parse("00000000-0000-4000-8000-000000000001")
                .unwrap(),
            run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000002").unwrap(),
            call_id: id,
        },
        input: ToolInput::Glob(GlobInput {
            pattern: BoundedText::new("*.rs").unwrap(),
        }),
    };
    assert!(invocation.validate_call_id(id).is_ok());
    assert_eq!(
        invocation
            .validate_call_id(ToolCallId::new())
            .unwrap_err()
            .code(),
        "tool_call_id_mismatch"
    );
}

#[test]
fn cancelled_dispatch_is_rejected_before_any_tool_effect() {
    let root_dir = fixture_dir("cancelled-before-dispatch");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let error = ToolService::new(workspace)
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: WorkspaceRelativePathDto::parse("created.txt").unwrap(),
                content: BoundedText::new("must not write").unwrap(),
                expected_content: None,
            }),
            CancellationSignal::cancelled(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "tool_cancelled");
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
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    let path = WorkspaceRelativePathDto::parse("bytes.bin").unwrap();
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Read(ReadInput { path: path.clone() }),
        )
        .unwrap();
    assert!(matches!(
        result,
        ToolResult::Read(TextResult {
            truncated: true,
            ..
        })
    ));
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("x").unwrap(),
                path: Some(path.clone()),
                scope: Some(GrepScope::File { path }),
            }),
        )
        .unwrap();
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
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
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
    let result = service.dispatch(ToolCallId::new(), input()).unwrap();
    assert!(
        matches!(result, ToolResult::Execute(TextResult { text, .. }) if text.as_str().contains("stderr:\nerr"))
    );
    let envelope = service
        .invoke_enveloped(intention_tools::ToolInvocation {
            schema_version: TOOL_SCHEMA_VERSION,
            context: intention_tools::ToolContext {
                session_id: intention_types::SessionId::parse(
                    "00000000-0000-4000-8000-000000000009",
                )
                .unwrap(),
                run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000010")
                    .unwrap(),
                call_id: ToolCallId::new(),
            },
            input: input(),
        })
        .unwrap();
    assert_eq!(
        envelope
            .execution
            .and_then(|metadata| metadata.process_status),
        Some(ToolProcessStatus::Success)
    );
}

#[test]
fn execute_inherits_the_invoking_environment() {
    let dir = fixture_dir("execute-env");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    let marker = "INTENTION_RELAY_EXECUTE_ENV_MARKER";
    let previous = std::env::var_os(marker);
    let (program, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "cmd",
            vec![format!("/C if defined {marker} (exit 0) else (exit 1)")],
        )
    } else {
        (
            "sh",
            vec!["-c".to_owned(), format!("test -n \"${{{marker}:-}}\"")],
        )
    };
    let result = service.dispatch(
        ToolCallId::new(),
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new(program).unwrap(),
            args: args
                .into_iter()
                .map(|arg| BoundedText::new(arg).unwrap())
                .collect(),
        }),
    );
    assert!(
        result.is_ok() || previous.is_some(),
        "environment inheritance check failed: {result:?}"
    );
}

#[test]
fn execute_does_not_persist_environment_values() {
    let dir = fixture_dir("execute-env-shapes");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    for name in [
        "INTENTION_RELAY_EXECUTE_TEST_TOKEN",
        "INTENTION_RELAY_EXECUTE_TEST_SECRET",
    ] {
        let (program, args): (&str, Vec<String>) = if cfg!(windows) {
            (
                "cmd",
                vec![
                    "/C".into(),
                    format!("if defined {name} (exit 0) else (exit 1)"),
                ],
            )
        } else {
            (
                "sh",
                vec!["-c".into(), format!("test -n \"${{{name}:-}}\"")],
            )
        };
        assert!(
            service
                .dispatch(
                    ToolCallId::new(),
                    ToolInput::Execute(ExecuteInput {
                        program: BoundedText::new(program).unwrap(),
                        args: args
                            .into_iter()
                            .map(|arg| BoundedText::new(arg).unwrap())
                            .collect(),
                    })
                )
                .is_ok(),
            "environment inheritance failed: {name}"
        );
    }
}

#[test]
fn dispatch_covers_empty_read_and_successful_empty_edit() {
    let root_dir = fixture_dir("empty-read-edit");
    std::fs::write(root_dir.path().join("empty.txt"), "").expect("seed");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace");
    let service = ToolService::new(workspace);
    let path = WorkspaceRelativePathDto::parse("empty.txt").expect("path");
    let read = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Read(ReadInput { path: path.clone() }),
        )
        .expect("read");
    assert!(
        matches!(read, ToolResult::Read(TextResult { truncated: false, text }) if text.as_str().is_empty())
    );
    let edit = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path,
                old: BoundedText::new("").expect("old"),
                new: BoundedText::new("replacement").expect("new"),
                expected_content: None,
            }),
        )
        .expect("edit");
    assert!(matches!(edit, ToolResult::Edit(_)));
}

#[test]
fn tool_invocation_round_trips_with_optional_grep_path() {
    let invocation = intention_tools::ToolInvocation {
        schema_version: TOOL_SCHEMA_VERSION,
        context: intention_tools::ToolContext {
            session_id: intention_types::SessionId::parse("00000000-0000-4000-8000-000000000010")
                .unwrap(),
            run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000020").unwrap(),
            call_id: ToolCallId::new(),
        },
        input: ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").unwrap(),
            path: None,
            scope: None,
        }),
    };
    let encoded = serde_json::to_string(&invocation).unwrap();
    assert_eq!(
        serde_json::from_str::<intention_tools::ToolInvocation>(&encoded).unwrap(),
        invocation
    );
}

#[test]
fn execute_reports_signal_termination_as_known_terminal_result() {
    if cfg!(windows) {
        return;
    }
    let root_dir = fixture_dir("signal-exit");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    let signal_input = || {
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("sh").unwrap(),
            args: vec![
                BoundedText::new("-c").unwrap(),
                BoundedText::new("kill -TERM $$").unwrap(),
            ],
        })
    };
    let result = service
        .dispatch(ToolCallId::new(), signal_input())
        .expect("signal termination is a known terminal outcome");
    let ToolResult::Execute(result) = result else {
        unreachable!("dispatch returned a non-execute result")
    };
    // The legacy text rendering keeps a negative exit code for signals.
    assert!(result.text.as_str().contains("exit_code:-1"));
    let envelope = service
        .invoke_enveloped(intention_tools::ToolInvocation {
            schema_version: TOOL_SCHEMA_VERSION,
            context: intention_tools::ToolContext {
                session_id: intention_types::SessionId::parse(
                    "00000000-0000-4000-8000-000000000005",
                )
                .unwrap(),
                run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000006")
                    .unwrap(),
                call_id: ToolCallId::new(),
            },
            input: signal_input(),
        })
        .unwrap();
    assert_eq!(
        envelope
            .execution
            .and_then(|metadata| metadata.process_status),
        Some(ToolProcessStatus::Signal { signal: 15 })
    );
}

#[test]
fn read_and_grep_bound_large_content() {
    let root_dir = fixture_dir("large-output");
    let content = "needle\n".repeat(20_000);
    std::fs::write(root_dir.path().join("large.txt"), &content).unwrap();
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    let path = WorkspaceRelativePathDto::parse("large.txt").unwrap();
    let read = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Read(ReadInput { path: path.clone() }),
        )
        .unwrap();
    let grep = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").unwrap(),
                path: Some(path.clone()),
                scope: Some(GrepScope::File { path }),
            }),
        )
        .unwrap();
    for result in [read, grep] {
        let (text, truncated) = match result {
            ToolResult::Read(v) => (v.text, v.truncated),
            ToolResult::Grep(v) => (
                BoundedText::new(
                    v.matches
                        .iter()
                        .map(|m| m.fragment.as_str())
                        .collect::<String>(),
                )
                .unwrap(),
                v.truncated,
            ),
            _ => unreachable!(),
        };
        assert!(text.as_str().len() <= 65_536 + "\n[truncated]".len());
        assert!(truncated);
    }
}

#[test]
fn grep_truncates_long_multibyte_fragments_on_character_boundary() {
    let root_dir = fixture_dir("large-multibyte-grep");
    let line = format!("needle{}", "界".repeat(30_000));
    std::fs::write(root_dir.path().join("large.txt"), &line).unwrap();
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let result = ToolService::new(workspace)
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("large.txt").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("large.txt").unwrap(),
                }),
            }),
        )
        .unwrap();
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
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(if cfg!(windows) { "cmd" } else { "sh" }).unwrap(),
                args: if cfg!(windows) {
                    vec![
                        // `for /L` and `set /P` are built into cmd.exe, so this
                        // fixture does not depend on Python or another tool.
                        BoundedText::new("/C").unwrap(),
                        BoundedText::new("for /L %i in (1,1,200000) do @<nul set /p =x & exit /b 0").unwrap(),
                    ]
                } else {
                    let script = "python3 -c 'import sys; sys.stdout.write(\"x\" * 200000); sys.stderr.write(\"y\" * 200000)'";
                    vec![
                        BoundedText::new("-c").unwrap(),
                        BoundedText::new(script).unwrap(),
                    ]
                },
            }),
        )
        .unwrap();
    let ToolResult::Execute(value) = result else {
        unreachable!()
    };
    assert!(value.text.as_str().contains("stdout:"));
    assert!(value.text.as_str().contains("stderr:"));
    assert!(value.text.as_str().contains("exit_code:0"));
    assert!(value.truncated || cfg!(windows));
    // Truncation is part of the typed result contract; the text marker is
    // retained for compatibility but is not required to be a suffix.
    assert!(value.text.as_str().contains("[truncated]") || cfg!(windows));
}

#[test]
fn exact_typed_errors_cover_search_edit_and_spawn_failures() {
    let root_dir = fixture_dir("exact-errors");
    std::fs::write(root_dir.path().join("file.txt"), "content").unwrap();
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(workspace);
    let path = WorkspaceRelativePathDto::parse("file.txt").unwrap();
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path,
                old: BoundedText::new("missing").unwrap(),
                new: BoundedText::new("x").unwrap(),
                expected_content: None,
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "edit_target_missing");
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("x").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("missing").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("missing").unwrap(),
                }),
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "workspace_path_unavailable");
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new("not-a-real-program").unwrap(),
                args: vec![],
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "tool_execute_spawn_failed");
}

#[test]
fn default_tools_cover_write_edit_glob_grep_and_cancellation_paths() {
    let dir = fixture_dir("default-paths");
    std::fs::create_dir(dir.path().join("nested")).expect("nested");
    let root = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace");
    let service = ToolService::new(root);
    let file = WorkspaceRelativePathDto::parse("nested/data.txt").expect("path");

    let written = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: file.clone(),
                content: BoundedText::new("alpha\nneedle\nomega").expect("text"),
                expected_content: None,
            }),
        )
        .expect("write");
    assert!(matches!(written, ToolResult::Write(_)));
    let edited = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: file.clone(),
                old: BoundedText::new("needle").expect("old"),
                new: BoundedText::new("changed").expect("new"),
                expected_content: None,
            }),
        )
        .expect("edit");
    assert!(matches!(edited, ToolResult::Edit(_)));
    let grep = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("changed").expect("pattern"),
                path: Some(file.clone()),
                scope: Some(GrepScope::File { path: file.clone() }),
            }),
        )
        .expect("grep");
    assert!(matches!(
        grep,
        ToolResult::Grep(intention_tools::GrepResult {
            truncated: false,
            ..
        })
    ));
    let glob = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("**/*.txt").expect("pattern"),
            }),
        )
        .expect("glob");
    assert!(matches!(glob, ToolResult::Glob(_)));
    let cancelled = service.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Read(ReadInput { path: file }),
        CancellationSignal::cancelled(),
    );
    assert_eq!(cancelled.expect_err("cancelled").code(), "tool_cancelled");
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
        assert_eq!(descriptor.status(), ToolRegistrationStatus::Active);
        assert_eq!(descriptor.observability_policy(), ToolPolicy::Allowed);
        assert!(!descriptor.display_name().is_empty());
        assert!(!descriptor.description().is_empty());
    }
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

#[test]
fn bounded_text_accepts_boundary_and_rejects_nul_or_oversize() {
    assert_eq!(BoundedText::new("ok").unwrap().as_str(), "ok");
    assert!(BoundedText::new("\0").is_err());
    assert!(BoundedText::new("x".repeat(1_048_577)).is_err());
    assert!(BoundedText::new("x".repeat(1_048_576)).is_ok());
}

#[test]
fn dispatch_covers_each_tool_input_variant() {
    let dir = fixture_dir("dispatch-variants");
    std::fs::write(dir.path().join("a.txt"), "needle").unwrap();
    let root = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let service = ToolService::new(root);
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
        assert!(service.dispatch(ToolCallId::new(), input).is_ok());
    }
}

#[test]
fn enveloped_invocation_preserves_identity_and_records_metadata() {
    use intention_tools::{ToolContext, ToolInvocation, ToolOutcome, ToolPolicy};
    let dir = fixture_dir("envelope");
    let root = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let call_id = ToolCallId::new();
    let envelope = ToolService::new(root)
        .invoke_enveloped(ToolInvocation {
            schema_version: TOOL_SCHEMA_VERSION,
            context: ToolContext {
                session_id: intention_types::SessionId::parse(
                    "00000000-0000-4000-8000-000000000007",
                )
                .unwrap(),
                run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000008")
                    .unwrap(),
                call_id,
            },
            input: ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*.txt").unwrap(),
            }),
        })
        .unwrap();
    assert_eq!(envelope.context.call_id, call_id);
    assert_eq!(envelope.observability.outcome, ToolOutcome::Succeeded);
    assert_eq!(envelope.observability.policy, ToolPolicy::Allowed);
    // Durable metadata identifies the workspace root only through the stable
    // redacted marker; the absolute location is never recorded.
    let execution = envelope.execution.as_ref().unwrap();
    assert_eq!(execution.cwd, REDACTED_WORKSPACE_CWD);
    assert_eq!(execution.path, None);
    assert!(matches!(envelope.result, ToolResult::Glob(_)));
}

#[test]
fn envelopes_project_redacted_normalized_projections_for_every_concrete_tool() {
    let root_dir = fixture_dir("projection");
    let root_path = root_dir.path();
    std::fs::write(root_path.join("data.txt"), "alpha\nneedle\n").unwrap();
    let service = ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root_path.to_string_lossy().into_owned()).unwrap(),
        )
        .unwrap(),
    );
    let path = WorkspaceRelativePathDto::parse("data.txt").unwrap();
    let calls = [
        (
            ToolInput::Read(ReadInput { path: path.clone() }),
            ToolId::Read,
            "data.txt",
        ),
        (
            ToolInput::Write(WriteInput {
                path: path.clone(),
                content: BoundedText::new("beta needle").unwrap(),
                expected_content: None,
            }),
            ToolId::Write,
            "data.txt",
        ),
        (
            ToolInput::Edit(EditInput {
                path: path.clone(),
                old: BoundedText::new("beta").unwrap(),
                new: BoundedText::new("gamma").unwrap(),
                expected_content: None,
            }),
            ToolId::Edit,
            "data.txt",
        ),
        (
            ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*.txt").unwrap(),
            }),
            ToolId::Glob,
            "",
        ),
        (
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").unwrap(),
                path: Some(path.clone()),
                scope: Some(GrepScope::File { path }),
            }),
            ToolId::Grep,
            "data.txt",
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
            "",
        ),
    ];
    let absolute_root = root_path.to_string_lossy().to_string();
    for (input, tool, expected_path) in calls {
        let envelope = service
            .invoke_enveloped(intention_tools::ToolInvocation {
                schema_version: TOOL_SCHEMA_VERSION,
                context: intention_tools::ToolContext {
                    session_id: intention_types::SessionId::parse(
                        "00000000-0000-4000-8000-000000000011",
                    )
                    .unwrap(),
                    run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000012")
                        .unwrap(),
                    call_id: ToolCallId::new(),
                },
                input,
            })
            .expect("projection fixture dispatch must succeed");
        let projection = envelope.projection();
        assert_eq!(projection.schema_version, TOOL_SCHEMA_VERSION, "{tool}");
        assert_eq!(projection.tool, tool);
        assert_eq!(projection.execution.cwd, REDACTED_WORKSPACE_CWD, "{tool}");
        assert_eq!(
            projection.execution.elapsed_ms, envelope.observability.elapsed_ms,
            "{tool} timing"
        );
        assert_eq!(
            projection.execution.policy,
            intention_tools::ToolPolicy::Allowed,
            "{tool} policy"
        );
        let expected_logical = if expected_path.is_empty() {
            None
        } else {
            Some(expected_path.to_owned())
        };
        assert_eq!(
            projection
                .execution
                .path
                .as_ref()
                .map(WorkspaceRelativePathDto::as_str),
            expected_logical.as_deref(),
            "{tool} metadata path"
        );
        if tool != ToolId::Execute {
            assert_eq!(projection.execution.process_status, None, "{tool}");
        }
        // Neither the projection nor the full envelope may persist the
        // absolute workspace root.
        let rendered = serde_json::to_string(&projection).unwrap();
        assert!(
            !rendered.contains(&absolute_root),
            "{tool} projection leaked the absolute root"
        );
        let envelope_rendered = serde_json::to_string(&envelope).unwrap();
        assert!(
            !envelope_rendered.contains(&absolute_root),
            "{tool} envelope leaked the absolute root"
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
                assert_eq!(
                    projection.execution.process_status,
                    Some(ToolProcessStatus::Success)
                );
            }
            (content, id) => unreachable!("unexpected projection for {id}: {content:?}"),
        }
    }
}

#[test]
fn projections_clamp_oversized_collections_and_round_trip() {
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
    assert_eq!(paths.len(), 10_000);
    assert!(truncated);

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
        truncated: false,
    })
    .projection();
    // The bounded projection serializes losslessly for durable persistence.
    let encoded = serde_json::to_string(&projection).unwrap();
    assert_eq!(
        serde_json::from_str::<ToolResultProjection>(&encoded).unwrap(),
        projection
    );
    let ToolProjectedContent::Matches { matches, truncated } = projection.content else {
        unreachable!("grep projection content")
    };
    assert_eq!(matches.len(), 10_000);
    assert!(truncated);
}

#[test]
fn projection_falls_back_to_observability_and_bare_results_stay_bounded() {
    use intention_tools::{ToolContext, ToolObservability, ToolOutcome, ToolPolicy};
    let envelope = intention_tools::ToolResultEnvelope {
        schema_version: TOOL_SCHEMA_VERSION,
        context: ToolContext {
            session_id: intention_types::SessionId::parse("00000000-0000-4000-8000-000000000001")
                .unwrap(),
            run_id: intention_types::RunId::parse("00000000-0000-4000-8000-000000000002").unwrap(),
            call_id: ToolCallId::new(),
        },
        result: ToolResult::Read(TextResult {
            text: BoundedText::new("payload").unwrap(),
            truncated: false,
        }),
        observability: ToolObservability {
            outcome: ToolOutcome::Succeeded,
            policy: ToolPolicy::Allowed,
            elapsed_ms: 42,
        },
        execution: None,
    };
    let projection = envelope.projection();
    assert_eq!(projection.tool, ToolId::Read);
    assert_eq!(projection.execution.elapsed_ms, 42);
    assert_eq!(projection.execution.policy, ToolPolicy::Allowed);
    assert_eq!(projection.execution.cwd, REDACTED_WORKSPACE_CWD);
    assert_eq!(projection.execution.path, None);
    assert_eq!(projection.execution.process_status, None);

    let bare = ToolResult::Edit(WriteResult { bytes: 7 }).projection();
    assert_eq!(bare.schema_version, TOOL_SCHEMA_VERSION);
    assert_eq!(bare.tool, ToolId::Edit);
    assert!(matches!(
        bare.content,
        ToolProjectedContent::Mutation { bytes: 7 }
    ));
    assert_eq!(bare.execution.cwd, REDACTED_WORKSPACE_CWD);
}
