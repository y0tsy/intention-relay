//! Provider identities, catalog values, capability and reasoning contracts, and
//! control-plane DTOs.
//!
//! This module owns the credential-free provider control-plane surface shared by
//! the configuration, provider, storage, engine, and daemon boundaries: the
//! validated provider kind and profile identities, the closed catalog,
//! capability, reasoning, and selection value types, and the typed
//! control-plane payloads. It carries no provider SDK, no endpoint parser, no
//! runtime or registry state, and no field may carry a credential literal.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de};

pub use crate::{
    CatalogRevisionId, ProviderDiscoveryAttemptId, ProviderKindDescriptorRevisionId,
    ProviderProfileRevisionId, ReasoningHistoryManifestId,
};
use crate::{ConfigRevisionId, DtoResult, ErrorDto, IdempotencyKey, RunId, SessionId};

/// Returns whether `value` is a non-blank printable ASCII token.
///
/// Provider identities and safe metadata tokens never contain whitespace or
/// control characters, and they carry no length cap.
fn is_safe_token(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_graphic())
}

/// Returns whether `value` is a lower `snake_case` code token.
fn is_code_token(value: &str) -> bool {
    let mut characters = value.chars();
    if !characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
    {
        return false;
    }
    characters.all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
    })
}

/// Returns whether `value` is a valid HTTP header field-name token.
fn is_http_header_name_token(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    '!' | '#'
                        | '$'
                        | '%'
                        | '&'
                        | '\''
                        | '*'
                        | '+'
                        | '-'
                        | '.'
                        | '^'
                        | '_'
                        | '`'
                        | '|'
                        | '~'
                )
        })
}

/// Requires `value` to be a non-blank printable ASCII token.
fn require_safe_token(value: &str, code: &'static str, message: &'static str) -> DtoResult<()> {
    if is_safe_token(value) {
        Ok(())
    } else {
        Err(ErrorDto::validation(code, message))
    }
}

/// Returns the URL scheme of a well-formed absolute provider endpoint.
///
/// The shape rule rejects empty authorities, userinfo, query strings,
/// fragments, control characters, and malformed percent escapes.
fn endpoint_scheme(endpoint: &str) -> Option<&'static str> {
    let (scheme, authority_text) = endpoint
        .strip_prefix("https://")
        .map(|rest| ("https", rest))
        .or_else(|| endpoint.strip_prefix("http://").map(|rest| ("http", rest)))?;
    if endpoint
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    if endpoint.contains('?') || endpoint.contains('#') {
        return None;
    }
    if malformed_percent_escape(endpoint) {
        return None;
    }
    let authority = authority_text.split('/').next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    endpoint_host(authority)?;
    Some(scheme)
}

/// Returns the host of one endpoint authority, when it is well formed.
fn endpoint_host(authority: &str) -> Option<&str> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, remainder) = bracketed.split_once(']')?;
        if !remainder.is_empty() && !remainder.starts_with(':') {
            return None;
        }
        return (!host.is_empty()).then_some(host);
    }
    let (host, port) = authority
        .split_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    if host.is_empty() {
        return None;
    }
    if let Some(port) = port
        && (port.is_empty() || !port.chars().all(|character| character.is_ascii_digit()))
    {
        return None;
    }
    Some(host)
}

/// Returns whether `value` contains a malformed percent escape.
fn malformed_percent_escape(value: &str) -> bool {
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '%'
            && !(characters.next().is_some_and(|c| c.is_ascii_hexdigit())
                && characters.next().is_some_and(|c| c.is_ascii_hexdigit()))
        {
            return true;
        }
    }
    false
}

/// Validates one normalized endpoint against its applicable loopback policy.
///
/// An `http` endpoint is permitted only under an explicit loopback policy and
/// only for `localhost`, `127.0.0.1`, or `[::1]`.
fn validate_endpoint(endpoint: &str, loopback_policy: LoopbackPolicyDto) -> DtoResult<()> {
    let Some(scheme) = endpoint_scheme(endpoint) else {
        return Err(ErrorDto::validation(
            "invalid_provider_endpoint",
            "provider endpoint must be an absolute HTTP(S) base URL without userinfo, query, fragment, controls, or malformed escapes",
        ));
    };
    if scheme == "http" {
        let authority = endpoint
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or_default();
        let is_loopback_host = endpoint_host(authority)
            .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
        if loopback_policy != LoopbackPolicyDto::ExplicitLoopback || !is_loopback_host {
            return Err(ErrorDto::validation(
                "invalid_provider_endpoint",
                "an HTTP provider endpoint requires an explicit loopback policy and a loopback host",
            ));
        }
    }
    Ok(())
}

/// Validates the endpoint shape without the loopback rule.
fn validate_endpoint_shape(endpoint: &str) -> DtoResult<()> {
    if endpoint_scheme(endpoint).is_some() {
        Ok(())
    } else {
        Err(ErrorDto::validation(
            "invalid_provider_endpoint",
            "provider endpoint must be an absolute HTTP(S) base URL without userinfo, query, fragment, controls, or malformed escapes",
        ))
    }
}

/// Validates the credential transport mode and its optional header name.
fn validate_credential_transport_fields(
    mode: CredentialTransportModeDto,
    safe_header_name: Option<&str>,
) -> DtoResult<()> {
    match mode {
        CredentialTransportModeDto::Bearer if safe_header_name.is_some() => {
            Err(ErrorDto::validation(
                "invalid_credential_transport",
                "bearer credential transport carries no header name",
            ))
        }
        CredentialTransportModeDto::Bearer => Ok(()),
        CredentialTransportModeDto::SafeHeader
            if safe_header_name.is_some_and(is_http_header_name_token) =>
        {
            Ok(())
        }
        CredentialTransportModeDto::SafeHeader => Err(ErrorDto::validation(
            "invalid_credential_transport",
            "safe-header credential transport requires a valid HTTP header name",
        )),
    }
}

/// Validates one resolved reasoning policy against a declared capability subset.
///
/// A selected effort, summary declaration, or history transfer the declared
/// subset cannot represent fails validation before any provider work occurs.
fn validate_reasoning_policy_against_capabilities(
    policy: &ResolvedReasoningPolicyDto,
    capabilities: &ModelCapabilitySetV1,
) -> DtoResult<()> {
    if policy
        .effort()
        .is_some_and(|effort| !capabilities.supports_effort(effort))
    {
        return Err(ErrorDto::validation(
            "invalid_provider_profile_revision",
            "the selected reasoning effort is not declared by the model capability subset",
        ));
    }
    if policy.summary_support() && !capabilities.supports_summary() {
        return Err(ErrorDto::validation(
            "invalid_provider_profile_revision",
            "reasoning summary support is not declared by the model capability subset",
        ));
    }
    if policy.transfer() != capabilities.reasoning_input_contract() {
        return Err(ErrorDto::validation(
            "invalid_provider_profile_revision",
            "the resolved reasoning transfer is not the declared capability input contract",
        ));
    }
    Ok(())
}

macro_rules! provider_token_id {
    ($name:ident, $code:literal, $description:literal, $subject:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(de::Error::custom)
            }
        }

        impl $name {
            /// Parses one validated token identity.
            ///
            /// # Errors
            ///
            /// Returns a validation error unless `value` is a non-blank printable
            /// ASCII token without whitespace or control characters.
            pub fn parse(value: &str) -> DtoResult<Self> {
                require_safe_token(
                    value,
                    $code,
                    concat!($subject, " must be a non-blank printable ASCII token"),
                )?;
                Ok(Self(value.to_owned()))
            }

            /// Returns the declared token identity.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

provider_token_id!(
    ProviderKindId,
    "invalid_provider_kind_id",
    "A stable identity for one provider kind.",
    "provider kind identity"
);
provider_token_id!(
    ProviderProfileId,
    "invalid_provider_profile_id",
    "A stable identity for one provider profile within its catalog.",
    "provider profile identity"
);

impl ProviderKindId {
    /// The reserved first-party provider kind identities.
    pub const FIRST_PARTY_IDS: [&'static str; 2] = ["openrouter", "generic-chat-completion-api"];

    /// Returns whether this identity is a reserved first-party provider kind.
    #[must_use]
    pub fn is_first_party(&self) -> bool {
        Self::FIRST_PARTY_IDS.contains(&self.as_str())
    }
}

/// The closed provider credential transport mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialTransportModeDto {
    /// The credential travels as a bearer authorization value.
    Bearer,
    /// The credential travels as the complete value of one declared safe header.
    SafeHeader,
}

impl CredentialTransportModeDto {
    /// Returns the canonical durable string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bearer => "bearer",
            Self::SafeHeader => "safe_header",
        }
    }

    /// Parses the canonical durable string representation.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `value` is not a declared transport mode.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "bearer" => Ok(Self::Bearer),
            "safe_header" => Ok(Self::SafeHeader),
            _ => Err(ErrorDto::validation(
                "invalid_credential_transport_mode",
                "the credential transport mode is not declared",
            )),
        }
    }
}

/// A validated credential transport declaration for one profile revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CredentialTransportDto {
    mode: CredentialTransportModeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    safe_header_name: Option<String>,
}

impl<'de> Deserialize<'de> for CredentialTransportDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawCredentialTransportDto {
            mode: CredentialTransportModeDto,
            #[serde(default)]
            safe_header_name: Option<String>,
        }

        let raw = RawCredentialTransportDto::deserialize(deserializer)?;
        Self::new(raw.mode, raw.safe_header_name).map_err(de::Error::custom)
    }
}

impl CredentialTransportDto {
    /// Creates a validated credential transport declaration.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a safe-header transport carries no valid
    /// header name or a bearer transport carries a header name.
    pub fn new(
        mode: CredentialTransportModeDto,
        safe_header_name: Option<String>,
    ) -> DtoResult<Self> {
        validate_credential_transport_fields(mode, safe_header_name.as_deref())?;
        Ok(Self {
            mode,
            safe_header_name,
        })
    }

    /// Creates a bearer credential transport declaration.
    #[must_use]
    pub const fn bearer() -> Self {
        Self {
            mode: CredentialTransportModeDto::Bearer,
            safe_header_name: None,
        }
    }

    /// Creates a safe-header credential transport declaration.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `header_name` is not a valid HTTP header name.
    pub fn safe_header(header_name: impl Into<String>) -> DtoResult<Self> {
        Self::new(
            CredentialTransportModeDto::SafeHeader,
            Some(header_name.into()),
        )
    }

    /// Returns the closed credential transport mode.
    #[must_use]
    pub const fn mode(&self) -> CredentialTransportModeDto {
        self.mode
    }

    /// Returns the declared safe header name, when the transport is safe-header.
    #[must_use]
    pub fn safe_header_name(&self) -> Option<&str> {
        self.safe_header_name.as_deref()
    }
}

/// The closed reasoning fragment category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningFragmentCategoryDto {
    /// The main textual reasoning representation.
    Primary,
    /// A separate detailed reasoning representation.
    Detail,
}

/// The closed normalized reasoning effort levels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffortLevelDto {
    /// No reasoning effort is requested.
    None,
    /// The lowest declared reasoning effort.
    Minimal,
    /// A low reasoning effort.
    Low,
    /// A medium reasoning effort.
    Medium,
    /// A high reasoning effort.
    High,
    /// An extra-high reasoning effort.
    #[serde(rename = "xhigh")]
    XHigh,
    /// The highest declared reasoning effort.
    Max,
}

impl ReasoningEffortLevelDto {
    /// Returns the canonical durable string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// Parses the canonical durable string representation.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `value` is not a declared reasoning effort.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" => Ok(Self::XHigh),
            "max" => Ok(Self::Max),
            _ => Err(ErrorDto::validation(
                "invalid_reasoning_effort",
                "the reasoning effort level is not declared",
            )),
        }
    }
}

/// The closed cross-turn reasoning history transfer contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ReasoningHistoryTransferDto {
    /// No cross-turn reasoning history is transferred.
    Disabled,
    /// Textual reasoning history transfers under one compatibility identity.
    TextualHistoryV1 {
        /// The code-owned descriptor compatibility identity.
        compatibility_id: String,
    },
}

impl<'de> Deserialize<'de> for ReasoningHistoryTransferDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "mode", rename_all = "snake_case")]
        enum RawReasoningHistoryTransferDto {
            Disabled,
            TextualHistoryV1 { compatibility_id: String },
        }

        match RawReasoningHistoryTransferDto::deserialize(deserializer)? {
            RawReasoningHistoryTransferDto::Disabled => Ok(Self::Disabled),
            RawReasoningHistoryTransferDto::TextualHistoryV1 { compatibility_id } => {
                Self::textual_history_v1(compatibility_id).map_err(de::Error::custom)
            }
        }
    }
}

impl ReasoningHistoryTransferDto {
    /// Creates a textual reasoning history transfer contract.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `compatibility_id` is not a safe token.
    pub fn textual_history_v1(compatibility_id: impl Into<String>) -> DtoResult<Self> {
        let compatibility_id = compatibility_id.into();
        require_safe_token(
            &compatibility_id,
            "invalid_reasoning_history_transfer",
            "reasoning compatibility identity must be a non-blank safe token",
        )?;
        Ok(Self::TextualHistoryV1 { compatibility_id })
    }

    /// Returns the compatibility identity, when textual history transfers.
    #[must_use]
    pub fn compatibility_id(&self) -> Option<&str> {
        match self {
            Self::Disabled => None,
            Self::TextualHistoryV1 { compatibility_id } => Some(compatibility_id),
        }
    }

    /// Returns whether cross-turn reasoning history is disabled.
    #[must_use]
    pub const fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

/// The single version of the closed model capability taxonomy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ModelCapabilityTaxonomyVersionDto {
    /// The initial closed taxonomy version.
    #[serde(rename = "model-capability-taxonomy-v1")]
    ModelCapabilityTaxonomyV1,
}

