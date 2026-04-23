use axum::Router;
use axum::routing::get;
use core::future::Future;
use futures_util::{FutureExt, StreamExt};
use std::net::{Ipv4Addr, SocketAddr};
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, AllowPrivateNetwork, CorsLayer};
use xy_rpc::XyRpcChannel;
use xy_rpc::axum::XyWebRpcService;
use xy_rpc::formats::JsonFormat;

#[path = "wasm_play/src/lib.rs"]
mod proto;

#[tokio::main]
pub async fn main() {
    tokio::spawn(async move {
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 30002))
                .await
                .unwrap();

        let (service, channel_receiver) = XyWebRpcService::new(
            |_req, _channel: XyRpcChannel<JsonFormat, ()>| proto::TestService,
            JsonFormat,
        );
        tokio::spawn(async move {
            let mut channels = vec![];
            while let Ok(channel) = channel_receiver.recv_async().await {
                println!("recv channel");
                channels.push(channel);
            }
        });
        let cors = CorsLayer::new()
            .allow_methods(AllowMethods::any())
            .allow_headers(AllowHeaders::any())
            .allow_origin(AllowOrigin::any())
            .allow_private_network(AllowPrivateNetwork::yes());
        axum::serve(
            listener,
            Router::new()
                .route("/hello", get(|| async { "Hello" }))
                .nest_service("/rpc_test", service)
                .layer(cors),
        )
        .await
        .unwrap();
    })
    .await
    .unwrap();
}
