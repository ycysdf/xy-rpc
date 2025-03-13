use bytes::{BufMut, BytesMut};
use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use serde::{Deserialize, Serialize};
use xy_rpc::compio::serve_duplex_from_compio;
use xy_rpc::formats::SerdeFormat;
use xy_rpc::maybe_send::AnyError;
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
trait ClientService {
    async fn hello1(&self, content: ComplexObj) -> ComplexObj;
}

#[rpc_service]
trait ServerService {
    async fn hello2(&self, content: ComplexObj) -> ComplexObj;
}

struct TestClientService;
struct TestServerService;

impl ClientService for TestClientService {
    fn hello1(&self, mut content: ComplexObj) -> impl Future<Output = ComplexObj> {
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
    fn serialize_to_buf<T>(&self, writer: &mut BytesMut, value: &T) -> Result<(), AnyError>
    where
        T: ?Sized + Serialize,
    {
        use bincode;
        Ok(bincode::serialize_into(writer.writer(), value).map_err(|e| Box::new(e))?)
    }

    fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> Result<T, AnyError>
    where
        T: Deserialize<'a>,
    {
        use bincode;
        Ok(bincode::deserialize(v).map_err(|e| Box::new(e))?)
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
                let mut obj = ComplexObj {
                    a: "A Value".to_string(),
                    b: 0,
                    c: true,
                    d: vec![1, 2, 3, 4, 5],
                    e: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                    f: vec![true, false, true],
                    g: vec![],
                };
                for i in 0..3 {
                    obj.b = i;
                    let r = channel.hello2(&obj).await;
                    println!("hello2 reply: {:?}", r);
                }
                Ok(())
            },
        ),
        (
            |_| TestServerService,
            async |channel| {
                let mut obj = ComplexObj {
                    a: "SDF Value".to_string(),
                    b: 0,
                    c: true,
                    d: vec![1, 2, 3, 4, 5],
                    e: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                    f: vec![true, false],
                    g: vec![],
                };
                for i in 0..3 {
                    obj.b = i;
                    let r = channel.hello1(&obj).await;
                    println!("hello1 reply: {:?}", r);
                }
                Ok(())
            },
        ),
    )
    .await
    .unwrap();
}
