//! Provider-neutral model contracts and the concrete provider adapters.
//!
//! The `model` module owns the provider-neutral contract: messages, requests,
//! capabilities, normalized stream events, and the execution boundary. The
//! `openrouter` and `generic_chat` modules translate their provider SDK traffic
//! into those DTOs; each keeps its SDK client private and exposes only its
//! driver and its closed driver options. The `mapping` module owns the
//! normalization rules both adapters share, the `stream` module owns the one
//! normalized event stream they both drive, the `auth` module owns the
//! authentication header policy an adapter applies at construction, and the
//! `descriptor` module owns the code-owned first-party kind descriptors and
//! their compatibility matrix.

mod auth;
mod descriptor;
mod generic_chat;
mod mapping;
mod model;
mod openrouter;
mod stream;

pub use auth::AuthenticationHeaderPolicyV1;
pub use descriptor::{
    GENERIC_CHAT_KIND_ID, OPENROUTER_KIND_ID, driver_capabilities, driver_contract,
    first_party_kind_descriptors, validate_capability_subset, validate_user_kind,
};
pub use generic_chat::{GenericChatDriver, GenericChatDriverOptions};
pub use model::{
    AssistantReasoningDto, AssistantReasoningHistoryDto, FinishReasonDto,
    MAX_MODEL_REASONING_FRAGMENT_BYTES, ModelCancellationSignal, ModelCancelledFuture,
    ModelCapabilitiesDto, ModelEventDto, ModelEventStream, ModelExecutionDriver, ModelMessageDto,
    ModelRequestDto, ModelRoleDto, ModelStreamLifecycleDto, ModelToolDefinitionDto,
    ProviderErrorDto, ProviderHealthEvidenceDto, ProviderHealthReasonDto, ProviderHealthStateDto,
    ProviderModelRecordDto, ProviderProbeFuture, ReasoningFragmentCategoryDto, ToolCallDto,
    UsageDto,
};
pub use openrouter::{OpenRouterDriver, OpenRouterDriverOptions};
