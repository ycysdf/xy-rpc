use crate::formats::SerdeFormat;
use crate::maybe_send::MaybeSend;
use crate::{
    ChannelBuilder, RpcError, RpcMsgHandler, RpcMsgHandlerWrapper, RpcServiceSchema, XyRpcChannel,
};
use futures_util::future::Either;

pub async fn serve_duplex_from<
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
            impl futures_util::AsyncRead + Unpin + MaybeSend + 'static,
            impl futures_util::AsyncWrite + Unpin + MaybeSend + 'static,
        ),
        (
            impl futures_util::AsyncRead + Unpin + MaybeSend + 'static,
            impl futures_util::AsyncWrite + Unpin + MaybeSend + 'static,
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
        .build_from_read_write(duplex1);
    let (channel2, serve_future_2) = json_channel_builder
        .call_and_serve(serve2)
        .build_from_read_write(duplex2);

    let r = match futures_util::future::select(
        std::pin::pin!({
            let channel = channel.clone();
            let channel2 = channel2.clone();
            async move { futures_util::try_join!(f1(channel), f2(channel2)) }
        }),
        std::pin::pin!(async move { futures_util::try_join!(serve_future_1, serve_future_2) }),
    )
    .await
    {
        Either::Left((f_end, r_future)) => {
            // println!("f end");
            drop(channel);
            drop(channel2);
            r_future.await?;
            f_end?
        }
        Either::Right((r_end, f_future)) => {
            f_future.await?;
            // println!("r end");
            drop(channel);
            drop(channel2);
            r_end?;
            return Err(RpcError::ServeExceptionEnd);
        }
    };
    Ok(r)
}
