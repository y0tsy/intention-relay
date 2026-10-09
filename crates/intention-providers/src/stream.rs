//! The one normalized event stream every adapter drives.
//!
//! Each adapter owns only what depends on its SDK: request translation, the
//! translation of one native item into normalized events, its native error
//! classification, and its error codes. The stream machinery is
//! provider-neutral and therefore lives here once: the pending queue, the
//! `Started` seeding, the terminal flag, and the emission order.
//!
//! Interruption is not observed here: the adapter refuses an already-cancelled
//! request before it builds a stream (`ModelExecutionDriver::execute`), and
//! the consumer that owns the run owns mid-stream interruption by racing the
//! cancellation signal and dropping this stream. Dropping propagates the stop
//! into the SDK's in-flight response.

use std::collections::VecDeque;
use std::pin::Pin;

use futures_util::{Stream, StreamExt, stream};
use intention_proto::DtoResult;

use crate::mapping;
use crate::model::{
    FinishReasonDto, MAX_MODEL_REASONING_FRAGMENT_BYTES, ModelEventDto, ModelEventStream,
    PROVIDER_REASONING_FRAGMENT_TOO_LARGE, PROVIDER_REASONING_STREAM_INVALID, ProviderErrorDto,
};

/// Translates one adapter's native stream into normalized model events.
pub trait EventTranslator {
    /// The adapter's native stream item.
    type Item;

    /// Translates one native item, emitting zero or more normalized facts.
    fn translate_item(&mut self, item: Self::Item, events: &mut NormalizedEvents<'_>);

    /// Handles the native end of stream: the adapter decides whether the
    /// response completed or ended without its terminal fact.
    fn native_ended(&mut self, events: &mut NormalizedEvents<'_>);
}

/// The one queue every normalized fact passes through.
///
/// A translator writes through this sink instead of owning a queue, so the
/// shared stream keeps the order contract: `Started` is seeded before the
/// first native poll, facts stay in translation order, and a terminal fact
/// (`Finished` or a provider failure) is what ends the stream.
///
/// The sink is total once terminal: every later write is dropped, so no fact
/// and no second terminal failure can be queued behind the terminal one, and a
/// translator may keep translating the rest of an item without re-checking.
pub struct NormalizedEvents<'a> {
    pending: &'a mut VecDeque<Result<ModelEventDto, ProviderErrorDto>>,
    terminal: &'a mut bool,
}

impl NormalizedEvents<'_> {
    /// Queues one normalized reasoning fact.
    ///
    /// Reasoning normalization is the one place the closed reasoning failures
    /// are produced: a value that cannot be constructed (a malformed native
    /// value) or that exceeds the per-fragment representation bound fails the
    /// stream instead of publishing raw, partial, or oversized reasoning. The
    /// per-fragment bound is enforced here, at the normalization boundary, and
    /// is never satisfied by truncation.
    pub(crate) fn push_reasoning(&mut self, event: DtoResult<ModelEventDto>) {
        match event {
            Ok(event) if reasoning_exceeds_fragment_bound(&event) => {
                self.fail(PROVIDER_REASONING_FRAGMENT_TOO_LARGE);
            }
            Ok(event) => self.push(event),
            Err(error) if error.code() == PROVIDER_REASONING_FRAGMENT_TOO_LARGE => {
                self.fail(PROVIDER_REASONING_FRAGMENT_TOO_LARGE);
            }
            Err(_) => self.fail(PROVIDER_REASONING_STREAM_INVALID),
        }
    }

    /// Queues one normalized fact, unless a terminal fact already ended the stream.
    pub(crate) fn push(&mut self, event: ModelEventDto) {
        if *self.terminal {
            return;
        }
        self.pending.push_back(Ok(event));
    }

    /// Queues the terminal `Finished` fact.
    pub(crate) fn finish(&mut self, reason: FinishReasonDto) {
        self.push(ModelEventDto::finished(reason));
        *self.terminal = true;
    }

    /// Queues one typed provider failure and terminalizes the stream.
    pub(crate) fn fail_error(&mut self, error: ProviderErrorDto) {
        if *self.terminal {
            return;
        }
        self.pending.push_back(Err(error));
        *self.terminal = true;
    }

    /// Queues one fixed non-retryable failure code and terminalizes the stream.
    pub(crate) fn fail(&mut self, code: &'static str) {
        self.fail_error(mapping::fixed_error(code));
    }
}

/// Whether one normalized reasoning fact exceeds the per-fragment bound.
const fn reasoning_exceeds_fragment_bound(event: &ModelEventDto) -> bool {
    match event {
        ModelEventDto::ReasoningDelta { content, .. }
        | ModelEventDto::ReasoningSummaryDelta { content } => {
            content.len() > MAX_MODEL_REASONING_FRAGMENT_BYTES
        }
        _ => false,
    }
}

/// Builds the normalized event stream over an adapter's native item stream.
pub fn normalized_stream<S, T>(native: S, translator: T) -> ModelEventStream
where
    S: Stream<Item = T::Item> + Send + 'static,
    T: EventTranslator + Send + 'static,
{
    Box::pin(stream::unfold(
        NormalizedStream::new(native, translator),
        |mut state| async move { state.next().await.map(|event| (event, state)) },
    ))
}

