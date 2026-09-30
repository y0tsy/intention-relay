//! JSON-RPC 2.0 envelopes for the local NDJSON wire.
//!
//! The local daemon speaks JSON-RPC 2.0; `intention-transport` writes one
//! serialized envelope per NDJSON line. Envelopes stay generic over their typed
//! payload, so no untyped JSON value crosses a crate boundary: this crate owns
//! the method table and parses each envelope into its typed DTO.
//!
//! Error mapping follows the specification: invalid JSON is a parse error,
//! valid JSON that is not a conformant envelope is an invalid-request error,
//! a payload that fails typed decoding is an invalid-params error, and a
//! differing local protocol version is answered with an implementation-defined
//! mismatch error before the daemon closes the connection.

use intention_types::{ErrorCategoryDto, ErrorDto, ErrorRetryDto};
use serde::de::{self, IgnoredAny};
use serde::{Deserialize, Deserializer, Serialize};

/// The JSON-RPC 2.0 version string carried by every envelope.
pub const JSONRPC_VERSION: &str = "2.0";

/// Parse error: the request line is not valid JSON.
pub const JSONRPC_PARSE_ERROR: i64 = -32700;
/// Invalid request: the line is not a conformant JSON-RPC 2.0 request.
pub const JSONRPC_INVALID_REQUEST: i64 = -32600;
/// Method not found: the requested method is not implemented.
pub const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;
/// Invalid params: the method parameters failed typed decoding.
pub const JSONRPC_INVALID_PARAMS: i64 = -32602;
/// Internal error: the daemon could not complete an otherwise valid request.
pub const JSONRPC_INTERNAL_ERROR: i64 = -32603;
/// The local protocol versions differ; the daemon answers before it closes.
pub const JSONRPC_VERSION_MISMATCH: i64 = -32001;

/// A JSON-RPC 2.0 request envelope carrying a typed parameter payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JsonRpcRequestDto<T> {
    jsonrpc: String,
    id: u64,
    method: String,
    params: T,
}

impl<T> JsonRpcRequestDto<T> {
    /// Creates a conformant request envelope with a typed payload.
    #[must_use]
    pub fn new(id: u64, method: impl Into<String>, params: T) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            method: method.into(),
            params,
        }
    }

    /// Returns the request identity echoed by the matching response.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Returns the requested method name.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Returns the typed parameter payload.
    #[must_use]
    pub const fn params(&self) -> &T {
        &self.params
    }

    /// Consumes the envelope and returns its typed parameter payload.
    #[must_use]
    pub fn into_params(self) -> T {
        self.params
    }
}

impl<T: de::DeserializeOwned> JsonRpcRequestDto<T> {
    /// Parses one request line with the standard JSON-RPC error mapping.
    ///
    /// # Errors
    ///
    /// Returns a parse error for invalid JSON, an invalid-request error for a
    /// non-conformant envelope, and an invalid-params error when the payload
    /// does not decode into `T`.
    pub fn parse(line: &str) -> Result<Self, JsonRpcRequestFailure> {
        let header = JsonRpcRequestHeader::parse(line)?;
        let request: Self = serde_json::from_str(line)
            .map_err(|_| JsonRpcRequestFailure::invalid_params(Some(header.id)))?;
        Ok(request)
    }
}

/// A JSON-RPC 2.0 response envelope; exactly one outcome is present.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct JsonRpcResponseDto<T> {
    jsonrpc: String,
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcErrorDto>,
}

impl<T> JsonRpcResponseDto<T> {
    /// Creates a successful response carrying the typed result.
    #[must_use]
    pub fn result(id: u64, result: T) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    /// Creates an error response; the identity is absent for unparseable lines.
    #[must_use]
    pub fn error(id: Option<u64>, error: JsonRpcErrorDto) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Returns the echoed request identity when one could be recovered.
    #[must_use]
    pub const fn id(&self) -> Option<u64> {
        self.id
    }

