//! Shared, dependency-light DTOs for every Intention Relay boundary.
//!
//! This crate owns validated identifiers, schema versions, safe errors,
//! temporal values, the shared value and wire types used by the domain,
//! protocol, storage, and runtime boundaries, and the versioned public
//! local-protocol DTOs. It deliberately has no domain, persistence, provider,
//! runtime, or presentation dependency.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de};
use uuid::Uuid;

pub mod jsonrpc;
mod model;
mod projection;
pub mod protocol;
mod run;
mod turn;
mod workspace;

pub use jsonrpc::{
    JSONRPC_INVALID_PARAMS, JSONRPC_INVALID_REQUEST, JSONRPC_METHOD_NOT_FOUND, JSONRPC_PARSE_ERROR,
    JSONRPC_VERSION, JSONRPC_VERSION_MISMATCH, JsonRpcErrorDto, JsonRpcNotificationDto,
    JsonRpcRequestDto, JsonRpcRequestFailure, JsonRpcResponseDto, is_notification_line,
};
pub use model::{FinishReasonDto, ProviderErrorDto, ToolCallDto, UsageDto};
pub use projection::{
    MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto, SessionProjectionDto,
};
pub use protocol::{
    CURRENT_DTO_SCHEMA_VERSION, CURRENT_PROTOCOL_VERSION, CreateSessionAcceptedDto,
    DaemonHealthDto, DaemonReadinessDto, InterruptRunAcceptedDto, PROTOCOL_HELLO_METHOD,
    ProtocolAcceptedDto, ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto,
    ProtocolDaemonMessageDto, ProtocolHelloDto, ProtocolMethodDto, ProtocolQueryDto,
    ProtocolQueryResultDto, ProtocolRequestDto, ProtocolRequestPayloadDto,
    ProtocolResponsePayloadDto, ProtocolVersionDto, RUN_FRAME_METHOD, RemoveTurnAcceptedDto,
    RunStatusFrameDto, RunStreamFrameDto, RunSubscriptionResponseDto, RunSubscriptionSnapshotDto,
    SendUserTurnAcceptedDto, SendUserTurnOutcomeDto, SessionSnapshotDto,
    SessionSubscriptionResponseDto, SubscribeRunCommandDto, SubscribeSessionCommandDto,
    decode_hello_request, decode_hello_response, decode_request_line, decode_response,
    encode_hello_request, encode_hello_response, encode_request, encode_response,
    parse_run_frame_notification,
};
pub use run::{RunModeDto, RunProjectionDto, RunStatusDto};
pub use turn::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, InterruptRunCommandDto,
    RemoveTurnCommandDto, SendUserTurnCommandDto,
};
pub use workspace::WorkspaceRootDto;

/// A safe result whose failure can cross a crate or process boundary.
pub type DtoResult<T> = Result<T, ErrorDto>;

fn parse_canonical_uuid(value: &str, code: &'static str) -> DtoResult<Uuid> {
    let parsed = Uuid::parse_str(value).map_err(|_| {
        ErrorDto::validation(
            code,
            "identifier must use the canonical UUID representation",
        )
    })?;
    if parsed.to_string() != value {
        return Err(ErrorDto::validation(
            code,
            "identifier must use the canonical UUID representation",
        ));
    }
    Ok(parsed)
}

macro_rules! define_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

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
            /// Creates a new random identifier owned by the calling boundary.
            #[allow(
                clippy::new_without_default,
                reason = "A random domain identifier has no meaningful default value."
            )]
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Parses the canonical UUID representation for this identifier.
            ///
            /// # Errors
            ///
            /// Returns a safe validation error when `value` is not a canonical UUID.
            pub fn parse(value: &str) -> DtoResult<Self> {
                parse_canonical_uuid(value, "invalid_id").map(Self)
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                Display::fmt(&self.0, formatter)
            }
        }
    };
}

