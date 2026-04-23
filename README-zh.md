[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/ycysdf/xy-rpc#license)
[![Crates.io](https://img.shields.io/crates/v/xy-rpc.svg)](https://crates.io/crates/xy-rpc)
[![Docs](https://docs.rs/xy-rpc/badge.svg)](https://docs.rs/xy-rpc)

# xy-rpc

[English](./README.md)

`xy-rpc` 是一个 Rust RPC 框架，使用 trait 定义 schema，支持双向调用，并且不绑定具体传输层。

## 特性

- 传输层无关：任何实现 `AsyncRead` + `AsyncWrite` 的通道都可以接入
- 支持双向 RPC
- 不需要 `.proto` 文件，直接用 Rust trait 定义服务
- 支持任意兼容 Serde 的序列化格式
- 核心层不绑定异步运行时
- 调用 RPC 时可直接传引用参数，不要求转移所有权
- 支持非 `Send` 与单线程环境
- 在 `wasm32` 目标下提供 web 支持

## 简单示例

先在公共库中定义服务 trait：

```rust
use xy_rpc::{formats::JsonFormat, rpc_service};

pub const PORT: u16 = 30003;
pub type FormatType = JsonFormat;

#[rpc_service]
pub trait ExampleService {
    async fn hello(&self, content: String, param2: u32) -> String;
}
```

调用端：

```rust
use crate::proto::*;
use core::net::SocketAddr;
use core::time::Duration;
use xy_rpc::tokio::ChannelBuilderTokioExt;

#[path = "proto.rs"]
mod proto;

#[tokio::main]
pub async fn main() {
   let stream = tokio::net::TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], proto::PORT)))
      .await
      .unwrap();
   let (channel, future) = xy_rpc::ChannelBuilder::new(proto::FormatType::default())
      .only_call::<ExampleServiceSchema>()
      .build_from_tokio(stream);
   tokio::spawn(future);
   let replay = channel
      .hello(&"Hello. I am Caller".into(), &39)
      .await
      .unwrap();
   println!("Reply: {:?}", replay);

   tokio::time::sleep(Duration::from_secs(1)).await;
}
```

服务端：

```rust
use crate::proto::*;
use core::net::SocketAddr;
use xy_rpc::tokio::ChannelBuilderTokioExt;

#[path = "proto.rs"]
mod proto;

pub struct ExampleServiceImpl {}

impl ExampleService for ExampleServiceImpl {
    fn hello(&self, content: String, param2: u32) -> impl Future<Output = String> + Send {
        println!("receive call hello. content: {content:?}, param2: {param2}");
        async move { format!("your params: {content:?},{param2}") }
    }
}

#[tokio::main]
pub async fn main() {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], proto::PORT)))
        .await
        .unwrap();
    let (stream, addr) = listener.accept().await.unwrap();
    println!("Accept connection from {:?}", addr);
    let (_channel, future) = xy_rpc::ChannelBuilder::new(proto::FormatType::default())
        .only_serve(|_channel| ExampleServiceImpl {})
        .build_from_tokio(stream);
    future.await.unwrap();
}
```

## 双向 RPC 示例

先定义双方的服务 trait：

```rust
use xy_rpc::{formats::JsonFormat, rpc_service};

pub const PORT: u16 = 30002;
pub type FormatType = JsonFormat;

#[rpc_service]
pub trait ClientService {
   async fn say(&self, content: String) -> String;

   async fn repetition(&self, content: String);
}

#[rpc_service]
pub trait ServerService {
   async fn say(&self, content: String) -> String;
}
```

连接端：

```rust
use crate::proto::*;
use core::net::SocketAddr;
use core::time::Duration;
use xy_rpc::tokio::ChannelBuilderTokioExt;
use xy_rpc::XyRpcChannel;

#[path = "proto.rs"]
mod proto;

pub struct ClientServiceImpl {
   channel: XyRpcChannel<proto::FormatType, ServerServiceSchema>,
}

impl ClientService for ClientServiceImpl {
   async fn say(&self, content: String) -> String {
      println!("Server said to me: {:?}", content);
      format!("You say: {:?}", content)
   }

   async fn repetition(&self, content: String) {
      println!("Server wants me to repeat this {:?}", content);
      self.channel.say(&content).await.unwrap();
   }
}

#[tokio::main]
pub async fn main() {
   let stream = tokio::net::TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], proto::PORT)))
      .await
      .unwrap();
   let (channel, future) = xy_rpc::ChannelBuilder::new(proto::FormatType::default())
      .call_and_serve(|channel| ClientServiceImpl { channel })
      .build_from_tokio(stream);
   tokio::spawn(future);
   let say_content: String = "Hello. I am Client".into();
   let replay = channel.say(&say_content).await.unwrap();
   println!("Server reply: {:?}", replay);

   tokio::time::sleep(Duration::from_secs(1)).await;
}
```

接收端：

```rust
use crate::proto::*;
use core::net::SocketAddr;
use xy_rpc::tokio::ChannelBuilderTokioExt;
use xy_rpc::XyRpcChannel;

#[path = "proto.rs"]
mod proto;

pub struct ServerServiceImpl {
   _channel: XyRpcChannel<proto::FormatType, ClientServiceSchema>,
}

impl ServerService for ServerServiceImpl {
   async fn say(&self, content: String) -> String {
      println!("Client said to me: {:?}", content);
      format!("You say: {:?}", content)
   }
}

#[tokio::main]
pub async fn main() {
   let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], proto::PORT)))
      .await
      .unwrap();
   let (stream, addr) = listener.accept().await.unwrap();
   println!("Accept connection from {:?}", addr);
   let (channel, future) = xy_rpc::ChannelBuilder::new(proto::FormatType::default())
      .call_and_serve(|channel| ServerServiceImpl { _channel: channel })
      .build_from_tokio(stream);
   let task = tokio::spawn(future);
   let say_reply = channel.say(&"Hello. I am Server".into()).await.unwrap();
   println!("Client reply: {:?}", say_reply);
   channel
      .repetition(&"Repeat the content".into())
      .await
      .unwrap();

   let _ = task.await.unwrap();
}
```

## License

MIT License ([LICENSE-MIT](https://github.com/ycysdf/xy-rpc/blob/main/LICENSE-MIT))
