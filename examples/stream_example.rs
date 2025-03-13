use futures_util::{Stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::io::Write;
use core::pin::pin;
use tracing::info;
use xy_rpc::formats::{JsonFormat, SerdeFormat};
use xy_rpc::maybe_send::MaybeSend;
use xy_rpc::tokio::{serve_duplex_from_tokio, serve_duplex_tokio};
use xy_rpc::{RpcError, TransStream};
use xy_rpc_macro::rpc_service;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComplexObj {
    a: String,
    b: u32,
    c: bool,
    d: Vec<u32>,
    e: Vec<String>,
    f: Vec<bool>,
    g: Vec<ComplexObj>,
}

#[rpc_service]
pub trait ClientService: Send + Sync {
    async fn hello1(
        &self,
        content: ComplexObj,
        f: String,
        stream: TransStream<String, impl SerdeFormat>,
    ) -> impl Stream<Item = Result<ComplexObj, RpcError>> + MaybeSend + 'static;
}

#[rpc_service]
trait ServerService: Send + Sync {
    async fn hello2(&self, content: ComplexObj) -> ComplexObj;
    async fn msg_streaming(&self, stream: TransStream<String, impl SerdeFormat>);
    async fn msg_streaming2(&self, foo: f32, stream: TransStream<String, impl SerdeFormat>);
    async fn reply_streaming(
        &self,
    ) -> impl Stream<Item = Result<bool, RpcError>> + MaybeSend + 'static;
    async fn reply_streaming2(
        &self,
        foo: f32,
    ) -> impl Stream<Item = Result<bool, RpcError>> + MaybeSend + 'static;
}

struct TestClientService;
struct TestServerService;

impl ClientService for TestClientService {
    fn hello1(
        &self,
        mut content: ComplexObj,
        f: String,
        mut stream: TransStream<String, impl SerdeFormat>,
    ) -> impl Future<Output = impl Stream<Item = Result<ComplexObj, RpcError>> + MaybeSend + 'static>
    + MaybeSend {
        async move {
            let next = stream.next().await.unwrap();
            info!(?content, ?f, ?next, "hello1");
            content.a = next.unwrap();
            futures_util::stream::once({
                let mut content = content.clone();
                async move {
                    println!("ClientService: {:?}", content);
                    content.g.push(content.clone());
                    Ok(content)
                }
            })
            .chain(futures_util::stream::once(async move { Ok(content) }))
        }
    }
}

impl ServerService for TestServerService {
    async fn hello2(&self, mut content: ComplexObj) -> ComplexObj {
        println!("ServerService: {:?}", content);
        content.g.push(content.clone());
        content
    }

    fn msg_streaming(
        &self,
        _: TransStream<String, impl SerdeFormat>,
    ) -> impl Future<Output = ()> + MaybeSend {
        async move { todo!() }
    }

    fn msg_streaming2(
        &self,
        foo: f32,
        _: TransStream<String, impl SerdeFormat>,
    ) -> impl Future<Output = ()> + MaybeSend {
        async move { todo!() }
    }

    fn reply_streaming(
        &self,
    ) -> impl Future<Output = impl Stream<Item = Result<bool, RpcError>> + MaybeSend + 'static> + MaybeSend
    {
        async move { futures_util::stream::once(async move { todo!() }) }
    }

    fn reply_streaming2(
        &self,
        foo: f32,
    ) -> impl Future<Output = impl Stream<Item = Result<bool, RpcError>> + MaybeSend + 'static> + MaybeSend
    {
        async move { futures_util::stream::once(async move { todo!() }) }
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt()
        // Configure formatting settings.
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_level(true)
        // Set the subscriber as the default.
        .init();
    serve_duplex_tokio(
        JsonFormat,
        (
            |_| TestClientService,
            async |channel| {
                let mut obj = ComplexObj {
                    a: "A Value".to_string(),
                    b: 0,
                    c: true,
                    d: vec![1, 2, 3, 4, 5],
                    e: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                    f: vec![true, false, true],
                    g: vec![],
                };
                for i in 0..3 {
                    obj.b = i;
                    let r = channel.hello2(&obj).await?;
                    println!("hello2 reply: {:?}", r);
                }
                Ok(())
            },
        ),
        (
            |_| TestServerService,
            async |channel| {
                let mut obj = ComplexObj {
                    a: "SDF Value".to_string(),
                    b: 0,
                    c: true,
                    d: vec![1, 2, 3, 4, 5],
                    e: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                    f: vec![true, false],
                    g: vec![],
                };
                for i in 0..3 {
                    obj.b = i;
                    let r = channel
                        .hello1(
                            &obj,
                            &"SDFDS".to_string(),
                            futures_util::stream::once(async move { Ok("Hello".to_string()) }),
                        )
                        .await
                        .unwrap();
                    let mut r = pin!(r);
                    while let Some(n) = r.next().await {
                        println!("hello1 reply: {:?}", n?);
                    }
                    println!("end stream");
                }
                Ok(())
            },
        ),
    )
    .await
    .unwrap();
}
