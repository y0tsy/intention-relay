//! Private Chat Completions wire types for the BYOT request and stream path.
//!
//! The pinned SDK cannot express the provider's `reasoning_content` field on
//! either side of the same-run tool-loop round trip, so this module declares
//! the exact wire surface and reuses every SDK leaf type whose serialized
//! shape stays byte-identical to the non-BYOT path. With the `byot` feature
//! enabled the SDK neither injects nor validates `stream`, so the request
//! serializes the field itself.

use async_openai::types::chat::{
    ChatCompletionMessageToolCallChunk, ChatCompletionMessageToolCalls,
    ChatCompletionStreamOptions, ChatCompletionTools, CompletionUsage, FinishReason,
    ReasoningEffort,
};
use serde::{Deserialize, Serialize};

/// One outbound streaming Chat Completions request.
#[derive(Debug, Serialize)]
pub struct WireRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    pub stream: bool,
    pub stream_options: ChatCompletionStreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ChatCompletionTools>,
}

/// One outbound Chat Completions context message.
///
/// The assistant variant is the only place a round-tripped reasoning channel
/// may appear: it stays a sibling of `content` and never merges into it.
#[derive(Debug, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum WireMessage {
    /// The daemon-owned system context.
    System { content: String },
    /// One user turn.
    User { content: String },
    /// One assistant turn, optionally carrying tool calls and the reasoning
    /// text that belongs to them.
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ChatCompletionMessageToolCalls>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
    },
    /// One tool result answering a previous assistant tool call.
    Tool {
        content: String,
        tool_call_id: String,
    },
}

/// One parsed streamed Chat Completions chunk.
#[derive(Debug, Deserialize)]
pub struct WireChunk {
    #[serde(default)]
    pub choices: Vec<WireChoice>,
    #[serde(default)]
    pub usage: Option<CompletionUsage>,
}

/// One streamed choice carried by a chunk.
#[derive(Debug, Deserialize)]
pub struct WireChoice {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub delta: WireDelta,
    #[serde(default)]
    pub finish_reason: Option<FinishReason>,
}

/// One streamed delta carried by a choice.
#[derive(Debug, Default, Deserialize)]
pub struct WireDelta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ChatCompletionMessageToolCallChunk>>,
}
