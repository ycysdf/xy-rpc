use bytes::{Bytes, BytesMut};
use core::pin::Pin;
use core::task::{Context, Poll};
use futures_util::stream::Stream;
use pin_project::pin_project;

const DEFAULT_CAPACITY: usize = 1024 * 8;

#[derive(Debug)]
#[pin_project]
pub struct ReadStream<R> {
    #[pin]
    reader: Option<R>,
    buf: BytesMut,
    capacity: usize,
}

impl<R> ReadStream<R> {
    pub fn new(reader: R) -> Self {
        ReadStream {
            reader: Some(reader),
            buf: BytesMut::new(),
            capacity: DEFAULT_CAPACITY,
        }
    }
    pub fn with_capacity(reader: R, capacity: usize) -> Self {
        ReadStream {
            reader: Some(reader),
            buf: BytesMut::with_capacity(capacity),
            capacity,
        }
    }
}

#[cfg(feature = "std")]
impl<R: futures_util::io::AsyncRead> Stream for ReadStream<R> {
    type Item = std::io::Result<Bytes>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let me = self.as_mut().project();

        let reader = match me.reader.as_pin_mut() {
            Some(r) => r,
            None => return Poll::Ready(None),
        };

        match {
            me.buf.resize(me.buf.capacity().max(DEFAULT_CAPACITY), 0);
            reader.poll_read(cx, me.buf.as_mut())
        } {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                self.project().reader.set(None);
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(Ok(0)) => {
                self.project().reader.set(None);
                Poll::Ready(None)
            }
            Poll::Ready(Ok(_)) => {
                let chunk = me.buf.split();
                Poll::Ready(Some(Ok(chunk.freeze())))
            }
        }
    }
}
