//! Provider-neutral model contracts and validated stream facts.
//!
//! This crate contains no provider SDK, asynchronous runtime, or transport
//! resources. Provider crates translate their native responses into these
//! validated DTOs before crossing the provider boundary.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

use futures_core::Stream;
use intention_types::{DtoResult, ErrorDto, ErrorRetryDto, RunId};
pub use intention_types::{FinishReasonDto, ProviderErrorDto, ToolCallDto, UsageDto};
use serde::{Deserialize, Deserializer, Serialize, de};

/// The sender role of a text-only model-context message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRoleDto {
    /// Daemon-selected model instruction context.
    System,
    /// User-provided turn content.
    User,
    /// A prior normalized assistant response.
    Assistant,
}

/// A validated text-only model-context message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelMessageDto {
    role: ModelRoleDto,
    content: String,
}

impl<'de> Deserialize<'de> for ModelMessageDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawModelMessageDto {
            role: ModelRoleDto,
            content: String,
        }

        let raw = RawModelMessageDto::deserialize(deserializer)?;
        Self::new(raw.role, raw.content).map_err(de::Error::custom)
    }
}

impl ModelMessageDto {
    /// Creates a non-blank text-only context message.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank.
    pub fn new(role: ModelRoleDto, content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_message_content",
                "model message content must not be empty",
            ));
        }
        Ok(Self { role, content })
    }

    /// Returns the message role.
    #[must_use]
    pub const fn role(&self) -> ModelRoleDto {
        self.role
    }

    /// Returns the text-only message content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// Requested model-context capabilities that require preflight support.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModelRequestedCapabilitiesDto {
    reasoning: bool,
    multimodal: bool,
    tool_calls: bool,
    vendor_extensions: bool,
}

impl ModelRequestedCapabilitiesDto {
    /// Creates an explicit request capability set.
    #[must_use]
    pub const fn new(
        reasoning: bool,
        multimodal: bool,
        tool_calls: bool,
        vendor_extensions: bool,
    ) -> Self {
        Self {
            reasoning,
            multimodal,
            tool_calls,
            vendor_extensions,
        }
    }

    /// Returns whether reasoning output was requested.
    #[must_use]
    pub const fn reasoning(self) -> bool {
        self.reasoning
    }

    /// Returns whether multimodal input or output was requested.
    #[must_use]
    pub const fn multimodal(self) -> bool {
        self.multimodal
    }

    /// Returns whether function-style tool calls were requested.
    #[must_use]
    pub const fn tool_calls(self) -> bool {
        self.tool_calls
    }

    /// Returns whether provider-specific extensions were requested.
    #[must_use]
    pub const fn vendor_extensions(self) -> bool {
        self.vendor_extensions
    }
}

/// The explicit capabilities declared by a selected provider driver.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModelCapabilitiesDto {
    text: bool,
    reasoning: bool,
    tool_calls: bool,
    multimodal: bool,
    vendor_extensions: bool,
    streaming: bool,
}

impl ModelCapabilitiesDto {
    /// Creates an explicit capability declaration.
    #[must_use]
    pub const fn new(
        text: bool,
        reasoning: bool,
        tool_calls: bool,
        multimodal: bool,
        vendor_extensions: bool,
        streaming: bool,
    ) -> Self {
        Self {
            text,
            reasoning,
            tool_calls,
            multimodal,
            vendor_extensions,
            streaming,
        }
    }

    /// Returns whether text input/output is supported.
    #[must_use]
    pub const fn supports_text(self) -> bool {
        self.text
    }

    /// Returns whether reasoning output is supported.
    #[must_use]
    pub const fn supports_reasoning(self) -> bool {
        self.reasoning
    }

    /// Returns whether tool calls are supported.
    #[must_use]
    pub const fn supports_tool_calls(self) -> bool {
        self.tool_calls
    }

    /// Returns whether multimodal input/output is supported.
    #[must_use]
    pub const fn supports_multimodal(self) -> bool {
        self.multimodal
    }

