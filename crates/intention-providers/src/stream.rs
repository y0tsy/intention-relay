//! The one normalized event stream every adapter drives.
//!
//! Each adapter owns only what depends on its SDK: request translation, the
//! translation of one native item into normalized events, its native error
//! classification, and its error codes. The stream machinery is
//! provider-neutral and therefore lives here once: the pending queue, the
//! `Started` seeding, the terminal flag, and the emission order. This is the
//! only place the order contract is enforced; a consumer reads an already
//! ordered stream and does not re-validate it.
//!
//! Interruption is not observed here: the adapter refuses an already-cancelled
//! request before it builds a stream (`ModelExecutionDriver::execute`), and
//! the consumer that owns the run owns mid-stream interruption by racing the
//! cancellation signal and dropping this stream. Dropping propagates the stop
//! into the SDK's in-flight response.

use std::collections::VecDeque;
use std::pin::Pin;

use futures_util::{Stream, StreamExt, stream};

use crate::mapping;
use crate::model::{FinishReasonDto, ModelEventDto, ModelEventStream, ProviderErrorDto};

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
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use futures_util::{StreamExt, stream};

    use super::{EventTranslator, NormalizedEvents, normalized_stream};
    use crate::model::{FinishReasonDto, ModelEventDto};

    /// A translator that writes past the terminal fact it already queued.
    struct Overrunning {
        /// How many native items the stream polled before it ended.
        polled: Arc<AtomicUsize>,
    }

    impl EventTranslator for Overrunning {
        type Item = u8;

        fn translate_item(&mut self, item: u8, events: &mut NormalizedEvents<'_>) {
            self.polled.fetch_add(1, Ordering::Relaxed);
            events.push(ModelEventDto::text_delta(format!("delta {item}")).expect("text is valid"));
            events.finish(FinishReasonDto::Stop);
            // The sink is total once terminal: neither a fact nor a second
            // terminal fact queues behind the finish.
            events.push(ModelEventDto::text_delta("after").expect("text is valid"));
            events.finish(FinishReasonDto::Length);
            events.fail("provider_stream_fixture_failure");
        }

        fn native_ended(&mut self, events: &mut NormalizedEvents<'_>) {
            events.fail("provider_stream_fixture_failure");
        }
    }

    #[test]
    fn the_normalized_stream_orders_its_facts_and_ends_at_the_first_terminal_one() {
        let polled = Arc::new(AtomicUsize::new(0));
        let events = futures_executor::block_on(
            normalized_stream(
                stream::iter([0_u8, 1]),
                Overrunning {
                    polled: Arc::clone(&polled),
                },
            )
            .collect::<Vec<_>>(),
        );
        assert_eq!(
            events,
            vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("delta 0").expect("text is valid")),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ],
            "the start fact is seeded, translation order is kept, and nothing queues behind the terminal fact"
        );
        assert_eq!(
            polled.load(Ordering::Relaxed),
            1,
            "the terminal fact ends the stream instead of polling the rest of the native stream"
        );
    }
}
