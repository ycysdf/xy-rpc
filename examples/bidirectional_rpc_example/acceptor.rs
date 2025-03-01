use crate::proto::*;
use std::net::SocketAddr;
use xy_rpc::XyRpcChannel;
use xy_rpc::tokio::ChannelBuilderTokioExt;

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
