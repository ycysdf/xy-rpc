#![allow(dead_code)]
#[allow(unused_imports)]
use bytes::{Buf, Bytes};
use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{Instrument, info_span};
use xy_rpc::TransStream;
use xy_rpc::formats::SerdeFormat;
use xy_rpc::maybe_send::MaybeSend;
use xy_rpc::{RpcError, XyRpcChannel};
use xy_rpc_macro::rpc_service;

#[cfg(feature = "rt_tokio")]
#[tokio::test]
async fn tokio_single_thread() {
    test_all_format(RunAsyncWay::Tokio).await;
}

#[cfg(feature = "rt_tokio")]
#[tokio::test]
async fn tokio_single_thread_futures() {
    // tracing_subscriber::fmt()
    //     .pretty()
    //     .with_target(false)
    //     .with_timer(tracing_subscriber::fmt::time::uptime())
    //     .with_level(true)
    //     .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
    //     .init();
    test_all_format(RunAsyncWay::Futures).await;
}

#[cfg(feature = "rt_tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn tokio_multi_thread() {
    // tracing_subscriber::fmt()
    //     .pretty()
    //     .with_target(false)
    //     .with_timer(tracing_subscriber::fmt::time::uptime())
    //     .with_level(true)
    //     .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
    //     .init();
    test_all_format(RunAsyncWay::Tokio).await;
}

#[cfg(feature = "rt_tokio")]
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn tokio_multi_thread_features() {
    // tracing_subscriber::fmt()
    //     .pretty()
    //     .with_target(false)
    //     .with_timer(tracing_subscriber::fmt::time::uptime())
    //     .with_level(true)
    //     .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
    //     .init();
    test_all_format(RunAsyncWay::Futures).await;
}

#[cfg(feature = "rt_compio")]
#[compio::test]
async fn compio() {
    test_all_format(RunAsyncWay::Compio).await;
}

#[cfg(feature = "rt_compio")]
#[compio::test]
async fn compio_futures() {
    test_all_format(RunAsyncWay::Futures).await;
}

async fn test_all_format(run_way: RunAsyncWay) {
    #[cfg(feature = "format_json")]
    test_channel2(run_way, xy_rpc::formats::JsonFormat).await;
    #[cfg(feature = "format_message_pack")]
    test_channel2(run_way, xy_rpc::formats::MessagePackFormat).await;
}

