// #[cfg(feature = "axum")]
// pub mod axum;
// #[cfg(target_arch = "wasm32")]
// pub mod web;

#[cfg(feature = "rt_tokio_without_send_sync")]
pub mod tokio;

pub use xy_rpc_macro as macros;

#[cfg(feature = "rt_compio")]
pub mod compio;
#[cfg(feature = "duplex")]
pub mod duplex;
pub mod formats;
mod frame;
pub mod maybe_send;

use crate::formats::SerdeFormat;
use crate::frame::{
    RpcFrame, RpcFrameHead, RpcMsgKind, RpcMsgSendId, RpcStreamKind, read_frame, write_frame,
};
use crate::maybe_send::{MaybeSend, MaybeSync};
use auto_enums::enum_derive;
use bytes::{BufMut, Bytes, BytesMut};
use derive_more::{Display, Error, From};
use futures_util::stream::FuturesUnordered;
use futures_util::{
    AsyncReadExt, AsyncWriteExt, FutureExt, Sink, SinkExt, StreamExt, TryStream, TryStreamExt,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::warn;
pub use xy_rpc_macro::rpc_service;

#[derive(Error, Debug)]
pub struct RpcMsgItem<T = Bytes> {
    pub send_id: RpcMsgSendId,
    pub msg: T,
}
impl Display for RpcMsgItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}
pub type RpcFrameKey = (RpcMsgKind, RpcMsgSendId);

pub struct RpcMsgInfo {
    pub id: RpcMsgKind,
    pub name: &'static str,
}

pub trait RpcRefMsg: Serialize + Unpin + Debug + MaybeSend {
    fn info(&self) -> RpcMsgInfo;
}

pub trait RpcMsg: RpcRefMsg + DeserializeOwned + 'static {
    type Ref<'a>: RpcRefMsg;
}

impl RpcRefMsg for () {
    fn info(&self) -> RpcMsgInfo {
        RpcMsgInfo {
            id: 0,
            name: "EMPTY",
        }
    }
}

impl RpcMsg for () {
    type Ref<'a> = ();
}

pub trait RpcTransportStream:
    TryStream<Ok = RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + MaybeSync + 'static>
    + MaybeSend
    + 'static
{
}

impl<S> RpcTransportStream for S where
    S: ?Sized
        + TryStream<Ok = RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + MaybeSync + 'static>
        + MaybeSend
        + 'static
{
}

pub trait RpcTransportSink:
    Sink<RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + MaybeSync + 'static> + MaybeSend + 'static
{
}

impl<S> RpcTransportSink for S where
    S: ?Sized
        + Sink<RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + MaybeSync + 'static>
        + MaybeSend
        + 'static
{
}

pub trait RpcTransport: RpcTransportStream + RpcTransportSink {}

impl<S> RpcTransport for S where S: ?Sized + RpcTransportStream + RpcTransportSink {}

pub trait RpcServiceSchema: Clone {
    type Msg: RpcMsg;
    type Reply: RpcMsg;
}

impl RpcServiceSchema for () {
    type Msg = ();
    type Reply = ();
}

pub trait RpcMsgHandler<S: RpcServiceSchema>: MaybeSend + MaybeSync {
    fn handle(&self, msg: S::Msg) -> impl Future<Output = S::Reply> + MaybeSend;
}
impl<S: RpcServiceSchema> RpcMsgHandler<S> for () {
    fn handle(
        &self,
        _msg: <S as RpcServiceSchema>::Msg,
    ) -> impl Future<Output = S::Reply> + MaybeSend {
        async move { unreachable!("no handler") }
    }
}

impl RpcMsgHandler<()> for RpcMsgHandlerWrapper<()> {
    fn handle(&self, _msg: ()) -> impl Future<Output = ()> + MaybeSend {
        async move { unreachable!("no handler") }
    }
}

// pub trait RpcMsgExclusiveHandler<Msg, Reply>: Send {
//     fn handle(&mut self, msg: Msg) -> impl Future<Output = Reply> + MaybeSend;
// }
// impl<Msg, Reply> RpcMsgExclusiveHandler<Msg, Reply> for () {
//     fn handle(&mut self, _: Msg) -> impl Future<Output = Reply> + MaybeSend {
//         async move { unreachable!("no handler") }
//     }
// }

