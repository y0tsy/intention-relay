//! Dynamic context-window accounting, sliding compression, and prompt-cache
//! breakpoints.
//!
//! The executor owns one [`ContextWindowState`] per provider attempt. The state
//! estimates the input size of the message list it is about to send, compresses
//! the largest tool results when that estimate crosses the configured sliding
//! window, and recomputes the prompt-cache breakpoints of the request that
//! follows.
//!
//! Messages are never removed and never reordered: a compression replaces one
//! message's content in place, so the assistant `tool_calls` message and its
//! tool replies stay paired.
//!
//! The estimate is calibrated from provider-reported usage whenever one
//! arrives; every uncalibrated increment is estimated at four characters per
//! token.
//!
//! The measured input is the whole request: message content, the tool-call
//! arguments of every assistant message, and the transient reasoning
//! attachments carried into the same-run tool-loop continuation.

use intention_proto::DtoResult;
use intention_providers::{
    AssistantReasoningDto, ModelMessageDto, ModelRequestDto, ModelRoleDto, UsageDto,
};

/// Characters per estimated token for every uncalibrated increment.
const ESTIMATED_CHARACTERS_PER_TOKEN: usize = 4;

/// Minimum characters of one compressed tool result.
const MIN_TOOL_RESULT_CHARACTERS: usize = 16;

/// Marker appended to the retained prefix of one compressed tool result.
const COMPRESSED_TOOL_RESULT_MARKER: &str = " [compressed]";

/// Fixed placeholder used when a compressed result keeps no text at all.
const COMPRESSED_TOOL_RESULT_PLACEHOLDER: &str = "[tool result compressed]";

/// Dynamic input-size accounting for one provider attempt.
pub struct ContextWindowState {
    window_tokens: u64,
    calibrated_input_tokens: Option<u64>,
    calibrated_characters: usize,
}

impl ContextWindowState {
    /// Creates the accounting state for one attempt from its frozen configuration.
    pub const fn new(window_tokens: u64) -> Self {
        Self {
            window_tokens,
            calibrated_input_tokens: None,
            calibrated_characters: 0,
        }
    }

    /// Returns the character count one request's whole input contributes.
    ///
    /// The count covers everything the request sends: the content of every
    /// message, the tool-call arguments of its assistant messages, and its
    /// transient reasoning attachments.
    #[must_use]
    pub fn request_characters(request: &ModelRequestDto) -> usize {
        message_characters(request.messages()) + reasoning_characters(request.assistant_reasoning())
    }

    /// Calibrates the estimate from one provider usage report.
    ///
    /// A reported usage replaces the whole estimate with the actual input size
    /// of the request that produced it, and every later increment counts at
    /// four characters per token. A not-reported usage leaves the estimate
    /// entirely character-based.
    pub const fn observe_usage(&mut self, usage: UsageDto, request_characters: usize) {
        match usage {
            UsageDto::Reported { input_tokens, .. } => {
                self.calibrated_input_tokens = Some(input_tokens);
                self.calibrated_characters = request_characters;
            }
            UsageDto::NotReported => {
                self.calibrated_input_tokens = None;
                self.calibrated_characters = 0;
            }
        }
    }

    /// Estimates the input size of one request in tokens.
    #[must_use]
    pub fn estimate_tokens(
        &self,
        messages: &[ModelMessageDto],
        reasoning: &[AssistantReasoningDto],
    ) -> u64 {
        let characters = message_characters(messages) + reasoning_characters(reasoning);
        self.calibrated_input_tokens.map_or_else(
            || estimated_tokens(characters),
            |calibrated| {
                calibrated.saturating_add(estimated_tokens(
                    characters.saturating_sub(self.calibrated_characters),
                ))
            },
        )
    }

    /// Applies one accounting pass to the message list about to be sent.
    ///
    /// The largest tool results are compressed first whenever the estimate
    /// crosses the sliding window, and the prompt-cache breakpoints are
    /// recomputed for the trimmed list. The reasoning attachments are part of
    /// the measured input; they are never compressed.
    ///
    /// # Errors
    ///
    /// Returns a validation error only when a compressed result cannot form a
    /// valid tool-role message, which the placeholder construction prevents.
    pub fn apply(
        &self,
        messages: &mut [ModelMessageDto],
        reasoning: &[AssistantReasoningDto],
    ) -> DtoResult<()> {
        self.trim_to_window(messages, reasoning)?;
        mark_cache_breakpoints(messages);
        Ok(())
    }

    /// Compresses the largest tool results until the estimate fits the window.
    fn trim_to_window(
        &self,
        messages: &mut [ModelMessageDto],
        reasoning: &[AssistantReasoningDto],
    ) -> DtoResult<()> {
        while self.estimate_tokens(messages, reasoning) > self.window_tokens {
            let Some(index) = largest_compressible_tool_result(messages) else {
                return Ok(());
            };
            let compressed = compressed_tool_result(messages[index].content());
            messages[index].replace_content(compressed)?;
        }
        Ok(())
    }
}

