use crate::channel::InnerMsg;
use crate::formats::SerdeFormat;
use crate::maybe_send::MaybeSend;
use crate::{BUF, EmptyStream, RpcError, RpcOpId};
use bytes::{BufMut, Bytes};
use flume::Sender;
use futures_util::{FutureExt, Stream, StreamExt};
use pin_project::pin_project;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use try_specialize::TrySpecialize;

pub trait HandleRpc<
    SF: SerdeFormat,
    MsgItem: Serialize + 'static,
    Reply: DeserializeOwned + MaybeSend + 'static,
    MsgStream: Stream<Item = Result<MsgItem, RpcError>> + MaybeSend + 'static,
>: MaybeSend + 'static
{
    type Out: MaybeSend + 'static;
    fn handle(
        self,
        msg_sender: flume::Sender<InnerMsg>,
        serde_format: SF,
        op_id: RpcOpId,
        msg: Bytes,
        stream: Option<MsgStream>,
    ) -> impl Future<Output = Result<Self::Out, RpcError>> + MaybeSend + 'static;
}

pub struct Unary;

impl<
    SF: SerdeFormat,
    MsgItem: Serialize + 'static,
    Reply: DeserializeOwned + MaybeSend + 'static,
    MsgStream: Stream<Item = Result<MsgItem, RpcError>> + MaybeSend + 'static,
> HandleRpc<SF, MsgItem, Reply, MsgStream> for Unary
where
    Reply: DeserializeOwned + MaybeSend + 'static,
{
    type Out = Reply;

    fn handle(
        self,
        msg_sender: Sender<InnerMsg>,
        serde_format: SF,
        op_id: RpcOpId,
        msg: Bytes,
        _msg_stream: Option<MsgStream>,
    ) -> impl Future<Output = Result<Self::Out, RpcError>> + MaybeSend + 'static {
        assert!(_msg_stream.is_none());
        let (reply_sender, reply_receiver) = futures_channel::oneshot::channel();
        let result = rpc_call(
            &msg_sender,
            op_id,
            msg,
            false,
            OneOrMultiSender::Unary(reply_sender),
        );

        async move {
            result?;
            let mut cancel_when_drop = OnDrop::new(move || {
                let _ = msg_sender.send(InnerMsg::Cancel(op_id));
            });

            let msg = reply_receiver
                .await
                .map_err(|_err| RpcError::RecvCallReplyCancelled)?;
            cancel_when_drop.forget_drop();
            let reply = serde_format
                .deserialize_from_slice_optimized::<Reply>(&msg)
                .map_err(RpcError::SerdeError)?;
            drop(cancel_when_drop);
            Ok(reply)
        }
    }
}

pub struct MsgStreaming;

impl<
    SF: SerdeFormat,
    MsgItem: Serialize + MaybeSend + 'static,
    Reply: DeserializeOwned + MaybeSend + 'static,
    MsgStream: Stream<Item = Result<MsgItem, RpcError>> + MaybeSend + 'static,
> HandleRpc<SF, MsgItem, Reply, MsgStream> for MsgStreaming
{
    type Out = Reply;

    fn handle(
        self,
        msg_sender: Sender<InnerMsg>,
        serde_format: SF,
        op_id: RpcOpId,
        start_msg: Bytes,
        stream: Option<MsgStream>,
    ) -> impl Future<Output = Result<Self::Out, RpcError>> + MaybeSend + 'static {
        let (reply_sender, reply_receiver) = futures_channel::oneshot::channel();
        let result = rpc_call(
            &msg_sender,
            op_id,
            start_msg,
            true,
            OneOrMultiSender::Unary(reply_sender),
        );

        let send_all = stream
            .map({
                let msg_sender = msg_sender.clone();
                let serde_format = serde_format.clone();
                move |stream| SendRpcMsgAll {
                    stream,
                    cancel_on_drop: SendCancelOnDrop {
                        msg_sender: msg_sender.clone(),
                        op_id,
                    },
                    msg_sender,
                    serde_format,
                    is_end: false,
                    op_id,
                    _marker: Default::default(),
                }
            })
            .map(|n| n.left_future())
            .unwrap_or(std::future::pending::<Result<Reply, RpcError>>().right_future());

        async move {
            futures_util::select! {
                r = async move {
                    let mut cancel_when_drop = OnDrop::new(move || {
                        let _ = msg_sender.send(InnerMsg::Cancel(op_id));
                    });

                    let msg = reply_receiver
                        .await
                        .map_err(|_err| RpcError::RecvCallReplyCancelled)?;
                    cancel_when_drop.forget_drop();
                    let reply = serde_format
                        .deserialize_from_slice_optimized::<Reply>(&msg)
                        .map_err(RpcError::SerdeError)?;
                    drop(cancel_when_drop);
                    Ok(reply)
                }.fuse() => {r},
                _ = send_all.fuse() => {
                    unreachable!()
                }
            }
        }
    }
}

pub struct ReplyStreaming;

