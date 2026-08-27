//! WorkspaceRoot resolution and fail-closed filesystem policy.

use std::path::{Path, PathBuf};

use intention_domain::WorkspaceRootDto;
use intention_types::{DtoResult, ErrorDto, WorkspaceRelativePathDto};

/// An authorized, canonical workspace root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRoot {
    canonical: PathBuf,
}

impl WorkspaceRoot {
    /// Resolves the declared root without consulting process CWD.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error when the root is unavailable or is not a directory.
    pub fn resolve(dto: &WorkspaceRootDto) -> DtoResult<Self> {
        let canonical = std::fs::canonicalize(Path::new(dto.as_str())).map_err(|_| {
            ErrorDto::validation(
                "workspace_root_unavailable",
                "workspace root is unavailable",
            )
        })?;
        if !canonical.is_dir() {
            return Err(ErrorDto::validation(
                "workspace_root_not_directory",
                "workspace root is not a directory",
            ));
        }
        Ok(Self { canonical })
    }

    /// Resolves a logical relative path and verifies canonical containment.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error when the path is unavailable or outside the root.
    pub fn resolve_path(&self, path: &WorkspaceRelativePathDto) -> DtoResult<PathBuf> {
        let candidate = self.canonical.join(path.as_str());
        let canonical = std::fs::canonicalize(&candidate).map_err(|_| {
            ErrorDto::validation(
                "workspace_path_unavailable",
                "workspace path is unavailable",
            )
        })?;
        // `starts_with` is component-aware, unlike string-prefix checks.  Keep
        // the root itself valid, but reject siblings such as `root-other`.
        if canonical == self.canonical || canonical.starts_with(&self.canonical) {
            Ok(canonical)
        } else {
            Err(ErrorDto::validation(
                "workspace_path_outside_root",
                "workspace path is outside the workspace root",
            ))
        }
    }

    /// Resolves a path while retaining the logical path in a safe DTO boundary.
    ///
    /// # Errors
    ///
    /// Returns the same safe validation errors as [`Self::resolve_path`].
    pub fn resolve_path_for_tool(&self, path: &WorkspaceRelativePathDto) -> DtoResult<PathBuf> {
        self.resolve_path(path)
    }

    /// Resolves the parent of a new file, preserving the final missing component.
    /// Every existing component is canonicalized, so symlink traversal fails closed.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error when the parent is missing or outside the workspace.
    pub fn resolve_new_file_path(&self, path: &WorkspaceRelativePathDto) -> DtoResult<PathBuf> {
        let logical = Path::new(path.as_str());
        let file_name = logical.file_name().ok_or_else(|| {
            ErrorDto::validation(
                "workspace_path_unavailable",
                "workspace path is unavailable",
            )
        })?;
        let parent = logical.parent().unwrap_or_else(|| Path::new("."));
        let parent = self.canonical.join(parent);
        let canonical_parent = std::fs::canonicalize(parent).map_err(|_| {
            ErrorDto::validation(
                "workspace_parent_unavailable",
                "workspace parent is unavailable",
            )
        })?;
        if canonical_parent != self.canonical && !canonical_parent.starts_with(&self.canonical) {
            return Err(ErrorDto::validation(
                "workspace_path_outside_root",
                "workspace path is outside the workspace root",
            ));
        }
        Ok(canonical_parent.join(file_name))
    }

    /// Prepares an execute working directory, explicitly independent of CWD.
    #[must_use]
    pub fn execute_cwd(&self) -> &Path {
        &self.canonical
    }

    /// Returns the canonical root for adapters that need an observation.
    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical
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

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "intention-workspace-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary workspace");
        root
    }
    #[test]
    fn resolves_relative_file_and_is_cwd_independent() {
        let root = temp_root();
        fs::write(root.join("file.txt"), "x").expect("write temporary file");
        let dto =
            WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("workspace root");
        let ws = WorkspaceRoot::resolve(&dto).expect("workspace");
        let p = WorkspaceRelativePathDto::parse("file.txt").expect("path");
        assert_eq!(
            ws.resolve_path(&p).expect("resolved path"),
            fs::canonicalize(root.join("file.txt")).expect("canonical file")
        );
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn rejects_missing_and_outside_paths() {
        let root = temp_root();
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        assert!(
            ws.resolve_path(&WorkspaceRelativePathDto::parse("missing").expect("path"))
                .is_err()
        );
        let outside = std::env::temp_dir().join("outside-intention");
        fs::write(&outside, "x").expect("outside file");
        let link = root.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");
        #[cfg(unix)]
        assert!(
            ws.resolve_path(&WorkspaceRelativePathDto::parse("link").expect("link path"))
                .is_err()
        );
        let _ = fs::remove_file(outside);
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

    #[test]
    fn tool_resolution_matches_path_resolution() {
        let root = temp_root();
        fs::write(root.join("tool.txt"), "x").expect("file");
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        let path = WorkspaceRelativePathDto::parse("tool.txt").expect("path");
        assert_eq!(ws.resolve_path_for_tool(&path), ws.resolve_path(&path));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_invalid_new_file_paths_and_outside_parents() {
        let root = temp_root();
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        assert_eq!(
            WorkspaceRelativePathDto::parse(".")
                .expect_err("invalid path")
                .code(),
            "invalid_workspace_relative_path"
        );

        let outside =
            std::env::temp_dir().join(format!("intention-workspace-parent-{}", std::process::id()));
        fs::create_dir_all(&outside).expect("outside parent");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("outside")).expect("symlink");
        #[cfg(unix)]
        {
            let path = WorkspaceRelativePathDto::parse("outside/new.txt").expect("path");
            assert_eq!(
                ws.resolve_new_file_path(&path)
                    .expect_err("outside parent")
                    .code(),
                "workspace_path_outside_root"
            );
        }
        let _ = fs::remove_dir_all(&outside);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_new_file_when_parent_exists() {
        let root = temp_root();
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        let path = WorkspaceRelativePathDto::parse("new.txt").expect("path");
        assert_eq!(
            ws.resolve_new_file_path(&path).expect("new file"),
            root.join("new.txt")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_new_file_with_missing_parent() {
        let root = temp_root();
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        let error = ws
            .resolve_new_file_path(
                &WorkspaceRelativePathDto::parse("missing/new.txt").expect("path"),
            )
            .expect_err("missing parent");
        assert_eq!(error.code(), "workspace_parent_unavailable");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn execute_cwd_is_the_canonical_root() {
        let root = temp_root();
        let dto = WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root");
        let ws = WorkspaceRoot::resolve(&dto).expect("workspace");
        assert_eq!(ws.execute_cwd(), ws.canonical_path());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_to_directory_outside_root() {
        let root = temp_root();
        let outside = std::env::temp_dir().join(format!(
            "intention-workspace-outside-{}",
            std::process::id()
        ));
        fs::create_dir_all(&outside).expect("outside directory");
        std::os::unix::fs::symlink(&outside, root.join("external")).expect("symlink");
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        assert_eq!(
            ws.resolve_path(&WorkspaceRelativePathDto::parse("external").expect("path"))
                .expect_err("outside symlink must be rejected")
                .code(),
            "workspace_path_outside_root"
        );
        let _ = fs::remove_dir_all(&outside);
        let _ = fs::remove_dir_all(root);
    }
}
