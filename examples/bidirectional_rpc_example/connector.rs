use crate::proto::*;
use std::net::SocketAddr;
use std::time::Duration;
use xy_rpc::XyRpcChannel;
use xy_rpc::tokio::ChannelBuilderTokioExt;

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
    // task.await.unwrap();
}
