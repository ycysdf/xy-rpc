use crate::formats::SerdeFormat;
use crate::frame::{
    RpcFrame, RpcFrameHead, RpcMsgKind, RpcMsgSendId, RpcStreamKind,
};
use crate::handle_rpc::{HandleRpc, OneOrMultiSender};
use crate::maybe_send::{AnyError, MaybeSend};
use crate::{
    HandleReply, RpcMsgHandler, RpcRawMsg, RpcSchema, RpcTransportSink, RpcTransportStream,
    ServiceFactory, maybe_send, temp_buf,
};
use alloc::string::String;
use alloc::sync::Arc;
use auto_enums::enum_derive;
use bytes::Bytes;
use core::fmt::Debug;
use core::marker::PhantomData;
use core::pin::Pin;
use core::task::{Context, Poll};
use derive_more::{Display, Error, From};
use futures_util::stream::FuturesUnordered;
use futures_util::{FutureExt, SinkExt, Stream, StreamExt, TryStreamExt};
use hashbrown::HashMap;
use pin_project::pin_project;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::warn;
use try_specialize::TrySpecialize;

#[derive(Clone, Hash, Eq, PartialEq, Debug, Copy, Display)]
#[display("{self:?}")]
pub struct RpcOpId {
    pub msg_kind: RpcMsgKind,
    pub msg_id: RpcMsgSendId,
}

#[derive(Error, From, Display, Debug)]
pub enum RpcError {
    #[cfg(feature = "std")]
    Io(std::io::Error),
    #[from(ignore)]
    SerdeError(AnyError),
    #[display("write error. len: {}",bytes.len())]
    WriteError {
        bytes: alloc::vec::Vec<u8>,
    },
    InvalidFrame,
    InvalidMsg,
    InvalidMsgKind,
    #[from(ignore)]
    CallSendError {
        op_id: RpcOpId,
    },
    #[from(ignore)]
    CallStreamingSendError {
        op_id: RpcOpId,
    },
    RecvCallReplyCancelled,
    ServeExceptionEnd,
    OtherError {
        message: String,
    },
}

pub enum InnerMsg {
    Call {
        op_id: RpcOpId,
        msg: Bytes,
        stream: bool,
        reply_sender: OneOrMultiSender,
    },
    CallStreaming {
        op_id: RpcOpId,
        item: Option<Bytes>,
    },
    Cancel(RpcOpId),
}

pub struct XyRpcChannel<SF, CS: RpcSchema = ()> {
    sender_id_atomic: Arc<portable_atomic::AtomicU16>,
    msg_sender: flume::Sender<InnerMsg>,
    serde_format: SF,
    _marker: PhantomData<CS>,
}
impl<SF, CS: RpcSchema> Clone for XyRpcChannel<SF, CS>
where
    SF: SerdeFormat,
{
    fn clone(&self) -> Self {
        Self {
            sender_id_atomic: self.sender_id_atomic.clone(),
            serde_format: self.serde_format.clone(),
            msg_sender: self.msg_sender.clone(),
            _marker: Default::default(),
        }
    }
}

