use crate::formats::SerdeFormat;
use crate::{
    ChannelBuilder, RpcError, RpcMsgHandler, RpcMsgHandlerWrapper, RpcServiceSchema, XyRpcChannel,
};
use futures_util::future::Either;

pub async fn serve_duplex_from<
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
    (duplex1, duplex2): (
        (
            impl futures_util::AsyncRead + Unpin + Send + 'static,
            impl futures_util::AsyncWrite + Unpin + Send + 'static,
        ),
        (
            impl futures_util::AsyncRead + Unpin + Send + 'static,
            impl futures_util::AsyncWrite + Unpin + Send + 'static,
        ),
    ),
    serde_format: SF,
    (serve1, mut f1): (
        impl FnOnce(XyRpcChannel<SF, CS2>) -> T1,
        impl FnMut(XyRpcChannel<SF, CS2>) -> F1 + Send + 'static,
    ),
    (serve2, mut f2): (
        impl FnOnce(XyRpcChannel<SF, CS1>) -> T2,
        impl FnMut(XyRpcChannel<SF, CS1>) -> F2 + Send + 'static,
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
    let mut f1_future = {
        let channel = channel.clone();
        async move {
            let o1 = f1(channel).await?;
            Ok::<_, RpcError>(o1)
        }
    };
    let (channel2, serve_future_2) = json_channel_builder
        .call_and_serve(serve2)
        .build_from_read_write(duplex2);

    let r = match futures_util::future::select(
        std::pin::pin!({
            let channel2 = channel2.clone();
            async move { futures_util::try_join!(f1_future, f2(channel2)) }
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
            println!("r end");
            drop(channel);
            drop(channel2);
            r_end?;
            return Err(RpcError::ServeExceptionEnd);
        }
    };
    Ok(r)
}
