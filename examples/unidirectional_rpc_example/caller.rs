use crate::proto::*;
use std::net::SocketAddr;
use std::time::Duration;
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
