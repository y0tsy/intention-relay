//! Dynamic context-window accounting, sliding compression, and prompt-cache
//! breakpoints.
//!
//! The executor owns one [`ContextWindowState`] per provider attempt. The state
//! estimates the input size of the message list it is about to send, compresses
//! the largest tool results when that estimate crosses the configured sliding
//! window, requests the (still unimplemented) full compression pass when it
//! crosses the model's capacity, and recomputes the prompt-cache breakpoints of
//! the request that follows.
//!
//! Messages are never removed and never reordered: a compression replaces one
//! message's content in place, so the assistant `tool_calls` message and its
//! tool replies stay paired.
//!
//! The estimate is calibrated from provider-reported usage whenever one
//! arrives; every uncalibrated increment is estimated at four characters per
//! token.

use intention_proto::DtoResult;
use intention_providers::{ModelMessageDto, ModelRequestDto, ModelRoleDto, UsageDto};

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
    capacity_tokens: u64,
    calibrated_input_tokens: Option<u64>,
    calibrated_characters: usize,
    /// Capacity-compression stub invocations, kept observable until the
    /// compression pass and a logging facade exist.
    compression_requests: u32,
}

impl ContextWindowState {
    /// Creates the accounting state for one attempt from its frozen configuration.
    pub const fn new(window_tokens: u64, capacity_tokens: u64) -> Self {
        Self {
            window_tokens,
            capacity_tokens,
            calibrated_input_tokens: None,
            calibrated_characters: 0,
            compression_requests: 0,
        }
    }

    /// Returns the character count one request's message list contributes.
    #[must_use]
    pub fn request_characters(request: &ModelRequestDto) -> usize {
        message_characters(request.messages())
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

    /// Estimates the input size of one message list in tokens.
    #[must_use]
    pub fn estimate_tokens(&self, messages: &[ModelMessageDto]) -> u64 {
        let characters = message_characters(messages);
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
    /// crosses the sliding window, an over-capacity estimate requests the
    /// still-unimplemented compression pass, and the prompt-cache breakpoints
    /// are recomputed for the trimmed list.
    ///
    /// # Errors
    ///
    /// Returns a validation error only when a compressed result cannot form a
    /// valid tool-role message, which the placeholder construction prevents.
    pub fn apply(&mut self, messages: &mut [ModelMessageDto]) -> DtoResult<()> {
        self.trim_to_window(messages)?;
        if self.estimate_tokens(messages) > self.capacity_tokens {
            self.compression_requests = self.compression_requests.saturating_add(1);
            compress_context(messages);
        }
        mark_cache_breakpoints(messages);
        Ok(())
    }

    /// Compresses the largest tool results until the estimate fits the window.
    fn trim_to_window(&self, messages: &mut [ModelMessageDto]) -> DtoResult<()> {
        while self.estimate_tokens(messages) > self.window_tokens {
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
fn message_characters(messages: &[ModelMessageDto]) -> usize {
    messages
        .iter()
        .map(|message| message.content().chars().count())
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

/// Requests full context compression for an over-capacity estimate.
///
/// The compression pass is not implemented: this stub never changes the
/// message list and exists so the capacity decision has a named owner. The
/// workspace carries no logging facade, so the warning that belongs here is
/// deferred with the implementation.
const fn compress_context(_messages: &mut [ModelMessageDto]) {
    // TODO(compression): implement context compression
    // TODO(observability): warn once the workspace exposes a logging facade
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Unit fixtures use expect to provide precise test failure messages."
)]
mod tests {
    use super::*;
    use intention_proto::ToolCallId;

    fn user(content: &str) -> ModelMessageDto {
        ModelMessageDto::new(ModelRoleDto::User, content).expect("user message is valid")
    }

    fn tool(content: &str) -> ModelMessageDto {
        ModelMessageDto::tool_result(ToolCallId::new(), content).expect("tool result is valid")
    }

    /// A generous window and capacity that never trigger a pass by themselves.
    fn roomy_state() -> ContextWindowState {
        ContextWindowState::new(1_000_000, 2_000_000)
    }

    #[test]
    fn the_largest_tool_result_is_compressed_first() {
        let smaller = "a".repeat(120);
        let larger = "b".repeat(240);
        let mut messages = vec![user("context"), tool(&smaller), tool(&larger)];
        let mut state = ContextWindowState::new(50, 1_000_000);

        state.apply(&mut messages).expect("pass applies");

        assert_eq!(
            messages[1].content(),
            smaller,
            "the smaller result stays untouched while the window already fits"
        );
        assert!(
            messages[2].content().contains("[compressed]"),
            "the largest result is compressed first"
        );
        assert!(state.estimate_tokens(&messages) <= 50);
    }

    #[test]
    fn compression_walks_down_toward_the_sixteen_character_floor() {
        let first = "a".repeat(120);
        let second = "b".repeat(240);
        let mut messages = vec![user("context"), tool(&first), tool(&second)];
        let mut state = ContextWindowState::new(20, 1_000_000);

        state.apply(&mut messages).expect("pass applies");

        for message in &messages[1..] {
            let content = message.content();
            assert!(
                content.chars().count() >= MIN_TOOL_RESULT_CHARACTERS,
                "a compressed result never falls below the sixteen-character floor"
            );
            assert!(content.contains("[compressed]"));
        }
        assert!(state.estimate_tokens(&messages) <= 20);
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
        let mut state = ContextWindowState::new(10, 1_000_000);

        state.apply(&mut messages).expect("pass applies");

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
        assert_eq!(state.estimate_tokens(&messages), 10);

        state.observe_usage(UsageDto::reported(100, 5, 105).expect("usage is valid"), 40);
        assert_eq!(
            state.estimate_tokens(&messages),
            100,
            "a reported usage replaces the whole estimate with the actual input size"
        );

        messages.push(tool(&"b".repeat(40)));
        assert_eq!(
            state.estimate_tokens(&messages),
            110,
            "the forty added characters are estimated at four characters per token"
        );

        state.observe_usage(UsageDto::NotReported, 80);
        assert_eq!(
            state.estimate_tokens(&messages),
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
        let mut state = roomy_state();

        state.apply(&mut messages).expect("pass applies");

        assert!(messages[0].cache_control(), "the system block is closed");
        assert!(!messages[1].cache_control());
        assert!(messages[2].cache_control(), "the stable prefix is closed");

        messages.push(user("second"));
        state.apply(&mut messages).expect("pass applies");

        assert!(messages[0].cache_control());
        assert!(
            !messages[2].cache_control(),
            "recomputing clears the previous stable-prefix breakpoint"
        );
        assert!(messages[3].cache_control());
    }

    #[test]
    fn an_over_capacity_estimate_requests_the_compression_stub() {
        let mut messages = vec![user(&"a".repeat(400))];
        let unchanged = messages.clone();
        let mut state = ContextWindowState::new(10, 20);

        state.apply(&mut messages).expect("pass applies");

        assert_eq!(state.compression_requests, 1);
        assert_eq!(messages.len(), unchanged.len());
        for (message, original) in messages.iter().zip(unchanged.iter()) {
            assert_eq!(message.role(), original.role());
            assert_eq!(
                message.content(),
                original.content(),
                "the compression stub never changes message content"
            );
        }
        assert!(state.estimate_tokens(&messages) > 20);
    }
}
