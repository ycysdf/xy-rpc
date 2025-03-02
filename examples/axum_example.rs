use futures_util::{FutureExt, StreamExt};
use std::future::Future;
use std::time::Duration;
use tokio::{select, try_join};
use xy_rpc::formats::JsonFormat;
use xy_rpc::tokio::serve_duplex_tokio;
use xy_rpc_macro::rpc_service;

#[tokio::main]
pub async fn main() {
   // tokio::spawn(async move {
   //     let listener =
   //         tokio::net::TcpListener::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 30002))
   //             .await
   //             .unwrap();
   //
   //     let (service, channel_receiver) = XyWebRpcService::new(
   //         |req, channel: XyRpcChannel<JsonFormat, ()>| TestService,
   //         JsonFormat,
   //     );
   //     tokio::spawn(async move {
   //         let mut channels = vec![];
   //         while let Ok(channel) = channel_receiver.recv_async().await {
   //             // channel.hello(12).await;
   //             println!("recv channel");
   //             channels.push(channel);
   //         }
   //     });
   //     let cors = CorsLayer::new()
   //         .allow_methods(AllowMethods::any())
   //         .allow_headers(AllowHeaders::any())
   //         .allow_origin(AllowOrigin::any())
   //         .allow_private_network(AllowPrivateNetwork::yes());
   //     axum::serve(
   //         listener,
   //         Router::new()
   //             .route("/hello", get(|| async { "Hello" }))
   //             .nest_service("/rpc_test", service)
   //             .layer(cors),
   //     )
   //     .await
   //     .unwrap();
   // })
   // .await
   // .unwrap();
}