impl ModelCapabilityTaxonomyVersionDto {
    /// Returns the current taxonomy version.
    #[must_use]
    pub const fn current() -> Self {
        Self::ModelCapabilityTaxonomyV1
    }

    /// Returns the canonical durable string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelCapabilityTaxonomyV1 => "model-capability-taxonomy-v1",
        }
    }
}

/// The closed model input kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelInputKindDto {
    /// The model accepts text input only.
    TextOnly,
}

/// The closed availability of one optional model capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapabilityAvailabilityDto {
    /// The capability is declared available.
    Enabled,
    /// The capability is declared unavailable.
    Disabled,
}

/// The closed reasoning capability declaration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReasoningCapabilityDto {
    /// The model declares no reasoning capability.
    Disabled,
    /// The model declares textual reasoning with its supported efforts.
    TextualReasoningV1 {
        /// The closed effort levels the model declares.
        supported_efforts: Vec<ReasoningEffortLevelDto>,
        /// Whether the model declares reasoning summary support.
        summary_support: bool,
    },
}

impl<'de> Deserialize<'de> for ReasoningCapabilityDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum RawReasoningCapabilityDto {
            Disabled,
            TextualReasoningV1 {
                supported_efforts: Vec<ReasoningEffortLevelDto>,
                summary_support: bool,
            },
        }

        match RawReasoningCapabilityDto::deserialize(deserializer)? {
            RawReasoningCapabilityDto::Disabled => Ok(Self::Disabled),
            RawReasoningCapabilityDto::TextualReasoningV1 {
                supported_efforts,
                summary_support,
            } => Self::textual_reasoning_v1(supported_efforts, summary_support)
                .map_err(de::Error::custom),
        }
    }
}

impl ReasoningCapabilityDto {
    /// Creates a textual reasoning capability declaration.
    ///
    /// # Errors
    ///
    /// Returns a validation error when an effort level is declared twice.
    pub fn textual_reasoning_v1(
        supported_efforts: Vec<ReasoningEffortLevelDto>,
        summary_support: bool,
    ) -> DtoResult<Self> {
        let mut unique = BTreeSet::new();
        if !supported_efforts
            .iter()
            .all(|effort| unique.insert(*effort))
        {
            return Err(ErrorDto::validation(
                "invalid_model_capability_set",
                "supported reasoning efforts must not repeat",
            ));
        }
        Ok(Self::TextualReasoningV1 {
            supported_efforts,
            summary_support,
        })
    }

    /// Returns whether this declaration supports `effort`.
    #[must_use]
    pub fn supports_effort(&self, effort: ReasoningEffortLevelDto) -> bool {
        match self {
            Self::Disabled => false,
            Self::TextualReasoningV1 {
                supported_efforts, ..
            } => supported_efforts.contains(&effort),
        }
    }

    /// Returns whether this declaration supports reasoning summaries.
    #[must_use]
    pub const fn supports_summary(&self) -> bool {
        match self {
            Self::Disabled => false,
            Self::TextualReasoningV1 {
                summary_support, ..
            } => *summary_support,
        }
    }
}

/// The closed tool-exchange capability declaration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolExchangeCapabilityDto {
    /// The model declares no tool exchange.
    Disabled,
    /// The model declares the local model tool loop.
    ModelToolLoopV1 {
        /// The code-owned translation revision for this tool loop.
        translation_revision: String,
    },
}

impl<'de> Deserialize<'de> for ToolExchangeCapabilityDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum RawToolExchangeCapabilityDto {
            Disabled,
            ModelToolLoopV1 { translation_revision: String },
        }

        match RawToolExchangeCapabilityDto::deserialize(deserializer)? {
            RawToolExchangeCapabilityDto::Disabled => Ok(Self::Disabled),
            RawToolExchangeCapabilityDto::ModelToolLoopV1 {
                translation_revision,
            } => Self::model_tool_loop_v1(translation_revision).map_err(de::Error::custom),
        }
    }
}

impl ToolExchangeCapabilityDto {
    /// Creates a model tool loop capability declaration.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `translation_revision` is not a safe token.
    pub fn model_tool_loop_v1(translation_revision: impl Into<String>) -> DtoResult<Self> {
        let translation_revision = translation_revision.into();
        require_safe_token(
            &translation_revision,
            "invalid_model_capability_set",
            "the tool-loop translation revision must be a non-blank safe token",
        )?;
        Ok(Self::ModelToolLoopV1 {
            translation_revision,
        })
    }

    /// Returns the declared translation revision, when a tool loop is declared.
    #[must_use]
    pub fn translation_revision(&self) -> Option<&str> {
        match self {
            Self::Disabled => None,
            Self::ModelToolLoopV1 {
                translation_revision,
            } => Some(translation_revision),
        }
    }
}

/// The closed context-preservation capability declaration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextPreservationCapabilityDto {
    /// Local durable history with its declared reasoning input contract.
    LocalDurableHistoryV1 {
        /// The reasoning input contract this capability preserves.
        reasoning_input_contract: ReasoningHistoryTransferDto,
    },
}

impl ContextPreservationCapabilityDto {
    /// Creates a local durable history capability declaration.
    #[must_use]
    pub const fn local_durable_history_v1(
        reasoning_input_contract: ReasoningHistoryTransferDto,
    ) -> Self {
        Self::LocalDurableHistoryV1 {
            reasoning_input_contract,
        }
    }

    /// Returns the declared reasoning input contract.
    #[must_use]
    pub const fn reasoning_input_contract(&self) -> &ReasoningHistoryTransferDto {
        let Self::LocalDurableHistoryV1 {
            reasoning_input_contract,
        } = self;
        reasoning_input_contract
    }
}

/// The closed model capability set of the current taxonomy version.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelCapabilitySetV1 {
    taxonomy_version: ModelCapabilityTaxonomyVersionDto,
    input: ModelInputKindDto,
    text_streaming: ProviderCapabilityAvailabilityDto,
    structured_output: ProviderCapabilityAvailabilityDto,
    reasoning: ReasoningCapabilityDto,
    tool_exchange: ToolExchangeCapabilityDto,
    context_preservation: ContextPreservationCapabilityDto,
}

impl<'de> Deserialize<'de> for ModelCapabilitySetV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawModelCapabilitySetV1 {
            taxonomy_version: ModelCapabilityTaxonomyVersionDto,
            input: ModelInputKindDto,
            text_streaming: ProviderCapabilityAvailabilityDto,
            structured_output: ProviderCapabilityAvailabilityDto,
            reasoning: ReasoningCapabilityDto,
            tool_exchange: ToolExchangeCapabilityDto,
            context_preservation: ContextPreservationCapabilityDto,
        }

        let raw = RawModelCapabilitySetV1::deserialize(deserializer)?;
        Self::new(
            raw.taxonomy_version,
            raw.input,
            raw.text_streaming,
            raw.structured_output,
            raw.reasoning,
            raw.tool_exchange,
            raw.context_preservation,
        )
        .map_err(de::Error::custom)
    }
}

impl ModelCapabilitySetV1 {
    /// Creates one coherent capability set of the current taxonomy version.
    ///
    /// Structured output and non-text input require a new taxonomy version, so
    /// the current version declares structured output disabled.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the declaration is not representable by
    /// the current taxonomy version.
    pub fn new(
        taxonomy_version: ModelCapabilityTaxonomyVersionDto,
        input: ModelInputKindDto,
        text_streaming: ProviderCapabilityAvailabilityDto,
        structured_output: ProviderCapabilityAvailabilityDto,
        reasoning: ReasoningCapabilityDto,
        tool_exchange: ToolExchangeCapabilityDto,
        context_preservation: ContextPreservationCapabilityDto,
    ) -> DtoResult<Self> {
        if structured_output != ProviderCapabilityAvailabilityDto::Disabled {
            return Err(ErrorDto::validation(
                "invalid_model_capability_set",
                "structured output requires a new capability taxonomy version",
            ));
        }
        Ok(Self {
            taxonomy_version,
            input,
            text_streaming,
            structured_output,
            reasoning,
            tool_exchange,
            context_preservation,
        })
    }

    /// Returns the closed taxonomy version.
    #[must_use]
    pub const fn taxonomy_version(&self) -> ModelCapabilityTaxonomyVersionDto {
        self.taxonomy_version
    }

    /// Returns the closed model input kind.
    #[must_use]
    pub const fn input(&self) -> ModelInputKindDto {
        self.input
    }

    /// Returns the text streaming availability.
    #[must_use]
    pub const fn text_streaming(&self) -> ProviderCapabilityAvailabilityDto {
        self.text_streaming
    }

    /// Returns the structured output availability.
    #[must_use]
    pub const fn structured_output(&self) -> ProviderCapabilityAvailabilityDto {
        self.structured_output
    }

    /// Returns the reasoning capability declaration.
    #[must_use]
    pub const fn reasoning(&self) -> &ReasoningCapabilityDto {
        &self.reasoning
    }

    /// Returns the tool-exchange capability declaration.
    #[must_use]
    pub const fn tool_exchange(&self) -> &ToolExchangeCapabilityDto {
        &self.tool_exchange
    }

    /// Returns the context-preservation capability declaration.
    #[must_use]
    pub const fn context_preservation(&self) -> &ContextPreservationCapabilityDto {
        &self.context_preservation
    }

    /// Returns whether the declared model supports `effort`.
    #[must_use]
    pub fn supports_effort(&self, effort: ReasoningEffortLevelDto) -> bool {
        self.reasoning.supports_effort(effort)
    }

    /// Returns whether the declared model supports reasoning summaries.
    #[must_use]
    pub const fn supports_summary(&self) -> bool {
        self.reasoning.supports_summary()
    }

    /// Returns the declared reasoning input contract.
    #[must_use]
    pub const fn reasoning_input_contract(&self) -> &ReasoningHistoryTransferDto {
        self.context_preservation.reasoning_input_contract()
    }
}

/// A code-owned driver contract revision (`family + major.minor`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderDriverContractRevisionDto {
    driver_family: String,
    major: u16,
    minor: u16,
}

impl<'de> Deserialize<'de> for ProviderDriverContractRevisionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderDriverContractRevisionDto {
            driver_family: String,
            major: u16,
            minor: u16,
        }

        let raw = RawProviderDriverContractRevisionDto::deserialize(deserializer)?;
        Self::new(raw.driver_family, raw.major, raw.minor).map_err(de::Error::custom)
    }
}

impl ProviderDriverContractRevisionDto {
    /// Creates a driver contract revision for one code-owned driver family.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `driver_family` is not a safe token.
    pub fn new(driver_family: impl Into<String>, major: u16, minor: u16) -> DtoResult<Self> {
        let driver_family = driver_family.into();
        require_safe_token(
            &driver_family,
            "invalid_provider_driver_contract_revision",
            "driver contract family must be a non-blank safe token",
        )?;
        Ok(Self {
            driver_family,
            major,
            minor,
        })
    }

    /// Returns the code-owned driver family.
    #[must_use]
    pub fn driver_family(&self) -> &str {
        &self.driver_family
    }

    /// Returns the incompatible-on-change contract component.
    #[must_use]
    pub const fn major(&self) -> u16 {
        self.major
    }

    /// Returns the compatible-on-change contract component.
    #[must_use]
    pub const fn minor(&self) -> u16 {
        self.minor
    }
}

/// The endpoint requirements and permissions of one provider kind descriptor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderEndpointPolicyDto {
    requires_explicit_endpoint: bool,
    permits_override: bool,
    permits_loopback_http: bool,
}

impl ProviderEndpointPolicyDto {
    /// Creates an explicit endpoint policy declaration.
    #[must_use]
    pub const fn new(
        requires_explicit_endpoint: bool,
        permits_override: bool,
        permits_loopback_http: bool,
    ) -> Self {
        Self {
            requires_explicit_endpoint,
            permits_override,
            permits_loopback_http,
        }
    }

    /// Returns whether a profile of this kind must declare an endpoint.
    #[must_use]
    pub const fn requires_explicit_endpoint(self) -> bool {
        self.requires_explicit_endpoint
    }

    /// Returns whether a profile of this kind may override the default endpoint.
    #[must_use]
    pub const fn permits_override(self) -> bool {
        self.permits_override
    }

    /// Returns whether a profile of this kind may use an explicit loopback HTTP endpoint.
    #[must_use]
    pub const fn permits_loopback_http(self) -> bool {
        self.permits_loopback_http
    }
}

/// The credential transports one provider kind descriptor supports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CredentialTransportContractDto {
    supported: Vec<CredentialTransportModeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    safe_header_name: Option<String>,
}

impl<'de> Deserialize<'de> for CredentialTransportContractDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawCredentialTransportContractDto {
            supported: Vec<CredentialTransportModeDto>,
            #[serde(default)]
            safe_header_name: Option<String>,
        }

        let raw = RawCredentialTransportContractDto::deserialize(deserializer)?;
        Self::new(raw.supported, raw.safe_header_name).map_err(de::Error::custom)
    }
}

impl CredentialTransportContractDto {
    /// Creates a validated credential transport contract.
    ///
    /// The contract declares at least one transport, never repeats a transport,
    /// and names exactly one safe header when safe-header transport is supported.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the declaration is incoherent.
    pub fn new(
        supported: Vec<CredentialTransportModeDto>,
        safe_header_name: Option<String>,
    ) -> DtoResult<Self> {
        let mut unique = BTreeSet::new();
        if supported.is_empty() || !supported.iter().all(|mode| unique.insert(*mode)) {
            return Err(ErrorDto::validation(
                "invalid_credential_transport_contract",
                "a credential transport contract declares at least one transport without repeats",
            ));
        }
        if safe_header_name
            .as_deref()
            .is_some_and(|name| !is_http_header_name_token(name))
        {
            return Err(ErrorDto::validation(
                "invalid_credential_transport_contract",
                "the declared safe header name must be a valid HTTP header name",
            ));
        }
        if supported.contains(&CredentialTransportModeDto::SafeHeader) == safe_header_name.is_none()
        {
            return Err(ErrorDto::validation(
                "invalid_credential_transport_contract",
                "safe-header transport support and one declared header name must agree",
            ));
        }
        Ok(Self {
            supported,
            safe_header_name,
        })
    }

