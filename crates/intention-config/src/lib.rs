//! TOML configuration parsing, validation, resolution, and safe projection.
//!
//! One TOML document is parsed and validated exactly once into one resolved,
//! credential-free shape; the same parse also yields the private credential for
//! startup. Raw credentials remain inside this crate and are never serialized,
//! displayed, or included in errors.

use std::fmt::{Display, Formatter};
use std::path::Path;

use intention_proto::{DtoResult, ErrorDto, TimestampDto};
use serde::{Deserialize, Serialize};

/// The one live configuration schema version the TOML parser accepts.
const CURRENT_SCHEMA_VERSION: u16 = 1;
const DEFAULT_CONTEXT_WINDOW_TOKENS: u64 = 250_000;

/// The validated absolute location of the one configuration file.
///
/// The location never crosses a wire and never appears in a projection: it is
/// a local startup input for reading the configuration document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigPathDto(String);

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

/// Selects the standard platform config location or a caller-supplied override.
pub struct ConfigPathResolver;

impl ConfigPathResolver {
    /// Resolves the caller's override or the one standard platform location.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error if the operating system has no usable config
    /// directory for the current user, or a validation error for an invalid override.
    pub fn resolve(override_path: Option<ConfigPathDto>) -> DtoResult<ConfigPathDto> {
        if let Some(path) = override_path {
            return Ok(path);
        }
        let path = platform_config_path().ok_or_else(|| {
            ErrorDto::unavailable(
                "platform_config_directory_unavailable",
                "a platform configuration directory could not be determined",
            )
        })?;
        ConfigPathDto::parse(path)
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

/// Raw configuration text.
///
/// This input intentionally has no `Debug`, serialization, or content accessor
/// because it may contain an open-text credential by explicit product decision.
pub struct RawConfigInputDto {
    text: String,
}

impl RawConfigInputDto {
    /// Creates opaque TOML input for parse/validate/resolve processing.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// The provider API contract selected by validated configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProviderKindDto {
    /// The OpenRouter provider adapter.
    #[serde(rename = "openrouter")]
    Openrouter,
    /// An OpenAI-compatible Chat Completions endpoint.
    #[serde(rename = "generic-chat-completion-api")]
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
///
/// Decoding re-validates the model and endpoint through the same constructor
/// the TOML path uses, so a decoded snapshot cannot carry a blank selection.
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedConfigDto {
    provider: ProviderSelectionDto,
    provider_execution: ProviderExecutionPolicyDto,
    context_window: ContextWindowPolicyDto,
}

impl ResolvedConfigDto {
    /// Parses, validates, and resolves opaque TOML input.
    ///
    /// # Errors
    ///
    /// Returns only safe typed validation errors. The input TOML and any
    /// credential it contains are deliberately omitted from errors.
    pub fn parse_resolve(input: RawConfigInputDto) -> DtoResult<Self> {
        Self::parse_document(&input.text).map(|(resolved, _credential)| resolved)
    }

    /// Parses configuration into opaque startup-only material and its safe projection.
    ///
    /// # Errors
    ///
    /// Returns only safe typed validation errors and never exposes the raw credential.
    pub fn parse_startup_material(input: RawConfigInputDto) -> DtoResult<StartupProviderMaterial> {
        let (resolved, credential) = Self::parse_document(&input.text)?;
        Ok(StartupProviderMaterial {
            resolved,
            opaque: credential,
        })
    }

    /// Parses, validates, and resolves one TOML document exactly once.
    ///
    /// The returned credential is the private `provider.credential` value; it
    /// never enters a public value, error, or log.
    ///
    /// # Errors
    ///
    /// Returns only safe typed validation errors. The input TOML and any
    /// credential it contains are deliberately omitted from errors.
    fn parse_document(text: &str) -> DtoResult<(Self, String)> {
        let document: toml::Value = toml::from_str(text).map_err(|_| {
            ErrorDto::validation(
                "invalid_config_toml",
                "configuration TOML could not be parsed",
            )
        })?;
        match document.get("schema_version") {
            Some(toml::Value::Integer(version))
                if *version == i64::from(CURRENT_SCHEMA_VERSION) => {}
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
            None => {
                return Err(ErrorDto::validation(
                    "invalid_config_schema",
                    "configuration does not include a schema version",
                ));
            }
        }
        let raw: RawV1Config = document.try_into().map_err(|_| {
            ErrorDto::validation(
                "invalid_config_schema",
                "configuration does not match the supported schema",
            )
        })?;
        let RawV1Config {
            _schema_version,
            provider: raw_provider,
        } = raw;
        let RawProviderConfig {
            kind,
            model,
            credential,
            endpoint,
            execution,
            context_window_tokens,
        } = raw_provider;
        let credential = match credential {
            Some(credential) if !credential.trim().is_empty() => credential,
            Some(_) => {
                return Err(ErrorDto::validation(
                    "missing_provider_credential",
                    "provider credential must not be empty",
                ));
            }
            None => {
                return Err(ErrorDto::validation(
                    "invalid_config_schema",
                    "configuration does not match the supported schema",
                ));
            }
        };
        let context_window = ContextWindowPolicyDto::from_raw(context_window_tokens)?;
        let provider = ProviderSelectionDto::new(kind, model, endpoint, true)?;
        let resolved = Self {
            provider,
            provider_execution: ProviderExecutionPolicyDto::from_raw(execution)?,
            context_window,
        };
        Ok((resolved, credential))
    }

    /// Returns the public credential-free provider selection.
    #[must_use]
    pub const fn provider(&self) -> &ProviderSelectionDto {
        &self.provider
    }

    /// Returns the fully effective safe provider execution policy.
    #[must_use]
    pub const fn provider_execution(&self) -> &ProviderExecutionPolicyDto {
        &self.provider_execution
    }

    /// Returns the fully effective safe context-window policy.
    #[must_use]
    pub const fn context_window(&self) -> &ContextWindowPolicyDto {
        &self.context_window
    }
}

/// An immutable, credential-free configuration selection captured for a future run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSnapshotDto {
    captured_at: TimestampDto,
    resolved: ResolvedConfigDto,
}

impl ConfigSnapshotDto {
    /// Creates an immutable credential-free configuration snapshot.
    #[must_use]
    pub const fn new(captured_at: TimestampDto, resolved: ResolvedConfigDto) -> Self {
        Self {
            captured_at,
            resolved,
        }
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
}

/// Safe per-run provider execution policy resolved at startup.
///
/// Decoding applies the same defaults and range validation as resolution, so a
/// decoded policy always carries a supported attempt budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderExecutionPolicyDto {
    attempt_timeout_seconds: u8,
    max_attempts: u8,
}

impl<'de> Deserialize<'de> for ProviderExecutionPolicyDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawProviderExecutionPolicyDto::deserialize(deserializer)?;
        Self::from_raw(Some(raw)).map_err(serde::de::Error::custom)
    }
}

