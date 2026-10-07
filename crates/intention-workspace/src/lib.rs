//! WorkspaceRoot addressing anchor.
//!
//! This crate owns the workspace hook boundary: the application applies
//! [`WorkspaceRoot`] between the `BeforeWorkspaceResolution` and
//! `AfterWorkspaceResolution` hook phases. The root is an addressing anchor: a
//! relative path addresses the root joined with that path, child processes
//! start in the root, and a pathless search addresses the root. It is not a
//! security boundary — absolute paths and `..` are not contained, and symbolic
//! links are ordinary filesystem material (architecture 05). Hook phase contexts may
//! identify the workspace only through safe identity — the daemon-owned
//! `intention_proto::WorkspaceId` — never through this crate's root path. This
//! crate owns no persistence and no publication.

use std::path::{Path, PathBuf};

use intention_proto::WorkspaceRootDto;
use intention_proto::{DtoResult, ErrorDto, WorkspaceRelativePathDto};

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
    /// canonicalized and not contained (architecture 05).
    #[must_use]
    pub fn resolve_path(&self, path: &WorkspaceRelativePathDto) -> PathBuf {
        self.root.join(path.as_str())
    }

    /// Re-authorizes this anchor and returns the bound root.
    ///
    /// The composition root binds the workspace between the two
    /// workspace-resolution hook phases, so the root one invocation addresses is
    /// authorized at that boundary rather than only when it was first resolved.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error when the root is unavailable or is not a directory.
    pub fn rebind(&self) -> DtoResult<Self> {
        let declared =
            WorkspaceRootDto::parse(self.root.to_string_lossy().into_owned()).map_err(|_| {
                ErrorDto::validation(
                    "workspace_root_unavailable",
                    "workspace root is unavailable",
                )
            })?;
        Self::resolve(&declared)
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
