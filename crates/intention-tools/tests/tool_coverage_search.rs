#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

use intention_domain::WorkspaceRootDto;
use intention_tools::*;
use intention_types::ToolCallId;
use tempfile::TempDir;

fn service() -> (TempDir, ToolService) {
    let dir = tempfile::tempdir().unwrap();
    let dto = WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap();
    let root = intention_workspace::WorkspaceRoot::resolve(&dto).unwrap();
    (dir, ToolService::new(root))
}

fn path(value: &str) -> intention_types::WorkspaceRelativePathDto {
    intention_types::WorkspaceRelativePathDto::parse(value).unwrap()
}

fn text(value: &str) -> BoundedText {
    BoundedText::new(value).unwrap()
}

#[test]
fn scoped_search_reports_file_directory_workspace_and_failures() {
    let (dir, service) = service();
    std::fs::create_dir_all(dir.path().join("nested/deep")).unwrap();
    std::fs::write(dir.path().join("root.txt"), "needle\n").unwrap();
    std::fs::write(dir.path().join("nested/deep/file.txt"), "needle\n").unwrap();
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
            .dispatch_with_cancellation(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: text("needle"),
                    scope: Some(scope),
                    path: None,
                }),
                CancellationSignal::new(),
            )
            .unwrap();
        assert!(matches!(result, ToolResult::Grep(v) if !v.matches.is_empty()));
    }
    for scope in [
        GrepScope::File {
            path: path("missing"),
        },
        GrepScope::Directory {
            path: path("missing"),
        },
        GrepScope::File {
            path: path("nested"),
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
