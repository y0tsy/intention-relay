//! Provider-neutral, validated model facts shared across domain boundaries.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{CorrelationIdDto, DtoResult, ErrorDto, ErrorRetryDto, ToolCallId};

/// A validated model-generated function call with JSON object input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolCallDto {
    call_id: ToolCallId,
    name: String,
    arguments_json: String,
}

impl<'de> Deserialize<'de> for ToolCallDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawToolCallDto {
            call_id: ToolCallId,
            name: String,
            arguments_json: String,
        }

        let raw = RawToolCallDto::deserialize(deserializer)?;
        Self::new(raw.call_id, raw.name, raw.arguments_json).map_err(de::Error::custom)
    }
}

impl ToolCallDto {
    /// Creates a function call with a non-empty name and object-shaped JSON arguments.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the name is blank or arguments are not a JSON object.
    pub fn new(
        call_id: ToolCallId,
        name: impl Into<String>,
        arguments_json: impl Into<String>,
    ) -> DtoResult<Self> {
        let name = name.into();
        let arguments_json = arguments_json.into();
        if name.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_tool_call_name",
                "tool call name must not be empty",
            ));
        }
        let _: std::collections::BTreeMap<String, serde::de::IgnoredAny> =
            serde_json::from_str(&arguments_json).map_err(|_| {
                ErrorDto::validation(
                    "invalid_tool_call_arguments",
                    "tool call arguments must be a JSON object",
                )
            })?;
        Ok(Self {
            call_id,
            name,
            arguments_json,
        })
    }

    /// Returns the daemon-assigned tool-call identity.
    #[must_use]
    pub const fn call_id(&self) -> ToolCallId {
        self.call_id
    }

    /// Returns the function name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the validated JSON object text for later typed tool decoding.
    #[must_use]
    pub fn arguments_json(&self) -> &str {
        &self.arguments_json
    }
}

/// Provider-normalized token usage with a reported or not-reported state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UsageDto {
    /// The provider did not report usage for this stream.
    NotReported,
    /// The provider reported internally consistent token counts.
    Reported {
        /// Tokens attributed to input/context.
        input_tokens: u64,
        /// Tokens attributed to model output.
        output_tokens: u64,
        /// Total tokens attributed to the request.
        total_tokens: u64,
    },
}

impl<'de> Deserialize<'de> for UsageDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "state", rename_all = "snake_case")]
        enum RawUsageDto {
            NotReported,
            Reported {
                input_tokens: u64,
                output_tokens: u64,
                total_tokens: u64,
            },
        }

        match RawUsageDto::deserialize(deserializer)? {
            RawUsageDto::NotReported => Ok(Self::NotReported),
            RawUsageDto::Reported {
                input_tokens,
                output_tokens,
                total_tokens,
            } => {
                Self::reported(input_tokens, output_tokens, total_tokens).map_err(de::Error::custom)
            }
        }
    }
}

impl UsageDto {
    /// Creates validated reported usage.
    ///
    /// # Errors
    ///
    /// Returns a validation error when total tokens do not equal input plus output.
    pub fn reported(input_tokens: u64, output_tokens: u64, total_tokens: u64) -> DtoResult<Self> {
        if input_tokens.checked_add(output_tokens) != Some(total_tokens) {
            return Err(ErrorDto::validation(
                "invalid_model_usage",
                "model usage total must equal input plus output tokens",
            ));
        }
        Ok(Self::Reported {
            input_tokens,
            output_tokens,
            total_tokens,
        })
    }
}

/// Provider-normalized terminal reason.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReasonDto {
    /// The model completed naturally.
    Stop,
    /// The requested or provider output limit was reached.
    Length,
    /// The model emitted one or more tool calls.
    ToolCalls,
    /// Provider content policy prevented normal completion.
    ContentFilter,
    /// The provider returned an explicit terminal error reason.
    Error,
    /// The provider terminated without a documented terminal reason.
    Unknown,
}

/// A safe provider-normalized failure that never includes native error text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderErrorDto {
    code: String,
    retry: ErrorRetryDto,
    correlation_id: Option<CorrelationIdDto>,
}

impl<'de> Deserialize<'de> for ProviderErrorDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawProviderErrorDto {
            code: String,
            retry: ErrorRetryDto,
            #[serde(default)]
            correlation_id: Option<CorrelationIdDto>,
        }

        let raw = RawProviderErrorDto::deserialize(deserializer)?;
        Self::new(raw.code, raw.retry, raw.correlation_id).map_err(de::Error::custom)
    }
}

impl ProviderErrorDto {
    /// Creates a safe normalized unavailable provider error.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the stable code is blank.
    pub fn unavailable(
        code: impl Into<String>,
        retryable: bool,
        correlation_id: Option<CorrelationIdDto>,
    ) -> DtoResult<Self> {
        Self::new(
            code,
            if retryable {
                ErrorRetryDto::Delayed
            } else {
                ErrorRetryDto::Never
            },
            correlation_id,
        )
    }

    fn new(
        code: impl Into<String>,
        retry: ErrorRetryDto,
        correlation_id: Option<CorrelationIdDto>,
    ) -> DtoResult<Self> {
        let code = code.into();
        if code.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_provider_error_code",
                "provider error code must not be empty",
            ));
        }
        Ok(Self {
            code,
            retry,
            correlation_id,
        })
    }

    /// Returns the stable safe provider failure code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the retry guidance selected by provider normalization.
    #[must_use]
    pub const fn retry(&self) -> ErrorRetryDto {
        self.retry
    }

    /// Returns an opaque correlation identifier when available.
    #[must_use]
    pub const fn correlation_id(&self) -> Option<CorrelationIdDto> {
        self.correlation_id
    }
}

impl Display for ProviderErrorDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.code)
    }
}
