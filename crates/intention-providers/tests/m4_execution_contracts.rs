use intention_providers::ModelCancellationSignal;

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
