pub mod channel {
    use core::pin::Pin;

    use core::fmt::Debug;
    use core::task::Waker;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;

    #[derive(Debug, Default)]
    struct Shared<T>(Mutex<Channel<T>>);

    type Slab<T> = Option<T>;
    type Queue<T> = VecDeque<(Slab<T>, Waker)>;

    #[derive(Debug, Default)]
    struct Channel<T> {
        sending: Option<(usize, Queue<T>)>,
        waiting: Queue<T>,
        queue: VecDeque<Option<T>>,
    }

    impl<T> Channel<T> {
        fn from_cap(cap: usize) -> Self {
            Self {
                sending: Some((cap, VecDeque::with_capacity(cap))),
                waiting: VecDeque::with_capacity(cap),
                queue: VecDeque::with_capacity(cap),
            }
        }
    }

    #[derive(Debug, Default, Clone)]
    pub struct Sender<T>(Arc<Shared<T>>);
    #[derive(Debug, Default, Clone)]
    pub struct Receiver<T>(Arc<Shared<T>>);

    pub fn bounded<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
        let shared = Arc::new(Shared(Mutex::from(Channel::from_cap(cap))));
        (Sender(shared.clone()), Receiver(shared))
    }

    impl<T: Debug> Sender<T> {
        pub fn send(&mut self, mut msg: Option<T>, _cx: Context<'_>) -> Result<(), io::Error> {
            let mut_self = Pin::new(self).get_mut();

            let mut channel = mut_self.0 .0.lock().expect("Error getting mutex lock!");

            if let Some((None, waker)) = channel.waiting.pop_front() {
                channel.queue.push_back(msg.take());
                waker.wake_by_ref();
                Ok(())
            } else if channel
                .sending
                .as_ref()
                .map_or(true, |(cap, _)| channel.queue.len() < *cap)
            {
                channel.queue.push_back(msg.take());
                Ok(())
            } else if let Some((_, sending)) = channel.sending.as_mut() {
                /* sending should block */
                println!("sending should be blocked!");
                sending.push_back((msg.take(), _cx.waker().clone()));

                Err(io::Error::from(io::ErrorKind::WouldBlock))
            } else {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "unbounded channel overflowing",
                ))
            }
        }
    }

    impl<T: Debug> Receiver<T> {
        pub fn recv(&mut self, _cx: Context<'_>) -> Result<Option<T>, io::Error> {
            let mut_self = Pin::new(self).get_mut();

            let mut channel = mut_self.0 .0.lock().expect("Error getting mutex lock!");

            if let Some(slot) = channel.queue.pop_front() {
                //dbg!(channel);
                Ok(slot)
            } else if let Some(slot) = channel
                .sending
                .as_mut()
                .and_then(|(_, s)| s.pop_front().and_then(|s| s.0))
            {
                //dbg!(channel);
                Ok(Some(slot))
            } else {
                /* receive should be blocked */
                println!("receive should be blocked!");

                channel.waiting.push_back((None, _cx.waker().clone()));

                Err(io::Error::from(io::ErrorKind::WouldBlock))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let (tx, rx) = channel::bounded::<usize>(2);
    }
}
