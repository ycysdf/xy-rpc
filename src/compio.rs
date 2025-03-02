use crate::formats::SerdeFormat;
use crate::maybe_send::MaybeSend;
use crate::{
    ChannelBuilder, RpcError, RpcFrameHead, RpcMsgHandler, RpcMsgHandlerWrapper, RpcServiceSchema,
    ServiceFactory, XyRpcChannel, new_transport_sink, new_transport_stream,
};
use compio::io::compat::AsyncStream;
use compio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use futures_util::future::Either;
use std::future::Future;

pub trait ChannelBuilderCompioExt<SF, CS, MH, MSG> {
    fn build_from_compio(
        self,
        io: impl AsyncRead + AsyncWrite + Unpin + 'static,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>;
    fn build_from_compio_read_write(
        self,
        io: (
            impl AsyncRead + Unpin + 'static,
            impl AsyncWrite + Unpin + 'static,
        ),
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>;
}
impl<SF, CS, MH, MSG> ChannelBuilderCompioExt<SF, CS, MH, MSG> for ChannelBuilder<SF, CS, MH, MSG>
where
    SF: SerdeFormat,
{
    fn build_from_compio(
        self,
        io: impl AsyncRead + AsyncWrite + Unpin + 'static,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>,
    {
        self.build_from_compio_read_write(compio::io::split(io))
    }
    fn build_from_compio_read_write(
        self,
        (read, write): (
            impl AsyncRead + Unpin + 'static,
            impl AsyncWrite + Unpin + 'static,
        ),
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>,
    {
        let read = AsyncStream::new(read);
        let write = AsyncStream::new(write);
        let stream = new_transport_stream(read);
        let sink = new_transport_sink(write);
        self.build_from_transport(sink, stream)
    }
}

pub async fn write_frame(
    mut write: impl AsyncWrite + Unpin,
    frame: &RpcFrameHead,
) -> Result<(), RpcError> {
    let bits: u64 = frame.into();
    write.write_u64(bits).await?;
    Ok(())
}

pub async fn read_frame(mut read: impl AsyncRead + Unpin) -> Result<RpcFrameHead, RpcError> {
    let bits = read.read_u64().await?;
    bits.try_into()
}

// #[cfg(feature = "duplex")]
// pub async fn serve_duplex_compio<
//     SF: SerdeFormat,
//     T1: 'static,
//     T2: 'static,
//     O1: 'static,
//     O2: 'static,
//     CS1: RpcServiceSchema + 'static,
//     CS2: RpcServiceSchema + 'static,
//     F1: Future<Output = Result<O1, RpcError>> + 'static,
//     F2: Future<Output = Result<O2, RpcError>> + 'static,
// >(
//     serde_format: SF,
//     (serve1, f1): (
//         impl FnOnce(XyRpcChannel<SF, CS2>) -> T1,
//         impl FnMut(XyRpcChannel<SF, CS2>) -> F1 + 'static,
//     ),
//     (serve2, f2): (
//         impl FnOnce(XyRpcChannel<SF, CS1>) -> T2,
//         impl FnMut(XyRpcChannel<SF, CS1>) -> F2 + 'static,
//     ),
// ) -> Result<(O1, O2), RpcError>
// where
//     RpcMsgHandlerWrapper<T1>: RpcMsgHandler<CS1>,
//     RpcMsgHandlerWrapper<T2>: RpcMsgHandler<CS2>,
// {
//     let (sender,receiver) = flume::unbounded();
//     let read = receiver.into_stream().into_async_read();
//     sender.into_sink().i
//
//     let (duplex1, duplex2) = tokio::io::duplex(1024 * 8);
//     let (read1, write1) = tokio::io::split(duplex1);
//     let (read2, write2) = tokio::io::split(duplex2);
//     crate::duplex::serve_duplex_from_compio(
//         (
//             (AsyncStream::new(read1), AsyncStream::new(write1)),
//             (AsyncStream::new(read2), AsyncStream::new(write2)),
//         ),
//         serde_format,
//         (serve1, f1),
//         (serve2, f2),
//     )
//     .await
// }

pub async fn serve_duplex_from_compio<
    SF: SerdeFormat,
    T1: 'static,
    T2: 'static,
    O1: MaybeSend + 'static,
    O2: MaybeSend + 'static,
    CS1: RpcServiceSchema + MaybeSend + 'static,
    CS2: RpcServiceSchema + MaybeSend + 'static,
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
    let json_channel_builder = ChannelBuilder::new(serde_format);

    let (channel, serve_future_1) = json_channel_builder
        .clone()
        .call_and_serve(serve1)
        .build_from_compio_read_write(duplex1);
    let (channel2, serve_future_2) = json_channel_builder
        .call_and_serve(serve2)
        .build_from_compio_read_write(duplex2);

    let r = match futures_util::future::select(
        {
            let channel = channel.clone();
            let channel2 = channel2.clone();
            compio::runtime::spawn(
                async move { futures_util::try_join!(f1(channel), f2(channel2)) },
            )
        },
        compio::runtime::spawn(async move {
            futures_util::try_join!(serve_future_1, serve_future_2)
        }),
    )
    .await
    {
        Either::Left((f_end, r_future)) => {
            let f_end = f_end.map_err(|err| RpcError::OtherError {
                message: format!("panic: {err:?}"),
            })?;
            // println!("f end");
            drop(channel);
            drop(channel2);
            r_future.await.map_err(|err| RpcError::OtherError {
                message: format!("panic: {err:?}"),
            })??;
            f_end?
        }
        Either::Right((r_end, f_future)) => {
            f_future.await.map_err(|err| RpcError::OtherError {
                message: format!("panic: {err:?}"),
            })??;
            // println!("r end");
            drop(channel);
            drop(channel2);
            r_end.map_err(|err| RpcError::OtherError {
                message: format!("panic: {err:?}"),
            })??;
            return Err(RpcError::ServeExceptionEnd);
        }
    };
    Ok(r)
}
