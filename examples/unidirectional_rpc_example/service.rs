use crate::proto::*;
use std::net::SocketAddr;
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
