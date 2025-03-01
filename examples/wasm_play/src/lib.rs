use wasm_bindgen::__rt::IntoJsResult;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;
use xy_rpc::{ChannelBuilder, XyRpcChannel, rpc_service};

#[rpc_service]
trait RpcTest2Service: Send + Sync {
    fn hello(&self, x: u32) -> impl Future<Output = u32> + Send;
}

struct TestService;

#[rpc_service]
trait RpcTestService: Send + Sync {
    fn a(&self, x: u32) -> impl Future<Output = u32> + Send;
    fn b(&self, x: u32) -> impl Future<Output = u32> + Send;
    fn c(&self, x: u32) -> impl Future<Output = String> + Send;
}
impl RpcTestService for TestService {
    fn a(&self, x: u32) -> impl Future<Output = u32> + Send {
        async move { x }
    }

    fn b(&self, x: u32) -> impl Future<Output = u32> + Send {
        async move { x }
    }

    fn c(&self, x: u32) -> impl Future<Output = String> + Send {
        async move { format!("{x} fdsasdf") }
    }
}

#[wasm_bindgen]
pub struct ServiceImpl {}

#[wasm_bindgen]
impl ServiceImpl {
    pub fn make_service(&self, channel: RpcTestServiceProxy) -> RpcTest2ServiceProxyImpl {
        let _ = channel;
        unreachable!()
    }
}

#[wasm_bindgen]
pub async fn xy_rpc_test(url: &str) -> Result<RpcTestServiceProxy, JsValue> {
    let (read, write) = get_http_stream_from_url(url).await?;
    let (rpc_channel, future) = ChannelBuilder::new()
        .only_call::<RpcTestServiceSchema>()
        // .serve(|rpc_channel| service.make_service(RpcTestServiceProxy { rpc_channel }))
        .build_from_web_stream(read, write);
    wasm_bindgen_futures::spawn_local(async move {
        let r = future.await;
    });
    Ok(RpcTestServiceProxy { rpc_channel })
}

#[wasm_bindgen]
pub struct RpcTest2ServiceProxyImpl {}

#[wasm_bindgen]
impl RpcTest2ServiceProxyImpl {
    pub async fn hello(&self, x: u32) -> u32 {
        todo!()
    }
}

impl RpcTest2Service for RpcTest2ServiceProxyImpl {
    fn hello(&self, x: u32) -> impl Future<Output = u32> + Send {
        self.hello(x)
    }
}

#[wasm_bindgen]
pub struct RpcTestServiceProxy {
    rpc_channel: XyRpcChannel<RpcTestServiceSchema>,
}

#[wasm_bindgen]
impl RpcTestServiceProxy {
    pub async fn a(&self, x: u32) -> u32 {
        self.rpc_channel.a(x).await
    }
    pub async fn b(&self, x: u32) -> u32 {
        self.rpc_channel.b(x).await
    }
    pub async fn c(&self, x: u32) -> String {
        self.rpc_channel.c(x).await
    }
}

#[wasm_bindgen]
pub fn test_wasm(str: String) -> String {
    format!("Hello {}", str)
}