impl<SF: SerdeFormat, CS: RpcSchema> XyRpcChannel<SF, CS> {
    pub fn new(
        #[allow(unused_mut)] mut transport_sink: impl RpcTransportSink + Unpin,
        #[allow(unused_mut)] mut transport_stream: impl RpcTransportStream + Unpin,
        msg_handler: impl ServiceFactory<SF, CS>,
        serde_format: SF,
    ) -> (
        Self,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
    {
        // #[cfg(feature = "tracing_live")]
        // use tracing_lv_track::{TLSinkInstrumentExt, TLStreamInstrumentExt};
        // #[cfg(feature = "tracing_live")]
        // let mut transport_sink = transport_sink.instrument_sink("transport sink");
        // #[cfg(feature = "tracing_live")]
        // let mut transport_stream = transport_stream.instrument_stream("transport stream");
        let (msg_sender, msg_receiver) = flume::unbounded();
        let channel = Self {
            sender_id_atomic: Arc::new(portable_atomic::AtomicU16::new(1)),
            msg_sender,
            serde_format: serde_format.clone(),
            _marker: Default::default(),
        };
        let msg_handler = msg_handler.create(channel.clone());

        (channel, async move {
            let mut calls: HashMap<RpcOpId, OneOrMultiSender> = HashMap::default();
            let mut call_msg_stream_item_sender: HashMap<RpcOpId, flume::Sender<Bytes>> =
                HashMap::default();

            #[enum_derive(Future)]
            enum SelectFutures<F1, F2, F4> {
                RpcHanded(F1),
                OutStreamNexted(F2),
                Msg(F4),
            }
            let mut futures = FuturesUnordered::new();
            futures.push(SelectFutures::Msg(
                msg_receiver.recv_async().map(SelectFutures::Msg),
            ));

            let mut handle_frame =
                |frame: RpcFrame,
                 futures: &mut FuturesUnordered<_>,
                 calls: &mut HashMap<RpcOpId, OneOrMultiSender>| {
                    match frame.head {
                        RpcFrameHead::Msg(_head) => {
                            warn!("unexpected msg frame.");
                        }
                        RpcFrameHead::Rpc(head) => {
                            let op_id = head.op_id();
                            // println!("Rpc key: {key:?}. reply: {reply}");
                            if !head.reply() {
                                match head.stream() {
                                    RpcStreamKind::None | RpcStreamKind::StreamStart => {
                                        let (stream_sender, stream_receiver) = (head.stream()
                                            == RpcStreamKind::StreamStart)
                                            .then_some(flume::unbounded())
                                            .unzip();
                                        match msg_handler.handle(
                                            RpcRawMsg {
                                                msg: frame.payload,
                                                msg_kind: head.msg_kind(),
                                            },
                                            stream_receiver,
                                            &serde_format,
                                        ) {
                                            Ok(fut) => {
                                                if let Some(stream_sender) = stream_sender {
                                                    call_msg_stream_item_sender
                                                        .insert(op_id, stream_sender);
                                                }
                                                futures.push(SelectFutures::RpcHanded(fut.map(
                                                    move |reply| {
                                                        SelectFutures::RpcHanded((reply, op_id))
                                                    },
                                                )));
                                            }
                                            Err(err) => {
                                                warn!("error handling message: {:?}", err);
                                            }
                                        }
                                    }
                                    RpcStreamKind::StreamItem => {
                                        let Some(sender) = call_msg_stream_item_sender.get(&op_id)
                                        else {
                                            warn!(?op_id, "not found stream item sender");
                                            return;
                                        };
                                        if sender.send(frame.payload).is_err() {
                                            warn!(?op_id, "stream item receiver dropped");
                                        }
                                    }
                                    RpcStreamKind::StreamEnd => {
                                        if call_msg_stream_item_sender.remove(&op_id).is_none() {
                                            warn!(?op_id, "not found stream item sender")
                                        }
                                    }
                                }
                            } else {
                                match head.stream() {
                                    RpcStreamKind::None => {
                                        let Some(call_variant) = calls.remove(&op_id) else {
                                            warn!(?op_id, "unexpected reply frame.");
                                            return;
                                        };
                                        let OneOrMultiSender::Unary(reply_sender) = call_variant
                                        else {
                                            warn!(?op_id, "unexpected call.");
                                            return;
                                        };
                                        // err when call cancel
                                        let _ = reply_sender.send(frame.payload);
                                    }
                                    RpcStreamKind::StreamStart => {}
                                    RpcStreamKind::StreamItem => {
                                        let Some(call_variant) = calls.get(&op_id) else {
                                            warn!(?op_id, "unexpected reply frame.");
                                            return;
                                        };
                                        let OneOrMultiSender::Streaming(reply_item_sender) =
                                            &call_variant
                                        else {
                                            warn!(?op_id, "unexpected call.");
                                            return;
                                        };
                                        let _ = reply_item_sender.send(frame.payload);
                                    }
                                    RpcStreamKind::StreamEnd => {
                                        let Some(call_variant) = calls.remove(&op_id) else {
                                            warn!(?op_id, "unexpected reply frame.");
                                            return;
                                        };
                                        drop(call_variant);
                                    }
                                }
                            }
                        }
                        RpcFrameHead::CancelRpc(head) => {
                            let op_id = head.op_id();
                            if calls.remove(&op_id).is_none() {
                                warn!(?op_id, "cancel op_id. but op not exist")
                            }
                        }
                    }
                };

            let mut handle_future =
                async |fut: SelectFutures<(Result<HandleReply, RpcError>, RpcOpId), _, _>,
                       futures: &mut FuturesUnordered<_>,
                       calls: &mut HashMap<RpcOpId, OneOrMultiSender>| {
                    match fut {
                        SelectFutures::RpcHanded((reply, op_id)) => match reply {
                            Ok(reply) => {
                                let payload = reply;
                                match payload {
                                    HandleReply::Once(payload) => {
                                        transport_sink
                                            .send(RpcFrame::reply(op_id, RpcStreamKind::None, payload))
                                            .await?;
                                    }
                                    HandleReply::Stream(out_stream) => {
                                        futures.push(SelectFutures::OutStreamNexted(
                                            get_next_out_stream_future(out_stream, op_id)
                                                .map(SelectFutures::OutStreamNexted),
                                        ));
                                    }
                                }
                            }
                            Err(err) => {
                                warn!("error handling message: {:?}", err);
                            }
                        },
                        SelectFutures::Msg(msg) => {
                            let Ok(msg): Result<InnerMsg, flume::RecvError> = msg else {
                                warn!("Call serve end");
                                return Ok(Some(()));
                            };
                            futures.push(SelectFutures::Msg(
                                msg_receiver.recv_async().map(SelectFutures::Msg),
                            ));
                            let frame = match msg {
                                InnerMsg::Call {
                                    op_id,
                                    msg: payload,
                                    stream,
                                    reply_sender,
                                } => {
                                    if let Some(_reply_sender) = calls.insert(op_id, reply_sender) {
                                        warn!(?op_id, "call conflict");
                                    }
                                    RpcFrame::call(
                                        op_id,
                                        if stream {
                                            RpcStreamKind::StreamStart
                                        } else {
                                            RpcStreamKind::None
                                        },
                                        payload,
                                    )
                                }
                                InnerMsg::CallStreaming { op_id, item } => {
                                    if let Some(payload) = item {
                                        RpcFrame::call(op_id, RpcStreamKind::StreamItem, payload)
                                    } else {
                                        RpcFrame::call(op_id, RpcStreamKind::StreamEnd, Default::default())
                                    }
                                }
                                InnerMsg::Cancel(op_id) => RpcFrame::cancel(op_id),
                            };

                            transport_sink.send(frame).await?;
                        }
                        SelectFutures::OutStreamNexted((next, op_id)) => match next {
                            Some(Ok((out_stream, out_item))) => {
                                transport_sink.send(out_item).await?;
                                futures.push(SelectFutures::OutStreamNexted(
                                    get_next_out_stream_future(out_stream, op_id)
                                        .map(SelectFutures::OutStreamNexted),
                                ));
                            }
                            Some(Err(err)) => {
                                warn!("error out stream error: {:?}", err);
                            }
                            None => {
                                transport_sink.send(RpcFrame::stream_end(op_id)).await?;
                            }
                        },
                    }
                    Ok::<_, RpcError>(None)
                };

            loop {
                futures_util::select! {
                   frame = transport_stream.try_next().fuse() => {
                       let Some(frame) = frame? else {
                           warn!("transport_stream serve end");
                           break;
                       };
                       handle_frame(frame,&mut futures,&mut calls);
                   }
                   r = futures.next().fuse() => {
                       let Some(r) = r else {
                           warn!("futures serve end");
                           break;
                       };
                       if let Some(_) = handle_future(r,&mut futures,&mut calls).await? {
                            break;
                        }
                 }
                }
            }
            Ok::<(), RpcError>(())
        })
    }

    pub fn call<
        Msg: Serialize + Debug,
        MsgItem: Serialize + 'static,
        Reply: DeserializeOwned + MaybeSend + 'static,
        MsgStream: Stream<Item = Result<MsgItem, RpcError>> + MaybeSend + 'static,
        H: HandleRpc<SF, MsgItem, Reply, MsgStream>,
    >(
        &self,
        msg: &Msg,
        stream: Option<MsgStream>,
        msg_kind: RpcMsgKind,
        handle_rpc: H,
    ) -> impl Future<Output = Result<H::Out, RpcError>>
    + MaybeSend
    + use<H, Msg, MsgItem, Reply, MsgStream, SF, CS>
    + 'static {
        let send_id = self
            .sender_id_atomic
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let op_id = RpcOpId {
            msg_kind,
            msg_id: send_id,
        };
        let msg = if let Some(raw_bytes) =
            unsafe { msg.try_specialize_ref_ignore_lifetimes::<Bytes>() }
        {
            raw_bytes.clone()
        } else {
            match temp_buf::with_buf(|buf| {
                self.serde_format.serialize_to_writer_optimized(buf, msg)?;
                let payload = buf.split().freeze();
                Ok(payload)
            })
            .map_err(RpcError::SerdeError)
            {
                Ok(msg) => msg,
                Err(err) => return futures_util::future::ready(Err(err)).left_future(),
            }
        };
        handle_rpc
            .handle(
                self.msg_sender.clone(),
                self.serde_format.clone(),
                op_id,
                msg,
                stream,
            )
            .right_future()
    }
}