    /// Returns the supported credential transport modes.
    #[must_use]
    pub fn supported(&self) -> &[CredentialTransportModeDto] {
        &self.supported
    }

    /// Returns the declared safe header name, when safe-header transport is supported.
    #[must_use]
    pub fn safe_header_name(&self) -> Option<&str> {
        self.safe_header_name.as_deref()
    }

    /// Returns whether this contract supports `mode`.
    #[must_use]
    pub fn supports(&self, mode: CredentialTransportModeDto) -> bool {
        self.supported.contains(&mode)
    }
}

/// One immutable, credential-free provider kind descriptor revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderKindDescriptorRevisionV1 {
    kind_id: ProviderKindId,
    descriptor_revision_id: ProviderKindDescriptorRevisionId,
    descriptor_family: String,
    ordered_protocol_part_revisions: Vec<String>,
    endpoint_policy: ProviderEndpointPolicyDto,
    credential_transport_contract: CredentialTransportContractDto,
    model_capability_envelope: ModelCapabilitySetV1,
    driver_contract_family: String,
}

impl<'de> Deserialize<'de> for ProviderKindDescriptorRevisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderKindDescriptorRevisionV1 {
            kind_id: ProviderKindId,
            descriptor_revision_id: ProviderKindDescriptorRevisionId,
            descriptor_family: String,
            ordered_protocol_part_revisions: Vec<String>,
            endpoint_policy: ProviderEndpointPolicyDto,
            credential_transport_contract: CredentialTransportContractDto,
            model_capability_envelope: ModelCapabilitySetV1,
            driver_contract_family: String,
        }

        let raw = RawProviderKindDescriptorRevisionV1::deserialize(deserializer)?;
        Self::new(
            raw.kind_id,
            raw.descriptor_revision_id,
            raw.descriptor_family,
            raw.ordered_protocol_part_revisions,
            raw.endpoint_policy,
            raw.credential_transport_contract,
            raw.model_capability_envelope,
            raw.driver_contract_family,
        )
        .map_err(de::Error::custom)
    }
}

impl ProviderKindDescriptorRevisionV1 {
    /// Creates one validated provider kind descriptor revision.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a family or protocol part revision token
    /// is blank, unsafe, or repeated.
    #[expect(
        clippy::too_many_arguments,
        reason = "This wire constructor preserves the frozen eight-field kind descriptor revision contract."
    )]
    pub fn new(
        kind_id: ProviderKindId,
        descriptor_revision_id: ProviderKindDescriptorRevisionId,
        descriptor_family: impl Into<String>,
        ordered_protocol_part_revisions: Vec<String>,
        endpoint_policy: ProviderEndpointPolicyDto,
        credential_transport_contract: CredentialTransportContractDto,
        model_capability_envelope: ModelCapabilitySetV1,
        driver_contract_family: impl Into<String>,
    ) -> DtoResult<Self> {
        let descriptor_family = descriptor_family.into();
        let driver_contract_family = driver_contract_family.into();
        require_safe_token(
            &descriptor_family,
            "invalid_provider_kind_descriptor_revision",
            "descriptor family must be a non-blank safe token",
        )?;
        require_safe_token(
            &driver_contract_family,
            "invalid_provider_kind_descriptor_revision",
            "driver contract family must be a non-blank safe token",
        )?;
        let mut unique = BTreeSet::new();
        if !ordered_protocol_part_revisions
            .iter()
            .all(|part| is_safe_token(part) && unique.insert(part.clone()))
        {
            return Err(ErrorDto::validation(
                "invalid_provider_kind_descriptor_revision",
                "protocol part revisions must be non-blank safe tokens without repeats",
            ));
        }
        Ok(Self {
            kind_id,
            descriptor_revision_id,
            descriptor_family,
            ordered_protocol_part_revisions,
            endpoint_policy,
            credential_transport_contract,
            model_capability_envelope,
            driver_contract_family,
        })
    }

    /// Returns the provider kind identity this descriptor describes.
    #[must_use]
    pub const fn kind_id(&self) -> &ProviderKindId {
        &self.kind_id
    }

    /// Returns the immutable descriptor revision identity.
    #[must_use]
    pub const fn descriptor_revision_id(&self) -> ProviderKindDescriptorRevisionId {
        self.descriptor_revision_id
    }

    /// Returns the code-owned descriptor family.
    #[must_use]
    pub fn descriptor_family(&self) -> &str {
        &self.descriptor_family
    }

    /// Returns the ordered protocol part revisions.
    #[must_use]
    pub fn ordered_protocol_part_revisions(&self) -> &[String] {
        &self.ordered_protocol_part_revisions
    }

    /// Returns the endpoint policy declaration.
    #[must_use]
    pub const fn endpoint_policy(&self) -> ProviderEndpointPolicyDto {
        self.endpoint_policy
    }

    /// Returns the credential transport contract.
    #[must_use]
    pub const fn credential_transport_contract(&self) -> &CredentialTransportContractDto {
        &self.credential_transport_contract
    }

    /// Returns the maximum model capability envelope of this kind.
    #[must_use]
    pub const fn model_capability_envelope(&self) -> &ModelCapabilitySetV1 {
        &self.model_capability_envelope
    }

    /// Returns the code-owned driver contract family.
    #[must_use]
    pub fn driver_contract_family(&self) -> &str {
        &self.driver_contract_family
    }
}

/// The closed loopback policy of one profile endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopbackPolicyDto {
    /// Loopback HTTP is not applicable to this profile.
    NotApplicable,
    /// An explicit loopback HTTP endpoint is permitted.
    ExplicitLoopback,
}

/// The safe per-run provider execution policy.
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
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawProviderExecutionPolicyDto {
            #[serde(default)]
            attempt_timeout_seconds: Option<u8>,
            #[serde(default)]
            max_attempts: Option<u8>,
        }

        let raw = RawProviderExecutionPolicyDto::deserialize(deserializer)?;
        Self::new(
            raw.attempt_timeout_seconds.unwrap_or(30),
            raw.max_attempts.unwrap_or(2),
        )
        .map_err(de::Error::custom)
    }
}

impl ProviderExecutionPolicyDto {
    /// Creates a validated provider execution policy.
    ///
    /// # Errors
    ///
    /// Returns a validation error unless the attempt timeout is between 1 and 60
    /// seconds and the attempt budget is between 1 and 2.
    pub fn new(attempt_timeout_seconds: u8, max_attempts: u8) -> DtoResult<Self> {
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

    /// Returns the timeout for one provider attempt in seconds.
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

/// The resolved reasoning policy of one profile revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResolvedReasoningPolicyDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effort: Option<ReasoningEffortLevelDto>,
    summary_support: bool,
    transfer: ReasoningHistoryTransferDto,
    supported_categories: Vec<ReasoningFragmentCategoryDto>,
}

impl<'de> Deserialize<'de> for ResolvedReasoningPolicyDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawResolvedReasoningPolicyDto {
            #[serde(default)]
            effort: Option<ReasoningEffortLevelDto>,
            summary_support: bool,
            transfer: ReasoningHistoryTransferDto,
            supported_categories: Vec<ReasoningFragmentCategoryDto>,
        }

        let raw = RawResolvedReasoningPolicyDto::deserialize(deserializer)?;
        Self::new(
            raw.effort,
            raw.summary_support,
            raw.transfer,
            raw.supported_categories,
        )
        .map_err(de::Error::custom)
    }
}

impl ResolvedReasoningPolicyDto {
    /// Creates one coherent resolved reasoning policy.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a fragment category repeats or a textual
    /// history transfer is declared without any transferable reasoning material.
    pub fn new(
        effort: Option<ReasoningEffortLevelDto>,
        summary_support: bool,
        transfer: ReasoningHistoryTransferDto,
        supported_categories: Vec<ReasoningFragmentCategoryDto>,
    ) -> DtoResult<Self> {
        let mut unique = BTreeSet::new();
        if !supported_categories
            .iter()
            .all(|category| unique.insert(*category))
        {
            return Err(ErrorDto::validation(
                "invalid_resolved_reasoning_policy",
                "supported reasoning categories must not repeat",
            ));
        }
        let is_empty_transfer = matches!(
            transfer,
            ReasoningHistoryTransferDto::TextualHistoryV1 { .. }
        ) && !summary_support
            && supported_categories.is_empty();
        if is_empty_transfer {
            return Err(ErrorDto::validation(
                "invalid_resolved_reasoning_policy",
                "a textual history transfer requires transferable reasoning material",
            ));
        }
        Ok(Self {
            effort,
            summary_support,
            transfer,
            supported_categories,
        })
    }

    /// Returns the selected effort, when the profile selects one.
    #[must_use]
    pub const fn effort(&self) -> Option<ReasoningEffortLevelDto> {
        self.effort
    }

    /// Returns whether reasoning summary support is selected.
    #[must_use]
    pub const fn summary_support(&self) -> bool {
        self.summary_support
    }

    /// Returns the selected cross-turn reasoning history transfer.
    #[must_use]
    pub const fn transfer(&self) -> &ReasoningHistoryTransferDto {
        &self.transfer
    }

    /// Returns the supported reasoning fragment categories.
    #[must_use]
    pub fn supported_categories(&self) -> &[ReasoningFragmentCategoryDto] {
        &self.supported_categories
    }
}

/// Optional product pricing policy for one profile.
///
/// Pricing is product policy, never an admission ceiling, quota, or entitlement.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ProviderPricingPolicyDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_per_million_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_per_million_tokens: Option<f64>,
}

/// Equality is reflexive because every stored price is validated finite.
impl Eq for ProviderPricingPolicyDto {}

impl<'de> Deserialize<'de> for ProviderPricingPolicyDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderPricingPolicyDto {
            #[serde(default)]
            input_per_million_tokens: Option<f64>,
            #[serde(default)]
            output_per_million_tokens: Option<f64>,
        }

        let raw = RawProviderPricingPolicyDto::deserialize(deserializer)?;
        Self::new(raw.input_per_million_tokens, raw.output_per_million_tokens)
            .map_err(de::Error::custom)
    }
}

impl ProviderPricingPolicyDto {
    /// Creates a validated pricing policy.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a declared price is not finite and
    /// non-negative.
    pub fn new(
        input_per_million_tokens: Option<f64>,
        output_per_million_tokens: Option<f64>,
    ) -> DtoResult<Self> {
        if [input_per_million_tokens, output_per_million_tokens]
            .into_iter()
            .flatten()
            .any(|price| !price.is_finite() || price < 0.0)
        {
            return Err(ErrorDto::validation(
                "invalid_provider_pricing_policy",
                "declared prices must be finite and non-negative",
            ));
        }
        Ok(Self {
            input_per_million_tokens,
            output_per_million_tokens,
        })
    }

    /// Returns the declared input price per million tokens.
    #[must_use]
    pub const fn input_per_million_tokens(&self) -> Option<f64> {
        self.input_per_million_tokens
    }

    /// Returns the declared output price per million tokens.
    #[must_use]
    pub const fn output_per_million_tokens(&self) -> Option<f64> {
        self.output_per_million_tokens
    }
}

/// One immutable, credential-free provider profile revision.
///
/// Display name, enabled state, and pricing are profile policy, not
/// revision-affecting meaning.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderProfileRevisionV1 {
    profile_id: ProviderProfileId,
    revision_id: ProviderProfileRevisionId,
    kind_id: ProviderKindId,
    kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
    model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normalized_effective_endpoint: Option<String>,
    credential_transport: CredentialTransportDto,
    declared_model_capability_subset: ModelCapabilitySetV1,
    resolved_reasoning_policy: ResolvedReasoningPolicyDto,
    effective_execution_policy: ProviderExecutionPolicyDto,
    effective_loopback_policy: LoopbackPolicyDto,
}

impl<'de> Deserialize<'de> for ProviderProfileRevisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderProfileRevisionV1 {
            profile_id: ProviderProfileId,
            revision_id: ProviderProfileRevisionId,
            kind_id: ProviderKindId,
            kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
            model_id: String,
            #[serde(default)]
            normalized_effective_endpoint: Option<String>,
            credential_transport: CredentialTransportDto,
            declared_model_capability_subset: ModelCapabilitySetV1,
            resolved_reasoning_policy: ResolvedReasoningPolicyDto,
            effective_execution_policy: ProviderExecutionPolicyDto,
            effective_loopback_policy: LoopbackPolicyDto,
        }

        let raw = RawProviderProfileRevisionV1::deserialize(deserializer)?;
        Self::new(
            raw.profile_id,
            raw.revision_id,
            raw.kind_id,
            raw.kind_descriptor_revision_id,
            raw.model_id,
            raw.normalized_effective_endpoint,
            raw.credential_transport,
            raw.declared_model_capability_subset,
            raw.resolved_reasoning_policy,
            raw.effective_execution_policy,
            raw.effective_loopback_policy,
        )
        .map_err(de::Error::custom)
    }
}

