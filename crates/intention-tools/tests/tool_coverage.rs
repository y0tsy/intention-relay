//! Consolidated coverage cases: schema version, dispatch and envelope
//! fallbacks, metadata validation, search scopes, logical paths, and symlink
//! handling, merged from the six former padded coverage targets.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

mod common;

use common::{DispatchCompleted, fixture_context, fixture_service, relative, text};
#[cfg(unix)]
use common::{fixture_dir, service};
use intention_proto::ToolCallId;
#[cfg(unix)]
use intention_tools::ToolDispatchOutcome;
use intention_tools::{
    BoundedText, CancellationSignal, EditInput, ExecuteInput, GlobInput, GrepInput, GrepScope,
    ReadInput, TOOL_SCHEMA_VERSION, TextResult, ToolContext, ToolExecutionMetadata, ToolInput,
    ToolInvocation, ToolObservability, ToolOutcome, ToolPolicy, ToolProcessStatus, ToolResult,
    ToolResultEnvelope, WriteInput,
};

#[test]
fn tool_schema_version_is_current() {
    assert_eq!(TOOL_SCHEMA_VERSION, 1);
}

#[test]
fn logical_paths_cover_all_inputs() {
    let path = relative("x.txt");
    let inputs = [
        ToolInput::Read(ReadInput { path: path.clone() }),
        ToolInput::Write(WriteInput {
            path: path.clone(),
            content: text("x"),
            expected_content: None,
        }),
        ToolInput::Edit(EditInput {
            path: path.clone(),
            old: text("x"),
            new: text("y"),
            expected_content: None,
        }),
        ToolInput::Grep(GrepInput {
            pattern: text("x"),
            path: Some(path.clone()),
            scope: None,
        }),
        ToolInput::Glob(GlobInput { pattern: text("*") }),
        ToolInput::Execute(ExecuteInput {
            program: text("true"),
            args: vec![],
        }),
    ];
    assert_eq!(inputs[0].logical_path(), Some(&path));
    assert_eq!(inputs[1].logical_path(), Some(&path));
    assert_eq!(inputs[2].logical_path(), Some(&path));
    assert_eq!(inputs[3].logical_path(), Some(&path));
    assert!(inputs[4].logical_path().is_none() && inputs[5].logical_path().is_none());
}

#[test]
fn exercises_plain_grep_and_envelope_fallback() {
    let (dir, service) = fixture_service();
    std::fs::write(dir.path().join("x.txt"), "é needle\nnope\n").unwrap();
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: text("needle"),
            scope: None,
            path: Some(relative("x.txt")),
        }),
        CancellationSignal::new(),
    );
    let ToolResult::Grep(result) = result else {
        return;
    };
    assert_eq!(result.matches[0].column, 3);
    let envelope = ToolResultEnvelope {
        schema_version: TOOL_SCHEMA_VERSION,
        context: ToolContext {
            session_id: intention_proto::SessionId::new(),
            run_id: intention_proto::RunId::new(),
            call_id: ToolCallId::new(),
        },
        result: ToolResult::Read(TextResult {
            text: text("ok"),
            truncated: false,
        }),
        observability: ToolObservability {
            outcome: ToolOutcome::Succeeded,
            policy: ToolPolicy::Denied,
            elapsed_ms: 7,
        },
        execution: None,
    };
    assert_eq!(envelope.projection().execution.policy, ToolPolicy::Denied);
}

#[test]
fn scoped_search_reports_file_directory_workspace_and_failures() {
    let (dir, service) = fixture_service();
    std::fs::create_dir_all(dir.path().join("nested/deep")).unwrap();
    std::fs::write(dir.path().join("root.txt"), "needle\n").unwrap();
    std::fs::write(dir.path().join("nested/deep/file.txt"), "needle\n").unwrap();
    for scope in [
        GrepScope::File {
            path: relative("root.txt"),
        },
        GrepScope::Directory {
            path: relative("nested"),
        },
        GrepScope::Workspace,
    ] {
        let result = service.dispatch_completed(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("needle"),
                scope: Some(scope),
                path: None,
            }),
            CancellationSignal::new(),
        );
        assert!(matches!(result, ToolResult::Grep(v) if !v.matches.is_empty()));
    }
    for scope in [
        GrepScope::File {
            path: relative("missing"),
        },
        GrepScope::Directory {
            path: relative("missing"),
        },
        GrepScope::File {
            path: relative("nested"),
        },
    ] {
        assert!(
            service
                .dispatch_with_cancellation(
                    ToolCallId::new(),
                    ToolInput::Grep(GrepInput {
                        pattern: text("needle"),
                        scope: Some(scope),
                        path: None,
                    }),
                    CancellationSignal::new()
                )
                .is_err()
        );
    }
    assert_eq!(
        service
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: text("needle"),
                    scope: None,
                    path: None,
                }),
                CancellationSignal::new()
            )
            .unwrap_err()
            .code(),
        "invalid_tool_path"
    );
}

#[test]
fn metadata_builders_and_schema_validation_are_checked() {
    let path = relative("file.txt");
    let metadata = ToolExecutionMetadata::for_workspace(ToolPolicy::Denied, 7)
        .with_path(Some(path.clone()))
        .with_process_status(Some(ToolProcessStatus::NonZero { code: 3 }));
    assert_eq!(metadata.path, Some(path));
    assert_eq!(
        metadata.process_status,
        Some(ToolProcessStatus::NonZero { code: 3 })
    );

    let (_dir, service) = fixture_service();
    let error = service
        .invoke_enveloped(ToolInvocation {
            schema_version: TOOL_SCHEMA_VERSION + 1,
            context: fixture_context(ToolCallId::new()),
            input: ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*").unwrap(),
            }),
        })
        .unwrap_err();
    assert_eq!(error.code(), "tool_schema_mismatch");
}

#[cfg(unix)]
#[test]
fn read_write_edit_follow_symlink_paths() {
    use std::os::unix::fs::symlink;
    let dir = fixture_dir("symlink");
    let target = dir.path().join("target");
    std::fs::write(&target, "old").unwrap_or_else(|_| unreachable!("seed"));
    symlink(&target, dir.path().join("link")).unwrap_or_else(|_| unreachable!("link"));
    let s = service(&dir);
    // A symbolic link is ordinary filesystem material: an explicitly
    // addressed link is followed like any other path.
    let read = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Read(ReadInput {
            path: relative("link"),
        }),
        CancellationSignal::new(),
    );
    assert!(
        matches!(
            read,
            Ok(ToolDispatchOutcome::Completed(ToolResult::Read(value)))
                if value.text.as_str() == "old"
        ),
        "read follows the addressed link"
    );
    let write = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Write(WriteInput {
            path: relative("link"),
            content: text("written"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    assert!(write.is_ok(), "write follows the addressed link");
    let edit = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Edit(EditInput {
            path: relative("link"),
            old: text("written"),
            new: text("edited"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    assert!(edit.is_ok(), "edit follows the addressed link");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap_or_else(|_| unreachable!("target readable")),
        "edited"
    );
}
