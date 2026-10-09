//! TOML configuration parsing, validation, resolution, and safe projection.
//!
//! The [`catalog`] module owns the one live configuration document shape
//! (`schema_version = 1`): a global `[provider]` policy, provider profiles, and
//! optional user-kind declarations. This module owns the shared configuration
//! location, input, policy, and snapshot value types that document path and the
//! daemon composition build on. Raw credentials remain inside this crate's
//! private loading boundary and are never serialized, displayed, or included in
//! errors.

use std::path::Path;

use intention_proto::{ConfigRevisionId, DtoResult, ErrorDto, SchemaVersionDto, TimestampDto};
use serde::{Deserialize, Serialize};

pub mod catalog;

const CURRENT_SCHEMA_MAJOR: u16 = 1;
const CURRENT_SCHEMA_MINOR: u16 = 0;
const DEFAULT_CONTEXT_WINDOW_TOKENS: u64 = 250_000;

/// Requires a schema version exactly equal to the current configuration schema.
///
/// # Errors
///
/// Returns an unavailable error when the schema version differs from the
/// current configuration schema (no same-major tolerance).
fn require_current_schema_version(schema_version: SchemaVersionDto) -> DtoResult<()> {
    if schema_version != SchemaVersionDto::new(CURRENT_SCHEMA_MAJOR, CURRENT_SCHEMA_MINOR) {
        return Err(ErrorDto::unavailable(
            "incompatible_schema_version",
            "schema version must equal the current configuration schema",
        ));
    }
    Ok(())
}

/// Decodes one snapshot schema version and requires it to be the current one.
fn deserialize_current_schema_version<'de, D>(deserializer: D) -> Result<SchemaVersionDto, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let schema_version = SchemaVersionDto::deserialize(deserializer)?;
    require_current_schema_version(schema_version).map_err(serde::de::Error::custom)?;
    Ok(schema_version)
}

/// A validated, absolute configuration path with semantic configuration intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConfigPathDto(String);

impl<'de> Deserialize<'de> for ConfigPathDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

