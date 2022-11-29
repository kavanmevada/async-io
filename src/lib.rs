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

pub struct FutThread<F: Future>(Option<::std::thread::JoinHandle<F::Output>>);

pub fn spawn<F: Future>(fut: F) -> FutThread<F>
where
    F: Send + 'static,
    F::Output: Send + 'static,
{
    let t = ::std::thread::current();
    FutThread(Some(::std::thread::spawn(move || {
        println!("inspection: thread future spawned");
        let res = block_on(fut);
        println!("inspection: unpark main thread");
        t.unpark();
        res
    })))
}

impl<F> ::core::future::Future for FutThread<F>
where
    F: Future<Output = ()> + Send + 'static,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let pinned = ::core::pin::pin!(&mut self);

        if pinned.0.as_ref().map_or(false, |s| s.is_finished()) {
            pinned
                .get_mut()
                .0
                .take()
                .and_then(|inner| inner.join().ok())
                .inspect(|s| println!("inspection: {:?}", s))
                .map_or(Poll::Pending, |res| Poll::Ready(res))
        } else {
            println!("warnning: future is polled before finished");
            Poll::Pending
        }
    }
}

pub fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = waker_fn::waker_fn(|| ::std::thread::current().unpark());
    let mut cx = ::core::task::Context::from_waker(&waker);

    let mut pinned = ::core::pin::pin!(fut);
    loop {
        let Poll::Ready(result) = pinned.as_mut().poll(&mut cx) else {
                ::std::thread::park(); continue
            };

        break result;
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn it_works() {
        let res = super::block_on(async {
            super::spawn(async {
                println!("msg from spawned thread #2 !");
                ::core::future::poll_fn(|_| ::core::task::Poll::<()>::Ready(())).await
            })
            .await
        });

        assert_eq!(res, ());
    }
}
