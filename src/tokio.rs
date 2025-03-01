use crate::formats::SerdeFormat;
use crate::{ChannelBuilder, RpcError, RpcFrameHead, RpcMsgHandler, RpcMsgHandlerWrapper, RpcServiceSchema, XyRpcChannel, new_transport_sink, new_transport_stream, ServiceFactory};
use std::future::Future;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub trait ChannelBuilderTokioExt<SF, CS, MH, MSG> {
    fn build_from_tokio(
        self,
        io: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + Send + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>;
    fn build_from_tokio_read_write<S: RpcServiceSchema>(
        self,
        io: (
            impl AsyncRead + Unpin + Send + 'static,
            impl AsyncWrite + Unpin + Send + 'static,
        ),
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + Send + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>;
}
impl<SF, CS, MH, MSG> ChannelBuilderTokioExt<SF, CS, MH, MSG> for ChannelBuilder<SF, CS, MH, MSG>
where
    SF: SerdeFormat,
{
    fn build_from_tokio(
        self,
        io: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + Send + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>,
    {
        self.build_from_tokio_read_write::<MH::S>(tokio::io::split(io))
    }
    fn build_from_tokio_read_write<S: RpcServiceSchema>(
        self,
        (read, write): (
            impl AsyncRead + Unpin + Send + 'static,
            impl AsyncWrite + Unpin + Send + 'static,
        ),
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + Send + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: ServiceFactory<SF, CS>,
    {
        let stream = new_transport_stream(read.compat());
        let sink = new_transport_sink(write.compat_write());
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

#[cfg(feature = "duplex")]
pub async fn serve_duplex<
    SF: SerdeFormat,
    T1: 'static,
    T2: 'static,
    O1: Send + 'static,
    O2: Send + 'static,
    CS1: RpcServiceSchema + Send + 'static,
    CS2: RpcServiceSchema + Send + 'static,
    F1: Future<Output = Result<O1, RpcError>> + Send + 'static,
    F2: Future<Output = Result<O2, RpcError>> + Send + 'static,
>(
    serde_format: SF,
    (serve1, f1): (
        impl FnOnce(XyRpcChannel<SF, CS2>) -> T1,
        impl FnMut(XyRpcChannel<SF, CS2>) -> F1 + Send + 'static,
    ),
    (serve2, f2): (
        impl FnOnce(XyRpcChannel<SF, CS1>) -> T2,
        impl FnMut(XyRpcChannel<SF, CS1>) -> F2 + Send + 'static,
    ),
) -> Result<(O1, O2), RpcError>
where
    RpcMsgHandlerWrapper<T1>: RpcMsgHandler<CS1>,
    RpcMsgHandlerWrapper<T2>: RpcMsgHandler<CS2>,
{
    let (duplex1, duplex2) = tokio::io::duplex(1024 * 8);
    let (read1, write1) = tokio::io::split(duplex1);
    let (read2, write2) = tokio::io::split(duplex2);
    crate::duplex::serve_duplex_fromserve_duplex_from(
        (
            (read1.compat(), write1.compat_write()),
            (read2.compat(), write2.compat_write()),
        ),
        serde_format,
        (serve1, f1),
        (serve2, f2),
    )
    .await
}