    /// Returns the typed result when this response succeeded.
    #[must_use]
    pub const fn result_value(&self) -> Option<&T> {
        self.result.as_ref()
    }

    /// Returns the error when this response failed.
    #[must_use]
    pub const fn error_value(&self) -> Option<&JsonRpcErrorDto> {
        self.error.as_ref()
    }

    /// Consumes the envelope and returns the typed result when it succeeded.
    #[must_use]
    pub fn into_result(self) -> Option<T> {
        self.result
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for JsonRpcResponseDto<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawResponseDto<T> {
            jsonrpc: String,
            id: Option<u64>,
            result: Option<T>,
            error: Option<JsonRpcErrorDto>,
        }

        let raw = RawResponseDto::<T>::deserialize(deserializer)?;
        build_response(raw.jsonrpc, raw.id, raw.result, raw.error).map_err(de::Error::custom)
    }
}

fn build_response<T>(
    jsonrpc: String,
    id: Option<u64>,
    result: Option<T>,
    error: Option<JsonRpcErrorDto>,
) -> Result<JsonRpcResponseDto<T>, ErrorDto> {
    if jsonrpc != JSONRPC_VERSION {
        return Err(ErrorDto::validation(
            "invalid_jsonrpc_version",
            "the JSON-RPC version must be exactly 2.0",
        ));
    }
    match (result, error) {
        (Some(result), None) => Ok(JsonRpcResponseDto {
            jsonrpc,
            id,
            result: Some(result),
            error: None,
        }),
        (None, Some(error)) => Ok(JsonRpcResponseDto {
            jsonrpc,
            id,
            result: None,
            error: Some(error),
        }),
        _ => Err(ErrorDto::validation(
            "invalid_jsonrpc_response",
            "exactly one of result or error must be present",
        )),
    }
}

impl<T: de::DeserializeOwned> JsonRpcResponseDto<T> {
    /// Parses one response line.
    ///
    /// # Errors
    ///
    /// Returns a parse error for invalid JSON and an invalid-request error for
    /// a non-conformant response envelope.
    pub fn parse(line: &str) -> Result<Self, JsonRpcErrorDto> {
        let _: IgnoredAny =
            serde_json::from_str(line).map_err(|_| JsonRpcErrorDto::parse_error())?;
        serde_json::from_str(line).map_err(|_| JsonRpcErrorDto::invalid_request())
    }
}

/// A JSON-RPC 2.0 notification envelope carrying a typed payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JsonRpcNotificationDto<T> {
    jsonrpc: String,
    method: String,
    params: T,
}

impl<T> JsonRpcNotificationDto<T> {
    /// Creates a conformant notification envelope with a typed payload.
    #[must_use]
    pub fn new(method: impl Into<String>, params: T) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            method: method.into(),
            params,
        }
    }

    /// Returns the notification method name.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Returns the typed notification payload.
    #[must_use]
    pub const fn params(&self) -> &T {
        &self.params
    }

    /// Consumes the envelope and returns its typed payload.
    #[must_use]
    pub fn into_params(self) -> T {
        self.params
    }
}

impl<T: de::DeserializeOwned> JsonRpcNotificationDto<T> {
    /// Parses one notification line.
    ///
    /// # Errors
    ///
    /// Returns a parse error for invalid JSON and an invalid-request error for
    /// a non-conformant notification envelope (including one carrying an id).
    pub fn parse(line: &str) -> Result<Self, JsonRpcErrorDto> {
        let _: IgnoredAny =
            serde_json::from_str(line).map_err(|_| JsonRpcErrorDto::parse_error())?;
        #[derive(Deserialize)]
        struct RawNotificationHeaderDto {
            jsonrpc: String,
            #[serde(default)]
            id: Option<u64>,
            method: String,
        }
        let header: RawNotificationHeaderDto =
            serde_json::from_str(line).map_err(|_| JsonRpcErrorDto::invalid_request())?;
        if header.jsonrpc != JSONRPC_VERSION || header.method.is_empty() || header.id.is_some() {
            return Err(JsonRpcErrorDto::invalid_request());
        }
        serde_json::from_str(line).map_err(|_| JsonRpcErrorDto::invalid_request())
    }
}

