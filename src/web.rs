use crate::{ChannelBuilder, RpcError, RpcMsgHandler, RpcServiceSchema, XyRpcChannel};
use futures_util::SinkExt;
use gloo_net::Error;
use js_sys::Uint8Array;
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};
use wasm_bindgen::JsValue;
use web_sys::{ReadableStream, WritableStream};

pub async fn get_http_stream_from_url(
    url: &str,
) -> Result<(ReadableStream, WritableStream), JsValue> {
    let (sender, receiver) = flume::unbounded::<JsValue>();
    let data_stream_url = format!("{}/data_stream", url);
    match gloo_net::http::Request::get(data_stream_url.as_str())
        .send()
        .await
    {
        Ok(response) => {
            let stream_id = "526fb8c9-b4af-45f1-8ca4-806d09602676".to_string();
            {
                let write_data_url = format!("{}/write_data", url);
                wasm_bindgen_futures::spawn_local(async move {
                    while let Ok(value) = receiver.recv_async().await {
                        let bytes: Uint8Array = value.into();
                        let response = gloo_net::http::Request::post(write_data_url.as_str())
                            .header("stream_id", stream_id.as_str())
                            .body(bytes)
                            .unwrap()
                            .send()
                            .await
                            .unwrap();
                        assert_eq!(response.ok(), true);
                    }
                });
            }
            let read = response
                .body()
                .ok_or_else(|| JsValue::from_str("response body is null"))?;
            Ok((
                read,
                wasm_streams::WritableStream::from(sender.into_sink().sink_map_err(|err| err.0))
                    .into_raw(),
            ))
        }
        Err(err) => {
            match err {
                Error::JsError(err) => Err(JsValue::from_str(err.to_string().as_str())),
                // Error::SerdeError(err) => {
                //     Err(JsValue::from_str(format!("serde err: {err:?}").as_str()))
                // }
                Error::GlooError(err) => Err(JsValue::from_str(err.as_str())),
            }
        }
    }
}

pub trait ChannelBuilderWebExt<SF, CS, MH, MSG> {
    fn build_from_web_stream<S: RpcServiceSchema, H: RpcMsgHandler<S> + 'static>(
        self,
        readable_stream: ReadableStream,
        writable_stream: WritableStream,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + Send + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: FnOnce(XyRpcChannel<SF, CS>) -> H;
}
impl<SF, CS, MH, MSG> ChannelBuilderWebExt<SF, CS, MH, MSG> for ChannelBuilder<SF, CS, MH, MSG> {
    fn build_from_web_stream<S: RpcServiceSchema, H: RpcMsgHandler<S> + 'static>(
        self,
        readable_stream: ReadableStream,
        writable_stream: WritableStream,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + Send + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcServiceSchema,
        MH: FnOnce(XyRpcChannel<SF, CS>) -> H,
    {
        let read = wasm_streams::ReadableStream::from_raw(readable_stream).into_async_read();
        let write = wasm_streams::WritableStream::from_raw(writable_stream).into_async_write();
        self.build_from_read_write((ForceSend(read), ForceSend(write)))
    }
}

#[pin_project]
pub struct ForceSend<T>(#[pin] T);

unsafe impl<T> Send for ForceSend<T> {}

impl<T: futures_util::AsyncRead> futures_util::AsyncRead for ForceSend<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        self.project().0.poll_read(cx, buf)
    }
}

impl<T: futures_util::AsyncWrite> futures_util::AsyncWrite for ForceSend<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.project().0.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.project().0.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.project().0.poll_close(cx)
    }
}