impl ConfigPathDto {
    /// Parses an absolute non-empty path used only for configuration access.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the path is blank or relative.
    pub fn parse(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        if value.trim().is_empty() || !Path::new(&value).is_absolute() {
            Err(ErrorDto::validation(
                "invalid_config_path",
                "configuration path must be non-empty and absolute",
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated absolute path for a local configuration operation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identifies how the configuration file location was selected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSourceDto {
    /// A caller explicitly selected the configuration file location.
    Explicit(ConfigPathDto),
    /// The location came from the operating system's standard config directory.
    PlatformDefault(ConfigPathDto),
}

impl ConfigSourceDto {
    /// Returns the validated configuration location.
    #[must_use]
    pub const fn path(&self) -> &ConfigPathDto {
        match self {
            Self::Explicit(path) | Self::PlatformDefault(path) => path,
        }
    }
}

/// Selects the standard platform config location or a caller-supplied override.
pub struct ConfigPathResolver;

impl ConfigPathResolver {
    /// Resolves an explicit configuration path or the standard platform location.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error if the operating system has no usable config
    /// directory for the current user, or a validation error for an invalid override.
    pub fn resolve(explicit_path: Option<ConfigPathDto>) -> DtoResult<ConfigSourceDto> {
        if let Some(path) = explicit_path {
            return Ok(ConfigSourceDto::Explicit(path));
        }
        let path = platform_config_path().ok_or_else(|| {
            ErrorDto::unavailable(
                "platform_config_directory_unavailable",
                "a platform configuration directory could not be determined",
            )
        })?;
        let path = ConfigPathDto::parse(path)?;
        Ok(ConfigSourceDto::PlatformDefault(path))
    }
}

fn platform_config_path() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|value| Path::new(value).is_absolute())
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .filter(|value| Path::new(value).is_absolute())
                    .map(|home| format!("{home}/.config"))
            });
        config_home.map(|directory| format!("{directory}/intention-relay/config.toml"))
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var("HOME")
            .ok()
            .filter(|value| Path::new(value).is_absolute())
            .map(|home| format!("{home}/Library/Application Support/intention-relay/config.toml"));
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .ok()
            .filter(|value| Path::new(value).is_absolute())
            .map(|directory| format!("{directory}\\\\intention-relay\\\\config.toml"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Raw configuration text for one catalog parse.
///
/// This input intentionally has no `Debug`, serialization, or content accessor
/// because it may contain an open-text credential by explicit product decision.
pub struct RawConfigInputDto {
    text: String,
}

impl RawConfigInputDto {
    /// Creates opaque TOML input for one catalog-document parse.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// An immutable, credential-free configuration selection captured for a future run.
///
/// The snapshot is the safe part of one committed configuration revision that
/// a run actually reads: its revision identity, its capture time, and the
/// effective context-window policy. Provider identity is never read from a
/// snapshot; a run's exact provider comes from its persisted resolved selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSnapshotDto {
    #[serde(deserialize_with = "deserialize_current_schema_version")]
    schema_version: SchemaVersionDto,
    revision_id: ConfigRevisionId,
    captured_at: TimestampDto,
    context_window: ContextWindowPolicyDto,
}

impl ConfigSnapshotDto {
    /// Creates an immutable credential-free configuration snapshot.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error when the snapshot schema version
    /// differs from the current configuration schema.
    pub fn new(
        schema_version: SchemaVersionDto,
        revision_id: ConfigRevisionId,
        captured_at: TimestampDto,
        context_window: ContextWindowPolicyDto,
    ) -> DtoResult<Self> {
        require_current_schema_version(schema_version)?;
        Ok(Self {
            schema_version,
            revision_id,
            captured_at,
            context_window,
        })
    }

    /// Returns the snapshot contract schema version.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the future durable configuration revision identity.
    #[must_use]
    pub const fn revision_id(&self) -> ConfigRevisionId {
        self.revision_id
    }

    /// Returns the snapshot capture time.
    #[must_use]
    pub const fn captured_at(&self) -> TimestampDto {
        self.captured_at
    }

    /// Returns the effective context-window policy this revision commits.
    #[must_use]
    pub const fn context_window(&self) -> &ContextWindowPolicyDto {
        &self.context_window
    }

    /// Verifies the already-redacted snapshot remains suitable for durable storage.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the snapshot schema version differs
    /// from the current configuration schema.
    pub fn validate_for_persistence(&self) -> DtoResult<()> {
        require_current_schema_version(self.schema_version)
    }
}

/// Safe per-run provider execution policy resolved at startup.
///
/// The shared Slice 2 DTO moved to `intention-proto`; this crate re-exports it
/// so every existing `intention_config::ProviderExecutionPolicyDto` path keeps
/// compiling, and keeps applying its own default/range resolution below.
pub use intention_proto::ProviderExecutionPolicyDto;

/// Applies the M4 defaults and range validation to one raw execution policy.
///
/// # Errors
///
/// Returns a safe validation error when the attempt timeout is outside `1..=60`
/// or the attempt budget is outside `1..=2`.
fn resolve_provider_execution_policy(
    raw: Option<RawProviderExecutionPolicyDto>,
) -> DtoResult<ProviderExecutionPolicyDto> {
    let raw = raw.unwrap_or_default();
    ProviderExecutionPolicyDto::new(
        raw.attempt_timeout_seconds.unwrap_or(30),
        raw.max_attempts.unwrap_or(2),
    )
}

/// Safe context-window policy committed with a configuration revision.
///
/// Decoding applies the same positive-window validation as resolution, so a
/// decoded policy can never carry an empty window.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ContextWindowPolicyDto {
    window_tokens: u64,
}

impl<'de> Deserialize<'de> for ContextWindowPolicyDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawContextWindowPolicyDto {
            window_tokens: u64,
        }

        let raw = RawContextWindowPolicyDto::deserialize(deserializer)?;
        Self::from_raw(Some(raw.window_tokens)).map_err(serde::de::Error::custom)
    }
}

impl ContextWindowPolicyDto {
    /// Returns the current default context-window policy.
    #[must_use]
    pub const fn default_policy() -> Self {
        Self {
            window_tokens: DEFAULT_CONTEXT_WINDOW_TOKENS,
        }
    }

    /// Creates a validated context-window policy.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the window is zero.
    pub fn new(window_tokens: u64) -> DtoResult<Self> {
        if window_tokens == 0 {
            return Err(ErrorDto::validation(
                "invalid_provider_context_window_tokens",
                "provider context window tokens must be greater than zero",
            ));
        }
        Ok(Self { window_tokens })
    }

    /// Applies the current default to an optional document window.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the supplied window is zero.
    fn from_raw(window_tokens: Option<u64>) -> DtoResult<Self> {
        Self::new(window_tokens.unwrap_or(DEFAULT_CONTEXT_WINDOW_TOKENS))
    }