async fn get_next_out_stream_future(
    mut out_stream: maybe_send::BoxedStreamMaybeLocal<'static, Result<Bytes, RpcError>>,
    op_id: RpcOpId,
) -> (
    Option<
        Result<
            (
                maybe_send::BoxedStreamMaybeLocal<'static, Result<Bytes, RpcError>>,
                RpcFrame,
            ),
            RpcError,
        >,
    >,
    RpcOpId,
) {
    let item = out_stream.next().await;
    (
        item.map(move |n| {
            n.map(move |payload| {
                (
                    out_stream,
                    RpcFrame::stream_item(op_id, payload),
                )
            })
        }),
        op_id,
    )
}

#[pin_project]
pub struct TransStream<T: 'static, SF: SerdeFormat> {
    #[pin]
    pub bytes_stream: flume::r#async::RecvStream<'static, Bytes>,
    pub serde_format: SF,
    pub _marker: PhantomData<T>,
}

impl<T: 'static, SF: SerdeFormat> TransStream<T, SF> {
    pub fn new(bytes_stream: flume::r#async::RecvStream<'static, Bytes>, serde_format: SF) -> Self {
        Self {
            bytes_stream,
            serde_format,
            _marker: Default::default(),
        }
    }
}

impl<T: DeserializeOwned + 'static, SF: SerdeFormat> Stream for TransStream<T, SF> {
    type Item = Result<T, RpcError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let me = self.project();
        let poll = me.bytes_stream.poll_next(cx);
        match poll {
            Poll::Ready(Some(bytes)) => {
                match me
                    .serde_format
                    .deserialize_from_slice_optimized::<T>(&bytes)
                {
                    Ok(n) => Poll::Ready(Some(Ok(n))),
                    Err(err) => Poll::Ready(Some(Err(RpcError::SerdeError(err)))),
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