/// A JSON-RPC 2.0 error object carrying the stable typed error in `data`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JsonRpcErrorDto {
    code: i64,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<Box<ErrorDto>>,
}

impl JsonRpcErrorDto {
    /// Creates a JSON-RPC error with an optional stable typed detail.
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>, data: Option<ErrorDto>) -> Self {
        Self {
            code,
            message: message.into(),
            data: data.map(Box::new),
        }
    }

    /// Creates the parse error for a line that is not valid JSON.
    #[must_use]
    pub fn parse_error() -> Self {
        Self::new(
            JSONRPC_PARSE_ERROR,
            "the request line is not valid JSON",
            Some(ErrorDto::validation(
                "jsonrpc_parse_error",
                "the request line is not valid JSON",
            )),
        )
    }

    /// Creates the invalid-request error for a non-conformant envelope.
    #[must_use]
    pub fn invalid_request() -> Self {
        Self::new(
            JSONRPC_INVALID_REQUEST,
            "the line is not a conformant JSON-RPC 2.0 request",
            Some(ErrorDto::validation(
                "jsonrpc_invalid_request",
                "the request envelope is not conformant",
            )),
        )
    }

    /// Creates the method-not-found error for an unknown method name.
    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        let message = format!("the method {method} is not implemented");
        let data = ErrorDto::new(
            "jsonrpc_method_not_found",
            ErrorCategoryDto::Validation,
            message.clone(),
            ErrorRetryDto::Manual,
            None,
        )
        .unwrap_or_else(|_| {
            ErrorDto::validation("jsonrpc_method_not_found", "the method is not implemented")
        });
        Self::new(JSONRPC_METHOD_NOT_FOUND, message, Some(data))
    }

    /// Creates the invalid-params error for a payload that failed decoding.
    #[must_use]
    pub fn invalid_params() -> Self {
        Self::new(
            JSONRPC_INVALID_PARAMS,
            "the method parameters are invalid",
            Some(ErrorDto::validation(
                "jsonrpc_invalid_params",
                "the method parameters failed typed decoding",
            )),
        )
    }

    /// Creates an error object from a stable typed error and an explicit code.
    #[must_use]
    pub fn from_error(code: i64, error: ErrorDto) -> Self {
        Self::new(code, error.message().to_owned(), Some(error))
    }

    /// Returns the JSON-RPC error code.
    #[must_use]
    pub const fn code(&self) -> i64 {
        self.code
    }

    /// Returns the safe human-readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the stable typed error detail when one is present.
    #[must_use]
    pub fn data(&self) -> Option<&ErrorDto> {
        self.data.as_deref()
    }

    /// Converts this error object into the stable typed error clients consume.
    #[must_use]
    pub fn to_error(&self) -> ErrorDto {
        self.data.as_deref().cloned().unwrap_or_else(|| {
            ErrorDto::unavailable(
                "local_daemon_jsonrpc_error",
                "the local daemon returned a JSON-RPC error",
            )
        })
    }
}

/// A request that failed JSON-RPC validation, with its recovered identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonRpcRequestFailure {
    id: Option<u64>,
    error: JsonRpcErrorDto,
}

impl JsonRpcRequestFailure {
    pub(crate) const fn new(id: Option<u64>, error: JsonRpcErrorDto) -> Self {
        Self { id, error }
    }

    pub(crate) fn invalid_params(id: Option<u64>) -> Self {
        Self::new(id, JsonRpcErrorDto::invalid_params())
    }

    /// Returns the request identity when the envelope could be recovered.
    #[must_use]
    pub const fn id(&self) -> Option<u64> {
        self.id
    }