// #[derive(Clone)]
pub struct ChannelBuilder<SF, CS = (), MH = (), MSG = ()> {
    msg_handler: MH,
    serde_format: SF,
    _marker: PhantomData<(CS, MSG)>,
}
impl<SF: SerdeFormat> Clone for ChannelBuilder<SF, (), ()> {
    fn clone(&self) -> Self {
        Self {
            msg_handler: (),
            serde_format: self.serde_format.clone(),
            _marker: Default::default(),
        }
    }
}
impl<SF> ChannelBuilder<SF>
where
    SF: SerdeFormat,
{
    pub fn new(serde_format: SF) -> ChannelBuilder<SF> {
        Self {
            msg_handler: (),
            serde_format,
            _marker: Default::default(),
        }
    }
}
impl<SF, MSG> ChannelBuilder<SF, (), (), MSG>
where
    SF: SerdeFormat,
{
    pub fn call_and_serve<T, S: RpcServiceSchema, CS>(
        self,
        service: impl FnOnce(XyRpcChannel<SF, CS>) -> T,
    ) -> ChannelBuilder<
        SF,
        CS,
        FnServiceFactory<impl FnOnce(XyRpcChannel<SF, CS>) -> RpcMsgHandlerWrapper<T>, (CS, S)>,
        MSG,
    >
    where
        CS: RpcServiceSchema,
        RpcMsgHandlerWrapper<T>: RpcMsgHandler<S>,
    {
        ChannelBuilder {
            msg_handler: FnServiceFactory::new(move |channel| RpcMsgHandlerWrapper {
                service: service(channel),
            }),
            serde_format: self.serde_format,
            _marker: Default::default(),
        }
    }
    pub fn only_serve<T, S: RpcServiceSchema>(
        self,
        service: impl FnOnce(XyRpcChannel<SF>) -> T,
    ) -> ChannelBuilder<
        SF,
        (),
        FnServiceFactory<impl FnOnce(XyRpcChannel<SF>) -> RpcMsgHandlerWrapper<T>, ((), S)>,
        MSG,
    >
    where
        RpcMsgHandlerWrapper<T>: RpcMsgHandler<S>,
    {
        self.call_and_serve(service)
    }
}

impl<F, MH, MSG> ChannelBuilder<F, (), MH, MSG> {
    pub fn only_call<CS: RpcServiceSchema>(self) -> ChannelBuilder<F, CS, MH, MSG> {
        ChannelBuilder {
            serde_format: self.serde_format,
            msg_handler: self.msg_handler,
            _marker: Default::default(),
        }
    }
}

impl<SF, CS, MH, MSG> ChannelBuilder<SF, CS, MH, MSG>
where
    SF: SerdeFormat,
{
    pub fn build_from_read_write(
        self,
        (read, write): (
            impl futures_util::AsyncRead + Unpin + MaybeSend + 'static,
            impl futures_util::AsyncWrite + Unpin + MaybeSend + 'static,
        ),
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>,
    {
        let stream = new_transport_stream(read);
        let sink = new_transport_sink(write);
        self.build_from_transport(sink, stream)
    }
    pub fn build_from_transport(
        self,
        transport_sink: impl RpcTransportSink + Unpin,
        transport_stream: impl RpcTransportStream + Unpin,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>,
    {
        XyRpcChannel::new(
            transport_sink,
            transport_stream,
            self.msg_handler,
            self.serde_format,
        )
    }
}

pub struct RpcMsgHandlerWrapper<T> {
    pub service: T,
}

pub struct XyRpcChannel<SF, CS: RpcServiceSchema = ()> {
    sender_id_atomic: Arc<portable_atomic::AtomicU16>,
    call_sender: flume::Sender<(
        RpcMsgKind,
        RpcMsgItem,
        futures_channel::oneshot::Sender<RpcMsgItem>,
    )>,
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
            call_sender: self.call_sender.clone(),
            _marker: Default::default(),
        }
    }
}

