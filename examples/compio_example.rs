use serde::{Deserialize, Serialize};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use xy_rpc::compio::serve_duplex_from_compio;
use xy_rpc::formats::SerdeFormat;
use xy_rpc_macro::rpc_service;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComplexObj {
    a: String,
    b: u32,
    c: bool,
    d: Vec<u32>,
    e: Vec<String>,
    f: Vec<bool>,
    g: Vec<ComplexObj>,
}

#[rpc_service]
trait ClientService: Send + Sync {
    async fn hello1(&self, content: ComplexObj) -> ComplexObj;
}

#[rpc_service]
trait ServerService: Send + Sync {
    async fn hello2(&self, content: ComplexObj) -> ComplexObj;
}

struct TestClientService;
struct TestServerService;

impl ClientService for TestClientService {
    fn hello1(&self, mut content: ComplexObj) -> impl Future<Output = ComplexObj> + Send {
        async move {
            println!("ClientService: {:?}", content);
            content.g.push(content.clone());
            content
        }
    }
}

impl ServerService for TestServerService {
    async fn hello2(&self, mut content: ComplexObj) -> ComplexObj {
        println!("ServerService: {:?}", content);
        content.g.push(content.clone());
        content
    }
}

#[derive(Clone)]
struct MySerdeFormat;

impl SerdeFormat for MySerdeFormat {
    fn serialize_to_writer<W, T>(&self, writer: W, value: &T) -> std::io::Result<()>
    where
        W: Write,
        T: ?Sized + Serialize,
    {
        use bincode;
        bincode::serialize_into(writer, value)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> std::io::Result<T>
    where
        T: Deserialize<'a>,
    {
        use bincode;
        bincode::deserialize(v).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

#[compio::main]
async fn main() {
    let listener =
        compio::net::TcpListener::bind(SocketAddr::from((IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)))
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
    println!("Accepted And Connected. addr: {addr:?}");
    serve_duplex_from_compio(
        (accepted_stream.into_split(), connected_stream.into_split()),
        MySerdeFormat,
        (
            |_| TestClientService,
            async |channel| {
                for i in 0..3 {
                    let r = channel
                        .hello2(&ComplexObj {
                            a: "A Value".to_string(),
                            b: i,
                            c: true,
                            d: vec![1, 2, 3, 4, 5],
                            e: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                            f: vec![true, false, true],
                            g: vec![],
                        })
                        .await;
                    println!("hello2 reply: {:?}", r);
                }
                Ok(())
            },
        ),
        (
            |_| TestServerService,
            async |channel| {
                for i in 0..3 {
                    let r = channel
                        .hello1(&ComplexObj {
                            a: "SDF Value".to_string(),
                            b: i,
                            c: true,
                            d: vec![1, 2, 3, 4, 5],
                            e: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                            f: vec![true, false],
                            g: vec![],
                        })
                        .await;
                    println!("hello1 reply: {:?}", r);
                }
                Ok(())
            },
        ),
    )
    .await
    .unwrap();
}
