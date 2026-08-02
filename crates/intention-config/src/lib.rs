//! TOML configuration parsing, migration, resolution, and safe projection.
//!
//! M1 parses a versioned TOML file into a validated public projection. Raw
//! credentials remain inside this crate and are never serialized, displayed, or
//! included in errors. M1 establishes the credential-free snapshot DTO shape;
//! configuration persistence, daemon reload, and per-run application remain
//! deferred to M3 and M4.

use std::fmt::{Display, Formatter};
use std::path::Path;

use intention_types::{ConfigRevisionId, DtoResult, ErrorDto, SchemaVersionDto, TimestampDto};
use serde::{Deserialize, Serialize};

const CURRENT_SCHEMA_MAJOR: u16 = 1;
const CURRENT_SCHEMA_MINOR: u16 = 0;

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

/// Identifies how a configuration location was selected without disclosing it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSourceKindDto {
    /// A caller explicitly selected the configuration file location.
    Explicit,
    /// The location came from the operating system's standard config directory.
    PlatformDefault,
}

impl ConfigSourceKindDto {
    /// Returns the stable safe representation for diagnostics and snapshots.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::PlatformDefault => "platform_default",
        }
    }
}

impl Display for ConfigSourceKindDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
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

    /// Returns the safe source category without disclosing the path.
    #[must_use]
    pub const fn kind(&self) -> ConfigSourceKindDto {
        match self {
            Self::Explicit(_) => ConfigSourceKindDto::Explicit,
            Self::PlatformDefault(_) => ConfigSourceKindDto::PlatformDefault,
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

/// Raw configuration text paired with a validated selection source.
///
/// This input intentionally has no `Debug`, serialization, or content accessor
/// because it may contain an open-text credential by explicit product decision.
pub struct RawConfigInputDto {
    text: String,
    source: ConfigSourceDto,
}

impl RawConfigInputDto {
    /// Creates opaque TOML input for parse/validate/resolve processing.
    #[must_use]
    pub fn new(text: impl Into<String>, source: ConfigSourceDto) -> Self {
        Self {
            text: text.into(),
            source,
        }
    }
}

/// The provider API contract selected by validated configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKindDto {
    /// The OpenRouter provider adapter.
    Openrouter,
    /// An OpenAI-compatible Chat Completions endpoint.
    GenericChatCompletionApi,
}

impl ProviderKindDto {
    /// Returns the stable TOML and projection representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Openrouter => "openrouter",
            Self::GenericChatCompletionApi => "generic-chat-completion-api",
        }
    }
}

impl Display for ProviderKindDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A public, credential-free provider selection projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderSelectionDto {
    kind: ProviderKindDto,
    model: String,
    endpoint: Option<String>,
    credential_configured: bool,
}

impl<'de> Deserialize<'de> for ProviderSelectionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawProviderSelectionDto {
            kind: ProviderKindDto,
            model: String,
            endpoint: Option<String>,
            credential_configured: bool,
        }

        let raw = RawProviderSelectionDto::deserialize(deserializer)?;
        Self::new(raw.kind, raw.model, raw.endpoint, raw.credential_configured)
            .map_err(serde::de::Error::custom)
    }
}

impl ProviderSelectionDto {
    fn new(
        kind: ProviderKindDto,
        model: String,
        endpoint: Option<String>,
        credential_configured: bool,
    ) -> DtoResult<Self> {
        if model.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_provider_model",
                "provider model must not be empty",
            ));
        }
        if endpoint
            .as_ref()
            .is_some_and(|configured_endpoint| configured_endpoint.trim().is_empty())
        {
            return Err(ErrorDto::validation(
                "invalid_provider_endpoint",
                "provider endpoint must not be empty when configured",
            ));
        }
        Ok(Self {
            kind,
            model,
            endpoint,
            credential_configured,
        })
    }
    /// Returns the selected provider API contract.
    #[must_use]
    pub const fn kind(&self) -> ProviderKindDto {
        self.kind
    }

    /// Returns the configured model identifier.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the optional non-secret provider endpoint.
    #[must_use]
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Returns whether a credential was supplied, without exposing it.
    #[must_use]
    pub const fn credential_configured(&self) -> bool {
        self.credential_configured
    }
}

/// A public, credential-free resolved configuration DTO.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResolvedConfigDto {
    schema_version: SchemaVersionDto,
    provider: ProviderSelectionDto,
    source_kind: ConfigSourceKindDto,
}

