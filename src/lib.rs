// #[cfg(feature = "axum")]
// pub mod axum;
// #[cfg(target_arch = "wasm32")]
// pub mod web;
pub use auto_enums::enum_derive;
pub use flume;
#[cfg(feature = "rt_tokio_without_send_sync")]
pub mod tokio;

pub use xy_rpc_macro as macros;

mod channel;
#[cfg(feature = "rt_compio")]
pub mod compio;
#[cfg(feature = "duplex")]
pub mod duplex;
pub mod formats;
mod frame;
mod handle_rpc;
pub mod maybe_send;

pub use handle_rpc::*;

pub use channel::*;

use crate::formats::SerdeFormat;
use crate::frame::{
    RpcFrame, RpcFrameHead, RpcFrameHeadRpc, RpcMsgKind, RpcMsgSendId, RpcStreamKind, read_frame,
    write_frame,
};
use crate::maybe_send::{MaybeSend, MaybeSync};
use bytes::{Bytes, BytesMut};
use derive_more::{Display, Error, From};
use flume::{Receiver, RecvError, Sender};
use futures_channel::oneshot::Cancellation;
use futures_util::stream::{BoxStream, FuturesUnordered, LocalBoxStream};
use futures_util::{
    AsyncReadExt, AsyncWriteExt, FutureExt, Sink, SinkExt, Stream, StreamExt, TryStream,
    TryStreamExt,
};
use pin_project::{pin_project, pinned_drop};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};
use tracing::{info, warn};
pub use xy_rpc_macro::rpc_service;

#[derive(Error, Debug)]
pub struct RpcMsgItem<T = Bytes> {
    pub op_id: RpcOpId,
    pub msg: T,
}
impl Display for RpcMsgItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

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
    Sink<RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + MaybeSync + 'static>
    + MaybeSend
    + 'static
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

pub trait RpcServiceSchema: Clone {}

impl RpcServiceSchema for () {}

pub struct RpcRawMsg {
    pub msg: Bytes,
    pub msg_kind: RpcMsgKind,
}

pub enum HandleReply {
    Once(Bytes),
    Stream(maybe_send::BoxedStreamMaybeLocal<'static, Result<Bytes, RpcError>>),
}

pub trait RpcMsgHandler<S: RpcServiceSchema>: MaybeSend + MaybeSync {
    fn handle(
        &self,
        msg: RpcRawMsg,
        stream: Option<flume::Receiver<Bytes>>,
        serde_format: &impl SerdeFormat,
    ) -> Result<impl Future<Output = Result<HandleReply, RpcError>> + MaybeSend, RpcError>;
}
impl<S: RpcServiceSchema> RpcMsgHandler<S> for () {
    fn handle(
        &self,
        _msg: RpcRawMsg,
        _stream: Option<flume::Receiver<Bytes>>,
        _serde_format: &impl SerdeFormat,
    ) -> Result<impl Future<Output = Result<HandleReply, RpcError>> + MaybeSend, RpcError> {
        Ok(async move { unreachable!("no handler") })
    }
}

impl RpcMsgHandler<()> for RpcMsgHandlerWrapper<()> {
    fn handle(
        &self,
        _msg: RpcRawMsg,
        _stream: Option<flume::Receiver<Bytes>>,
        _serde_format: &impl SerdeFormat,
    ) -> Result<impl Future<Output = Result<HandleReply, RpcError>> + MaybeSend, RpcError> {
        Ok(async move { unreachable!("no handler") })
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

pub fn new_transport_stream(
    read: impl futures_util::AsyncRead + Unpin + MaybeSend + 'static,
) -> impl RpcTransportStream<Error = RpcError> + Unpin {
    let buf = BytesMut::with_capacity(1024 * 8);
    let future = futures_util::stream::unfold((read, buf), move |(mut read, mut buf)| async move {
        match read_frame(&mut read).await? {
            Ok(frame_head) => {
                let payload_len = frame_head.payload_len();
                if payload_len == 0 {
                    return Some((
                        Ok(RpcFrame {
                            head: frame_head,
                            payload: Default::default(),
                        }),
                        (read, buf),
                    ));
                }

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
            write_frame(&mut write, frame.head).await?;
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

pub struct FnServiceFactory<F, M>(F, PhantomData<M>);

impl<F, M> FnServiceFactory<F, M> {
    pub fn new(f: F) -> Self {
        Self(f, PhantomData)
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
