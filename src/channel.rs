use crate::formats::SerdeFormat;
use crate::frame::{
    RpcFrame, RpcFrameHead, RpcFrameHeadRpc, RpcMsgKind, RpcMsgSendId, RpcStreamKind,
};
use crate::handle_rpc::{HandleRpc, OneOrMultiSender};
use crate::maybe_send::{AnyError, MaybeSend};
use crate::{
    HandleReply, RpcMsgHandler, RpcRawMsg, RpcServiceSchema, RpcTransportSink, RpcTransportStream,
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
use tracing::{info, warn};
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

impl RpcError {
    pub fn is_serve_aborted(&self) -> bool {
        matches!(
            self,
            RpcError::RecvCallReplyCancelled | RpcError::CallSendError { .. }
        )
    }
}

pub enum InnerMsg {
    Call {
        op_id: RpcOpId,
        msg: Bytes,
        stream: bool,
        reply_sender: OneOrMultiSender,
    },
    // Call(CallMsg),
    CallStreaming {
        op_id: RpcOpId,
        item: Option<Bytes>,
    },
    Cancel(RpcOpId),
}

// pub struct CallMsg {
//     item: RpcMsgItem,
//     reply_sender: futures_channel::oneshot::Sender<RpcMsgItem>,
// }

pub struct XyRpcChannel<SF, CS: RpcServiceSchema = ()> {
    sender_id_atomic: Arc<portable_atomic::AtomicU16>,
    msg_sender: flume::Sender<InnerMsg>,
    serde_format: SF,
    _marker: PhantomData<CS>,
}
impl<SF, CS: RpcServiceSchema> Clone for XyRpcChannel<SF, CS>
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

impl<SF: SerdeFormat, CS: RpcServiceSchema> XyRpcChannel<SF, CS> {
    pub fn new(
        mut transport_sink: impl RpcTransportSink + Unpin,
        mut transport_stream: impl RpcTransportStream + Unpin,
        msg_handler: impl ServiceFactory<SF, CS>,
        serde_format: SF,
    ) -> (
        Self,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
    {
        let (msg_sender, msg_receiver) = flume::unbounded();
        let channel = Self {
            sender_id_atomic: Arc::new(portable_atomic::AtomicU16::new(1)),
            msg_sender,
            serde_format: serde_format.clone(),
            _marker: Default::default(),
        };
        let msg_handler = msg_handler.create(channel.clone());

        (channel, async move {
            enum CallVariant {
                ReplyOnce {
                    reply_sender: futures_channel::oneshot::Sender<Bytes>,
                },
                ReplyMulti {
                    reply_item_sender: flume::Sender<Bytes>,
                },
            }
            let mut calls: HashMap<RpcOpId, CallVariant> = HashMap::default();
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
            loop {
                futures_util::select! {
                       frame = transport_stream.try_next().fuse() => {
                           let Some(frame) = frame.map_err(|err|err.into())? else {
                               warn!("transport_stream serve end");
                               break;
                           };
                           info!(?frame,"recv frame");
                           match frame.head {
                               RpcFrameHead::Msg(_head) => {
                                   warn!("unexpected msg frame.");
                               }
                               // RpcFrameHead::Rpc(RpcFrameHeadRpc{reply,exclusive: _,stream,msg_kind,msg_id,payload_len: _  }) => {
                               RpcFrameHead::Rpc(head) => {
                                   let op_id = head.op_id();
                                   // println!("Rpc key: {key:?}. reply: {reply}");
                                   if !head.reply() {
                                       match head.stream() {
                                           RpcStreamKind::None|RpcStreamKind::StreamStart => {
                                             let (stream_sender,stream_receiver) = (head.stream() == RpcStreamKind::StreamStart).then_some(flume::unbounded()).unzip();
                                             match msg_handler.handle(RpcRawMsg { msg: frame.payload.clone(),msg_kind:head.msg_kind(),},stream_receiver,&serde_format) {
                                                 Ok(fut) => {
                                                     if let Some(stream_sender) = stream_sender {
                                                         call_msg_stream_item_sender.insert(op_id, stream_sender);
                                                     }
                                                     futures.push(SelectFutures::RpcHanded(fut.map(move |reply| SelectFutures::RpcHanded((reply, op_id))),
                                                     ));
                                                 }
                                                 Err(err) => {
                                                     warn!("error handling message: {:?}", err);
                                                 }
                                            }
                                          }
                                           RpcStreamKind::StreamItem => {
                                             let Some(sender) = call_msg_stream_item_sender.get(&op_id) else {
                                                 warn!(?op_id,"not found stream item sender");
                                                 continue;
                                             };
                                             // TODO:
                                             sender.send(frame.payload).unwrap()
                                           }
                                           RpcStreamKind::StreamEnd => {
                                             if call_msg_stream_item_sender.remove(&op_id).is_none() {
                                                 warn!(?op_id,"not found stream item sender")
                                             }
                                         }
                                       }
                                   } else {
                                       match head.stream() {
                                           RpcStreamKind::None => {
                                               let Some(call_variant) = calls.remove(&op_id) else {
                                                   warn!(?op_id,"unexpected reply frame.");
                                                   continue;
                                               };
                                               let CallVariant::ReplyOnce{
                                                   reply_sender
                                               } = call_variant else {
                                                   warn!(?op_id,"unexpected call.");
                                                   continue;
                                               };
                                               // err when call cancel
                                               let _ = reply_sender.send(frame.payload);
                                           }
                                           RpcStreamKind::StreamStart => {}
                                           RpcStreamKind::StreamItem => {
                                               let Some(call_variant) = calls.get(&op_id) else {
                                                   warn!(?op_id,"unexpected reply frame.");
                                                   continue;
                                               };
                                               let CallVariant::ReplyMulti {
                                                   reply_item_sender
                                               } = &call_variant else {
                                                   warn!(?op_id,"unexpected call.");
                                                   continue;
                                               };
                                               let _ = reply_item_sender.send(frame.payload);
                                           }
                                           RpcStreamKind::StreamEnd => {
                                               let Some(call_variant) = calls.remove(&op_id) else {
                                                   warn!(?op_id,"unexpected reply frame.");
                                                   continue;
                                               };
                                               drop(call_variant);
                                           }
                                       }
                                   }
                               }
                             RpcFrameHead::CancelRpc(head) => {
                                  let op_id = head.op_id();
                                 if calls.remove(&op_id).is_none() {
                                     warn!(?op_id,"cancel op_id. but op not exist")
                                 }
                             }
                         }
                       }
                       r = futures.next().fuse() => {
                           let Some(r) = r else {
                               warn!("futures serve end");
                               break;
                           };
                           match r {
                               SelectFutures::RpcHanded((reply, op_id)) => {
                                   // serde_format.serialize_to_writer(&mut buf, &reply).map_err(|err| RpcError::SerdeError(err))?;
                                   // let payload = buf.get_mut().split().freeze();
                                  match reply {
                                     Ok(reply) => {
                                         let payload = reply;
                                         match payload {
                                             HandleReply::Once(payload) => {
                                                 transport_sink
                                                   .send(RpcFrame {
                                                   head: RpcFrameHead::Rpc(RpcFrameHeadRpc::new()
                                                         .with_reply(true)
                                                         .with_exclusive(false)
                                                         .with_stream(RpcStreamKind::None)
                                                         .with_msg_kind(op_id.msg_kind)
                                                         .with_msg_id(op_id.msg_id)
                                                         .with_payload_len(payload.len() as _)
                                                     ),
                                                     payload,
                                                 })
                                                   .await.map_err(|n| n.into())?;
                                             }
                                             HandleReply::Stream(out_stream) => {
                                                 futures.push(SelectFutures::OutStreamNexted(get_next_out_stream_future(out_stream,op_id).map(SelectFutures::OutStreamNexted)));
                                             }
                                         }
                                     }
                                     Err(err) => {
                                         warn!("error handling message: {:?}", err);
                                     }
                                 }
                               }
                               SelectFutures::Msg(msg) => {
                                   let Ok(msg): Result<
                                       InnerMsg,
                                       flume::RecvError,
                                   > = msg
                                   else {
                                       warn!("Call serve end");
                                       break;
                                   };
                                 futures.push(SelectFutures::Msg(
                                     msg_receiver.recv_async().map(SelectFutures::Msg),
                                 ));
                                   match msg {
                                       InnerMsg::Call{op_id,msg:payload,stream,reply_sender}=>{
                                         let frame = RpcFrame {
                                               head: RpcFrameHead::Rpc(RpcFrameHeadRpc::new()
                                                     .with_reply(false)
                                                     .with_exclusive(false)
                                                     .with_stream(if stream {RpcStreamKind::StreamStart}else{RpcStreamKind::None})
                                                     .with_msg_id(op_id.msg_id)
                                                     .with_msg_kind(op_id.msg_kind)
                                                     .with_payload_len(payload.len() as _)
                                               ),
                                                 payload,
                                             };
                                           info!(?frame,"send frame");
                                           transport_sink
                                               .send(frame)
                                               .await.map_err(|n| n.into())?;
                                           match reply_sender {
                                             OneOrMultiSender::Unary(reply_sender) => {
                                               calls.insert(op_id, CallVariant::ReplyOnce {
                                                   reply_sender
                                               });
                                             }
                                             OneOrMultiSender::Streaming(reply_item_sender) => {
                                               calls.insert(op_id, CallVariant::ReplyMulti {
                                                   reply_item_sender
                                               });
                                             }
                                          }
                                      }
                                     InnerMsg::CallStreaming{op_id,item} => {
                                           if let Some(payload) = item {
                                               transport_sink
                                                   .send(RpcFrame {
                                                   head: RpcFrameHead::Rpc(RpcFrameHeadRpc::new()
                                                         .with_reply(false)
                                                         .with_exclusive(false)
                                                         .with_stream(RpcStreamKind::StreamItem)
                                                         .with_msg_id(op_id.msg_id)
                                                         .with_msg_kind(op_id.msg_kind)
                                                         .with_payload_len(payload.len() as _)
                                                   ),
                                                     payload,})
                                                   .await.map_err(|n| n.into())?;

                                             } else {
                                               transport_sink
                                                   .send(RpcFrame {
                                                   head: RpcFrameHead::Rpc(RpcFrameHeadRpc::new()
                                                         .with_reply(false)
                                                         .with_exclusive(false)
                                                         .with_stream(RpcStreamKind::StreamEnd)
                                                         .with_msg_id(op_id.msg_id)
                                                         .with_msg_kind(op_id.msg_kind)
                                                         .with_payload_len(0)
                                                   ),payload:Bytes::default(),})
                                                   .await.map_err(|n| n.into())?;
                                             }
                                         }
                                     InnerMsg::Cancel(op_id) => {
                                           transport_sink
                                               .send(RpcFrame::cancel(op_id))
                                               .await.map_err(|n| n.into())?;
                                         // calls.remove(op_id);
                                     }
                                 }
                            }
                         SelectFutures::OutStreamNexted((next,op_id)) => {
                             match next {
                                 Some(Ok((out_stream,out_item))) => {
                                    transport_sink
                                       .send(out_item)
                                       .await.map_err(|n| n.into())?;
                                     futures.push(SelectFutures::OutStreamNexted(get_next_out_stream_future(out_stream,op_id).map(SelectFutures::OutStreamNexted)));
                                 }
                                 Some(Err(err)) => {
                                     warn!("error out stream error: {:?}", err);
                                 }
                                 None => {
                                    transport_sink
                                       .send(RpcFrame::out_stream_end(op_id))
                                       .await.map_err(|n| n.into())?;
                                 }
                             }
                         }
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
                    RpcFrame {
                        head: RpcFrameHead::Rpc(
                            RpcFrameHeadRpc::new()
                                .with_reply(true)
                                .with_exclusive(false)
                                .with_stream(RpcStreamKind::StreamItem)
                                .with_msg_kind(op_id.msg_kind)
                                .with_msg_id(op_id.msg_id)
                                .with_payload_len(payload.len() as _),
                        ),
                        payload,
                    },
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