impl<'de> Deserialize<'de> for ResolvedConfigDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawResolvedConfigDto {
            schema_version: SchemaVersionDto,
            provider: ProviderSelectionDto,
            source_kind: ConfigSourceKindDto,
        }

        let raw = RawResolvedConfigDto::deserialize(deserializer)?;
        Self::from_public_parts(raw.schema_version, raw.provider, raw.source_kind)
            .map_err(serde::de::Error::custom)
    }
}

impl ResolvedConfigDto {
    /// Parses, migrates, validates, and resolves opaque TOML input.
    ///
    /// # Errors
    ///
    /// Returns only safe typed validation errors. The input TOML and any
    /// credential it contains are deliberately omitted from errors.
    pub fn parse_resolve(input: RawConfigInputDto) -> DtoResult<Self> {
        let document: toml::Value = toml::from_str(&input.text).map_err(|_| {
            ErrorDto::validation(
                "invalid_config_toml",
                "configuration TOML could not be parsed",
            )
        })?;
        let normalized = match document.get("schema_version") {
            None => Self::migrate_v0(document)?,
            Some(toml::Value::Integer(major)) if *major == i64::from(CURRENT_SCHEMA_MAJOR) => {
                Self::parse_v1(document)?
            }
            Some(toml::Value::Integer(_)) => {
                return Err(ErrorDto::validation(
                    "unsupported_config_schema_version",
                    "configuration schema version is not supported",
                ));
            }
            Some(_) => {
                return Err(ErrorDto::validation(
                    "invalid_config_schema_version",
                    "configuration schema version must be an integer",
                ));
            }
        };
        Self::validate(normalized, input.source.kind())
    }

    fn parse_v1(document: toml::Value) -> DtoResult<NormalizedConfig> {
        let raw: RawV1Config = document.try_into().map_err(|_| {
            ErrorDto::validation(
                "invalid_config_schema",
                "configuration does not match the supported schema",
            )
        })?;
        Ok(NormalizedConfig {
            provider: raw.provider,
        })
    }

    fn migrate_v0(document: toml::Value) -> DtoResult<NormalizedConfig> {
        let raw: RawV0Config = document.try_into().map_err(|_| {
            ErrorDto::validation(
                "invalid_legacy_config_schema",
                "legacy configuration does not match the supported migration schema",
            )
        })?;
        Ok(NormalizedConfig {
            provider: RawProviderConfig {
                kind: raw.model.provider,
                model: raw.model.name,
                credential: raw.model.api_key,
                endpoint: None,
            },
        })
    }

    fn validate(config: NormalizedConfig, source_kind: ConfigSourceKindDto) -> DtoResult<Self> {
        if config.provider.credential.trim().is_empty() {
            return Err(ErrorDto::validation(
                "missing_provider_credential",
                "provider credential must not be empty",
            ));
        }
        let provider = ProviderSelectionDto::new(
            config.provider.kind,
            config.provider.model,
            config.provider.endpoint,
            true,
        )?;
        Self::from_public_parts(
            SchemaVersionDto::new(CURRENT_SCHEMA_MAJOR, CURRENT_SCHEMA_MINOR),
            provider,
            source_kind,
        )
    }

    fn from_public_parts(
        schema_version: SchemaVersionDto,
        provider: ProviderSelectionDto,
        source_kind: ConfigSourceKindDto,
    ) -> DtoResult<Self> {
        SchemaVersionDto::new(CURRENT_SCHEMA_MAJOR, CURRENT_SCHEMA_MINOR)
            .ensure_compatible_with(schema_version)?;
        Ok(Self {
            schema_version,
            provider,
            source_kind,
        })
    }

    /// Returns the normalized current configuration schema version.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the public credential-free provider selection.
    #[must_use]
    pub const fn provider(&self) -> &ProviderSelectionDto {
        &self.provider
    }

    /// Returns the safe source category without its local filesystem path.
    #[must_use]
    pub const fn source_kind(&self) -> ConfigSourceKindDto {
        self.source_kind
    }

    /// Returns a safe diagnostic projection with no source path or credential.
    #[must_use]
    pub fn safe_debug_projection(&self) -> String {
        format!(
            "schema_version={}.{} source={} provider={} model={} credential_configured={}",
            self.schema_version.major(),
            self.schema_version.minor(),
            self.source_kind,
            self.provider.kind,
            self.provider.model,
            self.provider.credential_configured
        )
    }
}