impl ProviderProfileRevisionV1 {
    /// Creates one validated provider profile revision.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the model identity is blank, the endpoint
    /// violates its loopback policy, or the resolved reasoning policy is not
    /// representable by the declared capability subset.
    #[expect(
        clippy::too_many_arguments,
        reason = "This wire constructor preserves the frozen eleven-field profile revision contract."
    )]
    pub fn new(
        profile_id: ProviderProfileId,
        revision_id: ProviderProfileRevisionId,
        kind_id: ProviderKindId,
        kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
        model_id: impl Into<String>,
        normalized_effective_endpoint: Option<String>,
        credential_transport: CredentialTransportDto,
        declared_model_capability_subset: ModelCapabilitySetV1,
        resolved_reasoning_policy: ResolvedReasoningPolicyDto,
        effective_execution_policy: ProviderExecutionPolicyDto,
        effective_loopback_policy: LoopbackPolicyDto,
    ) -> DtoResult<Self> {
        let model_id = model_id.into();
        if model_id.trim().is_empty() || model_id.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "invalid_provider_profile_revision",
                "model identity must be a non-blank control-free token",
            ));
        }
        if let Some(endpoint) = normalized_effective_endpoint.as_deref() {
            validate_endpoint(endpoint, effective_loopback_policy).map_err(|_| {
                ErrorDto::validation(
                    "invalid_provider_profile_revision",
                    "the normalized endpoint is not permitted by its loopback policy",
                )
            })?;
        }
        validate_reasoning_policy_against_capabilities(
            &resolved_reasoning_policy,
            &declared_model_capability_subset,
        )?;
        Ok(Self {
            profile_id,
            revision_id,
            kind_id,
            kind_descriptor_revision_id,
            model_id,
            normalized_effective_endpoint,
            credential_transport,
            declared_model_capability_subset,
            resolved_reasoning_policy,
            effective_execution_policy,
            effective_loopback_policy,
        })
    }

    /// Returns the stable profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the immutable profile revision identity.
    #[must_use]
    pub const fn revision_id(&self) -> ProviderProfileRevisionId {
        self.revision_id
    }

    /// Returns the provider kind identity.
    #[must_use]
    pub const fn kind_id(&self) -> &ProviderKindId {
        &self.kind_id
    }

    /// Returns the bound kind descriptor revision identity.
    #[must_use]
    pub const fn kind_descriptor_revision_id(&self) -> ProviderKindDescriptorRevisionId {
        self.kind_descriptor_revision_id
    }

    /// Returns the exact configured model identity.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the normalized effective endpoint, when the profile declares one.
    #[must_use]
    pub fn normalized_effective_endpoint(&self) -> Option<&str> {
        self.normalized_effective_endpoint.as_deref()
    }

    /// Returns the credential transport declaration.
    #[must_use]
    pub const fn credential_transport(&self) -> &CredentialTransportDto {
        &self.credential_transport
    }

    /// Returns the declared exact-model capability subset.
    #[must_use]
    pub const fn declared_model_capability_subset(&self) -> &ModelCapabilitySetV1 {
        &self.declared_model_capability_subset
    }

    /// Returns the resolved reasoning policy.
    #[must_use]
    pub const fn resolved_reasoning_policy(&self) -> &ResolvedReasoningPolicyDto {
        &self.resolved_reasoning_policy
    }

    /// Returns the effective execution policy.
    #[must_use]
    pub const fn effective_execution_policy(&self) -> ProviderExecutionPolicyDto {
        self.effective_execution_policy
    }

    /// Returns the effective loopback policy.
    #[must_use]
    pub const fn effective_loopback_policy(&self) -> LoopbackPolicyDto {
        self.effective_loopback_policy
    }
}

/// Safe profile policy that is not revision-affecting.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderProfilePolicyDto {
    display_name: String,
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pricing: Option<ProviderPricingPolicyDto>,
}

impl<'de> Deserialize<'de> for ProviderProfilePolicyDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderProfilePolicyDto {
            display_name: String,
            enabled: bool,
            #[serde(default)]
            pricing: Option<ProviderPricingPolicyDto>,
        }

        let raw = RawProviderProfilePolicyDto::deserialize(deserializer)?;
        Self::new(raw.display_name, raw.enabled, raw.pricing).map_err(de::Error::custom)
    }
}

impl ProviderProfilePolicyDto {
    /// Creates a validated profile policy.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the display name is blank or carries
    /// control characters.
    pub fn new(
        display_name: impl Into<String>,
        enabled: bool,
        pricing: Option<ProviderPricingPolicyDto>,
    ) -> DtoResult<Self> {
        let display_name = display_name.into();
        if display_name.trim().is_empty() || display_name.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "invalid_provider_profile_policy",
                "the profile display name must be non-blank and control-free",
            ));
        }
        Ok(Self {
            display_name,
            enabled,
            pricing,
        })
    }

    /// Returns the safe presentation display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns whether the profile is enabled.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the optional product pricing policy.
    #[must_use]
    pub const fn pricing(&self) -> Option<&ProviderPricingPolicyDto> {
        self.pricing.as_ref()
    }
}

/// The immutable provenance of one resolved provider selection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSelectionSourceDto {
    /// The global default profile selected the run.
    GlobalDefault,
    /// The durable session default selected the run.
    SessionDefault,
    /// An explicit per-turn override selected the run.
    TurnOverride,
}

/// The closed reason an exact selection is unavailable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSelectionUnavailabilityDto {
    /// The selected profile does not exist in the active catalog.
    Missing,
    /// The selected profile exists but is disabled.
    Disabled,
    /// The selected profile has no live private runtime entry.
    RuntimeUnavailable,
}

/// One immutable, credential-free resolved provider selection for a run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResolvedRunProviderSelectionDto {
    selection_contract_revision: u16,
    profile_id: ProviderProfileId,
    provider_profile_revision_id: ProviderProfileRevisionId,
    kind_id: ProviderKindId,
    kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
    model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normalized_effective_endpoint: Option<String>,
    credential_transport_mode: CredentialTransportModeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential_transport_safe_header_name: Option<String>,
    declared_model_capability_subset: ModelCapabilitySetV1,
    resolved_reasoning_policy: ResolvedReasoningPolicyDto,
    effective_execution_policy: ProviderExecutionPolicyDto,
    effective_loopback_policy: LoopbackPolicyDto,
    provider_driver_contract_revision: ProviderDriverContractRevisionDto,
    selection_source: ProviderSelectionSourceDto,
}

impl<'de> Deserialize<'de> for ResolvedRunProviderSelectionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawResolvedRunProviderSelectionDto {
            selection_contract_revision: u16,
            profile_id: ProviderProfileId,
            provider_profile_revision_id: ProviderProfileRevisionId,
            kind_id: ProviderKindId,
            kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
            model_id: String,
            #[serde(default)]
            normalized_effective_endpoint: Option<String>,
            credential_transport_mode: CredentialTransportModeDto,
            #[serde(default)]
            credential_transport_safe_header_name: Option<String>,
            declared_model_capability_subset: ModelCapabilitySetV1,
            resolved_reasoning_policy: ResolvedReasoningPolicyDto,
            effective_execution_policy: ProviderExecutionPolicyDto,
            effective_loopback_policy: LoopbackPolicyDto,
            provider_driver_contract_revision: ProviderDriverContractRevisionDto,
            selection_source: ProviderSelectionSourceDto,
        }

        let raw = RawResolvedRunProviderSelectionDto::deserialize(deserializer)?;
        Self::new(
            raw.selection_contract_revision,
            raw.profile_id,
            raw.provider_profile_revision_id,
            raw.kind_id,
            raw.kind_descriptor_revision_id,
            raw.model_id,
            raw.normalized_effective_endpoint,
            raw.credential_transport_mode,
            raw.credential_transport_safe_header_name,
            raw.declared_model_capability_subset,
            raw.resolved_reasoning_policy,
            raw.effective_execution_policy,
            raw.effective_loopback_policy,
            raw.provider_driver_contract_revision,
            raw.selection_source,
        )
        .map_err(de::Error::custom)
    }
}

impl ResolvedRunProviderSelectionDto {
    /// The current resolved-selection contract revision.
    pub const SELECTION_CONTRACT_REVISION: u16 = 1;

    /// Creates one validated resolved provider selection.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the model identity is blank, the endpoint
    /// violates its loopback policy, the credential transport is incoherent, or
    /// the reasoning policy is not representable by the capability subset.
    #[expect(
        clippy::too_many_arguments,
        reason = "This wire constructor preserves the frozen fifteen-field resolved selection contract."
    )]
    pub fn new(
        selection_contract_revision: u16,
        profile_id: ProviderProfileId,
        provider_profile_revision_id: ProviderProfileRevisionId,
        kind_id: ProviderKindId,
        kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
        model_id: impl Into<String>,
        normalized_effective_endpoint: Option<String>,
        credential_transport_mode: CredentialTransportModeDto,
        credential_transport_safe_header_name: Option<String>,
        declared_model_capability_subset: ModelCapabilitySetV1,
        resolved_reasoning_policy: ResolvedReasoningPolicyDto,
        effective_execution_policy: ProviderExecutionPolicyDto,
        effective_loopback_policy: LoopbackPolicyDto,
        provider_driver_contract_revision: ProviderDriverContractRevisionDto,
        selection_source: ProviderSelectionSourceDto,
    ) -> DtoResult<Self> {
        let model_id = model_id.into();
        if model_id.trim().is_empty() || model_id.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "invalid_resolved_run_provider_selection",
                "model identity must be a non-blank control-free token",
            ));
        }
        if let Some(endpoint) = normalized_effective_endpoint.as_deref() {
            validate_endpoint(endpoint, effective_loopback_policy).map_err(|_| {
                ErrorDto::validation(
                    "invalid_resolved_run_provider_selection",
                    "the normalized endpoint is not permitted by its loopback policy",
                )
            })?;
        }
        validate_credential_transport_fields(
            credential_transport_mode,
            credential_transport_safe_header_name.as_deref(),
        )
        .map_err(|_| {
            ErrorDto::validation(
                "invalid_resolved_run_provider_selection",
                "the credential transport declaration is incoherent",
            )
        })?;
        validate_reasoning_policy_against_capabilities(
            &resolved_reasoning_policy,
            &declared_model_capability_subset,
        )
        .map_err(|_| {
            ErrorDto::validation(
                "invalid_resolved_run_provider_selection",
                "the resolved reasoning policy is not representable by the capability subset",
            )
        })?;
        Ok(Self {
            selection_contract_revision,
            profile_id,
            provider_profile_revision_id,
            kind_id,
            kind_descriptor_revision_id,
            model_id,
            normalized_effective_endpoint,
            credential_transport_mode,
            credential_transport_safe_header_name,
            declared_model_capability_subset,
            resolved_reasoning_policy,
            effective_execution_policy,
            effective_loopback_policy,
            provider_driver_contract_revision,
            selection_source,
        })
    }

    /// Creates the current selection contract revision from one profile revision.
    ///
    /// The profile revision is already validated, so the derived selection is
    /// coherent by construction; only the driver contract revision and the
    /// immutable selection source are supplied by the caller.
    #[must_use]
    pub fn from_profile_revision(
        revision: &ProviderProfileRevisionV1,
        provider_driver_contract_revision: ProviderDriverContractRevisionDto,
        selection_source: ProviderSelectionSourceDto,
    ) -> Self {
        Self {
            selection_contract_revision: Self::SELECTION_CONTRACT_REVISION,
            profile_id: revision.profile_id().clone(),
            provider_profile_revision_id: revision.revision_id(),
            kind_id: revision.kind_id().clone(),
            kind_descriptor_revision_id: revision.kind_descriptor_revision_id(),
            model_id: revision.model_id().to_owned(),
            normalized_effective_endpoint: revision
                .normalized_effective_endpoint()
                .map(str::to_owned),
            credential_transport_mode: revision.credential_transport().mode(),
            credential_transport_safe_header_name: revision
                .credential_transport()
                .safe_header_name()
                .map(str::to_owned),
            declared_model_capability_subset: revision.declared_model_capability_subset().clone(),
            resolved_reasoning_policy: revision.resolved_reasoning_policy().clone(),
            effective_execution_policy: revision.effective_execution_policy(),
            effective_loopback_policy: revision.effective_loopback_policy(),
            provider_driver_contract_revision,
            selection_source,
        }
    }

    /// Returns the resolved-selection contract revision.
    #[must_use]
    pub const fn selection_contract_revision(&self) -> u16 {
        self.selection_contract_revision
    }

    /// Returns the selected profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the selected profile revision identity.
    #[must_use]
    pub const fn provider_profile_revision_id(&self) -> ProviderProfileRevisionId {
        self.provider_profile_revision_id
    }

    /// Returns the selected provider kind identity.
    #[must_use]
    pub const fn kind_id(&self) -> &ProviderKindId {
        &self.kind_id
    }

    /// Returns the bound kind descriptor revision identity.
    #[must_use]
    pub const fn kind_descriptor_revision_id(&self) -> ProviderKindDescriptorRevisionId {
        self.kind_descriptor_revision_id
    }

    /// Returns the exact selected model identity.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the normalized effective endpoint, when one applies.
    #[must_use]
    pub fn normalized_effective_endpoint(&self) -> Option<&str> {
        self.normalized_effective_endpoint.as_deref()
    }

    /// Returns the selected credential transport mode.
    #[must_use]
    pub const fn credential_transport_mode(&self) -> CredentialTransportModeDto {
        self.credential_transport_mode
    }

    /// Returns the selected safe header name, when safe-header transport applies.
    #[must_use]
    pub fn credential_transport_safe_header_name(&self) -> Option<&str> {
        self.credential_transport_safe_header_name.as_deref()
    }

    /// Returns the selected exact-model capability subset.
    #[must_use]
    pub const fn declared_model_capability_subset(&self) -> &ModelCapabilitySetV1 {
        &self.declared_model_capability_subset
    }

    /// Returns the selected resolved reasoning policy.
    #[must_use]
    pub const fn resolved_reasoning_policy(&self) -> &ResolvedReasoningPolicyDto {
        &self.resolved_reasoning_policy
    }

    /// Returns the selected effective execution policy.
    #[must_use]
    pub const fn effective_execution_policy(&self) -> ProviderExecutionPolicyDto {
        self.effective_execution_policy
    }

    /// Returns the selected effective loopback policy.
    #[must_use]
    pub const fn effective_loopback_policy(&self) -> LoopbackPolicyDto {
        self.effective_loopback_policy
    }

    /// Returns the selected driver contract revision.
    #[must_use]
    pub const fn provider_driver_contract_revision(&self) -> &ProviderDriverContractRevisionDto {
        &self.provider_driver_contract_revision
    }

    /// Returns the immutable provenance of this selection.
    #[must_use]
    pub const fn selection_source(&self) -> ProviderSelectionSourceDto {
        self.selection_source
    }
}