impl<
    SF: SerdeFormat,
    MsgItem: Serialize + MaybeSend + 'static,
    Reply: DeserializeOwned + MaybeSend + 'static,
    MsgStream: Stream<Item = Result<MsgItem, RpcError>> + MaybeSend + 'static,
> HandleRpc<SF, MsgItem, Reply, MsgStream> for ReplyStreaming
{
    type Out = StreamWithFut<
        DeSerdeStream<flume::r#async::RecvStream<'static, Bytes>, SF, Reply>,
        std::future::Pending<Result<Reply, RpcError>>,
    >;

    fn handle(
        self,
        msg_sender: Sender<InnerMsg>,
        serde_format: SF,
        op_id: RpcOpId,
        msg: Bytes,
        _msg_stream: Option<MsgStream>,
    ) -> impl Future<Output = Result<Self::Out, RpcError>> + MaybeSend + 'static {
        assert!(_msg_stream.is_none());
        let (reply_sender, reply_receiver) = flume::unbounded();
        let result = rpc_call(
            &msg_sender,
            op_id,
            msg,
            false,
            OneOrMultiSender::Streaming(reply_sender),
        );

        async move {
            result?;
            Ok(StreamWithFut {
                stream: DeSerdeStream {
                    stream: reply_receiver.into_stream(),
                    serde_format,
                    _marker: Default::default(),
                },
                future: std::future::pending(),
            })
        }
    }
}

pub struct BidirectionalStreaming;

impl<
    SF: SerdeFormat,
    MsgItem: Serialize + MaybeSend + 'static,
    Reply: DeserializeOwned + MaybeSend + 'static,
    MsgStream: Stream<Item = Result<MsgItem, RpcError>> + MaybeSend + 'static,
> HandleRpc<SF, MsgItem, Reply, MsgStream> for BidirectionalStreaming
{
    type Out = StreamWithFut<
        DeSerdeStream<flume::r#async::RecvStream<'static, Bytes>, SF, Reply>,
        futures_util::future::Either<
            SendRpcMsgAll<MsgStream, SF, MsgItem, Reply>,
            std::future::Pending<Result<Reply, RpcError>>,
        >,
    >;

    fn handle(
        self,
        msg_sender: Sender<InnerMsg>,
        serde_format: SF,
        op_id: RpcOpId,
        start_msg: Bytes,
        stream: Option<MsgStream>,
    ) -> impl Future<Output = Result<Self::Out, RpcError>> + MaybeSend + 'static {
        let (reply_sender, reply_receiver) = flume::unbounded();
        let result = rpc_call(
            &msg_sender,
            op_id,
            start_msg,
            true,
            OneOrMultiSender::Streaming(reply_sender),
        );

        let send_all = stream
            .map({
                let serde_format = serde_format.clone();
                move |stream| SendRpcMsgAll {
                    stream,
                    cancel_on_drop: SendCancelOnDrop {
                        msg_sender: msg_sender.clone(),
                        op_id,
                    },
                    msg_sender,
                    serde_format,
                    is_end: false,
                    op_id,
                    _marker: Default::default(),
                }
            })
            .map(|n| n.left_future())
            .unwrap_or(std::future::pending::<Result<Reply, RpcError>>().right_future());

        async move {
            result?;
            Ok(StreamWithFut {
                stream: DeSerdeStream {
                    stream: reply_receiver.into_stream(),
                    serde_format,
                    _marker: Default::default(),
                },
                future: send_all,
            })
        }
    }
}

// pub struct OnlyBytesMsg;
//
// impl<SF: SerdeFormat> HandleRpc<SF, (), (), EmptyStream> for OnlyBytesMsg {
//     type Out = ();
//
//     fn handle(
//         self,
//         msg_sender: Sender<InnerMsg>,
//         _serde_format: SF,
//         op_id: RpcOpId,
//         start_msg: Bytes,
//         _stream: Option<EmptyStream>,
//     ) -> impl Future<Output = Result<Self::Out, RpcError>> + MaybeSend + 'static {
//         let (reply_sender, reply_receiver) = futures_channel::oneshot::channel();
//         let result = rpc_call(
//             &msg_sender,
//             op_id,
//             start_msg,
//             false,
//             OneOrMultiSender::Unary(reply_sender),
//         );
//         async move {
//             result?;
//             let mut cancel_when_drop = OnDrop::new(move || {
//                 let _ = msg_sender.send(InnerMsg::Cancel(op_id));
//             });
//
//             let _ = reply_receiver
//                 .await
//                 .map_err(|_err| RpcError::RecvCallReplyCancelled)?;
//             cancel_when_drop.forget_drop();
//             Ok(())
//         }
//     }
// }

#[pin_project]
pub struct DeSerdeStream<S, SF, T> {
    #[pin]
    stream: S,
    serde_format: SF,
    _marker: PhantomData<T>,
}

