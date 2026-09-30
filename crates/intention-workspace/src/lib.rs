//! WorkspaceRoot addressing anchor.
//!
//! This crate owns the workspace hook boundary: the application applies
//! [`WorkspaceRoot`] between the `BeforeWorkspaceResolution` and
//! `AfterWorkspaceResolution` hook phases. The root is an addressing anchor: a
//! relative path addresses the root joined with that path, child processes
//! start in the root, and a pathless search addresses the root. It is not a
//! security boundary — absolute paths and `..` are not contained, and symbolic
//! links are ordinary filesystem material (ADR 0047). Hook phase contexts may
//! identify the workspace only through safe identity — the daemon-owned
//! `intention_types::WorkspaceId` — never through this crate's root path. This
//! crate owns no persistence and no publication.

use std::path::{Path, PathBuf};

use intention_domain::WorkspaceRootDto;
use intention_types::{DtoResult, ErrorDto, WorkspaceRelativePathDto};

/// A workspace root used as an addressing anchor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRoot {
    root: PathBuf,
}

impl WorkspaceRoot {
    /// Resolves the declared root without consulting process CWD.
    ///
    /// The root is canonicalized once at construction so it stays a stable
    /// absolute anchor even if the process CWD changes later; that is an
    /// addressing guarantee, not a containment check.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error when the root is unavailable or is not a directory.
    pub fn resolve(dto: &WorkspaceRootDto) -> DtoResult<Self> {
        let root = std::fs::canonicalize(Path::new(dto.as_str())).map_err(|_| {
            ErrorDto::validation(
                "workspace_root_unavailable",
                "workspace root is unavailable",
            )
        })?;
        if !root.is_dir() {
            return Err(ErrorDto::validation(
                "workspace_root_not_directory",
                "workspace root is not a directory",
            ));
        }
        Ok(Self { root })
    }

    /// Addresses a logical relative path under the root.
    ///
    /// This is exactly `root.join(path)`: the path is addressed as given, not
    /// canonicalized and not contained (ADR 0047).
    #[must_use]
    pub fn resolve_path(&self, path: &WorkspaceRelativePathDto) -> PathBuf {
        self.root.join(path.as_str())
    }

    /// Addresses a new file under the root with the same join rule as
    /// [`Self::resolve_path`].
    #[must_use]
    pub fn resolve_new_file_path(&self, path: &WorkspaceRelativePathDto) -> PathBuf {
        self.root.join(path.as_str())
    }

    /// Prepares an execute working directory, explicitly independent of CWD.
    #[must_use]
    pub fn execute_cwd(&self) -> &Path {
        &self.root
    }

    /// Returns the root anchor for adapters that need an observation.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Test fixtures use expect for setup failures."
)]
mod tests {
    use super::*;
    use intention_domain::WorkspaceRootDto;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_ROOT: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "intention-workspace-{}-{}",
            std::process::id(),
            NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary workspace");
        root
    }

    fn workspace(root: &Path) -> WorkspaceRoot {
        WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("workspace root"),
        )
        .expect("workspace resolves")
    }

    #[test]
    fn addresses_relative_paths_by_joining_the_root() {
        let root = temp_root();
        fs::write(root.join("file.txt"), "x").expect("write temporary file");
        let ws = workspace(&root);
        assert_eq!(
            ws.resolve_path(&WorkspaceRelativePathDto::parse("file.txt").expect("path")),
            ws.root().join("file.txt")
        );
        assert_eq!(
            ws.resolve_new_file_path(
                &WorkspaceRelativePathDto::parse("dir/new.txt").expect("path")
            ),
            ws.root().join("dir").join("new.txt")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_paths_are_addressed_as_joined_paths() {
        let root = temp_root();
        let ws = workspace(&root);
        let missing = WorkspaceRelativePathDto::parse("missing/leaf.txt").expect("path");
        assert_eq!(
            ws.resolve_path(&missing),
            ws.root().join("missing/leaf.txt")
        );
        assert_eq!(
            ws.resolve_new_file_path(&missing),
            ws.root().join("missing/leaf.txt")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unnormalized_or_absolute_input_is_rejected_by_the_dto() {
        // WorkspaceRelativePathDto owns input validation before addressing; the
        // anchor itself only joins (ADR 0047).
        let root = temp_root();
        let ws = workspace(&root);
        assert_eq!(
            WorkspaceRelativePathDto::parse("../sibling.txt")
                .expect_err("parent input is not a logical relative path")
                .code(),
            "invalid_workspace_relative_path"
        );
        let absolute = std::env::temp_dir().join("intention-workspace-outside.txt");
        assert_eq!(
            WorkspaceRelativePathDto::parse(absolute.to_string_lossy().into_owned())
                .expect_err("absolute input is not a logical relative path")
                .code(),
            "invalid_workspace_relative_path"
        );
        // An absolute path joined to the anchor stays absolute: the join
        // contains nothing.
        assert_eq!(ws.root().join(&absolute), absolute);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn root_is_a_stable_anchor_when_process_cwd_changes() {
        let root = temp_root();
        let other = temp_root();
        let ws = workspace(&root);
        assert_eq!(ws.root(), fs::canonicalize(&root).expect("canonical root"));
        assert_eq!(ws.execute_cwd(), ws.root());
        let original = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&other).expect("change cwd");
        assert_eq!(
            ws.resolve_path(&WorkspaceRelativePathDto::parse("file.txt").expect("path")),
            ws.root().join("file.txt")
        );
        assert_eq!(ws.execute_cwd(), ws.root());
        std::env::set_current_dir(original).expect("restore cwd");
        let _ = fs::remove_dir_all(other);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unavailable_and_non_directory_roots_with_safe_errors() {
        let missing = std::env::temp_dir().join(format!(
            "intention-workspace-missing-{}",
            std::process::id()
        ));
        let missing_dto =
            WorkspaceRootDto::parse(missing.to_string_lossy().into_owned()).expect("root");
        assert_eq!(
            WorkspaceRoot::resolve(&missing_dto)
                .expect_err("missing root")
                .code(),
            "workspace_root_unavailable"
        );

        let file =
            std::env::temp_dir().join(format!("intention-workspace-file-{}", std::process::id()));
        fs::write(&file, "not a directory").expect("file root");
        let file_dto = WorkspaceRootDto::parse(file.to_string_lossy().into_owned()).expect("root");
        assert_eq!(
            WorkspaceRoot::resolve(&file_dto)
                .expect_err("file root")
                .code(),
            "workspace_root_not_directory"
        );
        let _ = fs::remove_file(file);
    }
}
