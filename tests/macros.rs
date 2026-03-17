use futures_util::{AsyncRead, Stream};
use serde::{Deserialize, Serialize};
use core::pin::Pin;
use core::task::{Context, Poll};
use xy_rpc::formats::SerdeFormat;
use xy_rpc::maybe_send::{MaybeSend, MaybeSync};
use xy_rpc::{RpcError, TransStream};
use xy_rpc_macro::rpc_service;

#[test]
fn rpc_service_test() {
    #[rpc_service]
    trait TestService {
        async fn empty(&self);
        async fn with_args(&self, a: bool, b: u32, c: String, d: (), e: String);
        async fn with_return(&self) -> String;
        async fn with_return2(&self) -> (String, bool);
        async fn unary(&self, a: String) -> String;
        async fn msg_streaming(&self, stream: TransStream<String, impl SerdeFormat>);
        async fn msg_streaming2(&self, foo: f32, stream: TransStream<String, impl SerdeFormat>);
        async fn reply_streaming(
            &self,
        ) -> impl Stream<Item = Result<bool, RpcError>> + MaybeSend + 'static;
        async fn reply_streaming2(
            &self,
            foo: f32,
        ) -> impl Stream<Item = Result<bool, RpcError>> + MaybeSend + 'static;
        async fn bidirectional_streaming(
            &self,
            foo: f32,
            stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Stream<Item = Result<bool, RpcError>> + MaybeSend + 'static;

        async fn bytes(&self, a: bytes::Bytes) -> bytes::Bytes;
    }
    struct CustomFuture;
    impl Future for CustomFuture {
        type Output = String;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Ready("hello world".into())
        }
    }
    #[rpc_service]
    trait FutureTestService {
        fn with_future(&self) -> impl Future<Output = ()>;
        fn with_future2(&self, arg: String) -> impl Future<Output = String>;
        fn with_custom_future2(&self, arg: String) -> CustomFuture;
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Foo {}

    #[rpc_service]
    trait SerdeTestService {
        async fn abc(&self, a: Foo) -> Foo;
    }
}
