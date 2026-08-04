#![allow(
    clippy::expect_used,
    reason = "Execution contract fixtures use expect to provide precise test failure messages."
)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures_util::{StreamExt, stream};
use intention_model::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto,
};
use intention_types::RunId;

fn request() -> ModelRequestDto {
    ModelRequestDto::new(
        RunId::new(),
        "fixture-model",
        vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
        None,
        None,
    )
    .expect("request is valid")
}

#[test]
fn cancellation_signal_notifies_each_fresh_waiter_and_remains_cancelled() {
    let signal = ModelCancellationSignal::new();
    let first = signal.cancelled();
    let second = signal.cancelled();
    assert!(!signal.is_cancelled());

    signal.cancel();
    futures_executor::block_on(async {
        first.await;
        second.await;
        signal.cancelled().await;
    });
    assert!(signal.is_cancelled());
}

#[test]
fn cancellation_signal_deregisters_dropped_and_repolled_waiters() {
    use futures_util::{FutureExt, future::poll_fn};

    let signal = ModelCancellationSignal::new();
    let mut pending = Box::pin(signal.cancelled());
    futures_executor::block_on(poll_fn(|context| {
        assert!(pending.as_mut().poll_unpin(context).is_pending());
        assert!(pending.as_mut().poll_unpin(context).is_pending());
        std::task::Poll::Ready(())
    }));
    drop(pending);

    let mut live = Box::pin(signal.cancelled());
    futures_executor::block_on(poll_fn(|context| {
        assert!(live.as_mut().poll_unpin(context).is_pending());
        std::task::Poll::Ready(())
    }));
    signal.cancel();
    futures_executor::block_on(live);
}

struct FixtureExecutionDriver {
    drops: Arc<AtomicUsize>,
}

impl ModelDriver for FixtureExecutionDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, false, false, false, false, true)
    }
}

impl ModelExecutionDriver for FixtureExecutionDriver {
    fn execute(
        &self,
        _request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        let guard = DropGuard(self.drops.clone());
        Box::pin(
            stream::iter(vec![
                Ok(ModelEventDto::started()),
                Ok(ModelEventDto::text_delta("ordered").expect("text is valid")),
                Ok(ModelEventDto::usage(
                    intention_model::UsageDto::reported(1, 2, 3).expect("usage is valid"),
                )),
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ])
            .chain(stream::once(async move {
                drop(guard);
                Err(
                    ProviderErrorDto::unavailable("fixture_after_finish", false, None)
                        .expect("error is valid"),
                )
            })),
        )
    }
}

struct DropGuard(Arc<AtomicUsize>);

impl Drop for DropGuard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn execution_driver_yields_ordered_normalized_events_and_drops_resources() {
    let drops = Arc::new(AtomicUsize::new(0));
    let driver = FixtureExecutionDriver {
        drops: drops.clone(),
    };
    let events = futures_executor::block_on(
        driver
            .execute(request(), ModelCancellationSignal::new())
            .collect::<Vec<_>>(),
    );
    assert_eq!(events.len(), 5);
    assert_eq!(events[0], Ok(ModelEventDto::started()));
    assert_eq!(
        events[3],
        Ok(ModelEventDto::finished(FinishReasonDto::Stop))
    );
    assert_eq!(
        events[4]
            .as_ref()
            .expect_err("fixture error is ordered last")
            .code(),
        "fixture_after_finish"
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
