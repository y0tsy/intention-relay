#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

use intention_domain::WorkspaceRootDto;
use intention_tools::*;
use intention_types::{RunId, SessionId, ToolCallId, WorkspaceRelativePathDto};
use tempfile::TempDir;

fn service() -> (TempDir, ToolService) {
    let dir = tempfile::tempdir().unwrap();
    let dto = WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap();
    let root = intention_workspace::WorkspaceRoot::resolve(&dto).unwrap();
    (dir, ToolService::new(root))
}
fn path(value: &str) -> WorkspaceRelativePathDto {
    WorkspaceRelativePathDto::parse(value).unwrap()
}
fn text(value: &str) -> BoundedText {
    BoundedText::new(value).unwrap()
}
fn context() -> ToolContext {
    ToolContext {
        session_id: SessionId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
        run_id: RunId::parse("00000000-0000-4000-8000-000000000002").unwrap(),
        call_id: ToolCallId::new(),
    }
}

#[test]
fn grep_scopes_cover_file_directory_workspace_and_missing_scope() {
    let (dir, service) = service();
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("root.txt"), "needle\n").unwrap();
    std::fs::write(dir.path().join("nested/deep.txt"), "needle\n").unwrap();
    for scope in [
        GrepScope::File {
            path: path("root.txt"),
        },
        GrepScope::Directory {
            path: path("nested"),
        },
        GrepScope::Workspace,
    ] {
        let result = service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: text("needle"),
                    scope: Some(scope),
                    path: None,
                }),
            )
            .unwrap();
        assert!(matches!(result, ToolResult::Grep(value) if !value.matches.is_empty()));
    }
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("needle"),
                scope: None,
                path: None,
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "invalid_tool_path");
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("needle"),
                scope: Some(GrepScope::File {
                    path: path("missing"),
                }),
                path: None,
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "workspace_path_unavailable");
}

#[test]
fn directory_scope_recurses_and_rejects_non_files() {
    let (dir, service) = service();
    std::fs::create_dir(dir.path().join("a")).unwrap();
    std::fs::create_dir(dir.path().join("a/b")).unwrap();
    std::fs::write(dir.path().join("a/b/file"), "x needle").unwrap();
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("needle"),
                scope: Some(GrepScope::Directory { path: path("a") }),
                path: None,
            }),
        )
        .unwrap();
    assert_eq!(
        match result {
            ToolResult::Grep(v) => v.matches.len(),
            _ => 0,
        },
        1
    );
    let error = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("needle"),
                scope: Some(GrepScope::File { path: path("a") }),
                path: None,
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "tool_search_failed");
}

#[test]
fn all_result_projections_and_metadata_fallbacks_are_typed() {
    let results = [
        ToolResult::Read(TextResult { text: text("r") }),
        ToolResult::Execute(TextResult { text: text("e") }),
        ToolResult::Glob(PathsResult {
            paths: vec![path("a")],
        }),
        ToolResult::Grep(GrepResult { matches: vec![] }),
        ToolResult::Write(WriteResult { bytes: 2 }),
        ToolResult::Edit(WriteResult { bytes: 3 }),
    ];
    for result in results {
        let projection = result.projection();
        assert_eq!(projection.schema_version, TOOL_SCHEMA_VERSION);
        assert_eq!(projection.tool, result.tool_id());
        assert_eq!(projection.execution.cwd, REDACTED_WORKSPACE_CWD);
    }
    let envelope = ToolResultEnvelope {
        schema_version: 7,
        context: context(),
        result: ToolResult::Write(WriteResult { bytes: 1 }),
        observability: ToolObservability {
            outcome: ToolOutcome::Failed,
            policy: ToolPolicy::Denied,
            elapsed_ms: 9,
        },
        execution: None,
    };
    let projection = envelope.projection();
    assert_eq!(projection.schema_version, 7);
    assert_eq!(projection.execution.policy, ToolPolicy::Denied);
    assert_eq!(projection.execution.elapsed_ms, 9);
}

#[test]
fn descriptors_reserved_paths_and_invocation_schema_ids_are_checked() {
    for descriptor in registry() {
        if descriptor.status() == ToolRegistrationStatus::Reserved {
            assert!(descriptor.input_schema().is_none());
            assert!(descriptor.output_schema().is_none());
            assert_eq!(descriptor.schema_version(), 0);
        }
    }
    let id = ToolCallId::new();
    let invocation = ToolInvocation {
        schema_version: TOOL_SCHEMA_VERSION,
        context: ToolContext {
            call_id: id,
            ..context()
        },
        input: ToolInput::Glob(GlobInput { pattern: text("*") }),
    };
    assert!(invocation.validate_schema_version().is_ok());
    assert!(invocation.validate_call_id(id).is_ok());
    assert_eq!(
        invocation
            .validate_call_id(ToolCallId::new())
            .unwrap_err()
            .code(),
        "tool_call_id_mismatch"
    );
    let bad = ToolInvocation {
        schema_version: TOOL_SCHEMA_VERSION + 1,
        ..invocation
    };
    assert_eq!(
        bad.validate_schema_version().unwrap_err().code(),
        "tool_schema_mismatch"
    );
}

#[test]
fn write_and_edit_conflicts_leave_content_unchanged() {
    let (dir, service) = service();
    std::fs::write(dir.path().join("f"), "original").unwrap();
    let p = path("f");
    let write = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Write(WriteInput {
                path: p.clone(),
                content: text("new"),
                expected_content: Some(text("stale")),
            }),
        )
        .unwrap_err();
    assert_eq!(write.code(), "tool_write_conflict");
    let edit = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Edit(EditInput {
                path: p,
                old: text("original"),
                new: text("new"),
                expected_content: Some(text("stale")),
            }),
        )
        .unwrap_err();
    assert_eq!(edit.code(), "tool_edit_conflict");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f")).unwrap(),
        "original"
    );
}