/// One ordered reasoning-record reference of a history source entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ReasoningHistoryRecordReferenceDto {
    category: ReasoningFragmentCategoryDto,
    size_bytes: u64,
}

impl<'de> Deserialize<'de> for ReasoningHistoryRecordReferenceDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawReasoningHistoryRecordReferenceDto {
            category: ReasoningFragmentCategoryDto,
            size_bytes: u64,
        }

        let raw = RawReasoningHistoryRecordReferenceDto::deserialize(deserializer)?;
        Self::new(raw.category, raw.size_bytes).map_err(de::Error::custom)
    }
}

impl ReasoningHistoryRecordReferenceDto {
    /// Creates one reasoning-record reference.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the referenced record size is zero.
    pub fn new(category: ReasoningFragmentCategoryDto, size_bytes: u64) -> DtoResult<Self> {
        if size_bytes == 0 {
            return Err(ErrorDto::validation(
                "invalid_reasoning_history_manifest",
                "a reasoning-record reference must carry a positive size",
            ));
        }
        Ok(Self {
            category,
            size_bytes,
        })
    }

    /// Returns the referenced fragment category.
    #[must_use]
    pub const fn category(self) -> ReasoningFragmentCategoryDto {
        self.category
    }

    /// Returns the referenced record size in bytes.
    #[must_use]
    pub const fn size_bytes(self) -> u64 {
        self.size_bytes
    }
}

/// One immutable source-response reference of a reasoning history manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReasoningHistorySourceEntryDto {
    session_id: SessionId,
    run_id: RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    final_assistant_message_id: Option<i64>,
    records: Vec<ReasoningHistoryRecordReferenceDto>,
}

impl ReasoningHistorySourceEntryDto {
    /// Creates one source-response reference with its ordered reasoning records.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        final_assistant_message_id: Option<i64>,
        records: Vec<ReasoningHistoryRecordReferenceDto>,
    ) -> Self {
        Self {
            session_id,
            run_id,
            final_assistant_message_id,
            records,
        }
    }

    /// Returns the source session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the source run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the final assistant message identity, when one exists.
    #[must_use]
    pub const fn final_assistant_message_id(&self) -> Option<i64> {
        self.final_assistant_message_id
    }

    /// Returns the ordered reasoning-record references.
    #[must_use]
    pub fn records(&self) -> &[ReasoningHistoryRecordReferenceDto] {
        &self.records
    }
}

/// One immutable reasoning history manifest binding a dependent run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReasoningHistoryManifestDto {
    manifest_id: ReasoningHistoryManifestId,
    transfer: ReasoningHistoryTransferDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compatibility_id: Option<String>,
    sources: Vec<ReasoningHistorySourceEntryDto>,
    aggregate_size_bytes: u64,
}

impl<'de> Deserialize<'de> for ReasoningHistoryManifestDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawReasoningHistoryManifestDto {
            manifest_id: ReasoningHistoryManifestId,
            transfer: ReasoningHistoryTransferDto,
            #[serde(default)]
            compatibility_id: Option<String>,
            sources: Vec<ReasoningHistorySourceEntryDto>,
            aggregate_size_bytes: u64,
        }

        let raw = RawReasoningHistoryManifestDto::deserialize(deserializer)?;
        Self::new(
            raw.manifest_id,
            raw.transfer,
            raw.compatibility_id,
            raw.sources,
            raw.aggregate_size_bytes,
        )
        .map_err(de::Error::custom)
    }
}

impl ReasoningHistoryManifestDto {
    /// Creates one validated reasoning history manifest.
    ///
    /// The manifest compatibility identity matches its transfer contract: a
    /// textual transfer carries the transfer's identity, and a disabled transfer
    /// carries none.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the compatibility identity does not match
    /// the transfer contract.
    pub fn new(
        manifest_id: ReasoningHistoryManifestId,
        transfer: ReasoningHistoryTransferDto,
        compatibility_id: Option<String>,
        sources: Vec<ReasoningHistorySourceEntryDto>,
        aggregate_size_bytes: u64,
    ) -> DtoResult<Self> {
        validate_history_compatibility(&transfer, compatibility_id.as_deref())?;
        Ok(Self {
            manifest_id,
            transfer,
            compatibility_id,
            sources,
            aggregate_size_bytes,
        })
    }

    /// Returns the manifest identity.
    #[must_use]
    pub const fn manifest_id(&self) -> ReasoningHistoryManifestId {
        self.manifest_id
    }

    /// Returns the transfer contract this manifest carries.
    #[must_use]
    pub const fn transfer(&self) -> &ReasoningHistoryTransferDto {
        &self.transfer
    }

    /// Returns the manifest compatibility identity, when one transfers.
    #[must_use]
    pub fn compatibility_id(&self) -> Option<&str> {
        self.compatibility_id.as_deref()
    }

    /// Returns the ordered source-response references.
    #[must_use]
    pub fn sources(&self) -> &[ReasoningHistorySourceEntryDto] {
        &self.sources
    }

    /// Returns the aggregate size of the referenced reasoning material in bytes.
    #[must_use]
    pub const fn aggregate_size_bytes(&self) -> u64 {
        self.aggregate_size_bytes
    }
}

/// Validates that a history compatibility identity matches its transfer contract.
fn validate_history_compatibility(
    transfer: &ReasoningHistoryTransferDto,
    compatibility_id: Option<&str>,
) -> DtoResult<()> {
    let coherent = transfer.compatibility_id().map_or_else(
        || compatibility_id.is_none(),
        |declared| compatibility_id == Some(declared),
    );
    if coherent {
        Ok(())
    } else {
        Err(ErrorDto::validation(
            "invalid_reasoning_history_manifest",
            "the history compatibility identity must match the transfer contract",
        ))
    }
}

/// One closed durable audit record accompanying a reasoning history manifest.
///
/// The bound carries no reasoning text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReasoningHistoryBoundDto {
    manifest_id: ReasoningHistoryManifestId,
    transfer: ReasoningHistoryTransferDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compatibility_id: Option<String>,
    source_entry_count: u32,
    aggregate_size_bytes: u64,
}

impl<'de> Deserialize<'de> for ReasoningHistoryBoundDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawReasoningHistoryBoundDto {
            manifest_id: ReasoningHistoryManifestId,
            transfer: ReasoningHistoryTransferDto,
            #[serde(default)]
            compatibility_id: Option<String>,
            source_entry_count: u32,
            aggregate_size_bytes: u64,
        }

        let raw = RawReasoningHistoryBoundDto::deserialize(deserializer)?;
        Self::new(
            raw.manifest_id,
            raw.transfer,
            raw.compatibility_id,
            raw.source_entry_count,
            raw.aggregate_size_bytes,
        )
        .map_err(de::Error::custom)
    }
}

impl ReasoningHistoryBoundDto {
    /// Creates one validated reasoning history bound audit record.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the compatibility identity does not match
    /// the transfer contract.
    pub fn new(
        manifest_id: ReasoningHistoryManifestId,
        transfer: ReasoningHistoryTransferDto,
        compatibility_id: Option<String>,
        source_entry_count: u32,
        aggregate_size_bytes: u64,
    ) -> DtoResult<Self> {
        validate_history_compatibility(&transfer, compatibility_id.as_deref())?;
        Ok(Self {
            manifest_id,
            transfer,
            compatibility_id,
            source_entry_count,
            aggregate_size_bytes,
        })
    }

    /// Returns the bound manifest identity.
    #[must_use]
    pub const fn manifest_id(&self) -> ReasoningHistoryManifestId {
        self.manifest_id
    }

    /// Returns the recorded transfer contract.
    #[must_use]
    pub const fn transfer(&self) -> &ReasoningHistoryTransferDto {
        &self.transfer
    }

    /// Returns the recorded compatibility identity, when one transfers.
    #[must_use]
    pub fn compatibility_id(&self) -> Option<&str> {
        self.compatibility_id.as_deref()
    }

    /// Returns the recorded source-entry count.
    #[must_use]
    pub const fn source_entry_count(&self) -> u32 {
        self.source_entry_count
    }

    /// Returns the recorded aggregate size in bytes.
    #[must_use]
    pub const fn aggregate_size_bytes(&self) -> u64 {
        self.aggregate_size_bytes
    }
}

/// The closed stream-framing part of a user kind composition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKindStreamPartDto {
    /// Chat Completions server-sent-event framing.
    ///
    /// Explicit native streaming framings (including Ollama-native framing) are
    /// owned by dedicated first-party descriptors, not by user kinds.
    ChatCompletionsSse,
}

/// The closed textual reasoning field part of a user kind composition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKindReasoningPartDto {
    /// The composition declares no textual reasoning field.
    Absent,
    /// The textual reasoning field is `reasoning_content`.
    ReasoningContent,
    /// The textual reasoning field is `reasoning`.
    Reasoning,
    /// The textual reasoning field is `reasoning_details[].text`.
    ReasoningDetailsText,
    /// The textual reasoning field is `message.thinking`.
    MessageThinking,
}

/// The closed thinking activation part of a user kind composition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKindActivationPartDto {
    /// The composition declares no thinking activation field.
    Absent,
    /// Activation uses a `thinking` field with the closed `enabled` value.
    ThinkingEnabled,
    /// Activation uses a `thinking` field with the closed `adaptive` value.
    ThinkingAdaptive,
    /// Activation uses the `enable_thinking` boolean field.
    EnableThinking,
    /// Activation uses the `think` boolean field.
    ThinkBoolean,
    /// Activation uses the `think` field with a supported closed effort value.
    ThinkEffort,
}

/// The closed effort field part of a user kind composition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKindEffortPartDto {
    /// The composition declares no effort field.
    Absent,
    /// The closed effort field is `reasoning_effort`.
    ReasoningEffort,
    /// The closed effort field is `thinking_budget`.
    ThinkingBudget,
    /// The closed effort field is `thinking_token_budget`.
    ThinkingTokenBudget,
}

/// One immutable composition of closed protocol parts declared as a user kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UserKindCompositionDto {
    kind_id: ProviderKindId,
    stream: UserKindStreamPartDto,
    reasoning: UserKindReasoningPartDto,
    activation: UserKindActivationPartDto,
    effort: UserKindEffortPartDto,
    credential_transport: CredentialTransportDto,
}

impl<'de> Deserialize<'de> for UserKindCompositionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawUserKindCompositionDto {
            kind_id: ProviderKindId,
            stream: UserKindStreamPartDto,
            reasoning: UserKindReasoningPartDto,
            activation: UserKindActivationPartDto,
            effort: UserKindEffortPartDto,
            credential_transport: CredentialTransportDto,
        }

        let raw = RawUserKindCompositionDto::deserialize(deserializer)?;
        Self::new(
            raw.kind_id,
            raw.stream,
            raw.reasoning,
            raw.activation,
            raw.effort,
            raw.credential_transport,
        )
        .map_err(de::Error::custom)
    }
}

impl UserKindCompositionDto {
    /// Creates one validated user kind composition.
    ///
    /// A reserved first-party kind identity cannot be declared as a user kind.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `kind_id` is reserved for a first-party kind.
    pub fn new(
        kind_id: ProviderKindId,
        stream: UserKindStreamPartDto,
        reasoning: UserKindReasoningPartDto,
        activation: UserKindActivationPartDto,
        effort: UserKindEffortPartDto,
        credential_transport: CredentialTransportDto,
    ) -> DtoResult<Self> {
        if kind_id.is_first_party() {
            return Err(ErrorDto::validation(
                "invalid_user_kind_composition",
                "a reserved first-party kind identity cannot be declared as a user kind",
            ));
        }
        Ok(Self {
            kind_id,
            stream,
            reasoning,
            activation,
            effort,
            credential_transport,
        })
    }

    /// Returns the declared user kind identity.
    #[must_use]
    pub const fn kind_id(&self) -> &ProviderKindId {
        &self.kind_id
    }

    /// Returns the stream-framing part.
    #[must_use]
    pub const fn stream(&self) -> UserKindStreamPartDto {
        self.stream
    }

    /// Returns the textual reasoning field part.
    #[must_use]
    pub const fn reasoning(&self) -> UserKindReasoningPartDto {
        self.reasoning
    }

    /// Returns the thinking activation part.
    #[must_use]
    pub const fn activation(&self) -> UserKindActivationPartDto {
        self.activation
    }

    /// Returns the effort field part.
    #[must_use]
    pub const fn effort(&self) -> UserKindEffortPartDto {
        self.effort
    }

    /// Returns the credential transport part.
    #[must_use]
    pub const fn credential_transport(&self) -> &CredentialTransportDto {
        &self.credential_transport
    }
}

/// A paginated provider catalog read request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ListProviderCatalogQueryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page_token: Option<String>,
}

impl<'de> Deserialize<'de> for ListProviderCatalogQueryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawListProviderCatalogQueryDto {
            #[serde(default)]
            page_token: Option<String>,
        }

        let raw = RawListProviderCatalogQueryDto::deserialize(deserializer)?;
        Self::new(raw.page_token).map_err(de::Error::custom)
    }
}

impl ListProviderCatalogQueryDto {
    /// Creates a catalog read request; the page token is opaque.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_page_token_invalid` when a supplied token is not
    /// a non-blank safe token.
    pub fn new(page_token: Option<String>) -> DtoResult<Self> {
        if page_token
            .as_deref()
            .is_some_and(|token| !is_safe_token(token))
        {
            return Err(ErrorDto::validation(
                "provider_catalog_page_token_invalid",
                "the catalog page token is malformed",
            ));
        }
        Ok(Self { page_token })
    }

    /// Returns the opaque continuation token, when the caller supplied one.
    #[must_use]
    pub fn page_token(&self) -> Option<&str> {
        self.page_token.as_deref()
    }
}

/// One bounded page of the active provider catalog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogPageDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    catalog_revision_id: Option<CatalogRevisionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_profile_id: Option<ProviderProfileId>,
    entries: Vec<ProviderProfileEntryDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_page_token: Option<String>,
    has_more: bool,
}

