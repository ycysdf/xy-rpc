use bytes::{Buf, Bytes};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::ops::Index;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use xy_rpc::TransStream;
use xy_rpc::duplex::serve_duplex_from;
use xy_rpc::formats::{JsonFormat, MessagePackFormat, SerdeFormat};
use xy_rpc::maybe_send::MaybeSend;
use xy_rpc::tokio::serve_duplex_tokio;
use xy_rpc::{RpcError, XyRpcChannel};
use xy_rpc_macro::rpc_service;

#[tokio::test]
async fn test_single_thread() {
    test_all_format(RunAsyncWay::Tokio).await;
    test_all_format(RunAsyncWay::Futures).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multi_thread() {
    test_all_format(RunAsyncWay::Tokio).await;
    test_all_format(RunAsyncWay::Futures).await;
}

#[cfg(feature = "rt_compio")]
#[compio::test]
async fn test_compio() {
    test_all_format(RunAsyncWay::Compio).await;
    test_all_format(RunAsyncWay::Futures).await;
}

async fn test_all_format(run_way: RunAsyncWay) {
    test_channel2(run_way, JsonFormat).await;
    test_channel2(run_way, MessagePackFormat).await;
}

async fn test_channel2(run_way: RunAsyncWay, serde_format: impl SerdeFormat) {
    #[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
    enum TestEnum {
        A(u32),
        B(f32),
        C(String),
    }
    #[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
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
    trait FooService {
        async fn unary(&self, index: u64, arg1: TestStruct) -> TestStruct;
        async fn msg_streaming(&self, stream: TransStream<String, impl SerdeFormat>);
        async fn msg_streaming2(&self, count: usize, stream: TransStream<String, impl SerdeFormat>);
        async fn reply_streaming(
            &self,
        ) -> impl Stream<Item = Result<usize, RpcError>> + MaybeSend + 'static;
        async fn reply_streaming2(
            &self,
            count: usize,
        ) -> impl Stream<Item = Result<usize, RpcError>> + MaybeSend + 'static;
        async fn bidirectional_streaming(
            &self,
            count: usize,
            stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Stream<Item = Result<String, RpcError>> + MaybeSend + 'static;
        async fn bidirectional_streaming2(
            &self,
            count: usize,
            stream: TransStream<Bytes, impl SerdeFormat>,
        ) -> impl Stream<Item = Result<Bytes, RpcError>> + MaybeSend + 'static;

        async fn bytes(&self, a: Bytes) -> Bytes;
    }
    struct FooServiceImpl(String);
    impl FooService for FooServiceImpl {
        fn unary(
            &self,
            _index: u64,
            mut arg1: TestStruct,
        ) -> impl Future<Output = TestStruct> + MaybeSend {
            async move { arg1 }
        }

        fn msg_streaming(
            &self,
            mut stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Future<Output = ()> + MaybeSend {
            async move {
                let mut i = 0;
                while let Some(n) = stream.next().await {
                    let item = n.unwrap();
                    assert_eq!(item, get_tests_string_item(i));
                    i += 1;
                }
                assert_eq!(i, TEST_ITEM_COUNT);
            }
        }

        fn msg_streaming2(
            &self,
            count: usize,
            mut stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Future<Output = ()> + MaybeSend {
            async move {
                assert_eq!(count, TEST_ITEM_COUNT);
                let mut i = 0;
                while let Some(n) = stream.next().await {
                    let item = n.unwrap();
                    assert_eq!(item, get_tests_string_item(i));
                    i += 1;
                }
                assert_eq!(i, count);
            }
        }

        fn reply_streaming(
            &self,
        ) -> impl Future<
            Output = impl Stream<Item = Result<usize, RpcError>> + MaybeSend + 'static,
        > + MaybeSend {
            async { futures_util::stream::iter((0..TEST_ITEM_COUNT).map(|n| Ok(n))) }
        }

        fn reply_streaming2(
            &self,
            count: usize,
        ) -> impl Future<
            Output = impl Stream<Item = Result<usize, RpcError>> + MaybeSend + 'static,
        > + MaybeSend {
            assert_eq!(count, TEST_ITEM_COUNT);
            async move { futures_util::stream::iter((0..count).map(|n| Ok(n))) }
        }

        fn bidirectional_streaming(
            &self,
            count: usize,
            stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Future<
            Output = impl Stream<Item = Result<String, RpcError>> + MaybeSend + 'static,
        > + MaybeSend {
            assert_eq!(count, TEST_ITEM_COUNT);
            async move { stream.map(|n| n.map(|n| format!("Result:{n}"))) }
        }

        fn bidirectional_streaming2(
            &self,
            count: usize,
            stream: TransStream<Bytes, impl SerdeFormat>,
        ) -> impl Future<
            Output = impl Stream<Item = Result<Bytes, RpcError>> + MaybeSend + 'static,
        > + MaybeSend {
            assert_eq!(count, TEST_ITEM_COUNT);
            async move {
                stream.map(|n| {
                    n.map(|n| format!("Result:{}", String::from_utf8_lossy(n.chunk())).into())
                })
            }
        }

        fn bytes(&self, a: Bytes) -> impl Future<Output = Bytes> + MaybeSend {
            async move { a }
        }
    }
    const TEST_ITEM_COUNT: usize = 1024;
    fn get_tests_string_item(i: usize) -> String {
        format!("Item {}", i)
    }
    let gen_serve = move |name: String| {
        move |channel: XyRpcChannel<_, FooServiceSchema>| {
            let name = name.clone();
            async move {
                let mut test_struct1 = TestStruct::test_data(name);
                for i in 0..8 {
                    test_struct1.index = i;
                    let _r = channel.unary(&(i as _), &test_struct1).await?;
                    assert_eq!(test_struct1, _r);
                }
                for i in 0..8 {
                    let bytes1 = Bytes::copy_from_slice(&[i, u8::MAX - i]).into();
                    let _r = channel.bytes(&bytes1).await?;
                    assert_eq!(bytes1, _r)
                }
                let stream = futures_util::stream::iter(
                    (0..TEST_ITEM_COUNT).map(|n| Ok(get_tests_string_item(n))),
                );
                let _r = channel.msg_streaming(stream).await?;

                let stream = futures_util::stream::iter(
                    (0..TEST_ITEM_COUNT).map(|n| Ok(get_tests_string_item(n))),
                );
                let _r = channel.msg_streaming2(&TEST_ITEM_COUNT, stream).await?;

                {
                    let mut stream = channel.reply_streaming().await?;

                    let mut i = 0;
                    while let Some(n) = stream.next().await {
                        let item = n.unwrap();
                        assert_eq!(item, i);
                        i += 1;
                    }
                    assert_eq!(i, TEST_ITEM_COUNT);
                }
                {
                    let mut stream = channel.reply_streaming2(&TEST_ITEM_COUNT).await?;

                    let mut i = 0;
                    while let Some(n) = stream.next().await {
                        let item = n.unwrap();
                        assert_eq!(item, i);
                        i += 1;
                    }
                    assert_eq!(i, TEST_ITEM_COUNT);
                }
                {
                    let stream = futures_util::stream::iter(
                        (0..TEST_ITEM_COUNT).map(|n| Ok(get_tests_string_item(n))),
                    );
                    let mut stream = channel
                        .bidirectional_streaming(&TEST_ITEM_COUNT, stream)
                        .await?;

                    let mut i = 0;
                    while let Some(n) = stream.next().await {
                        let item = n.unwrap();
                        assert_eq!(item, format!("Result:{}", get_tests_string_item(i)));
                        i += 1;
                    }
                    assert_eq!(i, TEST_ITEM_COUNT);
                }
                {
                    let stream = futures_util::stream::iter(
                        (0..TEST_ITEM_COUNT).map(|n| Ok(get_tests_string_item(n).into())),
                    );
                    let mut stream = channel
                        .bidirectional_streaming2(&TEST_ITEM_COUNT, stream)
                        .await?;

                    let mut i = 0;
                    while let Some(n) = stream.next().await {
                        let item = n.unwrap();
                        assert_eq!(item, {
                            let r: Bytes = format!("Result:{}", get_tests_string_item(i)).into();
                            r
                        });
                        i += 1;
                    }
                    assert_eq!(i, TEST_ITEM_COUNT);
                }
                Ok(())
            }
        }
    };
    match run_way {
        RunAsyncWay::Futures => {
            let (peer1, peer2) = get_tokio_net_duplex().await;
            serve_duplex_from(
                (
                    {
                        let (read, write) = peer1.into_split();
                        (read.compat(), write.compat_write())
                    },
                    {
                        let (read, write) = peer2.into_split();
                        (read.compat(), write.compat_write())
                    },
                ),
                serde_format,
                (
                    |_| FooServiceImpl("Futures".into()),
                    gen_serve("peer1".into()),
                ),
                (
                    |_| FooServiceImpl("Futures".into()),
                    gen_serve("peer2".into()),
                ),
            )
            .await
            .unwrap();
        }
        RunAsyncWay::Tokio => {
            serve_duplex_tokio(
                serde_format,
                (
                    |_| FooServiceImpl("Futures".into()),
                    gen_serve("peer1".into()),
                ),
                (
                    |_| FooServiceImpl("Futures".into()),
                    gen_serve("peer2".into()),
                ),
            )
            .await
            .unwrap();
        }
        RunAsyncWay::Compio => {
            #[cfg(feature = "rt_compio")]
            {
                let (peer1, peer2) = get_compio_net_duplex().await;
                xy_rpc::compio::serve_duplex_from_compio(
                    (peer1.into_split(), peer2.into_split()),
                    serde_format,
                    (
                        |_| FooServiceImpl("Futures".into()),
                        gen_serve("peer1".into()),
                    ),
                    (
                        |_| FooServiceImpl("Futures".into()),
                        gen_serve("peer2".into()),
                    ),
                )
                .await
                .unwrap();
            }
        }
    }
}

#[derive(Clone, Copy)]
enum RunAsyncWay {
    Futures,
    Tokio,
    Compio,
}

#[cfg(feature = "rt_compio")]
async fn get_compio_net_duplex() -> (compio::net::TcpStream, compio::net::TcpStream) {
    let listener = compio::net::TcpListener::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let ((accepted_stream, _), connected_stream) = futures_util::try_join!(
        listener.accept(),
        compio::net::TcpStream::connect(SocketAddr::from((
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            addr.port()
        )))
    )
    .unwrap();
    (accepted_stream, connected_stream)
}

async fn get_tokio_net_duplex() -> (tokio::net::TcpStream, tokio::net::TcpStream) {
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let ((accepted_stream, _), connected_stream) = futures_util::try_join!(
        listener.accept(),
        tokio::net::TcpStream::connect(SocketAddr::from((
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            addr.port()
        )))
    )
    .unwrap();
    (accepted_stream, connected_stream)
}
