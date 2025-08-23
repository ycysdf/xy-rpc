use crate::formats::SerdeFormat;
use crate::tokio::ChannelBuilderTokioExt;
use crate::{ChannelBuilder, RpcMsgHandler, RpcMsgHandlerWrapper, RpcSchema, XyRpcChannel};
use alloc::string::ToString;
use alloc::sync::Arc;
use axum::body::Body;
use axum::extract::Request;
use axum::handler::Handler;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use core::convert::Infallible;
use core::marker::PhantomData;
use core::task::{Context, Poll};
use std::println;
use futures_util::future::BoxFuture;
use futures_util::{FutureExt, StreamExt};
use tokio::io::{AsyncWriteExt, SimplexStream, WriteHalf};
use tower::Service;
use uuid::{Uuid, uuid};

struct StreamInfo {
    write: WriteHalf<SimplexStream>,
}

pub struct XyWebRpcService<SF, F, CS, S, T>
where
    CS: RpcSchema,
{
    serde_format: SF,
    streams: Arc<dashmap::DashMap<Uuid, StreamInfo>>,
    channel_sender: flume::Sender<(Request, XyRpcChannel<SF, CS>)>,
    f: Arc<F>,
    _marker: PhantomData<(F, S, T)>,
}
impl<SF, F, CS, S, T> Clone for XyWebRpcService<SF, F, CS, S, T>
where
    SF: SerdeFormat,
    CS: RpcSchema,
{
    fn clone(&self) -> Self {
        Self {
            serde_format: self.serde_format.clone(),
            streams: self.streams.clone(),
            channel_sender: self.channel_sender.clone(),
            f: self.f.clone(),
            _marker: Default::default(),
        }
    }
}

impl<SF, F, CS, S, T> XyWebRpcService<SF, F, CS, S, T>
where
    SF: SerdeFormat,
    F: for<'a> Fn(&'a mut Request, XyRpcChannel<SF, CS>) -> T,
    CS: RpcSchema,
    S: RpcSchema,
    RpcMsgHandlerWrapper<T>: RpcMsgHandler<S>,
{
    pub fn new(f: F, serde_format: SF) -> (Self, flume::Receiver<(Request, XyRpcChannel<SF, CS>)>) {
        let (channel_sender, channel_receiver) =
            flume::unbounded::<(Request, XyRpcChannel<SF, CS>)>();

        (
            Self {
                serde_format,
                streams: Arc::new(Default::default()),
                channel_sender,
                f: f.into(),
                _marker: Default::default(),
            },
            channel_receiver,
        )
    }
}

pub const XY_RPC_HEADER_KEY_STREAM_ID: &'static str = "stream_id";

impl<SF, F, CS, S, T> Service<Request> for XyWebRpcService<SF, F, CS, S, T>
where
    SF: SerdeFormat,
    T: 'static,
    F: for<'a> Fn(&'a mut Request, XyRpcChannel<SF, CS>) -> T,
    CS: RpcSchema,
    S: RpcSchema,
    RpcMsgHandlerWrapper<T>: RpcMsgHandler<S>,
{
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        println!("{:?}", req.uri());
        // async move { Ok(StatusCode::OK.into_response()) }.boxed()
        let stream_id = req.uri().query().unwrap().split("=").last().unwrap().trim_end_matches("&");
        let stream_id = Uuid::parse_str(stream_id).unwrap();
        match req.uri().path() {
            "/data_stream" => {
                let (read_read, read_write) = tokio::io::simplex(1024 * 8);
                let (write_read, write_write) = tokio::io::simplex(1024 * 8);
                let write = tokio_util::io::ReaderStream::new(write_read);

                let (channel, future) = ChannelBuilder::new(self.serde_format.clone())
                    .call_and_serve(|channel| (self.f)(&mut req, channel))
                    .build_from_tokio_read_write((read_read, write_write));
                tokio::spawn(future);
                self.channel_sender.send((req, channel)).unwrap();

                let body = Body::from_stream(write);
                let mut response = Response::new(body);
                response.headers_mut().insert(
                    XY_RPC_HEADER_KEY_STREAM_ID,
                    HeaderValue::from_bytes(stream_id.to_string().as_bytes()).unwrap(),
                );
                self.streams
                    .insert(stream_id, StreamInfo { write: read_write });
                async move { Ok(response) }.boxed()
            }
            "/write_data" => {
                let mut body = req.into_body().into_data_stream();

                if !self.streams.contains_key(&stream_id) {
                    unreachable!()
                }
                let streams = self.streams.clone();
                async move {
                    // let mut stream = streams.get_mut(&stream_id).unwrap();
                    let mut stream = streams.get_mut(&stream_id).unwrap();
                    while let Some(item) = body.next().await {
                        match item {
                            Ok(bytes) => {
                                stream.write.write_all(bytes.as_ref()).await.unwrap();
                            }
                            Err(err) => {}
                        }
                    }
                    Ok(StatusCode::OK.into_response())
                }
                .boxed()
            }
            _ => {
                todo!()
            }
        }
    }
}
