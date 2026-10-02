#![cfg(unix)]

//! The remaining contract in this file addresses a symbolic link, which the
//! Windows test environment cannot create without elevation. The whole file
//! is Unix-only so the declared test target stays valid on both platforms
//! instead of leaving its fixture helpers unused under `-D warnings`.

use intention_domain::WorkspaceRootDto;
use intention_tools::{BoundedText, CancellationSignal, ToolInput, ToolResult, ToolService};
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
