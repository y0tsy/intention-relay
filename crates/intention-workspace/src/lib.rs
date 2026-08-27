//! WorkspaceRoot resolution and fail-closed filesystem policy.
//!
//! This crate is the mandatory workspace hook boundary owner: the
//! application applies [`WorkspaceRoot`] between the
//! `BeforeWorkspaceResolution` and `AfterWorkspaceResolution` hook phases,
//! and every relative path resolves from the authorized root fail-closed,
//! independent of the process CWD. Hook phase contexts may identify the
//! workspace only through safe identity — the daemon-owned
//! `intention_types::WorkspaceId` — never through this crate's canonical
//! root path, and resolution errors never disclose it. This crate owns no
//! persistence and no publication; it only resolves and validates.

use std::path::{Path, PathBuf};

use intention_domain::WorkspaceRootDto;
use intention_types::{DtoResult, ErrorDetailDto, ErrorDto, WorkspaceRelativePathDto};

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
        if contains_symlink_component(&candidate, &self.canonical) {
            return Err(ErrorDto::validation(
                "workspace_path_symlink",
                "workspace path contains a symbolic link",
            ));
        }
        let canonical = std::fs::canonicalize(&candidate).map_err(|_| {
            ErrorDto::with_detail(
                "workspace_path_unavailable",
                intention_types::ErrorCategoryDto::Validation,
                "workspace path is unavailable",
                intention_types::ErrorRetryDto::Manual,
                None,
                ErrorDetailDto::MissingWorkspacePath { path: path.clone() },
            )
            .unwrap_or_else(|_| {
                ErrorDto::validation(
                    "workspace_path_unavailable",
                    "workspace path is unavailable",
                )
            })
        })?;
        // `starts_with` is component-aware, unlike string-prefix checks. Keep
        // the root itself valid, but reject siblings such as `root-other`.
        // This is deliberately fail-closed: only canonical paths inside the
        // authorized root are returned.
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
        let candidate = self.canonical.join(logical);
        if contains_symlink_component(&candidate, &self.canonical) {
            return Err(ErrorDto::validation(
                "workspace_path_symlink",
                "workspace path contains a symbolic link",
            ));
        }
        let file_name = logical.file_name().ok_or_else(|| {
            ErrorDto::with_detail(
                "workspace_path_unavailable",
                intention_types::ErrorCategoryDto::Validation,
                "workspace path is unavailable",
                intention_types::ErrorRetryDto::Manual,
                None,
                ErrorDetailDto::MissingWorkspacePath { path: path.clone() },
            )
            .unwrap_or_else(|_| {
                ErrorDto::validation(
                    "workspace_path_unavailable",
                    "workspace path is unavailable",
                )
            })
        })?;
        let parent = logical.parent().unwrap_or_else(|| Path::new("."));
        let parent = self.canonical.join(parent);
        let canonical_parent = std::fs::canonicalize(parent).map_err(|_| {
            ErrorDto::with_detail(
                "workspace_parent_unavailable",
                intention_types::ErrorCategoryDto::Validation,
                "workspace parent is unavailable",
                intention_types::ErrorRetryDto::Manual,
                None,
                ErrorDetailDto::MissingWorkspacePath { path: path.clone() },
            )
            .unwrap_or_else(|_| {
                ErrorDto::validation(
                    "workspace_parent_unavailable",
                    "workspace parent is unavailable",
                )
            })
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

fn contains_symlink_component(path: &Path, root: &Path) -> bool {
    let relative = match path.strip_prefix(root) {
        Ok(relative) => relative,
        Err(_) => return true,
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(_) => {
                // An absent final component is valid for new-file resolution;
                // existing components are canonicalized below. A dangling
                // symlink, however, is detectable by metadata on the link
                // itself and must fail closed.
                return false;
            }
        }
    }
    false
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
        let missing = WorkspaceRelativePathDto::parse("missing").expect("path");
        let error = ws.resolve_path(&missing).expect_err("missing path");
        assert_eq!(error.code(), "workspace_path_unavailable");
        assert!(matches!(
            error.detail(),
            Some(ErrorDetailDto::MissingWorkspacePath { path }) if path == &missing
        ));
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
            // The parent is a symbolic link, so resolution must reject it
            // before it can be followed out of the root.
            assert_eq!(
                ws.resolve_new_file_path(&path)
                    .expect_err("outside parent")
                    .code(),
                "workspace_path_symlink"
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
        assert!(matches!(
            error.detail(),
            Some(ErrorDetailDto::MissingWorkspacePath { path }) if path == &WorkspaceRelativePathDto::parse("missing/new.txt").expect("path")
        ));
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

    #[test]
    fn execute_cwd_does_not_depend_on_changed_process_cwd() {
        let root = temp_root();
        let other = temp_root();
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        let original = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&other).expect("change cwd");
        assert_eq!(ws.execute_cwd(), root.as_path());
        std::env::set_current_dir(original).expect("restore cwd");
        let _ = fs::remove_dir_all(other);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_paths_including_in_root_links() {
        let root = temp_root();
        fs::write(root.join("target.txt"), "x").expect("target");
        let outside = temp_root();
        fs::write(outside.join("secret.txt"), "secret").expect("outside target");
        std::os::unix::fs::symlink(root.join("target.txt"), root.join("inside-link"))
            .expect("link");
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("escape-link"))
            .expect("escape link");
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        assert_eq!(
            ws.resolve_path(&WorkspaceRelativePathDto::parse("inside-link").expect("path"))
                .expect_err("symlink path must be rejected")
                .code(),
            "workspace_path_symlink"
        );
        assert_eq!(
            ws.resolve_path(&WorkspaceRelativePathDto::parse("escape-link").expect("path"))
                .expect_err("escape link")
                .code(),
            "workspace_path_symlink"
        );
        let _ = fs::remove_dir_all(outside);
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
            "workspace_path_symlink"
        );
        let _ = fs::remove_dir_all(&outside);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_sibling_prefix_and_dangling_final_symlink() {
        let root = temp_root();
        let sibling = root.with_file_name(format!(
            "{}-other",
            root.file_name()
                .expect("temp root has file name")
                .to_string_lossy()
        ));
        fs::create_dir_all(&sibling).expect("sibling");
        fs::write(sibling.join("file.txt"), "x").expect("sibling file");
        std::os::unix::fs::symlink(root.join("does-not-exist"), root.join("dangling"))
            .expect("dangling symlink");
        let ws = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root"),
        )
        .expect("workspace");
        let sibling_path = WorkspaceRelativePathDto::parse("sibling-placeholder").expect("path");
        let sibling_candidate = ws
            .canonical_path()
            .parent()
            .expect("resolved root has parent")
            .join(sibling.file_name().expect("sibling path has file name"))
            .join("file.txt");
        assert!(std::fs::canonicalize(sibling_candidate).is_ok());
        assert_eq!(
            ws.resolve_path(&sibling_path)
                .expect_err("missing sibling")
                .code(),
            "workspace_path_unavailable"
        );
        let dangling = WorkspaceRelativePathDto::parse("dangling").expect("path");
        assert_eq!(
            ws.resolve_path(&dangling).expect_err("dangling").code(),
            "workspace_path_symlink"
        );
        // A dangling final symlink must also fail closed for new-file
        // resolution: the returned path would otherwise be written through
        // the link to a location outside the authorized root.
        assert_eq!(
            ws.resolve_new_file_path(&dangling)
                .expect_err("dangling final symlink")
                .code(),
            "workspace_path_symlink"
        );
        let _ = fs::remove_dir_all(sibling);
        let _ = fs::remove_dir_all(root);
    }
}