impl ProviderExecutionPolicyDto {
    fn from_raw(raw: Option<RawProviderExecutionPolicyDto>) -> DtoResult<Self> {
        let raw = raw.unwrap_or_default();
        let attempt_timeout_seconds = raw.attempt_timeout_seconds.unwrap_or(30);
        let max_attempts = raw.max_attempts.unwrap_or(2);
        if !(1..=60).contains(&attempt_timeout_seconds) {
            return Err(ErrorDto::validation(
                "invalid_provider_attempt_timeout_seconds",
                "provider attempt timeout seconds must be between 1 and 60",
            ));
        }
        if !(1..=2).contains(&max_attempts) {
            return Err(ErrorDto::validation(
                "invalid_provider_max_attempts",
                "provider max attempts must be between 1 and 2",
            ));
        }
        Ok(Self {
            attempt_timeout_seconds,
            max_attempts,
        })
    }

    /// Returns the idle window of one provider attempt in seconds.
    ///
    /// The window bounds the silence between two provider facts, not the
    /// attempt's total duration: it is armed when the attempt's stream opens
    /// and re-armed by every event that stream delivers.
    #[must_use]
    pub const fn attempt_timeout_seconds(self) -> u8 {
        self.attempt_timeout_seconds
    }

    /// Returns the bounded number of attempts available to the run.
    #[must_use]
    pub const fn max_attempts(self) -> u8 {
        self.max_attempts
    }
}

/// Safe per-run provider context-window policy resolved at startup.
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
    fn from_raw(window_tokens: Option<u64>) -> DtoResult<Self> {
        let window_tokens = window_tokens.unwrap_or(DEFAULT_CONTEXT_WINDOW_TOKENS);
        if window_tokens == 0 {
            return Err(ErrorDto::validation(
                "invalid_provider_context_window_tokens",
                "provider context window tokens must be greater than zero",
            ));
        }
        Ok(Self { window_tokens })
    }

    /// Returns the sliding context window size in tokens.
    #[must_use]
    pub const fn window_tokens(self) -> u64 {
        self.window_tokens
    }
}

/// Startup-only opaque provider material paired with its safe resolved projection.
///
/// This type intentionally implements no `Debug`, `Display`, or serde traits and
/// provides no credential accessor. Composition may move it only into a selected
/// provider constructor during daemon startup.
pub struct StartupProviderMaterial {
    resolved: ResolvedConfigDto,
    opaque: String,
}

impl StartupProviderMaterial {
    /// Returns the credential-free startup selection.
    #[must_use]
    pub const fn safe_resolved(&self) -> &ResolvedConfigDto {
        &self.resolved
    }

