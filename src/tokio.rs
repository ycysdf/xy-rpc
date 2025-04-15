use crate::formats::SerdeFormat;
use crate::frame::{RpcFrameHead, RpcFrameHeadBits};
use crate::maybe_send::MaybeSend;
use crate::{
    ChannelBuilder, RpcError, RpcMsgHandler, RpcMsgHandlerWrapper, RpcSchema, ServiceFactory,
    XyRpcChannel, new_transport_sink, new_transport_stream,
};
use alloc::format;
use core::future::Future;
use futures_util::future::Either;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub trait ChannelBuilderTokioExt<SF, CS, MH, MSG> {
    fn build_from_tokio(
        self,
        io: impl AsyncRead + AsyncWrite + Unpin + MaybeSend + 'static,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcSchema,
        MH: ServiceFactory<SF, CS>;
    fn build_from_tokio_read_write(
        self,
        io: (
            impl AsyncRead + Unpin + MaybeSend + 'static,
            impl AsyncWrite + Unpin + MaybeSend + 'static,
        ),
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcSchema,
        MH: ServiceFactory<SF, CS>;
}
impl<SF, CS, MH, MSG> ChannelBuilderTokioExt<SF, CS, MH, MSG> for ChannelBuilder<SF, CS, MH, MSG>
where
    SF: SerdeFormat,
{
    fn build_from_tokio(
        self,
        io: impl AsyncRead + AsyncWrite + Unpin + MaybeSend + 'static,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcSchema,
        MH: ServiceFactory<SF, CS>,
    {
        self.build_from_tokio_read_write(tokio::io::split(io))
    }
    fn build_from_tokio_read_write(
        self,
        (read, write): (
            impl AsyncRead + Unpin + MaybeSend + 'static,
            impl AsyncWrite + Unpin + MaybeSend + 'static,
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
        let stream = new_transport_stream(read.compat());
        let sink = new_transport_sink(write.compat_write());
        self.build_from_transport(sink, stream)
    }
}

pub async fn write_frame(
    mut write: impl AsyncWrite + Unpin,
    frame: RpcFrameHead,
) -> Result<(), RpcError> {
    let bits: RpcFrameHeadBits = frame.into();
    write.write_all(bits.as_slice()).await?;
    Ok(())
}

pub async fn read_frame(mut read: impl AsyncRead + Unpin) -> Result<RpcFrameHead, RpcError> {
    let mut bits: RpcFrameHeadBits = [0; 8];
    read.read_exact(&mut bits).await?;
    Ok(bits.into())
}

#[cfg(feature = "duplex")]
pub async fn serve_duplex_tokio<
    SF: SerdeFormat,
    T1: 'static,
    T2: 'static,
    O1: MaybeSend + 'static,
    O2: MaybeSend + 'static,
    CS1: RpcSchema + MaybeSend + 'static,
    CS2: RpcSchema + MaybeSend + 'static,
    F1: Future<Output = Result<O1, RpcError>> + MaybeSend + 'static,
    F2: Future<Output = Result<O2, RpcError>> + MaybeSend + 'static,
>(
    serde_format: SF,
    (serve1, f1): (
        impl FnOnce(XyRpcChannel<SF, CS2>) -> T1,
        impl FnMut(XyRpcChannel<SF, CS2>) -> F1 + MaybeSend + 'static,
    ),
    (serve2, f2): (
        impl FnOnce(XyRpcChannel<SF, CS1>) -> T2,
        impl FnMut(XyRpcChannel<SF, CS1>) -> F2 + MaybeSend + 'static,
    ),
) -> Result<(O1, O2), RpcError>
where
    RpcMsgHandlerWrapper<T1>: RpcMsgHandler<CS1>,
    RpcMsgHandlerWrapper<T2>: RpcMsgHandler<CS2>,
{
    let (duplex1, duplex2) = tokio::io::duplex(usize::MAX);
    serve_duplex_from_tokio(
        (tokio::io::split(duplex1), tokio::io::split(duplex2)),
        serde_format,
        (serve1, f1),
        (serve2, f2),
    )
    .await
}

pub async fn serve_duplex_from_tokio<
    SF: SerdeFormat,
    T1: 'static,
    T2: 'static,
    O1: MaybeSend + 'static,
    O2: MaybeSend + 'static,
    CS1: RpcSchema + MaybeSend + 'static,
    CS2: RpcSchema + MaybeSend + 'static,
    F1: Future<Output = Result<O1, RpcError>> + MaybeSend + 'static,
    F2: Future<Output = Result<O2, RpcError>> + MaybeSend + 'static,
>(
    (duplex1, duplex2): (
        (
            impl AsyncRead + Unpin + MaybeSend + 'static,
            impl AsyncWrite + Unpin + MaybeSend + 'static,
        ),
        (
            impl AsyncRead + Unpin + MaybeSend + 'static,
            impl AsyncWrite + Unpin + MaybeSend + 'static,
        ),
    ),
    serde_format: SF,
    (serve1, mut f1): (
        impl FnOnce(XyRpcChannel<SF, CS2>) -> T1,
        impl FnMut(XyRpcChannel<SF, CS2>) -> F1 + MaybeSend + 'static,
    ),
    (serve2, mut f2): (
        impl FnOnce(XyRpcChannel<SF, CS1>) -> T2,
        impl FnMut(XyRpcChannel<SF, CS1>) -> F2 + MaybeSend + 'static,
    ),
) -> Result<(O1, O2), RpcError>
where
    RpcMsgHandlerWrapper<T1>: RpcMsgHandler<CS1>,
    RpcMsgHandlerWrapper<T2>: RpcMsgHandler<CS2>,
{
    let channel_builder = ChannelBuilder::new(serde_format);

    let (channel, serve_future_1) = channel_builder
        .clone()
        .call_and_serve(serve1)
        .build_from_tokio_read_write(duplex1);
    let (channel2, serve_future_2) = channel_builder
        .call_and_serve(serve2)
        .build_from_tokio_read_write(duplex2);

    let r = match futures_util::future::select(
        core::pin::pin!({
            let channel = channel.clone();
            let channel2 = channel2.clone();
            let future = async move { futures_util::try_join!(f1(channel), f2(channel2)) };
            #[cfg(feature = "send_sync")]
            {
                tokio::spawn(future)
            }
            #[cfg(not(feature = "send_sync"))]
            {
                future
            }
        }),
        core::pin::pin!({
            let future = async move { futures_util::try_join!(serve_future_1, serve_future_2) };
            #[cfg(feature = "send_sync")]
            {
                tokio::spawn(future)
            }
            #[cfg(not(feature = "send_sync"))]
            {
                future
            }
        }),
    )
    .await
    {
        Either::Left((f_end, r_future)) => {
            #[cfg(feature = "send_sync")]
            let f_end = f_end.map_err(|err| RpcError::OtherError {
                message: format!("panic: {err:?}"),
            })?;
            // println!("f end");
            drop(channel);
            drop(channel2);
            #[cfg(feature = "send_sync")]
            r_future.await.map_err(|err| RpcError::OtherError {
                message: format!("panic: {err:?}"),
            })??;
            #[cfg(not(feature = "send_sync"))]
            r_future.await?;
            f_end?
        }
        Either::Right((r_end, f_future)) => {
            #[cfg(feature = "send_sync")]
            f_future.await.map_err(|err| RpcError::OtherError {
                message: format!("panic: {err:?}"),
            })??;
            #[cfg(not(feature = "send_sync"))]
            f_future.await?;
            // println!("r end");
            drop(channel);
            drop(channel2);
            #[cfg(feature = "send_sync")]
            r_end.map_err(|err| RpcError::OtherError {
                message: format!("panic: {err:?}"),
            })??;
            #[cfg(not(feature = "send_sync"))]
            r_end?;
            return Err(RpcError::ServeExceptionEnd);
        }
    };
    Ok(r)
}
