#[cfg(test)]
mod tests {
    #[derive(Debug, Serialize, Deserialize)]
    struct ComplexObj {
        a: String,
        b: u32,
        c: bool,
    }

    struct Test2Service;

    #[rpc_service]
    trait RpcTest2Service: Send + Sync {
        fn hello(&self, x: u32) -> impl Future<Output = u32> + Send;
    }
    impl RpcTest2Service for Test2Service {
        fn hello(&self, x: u32) -> impl Future<Output = u32> + Send {
            async move {
                tokio::time::sleep(Duration::from_secs(x as _)).await;
                x
            }
        }
    }

    struct TestService;

    #[rpc_service]
    trait RpcTestService: Send + Sync {
        fn a(&self, x: u32) -> impl Future<Output = u32> + Send;
        fn b(&self, p1: u32, p2: String, p3: bool) -> impl Future<Output = u32> + Send;
        fn c(&self, x: ComplexObj) -> impl Future<Output = String> + Send;
    }
    impl RpcTestService for TestService {
        fn a(&self, x: u32) -> impl Future<Output = u32> + Send {
            async move {
                tokio::time::sleep(Duration::from_secs(x as _)).await;
                x
            }
        }

        fn b(&self, p1: u32, p2: String, p3: bool) -> impl Future<Output = u32> + Send {
            async move { p1 }
        }

        fn c(&self, x: ComplexObj) -> impl Future<Output = String> + Send {
            async move { format!("{x:?}") }
        }
    }

    use serde::{Deserialize, Serialize};
    use std::time::Duration;
    use tokio::try_join;
    use xy_rpc::formats::JsonFormat;
    use xy_rpc::tokio::serve_duplex;
    use xy_rpc_macro::rpc_service;

    #[tokio::test]
    async fn duplex_test() {
        serve_duplex(
            JsonFormat,
            (
                |_| TestService,
                async |channel| {
                    channel.hello(&1).await?;
                    Ok(())
                },
            ),
            (
                |_| Test2Service,
                async |channel| {
                    channel.a(&1).await?;
                    channel.b(&12322, &"TEST".to_string(), &false).await?;
                    channel
                        .c(&ComplexObj {
                            a: "".to_string(),
                            b: 0,
                            c: false,
                        })
                        .await?;
                    Ok(())
                },
            ),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn concurrent_test() {
        serve_duplex(
            JsonFormat,
            (
                |_| TestService,
                async |channel| {
                    let r = try_join!(
                        channel.hello(&1),
                        channel.hello(&2),
                        channel.hello(&3),
                        channel.hello(&4)
                    )?;
                    assert_eq!(r, (1, 2, 3, 4));
                    Ok(())
                },
            ),
            (
                |_| Test2Service,
                async |channel| {
                    let r = try_join!(channel.a(&1), channel.a(&2), channel.a(&3), channel.a(&4))?;
                    assert_eq!(r, (1, 2, 3, 4));
                    Ok(())
                },
            ),
        )
        .await
        .unwrap();
    }
}
