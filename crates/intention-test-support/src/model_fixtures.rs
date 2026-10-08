//! Deterministic model-driver fixtures shared by integration suites.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use futures_util::StreamExt;
use intention_providers::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelRequestDto, ProviderErrorDto,
};

/// Blocks the calling thread on one ready test future.
///
/// The engine suites share this helper instead of depending on a block-on
/// executor directly.
pub fn run_ready<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

/// One scripted provider round: the ordered events one execution yields.
pub type ScriptedRound = Vec<Result<ModelEventDto, ProviderErrorDto>>;

/// Deterministic model driver yielding one scripted round per execution.
///
/// The driver records every request and execution count, reports the same
/// full capability set every suite asserts, and offers explicit injection
/// points for a never-ending stream and an in-stream cancellation at a
/// selected event index of the first round.
pub struct ScriptedDriver {
    rounds: Mutex<VecDeque<ScriptedRound>>,
    executions: Mutex<usize>,
    requests: Mutex<Vec<ModelRequestDto>>,
    cancel_during_stream: Mutex<Option<(usize, ModelCancellationSignal)>>,
    pending_stream: Mutex<bool>,
}

impl ScriptedDriver {
    /// Creates a driver that yields one round of the supplied events.
    #[must_use]
    pub fn new(events: ScriptedRound) -> Self {
        Self::with_rounds(vec![events])
    }

    /// Creates a driver that yields one supplied round per provider execution.
    #[must_use]
    pub fn with_rounds(rounds: Vec<ScriptedRound>) -> Self {
        Self {
            rounds: Mutex::new(rounds.into()),
            executions: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
            cancel_during_stream: Mutex::new(None),
            pending_stream: Mutex::new(false),
        }
    }

    /// Returns a driver that streams exactly one completed text response.
    #[must_use]
    pub fn completed_text() -> Self {
        Self::new(vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("complete response")
                .unwrap_or_else(|_| unreachable!("fixture text is valid"))),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ])
    }

    /// Makes every execution stream stay pending without yielding an event.
    pub fn stay_pending(&self) {
        *self
            .pending_stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
    }

    /// Cancels the supplied signal after the selected event index of the first
    /// provider round.
    pub fn cancel_during_stream(&self, index: usize, signal: ModelCancellationSignal) {
        *self
            .cancel_during_stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((index, signal));
    }

    /// Returns how many provider executions started.
    #[must_use]
    pub fn executions(&self) -> usize {
        *self
            .executions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns every executed request in provider order.
    #[must_use]
    pub fn requests(&self) -> Vec<ModelRequestDto> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl ModelExecutionDriver for ScriptedDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }

    fn execute(
        &self,
        request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        let execution = {
            let mut executions = self
                .executions
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *executions += 1;
            *executions
        };
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request);
        if *self
            .pending_stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            return Box::pin(futures_util::stream::pending::<
                Result<ModelEventDto, ProviderErrorDto>,
            >());
        }
        let events = self
            .rounds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| unreachable!("a scripted provider round exists"));
        if execution == 1
            && let Some((index, signal)) = self
                .cancel_during_stream
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        {
            let mut seen = 0usize;
            return Box::pin(futures_util::stream::iter(events).inspect(move |_| {
                if seen == index {
                    signal.cancel();
                }
                seen += 1;
            }));
        }
        Box::pin(futures_util::stream::iter(events))
    }
}
