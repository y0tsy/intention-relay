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
fn read_write_edit_follow_symlink_paths() {
    use intention_tools::{EditInput, ReadInput, WriteInput};
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let target = dir.path().join("target");
    std::fs::write(&target, "old").unwrap_or_else(|_| unreachable!("seed"));
    symlink(&target, dir.path().join("link")).unwrap_or_else(|_| unreachable!("link"));
    let s = service(&dir);
    // A symbolic link is ordinary filesystem material: an explicitly
    // addressed link is followed like any other path.
    let read = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Read(ReadInput { path: path("link") }),
        CancellationSignal::new(),
    );
    assert!(
        matches!(read, Ok(ToolResult::Read(value)) if value.text.as_str() == "old"),
        "read follows the addressed link"
    );
    let write = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Write(WriteInput {
            path: path("link"),
            content: text("written"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    assert!(write.is_ok(), "write follows the addressed link");
    let edit = s.dispatch_with_cancellation(
        ToolCallId::new(),
        ToolInput::Edit(EditInput {
            path: path("link"),
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

#[test]
fn direct_grep_rejects_missing_and_directory_paths() {
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
        assert_eq!(
            result.as_ref().err().map(|e| e.code()),
            Some("tool_search_failed")
        );
    }
}

#[test]
fn glob_accepts_patterns_and_rejects_traversal() {
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
