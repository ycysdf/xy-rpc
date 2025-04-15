use crate::formats::SerdeFormat;
use crate::frame::{RpcFrame, RpcMsgKind};
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::{RpcError, RpcTransportSink, RpcTransportStream, XyRpcChannel, frame, maybe_send};
use bytes::{Bytes, BytesMut};
use futures_util::stream::BoxStream;
use futures_util::{AsyncReadExt, AsyncWriteExt, Stream, StreamExt, TryStream};
use std::marker::PhantomData;
use std::prelude::rust_2015::Box;

pub trait RpcSchema: Clone {}

impl RpcSchema for () {}

pub struct RpcRawMsg {
    pub msg: Bytes,
    pub msg_kind: RpcMsgKind,
}

pub enum HandleReply {
    Once(Bytes),
    Stream(maybe_send::BoxedStreamMaybeLocal<'static, Result<Bytes, RpcError>>),
}
pub trait RpcMsgHandler<S: RpcSchema>: MaybeSend + MaybeSync {
    fn handle(
        &self,
        msg: RpcRawMsg,
        stream: Option<flume::Receiver<Bytes>>,
        serde_format: &impl SerdeFormat,
    ) -> Result<impl Future<Output = Result<HandleReply, RpcError>> + MaybeSend, RpcError>;
}
impl<S: RpcSchema> RpcMsgHandler<S> for () {
    fn handle(
        &self,
        _msg: RpcRawMsg,
        _stream: Option<flume::Receiver<Bytes>>,
        _serde_format: &impl SerdeFormat,
    ) -> Result<impl Future<Output = Result<HandleReply, RpcError>> + MaybeSend, RpcError> {
        Ok(async move { unreachable!("no handler") })
    }
}

// impl RpcMsgHandler<()> for RpcMsgHandlerWrapper<()> {
//     fn handle(
//         &self,
//         _msg: RpcRawMsg,
//         _stream: Option<flume::Receiver<Bytes>>,
//         _serde_format: &impl SerdeFormat,
//     ) -> Result<impl Future<Output = Result<HandleReply, RpcError>> + MaybeSend, RpcError> {
//         Ok(async move { unreachable!("no handler") })
//     }
// }
//
// pub trait RpcMsgExclusiveHandler<S: RpcSchema>: MaybeSend + MaybeSync {
//     fn exclusive_handle(
//         &mut self,
//         msg: RpcRawMsg,
//         stream: Option<flume::Receiver<Bytes>>,
//         serde_format: &impl SerdeFormat,
//     ) -> Result<impl Future<Output = Result<HandleReply, RpcError>> + MaybeSend, RpcError>;
// }

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
    pub fn call_and_serve<T, S: RpcSchema, CS>(
        self,
        service: impl FnOnce(XyRpcChannel<SF, CS>) -> T,
    ) -> ChannelBuilder<
        SF,
        CS,
        FnServiceFactory<impl FnOnce(XyRpcChannel<SF, CS>) -> RpcMsgHandlerWrapper<T>, (CS, S)>,
        MSG,
    >
    where
        CS: RpcSchema,
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
    pub fn only_serve<T, S: RpcSchema>(
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
    pub fn only_call<CS: RpcSchema>(self) -> ChannelBuilder<F, CS, MH, MSG> {
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
    #[cfg(feature = "std")]
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
        CS: RpcSchema,
        MH: ServiceFactory<SF, CS>,
    {
        let stream = new_transport_stream(read);
        let sink = new_transport_sink(write);
        self.build_from_transport(sink, stream)
    }
    pub fn build_from_transport<TSK: RpcTransportSink + Unpin, TSM: RpcTransportStream + Unpin>(
        self,
        transport_sink: TSK,
        transport_stream: TSM,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcSchema,
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

#[cfg(feature = "std")]
pub fn new_transport_stream(
    read: impl futures_util::AsyncRead + Unpin + MaybeSend + 'static,
) -> impl RpcTransportStream + Unpin {
    let buf = BytesMut::with_capacity(1024 * 8);
    let future = futures_util::stream::unfold((read, buf), move |(mut read, mut buf)| async move {
        match frame::read_frame(&mut read).await? {
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

#[cfg(feature = "std")]
pub fn new_transport_sink(
    write: impl futures_util::AsyncWrite + Unpin + MaybeSend + 'static,
) -> impl RpcTransportSink<Error = RpcError> + Unpin {
    Box::pin(futures_util::sink::unfold(
        write,
        |mut write, frame: RpcFrame| async move {
            frame::write_frame(&mut write, frame.head).await?;
            write.write_all(&frame.payload).await?;
            write.flush().await?;
            Ok(write)
        },
    ))
}

pub trait ServiceFactory<SF, CS: RpcSchema> {
    type S: RpcSchema;
    fn create(self, channel: XyRpcChannel<SF, CS>) -> impl RpcMsgHandler<Self::S> + 'static;
}

impl<SF, CS: RpcSchema> ServiceFactory<SF, CS> for () {
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

impl<F, S: RpcSchema, H, SF, CS: RpcSchema> ServiceFactory<SF, CS> for FnServiceFactory<F, (CS, S)>
where
    F: FnOnce(XyRpcChannel<SF, CS>) -> H,
    H: RpcMsgHandler<S> + 'static,
{
    type S = S;

    fn create(self, channel: XyRpcChannel<SF, CS>) -> impl RpcMsgHandler<S> + 'static {
        self.0(channel)
    }
}
