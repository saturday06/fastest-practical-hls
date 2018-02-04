use futures::{Async, Poll};
use futures::stream::Stream;
use std::io;

pub struct RealtimeOutput {
    processed_bytes: usize
}

impl RealtimeOutput {
    pub fn new() {
        RealtimeOutput {
            processed_bytes: 0,
        }
    }
}

impl Stream for RealtimeOutput {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        Ok(Async::NotReady)
    }
}