    /// Returns whether provider-specific extensions are supported.
    #[must_use]
    pub const fn supports_vendor_extensions(self) -> bool {
        self.vendor_extensions
    }

    /// Returns whether streaming is supported.
    #[must_use]
    pub const fn supports_streaming(self) -> bool {
        self.streaming
    }

    /// Rejects a request requiring an undeclared capability before any outbound call.
    ///
    /// # Errors
    ///
    /// Returns a policy error when the requested model behavior is unsupported.
    pub fn ensure_supports(self, requested: ModelRequestedCapabilitiesDto) -> DtoResult<()> {
        let unsupported = (requested.reasoning && !self.reasoning)
            || (requested.multimodal && !self.multimodal)
            || (requested.tool_calls && !self.tool_calls)
            || (requested.vendor_extensions && !self.vendor_extensions);
        if unsupported {
            Err(ErrorDto::new(
                "unsupported_model_capability",
                intention_types::ErrorCategoryDto::Policy,
                "the selected provider does not support the requested model capability",
                ErrorRetryDto::Never,
                None,
            )?)
        } else {
            Ok(())
        }
    }
}

/// A validated provider-neutral model request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelRequestDto {
    run_id: RunId,
    model: String,
    messages: Vec<ModelMessageDto>,
    system_context: Option<String>,
    requested_capabilities: ModelRequestedCapabilitiesDto,
}

impl<'de> Deserialize<'de> for ModelRequestDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawModelRequestDto {
            run_id: RunId,
            model: String,
            messages: Vec<ModelMessageDto>,
            #[serde(default)]
            system_context: Option<String>,
            #[serde(default)]
            requested_capabilities: ModelRequestedCapabilitiesDto,
        }

        let raw = RawModelRequestDto::deserialize(deserializer)?;
        Self::new(
            raw.run_id,
            raw.model,
            raw.messages,
            raw.system_context,
            Some(raw.requested_capabilities),
        )
        .map_err(de::Error::custom)
    }
}

impl ModelRequestDto {
    /// Creates a text-only provider-neutral model request.
    ///
    /// # Errors
    ///
    /// Returns a validation error for a blank model, an empty message list, or a blank system context.
    pub fn new(
        run_id: RunId,
        model: impl Into<String>,
        messages: Vec<ModelMessageDto>,
        system_context: Option<String>,
        requested_capabilities: Option<ModelRequestedCapabilitiesDto>,
    ) -> DtoResult<Self> {
        let model = model.into();
        if model.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_model_identifier",
                "model identifier must not be empty",
            ));
        }
        if messages.is_empty() {
            return Err(ErrorDto::validation(
                "missing_model_messages",
                "model request must contain at least one message",
            ));
        }
        if system_context
            .as_ref()
            .is_some_and(|context| context.trim().is_empty())
        {
            return Err(ErrorDto::validation(
                "invalid_model_system_context",
                "model system context must not be empty when provided",
            ));
        }
        Ok(Self {
            run_id,
            model,
            messages,
            system_context,
            requested_capabilities: requested_capabilities.unwrap_or_default(),
        })
    }

    /// Returns the daemon-owned run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the selected model identifier.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the text-only model context messages.
    #[must_use]
    pub fn messages(&self) -> &[ModelMessageDto] {
        &self.messages
    }

    /// Returns optional daemon-selected system context.
    #[must_use]
    pub fn system_context(&self) -> Option<&str> {
        self.system_context.as_deref()
    }

    /// Returns capability requirements for preflight.
    #[must_use]
    pub const fn requested_capabilities(&self) -> ModelRequestedCapabilitiesDto {
        self.requested_capabilities
    }
}

/// A provider-neutral normalized stream fact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelEventDto {
    /// The provider stream started successfully.
    Started,
    /// A non-empty text content delta arrived.
    TextDelta { content: String },
    /// A non-empty reasoning delta arrived.
    ReasoningDelta { content: String },
    /// A complete provider-normalized tool call arrived.
    ToolCall { call: ToolCallDto },
    /// Final usage became available.
    Usage { usage: UsageDto },
    /// The provider stream reached a terminal reason.
    Finished { reason: FinishReasonDto },
}

