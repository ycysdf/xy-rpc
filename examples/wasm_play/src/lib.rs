use futures_util::Stream;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::wasm_bindgen;
use xy_rpc::maybe_send::{MaybeSend, MaybeSync};
use xy_rpc::{ChannelBuilder, TransStream, formats::SerdeFormat, rpc_service, RpcError};

#[rpc_service]
pub trait RpcTest2Service: MaybeSend + MaybeSync {
    fn hello(&self, x: u32) -> impl Future<Output = u32> + MaybeSend;
}

pub struct TestService;

#[rpc_service]
pub trait RpcTestService: MaybeSend + MaybeSync {
    async fn msg_streaming(&self, stream: TransStream<String, impl SerdeFormat>);
    fn a(&self, x: u32) -> impl Future<Output = u32> + MaybeSend;
    fn b(&self, x: u32) -> impl Future<Output = u32> + MaybeSend;
    fn c(&self, x: u32) -> impl Future<Output = String> + MaybeSend;
}

impl RpcTestService for TestService {
    async fn msg_streaming(&self, stream: TransStream<String, impl SerdeFormat>) {}
    fn a(&self, x: u32) -> impl Future<Output = u32> + MaybeSend {
        async move { x }
    }

    fn b(&self, x: u32) -> impl Future<Output = u32> + MaybeSend {
        async move { x }
    }

    fn c(&self, x: u32) -> impl Future<Output = String> + MaybeSend {
        // futures_util::stream::unfold(str,move || async move {
        //     let promise = str.next().unwrap();
        //     let value = wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
        //     let result = value.into_serde::<bool>().map_err(|err| RpcError::SerdeError(Box::new(err)))?;
        //     Ok(result)
        // });

        async move { format!("{x} fdsasdf") }
    }
}

#[wasm_bindgen]
pub fn test_wasm(
    #[wasm_bindgen(unchecked_param_type = "AsyncIterator<any>")] str: js_sys::AsyncIterator,
) -> String {
    format!("Hello")
}

