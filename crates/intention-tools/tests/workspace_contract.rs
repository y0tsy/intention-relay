#![allow(
    clippy::expect_used,
    reason = "Contract test setup failures are reported with local context."
)]
#![allow(
    clippy::unwrap_used,
    reason = "Test setup failures are reported with local context."
)]

mod common;

use intention_proto::WorkspaceRelativePathDto;
use intention_proto::WorkspaceRootDto;
use std::sync::{Mutex, MutexGuard, OnceLock};

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "intention-workspace-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        std::fs::create_dir(&path).expect("temporary workspace");
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

struct CwdGuard(std::path::PathBuf);
impl CwdGuard {
    fn change_to(path: &std::path::Path) -> Self {
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(path).expect("change cwd");
        Self(old)
    }
}
impl Drop for CwdGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore cwd");
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn cwd_guard() -> MutexGuard<'static, ()> {
    CWD_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn relative_resolution_joins_the_declared_root_independent_of_process_cwd() {
    let root = TempDir::new("contract");
    std::fs::write(root.path().join("file.txt"), "ok").expect("file");
    let workspace = common::workspace(root.path());
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    let expected = std::fs::canonicalize(root.path().join("file.txt")).expect("canonical file");
    let _guard = cwd_guard();
    let _cwd = CwdGuard::change_to(&std::env::temp_dir());
    assert_eq!(workspace.resolve_path(&path), expected);
    assert_eq!(workspace.execute_cwd(), workspace.root());
    assert_eq!(
        workspace.execute_cwd(),
        std::fs::canonicalize(root.path()).expect("canonical root")
    );
}

#[test]
fn unavailable_and_non_directory_roots_fail_safely() {
    let missing = std::env::temp_dir().join(format!(
        "intention-workspace-missing-{}",
        std::process::id()
    ));
    let missing_dto =
        WorkspaceRootDto::parse(missing.to_string_lossy().into_owned()).expect("root");
    let error = intention_tools::WorkspaceRoot::resolve(&missing_dto).expect_err("missing root");
    assert_eq!(error.code(), "workspace_root_unavailable");

    let file =
        std::env::temp_dir().join(format!("intention-workspace-file-{}", std::process::id()));
    std::fs::write(&file, "not a directory").expect("file root");
    let file_dto = WorkspaceRootDto::parse(file.to_string_lossy().into_owned()).expect("root");
    let error = intention_tools::WorkspaceRoot::resolve(&file_dto).expect_err("file root");
    assert_eq!(error.code(), "workspace_root_not_directory");
    let _ = std::fs::remove_file(file);
}