impl<'de> Deserialize<'de> for ModelEventDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum RawModelEventDto {
            Started,
            TextDelta { content: String },
            ReasoningDelta { content: String },
            ToolCall { call: ToolCallDto },
            Usage { usage: UsageDto },
            Finished { reason: FinishReasonDto },
        }

        match RawModelEventDto::deserialize(deserializer)? {
            RawModelEventDto::Started => Ok(Self::started()),
            RawModelEventDto::TextDelta { content } => {
                Self::text_delta(content).map_err(de::Error::custom)
            }
            RawModelEventDto::ReasoningDelta { content } => {
                Self::reasoning_delta(content).map_err(de::Error::custom)
            }
            RawModelEventDto::ToolCall { call } => Ok(Self::tool_call(call)),
            RawModelEventDto::Usage { usage } => Ok(Self::usage(usage)),
            RawModelEventDto::Finished { reason } => Ok(Self::finished(reason)),
        }
    }
}

impl ModelEventDto {
    /// Creates a stream-start fact.
    #[must_use]
    pub const fn started() -> Self {
        Self::Started
    }

    /// Creates a non-empty text content delta.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the delta is empty.
    pub fn text_delta(content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.is_empty() {
            Err(ErrorDto::validation(
                "invalid_model_text_delta",
                "model text delta must not be empty",
            ))
        } else {
            Ok(Self::TextDelta { content })
        }
    }

    /// Creates a non-empty reasoning delta.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the delta is empty.
    pub fn reasoning_delta(content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.is_empty() {
            Err(ErrorDto::validation(
                "invalid_model_reasoning_delta",
                "model reasoning delta must not be empty",
            ))
        } else {
            Ok(Self::ReasoningDelta { content })
        }
    }

    /// Creates a complete normalized tool-call fact.
    #[must_use]
    pub const fn tool_call(call: ToolCallDto) -> Self {
        Self::ToolCall { call }
    }

    /// Creates a normalized usage fact.
    #[must_use]
    pub const fn usage(usage: UsageDto) -> Self {
        Self::Usage { usage }
    }

    /// Creates a terminal stream fact.
    #[must_use]
    pub const fn finished(reason: FinishReasonDto) -> Self {
        Self::Finished { reason }
    }
}

/// Validates normalized model-stream ordering without owning runtime delivery.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModelStreamLifecycleDto {
    started: bool,
    terminal: bool,
    usage_seen: bool,
}

impl ModelStreamLifecycleDto {
    /// Creates a stream validator before the provider emits a start fact.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            started: false,
            terminal: false,
            usage_seen: false,
        }
    }

    /// Accepts one ordered normalized event.
    ///
    /// # Errors
    ///
    /// Returns a validation error if the event cannot occur at this stream position.
    pub fn accept(&mut self, event: &ModelEventDto) -> DtoResult<()> {
        match event {
            ModelEventDto::Started if !self.started && !self.terminal => {
                self.started = true;
                Ok(())
            }
            ModelEventDto::Started => Err(stream_order_error()),
            ModelEventDto::Finished { .. } if self.started && !self.terminal => {
                self.terminal = true;
                Ok(())
            }
            ModelEventDto::Usage { .. } if self.started && !self.terminal && !self.usage_seen => {
                self.usage_seen = true;
                Ok(())
            }
            ModelEventDto::TextDelta { .. }
            | ModelEventDto::ReasoningDelta { .. }
            | ModelEventDto::ToolCall { .. }
                if self.started && !self.terminal =>
            {
                Ok(())
            }
            ModelEventDto::Usage { .. } | ModelEventDto::Finished { .. } => {
                Err(stream_order_error())
            }
            ModelEventDto::TextDelta { .. }
            | ModelEventDto::ReasoningDelta { .. }
            | ModelEventDto::ToolCall { .. } => Err(stream_order_error()),
        }
    }

    /// Returns whether a terminal fact was accepted.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        self.terminal
    }
}