impl<'de> Deserialize<'de> for ProviderCatalogPageDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderCatalogPageDto {
            #[serde(default)]
            catalog_revision_id: Option<CatalogRevisionId>,
            #[serde(default)]
            default_profile_id: Option<ProviderProfileId>,
            entries: Vec<ProviderProfileEntryDto>,
            #[serde(default)]
            next_page_token: Option<String>,
            has_more: bool,
        }

        let raw = RawProviderCatalogPageDto::deserialize(deserializer)?;
        Self::new(
            raw.catalog_revision_id,
            raw.default_profile_id,
            raw.entries,
            raw.next_page_token,
            raw.has_more,
        )
        .map_err(de::Error::custom)
    }
}

impl ProviderCatalogPageDto {
    /// Creates one coherent, ordered catalog page.
    ///
    /// Entries are ordered by stable profile identity, and a page reports more
    /// entries exactly when it carries a continuation token.
    ///
    /// # Errors
    ///
    /// Returns a validation error when entries are not strictly ordered or the
    /// continuation state is incoherent.
    pub fn new(
        catalog_revision_id: Option<CatalogRevisionId>,
        default_profile_id: Option<ProviderProfileId>,
        entries: Vec<ProviderProfileEntryDto>,
        next_page_token: Option<String>,
        has_more: bool,
    ) -> DtoResult<Self> {
        if !entries
            .windows(2)
            .all(|pair| pair[0].profile_id() < pair[1].profile_id())
        {
            return Err(ErrorDto::validation(
                "invalid_provider_catalog_page",
                "catalog page entries must be ordered by stable profile identity",
            ));
        }
        if next_page_token
            .as_deref()
            .is_some_and(|token| !is_safe_token(token))
        {
            return Err(ErrorDto::validation(
                "provider_catalog_page_token_invalid",
                "the catalog page token is malformed",
            ));
        }
        if has_more != next_page_token.is_some() {
            return Err(ErrorDto::validation(
                "invalid_provider_catalog_page",
                "a catalog page reports more entries exactly when it carries a continuation token",
            ));
        }
        Ok(Self {
            catalog_revision_id,
            default_profile_id,
            entries,
            next_page_token,
            has_more,
        })
    }

    /// Returns the active catalog revision, when one is active.
    #[must_use]
    pub const fn catalog_revision_id(&self) -> Option<CatalogRevisionId> {
        self.catalog_revision_id
    }

    /// Returns the global default profile identity, when one is declared.
    #[must_use]
    pub const fn default_profile_id(&self) -> Option<&ProviderProfileId> {
        self.default_profile_id.as_ref()
    }

    /// Returns the ordered page entries.
    #[must_use]
    pub fn entries(&self) -> &[ProviderProfileEntryDto] {
        &self.entries
    }

    /// Returns the opaque continuation token, when more entries exist.
    #[must_use]
    pub fn next_page_token(&self) -> Option<&str> {
        self.next_page_token.as_deref()
    }

    /// Returns whether more entries exist after this page.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
}

/// The deterministic driver-declared capabilities of one profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderDriverCapabilitiesDto {
    text_streaming: bool,
    reasoning: bool,
    tool_calls: bool,
}

impl ProviderDriverCapabilitiesDto {
    /// Creates a deterministic driver capability declaration.
    #[must_use]
    pub const fn new(text_streaming: bool, reasoning: bool, tool_calls: bool) -> Self {
        Self {
            text_streaming,
            reasoning,
            tool_calls,
        }
    }

    /// Returns whether the driver declares streamed text.
    #[must_use]
    pub const fn text_streaming(self) -> bool {
        self.text_streaming
    }

    /// Returns whether the driver declares reasoning output.
    #[must_use]
    pub const fn reasoning(self) -> bool {
        self.reasoning
    }

    /// Returns whether the driver declares tool calls.
    #[must_use]
    pub const fn tool_calls(self) -> bool {
        self.tool_calls
    }
}

/// The closed local readiness projection of one profile.
///
/// Readiness never claims network or credential health.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProfileReadinessDto {
    /// The profile is enabled and has a live private runtime entry.
    Ready,
    /// The profile is disabled.
    Disabled,
    /// The profile has no live private runtime entry.
    Unavailable,
}

/// One credential-free catalog entry for a profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderProfileEntryDto {
    profile_id: ProviderProfileId,
    display_name: String,
    enabled: bool,
    kind_id: ProviderKindId,
    kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
    model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normalized_endpoint: Option<String>,
    effective_execution_policy: ProviderExecutionPolicyDto,
    declared_model_capability_subset: ModelCapabilitySetV1,
    credential_transport: CredentialTransportDto,
    credential_configured: bool,
    driver_capabilities: ProviderDriverCapabilitiesDto,
    readiness: ProviderProfileReadinessDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pricing_policy: Option<ProviderPricingPolicyDto>,
}

impl<'de> Deserialize<'de> for ProviderProfileEntryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderProfileEntryDto {
            profile_id: ProviderProfileId,
            display_name: String,
            enabled: bool,
            kind_id: ProviderKindId,
            kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
            model_id: String,
            #[serde(default)]
            normalized_endpoint: Option<String>,
            effective_execution_policy: ProviderExecutionPolicyDto,
            declared_model_capability_subset: ModelCapabilitySetV1,
            credential_transport: CredentialTransportDto,
            credential_configured: bool,
            driver_capabilities: ProviderDriverCapabilitiesDto,
            readiness: ProviderProfileReadinessDto,
            #[serde(default)]
            pricing_policy: Option<ProviderPricingPolicyDto>,
        }

        let raw = RawProviderProfileEntryDto::deserialize(deserializer)?;
        Self::new(
            raw.profile_id,
            raw.display_name,
            raw.enabled,
            raw.kind_id,
            raw.kind_descriptor_revision_id,
            raw.model_id,
            raw.normalized_endpoint,
            raw.effective_execution_policy,
            raw.declared_model_capability_subset,
            raw.credential_transport,
            raw.credential_configured,
            raw.driver_capabilities,
            raw.readiness,
            raw.pricing_policy,
        )
        .map_err(de::Error::custom)
    }
}

impl ProviderProfileEntryDto {
    /// Creates one validated credential-free catalog entry.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the display name or model identity is
    /// blank or the normalized endpoint is not a well-formed absolute endpoint.
    #[expect(
        clippy::too_many_arguments,
        reason = "This wire constructor preserves the frozen fourteen-field catalog entry contract."
    )]
    pub fn new(
        profile_id: ProviderProfileId,
        display_name: impl Into<String>,
        enabled: bool,
        kind_id: ProviderKindId,
        kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
        model_id: impl Into<String>,
        normalized_endpoint: Option<String>,
        effective_execution_policy: ProviderExecutionPolicyDto,
        declared_model_capability_subset: ModelCapabilitySetV1,
        credential_transport: CredentialTransportDto,
        credential_configured: bool,
        driver_capabilities: ProviderDriverCapabilitiesDto,
        readiness: ProviderProfileReadinessDto,
        pricing_policy: Option<ProviderPricingPolicyDto>,
    ) -> DtoResult<Self> {
        let display_name = display_name.into();
        if display_name.trim().is_empty() || display_name.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "invalid_provider_profile_entry",
                "the profile display name must be non-blank and control-free",
            ));
        }
        let model_id = model_id.into();
        if model_id.trim().is_empty() || model_id.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "invalid_provider_profile_entry",
                "the model identity must be a non-blank control-free token",
            ));
        }
        if let Some(endpoint) = normalized_endpoint.as_deref() {
            validate_endpoint_shape(endpoint).map_err(|_| {
                ErrorDto::validation(
                    "invalid_provider_profile_entry",
                    "the normalized endpoint is not a well-formed absolute endpoint",
                )
            })?;
        }
        Ok(Self {
            profile_id,
            display_name,
            enabled,
            kind_id,
            kind_descriptor_revision_id,
            model_id,
            normalized_endpoint,
            effective_execution_policy,
            declared_model_capability_subset,
            credential_transport,
            credential_configured,
            driver_capabilities,
            readiness,
            pricing_policy,
        })
    }

    /// Returns the stable profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the safe presentation display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns whether the profile is enabled.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the provider kind identity.
    #[must_use]
    pub const fn kind_id(&self) -> &ProviderKindId {
        &self.kind_id
    }

    /// Returns the bound kind descriptor revision identity.
    #[must_use]
    pub const fn kind_descriptor_revision_id(&self) -> ProviderKindDescriptorRevisionId {
        self.kind_descriptor_revision_id
    }

    /// Returns the exact configured model identity.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the normalized endpoint, when the profile declares one.
    #[must_use]
    pub fn normalized_endpoint(&self) -> Option<&str> {
        self.normalized_endpoint.as_deref()
    }

    /// Returns the effective execution policy.
    #[must_use]
    pub const fn effective_execution_policy(&self) -> ProviderExecutionPolicyDto {
        self.effective_execution_policy
    }

    /// Returns the declared exact-model capability subset.
    #[must_use]
    pub const fn declared_model_capability_subset(&self) -> &ModelCapabilitySetV1 {
        &self.declared_model_capability_subset
    }

    /// Returns the credential transport declaration.
    #[must_use]
    pub const fn credential_transport(&self) -> &CredentialTransportDto {
        &self.credential_transport
    }

    /// Returns whether a private credential is configured for the profile.
    #[must_use]
    pub const fn credential_configured(&self) -> bool {
        self.credential_configured
    }

    /// Returns the deterministic driver-declared capabilities.
    #[must_use]
    pub const fn driver_capabilities(&self) -> ProviderDriverCapabilitiesDto {
        self.driver_capabilities
    }

    /// Returns the closed local readiness projection.
    #[must_use]
    pub const fn readiness(&self) -> ProviderProfileReadinessDto {
        self.readiness
    }

    /// Returns the optional product pricing policy.
    #[must_use]
    pub const fn pricing_policy(&self) -> Option<&ProviderPricingPolicyDto> {
        self.pricing_policy.as_ref()
    }
}

/// The closed activation state of the provider catalog.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCatalogActivationStateDto {
    /// The catalog is being prepared before readiness.
    Preparing,
    /// The accepted catalog is active.
    Active,
    /// One removal candidate awaits explicit accept or reject.
    PendingRemoval,
    /// An accepted catalog awaits exact activation recovery.
    ActivationRecoveryRequired,
}

/// The closed degraded reason of the provider catalog.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCatalogDegradedReasonDto {
    /// One removal candidate awaits explicit accept or reject.
    RemovalCandidatePending,
    /// The removal candidate was rejected.
    RemovalCandidateRejected,
    /// An accepted catalog awaits exact activation recovery.
    ActivationRecoveryRequired,
}

/// One process-local pending removal candidate handle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogCandidateHandleDto {
    candidate_revision_id: CatalogRevisionId,
    expected_active_revision_id: CatalogRevisionId,
}

impl ProviderCatalogCandidateHandleDto {
    /// Creates one exact pending removal candidate handle.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the candidate revision does not differ
    /// from the expected active revision.
    pub fn new(
        candidate_revision_id: CatalogRevisionId,
        expected_active_revision_id: CatalogRevisionId,
    ) -> DtoResult<Self> {
        if candidate_revision_id == expected_active_revision_id {
            return Err(ErrorDto::validation(
                "invalid_provider_catalog_candidate_handle",
                "a removal candidate revision must differ from the expected active revision",
            ));
        }
        Ok(Self {
            candidate_revision_id,
            expected_active_revision_id,
        })
    }

    /// Returns the pending candidate revision identity.
    #[must_use]
    pub const fn candidate_revision_id(self) -> CatalogRevisionId {
        self.candidate_revision_id
    }

    /// Returns the active revision the candidate expected at preparation time.
    #[must_use]
    pub const fn expected_active_revision_id(self) -> CatalogRevisionId {
        self.expected_active_revision_id
    }
}

/// One safe catalog validation issue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogValidationIssueDto {
    code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile_id: Option<ProviderProfileId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind_id: Option<ProviderKindId>,
}

impl<'de> Deserialize<'de> for ProviderCatalogValidationIssueDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderCatalogValidationIssueDto {
            code: String,
            #[serde(default)]
            profile_id: Option<ProviderProfileId>,
            #[serde(default)]
            kind_id: Option<ProviderKindId>,
        }

        let raw = RawProviderCatalogValidationIssueDto::deserialize(deserializer)?;
        Self::new(raw.code, raw.profile_id, raw.kind_id).map_err(de::Error::custom)
    }
}

impl ProviderCatalogValidationIssueDto {
    /// Creates one closed safe catalog validation issue.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `code` is not a lower `snake_case` token.
    pub fn new(
        code: impl Into<String>,
        profile_id: Option<ProviderProfileId>,
        kind_id: Option<ProviderKindId>,
    ) -> DtoResult<Self> {
        let code = code.into();
        if !is_code_token(&code) {
            return Err(ErrorDto::validation(
                "invalid_provider_catalog_validation_issue",
                "a validation issue code must be a lower snake_case token",
            ));
        }
        Ok(Self {
            code,
            profile_id,
            kind_id,
        })
    }

    /// Returns the stable machine-readable issue code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the affected profile identity, when the issue names one.
    #[must_use]
    pub const fn profile_id(&self) -> Option<&ProviderProfileId> {
        self.profile_id.as_ref()
    }

    /// Returns the affected kind identity, when the issue names one.
    #[must_use]
    pub const fn kind_id(&self) -> Option<&ProviderKindId> {
        self.kind_id.as_ref()
    }
}

/// The safe current status of the provider catalog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogStatusDto {
    activation_state: ProviderCatalogActivationStateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    degraded_reason: Option<ProviderCatalogDegradedReasonDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_catalog_revision_id: Option<CatalogRevisionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    candidate: Option<ProviderCatalogCandidateHandleDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_profile_id: Option<ProviderProfileId>,
    validation_issues: Vec<ProviderCatalogValidationIssueDto>,
}

