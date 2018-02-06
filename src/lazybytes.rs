use futures::{Async, Poll};
use futures::stream::Stream;
use hyper;
use std::sync::{Arc, RwLock};
use bytes::Bytes;
use std::sync::TryLockError;

pub struct LazyBytes {
    pub bytes: Bytes,
    pub completion: bool,
}

pub struct LazyBytesStream {
    processed_bytes: usize,
    segment: Arc<RwLock<LazyBytes>>,
}

impl LazyBytesStream {
    pub fn new(segment: Arc<RwLock<LazyBytes>>) -> LazyBytesStream {
        LazyBytesStream {
            processed_bytes: 0,
            segment,
        }
    }
}

impl Stream for LazyBytesStream {
    type Item = hyper::Chunk;
    type Error = hyper::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let segment = match self.segment.try_read() {
            Ok(segment) => segment,
            Err(TryLockError::WouldBlock) => return Ok(Async::Ready(Some(hyper::Chunk::from("")))),
            Err(TryLockError::Poisoned(err)) => {
                panic!("Failed to try lock segment for read: {:?}", err)
            }
        };
        let bytes = &segment.bytes;
        if bytes.len() == self.processed_bytes {
            if segment.completion {
                Ok(Async::Ready(None))
            } else {
                // Ok(Async::NotReady)
                Ok(Async::Ready(Some(hyper::Chunk::from(""))))
            }
        } else if bytes.len() > self.processed_bytes {
            let ready = Async::Ready(Some(hyper::Chunk::from(
                bytes.slice(self.processed_bytes, bytes.len()),
            )));
            self.processed_bytes = bytes.len();
            Ok(ready)
        } else {
            panic!(
                "Logic error: segment length is lessor than processed length: {} < {}",
                bytes.len(),
                self.processed_bytes
            )
        }
    }
}