/// An immutable, credential-free configuration selection captured for a future run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConfigSnapshotDto {
    schema_version: SchemaVersionDto,
    revision_id: ConfigRevisionId,
    captured_at: TimestampDto,
    resolved: ResolvedConfigDto,
}

impl<'de> Deserialize<'de> for ConfigSnapshotDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawConfigSnapshotDto {
            schema_version: SchemaVersionDto,
            revision_id: ConfigRevisionId,
            captured_at: TimestampDto,
            resolved: ResolvedConfigDto,
        }

        let raw = RawConfigSnapshotDto::deserialize(deserializer)?;
        SchemaVersionDto::new(CURRENT_SCHEMA_MAJOR, CURRENT_SCHEMA_MINOR)
            .ensure_compatible_with(raw.schema_version)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            schema_version: raw.schema_version,
            revision_id: raw.revision_id,
            captured_at: raw.captured_at,
            resolved: raw.resolved,
        })
    }
}

impl ConfigSnapshotDto {
    /// Creates an immutable credential-free configuration snapshot.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error when the snapshot schema major differs
    /// from the supported configuration snapshot major.
    pub fn new(
        schema_version: SchemaVersionDto,
        revision_id: ConfigRevisionId,
        captured_at: TimestampDto,
        resolved: ResolvedConfigDto,
    ) -> DtoResult<Self> {
        SchemaVersionDto::new(CURRENT_SCHEMA_MAJOR, CURRENT_SCHEMA_MINOR)
            .ensure_compatible_with(schema_version)?;
        Ok(Self {
            schema_version,
            revision_id,
            captured_at,
            resolved,
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

    /// Returns the safe resolved configuration projection.
    #[must_use]
    pub const fn resolved(&self) -> &ResolvedConfigDto {
        &self.resolved
    }

    /// Verifies the already-redacted snapshot remains suitable for durable storage.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when either nested public schema major is unsupported.
    pub fn validate_for_persistence(&self) -> DtoResult<()> {
        SchemaVersionDto::new(CURRENT_SCHEMA_MAJOR, CURRENT_SCHEMA_MINOR)
            .ensure_compatible_with(self.schema_version)?;
        SchemaVersionDto::new(CURRENT_SCHEMA_MAJOR, CURRENT_SCHEMA_MINOR)
            .ensure_compatible_with(self.resolved.schema_version())
    }
}

impl Display for ResolvedConfigDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.safe_debug_projection())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawV1Config {
    #[serde(rename = "schema_version")]
    _schema_version: u16,
    provider: RawProviderConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawV0Config {
    model: RawV0ModelConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawV0ModelConfig {
    provider: ProviderKindDto,
    name: String,
    api_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProviderConfig {
    kind: ProviderKindDto,
    model: String,
    credential: String,
    endpoint: Option<String>,
}

struct NormalizedConfig {
    provider: RawProviderConfig,
}

impl<'de> Deserialize<'de> for ProviderKindDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "openrouter" => Ok(Self::Openrouter),
            "generic-chat-completion-api" => Ok(Self::GenericChatCompletionApi),
            _ => Err(serde::de::Error::custom("unsupported provider kind")),
        }
    }
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
            intention_types::ErrorCategoryDto::Policy,
            "configuration file must be readable only by its owner",
            intention_types::ErrorRetryDto::Manual,
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

    const CREDENTIAL: &str = "fixture-credential-not-real-12345";

    fn fixture_path(filename: &str) -> String {
        std::env::temp_dir()
            .join(filename)
            .to_string_lossy()
            .into_owned()
    }

    fn explicit_source() -> ConfigSourceDto {
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(fixture_path("intention-relay-test.toml"))
                .expect("fixture path is absolute"),
        )
    }

    fn v1(provider: &str, model: &str, credential: &str, endpoint: Option<&str>) -> String {
        let endpoint = endpoint.map_or_else(String::new, |value| {
            format!(
                "endpoint = \"{value}\"
"
            )
        });
        format!(
            "schema_version = 1
[provider]
kind = \"{provider}\"
model = \"{model}\"
credential = \"{credential}\"
{endpoint}"
        )
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
        assert_eq!(explicit.kind(), ConfigSourceKindDto::Explicit);
        assert_eq!(explicit.path(), &path);
        let platform = ConfigPathResolver::resolve(None).expect("platform path resolves");
        assert_eq!(platform.kind(), ConfigSourceKindDto::PlatformDefault);
        assert!(platform.path().as_str().ends_with("config.toml"));
    }

    #[test]
    fn provider_kinds_and_public_selection_projection_cover_supported_values() {
        for (kind, expected) in [
            (ProviderKindDto::Openrouter, "openrouter"),
            (
                ProviderKindDto::GenericChatCompletionApi,
                "generic-chat-completion-api",
            ),
        ] {
            assert_eq!(kind.as_str(), expected);
            assert_eq!(kind.to_string(), expected);
        }
        for provider in ["openrouter", "openrouter", "generic-chat-completion-api"] {
            let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
                v1(
                    provider,
                    "fixture-model",
                    CREDENTIAL,
                    Some("https://example.invalid/v1"),
                ),
                explicit_source(),
            ))
            .expect("supported provider resolves");
            assert_eq!(
                resolved.provider().endpoint(),
                Some("https://example.invalid/v1")
            );
            assert!(resolved.provider().credential_configured());
            assert_eq!(resolved.schema_version(), SchemaVersionDto::new(1, 0));
        }
        let unknown = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            v1("unknown", "fixture-model", CREDENTIAL, None),
            explicit_source(),
        ))
        .expect_err("unknown provider must fail");
        assert_eq!(unknown.code(), "invalid_config_schema");
    }

    #[test]
    fn configuration_validation_rejects_all_boundary_failures_safely() {
        let fixtures = [
            (
                v1("openrouter", " ", CREDENTIAL, None),
                "invalid_provider_model",
            ),
            (
                v1("openrouter", "fixture-model", " ", None),
                "missing_provider_credential",
            ),
            (
                v1("openrouter", "fixture-model", CREDENTIAL, Some(" ")),
                "invalid_provider_endpoint",
            ),
            (
                "schema_version = 2
"
                .to_owned(),
                "unsupported_config_schema_version",
            ),
            (
                "schema_version = -1
"
                .to_owned(),
                "unsupported_config_schema_version",
            ),
            ("not = [valid".to_owned(), "invalid_config_toml"),
        ];
        for (text, expected_code) in fixtures {
            let error =
                ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(text, explicit_source()))
                    .expect_err("invalid fixture must fail");
            assert_eq!(error.code(), expected_code);
            assert!(!error.to_string().contains(CREDENTIAL));
        }
    }

    #[test]
    fn configuration_projection_is_redacted_and_source_kind_is_safe() {
        let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            v1("openrouter", "gpt-5.6-terra", CREDENTIAL, None),
            explicit_source(),
        ))
        .expect("fixture resolves");
        let projection = resolved.safe_debug_projection();
        assert!(projection.contains("source=explicit"));
        assert!(projection.contains("credential_configured=true"));
        assert!(!projection.contains(CREDENTIAL));
        assert_eq!(resolved.to_string(), projection);
    }

    #[test]
    fn parse_and_migration_helpers_reject_schema_and_legacy_shape_mismatches() {
        let wrong_major = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            v1("openrouter", "fixture", CREDENTIAL, None).replacen(
                "schema_version = 1",
                "schema_version = 2",
                1,
            ),
            explicit_source(),
        ))
        .expect_err("wrong version must fail");
        assert_eq!(wrong_major.code(), "unsupported_config_schema_version");
        let missing_provider = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            "schema_version = 1".to_owned(),
            explicit_source(),
        ))
        .expect_err("missing provider must fail");
        assert_eq!(missing_provider.code(), "invalid_config_schema");
        let incomplete_legacy = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            "[model]\nprovider = \"openrouter\"".to_owned(),
            explicit_source(),
        ))
        .expect_err("incomplete legacy configuration must fail");
        assert_eq!(incomplete_legacy.code(), "invalid_legacy_config_schema");
    }

    #[test]
    fn generic_provider_preserves_configured_model_identifier() {
        let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            v1(
                "generic-chat-completion-api",
                "example-chat-model",
                CREDENTIAL,
                None,
            ),
            explicit_source(),
        ))
        .expect("generic provider selection must preserve the configured model identifier");

        assert_eq!(
            resolved.provider().kind(),
            ProviderKindDto::GenericChatCompletionApi
        );
        assert_eq!(resolved.provider().model(), "example-chat-model");
    }

    #[test]
    fn raw_config_input_accepts_owned_text_without_public_projection() {
        let raw = RawConfigInputDto::new(
            v1("openrouter", "fixture", CREDENTIAL, None),
            explicit_source(),
        );
        let resolved = ResolvedConfigDto::parse_resolve(raw).expect("opaque input resolves");
        assert_eq!(resolved.provider().model(), "fixture");
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
