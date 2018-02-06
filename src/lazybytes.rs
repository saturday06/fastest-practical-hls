use futures::{Async, Poll};
use futures::stream::Stream;
use hyper;
use std::sync::{Arc, RwLock};
use bytes::Bytes;

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
            segment: segment,
        }
    }
}

impl From<Bytes> for LazyBytesStream {
    fn from(bytes: Bytes) -> Self {
        LazyBytesStream {
            processed_bytes: 0,
            segment: Arc::new(RwLock::new(LazyBytes {
                bytes: bytes,
                completion: true,
            })),
        }
    }
}

impl From<String> for LazyBytesStream {
    fn from(string: String) -> Self {
        LazyBytesStream {
            processed_bytes: 0,
            segment: Arc::new(RwLock::new(LazyBytes {
                bytes: Bytes::from(string),
                completion: true,
            })),
        }
    }
}

impl From<Vec<u8>> for LazyBytesStream {
    fn from(vec: Vec<u8>) -> Self {
        LazyBytesStream {
            processed_bytes: 0,
            segment: Arc::new(RwLock::new(LazyBytes {
                bytes: Bytes::from(vec),
                completion: true,
            })),
        }
    }
}

impl Stream for LazyBytesStream {
    type Item = hyper::Chunk;
    type Error = hyper::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let segment = self.segment.read().expect("Failed to lock segment bytes");
        let bytes = &segment.bytes;
        if bytes.len() == self.processed_bytes {
            if segment.completion {
                Ok(Async::Ready(None))
            } else {
                Ok(Async::NotReady)
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