async fn test_channel2<SF: SerdeFormat>(run_way: RunAsyncWay, serde_format: SF) {
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
        async fn msg_streaming(&self, stream: TransStream<String, impl SerdeFormat>) -> u64;
        async fn msg_streaming2(
            &self,
            count: u64,
            stream: TransStream<String, impl SerdeFormat>,
        ) -> u64;
        async fn reply_streaming(
            &self,
        ) -> impl Stream<Item = Result<u64, RpcError>> + MaybeSend + Unpin + 'static;
        async fn reply_streaming2(
            &self,
            count: u64,
        ) -> impl Stream<Item = Result<u64, RpcError>> + MaybeSend + 'static;
        async fn bidirectional_streaming(
            &self,
            count: u64,
            stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Stream<Item = Result<String, RpcError>> + MaybeSend + 'static;
        async fn bidirectional_streaming2(
            &self,
            count: u64,
            stream: TransStream<Bytes, impl SerdeFormat>,
        ) -> impl Stream<Item = Result<Bytes, RpcError>> + MaybeSend + 'static;

        async fn bytes(&self, a: Bytes) -> Bytes;
    }
    struct FooServiceImpl {
        _name: String,
        count: u64,
    }
    impl FooService for FooServiceImpl {
        fn unary(
            &self,
            _index: u64,
            arg1: TestStruct,
        ) -> impl Future<Output = TestStruct> + MaybeSend {
            async move { arg1 }
        }

        fn msg_streaming(
            &self,
            mut stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Future<Output = u64> + MaybeSend {
            async move {
                let mut i = 0;
                while let Some(n) = stream.next().await {
                    let item = n.unwrap();
                    assert_eq!(item, get_tests_string_item(i));
                    i += 1;
                }
                assert_eq!(i, self.count);
                i
            }
        }

        fn msg_streaming2(
            &self,
            count: u64,
            mut stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Future<Output = u64> + MaybeSend {
            async move {
                assert_eq!(count, self.count);
                let mut i = 0;
                while let Some(n) = stream.next().await {
                    let item = n.unwrap();
                    assert_eq!(item, get_tests_string_item(i));
                    i += 1;
                }
                assert_eq!(i, count);
                i
            }
        }

        fn reply_streaming(
            &self,
        ) -> impl Future<
            Output = impl Stream<Item = Result<u64, RpcError>> + MaybeSend + Unpin + 'static,
        > + MaybeSend {
            async { futures_util::stream::iter((0..self.count).map(|n| Ok(n))) }
        }

        fn reply_streaming2(
            &self,
            count: u64,
        ) -> impl Future<
            Output = impl Stream<Item = Result<u64, RpcError>> + MaybeSend + 'static,
        > + MaybeSend {
            assert_eq!(count, self.count);
            async move { futures_util::stream::iter((0..count).map(|n| Ok(n))) }
        }

        fn bidirectional_streaming(
            &self,
            count: u64,
            stream: TransStream<String, impl SerdeFormat>,
        ) -> impl Future<
            Output = impl Stream<Item = Result<String, RpcError>> + MaybeSend + 'static,
        > + MaybeSend {
            assert_eq!(count, self.count);
            async move { stream.map(move |n| n.map(|n| format!("Result:{n}"))) }
        }

        fn bidirectional_streaming2(
            &self,
            count: u64,
            stream: TransStream<Bytes, impl SerdeFormat>,
        ) -> impl Future<
            Output = impl Stream<Item = Result<Bytes, RpcError>> + MaybeSend + 'static,
        > + MaybeSend {
            assert_eq!(count, self.count);
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
    fn get_tests_string_item(i: u64) -> String {
        format!("Item {}", i)
    }
    let gen_serve = move |name: String, count: u64| {
        move |channel: XyRpcChannel<SF, FooServiceSchema>| {
            let name = name.clone();
            let span = info_span!("channel serve", name);
            async move {
                let mut test_struct1 = TestStruct::test_data(name);
                for i in 0..u8::MAX {
                    test_struct1.index = i as _;
                    let _r = channel.unary(&(i as _), &test_struct1).await?;
                    assert_eq!(test_struct1, _r);
                }
                for i in 0..u8::MAX {
                    let bytes1 = Bytes::copy_from_slice(&[i, u8::MAX - i]).into();
                    let _r = channel.bytes(&bytes1).await?;
                    assert_eq!(bytes1, _r)
                }
                let stream = futures_util::stream::unfold(0, move |state| async move {
                    if state == count {
                        return None;
                    }

                    sleep_some().await;
                    Some((Ok(get_tests_string_item(state as _)), state + 1))
                });
                let _r = channel.msg_streaming(stream).await?;
                assert_eq!(_r, count);

                let stream = futures_util::stream::unfold(0, move |state| async move {
                    if state == count {
                        return None;
                    }
                    sleep_some().await;
                    Some((Ok(get_tests_string_item(state as _)), state + 1))
                });
                let _r = channel.msg_streaming2(&count, stream).await?;
                assert_eq!(_r, count);

                {
                    let mut stream = channel.reply_streaming().await?;

                    let mut i = 0;
                    while let Some(n) = stream.next().await {
                        let item = n.unwrap();
                        assert_eq!(item, i);
                        i += 1;
                    }
                    assert_eq!(i, count);
                }
                {
                    let mut stream = channel.reply_streaming2(&count).await?;

                    let mut i = 0;
                    while let Some(n) = stream.next().await {
                        let item = n.unwrap();
                        assert_eq!(item, i);
                        i += 1;
                    }
                    assert_eq!(i, count);
                }
                {
                    let stream = futures_util::stream::unfold(0, move |state| async move {
                        if state == count {
                            return None;
                        }

                        sleep_some().await;
                        Some((Ok(get_tests_string_item(state as _)), state + 1))
                    });
                    let stream = channel.bidirectional_streaming(&count, stream).await?;
                    let mut stream = std::pin::pin!(stream);
                    let mut i = 0;
                    while let Some(n) = stream.next().await {
                        let item = n.unwrap();
                        assert_eq!(item, format!("Result:{}", get_tests_string_item(i)));
                        i += 1;
                    }
                    assert_eq!(i, count);
                }
                {
                    let stream = futures_util::stream::unfold(0, move |state| async move {
                        if state == count {
                            return None;
                        }

                        sleep_some().await;
                        Some((
                            Ok(Bytes::copy_from_slice(
                                get_tests_string_item(state as _).as_bytes(),
                            )),
                            state + 1,
                        ))
                    });
                    let stream = channel.bidirectional_streaming2(&count, stream).await?;

                    let mut stream = std::pin::pin!(stream);
                    let mut i = 0;
                    while let Some(n) = stream.next().await {
                        let item = n.unwrap();
                        assert_eq!(item, {
                            let r: Bytes = format!("Result:{}", get_tests_string_item(i)).into();
                            r
                        });
                        i += 1;
                    }
                    assert_eq!(i, count);
                }
                Ok::<(), RpcError>(())
            }
            .instrument(span)
        }
    };
    let count1 = 32;
    let count2 = 32;
    let serve1 = gen_serve("peer1".into(), count2);
    let serve2 = gen_serve("peer2".into(), count1);
    // let serve2 = |_| async move { Ok(()) };
    match run_way {
        RunAsyncWay::Futures => {
            #[cfg(feature = "duplex")]
            xy_rpc::duplex::serve_duplex_from(
                {
                    #[cfg(feature = "rt_compio")]
                    {
                        use compio::io::compat::AsyncStream;
                        let (peer1, peer2) = get_compio_net_duplex().await;
                        (
                            {
                                let (read, write) = peer1.into_split();

                                (AsyncStream::new(read), AsyncStream::new(write))
                            },
                            {
                                let (read, write) = peer2.into_split();

                                (AsyncStream::new(read), AsyncStream::new(write))
                            },
                        )
                    }
                    #[cfg(feature = "rt_tokio_without_send_sync")]
                    {
                        use tokio_util::compat::{
                            TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt,
                        };
                        let (peer1, peer2) = crate::get_tokio_net_duplex().await;
                        (
                            {
                                let (read, write) = peer1.into_split();
                                (read.compat(), write.compat_write())
                            },
                            {
                                let (read, write) = peer2.into_split();
                                (read.compat(), write.compat_write())
                            },
                        )
                    }
                },
                serde_format,
                (
                    |_| FooServiceImpl {
                        _name: "Futures".into(),
                        count: count1,
                    },
                    serve1,
                ),
                (
                    |_| FooServiceImpl {
                        _name: "Futures".into(),
                        count: count2,
                    },
                    serve2,
                ),
            )
            .await
            .unwrap();
        }
        RunAsyncWay::Tokio => {
            let (peer1, peer2) = get_tokio_net_duplex().await;

            #[cfg(feature = "rt_tokio_without_send_sync")]
            xy_rpc::tokio::serve_duplex_from_tokio(
                (peer1.into_split(), peer2.into_split()),
                serde_format,
                (
                    |_| FooServiceImpl {
                        _name: "Tokio".into(),
                        count: count1,
                    },
                    serve1,
                ),
                (
                    |_| FooServiceImpl {
                        _name: "Tokio".into(),
                        count: count2,
                    },
                    serve2,
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
                        |_| FooServiceImpl {
                            _name: "Compio".into(),
                            count: count1,
                        },
                        serve1,
                    ),
                    (
                        |_| FooServiceImpl {
                            _name: "Compio".into(),
                            count: count2,
                        },
                        serve2,
                    ),
                )
                .await
                .unwrap();
            }
        }
    }
}

#[allow(dead_code)]
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

async fn sleep_some() {
    #[cfg(feature = "rt_tokio_without_send_sync")]
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    #[cfg(feature = "rt_compio")]
    compio::time::sleep(std::time::Duration::from_millis(1)).await;
}
