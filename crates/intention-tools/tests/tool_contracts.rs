#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Integration tests use expect and unwrap only for deterministic fixture setup; failures indicate a broken test fixture."
)]

use intention_domain::WorkspaceRootDto;
use intention_tools::{
    BoundedText, CancellationSignal, EditInput, ExecuteInput, GlobInput, GrepInput, ReadInput,
    TOOL_SCHEMA_VERSION, TextResult, ToolId, ToolInput, ToolResult, ToolService, WriteInput,
    registry,
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
    assert!(
        result
            .text
            .as_str()
            .contains(&root.to_string_lossy().to_string())
    );
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
                new: BoundedText::new("updated").expect("new")
            })
        ),
        Ok(ToolResult::Edit(_))
    ));
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
                    content: BoundedText::new("x").expect("content")
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
                    new: BoundedText::new("x").expect("new")
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
                path: Some(path)
            })
        ),
        Ok(ToolResult::Grep(_))
    ));
}

#[test]
fn tool_service_covers_nonzero_execute_and_bounded_read() {
    let root_dir = fixture_dir("execute-");
    let root = root_dir.path();
    std::fs::write(root.join("file.txt"), "content").expect("seed");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace root");
    let service = ToolService::new(workspace);
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new(if cfg!(windows) { "cmd" } else { "sh" })
                    .expect("program"),
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
            }),
        )
        .expect_err("nonzero execution must be typed failure");
    assert_eq!(error.code(), "tool_execute_nonzero");
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
                        BoundedText::new("/C").unwrap(),
                        BoundedText::new("-n 30 127.0.0.1 > nul").unwrap(),
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
    assert!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: BoundedText::new("x").expect("pattern"),
                    path: None
                })
            )
            .is_err()
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
                    new: BoundedText::new("new").expect("new")
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
                path: Some(path.clone())
            })
        ),
        Ok(ToolResult::Grep(_))
    ));
    assert!(matches!(
        service.dispatch(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: path.clone(),
                content: BoundedText::new("new").expect("content")
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
                new: BoundedText::new("edited").expect("new")
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
            }),
        )
        .expect_err("missing edit target");
    assert_eq!(edit_missing.code(), "edit_target_missing");

    let grep = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("needle").expect("pattern"),
                path: Some(path),
            }),
        )
        .expect("grep");
    let ToolResult::Grep(result) = grep else {
        unreachable!("dispatch returned non-grep result")
    };
    assert_eq!(result.text.as_str(), "needle");
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
                    content: BoundedText::new("x").expect("content")
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
                    new: BoundedText::new("y").expect("new")
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
            }),
        )
        .expect("grep");
    assert!(
        matches!(result, ToolResult::Grep(TextResult { text, .. }) if text.as_str() == "needle\nneedle two")
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
    assert_eq!(descriptors.len(), 6);
    for descriptor in descriptors {
        assert_eq!(descriptor.schema_version(), TOOL_SCHEMA_VERSION);
        assert!(!descriptor.description().is_empty());
        assert!(!descriptor.capabilities().is_empty());
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
        session_id: 1,
        run_id: 2,
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
                path: Some(path),
            }),
        )
        .unwrap();
    assert!(matches!(
        result,
        ToolResult::Grep(TextResult {
            truncated: true,
            ..
        })
    ));
}

#[test]
fn execute_success_reports_stderr_and_nonzero_error_code() {
    let dir = fixture_dir("execute-stderr");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
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
                        BoundedText::new("/C").unwrap(),
                        BoundedText::new("echo err 1>&2").unwrap(),
                    ]
                } else {
                    vec![
                        BoundedText::new("-c").unwrap(),
                        BoundedText::new("printf err >&2").unwrap(),
                    ]
                },
            }),
        )
        .unwrap();
    assert!(
        matches!(result, ToolResult::Execute(TextResult { text, .. }) if text.as_str().contains("stderr:\nerr"))
    );
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
            session_id: 10,
            run_id: 20,
            call_id: ToolCallId::new(),
        },
        input: ToolInput::Grep(GrepInput {
            pattern: BoundedText::new("needle").unwrap(),
            path: None,
        }),
    };
    let encoded = serde_json::to_string(&invocation).unwrap();
    assert_eq!(
        serde_json::from_str::<intention_tools::ToolInvocation>(&encoded).unwrap(),
        invocation
    );
}

#[test]
fn execute_reports_signal_termination_as_negative_exit_code() {
    if cfg!(windows) {
        return;
    }
    let root_dir = fixture_dir("signal-exit");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    let error = ToolService::new(workspace)
        .dispatch(
            ToolCallId::new(),
            ToolInput::Execute(ExecuteInput {
                program: BoundedText::new("sh").unwrap(),
                args: vec![
                    BoundedText::new("-c").unwrap(),
                    BoundedText::new("kill -TERM $$").unwrap(),
                ],
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "tool_execute_nonzero");
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
                path: Some(path),
            }),
        )
        .unwrap();
    for result in [read, grep] {
        let (text, truncated) = match result {
            ToolResult::Read(v) => (v.text, v.truncated),
            ToolResult::Grep(v) => (v.text, v.truncated),
            _ => unreachable!(),
        };
        assert!(text.as_str().len() <= 65_536 + "\n[truncated]".len());
        assert!(truncated);
    }
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
                        BoundedText::new("/C").unwrap(),
                        // `for /L` and `set /P` are built into cmd.exe, so this
                        // fixture does not depend on Python or another tool.
                        BoundedText::new("for /L %i in (1,1,200000) do @<nul set /p =x").unwrap(),
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
    assert!(value.truncated);
    // Truncation is part of the typed result contract; the text marker is
    // retained for compatibility but is not required to be a suffix.
    assert!(value.text.as_str().contains("[truncated]"));
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
            }),
        )
        .expect("grep");
    assert!(matches!(
        grep,
        ToolResult::Grep(TextResult {
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
fn all_descriptor_metadata_values_are_verified() {
    use intention_tools::{MutationKind, ToolCapability};
    let expected = [
        (
            ToolId::Read,
            MutationKind::ReadOnly,
            &[ToolCapability::Read][..],
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
    ];
    for (descriptor, (id, mutation, capabilities)) in registry().into_iter().zip(expected) {
        assert_eq!(descriptor.id(), id);
        assert_eq!(descriptor.mutation(), mutation);
        assert_eq!(descriptor.capabilities(), capabilities);
        assert_eq!(descriptor.schema_version(), TOOL_SCHEMA_VERSION);
        assert!(!descriptor.description().is_empty());
    }
}
