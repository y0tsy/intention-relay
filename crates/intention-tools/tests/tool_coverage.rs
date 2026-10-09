//! Consolidated coverage cases: search scopes and symlink handling, merged
//! from the six former padded coverage targets.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

mod common;

use common::{DispatchCompleted, fixture_service, relative, text};
#[cfg(unix)]
use common::{fixture_dir, service};
#[cfg(unix)]
use intention_tools::ToolDispatchOutcome;
use intention_tools::{CancellationSignal, GrepInput, GrepScope, ToolInput, ToolResult};
#[cfg(unix)]
use intention_tools::{EditInput, ReadInput, WriteInput};

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
        ToolInput::Write(WriteInput {
            path: relative("link"),
            content: text("written"),
            expected_content: None,
        }),
        CancellationSignal::new(),
    );
    assert!(write.is_ok(), "write follows the addressed link");
    let edit = s.dispatch_with_cancellation(
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