define_id!(SessionId, "A stable identity for one durable user session.");
define_id!(
    RunId,
    "A stable identity for one agent execution lifecycle."
);
define_id!(TurnId, "A stable identity for one conversational turn.");
define_id!(ProjectId, "A stable identity for one logical user project.");
define_id!(WorkspaceId, "A stable identity for one declared workspace.");
define_id!(ToolCallId, "A stable identity for one tool invocation.");
define_id!(
    ConfigRevisionId,
    "A stable identity for one accepted configuration revision."
);
define_id!(
    IdempotencyKey,
    "A caller-supplied identity that makes one mutating operation repeatable."
);

/// The schema version carried by persisted and transport DTOs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SchemaVersionDto {
    major: u16,
    minor: u16,
}

impl SchemaVersionDto {
    /// Creates an explicit major/minor schema version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Returns the incompatible-on-change schema component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the additive-compatible schema component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// A safe Unix timestamp represented in whole seconds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct TimestampDto(i64);

impl<'de> Deserialize<'de> for TimestampDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_unix_seconds(i64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl TimestampDto {
    /// Creates a timestamp from non-negative Unix seconds.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error for a timestamp before the Unix epoch.
    pub fn from_unix_seconds(seconds: i64) -> DtoResult<Self> {
        if seconds < 0 {
            Err(ErrorDto::validation(
                "invalid_timestamp",
                "timestamp must not precede the Unix epoch",
            ))
        } else {
            Ok(Self(seconds))
        }
    }

    /// Returns the represented whole Unix seconds.
    #[must_use]
    pub const fn unix_seconds(self) -> i64 {
        self.0
    }
}

/// A closed classification for a safe boundary error.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategoryDto {
    /// The input does not satisfy a declared DTO or configuration constraint.
    Validation,
    /// A declared security or operational policy denied the request.
    Policy,
    /// The referenced durable object does not exist.
    NotFound,
    /// The requested operation conflicts with current durable state.
    Conflict,
    /// A necessary local or remote service is unavailable.
    Unavailable,
    /// An unexpected implementation failure was safely projected.
    Internal,
}

impl ErrorCategoryDto {
    /// Returns the stable machine-readable category representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::Policy => "policy",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal",
        }
    }
}

/// A safe retry instruction for a boundary error.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorRetryDto {
    /// Retrying will not change the result without user or state changes.
    Never,
    /// Retrying immediately may succeed.
    Immediate,
    /// Retrying after a delay may succeed.
    Delayed,
    /// A user must explicitly resolve the error before retrying.
    Manual,
}

/// An opaque correlation identifier that cannot disclose diagnostic content.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CorrelationIdDto(Uuid);

impl<'de> Deserialize<'de> for CorrelationIdDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

impl CorrelationIdDto {
    /// Creates a new opaque diagnostic correlation identifier.
    #[allow(
        clippy::new_without_default,
        reason = "A random diagnostic correlation identifier has no meaningful default value."
    )]
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses the canonical UUID representation of a correlation identifier.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error if `value` is not a canonical UUID.
    pub fn parse(value: &str) -> DtoResult<Self> {
        parse_canonical_uuid(value, "invalid_correlation_id").map(Self)
    }

    /// Returns the canonical opaque identifier representation.
    #[must_use]
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

/// A normalized logical path relative to an already-authorized workspace root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct WorkspaceRelativePathDto(String);

impl<'de> Deserialize<'de> for WorkspaceRelativePathDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

