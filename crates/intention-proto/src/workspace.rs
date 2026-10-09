//! Validated workspace addressing values shared across boundaries.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{DtoResult, ErrorDto};

/// A typed workspace path declared by the session boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkspaceRootDto(String);

impl<'de> Deserialize<'de> for WorkspaceRootDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl WorkspaceRootDto {
    /// Parses an absolute, non-empty native workspace path without resolving it.
    ///
    /// Resolution and root validation belong to `intention-workspace`, where
    /// the root is an addressing anchor rather than a containment boundary
    /// (architecture 05).
    ///
    /// # Errors
    ///
    /// Returns a validation error if `value` is empty or not absolute.
    pub fn parse(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        if value.trim().is_empty() || !std::path::Path::new(&value).is_absolute() {
            Err(ErrorDto::validation(
                "invalid_workspace_root",
                "workspace root must be a non-empty absolute native path",
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the declared workspace path without attempting filesystem access.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for WorkspaceRootDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
