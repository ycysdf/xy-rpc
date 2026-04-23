use crate::formats::SerdeFormat;
use crate::maybe_send::MaybeSend;
use crate::{ChannelBuilder, RpcError, RpcSchema, ServiceFactory, XyRpcChannel};
use alloc::format;
use alloc::string::{String, ToString};
use futures_util::{SinkExt, Stream};
use gloo_net::Error;
use js_sys::Uint8Array;
use std::sync::{Arc, Mutex};
use wasm_bindgen::__rt::IntoJsResult;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use web_sys::{ReadableStream, WritableStream};

pub async fn get_http_stream_from_url(
    url: &str,
) -> Result<(ReadableStream, WritableStream), JsValue> {
    let (sender, receiver) = flume::unbounded::<JsValue>();
    let stream_id: String = uuid::Uuid::new_v4().to_string();
    let data_stream_url = format!("{url}/data_stream?stream_id={stream_id}");
    match gloo_net::http::Request::get(data_stream_url.as_str())
        .send()
        .await
    {
        Ok(response) => {
            {
                let write_data_url = format!("{url}/write_data?stream_id={stream_id}");
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
    fn build_from_web_stream(
        self,
        readable_stream: ReadableStream,
        writable_stream: WritableStream,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcSchema,
        MH: ServiceFactory<SF, CS>;
}
impl<SF, CS, MH, MSG> ChannelBuilderWebExt<SF, CS, MH, MSG> for ChannelBuilder<SF, CS, MH, MSG>
where
    SF: SerdeFormat,
{
    fn build_from_web_stream(
        self,
        readable_stream: ReadableStream,
        writable_stream: WritableStream,
    ) -> (
        XyRpcChannel<SF, CS>,
        impl Future<Output = Result<(), RpcError>> + MaybeSend + 'static,
    )
    where
        XyRpcChannel<SF, CS>: Clone,
        CS: RpcSchema,
        MH: ServiceFactory<SF, CS>,
    {
        let read = wasm_streams::ReadableStream::from_raw(readable_stream).into_async_read();
        let write = wasm_streams::WritableStream::from_raw(writable_stream).into_async_write();
        self.build_from_read_write((read, write))
    }
}

pub fn stream_to_js_async_iterator(
    stream: impl Stream<Item: Into<JsValue>> + Unpin + 'static,
) -> JsValue {
    use futures_util::StreamExt;
    let symbol = js_sys::Symbol::async_iterator();
    let object = js_sys::Object::new();
    let stream = Arc::new(Mutex::new(stream));
    let ff = Closure::<dyn Fn() -> js_sys::Promise>::new(move || {
        let stream = stream.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let mut stream = stream.lock().unwrap();
            let object = js_sys::Object::new();
            if let Some(n) = stream.next().await {
                js_sys::Reflect::set(&object, &"value".into(), &n.into())?;
                js_sys::Reflect::set(&object, &"done".into(), &false.into())?;
            } else {
                js_sys::Reflect::set(&object, &"done".into(), &true.into())?;
            }
            Ok(object.into_js_result()?)
        })
    })
    .into_js_value();
    let rr = Closure::<dyn Fn() -> JsValue>::new(move || {
        let object = js_sys::Object::new();
        js_sys::Reflect::set(&object, &"next".into(), &ff).unwrap();
        object.into_js_result().unwrap()
    })
    .into_js_value();
    js_sys::Reflect::set(&object, &symbol, &rr).unwrap();
    object.into_js_result().unwrap()
}

pub fn try_stream_to_js_async_iterator<
    T: Into<JsValue> + 'static,
    E: std::error::Error + 'static,
>(
    stream: impl Stream<Item = Result<T, E>> + 'static,
) -> Result<JsValue, JsValue> {
    use futures_util::StreamExt;
    let symbol = js_sys::Symbol::async_iterator();
    let object = js_sys::Object::new();
    let stream = Arc::new(Mutex::new(stream.boxed_local()));
    let ff = Closure::<dyn Fn() -> js_sys::Promise>::new(move || {
        let stream = stream.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let mut stream = stream.lock().unwrap();
            let object = js_sys::Object::new();
            if let Some(n) = stream.next().await {
                let n = n.map_err(|err| wasm_bindgen::JsValue::from(err.to_string()))?;
                let n = n.into();
                js_sys::Reflect::set(&object, &"value".into(), &n)?;
                js_sys::Reflect::set(&object, &"done".into(), &false.into())?;
            } else {
                js_sys::Reflect::set(&object, &"done".into(), &true.into())?;
            }
            Ok(object.into_js_result()?)
        })
    })
    .into_js_value();
    let rr = Closure::<dyn Fn() -> JsValue>::new(move || {
        let object = js_sys::Object::new();
        js_sys::Reflect::set(&object, &"next".into(), &ff).unwrap();
        object.into_js_result().unwrap()
    })
    .into_js_value();
    js_sys::Reflect::set(&object, &symbol, &rr)?;
    object.into_js_result()
}