struct NormalizedStream<S, T> {
    native: Pin<Box<S>>,
    translator: T,
    pending: VecDeque<Result<ModelEventDto, ProviderErrorDto>>,
    terminal: bool,
}

impl<S, T> NormalizedStream<S, T>
where
    S: Stream<Item = T::Item>,
    T: EventTranslator,
{
    fn new(native: S, translator: T) -> Self {
        let mut pending = VecDeque::new();
        pending.push_back(Ok(ModelEventDto::started()));
        Self {
            native: Box::pin(native),
            translator,
            pending,
            terminal: false,
        }
    }

    async fn next(&mut self) -> Option<Result<ModelEventDto, ProviderErrorDto>> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            if self.terminal {
                return None;
            }
            let item = self.native.next().await;
            let Self {
                translator,
                pending,
                terminal,
                ..
            } = self;
            match item {
                Some(item) => {
                    translator.translate_item(item, &mut NormalizedEvents { pending, terminal });
                }
                None => {
                    translator.native_ended(&mut NormalizedEvents { pending, terminal });
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "Normalization fixtures use expect to name impossible local failures."
)]
mod tests {
    use super::*;
    use crate::model::ReasoningFragmentCategoryDto;

    /// A translator that normalizes each native string as one primary fragment.
    struct PrimaryFragmentTranslator;

    impl EventTranslator for PrimaryFragmentTranslator {
        type Item = String;

        fn translate_item(&mut self, item: Self::Item, events: &mut NormalizedEvents<'_>) {
            events.push_reasoning(ModelEventDto::reasoning_delta(
                ReasoningFragmentCategoryDto::Primary,
                item,
            ));
        }

        fn native_ended(&mut self, events: &mut NormalizedEvents<'_>) {
            events.finish(FinishReasonDto::Stop);
        }
    }

    /// A translator that forwards one malformed native reasoning value.
    struct MalformedReasoningTranslator;

    impl EventTranslator for MalformedReasoningTranslator {
        type Item = ();

        fn translate_item(&mut self, (): Self::Item, events: &mut NormalizedEvents<'_>) {
            events.push_reasoning(ModelEventDto::reasoning_delta(
                ReasoningFragmentCategoryDto::Primary,
                "",
            ));
        }

        fn native_ended(&mut self, events: &mut NormalizedEvents<'_>) {
            events.finish(FinishReasonDto::Stop);
        }
    }

    fn collect(
        native: Vec<String>,
        translator: PrimaryFragmentTranslator,
    ) -> Vec<Result<ModelEventDto, ProviderErrorDto>> {
        futures_executor::block_on(
            normalized_stream(stream::iter(native), translator).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn reasoning_fragments_normalize_with_the_per_fragment_bound_never_truncating() {
        let events = collect(
            vec!["first".to_owned(), "second".to_owned()],
            PrimaryFragmentTranslator,
        );
        assert_eq!(
            events,
            vec![
                Ok(ModelEventDto::started()),
                Ok(
                    ModelEventDto::reasoning_delta(ReasoningFragmentCategoryDto::Primary, "first")
                        .expect("fixture fragment is valid")
                ),
                Ok(
                    ModelEventDto::reasoning_delta(ReasoningFragmentCategoryDto::Primary, "second")
                        .expect("fixture fragment is valid")
                ),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ]
        );

        let at_bound = "a".repeat(MAX_MODEL_REASONING_FRAGMENT_BYTES);
        let bounded = collect(vec![at_bound.clone()], PrimaryFragmentTranslator);
        assert!(
            bounded.iter().any(|event| matches!(
                event,
                Ok(ModelEventDto::ReasoningDelta { content, .. }) if content.len() == at_bound.len()
            )),
            "a fragment at the bound stays whole"
        );

        let oversized = "a".repeat(MAX_MODEL_REASONING_FRAGMENT_BYTES + 1);
        let rejected = collect(vec![oversized], PrimaryFragmentTranslator);
        assert!(matches!(
            rejected.last(),
            Some(Err(error)) if error.code() == PROVIDER_REASONING_FRAGMENT_TOO_LARGE
        ));
        assert!(
            !rejected.iter().any(|event| matches!(
                event,
                Ok(ModelEventDto::ReasoningDelta { .. })
                    | Ok(ModelEventDto::ReasoningSummaryDelta { .. })
            )),
            "an over-bound fragment is rejected whole, never truncated"
        );
    }

    #[test]
    fn malformed_reasoning_values_fail_with_the_closed_stream_failure() {
        let events = futures_executor::block_on(
            normalized_stream(stream::iter(vec![()]), MalformedReasoningTranslator)
                .collect::<Vec<_>>(),
        );
        assert_eq!(events.first(), Some(&Ok(ModelEventDto::started())));
        assert!(matches!(
            events.last(),
            Some(Err(error)) if error.code() == PROVIDER_REASONING_STREAM_INVALID
        ));
    }
}
