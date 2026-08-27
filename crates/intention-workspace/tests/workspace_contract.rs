#![allow(
    clippy::expect_used,
    reason = "Contract test setup failures are reported with local context."
)]
#![allow(
    clippy::unwrap_used,
    reason = "Test setup failures are reported with local context."
)]

use intention_domain::WorkspaceRootDto;
use intention_types::WorkspaceRelativePathDto;
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
fn relative_resolution_does_not_depend_on_process_cwd() {
    let root = TempDir::new("contract");
    std::fs::write(root.path().join("file.txt"), "ok").expect("file");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.path().to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace");
    let path = WorkspaceRelativePathDto::parse("file.txt").expect("path");
    let _guard = cwd_guard();
    let _cwd = CwdGuard::change_to(&std::env::temp_dir());
    assert!(workspace.resolve_path(&path).is_ok());
}

#[test]
fn missing_path_is_safe_and_cwd_changes_do_not_escape_root() {
    let root = std::env::temp_dir().join(format!("intention-workspace-m5-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("root");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy()).expect("dto"),
    )
    .expect("workspace");
    let missing = WorkspaceRelativePathDto::parse("missing.txt").expect("path");
    let error = workspace.resolve_path(&missing).expect_err("missing path");
    assert_eq!(error.code(), "workspace_path_unavailable");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn traversal_and_absolute_outside_paths_are_rejected() {
    let root = std::env::temp_dir().join(format!(
        "intention-workspace-contract-boundary-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "intention-workspace-contract-outside-file-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(&outside, "outside").expect("outside file");
    assert!(WorkspaceRelativePathDto::parse("../outside-file").is_err());
    assert!(WorkspaceRelativePathDto::parse(outside.to_string_lossy()).is_err());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_file(outside);
}

#[test]
fn execute_cwd_is_explicit_workspace_root() {
    let root = std::env::temp_dir().join(format!(
        "intention-workspace-contract-cwd-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary workspace");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace");
    assert_eq!(workspace.execute_cwd(), workspace.canonical_path());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn symlink_to_outside_is_rejected() {
    let root = std::env::temp_dir().join(format!(
        "intention-workspace-contract-link-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "intention-workspace-contract-outside-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&root).expect("root");
    std::fs::create_dir_all(&outside).expect("outside");
    std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");
    let workspace = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
    )
    .expect("workspace");
    let path = WorkspaceRelativePathDto::parse("link").expect("path");
    assert!(workspace.resolve_path(&path).is_err());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}