impl WorkspaceRelativePathDto {
    /// Parses a safe logical workspace-relative path.
    ///
    /// # Errors
    ///
    /// Returns a validation error for blank, absolute, traversal, or
    /// control-character path input.
    pub fn parse(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        let is_invalid = value.trim().is_empty()
            || value.starts_with('/')
            || value.starts_with('\\')
            || value.contains('\\')
            || value.chars().any(char::is_control)
            || value
                .split('/')
                .any(|component| component.is_empty() || component == "." || component == "..");
        if is_invalid {
            Err(ErrorDto::validation(
                "invalid_workspace_relative_path",
                "workspace-relative path must be normalized and relative",
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the normalized logical relative path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A closed set of safe dynamic details for a boundary error.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorDetailDto {
    /// An authorized logical workspace-relative path was not found.
    ///
    /// Reserved vocabulary: no production path produces this variant today;
    /// the file-oriented tools answer `tool_read_failed` instead. It is kept as
    /// the reviewed not-found detail shape rather than deleted.
    MissingWorkspacePath {
        /// The logical path relative to the authorized workspace root.
        path: WorkspaceRelativePathDto,
    },
}

/// A safe, structured error usable at crate and process boundaries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ErrorDto {
    code: String,
    category: ErrorCategoryDto,
    message: String,
    retry: ErrorRetryDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    correlation_id: Option<CorrelationIdDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<ErrorDetailDto>,
}

impl<'de> Deserialize<'de> for ErrorDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawErrorDto {
            code: String,
            category: ErrorCategoryDto,
            message: String,
            retry: ErrorRetryDto,
            #[serde(default)]
            correlation_id: Option<CorrelationIdDto>,
            #[serde(default)]
            detail: Option<ErrorDetailDto>,
        }

        let raw = RawErrorDto::deserialize(deserializer)?;
        Self::build(
            raw.code,
            raw.category,
            raw.message,
            raw.retry,
            raw.correlation_id,
            raw.detail,
        )
        .map_err(de::Error::custom)
    }
}

impl ErrorDto {
    /// Creates a safe error after validating its stable code and human message.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the code or message is blank.
    pub fn new(
        code: impl Into<String>,
        category: ErrorCategoryDto,
        message: impl Into<String>,
        retry: ErrorRetryDto,
        correlation_id: Option<CorrelationIdDto>,
    ) -> DtoResult<Self> {
        Self::build(
            code.into(),
            category,
            message.into(),
            retry,
            correlation_id,
            None,
        )
    }

    /// Creates a safe error with a reviewed typed dynamic detail.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the code or message is blank.
    pub fn with_detail(
        code: impl Into<String>,
        category: ErrorCategoryDto,
        message: impl Into<String>,
        retry: ErrorRetryDto,
        correlation_id: Option<CorrelationIdDto>,
        detail: ErrorDetailDto,
    ) -> DtoResult<Self> {
        Self::build(
            code.into(),
            category,
            message.into(),
            retry,
            correlation_id,
            Some(detail),
        )
    }

    fn build(
        code: String,
        category: ErrorCategoryDto,
        message: String,
        retry: ErrorRetryDto,
        correlation_id: Option<CorrelationIdDto>,
        detail: Option<ErrorDetailDto>,
    ) -> DtoResult<Self> {
        if code.trim().is_empty() || message.trim().is_empty() {
            return Err(Self {
                code: "invalid_error_dto".to_owned(),
                category: ErrorCategoryDto::Validation,
                message: "error code and message must not be empty".to_owned(),
                retry: ErrorRetryDto::Never,
                correlation_id: None,
                detail: None,
            });
        }
        Ok(Self {
            code,
            category,
            message,
            retry,
            correlation_id,
            detail,
        })
    }

    /// Creates a stable validation error from trusted internal constants.
    #[must_use]
    pub fn validation(code: &'static str, message: &'static str) -> Self {
        Self {
            code: code.to_owned(),
            category: ErrorCategoryDto::Validation,
            message: message.to_owned(),
            retry: ErrorRetryDto::Manual,
            correlation_id: None,
            detail: None,
        }
    }

    /// Creates a stable unavailable error from trusted internal constants.
    #[must_use]
    pub fn unavailable(code: &'static str, message: &'static str) -> Self {
        Self {
            code: code.to_owned(),
            category: ErrorCategoryDto::Unavailable,
            message: message.to_owned(),
            retry: ErrorRetryDto::Manual,
            correlation_id: None,
            detail: None,
        }
    }

    /// Returns the stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the safe error category.
    #[must_use]
    pub const fn category(&self) -> ErrorCategoryDto {
        self.category
    }

    /// Returns the safe human-readable message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the caller guidance for retry behavior.
    #[must_use]
    pub const fn retry(&self) -> ErrorRetryDto {
        self.retry
    }

    /// Returns an opaque safe diagnostic reference when one was provided.
    #[must_use]
    pub const fn correlation_id(&self) -> Option<CorrelationIdDto> {
        self.correlation_id
    }

    /// Returns the reviewed typed dynamic detail, when one was provided.
    #[must_use]
    pub const fn detail(&self) -> Option<&ErrorDetailDto> {
        self.detail.as_ref()
    }
}

impl Display for ErrorDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ErrorDto {}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;

    #[test]
    fn schema_and_temporal_values_validate_their_boundaries() {
        let current = SchemaVersionDto::new(1, 2);
        assert_eq!(current.major(), 1);
        assert_eq!(current.minor(), 2);
        // Schema versions compare by exact equality: a differing minor is
        // rejected exactly like a differing major (no same-major tolerance).
        assert_ne!(current, SchemaVersionDto::new(1, 9));
        assert_ne!(current, SchemaVersionDto::new(2, 0));
        assert_eq!(current, SchemaVersionDto::new(1, 2));

        assert_eq!(
            TimestampDto::from_unix_seconds(4)
                .expect("positive timestamp is valid")
                .unix_seconds(),
            4
        );
        assert_eq!(
            TimestampDto::from_unix_seconds(-1)
                .expect_err("negative timestamp must fail")
                .code(),
            "invalid_timestamp"
        );
    }

    #[test]
    fn error_categories_and_fields_are_safe_and_complete() {
        let categories = [
            (ErrorCategoryDto::Validation, "validation"),
            (ErrorCategoryDto::Policy, "policy"),
            (ErrorCategoryDto::NotFound, "not_found"),
            (ErrorCategoryDto::Conflict, "conflict"),
            (ErrorCategoryDto::Unavailable, "unavailable"),
            (ErrorCategoryDto::Internal, "internal"),
        ];
        for (category, expected) in categories {
            assert_eq!(category.as_str(), expected);
        }
        let correlation = CorrelationIdDto::parse("11111111-1111-4111-8111-111111111111")
            .expect("fixture correlation identifier is valid");
        let error = ErrorDto::new(
            "fixture",
            ErrorCategoryDto::Policy,
            "safe message",
            ErrorRetryDto::Delayed,
            Some(correlation),
        )
        .expect("complete safe error is valid");
        assert_eq!(error.code(), "fixture");
        assert_eq!(error.category(), ErrorCategoryDto::Policy);
        assert_eq!(error.message(), "safe message");
        assert_eq!(error.retry(), ErrorRetryDto::Delayed);
        assert_eq!(error.correlation_id(), Some(correlation));
        assert_eq!(error.to_string(), "fixture: safe message");
        assert_eq!(
            ErrorDto::new(
                "",
                ErrorCategoryDto::Internal,
                "message",
                ErrorRetryDto::Never,
                None
            )
            .expect_err("blank code must fail")
            .code(),
            "invalid_error_dto"
        );
        assert_eq!(
            ErrorDto::new(
                "code",
                ErrorCategoryDto::Internal,
                "",
                ErrorRetryDto::Never,
                None
            )
            .expect_err("blank message must fail")
            .code(),
            "invalid_error_dto"
        );
        assert_eq!(
            ErrorDto::validation("validation", "message").retry(),
            ErrorRetryDto::Manual
        );
        assert_eq!(
            ErrorDto::unavailable("unavailable", "message").category(),
            ErrorCategoryDto::Unavailable
        );
    }
}
