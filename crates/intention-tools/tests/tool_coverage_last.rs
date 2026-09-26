use intention_domain::WorkspaceRootDto;
use intention_tools::{
    BoundedText, CancellationSignal, GlobInput, GrepInput, GrepScope, ToolInput, ToolResult,
    ToolService,
};
use intention_types::{ToolCallId, WorkspaceRelativePathDto};
use tempfile::TempDir;

fn service(dir: &TempDir) -> ToolService {
    let dto = WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned())
        .unwrap_or_else(|_| unreachable!("valid fixture root"));
    ToolService::new(
        intention_workspace::WorkspaceRoot::resolve(&dto)
            .unwrap_or_else(|_| unreachable!("resolvable fixture root")),
    )
}
fn path(value: &str) -> WorkspaceRelativePathDto {
    WorkspaceRelativePathDto::parse(value).unwrap_or_else(|_| unreachable!("valid relative path"))
}
fn text(value: &str) -> BoundedText {
    BoundedText::new(value).unwrap_or_else(|_| unreachable!("valid bounded text"))
}
fn fixture() -> TempDir {
    tempfile::tempdir().unwrap_or_else(|_| unreachable!("temporary directory"))
}

#[cfg(unix)]
#[test]
fn rejects_read_write_edit_symlinks() {
    use intention_tools::{EditInput, ReadInput, WriteInput};
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let target = dir.path().join("target");
    std::fs::write(&target, "old").unwrap_or_else(|_| unreachable!("seed"));
    symlink(&target, dir.path().join("link")).unwrap_or_else(|_| unreachable!("link"));
    let s = service(&dir);
    let read = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Read(ReadInput { path: path("link") }),
        CancellationSignal::new(),
    );
    assert!(read.is_err());
    let write = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Write(WriteInput {
            path: path("link"),
            content: text("x"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    assert_eq!(
        write.as_ref().err().map(|e| e.code()),
        Some("workspace_path_symlink")
    );
    let edit = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Edit(EditInput {
            path: path("link"),
            old: text("old"),
            new: text("new"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    assert_eq!(
        edit.as_ref().err().map(|e| e.code()),
        Some("workspace_path_symlink")
    );
}

#[test]
fn direct_grep_rejects_missing_and_directory() {
    let dir = fixture();
    let s = service(&dir);
    for name in ["missing", "ok.txt"] {
        if name == "ok.txt" {
            std::fs::create_dir_all(dir.path().join(name))
                .unwrap_or_else(|_| unreachable!("directory"));
        }
        let result = s.dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("x"),
                scope: None,
                path: Some(path(name)),
            }),
            CancellationSignal::new(),
        );
        let expected = if name == "missing" {
            "workspace_path_unavailable"
        } else {
            "tool_search_failed"
        };
        assert_eq!(result.as_ref().err().map(|e| e.code()), Some(expected));
    }
}

#[test]
fn glob_filters_symlink_and_invalid_pattern() {
    let dir = fixture();
    std::fs::write(dir.path().join("ok.txt"), "x").unwrap_or_else(|_| unreachable!("seed"));
    let s = service(&dir);

    let result = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Glob(GlobInput {
            pattern: text("*.txt"),
        }),
        CancellationSignal::new(),
    );
    assert!(matches!(result, Ok(ToolResult::Glob(_))));
    let bad = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Glob(GlobInput {
            pattern: text("../*"),
        }),
        CancellationSignal::new(),
    );
    assert!(bad.is_err());
    let scoped = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: text("x"),
            scope: Some(GrepScope::Workspace),
            path: None,
        }),
        CancellationSignal::new(),
    );
    assert!(scoped.is_ok());
}
