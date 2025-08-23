use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::backtrace::{Backtrace, BacktraceStatus};
use std::collections::HashMap;
use std::panic::{set_hook, take_hook};
use std::sync::{Arc, Mutex};
use xy_rpc::maybe_send::{MaybeSend, MaybeSync};
use xy_rpc::{ChannelBuilder, RpcError, TransStream, formats::SerdeFormat, rpc_service};

#[rpc_service]
pub trait RpcTest2Service: MaybeSend + MaybeSync {
    fn hello(&self, x: u32) -> impl Future<Output = u32> + MaybeSend;
}

pub struct TestService;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[cfg_attr(
    target_arch = "wasm32",
    derive(tsify::Tsify),
    tsify(into_wasm_abi, from_wasm_abi)
)]
enum TestEnum {
    A(u32),
    B(f32),
    C(String),
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[cfg_attr(
    target_arch = "wasm32",
    derive(tsify::Tsify),
    tsify(into_wasm_abi, from_wasm_abi)
)]
struct TestStruct {
    pub name: String,
    pub string: String,
    pub number: u32,
    pub bool: bool,
    pub float: f32,
    pub array_1: Vec<u32>,
    pub array_2: Vec<String>,
    pub map: HashMap<String, u32>,
    pub optional: Option<String>,
    pub tuples: (u32, u32),
    pub nest: Vec<TestStruct>,
    pub variant: Option<TestEnum>,
    pub index: u64,
}

impl TestStruct {
    pub fn test_data(name: impl Into<String>) -> Self {
        let mut test_struct = Self {
            name: name.into(),
            string: "String Value".into(),
            number: 39,
            bool: true,
            float: 3.9,
            array_1: vec![1, 2, 3, 4, 5, 6, 8, 9],
            array_2: vec!["Element 0".into(), "Element 1".into(), "Element 3".into()],
            map: {
                let mut map = HashMap::default();
                map.insert("A".into(), 1);
                map.insert("B".into(), 2);
                map.insert("C".into(), 3);
                map
            },
            optional: Some("Value".into()),
            tuples: (3, 9),
            nest: vec![],
            variant: Some(TestEnum::C("String Value".into())),
            index: 0,
        };
        let obj = test_struct.clone();
        test_struct.nest.push(obj.clone());
        test_struct.nest.push(obj.clone());
        test_struct.nest[0].nest.push(obj.clone());
        test_struct
    }
}

#[rpc_service]
pub trait RpcTestService: MaybeSend + MaybeSync {
    async fn msg_streaming(&self, stream: TransStream<String, impl SerdeFormat>);
    async fn reply_streaming(
        &self,
    ) -> impl Stream<Item = Result<u64, RpcError>> + MaybeSend + 'static;
    async fn test2(
        &self,
        stream: TransStream<String, impl SerdeFormat>,
    ) -> impl Stream<Item = Result<TestStruct, RpcError>> + MaybeSend + 'static;
    fn a(&self, x: u32) -> impl Future<Output = u32> + MaybeSend;
    fn b(&self, x: u32) -> impl Future<Output = u32> + MaybeSend;
    fn c(&self, x: u32) -> impl Future<Output = String> + MaybeSend;
}

impl RpcTestService for TestService {
    async fn msg_streaming(&self, mut stream: TransStream<String, impl SerdeFormat>) {
        while let Some(item) = stream.next().await {
            println!("msg_streaming item: {:?}", item);
        }
    }
    async fn reply_streaming(
        &self,
    ) -> impl Stream<Item = Result<u64, RpcError>> + MaybeSend + 'static {
        futures_util::stream::iter(vec![Ok(0), Ok(1), Ok(2), Ok(3), Ok(4)])
    }

    async fn test2(
        &self,
        stream: TransStream<String, impl SerdeFormat>,
    ) -> impl Stream<Item = Result<TestStruct, RpcError>> + MaybeSend + 'static {
        futures_util::stream::iter(vec![
            Ok(TestStruct::test_data("A")),
            Ok(TestStruct::test_data("B")),
            Ok(TestStruct::test_data("C")),
            Ok(TestStruct::test_data("D")),
        ])
    }

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

// unchecked_return_type
// #[wasm_bindgen(unchecked_param_type = "AsyncIterator<any>")] str: js_sys::AsyncIterator,
// return_call: wasm_bindgen::closure::Closure<dyn FnMut(JsValue)>
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn test_wasm() -> wasm_bindgen::JsValue {
    // (|next: JsValue| {
    //     // web_sys::window().unwrap().confirm_with_message("XXX");
    //     let fun = next.dyn_into::<Function>().unwrap();
    //     fun.call0(&JsValue::UNDEFINED).unwrap();
    // })
    // .into_js_function()
    xy_rpc::stream_to_js_async_iterator(futures_util::stream::iter(vec![1, 2, 3]))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn debug() {
    let hook = take_hook();
    set_hook(Box::new(move |panic_info| {
        let payload = panic_info.payload();
        #[allow(clippy::manual_map)]
        let payload = if let Some(s) = payload.downcast_ref::<&str>() {
            Some(&**s)
        } else if let Some(s) = payload.downcast_ref::<String>() {
            Some(s.as_str())
        } else {
            None
        };

        let location = panic_info.location().map(|l| l.to_string());
        let backtrace = Backtrace::capture();
        let note = (backtrace.status() == BacktraceStatus::Disabled)
            .then_some("run with RUST_BACKTRACE=1 environment variable to display a backtrace");

        web_sys::console::log_1(&format!("panic occurred: \r\n\t{note:?}\r\n\tpayload: {payload:?}\r\n\tlocation:{location:?}\r\n\t{backtrace:?}").into());
        hook(panic_info)
    }));
}

fn ff() {
    // web_sys::console::log_2()
    // let value = wasm_bindgen::JsValue::null();
    // value.into_serde()
}