    /// Moves this opaque material into the supplied startup-only consumer.
    pub fn into_parts_for_provider<T>(
        self,
        consumer: impl FnOnce(ResolvedConfigDto, String) -> T,
    ) -> T {
        consumer(self.resolved, self.opaque)
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
struct RawProviderConfig {
    kind: ProviderKindDto,
    model: String,
    credential: Option<String>,
    endpoint: Option<String>,
    execution: Option<RawProviderExecutionPolicyDto>,
    context_window_tokens: Option<u64>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProviderExecutionPolicyDto {
    attempt_timeout_seconds: Option<u8>,
    max_attempts: Option<u8>,
}

#[cfg(test)]
mod tests {

    use super::*;

    const CREDENTIAL: &str = "fixture-credential-not-real-12345";

    fn fixture_path(filename: &str) -> String {
        std::env::temp_dir()
            .join(filename)
            .to_string_lossy()
            .into_owned()
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
    fn configuration_path_resolution_keeps_the_override_and_platform_location() {
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
        let resolved = ConfigPathResolver::resolve(Some(path.clone())).expect("override resolves");
        assert_eq!(resolved, path);
        let platform = ConfigPathResolver::resolve(None).expect("platform path resolves");
        assert!(platform.as_str().ends_with("config.toml"));
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
        for (provider, model) in [
            ("openrouter", "fixture-model"),
            ("generic-chat-completion-api", "example-chat-model"),
        ] {
            let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(v1(
                provider,
                model,
                CREDENTIAL,
                Some("https://example.invalid/v1"),
            )))
            .expect("supported provider resolves");
            assert_eq!(resolved.provider().kind().as_str(), provider);
            assert_eq!(resolved.provider().model(), model);
            assert_eq!(
                resolved.provider().endpoint(),
                Some("https://example.invalid/v1")
            );
            assert!(resolved.provider().credential_configured());
        }
        let unknown = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(v1(
            "unknown",
            "fixture-model",
            CREDENTIAL,
            None,
        )))
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
            ("not = [valid".to_owned(), "invalid_config_toml"),
        ];
        for (text, expected_code) in fixtures {
            let error = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(text))
                .expect_err("invalid fixture must fail");
            assert_eq!(error.code(), expected_code);
            assert!(!error.to_string().contains(CREDENTIAL));
        }
    }

    #[test]
    fn parse_helpers_reject_schema_and_shape_mismatches() {
        // The single live schema version is the only accepted one: another
        // integer, a negative integer, a non-integer, and an absent version
        // each fail closed with their own typed code.
        for (text, expected_code) in [
            (
                v1("openrouter", "fixture", CREDENTIAL, None).replacen(
                    "schema_version = 1",
                    "schema_version = 2",
                    1,
                ),
                "unsupported_config_schema_version",
            ),
            (
                "schema_version = -1\n".to_owned(),
                "unsupported_config_schema_version",
            ),
            (
                "schema_version = \"one\"\n".to_owned(),
                "invalid_config_schema_version",
            ),
            ("schema_version = 1".to_owned(), "invalid_config_schema"),
            (
                "[model]\nprovider = \"openrouter\"\nname = \"fixture\"\napi_key = \"fixture-credential-not-real-12345\"\n"
                    .to_owned(),
                "invalid_config_schema",
            ),
        ] {
            let error = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(text))
                .expect_err("a mismatched document must fail closed");
            assert_eq!(error.code(), expected_code);
        }
    }

    #[test]
    fn startup_material_preserves_the_exact_private_credential_without_echoing_it() {
        // Serialize the document so string escaping is the TOML serializer's
        // responsibility, exactly like a raw configuration file.
        fn document_with_credential(credential: &str) -> String {
            let mut provider = toml::map::Map::new();
            provider.insert(
                "kind".to_owned(),
                toml::Value::String("openrouter".to_owned()),
            );
            provider.insert(
                "model".to_owned(),
                toml::Value::String("fixture-model".to_owned()),
            );
            provider.insert(
                "credential".to_owned(),
                toml::Value::String(credential.to_owned()),
            );
            let mut document = toml::map::Map::new();
            document.insert("schema_version".to_owned(), toml::Value::Integer(1));
            document.insert("provider".to_owned(), toml::Value::Table(provider));
            toml::to_string(&toml::Value::Table(document)).expect("document serializes")
        }

        let tricky = "sk-\"quoted\"-and\\backslash";
        let material = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
            document_with_credential(tricky),
        ))
        .expect("credential parses from the document");
        let (resolved, credential) =
            material.into_parts_for_provider(|resolved, credential| (resolved, credential));
        assert_eq!(
            credential, tricky,
            "escaping is the TOML parser's responsibility"
        );
        assert_eq!(resolved.provider().kind(), ProviderKindDto::Openrouter);

        // A blank credential is a value failure, an absent one is a shape
        // failure, and malformed TOML is a document failure: three distinct
        // typed codes that all stay free of document content.
        for (failure, expected_code) in [
            (document_with_credential(" "), "missing_provider_credential"),
            (
                "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\n"
                    .to_owned(),
                "invalid_config_schema",
            ),
            ("not a toml document [[".to_owned(), "invalid_config_toml"),
        ] {
            let error = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(failure))
                .err()
                .expect("invalid document must fail");
            assert_eq!(error.code(), expected_code);
            assert!(
                !error.to_string().contains(CREDENTIAL),
                "the error never echoes document content"
            );
        }
    }
}