thread_local! {
    static BUF: std::cell::RefCell<BytesMut> = std::cell::RefCell::new(BytesMut::with_capacity(1024 * 8));
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
        let mut buf = BytesMut::with_capacity(1024 * 8).writer();
        let (call_sender, call_receiver) = flume::unbounded();
        let channel = Self {
            sender_id_atomic: Arc::new(portable_atomic::AtomicU16::new(1)),
            call_sender,
            serde_format: serde_format.clone(),
            _marker: Default::default(),
        };
        let msg_handler = msg_handler.create(channel.clone());

        (channel, async move {
            let mut calls: HashMap<RpcFrameKey, futures_channel::oneshot::Sender<RpcMsgItem>> =
                HashMap::default();

            #[enum_derive(Future)]
            enum SelectFutures<F1, F4> {
                RpcHanded(F1),
                Call(F4),
            }
            let mut futures = FuturesUnordered::new();
            futures.push(SelectFutures::Call(
                call_receiver.recv_async().map(SelectFutures::Call),
            ));
            loop {
                futures_util::select! {
                    frame = transport_stream.try_next().fuse() => {
                        let Some(frame) = frame.map_err(|err|err.into())? else {
                            // println!("transport_stream serve end");
                            break;
                        };
                        match frame.head {
                            RpcFrameHead::Msg{id: _,payload_len: _  } => {
                                warn!("unexpected msg frame.");
                            }
                            RpcFrameHead::Rpc{reply,exclusive: _,stream: _,id,send_id,payload_len: _  } => {
                                let key = (id,send_id);
                                // println!("Rpc key: {key:?}. reply: {reply}");
                                if !reply {
                                    match serde_format.deserialize_from_slice(frame.payload.as_ref()) {
                                          Ok(msg) => {
                                            futures.push(SelectFutures::RpcHanded(
                                                msg_handler
                                                    .handle(msg)
                                                    .map(move |reply| SelectFutures::RpcHanded((reply, key))),
                                            ));
                                          }
                                          Err(err) => {
                                             warn!("deserialize error: {:?}", err);
                                          }
                                    }
                                } else {
                                    let Some(return_sender) = calls.remove(&key) else {
                                        warn!(?key,"unexpected reply frame.");
                                        continue;
                                    };
                                    // err when call cancel
                                    let _ = return_sender.send(RpcMsgItem {
                                        send_id:key.1,msg:frame.payload,
                                    });
                                }
                            }
                        }
                    }
                    r = futures.next().fuse() => {
                        let Some(r) = r else {
                            println!("futures serve end");
                            break;
                        };
                        match r {
                            SelectFutures::RpcHanded((reply, (id,send_id))) => {
                                serde_format.serialize_to_writer(&mut buf, &reply).map_err(|err| RpcError::SerdeError(err))?;
                                let payload = buf.get_mut().split().freeze();
                                transport_sink
                                    .send(RpcFrame {
                                    head: RpcFrameHead::Rpc {
                                        reply: true,exclusive: false,stream: RpcStreamKind::None,id,send_id,payload_len: payload.len() as _,},payload,})
                                    .await.map_err(|n| n.into())?;
                            }
                            SelectFutures::Call(call_msg) => {
                                let Ok((msg_kind,call_msg, return_sender)): Result<
                                    (RpcMsgKind,RpcMsgItem<_>, _),
                                    flume::RecvError,
                                > = call_msg
                                else {
                                    // println!("Call serve end");
                                    break;
                                };
                                // println!("call key: {:?}",(msg_kind,call_msg.send_id));
                                calls.insert((msg_kind,call_msg.send_id), return_sender);

                                let payload = call_msg.msg;
                                transport_sink
                                    .send(RpcFrame {
                                    head: RpcFrameHead::Rpc {
                                        reply: false,exclusive: false,stream: RpcStreamKind::None,send_id:call_msg.send_id,payload_len: payload.len() as _,
                                        id: msg_kind,},payload,})
                                    .await.map_err(|n| n.into())?;
                                futures.push(SelectFutures::Call(
                                    call_receiver.recv_async().map(SelectFutures::Call),
                                ));
                            }
                        }
                    }
                }
            }
            Ok::<(), RpcError>(())
        })
    }

    pub fn call<'a>(
        &self,
        msg: <CS::Msg as RpcMsg>::Ref<'a>,
    ) -> impl Future<Output = Result<RpcMsgItem<CS::Reply>, RpcError>> + MaybeSend + 'static {
        let return_receiver: Result<_, RpcError> = (|| {
            let msg_kind = msg.info().id;
            let msg = BUF
                .with_borrow_mut(|buf| {
                    buf.clear();
                    self.serde_format.serialize_to_writer(buf.writer(), &msg)?;
                    let payload = buf.split().freeze();
                    Ok(payload)
                })
                .map_err(RpcError::SerdeError)?;
            let (return_sender, return_receiver) = futures_channel::oneshot::channel();
            let send_id = self
                .sender_id_atomic
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.call_sender
                .send((msg_kind, RpcMsgItem { send_id, msg }, return_sender))
                .map_err(|err| RpcError::CallSendError(err.0.1))?;
            Ok(return_receiver)
        })();
        let serde_format = self.serde_format.clone();
        async move {
            let msg_item = return_receiver?
                .await
                .map_err(|_err| RpcError::RecvCallReplyCancelled)?;
            let reply = serde_format
                .deserialize_from_slice::<CS::Reply>(msg_item.msg.as_ref())
                .map_err(RpcError::SerdeError)?;
            Ok(RpcMsgItem {
                send_id: msg_item.send_id,
                msg: reply,
            })
        }
    }
}