/// Estimates the token count of one character count at four characters per token.
fn estimated_tokens(characters: usize) -> u64 {
    u64::try_from(characters.div_ceil(ESTIMATED_CHARACTERS_PER_TOKEN)).unwrap_or(u64::MAX)
}

/// Returns the total character count of one message list.
///
/// Every message contributes its content plus, for an assistant tool-call
/// message, the arguments document it sends with each call: both are part of
/// the request's input.
fn message_characters(messages: &[ModelMessageDto]) -> usize {
    messages
        .iter()
        .map(|message| {
            message.content().chars().count()
                + message
                    .tool_calls()
                    .unwrap_or_default()
                    .iter()
                    .map(|call| call.arguments_json().chars().count())
                    .sum::<usize>()
        })
        .sum()
}

/// Returns the total character count of one reasoning-attachment list.
fn reasoning_characters(reasoning: &[AssistantReasoningDto]) -> usize {
    reasoning
        .iter()
        .map(|attachment| attachment.text().chars().count())
        .sum()
}

/// Returns the index of the longest tool result that still shrinks when compressed.
fn largest_compressible_tool_result(messages: &[ModelMessageDto]) -> Option<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role() == ModelRoleDto::Tool)
        .filter(|(_, message)| {
            compressed_tool_result(message.content()).chars().count()
                < message.content().chars().count()
        })
        .max_by_key(|(_, message)| message.content().chars().count())
        .map(|(index, _)| index)
}

/// Builds the short placeholder replacing one compressed tool result.
///
/// The placeholder keeps the first [`MIN_TOOL_RESULT_CHARACTERS`] characters of
/// the result text and appends a fixed marker, so it always stays non-empty and
/// at or above the floor while the model can still tell it apart from the
/// original output.
fn compressed_tool_result(content: &str) -> String {
    let retained: String = content
        .trim_start()
        .chars()
        .take(MIN_TOOL_RESULT_CHARACTERS)
        .collect();
    if retained.trim().is_empty() {
        return COMPRESSED_TOOL_RESULT_PLACEHOLDER.to_owned();
    }
    format!("{retained}{COMPRESSED_TOOL_RESULT_MARKER}")
}

/// Recomputes the prompt-cache breakpoints of one message list.
///
/// One breakpoint closes the leading system/instruction block, and one closes
/// the stable window prefix that the next provider request repeats: the last
/// message currently in the list. Every other message carries no marker, so a
/// trimming run never leaves a stale breakpoint behind.
fn mark_cache_breakpoints(messages: &mut [ModelMessageDto]) {
    for message in messages.iter_mut() {
        message.set_cache_control(false);
    }
    if let Some(index) = leading_system_block_end(messages) {
        messages[index].set_cache_control(true);
    }
    if let Some(last) = messages.last_mut() {
        last.set_cache_control(true);
    }
}

/// Returns the last index of the leading system/instruction block, if any.
fn leading_system_block_end(messages: &[ModelMessageDto]) -> Option<usize> {
    if messages
        .first()
        .is_none_or(|message| message.role() != ModelRoleDto::System)
    {
        return None;
    }
    messages
        .iter()
        .take_while(|message| message.role() == ModelRoleDto::System)
        .count()
        .checked_sub(1)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Unit fixtures use expect to provide precise test failure messages."
)]
mod tests {
    use super::*;
    use intention_proto::{RunId, ToolCallId};

    fn user(content: &str) -> ModelMessageDto {
        ModelMessageDto::new(ModelRoleDto::User, content).expect("user message is valid")
    }

    fn tool(content: &str) -> ModelMessageDto {
        ModelMessageDto::tool_result(ToolCallId::new(), content).expect("tool result is valid")
    }

    /// A generous window that never triggers a pass by itself.
    fn roomy_state() -> ContextWindowState {
        ContextWindowState::new(1_000_000)
    }

    #[test]
    fn the_largest_tool_result_is_compressed_first() {
        let smaller = "a".repeat(120);
        let larger = "b".repeat(240);
        let mut messages = vec![user("context"), tool(&smaller), tool(&larger)];
        let state = ContextWindowState::new(50);

        state.apply(&mut messages, &[]).expect("pass applies");

        assert_eq!(
            messages[1].content(),
            smaller,
            "the smaller result stays untouched while the window already fits"
        );
        assert!(
            messages[2].content().contains("[compressed]"),
            "the largest result is compressed first"
        );
        assert!(state.estimate_tokens(&messages, &[]) <= 50);
    }

    #[test]
    fn compression_walks_down_toward_the_sixteen_character_floor() {
        let first = "a".repeat(120);
        let second = "b".repeat(240);
        let mut messages = vec![user("context"), tool(&first), tool(&second)];
        let state = ContextWindowState::new(20);

        state.apply(&mut messages, &[]).expect("pass applies");

        for message in &messages[1..] {
            let content = message.content();
            assert!(
                content.chars().count() >= MIN_TOOL_RESULT_CHARACTERS,
                "a compressed result never falls below the sixteen-character floor"
            );
            assert!(content.contains("[compressed]"));
        }
        assert!(state.estimate_tokens(&messages, &[]) <= 20);
    }

