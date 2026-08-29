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
            .dispatch(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: text("needle"),
                    scope: Some(scope),
                    path: None,
                }),
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
                .dispatch(
                    ToolCallId::new(),
                    ToolInput::Grep(GrepInput {
                        pattern: text("needle"),
                        scope: Some(scope),
                        path: None,
                    })
                )
                .is_err()
        );
    }
    assert_eq!(
        service
            .dispatch(
                ToolCallId::new(),
                ToolInput::Grep(GrepInput {
                    pattern: text("needle"),
                    scope: None,
                    path: None,
                })
            )
            .unwrap_err()
            .code(),
        "invalid_tool_path"
    );
}

#[test]
fn scoped_search_handles_invalid_utf8_long_fragments_in_full() {
    let (dir, service) = service();
    let mut bytes = b"needle ".to_vec();
    bytes.extend(std::iter::repeat_n(b'x', 65_600));
    bytes.extend_from_slice(&[0xff, b'\n']);
    std::fs::write(dir.path().join("bad.bin"), bytes).unwrap();
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("needle"),
                scope: Some(GrepScope::File {
                    path: path("bad.bin"),
                }),
                path: None,
            }),
        )
        .unwrap();
    let ToolResult::Grep(result) = result else {
        return;
    };
    assert_eq!(result.matches.len(), 1);
    let fragment = result.matches[0].fragment.as_str();
    assert!(fragment.starts_with("needle "));
    assert!(fragment.len() >= 65_600);

    std::fs::create_dir(dir.path().join("many")).unwrap();
    for i in 0..10_001 {
        std::fs::write(dir.path().join(format!("many/{i}.txt")), "needle\n").unwrap();
    }
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("needle"),
                scope: Some(GrepScope::Directory { path: path("many") }),
                path: None,
            }),
        )
        .unwrap();
    let ToolResult::Grep(result) = result else {
        return;
    };
    assert_eq!(result.matches.len(), 10_001);
}

#[test]
fn glob_skips_filtered_entries_and_lists_every_match() {
    let (dir, service) = service();
    std::fs::create_dir(dir.path().join("real")).unwrap();
    std::fs::write(dir.path().join("real/ok.txt"), "x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("linked")).unwrap();
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: text("**/*.txt"),
            }),
        )
        .unwrap();
    let ToolResult::Glob(result) = result else {
        return;
    };
    assert_eq!(
        result.paths.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        vec!["real/ok.txt"]
    );

    std::fs::create_dir(dir.path().join("many")).unwrap();
    for i in 0..10_001 {
        std::fs::write(dir.path().join(format!("many/{i}.txt")), "x").unwrap();
    }
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: text("many/*.txt"),
            }),
        )
        .unwrap();
    let ToolResult::Glob(result) = result else {
        return;
    };
    assert_eq!(result.paths.len(), 10_001);
}