impl<'de> Deserialize<'de> for ProviderCatalogStatusDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderCatalogStatusDto {
            activation_state: ProviderCatalogActivationStateDto,
            #[serde(default)]
            degraded_reason: Option<ProviderCatalogDegradedReasonDto>,
            #[serde(default)]
            active_catalog_revision_id: Option<CatalogRevisionId>,
            #[serde(default)]
            candidate: Option<ProviderCatalogCandidateHandleDto>,
            #[serde(default)]
            default_profile_id: Option<ProviderProfileId>,
            validation_issues: Vec<ProviderCatalogValidationIssueDto>,
        }

        let raw = RawProviderCatalogStatusDto::deserialize(deserializer)?;
        Self::new(
            raw.activation_state,
            raw.degraded_reason,
            raw.active_catalog_revision_id,
            raw.candidate,
            raw.default_profile_id,
            raw.validation_issues,
        )
        .map_err(de::Error::custom)
    }
}

impl ProviderCatalogStatusDto {
    /// Creates one coherent safe catalog status projection.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a pending removal is reported without its
    /// exact candidate handle.
    pub fn new(
        activation_state: ProviderCatalogActivationStateDto,
        degraded_reason: Option<ProviderCatalogDegradedReasonDto>,
        active_catalog_revision_id: Option<CatalogRevisionId>,
        candidate: Option<ProviderCatalogCandidateHandleDto>,
        default_profile_id: Option<ProviderProfileId>,
        validation_issues: Vec<ProviderCatalogValidationIssueDto>,
    ) -> DtoResult<Self> {
        if activation_state == ProviderCatalogActivationStateDto::PendingRemoval
            && candidate.is_none()
        {
            return Err(ErrorDto::validation(
                "invalid_provider_catalog_status",
                "a pending removal reports its exact candidate handle",
            ));
        }
        Ok(Self {
            activation_state,
            degraded_reason,
            active_catalog_revision_id,
            candidate,
            default_profile_id,
            validation_issues,
        })
    }

    /// Returns the closed activation state.
    #[must_use]
    pub const fn activation_state(&self) -> ProviderCatalogActivationStateDto {
        self.activation_state
    }

    /// Returns the applicable closed degraded reason, when the catalog is degraded.
    #[must_use]
    pub const fn degraded_reason(&self) -> Option<ProviderCatalogDegradedReasonDto> {
        self.degraded_reason
    }

    /// Returns the active catalog revision, when one is active.
    #[must_use]
    pub const fn active_catalog_revision_id(&self) -> Option<CatalogRevisionId> {
        self.active_catalog_revision_id
    }

    /// Returns the pending removal candidate handle, when one exists.
    #[must_use]
    pub const fn candidate(&self) -> Option<ProviderCatalogCandidateHandleDto> {
        self.candidate
    }

    /// Returns the global default profile identity, when one is declared.
    #[must_use]
    pub const fn default_profile_id(&self) -> Option<&ProviderProfileId> {
        self.default_profile_id.as_ref()
    }

    /// Returns the safe validation issues of the candidate or active catalog.
    #[must_use]
    pub fn validation_issues(&self) -> &[ProviderCatalogValidationIssueDto] {
        &self.validation_issues
    }
}

/// A command requesting one durable session default profile change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SetSessionProviderProfileCommandDto {
    session_id: SessionId,
    profile_id: ProviderProfileId,
    expected_session_projection_revision: u64,
    operation_id: IdempotencyKey,
}

impl SetSessionProviderProfileCommandDto {
    /// Creates an optimistic durable session default change request.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        profile_id: ProviderProfileId,
        expected_session_projection_revision: u64,
        operation_id: IdempotencyKey,
    ) -> Self {
        Self {
            session_id,
            profile_id,
            expected_session_projection_revision,
            operation_id,
        }
    }

    /// Returns the target session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the requested durable default profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the expected durable session projection revision.
    #[must_use]
    pub const fn expected_session_projection_revision(&self) -> u64 {
        self.expected_session_projection_revision
    }

    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> IdempotencyKey {
        self.operation_id
    }
}

/// Acceptance evidence for one session default profile change.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SetSessionProviderProfileAcceptedDto {
    session_id: SessionId,
    changed: bool,
    session_projection_revision: u64,
}

impl SetSessionProviderProfileAcceptedDto {
    /// Creates complete session default acceptance evidence.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        changed: bool,
        session_projection_revision: u64,
    ) -> Self {
        Self {
            session_id,
            changed,
            session_projection_revision,
        }
    }

    /// Returns the target session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns whether the durable default actually changed.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.changed
    }

    /// Returns the committed durable session projection revision.
    #[must_use]
    pub const fn session_projection_revision(self) -> u64 {
        self.session_projection_revision
    }
}

/// A query requesting one session's provider profile projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetSessionProviderProfileQueryDto {
    session_id: SessionId,
}

impl GetSessionProviderProfileQueryDto {
    /// Creates a typed session provider profile query.
    #[must_use]
    pub const fn new(session_id: SessionId) -> Self {
        Self { session_id }
    }

    /// Returns the target session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }
}

/// The safe current provider profile projection of one session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionProviderProfileProjectionDto {
    session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    durable_profile_id: Option<ProviderProfileId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_entry: Option<ProviderProfileEntryDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unavailability: Option<ProviderSelectionUnavailabilityDto>,
    session_projection_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    global_default_profile_id: Option<ProviderProfileId>,
}

impl<'de> Deserialize<'de> for SessionProviderProfileProjectionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawSessionProviderProfileProjectionDto {
            session_id: SessionId,
            #[serde(default)]
            durable_profile_id: Option<ProviderProfileId>,
            #[serde(default)]
            resolved_entry: Option<ProviderProfileEntryDto>,
            #[serde(default)]
            unavailability: Option<ProviderSelectionUnavailabilityDto>,
            session_projection_revision: u64,
            #[serde(default)]
            global_default_profile_id: Option<ProviderProfileId>,
        }

        let raw = RawSessionProviderProfileProjectionDto::deserialize(deserializer)?;
        Self::new(
            raw.session_id,
            raw.durable_profile_id,
            raw.resolved_entry,
            raw.unavailability,
            raw.session_projection_revision,
            raw.global_default_profile_id,
        )
        .map_err(de::Error::custom)
    }
}

impl SessionProviderProfileProjectionDto {
    /// Creates one coherent session provider profile projection.
    ///
    /// A projection never claims a resolved entry and an unavailability reason at
    /// the same time.
    ///
    /// # Errors
    ///
    /// Returns a validation error when both a resolved entry and an
    /// unavailability reason are present.
    pub fn new(
        session_id: SessionId,
        durable_profile_id: Option<ProviderProfileId>,
        resolved_entry: Option<ProviderProfileEntryDto>,
        unavailability: Option<ProviderSelectionUnavailabilityDto>,
        session_projection_revision: u64,
        global_default_profile_id: Option<ProviderProfileId>,
    ) -> DtoResult<Self> {
        if resolved_entry.is_some() && unavailability.is_some() {
            return Err(ErrorDto::validation(
                "invalid_session_provider_profile_projection",
                "a session provider projection is either resolved or unavailable",
            ));
        }
        Ok(Self {
            session_id,
            durable_profile_id,
            resolved_entry,
            unavailability,
            session_projection_revision,
            global_default_profile_id,
        })
    }

    /// Returns the target session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the durable session default profile identity, when one exists.
    #[must_use]
    pub const fn durable_profile_id(&self) -> Option<&ProviderProfileId> {
        self.durable_profile_id.as_ref()
    }

    /// Returns the current safe resolved entry, when the selection resolves.
    #[must_use]
    pub const fn resolved_entry(&self) -> Option<&ProviderProfileEntryDto> {
        self.resolved_entry.as_ref()
    }

    /// Returns the closed unavailability reason, when the selection is unavailable.
    #[must_use]
    pub const fn unavailability(&self) -> Option<ProviderSelectionUnavailabilityDto> {
        self.unavailability
    }

    /// Returns the current durable session projection revision.
    #[must_use]
    pub const fn session_projection_revision(&self) -> u64 {
        self.session_projection_revision
    }

    /// Returns the global default profile identity, when one is declared.
    #[must_use]
    pub const fn global_default_profile_id(&self) -> Option<&ProviderProfileId> {
        self.global_default_profile_id.as_ref()
    }
}

/// An optional explicit provider profile override on one user turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderProfileOverrideDto {
    profile_id: ProviderProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_profile_revision_id: Option<ProviderProfileRevisionId>,
}

impl ProviderProfileOverrideDto {
    /// Creates an optional explicit profile override.
    #[must_use]
    pub const fn new(
        profile_id: ProviderProfileId,
        expected_profile_revision_id: Option<ProviderProfileRevisionId>,
    ) -> Self {
        Self {
            profile_id,
            expected_profile_revision_id,
        }
    }

    /// Returns the overridden profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the expected profile revision, when the caller pins one.
    #[must_use]
    pub const fn expected_profile_revision_id(&self) -> Option<ProviderProfileRevisionId> {
        self.expected_profile_revision_id
    }
}

/// A command requesting one controlled configuration reload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReloadConfigurationCommandDto {
    operation_id: IdempotencyKey,
}

impl ReloadConfigurationCommandDto {
    /// Creates a typed configuration reload request.
    #[must_use]
    pub const fn new(operation_id: IdempotencyKey) -> Self {
        Self { operation_id }
    }

    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn operation_id(self) -> IdempotencyKey {
        self.operation_id
    }
}

/// Acceptance evidence for one controlled configuration reload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigurationReloadAcceptedDto {
    config_revision_id: ConfigRevisionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    catalog_revision_id: Option<CatalogRevisionId>,
}

impl ConfigurationReloadAcceptedDto {
    /// Creates complete configuration reload acceptance evidence.
    #[must_use]
    pub const fn new(
        config_revision_id: ConfigRevisionId,
        catalog_revision_id: Option<CatalogRevisionId>,
    ) -> Self {
        Self {
            config_revision_id,
            catalog_revision_id,
        }
    }

    /// Returns the committed configuration revision identity.
    #[must_use]
    pub const fn config_revision_id(self) -> ConfigRevisionId {
        self.config_revision_id
    }

    /// Returns the active catalog revision, when one applies.
    #[must_use]
    pub const fn catalog_revision_id(self) -> Option<CatalogRevisionId> {
        self.catalog_revision_id
    }
}

/// One closed typed configuration edit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigurationEditDto {
    /// Sets the global default provider profile.
    SetDefaultProfile {
        /// The requested global default profile identity.
        profile_id: ProviderProfileId,
    },
    /// Sets one profile's enabled state.
    SetProfileEnabled {
        /// The edited profile identity.
        profile_id: ProviderProfileId,
        /// The requested enabled state.
        enabled: bool,
    },
    /// Sets one profile's display name.
    SetProfileDisplayName {
        /// The edited profile identity.
        profile_id: ProviderProfileId,
        /// The requested safe display name.
        display_name: String,
    },
}

impl<'de> Deserialize<'de> for ConfigurationEditDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[expect(
            clippy::enum_variant_names,
            reason = "Raw edit variants mirror the public closed edit tags."
        )]
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum RawConfigurationEditDto {
            SetDefaultProfile {
                profile_id: ProviderProfileId,
            },
            SetProfileEnabled {
                profile_id: ProviderProfileId,
                enabled: bool,
            },
            SetProfileDisplayName {
                profile_id: ProviderProfileId,
                display_name: String,
            },
        }

        match RawConfigurationEditDto::deserialize(deserializer)? {
            RawConfigurationEditDto::SetDefaultProfile { profile_id } => {
                Ok(Self::set_default_profile(profile_id))
            }
            RawConfigurationEditDto::SetProfileEnabled {
                profile_id,
                enabled,
            } => Ok(Self::set_profile_enabled(profile_id, enabled)),
            RawConfigurationEditDto::SetProfileDisplayName {
                profile_id,
                display_name,
            } => {
                Self::set_profile_display_name(profile_id, display_name).map_err(de::Error::custom)
            }
        }
    }
}

impl ConfigurationEditDto {
    /// Creates a global default profile edit.
    #[must_use]
    pub const fn set_default_profile(profile_id: ProviderProfileId) -> Self {
        Self::SetDefaultProfile { profile_id }
    }

    /// Creates a profile enablement edit.
    #[must_use]
    pub const fn set_profile_enabled(profile_id: ProviderProfileId, enabled: bool) -> Self {
        Self::SetProfileEnabled {
            profile_id,
            enabled,
        }
    }

    /// Creates a profile display name edit.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `display_name` is blank or carries
    /// control characters.
    pub fn set_profile_display_name(
        profile_id: ProviderProfileId,
        display_name: impl Into<String>,
    ) -> DtoResult<Self> {
        let display_name = display_name.into();
        if display_name.trim().is_empty() || display_name.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "invalid_configuration_edit",
                "the profile display name must be non-blank and control-free",
            ));
        }
        Ok(Self::SetProfileDisplayName {
            profile_id,
            display_name,
        })
    }

    /// Returns the edited profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        match self {
            Self::SetDefaultProfile { profile_id }
            | Self::SetProfileEnabled { profile_id, .. }
            | Self::SetProfileDisplayName { profile_id, .. } => profile_id,
        }
    }

    /// Returns the requested enabled state of an enablement edit.
    #[must_use]
    pub const fn enabled(&self) -> Option<bool> {
        match self {
            Self::SetDefaultProfile { .. } | Self::SetProfileDisplayName { .. } => None,
            Self::SetProfileEnabled { enabled, .. } => Some(*enabled),
        }
    }

    /// Returns the requested display name of a display-name edit.
    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        match self {
            Self::SetDefaultProfile { .. } | Self::SetProfileEnabled { .. } => None,
            Self::SetProfileDisplayName { display_name, .. } => Some(display_name),
        }
    }
}