#[derive(Error, From, Display, Debug)]
pub enum RpcError {
    Io(std::io::Error),
    #[from(ignore)]
    SerdeError(std::io::Error),
    InvalidFrame,
    InvalidMsg,
    CallSendError(RpcMsgItem),
    RecvCallReplyCancelled,
    ServeExceptionEnd,
    OtherError{
        message: String
    }
}

impl RpcError {
    pub fn is_serve_aborted(&self) -> bool {
        matches!(
            self,
            RpcError::RecvCallReplyCancelled | RpcError::CallSendError(_)
        )
    }
}

pub fn new_transport_stream(
    read: impl futures_util::AsyncRead + Unpin + MaybeSend + 'static,
) -> impl RpcTransportStream<Error = RpcError> + Unpin {
    let buf = BytesMut::with_capacity(1024 * 8);
    let future = futures_util::stream::unfold((read, buf), move |(mut read, mut buf)| async move {
        match read_frame(&mut read).await? {
            Ok(frame_head) => {
                // println!("frame_head: {frame_head:#?}");
                let payload_len = match &frame_head {
                    RpcFrameHead::Msg { payload_len, .. } => *payload_len,
                    RpcFrameHead::Rpc { payload_len, .. } => *payload_len,
                };

                buf.resize(payload_len as _, 0);
                match read.read_exact(&mut buf[..payload_len as usize]).await {
                    Ok(_) => {
                        let payload = buf.split().freeze();
                        Some((
                            Ok(RpcFrame {
                                head: frame_head,
                                payload,
                            }),
                            (read, buf),
                        ))
                    }
                    Err(err) => Some((Err(err.into()), (read, buf))),
                }
            }
            Err(err) => Some((Err(err.into()), (read, buf))),
        }
    });

    #[cfg(feature = "send_sync")]
    {
        future.boxed()
    }
    #[cfg(not(feature = "send_sync"))]
    {
        future.boxed_local()
    }
}

pub fn new_transport_sink(
    write: impl futures_util::AsyncWrite + Unpin + MaybeSend + 'static,
) -> impl RpcTransportSink<Error = RpcError> + Unpin {
    Box::pin(futures_util::sink::unfold(
        write,
        |mut write, frame: RpcFrame| async move {
            write_frame(&mut write, &frame.head).await?;
            write.write_all(&frame.payload).await?;
            write.flush().await?;
            Ok(write)
        },
    ))
}

pub trait ServiceFactory<SF, CS: RpcServiceSchema> {
    type S: RpcServiceSchema;
    fn create(self, channel: XyRpcChannel<SF, CS>) -> impl RpcMsgHandler<Self::S> + 'static;
}

impl<SF, CS: RpcServiceSchema> ServiceFactory<SF, CS> for () {
    type S = ();

    fn create(self, _channel: XyRpcChannel<SF, CS>) -> impl RpcMsgHandler<Self::S> + 'static {
        ()
    }
}

pub struct FnServiceFactory<F, M>(F, std::marker::PhantomData<M>);

impl<F, M> FnServiceFactory<F, M> {
    pub fn new(f: F) -> Self {
        Self(f, std::marker::PhantomData)
    }
}

impl<F, S: RpcServiceSchema, H, SF, CS: RpcServiceSchema> ServiceFactory<SF, CS>
    for FnServiceFactory<F, (CS, S)>
where
    F: FnOnce(XyRpcChannel<SF, CS>) -> H,
    H: RpcMsgHandler<S> + 'static,
{
    type S = S;

    fn create(self, channel: XyRpcChannel<SF, CS>) -> impl RpcMsgHandler<S> + 'static {
        self.0(channel)
    }
}
