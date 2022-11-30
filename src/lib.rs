#![feature(pin_macro)]
#![feature(result_option_inspect)]
#![feature(type_name_of_val)]

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

mod channel;
mod select;

pub struct FutThread<F: Future>(Option<::std::thread::JoinHandle<F::Output>>, Option<F>);
impl<F: Future> Unpin for FutThread<F> {}

pub fn spawn<F>(fut: F) -> FutThread<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let thread = std::thread::current();
    let waker = waker_fn::waker_fn(move || thread.unpark());

    if let Some(w) = Some(waker) {
        FutThread(
            Some(::std::thread::spawn(move || {
                let res = block_on(fut);
                w.wake_by_ref();
                res
            })),
            None,
        )
    } else {
        FutThread(None, Some(fut))
    }
}

impl<F> Future for FutThread<F>
where
    F: Future + Send + 'static,
    F::Output: Send,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("check point reached");

        let Self(handle, fut) = core::pin::Pin::get_mut(self);

        match (handle.take(), fut.take()) {
            (None, Some(fut)) => {
                let waker_arc = _cx.waker().clone();
                handle.replace(::std::thread::spawn(move || {
                    let res = block_on(fut);
                    waker_arc.wake_by_ref();
                    res
                }));

                Poll::Pending
            }

            (Some(handle), None) => {
                println!("Here we join!");

                handle
                    .join()
                    .ok()
                    .map_or(Poll::Pending, |res| Poll::Ready(res))
            }
            _ => unreachable!(),
        }
    }
}

pub fn block_on<F: Future>(fut: F) -> F::Output {
    let thread = ::std::thread::current();

    let waker = waker_fn::waker_fn(move || thread.unpark());
    let mut cx = ::core::task::Context::from_waker(&waker);

    let mut pinned = ::core::pin::pin!(fut);
    loop {
        println!("main: polling master thread");
        let Poll::Ready(result) = pinned.as_mut().poll(&mut cx) else {
                ::std::thread::park(); continue
            };

        break result;
    }
}

#[cfg(test)]
mod tests {

    // #[test]
    // fn it_works() {
    //     let res = super::block_on(async {
    //         super::spawn(async {
    //             println!("msg from spawned thread #2 !");
    //             // ::core::future::poll_fn(|_| ::core::task::Poll::<()>::Pending).await
    //         })
    //         .await
    //     });

    //     dbg!(&res);
    // }

    #[test]
    fn functional() {
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
}
