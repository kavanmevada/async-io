#![forbid(unsafe_code)]

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use std::task::Waker;

mod channel;
mod select;

pub enum FutState<F: Future> {
    Handle(::std::thread::JoinHandle<F::Output>),
    Future(F),
}

pub struct FutThread<F: Future>(
    Option<::std::thread::JoinHandle<F::Output>>,
    ::std::sync::Arc<::std::sync::Mutex<Option<Waker>>>,
);
impl<F: Future> Unpin for FutThread<F> {}

pub fn spawn<F>(fut: F) -> FutThread<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let bridge = ::std::sync::Arc::new(::std::sync::Mutex::<Option<Waker>>::default());

    FutThread(
        Some(::std::thread::spawn({
            let bridge = ::std::sync::Arc::clone(&bridge);
            move || {
                log::info!(target: "[SPAWNED]", "future thread");
                let res = block_on(fut);
                log::info!(target: "[AWAKEN]", "spawner thread from future thread");
                bridge
                    .lock()
                    .map_or((), |lock| lock.as_ref().map_or((), |w| w.wake_by_ref()));
                drop(bridge);
                res
            }
        })),
        bridge,
    )
}

impl<F> Future for FutThread<F>
where
    F: Future + Send + 'static,
    F::Output: Send,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        log::info!(target: "[POLLED]", "spawned future");

        let Self(state, bridge) = core::pin::Pin::get_mut(self);

        match state.take() {
            Some(handle) if handle.is_finished() => {
                log::info!(target: "[STATE]", "future thread finished and future polled");
                handle.join().ok().map_or(Poll::Pending, |r| Poll::Ready(r))
            }

            Some(handle) => {
                log::info!(target: "[STATE]", "future thread unfinished polled");
                *bridge.lock().unwrap() = Some(_cx.waker().clone());
                state.replace(handle);

                log::info!(target: "[STATE]", "waker registered, going back to sleep");

                Poll::Pending
            }

            None => unreachable!(),
        }
    }
}

pub fn block_on<F: Future>(fut: F) -> F::Output {
    let thread = ::std::thread::current();

    let waker = waker_fn::waker_fn(move || thread.unpark());
    let mut cx = ::core::task::Context::from_waker(&waker);

    let mut pinned = Box::pin(fut);
    loop {
        log::info!(target: "[POLLED]", "main executor");

        let Poll::Ready(result) = pinned.as_mut().poll(&mut cx) else {
                ::std::thread::park(); continue
            };

        break result;
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn imperetive_futures_rs() {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<i32>();

        // Create a future by an async block, where async is responsible for generating
        // an implementation of Future. At this point no executor has been provided
        // to this future, so it will not be running.
        let fut_values = async {
            // Create another async block, again where Future is implemented by
            // async. Since this is inside of a parent async block, it will be
            // provided with the executor of the parent block when the parent
            // block is executed.
            //
            // This executor chaining is done by Future::poll whose second argument
            // is a std::task::Context. This represents our executor, and the Future
            // implemented by this async block can be polled using the parent async
            // block's executor.
            let fut_tx_result = async move {
                (0..100).for_each(|v| {
                    tx.unbounded_send(v).expect("Failed to send");
                })
            };

            // Use the provided thread pool to spawn the transmission
            super::spawn(fut_tx_result);

            let mut pending = vec![];
            // Use the provided executor to wait for the next value
            // of the stream to be available.

            use futures::StreamExt;
            while let Some(v) = rx.next().await {
                pending.push(v * 2);
            }

            pending
        };

        // Actually execute the above future, which will invoke Future::poll and
        // subsequently chain appropriate Future::poll and methods needing executors
        // to drive all futures. Eventually fut_values will be driven to completion.
        let values: Vec<i32> = super::block_on(fut_values);

        println!("Values={values:?}");
    }

    #[test]
    fn functional_futures_rs() {
        let (tx, rx) = futures::channel::mpsc::unbounded::<i32>();

        // Create a future by an async block, where async is responsible for an
        // implementation of Future. At this point no executor has been provided
        // to this future, so it will not be running.
        let fut_values = async {
            // Create another async block, again where the Future implementation
            // is generated by async. Since this is inside of a parent async block,
            // it will be provided with the executor of the parent block when the parent
            // block is executed.
            //
            // This executor chaining is done by Future::poll whose second argument
            // is a std::task::Context. This represents our executor, and the Future
            // implemented by this async block can be polled using the parent async
            // block's executor.
            let fut_tx_result = async move {
                (0..100).for_each(|v| {
                    tx.unbounded_send(v).expect("Failed to send");
                })
            };

            // Use the provided thread pool to spawn the generated future
            // responsible for transmission
            super::spawn(fut_tx_result);

            use futures::StreamExt;
            let fut_values = rx.map(|v| v * 2).collect();

            // Use the executor provided to this async block to wait for the
            // future to complete.
            fut_values.await
        };

        // Actually execute the above future, which will invoke Future::poll and
        // subsequently chain appropriate Future::poll and methods needing executors
        // to drive all futures. Eventually fut_values will be driven to completion.
        let values: Vec<i32> = super::block_on(fut_values);

        println!("Values={values:?}");
    }

    #[test]
    fn flume_channel() {
        super::block_on(async {
            let (tx, rx) = flume::bounded::<&str>(1);

            let t = super::spawn(async move {
                while let Ok(msg) = rx.recv_async().await {
                    println!("Received: {}", msg);
                }
            });

            tx.send_async("Hello, world!").await.unwrap();
            tx.send_async("How are you today?").await.unwrap();

            drop(tx);

            t.await;
        });
    }

    #[test]
    fn smol_executor() {
        smol::block_on(async {
            let (tx, rx) = flume::bounded::<&str>(1);

            let t = super::spawn(async move {
                while let Ok(msg) = rx.recv_async().await {
                    println!("Received: {}", msg);
                }
            });

            tx.send_async("Hello, world!").await.unwrap();
            tx.send_async("How are you today?").await.unwrap();

            drop(tx);

            t.await;
        })
    }
}