    /// Returns the JSON-RPC error object that answers the failed request.
    #[must_use]
    pub const fn error(&self) -> &JsonRpcErrorDto {
        &self.error
    }

    /// Consumes the failure into its recovered identity and error object.
    #[must_use]
    pub fn into_parts(self) -> (Option<u64>, JsonRpcErrorDto) {
        (self.id, self.error)
    }
}

/// The envelope fields needed to classify a request before payload decoding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct JsonRpcRequestHeader {
    jsonrpc: String,
    id: u64,
    method: String,
}

impl JsonRpcRequestHeader {
    pub(crate) fn parse(line: &str) -> Result<Self, JsonRpcRequestFailure> {
        let _: IgnoredAny = serde_json::from_str(line)
            .map_err(|_| JsonRpcRequestFailure::new(None, JsonRpcErrorDto::parse_error()))?;
        let header: Self = serde_json::from_str(line)
            .map_err(|_| JsonRpcRequestFailure::new(None, JsonRpcErrorDto::invalid_request()))?;
        if header.jsonrpc != JSONRPC_VERSION || header.method.is_empty() {
            return Err(JsonRpcRequestFailure::new(
                Some(header.id),
                JsonRpcErrorDto::invalid_request(),
            ));
        }
        Ok(header)
    }

    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn method(&self) -> &str {
        &self.method
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Envelope conformance fixtures use direct assertions for diagnostics."
    )]

    use super::*;

    #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct FixtureParams {
        value: u64,
    }

    fn fixture(value: u64) -> FixtureParams {
        FixtureParams { value }
    }

    #[test]
    fn requests_round_trip_and_expose_their_fields() {
        let request = JsonRpcRequestDto::new(7, "fixture.method", fixture(3));
        assert_eq!(request.id(), 7);
        assert_eq!(request.method(), "fixture.method");
        assert_eq!(request.params(), &fixture(3));
        let wire = serde_json::to_string(&request).expect("request serializes");
        assert!(wire.contains(r#""jsonrpc":"2.0""#));
        let decoded: JsonRpcRequestDto<FixtureParams> =
            JsonRpcRequestDto::parse(&wire).expect("request parses");
        assert_eq!(decoded, request);
        assert_eq!(decoded.into_params(), fixture(3));
    }

    #[test]
    fn request_failures_map_to_standard_error_codes() {
        let parse = JsonRpcRequestDto::<FixtureParams>::parse("{").expect_err("syntax fails");
        assert_eq!(parse.error().code(), JSONRPC_PARSE_ERROR);
        assert_eq!(parse.id(), None);

        let shape = JsonRpcRequestDto::<FixtureParams>::parse("{}").expect_err("shape fails");
        assert_eq!(shape.error().code(), JSONRPC_INVALID_REQUEST);
        assert_eq!(shape.id(), None);

        let version = JsonRpcRequestDto::<FixtureParams>::parse(
            r#"{"jsonrpc":"1.0","id":9,"method":"fixture.method","params":{"value":1}}"#,
        )
        .expect_err("version fails");
        assert_eq!(version.error().code(), JSONRPC_INVALID_REQUEST);
        assert_eq!(version.id(), Some(9));

        let params = JsonRpcRequestDto::<FixtureParams>::parse(
            r#"{"jsonrpc":"2.0","id":9,"method":"fixture.method","params":{"value":"text"}}"#,
        )
        .expect_err("params fail");
        assert_eq!(params.error().code(), JSONRPC_INVALID_PARAMS);
        assert_eq!(params.id(), Some(9));
        let (id, error) = params.into_parts();
        assert_eq!(id, Some(9));
        assert_eq!(error.to_error().code(), "jsonrpc_invalid_params");
    }

    #[test]
    fn responses_carry_exactly_one_outcome() {
        let success = JsonRpcResponseDto::result(4, fixture(1));
        assert_eq!(success.id(), Some(4));
        assert_eq!(success.result_value(), Some(&fixture(1)));
        assert!(success.error_value().is_none());
        let wire = serde_json::to_string(&success).expect("result serializes");
        let decoded: JsonRpcResponseDto<FixtureParams> =
            JsonRpcResponseDto::parse(&wire).expect("result parses");
        assert_eq!(decoded.into_result(), Some(fixture(1)));

        let failure =
            JsonRpcResponseDto::<FixtureParams>::error(Some(4), JsonRpcErrorDto::invalid_params());
        let wire = serde_json::to_string(&failure).expect("error serializes");
        let decoded: JsonRpcResponseDto<FixtureParams> =
            JsonRpcResponseDto::parse(&wire).expect("error parses");
        assert!(decoded.result_value().is_none());
        assert_eq!(
            decoded.error_value().map(JsonRpcErrorDto::code),
            Some(JSONRPC_INVALID_PARAMS)
        );

        let both = JsonRpcResponseDto::<FixtureParams>::parse(
            r#"{"jsonrpc":"2.0","id":1,"result":{"value":1},"error":{"code":-32600,"message":"x"}}"#,
        )
        .expect_err("both outcomes are rejected");
        assert_eq!(both.code(), JSONRPC_INVALID_REQUEST);
        let neither = JsonRpcResponseDto::<FixtureParams>::parse(r#"{"jsonrpc":"2.0","id":1}"#)
            .expect_err("no outcome is rejected");
        assert_eq!(neither.code(), JSONRPC_INVALID_REQUEST);
        let wrong_version = JsonRpcResponseDto::<FixtureParams>::parse(
            r#"{"jsonrpc":"1.0","id":1,"result":{"value":1}}"#,
        )
        .expect_err("version is checked");
        assert_eq!(wrong_version.code(), JSONRPC_INVALID_REQUEST);
    }

    #[test]
    fn notifications_round_trip_without_an_identity() {
        let notification = JsonRpcNotificationDto::new("run.frame", fixture(2));
        assert_eq!(notification.method(), "run.frame");
        assert_eq!(notification.params(), &fixture(2));
        let wire = serde_json::to_string(&notification).expect("notification serializes");
        let decoded: JsonRpcNotificationDto<FixtureParams> =
            JsonRpcNotificationDto::parse(&wire).expect("notification parses");
        assert_eq!(decoded.into_params(), fixture(2));

        let with_id = JsonRpcNotificationDto::<FixtureParams>::parse(
            r#"{"jsonrpc":"2.0","id":3,"method":"run.frame","params":{"value":2}}"#,
        )
        .expect_err("notifications forbid ids");
        assert_eq!(with_id.code(), JSONRPC_INVALID_REQUEST);
    }

    #[test]
    fn error_helpers_use_standard_codes_and_typed_data() {
        assert_eq!(JsonRpcErrorDto::parse_error().code(), JSONRPC_PARSE_ERROR);
        assert_eq!(
            JsonRpcErrorDto::invalid_request().code(),
            JSONRPC_INVALID_REQUEST
        );
        assert_eq!(
            JsonRpcErrorDto::method_not_found("fixture.method").code(),
            JSONRPC_METHOD_NOT_FOUND
        );
        assert_eq!(
            JsonRpcErrorDto::invalid_params().code(),
            JSONRPC_INVALID_PARAMS
        );
        assert_eq!(
            JsonRpcErrorDto::from_error(
                JSONRPC_VERSION_MISMATCH,
                ErrorDto::unavailable(
                    "incompatible_protocol_version",
                    "protocol version must equal the current version"
                ),
            )
            .code(),
            JSONRPC_VERSION_MISMATCH
        );
        let typed = JsonRpcErrorDto::invalid_params().to_error();
        assert_eq!(typed.code(), "jsonrpc_invalid_params");
        let foreign = JsonRpcErrorDto::new(JSONRPC_INTERNAL_ERROR, "boom", None).to_error();
        assert_eq!(foreign.code(), "local_daemon_jsonrpc_error");
    }
}
