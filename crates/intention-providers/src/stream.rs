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
pub struct NormalizedEvents<'a> {
    pending: &'a mut VecDeque<Result<ModelEventDto, ProviderErrorDto>>,
    terminal: &'a mut bool,
}

impl NormalizedEvents<'_> {
    /// Queues one normalized fact.
    pub(crate) fn push(&mut self, event: ModelEventDto) {
        self.pending.push_back(Ok(event));
    }

    /// Queues the terminal `Finished` fact.
    pub(crate) fn finish(&mut self, reason: FinishReasonDto) {
        self.push(ModelEventDto::finished(reason));
        *self.terminal = true;
    }

    /// Queues one typed provider failure and terminalizes the stream.
    pub(crate) fn fail_error(&mut self, error: ProviderErrorDto) {
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