    #[test]
    fn compressed_tool_results_keep_their_pairing_and_floor() {
        let id = ToolCallId::new();
        let call = intention_providers::ToolCallDto::new(id, "read", "{}").expect("call is valid");
        let mut messages = vec![
            user("context"),
            ModelMessageDto::assistant_tool_calls(None, vec![call]).expect("message is valid"),
            ModelMessageDto::tool_result(id, "x".repeat(400)).expect("tool result is valid"),
        ];
        let count = messages.len();
        let state = ContextWindowState::new(10);

        state.apply(&mut messages, &[]).expect("pass applies");

        assert_eq!(messages.len(), count, "no message is ever removed");
        assert_eq!(messages[1].role(), ModelRoleDto::Assistant);
        assert_eq!(
            messages[1]
                .tool_calls()
                .expect("tool calls are preserved")
                .first()
                .expect("one call is preserved")
                .call_id(),
            id
        );
        assert_eq!(messages[2].role(), ModelRoleDto::Tool);
        assert_eq!(messages[2].tool_call_id(), Some(id));
        let content = messages[2].content();
        assert!(content.chars().count() >= MIN_TOOL_RESULT_CHARACTERS);
        assert!(!content.trim().is_empty());
    }

    #[test]
    fn reported_usage_calibrates_the_estimate_and_later_increments_are_character_based() {
        let mut state = roomy_state();
        let mut messages = vec![user(&"a".repeat(40))];
        assert_eq!(state.estimate_tokens(&messages, &[]), 10);

        state.observe_usage(UsageDto::reported(100, 5, 105).expect("usage is valid"), 40);
        assert_eq!(
            state.estimate_tokens(&messages, &[]),
            100,
            "a reported usage replaces the whole estimate with the actual input size"
        );

        messages.push(tool(&"b".repeat(40)));
        assert_eq!(
            state.estimate_tokens(&messages, &[]),
            110,
            "the forty added characters are estimated at four characters per token"
        );

        state.observe_usage(UsageDto::NotReported, 80);
        assert_eq!(
            state.estimate_tokens(&messages, &[]),
            20,
            "a not-reported usage leaves the whole estimate character-based"
        );
    }

    #[test]
    fn cache_breakpoints_close_the_system_block_and_the_stable_prefix() {
        let mut messages = vec![
            ModelMessageDto::new(ModelRoleDto::System, "instructions")
                .expect("system message is valid"),
            user("first"),
            tool("result"),
        ];
        let state = roomy_state();

        state.apply(&mut messages, &[]).expect("pass applies");

        assert!(messages[0].cache_control(), "the system block is closed");
        assert!(!messages[1].cache_control());
        assert!(messages[2].cache_control(), "the stable prefix is closed");

        messages.push(user("second"));
        state.apply(&mut messages, &[]).expect("pass applies");

        assert!(messages[0].cache_control());
        assert!(
            !messages[2].cache_control(),
            "recomputing clears the previous stable-prefix breakpoint"
        );
        assert!(messages[3].cache_control());
    }

    #[test]
    fn request_characters_cover_tool_arguments_and_reasoning() {
        let id = ToolCallId::new();
        let arguments = r#"{"path":"a.rs"}"#;
        let call =
            intention_providers::ToolCallDto::new(id, "read", arguments).expect("call is valid");
        let request = ModelRequestDto::new(
            RunId::new(),
            "fixture",
            vec![
                user("context"),
                ModelMessageDto::assistant_tool_calls(None, vec![call])
                    .expect("assistant tool-call message is valid"),
                ModelMessageDto::tool_result(id, "x".repeat(100)).expect("tool result is valid"),
            ],
            None,
        )
        .expect("request is valid")
        .with_assistant_reasoning(vec![
            AssistantReasoningDto::new(vec![id], "why").expect("reasoning is valid"),
        ])
        .expect("reasoning attachment is valid");

        // The whole request input is measured: every message's content, the
        // arguments of its tool calls, and the transient reasoning attachments.
        assert_eq!(
            ContextWindowState::request_characters(&request),
            "context".len() + arguments.len() + 100 + "why".len()
        );
        let mut messages = request.messages().to_vec();
        let roomy = roomy_state();
        assert_eq!(
            roomy.estimate_tokens(&messages, request.assistant_reasoning()),
            roomy.estimate_tokens(&messages, &[]) + estimated_tokens("why".len()),
            "the reasoning attachments add to the estimated input"
        );

        // A window that fits the message content alone no longer fits the
        // request, so the pass compresses the tool result.
        let state = ContextWindowState::new(30);
        state
            .apply(&mut messages, request.assistant_reasoning())
            .expect("pass applies");
        assert!(
            messages[2].content().contains("[compressed]"),
            "tool-call arguments and reasoning attachments count toward the window"
        );
    }
}