fn stream_order_error() -> ErrorDto {
    ErrorDto::validation(
        "invalid_model_stream_order",
        "model stream event order is invalid",
    )
}

/// Provider-neutral stream driver boundary. SDK and runtime resources stay private to providers/runtime.
pub trait ModelDriver {
    /// Returns the static capability declaration for this configured driver.
    fn capabilities(&self) -> ModelCapabilitiesDto;

    /// Validates whether this driver can accept the request before outbound work begins.
    ///
    /// # Errors
    ///
    /// Returns a policy error when a request requires unsupported capability.
    fn preflight(&self, request: &ModelRequestDto) -> DtoResult<()> {
        self.capabilities()
            .ensure_supports(request.requested_capabilities())
    }
}

/// Provider-neutral cancellation state shared with a model execution stream.
#[derive(Clone, Default)]
pub struct ModelCancellationSignal {
    state: Arc<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    next_waiter_id: AtomicUsize,
    waiters: Mutex<Vec<(usize, Waker)>>,
}

impl ModelCancellationSignal {
    /// Creates a cancellation signal in its active state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    /// Requests cancellation and wakes every current waiter.
    pub fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            let waiters = match self.state.waiters.lock() {
                Ok(mut waiters) => std::mem::take(&mut *waiters),
                Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
            };
            for (_, waiter) in waiters {
                waiter.wake();
            }
        }
    }

    /// Returns a fresh independently awaitable future that completes on cancellation.
    #[must_use]
    pub fn cancelled(&self) -> ModelCancelledFuture {
        ModelCancelledFuture {
            state: self.state.clone(),
            waiter_id: None,
        }
    }
}

/// Provider-neutral future returned by [`ModelCancellationSignal::cancelled`].
pub struct ModelCancelledFuture {
    state: Arc<CancellationState>,
    waiter_id: Option<usize>,
}

impl Future for ModelCancelledFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.state.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        let waiter_id = self.waiter_id.unwrap_or_else(|| {
            let waiter_id = self.state.next_waiter_id.fetch_add(1, Ordering::Relaxed);
            self.waiter_id = Some(waiter_id);
            waiter_id
        });
        match self.state.waiters.lock() {
            Ok(mut waiters) => replace_waiter(&mut waiters, waiter_id, context.waker()),
            Err(poisoned) => {
                let mut waiters = poisoned.into_inner();
                replace_waiter(&mut waiters, waiter_id, context.waker());
            }
        }
        if self.state.cancelled.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for ModelCancelledFuture {
    fn drop(&mut self) {
        let Some(waiter_id) = self.waiter_id else {
            return;
        };
        match self.state.waiters.lock() {
            Ok(mut waiters) => remove_waiter(&mut waiters, waiter_id),
            Err(poisoned) => remove_waiter(&mut poisoned.into_inner(), waiter_id),
        }
    }
}

fn replace_waiter(waiters: &mut Vec<(usize, Waker)>, waiter_id: usize, waker: &Waker) {
    if let Some((_, current)) = waiters.iter_mut().find(|(id, _)| *id == waiter_id) {
        if !current.will_wake(waker) {
            *current = waker.clone();
        }
    } else {
        waiters.push((waiter_id, waker.clone()));
    }
}

fn remove_waiter(waiters: &mut Vec<(usize, Waker)>, waiter_id: usize) {
    waiters.retain(|(id, _)| *id != waiter_id);
}

/// Ordered provider-neutral execution stream.
pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = Result<ModelEventDto, ProviderErrorDto>> + Send>>;

/// Provider-neutral asynchronous execution boundary.
pub trait ModelExecutionDriver: ModelDriver {
    /// Starts a validated model request and returns ordered normalized provider events.
    fn execute(
        &self,
        request: ModelRequestDto,
        cancellation: ModelCancellationSignal,
    ) -> ModelEventStream;
}
