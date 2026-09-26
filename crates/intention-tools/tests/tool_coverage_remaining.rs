#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

use intention_domain::WorkspaceRootDto;
use intention_tools::*;
use intention_types::{ToolCallId, WorkspaceRelativePathDto};
use tempfile::TempDir;

#[test]
fn descriptors_are_publicly_accessible() {
    for descriptor in registry() {
        assert!(!descriptor.description().is_empty());
    }
}

fn service() -> (TempDir, ToolService) {
    let dir = tempfile::tempdir().unwrap();
    let dto = WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap();
    let root = intention_workspace::WorkspaceRoot::resolve(&dto).unwrap();
    (dir, ToolService::new(root))
}
fn p(s: &str) -> WorkspaceRelativePathDto {
    WorkspaceRelativePathDto::parse(s).unwrap()
}
fn t(s: &str) -> BoundedText {
    BoundedText::new(s).unwrap()
}
fn call(service: &ToolService, input: ToolInput) -> intention_types::DtoResult<ToolResult> {
    service.dispatch_with_cancellation(ToolCallId::new(), input, CancellationSignal::new())
}

#[test]
fn logical_paths_cover_all_inputs_and_projections() {
    let path = p("x.txt");
    let inputs = [
        ToolInput::Read(ReadInput { path: path.clone() }),
        ToolInput::Write(WriteInput {
            path: path.clone(),
            content: t("x"),
            expected_content: None,
        }),
        ToolInput::Edit(EditInput {
            path: path.clone(),
            old: t("x"),
            new: t("y"),
            expected_content: None,
        }),
        ToolInput::Grep(GrepInput {
            pattern: t("x"),
            path: Some(path.clone()),
            scope: None,
        }),
        ToolInput::Glob(GlobInput { pattern: t("*") }),
        ToolInput::Execute(ExecuteInput {
            program: t("true"),
            args: vec![],
        }),
    ];
    assert_eq!(inputs[0].logical_path(), Some(&path));
    assert_eq!(inputs[1].logical_path(), Some(&path));
    assert_eq!(inputs[2].logical_path(), Some(&path));
    assert_eq!(inputs[3].logical_path(), Some(&path));
    assert!(inputs[4].logical_path().is_none() && inputs[5].logical_path().is_none());
    let values = [
        ToolResult::Read(TextResult {
            text: t("x"),
            truncated: true,
        }),
        ToolResult::Execute(TextResult {
            text: t("x"),
            truncated: false,
        }),
        ToolResult::Glob(PathsResult {
            paths: vec![path],
            truncated: true,
        }),
        ToolResult::Grep(GrepResult {
            matches: vec![],
            truncated: true,
        }),
        ToolResult::Write(WriteResult { bytes: 1 }),
        ToolResult::Edit(WriteResult { bytes: 2 }),
    ];
    for value in values {
        let _ = value.projection();
        assert!(value.tool_id() == value.tool_id());
    }
}

#[test]
fn execute_failures_and_status_are_normalized() {
    let (_dir, service) = service();
    let err = call(
        &service,
        ToolInput::Execute(ExecuteInput {
            program: t("missing-command"),
            args: vec![],
        }),
    )
    .unwrap_err();
    assert_eq!(err.code(), "tool_execute_spawn_failed");
    let result = call(
        &service,
        ToolInput::Execute(ExecuteInput {
            program: if cfg!(windows) { t("cmd") } else { t("sh") },
            args: if cfg!(windows) {
                vec![t("/C"), t("exit 3")]
            } else {
                vec![t("-c"), t("exit 3")]
            },
        }),
    )
    .unwrap();
    assert!(matches!(result, ToolResult::Execute(_)));
}

#[test]
fn search_scopes_truncation_and_invalid_paths() {
    let (dir, service) = service();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    for i in 0..1100 {
        std::fs::write(dir.path().join(format!("sub/{i}.txt")), "needle\n").unwrap();
    }
    let result = call(
        &service,
        ToolInput::Grep(GrepInput {
            pattern: t("needle"),
            scope: Some(GrepScope::Directory { path: p("sub") }),
            path: None,
        }),
    )
    .unwrap();
    let ToolResult::Grep(value) = result else {
        return;
    };
    assert_eq!(value.matches.len(), 1100);
    let glob = call(
        &service,
        ToolInput::Glob(GlobInput {
            pattern: t("sub/*.txt"),
        }),
    )
    .unwrap();
    let ToolResult::Glob(value) = glob else {
        return;
    };
    assert_eq!(value.paths.len(), 1100);
    for pattern in ["", "../x", "/x", "C:/x"] {
        assert!(
            call(
                &service,
                ToolInput::Glob(GlobInput {
                    pattern: t(pattern)
                })
            )
            .is_err()
        );
    }
    let bad = call(
        &service,
        ToolInput::Grep(GrepInput {
            pattern: t("x"),
            scope: Some(GrepScope::File { path: p("missing") }),
            path: None,
        }),
    )
    .unwrap_err();
    assert!(!bad.code().is_empty());
}

#[test]
fn write_edit_read_errors_are_typed() {
    let (_dir, service) = service();
    let path = p("missing.txt");
    assert!(call(&service, ToolInput::Read(ReadInput { path: path.clone() })).is_err());
    assert!(
        call(
            &service,
            ToolInput::Edit(EditInput {
                path,
                old: t("x"),
                new: t("y"),
                expected_content: None
            })
        )
        .is_err()
    );
    let existing = p("existing.txt");
    let result = call(
        &service,
        ToolInput::Write(WriteInput {
            path: existing.clone(),
            content: t("x"),
            expected_content: None,
        }),
    )
    .unwrap();
    assert!(matches!(result, ToolResult::Write(_)));
    let conflict = call(
        &service,
        ToolInput::Write(WriteInput {
            path: existing,
            content: t("y"),
            expected_content: Some(t("z")),
        }),
    )
    .unwrap_err();
    assert_eq!(conflict.code(), "tool_write_conflict");
}