impl<T, S: Stream<Item = Bytes>, SF: SerdeFormat> Stream for DeSerdeStream<S, SF, T>
where
    T: DeserializeOwned + MaybeSend + 'static,
{
    type Item = Result<T, RpcError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let me = self.project();
        Poll::Ready(match ready!(me.stream.poll_next(cx)) {
            Some(bytes) => {
                match me
                    .serde_format
                    .deserialize_from_slice_optimized::<T>(&bytes)
                    .map_err(RpcError::SerdeError)
                {
                    Ok(item) => Some(Ok(item)),
                    Err(err) => Some(Err(err)),
                }
            }
            None => None,
        })
    }
}

#[pin_project]
pub struct StreamWithFut<S, F> {
    #[pin]
    stream: S,
    #[pin]
    future: F,
}

impl<S: Stream, F: Future<Output = S::Item>> Stream for StreamWithFut<S, F> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let me = self.project();
        let _poll = me.future.poll(cx);
        if _poll.is_ready() {
            return _poll.map(|n| Some(n));
        }
        me.stream.poll_next(cx)
    }
}

pub struct OnDrop<F>
where
    F: FnOnce(),
{
    on_drop: Option<F>,
}

impl<F> OnDrop<F>
where
    F: FnOnce(),
{
    pub fn new(f: F) -> Self {
        Self { on_drop: Some(f) }
    }
    pub fn forget_drop(&mut self) {
        self.on_drop = None;
    }
}

impl<F> Drop for OnDrop<F>
where
    F: FnOnce(),
{
    fn drop(&mut self) {
        if let Some(drop) = self.on_drop.take() {
            drop();
        }
    }
}

pub(crate) enum OneOrMultiSender {
    Unary(futures_channel::oneshot::Sender<Bytes>),
    Streaming(flume::Sender<Bytes>),
}

fn rpc_call(
    msg_sender: &flume::Sender<InnerMsg>,
    op_id: RpcOpId,
    msg: Bytes,
    msg_stream: bool,
    reply_sender: OneOrMultiSender,
) -> Result<(), RpcError> {
    msg_sender
        .send(InnerMsg::Call {
            op_id,
            msg,
            stream: msg_stream,
            reply_sender,
        })
        .map_err(|err| RpcError::CallSendError {
            op_id: {
                let InnerMsg::Call { op_id, .. } = err.0 else {
                    unreachable!()
                };
                op_id
            },
        })?;
    Ok(())
}

pub(crate) struct SendCancelOnDrop {
    msg_sender: flume::Sender<InnerMsg>,
    op_id: RpcOpId,
}

#[pin_project]
pub struct SendRpcMsgAll<S, SF, T, OI = T> {
    #[pin]
    stream: S,
    msg_sender: flume::Sender<InnerMsg>,
    serde_format: SF,
    is_end: bool,
    op_id: RpcOpId,
    cancel_on_drop: SendCancelOnDrop,
    _marker: std::marker::PhantomData<(T, OI)>,
}

impl<S, SF, T, OI> SendRpcMsgAll<S, SF, T, OI> {
    pub fn cast<TOR>(self) -> SendRpcMsgAll<S, SF, T, TOR> {
        SendRpcMsgAll {
            stream: self.stream,
            msg_sender: self.msg_sender,
            serde_format: self.serde_format,
            is_end: self.is_end,
            op_id: self.op_id,
            cancel_on_drop: self.cancel_on_drop,
            _marker: Default::default(),
        }
    }
}

impl<S, T: Serialize + 'static, SF: SerdeFormat, OI> Future for SendRpcMsgAll<S, SF, T, OI>
where
    S: Stream<Item = Result<T, RpcError>> + MaybeSend + 'static,
{
    type Output = Result<OI, RpcError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut me = self.project();

        if *me.is_end {
            return Poll::Pending;
        }
        match ready!(me.stream.poll_next(cx)) {
            None => {
                me.msg_sender
                    .send(InnerMsg::CallStreaming {
                        op_id: me.op_id.clone(),
                        item: None,
                    })
                    .map_err(|err| RpcError::CallStreamingSendError {
                        op_id: {
                            let InnerMsg::CallStreaming { op_id, .. } = err.0 else {
                                unreachable!()
                            };
                            op_id
                        },
                    })?;
                *me.is_end = true;
            }
            Some(item) => {
                let item = item?;
                let msg: Bytes = match unsafe { item.try_specialize_static::<Bytes>() } {
                    Ok(msg) => msg,
                    Err(item) => BUF
                        .with_borrow_mut(|buf| {
                            buf.clear();
                            me.serde_format
                                .serialize_to_writer_optimized(buf.writer(), &item)?;
                            let payload = buf.split().freeze();
                            Ok(payload)
                        })
                        .map_err(RpcError::SerdeError)?,
                };
                me.msg_sender
                    .send(InnerMsg::CallStreaming {
                        op_id: me.op_id.clone(),
                        item: Some(msg),
                    })
                    .map_err(|err| RpcError::CallStreamingSendError {
                        op_id: {
                            let InnerMsg::CallStreaming { op_id, .. } = err.0 else {
                                unreachable!()
                            };
                            op_id
                        },
                    })?;
            }
        }
        Poll::Pending
    }
}
