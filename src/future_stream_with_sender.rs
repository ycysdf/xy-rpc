use crate::StreamWithFut;
use crate::maybe_send::MaybeSend;
use futures_util::Stream;

pub fn stream_with_sender<T: MaybeSend + 'static, F: Future + MaybeSend>(
    f: impl FnOnce(flume::Sender<T>) -> F + MaybeSend,
) -> impl Stream<Item = T> + MaybeSend {
    let (tx, rx) = flume::unbounded();
    StreamWithFut::new(rx.into_stream(), async move {
        f(tx).await;
        core::future::pending::<()>().await
    })
}
