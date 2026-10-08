//! Provider-neutral model contracts and the concrete provider adapters.
//!
//! The `model` module owns the provider-neutral contract: messages, requests,
//! capabilities, normalized stream events, and the execution boundary. The
//! `openrouter` and `generic_chat` modules translate their provider SDK traffic
//! into those DTOs; each keeps its SDK client private and exposes only its
//! driver. The `mapping` module owns the normalization rules both adapters
//! share.

mod generic_chat;
mod mapping;
mod model;
mod openrouter;

pub use generic_chat::GenericChatDriver;
pub use model::{
    AssistantReasoningDto, FinishReasonDto, ModelCancellationSignal, ModelCancelledFuture,
    ModelCapabilitiesDto, ModelEventDto, ModelEventStream, ModelExecutionDriver, ModelMessageDto,
    ModelRequestDto, ModelRoleDto, ModelStreamLifecycleDto, ModelToolDefinitionDto,
    ProviderErrorDto, ToolCallDto, UsageDto,
};
pub use openrouter::OpenRouterDriver;