/// A command carrying one credential-free configuration document to apply.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApplyConfigurationDocumentCommandDto {
    document: String,
    operation_id: IdempotencyKey,
}

impl<'de> Deserialize<'de> for ApplyConfigurationDocumentCommandDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawApplyConfigurationDocumentCommandDto {
            document: String,
            operation_id: IdempotencyKey,
        }

        let raw = RawApplyConfigurationDocumentCommandDto::deserialize(deserializer)?;
        Self::new(raw.document, raw.operation_id).map_err(de::Error::custom)
    }
}

impl ApplyConfigurationDocumentCommandDto {
    /// Creates a validated configuration document apply request.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the document is empty.
    pub fn new(document: impl Into<String>, operation_id: IdempotencyKey) -> DtoResult<Self> {
        let document = document.into();
        if document.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_configuration_edit",
                "the applied configuration document must not be empty",
            ));
        }
        Ok(Self {
            document,
            operation_id,
        })
    }

    /// Returns the credential-free candidate document.
    #[must_use]
    pub fn document(&self) -> &str {
        &self.document
    }

    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> IdempotencyKey {
        self.operation_id
    }
}

/// A command carrying closed typed configuration edits to apply.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApplyConfigurationEditsCommandDto {
    edits: Vec<ConfigurationEditDto>,
    operation_id: IdempotencyKey,
}

impl<'de> Deserialize<'de> for ApplyConfigurationEditsCommandDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawApplyConfigurationEditsCommandDto {
            edits: Vec<ConfigurationEditDto>,
            operation_id: IdempotencyKey,
        }

        let raw = RawApplyConfigurationEditsCommandDto::deserialize(deserializer)?;
        Self::new(raw.edits, raw.operation_id).map_err(de::Error::custom)
    }
}

impl ApplyConfigurationEditsCommandDto {
    /// Creates a validated typed configuration edit request.
    ///
    /// # Errors
    ///
    /// Returns a validation error when no edit is supplied.
    pub fn new(edits: Vec<ConfigurationEditDto>, operation_id: IdempotencyKey) -> DtoResult<Self> {
        if edits.is_empty() {
            return Err(ErrorDto::validation(
                "invalid_configuration_edit",
                "a typed configuration edit request carries at least one edit",
            ));
        }
        Ok(Self {
            edits,
            operation_id,
        })
    }

    /// Returns the ordered closed typed edits.
    #[must_use]
    pub fn edits(&self) -> &[ConfigurationEditDto] {
        &self.edits
    }

    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> IdempotencyKey {
        self.operation_id
    }
}

/// Acceptance evidence for one applied configuration edit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigurationEditAcceptedDto {
    config_revision_id: ConfigRevisionId,
    requires_restart: bool,
}

impl ConfigurationEditAcceptedDto {
    /// Creates complete configuration edit acceptance evidence.
    #[must_use]
    pub const fn new(config_revision_id: ConfigRevisionId, requires_restart: bool) -> Self {
        Self {
            config_revision_id,
            requires_restart,
        }
    }

    /// Returns the committed configuration revision identity.
    #[must_use]
    pub const fn config_revision_id(self) -> ConfigRevisionId {
        self.config_revision_id
    }

    /// Returns whether the committed edit takes effect only at the next daemon restart.
    #[must_use]
    pub const fn requires_restart(&self) -> bool {
        self.requires_restart
    }
}

/// A command requesting credential rotation for one profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RotateProviderCredentialCommandDto {
    profile_id: ProviderProfileId,
    operation_id: IdempotencyKey,
}

impl RotateProviderCredentialCommandDto {
    /// Returns a credential rotation request.
    ///
    /// The command carries no credential material; rotation re-reads the
    /// private source inside the composition's loading boundary.
    #[must_use]
    pub const fn new(profile_id: ProviderProfileId, operation_id: IdempotencyKey) -> Self {
        Self {
            profile_id,
            operation_id,
        }
    }

    /// Returns the rotated profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> IdempotencyKey {
        self.operation_id
    }
}

/// Acceptance evidence for one credential rotation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CredentialRotationAcceptedDto {
    profile_id: ProviderProfileId,
}

impl CredentialRotationAcceptedDto {
    /// Creates complete credential rotation acceptance evidence.
    #[must_use]
    pub const fn new(profile_id: ProviderProfileId) -> Self {
        Self { profile_id }
    }

    /// Returns the rotated profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }
}

/// A command requesting one provider health check.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckProviderHealthCommandDto {
    profile_id: ProviderProfileId,
}

impl CheckProviderHealthCommandDto {
    /// Creates a typed provider health check request.
    #[must_use]
    pub const fn new(profile_id: ProviderProfileId) -> Self {
        Self { profile_id }
    }

    /// Returns the checked profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }
}

/// The closed provider health state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealthStateDto {
    /// The provider answered the declared health check.
    Available,
    /// The provider did not answer the declared health check.
    Unavailable,
}

/// The closed provider unavailability reason.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealthReasonDto {
    /// The profile has no configured private credential.
    CredentialNotConfigured,
    /// The endpoint could not be reached.
    EndpointUnreachable,
    /// The provider rejected the declared health request.
    ProviderRejected,
    /// The declared health request timed out.
    TimedOut,
}

/// Non-authorizing provider health evidence.
///
/// Health evidence never creates a run, retry counter, or quota, and no profile
/// revision is reported while the catalog is not wired into the health path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderHealthEvidenceDto {
    provider_id: ProviderProfileId,
    state: ProviderHealthStateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<ProviderHealthReasonDto>,
}

impl<'de> Deserialize<'de> for ProviderHealthEvidenceDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderHealthEvidenceDto {
            provider_id: ProviderProfileId,
            state: ProviderHealthStateDto,
            #[serde(default)]
            reason: Option<ProviderHealthReasonDto>,
        }

        let raw = RawProviderHealthEvidenceDto::deserialize(deserializer)?;
        Self::new(raw.provider_id, raw.state, raw.reason).map_err(de::Error::custom)
    }
}

impl ProviderHealthEvidenceDto {
    /// Creates one coherent health evidence value.
    ///
    /// An unavailable result retains its exact closed reason, and an available
    /// result carries none.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the state and reason disagree.
    pub fn new(
        provider_id: ProviderProfileId,
        state: ProviderHealthStateDto,
        reason: Option<ProviderHealthReasonDto>,
    ) -> DtoResult<Self> {
        let coherent = match state {
            ProviderHealthStateDto::Available => reason.is_none(),
            ProviderHealthStateDto::Unavailable => reason.is_some(),
        };
        if !coherent {
            return Err(ErrorDto::validation(
                "invalid_provider_health_evidence",
                "provider health evidence is either available without a reason or unavailable with one",
            ));
        }
        Ok(Self {
            provider_id,
            state,
            reason,
        })
    }

    /// Returns the checked provider identity.
    #[must_use]
    pub const fn provider_id(&self) -> &ProviderProfileId {
        &self.provider_id
    }

    /// Returns the closed health state.
    #[must_use]
    pub const fn state(&self) -> ProviderHealthStateDto {
        self.state
    }

    /// Returns the closed unavailability reason, when the provider is unavailable.
    #[must_use]
    pub const fn reason(&self) -> Option<ProviderHealthReasonDto> {
        self.reason
    }
}

/// A command requesting one provider/model discovery attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiscoverProviderModelsCommandDto {
    profile_id: ProviderProfileId,
}

impl DiscoverProviderModelsCommandDto {
    /// Creates a typed provider/model discovery request.
    #[must_use]
    pub const fn new(profile_id: ProviderProfileId) -> Self {
        Self { profile_id }
    }

    /// Returns the discovery profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }
}

/// One additive, non-authorizing discovered provider model record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderModelRecordDto {
    model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
}

impl<'de> Deserialize<'de> for ProviderModelRecordDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderModelRecordDto {
            model_id: String,
            #[serde(default)]
            display_name: Option<String>,
        }

        let raw = RawProviderModelRecordDto::deserialize(deserializer)?;
        Self::new(raw.model_id, raw.display_name).map_err(de::Error::custom)
    }
}

impl ProviderModelRecordDto {
    /// Creates one validated discovered model record.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the model identity or display name is
    /// blank or carries control characters.
    pub fn new(model_id: impl Into<String>, display_name: Option<String>) -> DtoResult<Self> {
        let model_id = model_id.into();
        if model_id.trim().is_empty() || model_id.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "invalid_provider_model_record",
                "the discovered model identity must be a non-blank control-free token",
            ));
        }
        if display_name
            .as_deref()
            .is_some_and(|name| name.trim().is_empty() || name.chars().any(char::is_control))
        {
            return Err(ErrorDto::validation(
                "invalid_provider_model_record",
                "the discovered display name must be non-blank and control-free",
            ));
        }
        Ok(Self {
            model_id,
            display_name,
        })
    }

    /// Returns the discovered exact model identity.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the discovered display name, when the provider supplied one.
    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}

/// The closed non-authorizing result of one discovery attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderDiscoveryResultDto {
    attempt_id: ProviderDiscoveryAttemptId,
    records: Vec<ProviderModelRecordDto>,
}

impl<'de> Deserialize<'de> for ProviderDiscoveryResultDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProviderDiscoveryResultDto {
            attempt_id: ProviderDiscoveryAttemptId,
            records: Vec<ProviderModelRecordDto>,
        }

        let raw = RawProviderDiscoveryResultDto::deserialize(deserializer)?;
        Self::new(raw.attempt_id, raw.records).map_err(de::Error::custom)
    }
}

impl ProviderDiscoveryResultDto {
    /// Creates one validated discovery result.
    ///
    /// Discovered records are additive and never authorize or reconstruct a
    /// selection; a repeated model identity is not a typed record list.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a model identity repeats.
    pub fn new(
        attempt_id: ProviderDiscoveryAttemptId,
        records: Vec<ProviderModelRecordDto>,
    ) -> DtoResult<Self> {
        let mut unique = BTreeSet::new();
        if !records
            .iter()
            .all(|record| unique.insert(record.model_id().to_owned()))
        {
            return Err(ErrorDto::validation(
                "invalid_provider_discovery_result",
                "discovered model identities must not repeat",
            ));
        }
        Ok(Self {
            attempt_id,
            records,
        })
    }

    /// Returns the discovery attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> ProviderDiscoveryAttemptId {
        self.attempt_id
    }

    /// Returns the ordered discovered records.
    #[must_use]
    pub fn records(&self) -> &[ProviderModelRecordDto] {
        &self.records
    }
}

/// A command requesting acceptance of one pending catalog removal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcceptProviderCatalogRemovalCommandDto {
    candidate: ProviderCatalogCandidateHandleDto,
    operation_id: IdempotencyKey,
}

impl AcceptProviderCatalogRemovalCommandDto {
    /// Creates an exact pending-removal acceptance request.
    #[must_use]
    pub const fn new(
        candidate: ProviderCatalogCandidateHandleDto,
        operation_id: IdempotencyKey,
    ) -> Self {
        Self {
            candidate,
            operation_id,
        }
    }

    /// Returns the exact candidate handle the caller accepted.
    #[must_use]
    pub const fn candidate(self) -> ProviderCatalogCandidateHandleDto {
        self.candidate
    }

    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn operation_id(self) -> IdempotencyKey {
        self.operation_id
    }
}

/// A command requesting rejection of one pending catalog candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RejectProviderCatalogCandidateCommandDto {
    candidate: ProviderCatalogCandidateHandleDto,
    operation_id: IdempotencyKey,
}

impl RejectProviderCatalogCandidateCommandDto {
    /// Creates an exact pending-candidate rejection request.
    #[must_use]
    pub const fn new(
        candidate: ProviderCatalogCandidateHandleDto,
        operation_id: IdempotencyKey,
    ) -> Self {
        Self {
            candidate,
            operation_id,
        }
    }

    /// Returns the exact candidate handle the caller rejected.
    #[must_use]
    pub const fn candidate(self) -> ProviderCatalogCandidateHandleDto {
        self.candidate
    }

    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn operation_id(self) -> IdempotencyKey {
        self.operation_id
    }
}

/// Acceptance evidence for one accepted catalog removal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogRemovalAcceptedDto {
    catalog_revision_id: CatalogRevisionId,
}

impl ProviderCatalogRemovalAcceptedDto {
    /// Creates complete catalog removal acceptance evidence.
    #[must_use]
    pub const fn new(catalog_revision_id: CatalogRevisionId) -> Self {
        Self {
            catalog_revision_id,
        }
    }

    /// Returns the newly accepted catalog revision identity.
    #[must_use]
    pub const fn catalog_revision_id(self) -> CatalogRevisionId {
        self.catalog_revision_id
    }
}

/// Evidence for one rejected catalog candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogCandidateRejectedDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_catalog_revision_id: Option<CatalogRevisionId>,
}

impl ProviderCatalogCandidateRejectedDto {
    /// Creates candidate rejection evidence naming the revision left active.
    #[must_use]
    pub const fn new(active_catalog_revision_id: Option<CatalogRevisionId>) -> Self {
        Self {
            active_catalog_revision_id,
        }
    }

    /// Returns the catalog revision left active after rejection.
    #[must_use]
    pub const fn active_catalog_revision_id(self) -> Option<CatalogRevisionId> {
        self.active_catalog_revision_id
    }
}

/// The closed typed value published when a session default profile changes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionProviderProfileChangedDto {
    session_id: SessionId,
    profile_id: ProviderProfileId,
    session_projection_revision: u64,
}

impl SessionProviderProfileChangedDto {
    /// Creates the typed change value for one committed durable default change.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        profile_id: ProviderProfileId,
        session_projection_revision: u64,
    ) -> Self {
        Self {
            session_id,
            profile_id,
            session_projection_revision,
        }
    }

    /// Returns the changed session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the newly durable session default profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the committed durable session projection revision.
    #[must_use]
    pub const fn session_projection_revision(&self) -> u64 {
        self.session_projection_revision
    }
}