    /// Returns the sliding context window size in tokens.
    #[must_use]
    pub const fn window_tokens(self) -> u64 {
        self.window_tokens
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProviderExecutionPolicyDto {
    attempt_timeout_seconds: Option<u8>,
    max_attempts: Option<u8>,
}

#[cfg(unix)]
/// Verifies that an existing configuration file is not group- or world-readable.
///
/// # Errors
///
/// Returns a policy error when the file mode exposes the configuration to other
/// users, or an unavailable error if metadata cannot be inspected.
pub fn ensure_user_only_permissions(path: &ConfigPathDto) -> DtoResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path.as_str()).map_err(|_| {
        ErrorDto::unavailable(
            "config_permission_metadata_unavailable",
            "configuration file permissions could not be inspected",
        )
    })?;
    if metadata.permissions().mode() & 0o077 == 0 {
        Ok(())
    } else {
        Err(ErrorDto::new(
            "unsafe_config_permissions",
            intention_proto::ErrorCategoryDto::Policy,
            "configuration file must be readable only by its owner",
            intention_proto::ErrorRetryDto::Manual,
            None,
        )?)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;

    fn fixture_path(filename: &str) -> String {
        std::env::temp_dir()
            .join(filename)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn paths_and_sources_preserve_semantic_configuration_intent() {
        let fixture_path = fixture_path("intention.toml");
        let path = ConfigPathDto::parse(fixture_path.as_str()).expect("absolute path is valid");
        assert_eq!(path.as_str(), fixture_path);
        for invalid in ["", "relative.toml"] {
            assert_eq!(
                ConfigPathDto::parse(invalid)
                    .expect_err("invalid path must fail")
                    .code(),
                "invalid_config_path"
            );
        }
        let explicit = ConfigPathResolver::resolve(Some(path.clone())).expect("override resolves");
        assert!(matches!(explicit, ConfigSourceDto::Explicit(_)));
        assert_eq!(explicit.path(), &path);
        let platform = ConfigPathResolver::resolve(None).expect("platform path resolves");
        assert!(matches!(platform, ConfigSourceDto::PlatformDefault(_)));
        assert!(platform.path().as_str().ends_with("config.toml"));
    }

    #[test]
    fn context_window_policy_validates_and_commits_into_a_snapshot() {
        assert_eq!(
            ContextWindowPolicyDto::new(0)
                .expect_err("a zero window rejects")
                .code(),
            "invalid_provider_context_window_tokens"
        );
        assert_eq!(
            ContextWindowPolicyDto::default_policy().window_tokens(),
            250_000
        );
        let window = ContextWindowPolicyDto::new(180_000).expect("a positive window is valid");
        assert_eq!(window.window_tokens(), 180_000);

        let captured_at =
            TimestampDto::from_unix_seconds(1_700_000_000).expect("fixture timestamp is valid");
        let snapshot = ConfigSnapshotDto::new(
            SchemaVersionDto::new(1, 0),
            ConfigRevisionId::new(),
            captured_at,
            window,
        )
        .expect("a compatible snapshot is valid");
        assert_eq!(snapshot.schema_version(), SchemaVersionDto::new(1, 0));
        assert_eq!(snapshot.captured_at(), captured_at);
        assert_eq!(snapshot.context_window().window_tokens(), 180_000);
        assert!(snapshot.validate_for_persistence().is_ok());
        assert!(
            ConfigSnapshotDto::new(
                SchemaVersionDto::new(2, 0),
                ConfigRevisionId::new(),
                captured_at,
                window,
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_permission_policy_accepts_owner_only_and_rejects_unsafe_or_missing_files() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "intention-config-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time must be after the Unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&directory).expect("temporary directory is available");
        let safe_path = directory.join("safe.toml");
        fs::write(&safe_path, "fixture").expect("fixture file is writable");
        fs::set_permissions(&safe_path, fs::Permissions::from_mode(0o600))
            .expect("safe permissions can be set");
        let safe_dto = ConfigPathDto::parse(safe_path.to_string_lossy().into_owned())
            .expect("temporary path is absolute");
        assert!(ensure_user_only_permissions(&safe_dto).is_ok());

        fs::set_permissions(&safe_path, fs::Permissions::from_mode(0o644))
            .expect("unsafe permissions can be set");
        assert_eq!(
            ensure_user_only_permissions(&safe_dto)
                .expect_err("group-readable file must fail")
                .code(),
            "unsafe_config_permissions"
        );
        let missing = ConfigPathDto::parse(
            directory
                .join("missing.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("temporary path is absolute");
        assert_eq!(
            ensure_user_only_permissions(&missing)
                .expect_err("missing file must fail")
                .code(),
            "config_permission_metadata_unavailable"
        );
    }
}
