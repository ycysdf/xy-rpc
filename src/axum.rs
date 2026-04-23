use crate::formats::SerdeFormat;
use crate::tokio::ChannelBuilderTokioExt;
use crate::{ChannelBuilder, RpcMsgHandler, RpcMsgHandlerWrapper, RpcSchema, XyRpcChannel};
use alloc::string::ToString;
use alloc::sync::Arc;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use core::convert::Infallible;
use core::marker::PhantomData;
use core::task::{Context, Poll};
use futures_util::future::BoxFuture;
use futures_util::{FutureExt, StreamExt};
use tokio::io::{AsyncWriteExt, SimplexStream, WriteHalf};
use tower::Service;
use uuid::Uuid;

struct StreamInfo {
    write: WriteHalf<SimplexStream>,
}

pub struct XyWebRpcService<SF, F, CS, S, T>
where
    CS: RpcSchema,
{
    serde_format: SF,
    streams: Arc<dashmap::DashMap<Uuid, StreamInfo>>,
    channel_sender: flume::Sender<XyRpcChannel<SF, CS>>,
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

impl<SF, F, CS, S, T: 'static> XyWebRpcService<SF, F, CS, S, T>
where
    SF: SerdeFormat,
    F: for<'a> Fn(&'a mut Request, XyRpcChannel<SF, CS>) -> T,
    CS: RpcSchema,
    S: RpcSchema,
    RpcMsgHandlerWrapper<T>: RpcMsgHandler<S>,
{
    pub fn new(f: F, serde_format: SF) -> (Self, flume::Receiver<XyRpcChannel<SF, CS>>) {
        let (channel_sender, channel_receiver) = flume::unbounded::<XyRpcChannel<SF, CS>>();

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

pub const XY_RPC_HEADER_KEY_STREAM_ID: &str = "stream_id";

impl<SF, F, CS, S, T: 'static> Service<Request> for XyWebRpcService<SF, F, CS, S, T>
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

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let path = req.uri().path().to_string();
        let stream_id = match parse_stream_id(&req) {
            Ok(id) => id,
            Err(resp) => return async move { Ok(resp) }.boxed(),
        };
        match path.as_str() {
            "/data_stream" => self.handle_data_stream(req, stream_id),
            "/write_data" => self.handle_write_data(req, stream_id),
            _ => {
                async move {
                    Ok(StatusCode::NOT_FOUND.into_response())
                }
                .boxed()
            }
        }
    }
}

fn parse_stream_id(req: &Request) -> Result<Uuid, Response> {
    let query = req
        .uri()
        .query()
        .ok_or_else(|| StatusCode::BAD_REQUEST.into_response())?;
    let id_str = query
        .split('=')
        .last()
        .ok_or_else(|| StatusCode::BAD_REQUEST.into_response())?
        .trim_end_matches('&');
    Uuid::parse_str(id_str).map_err(|_| StatusCode::BAD_REQUEST.into_response())
}

impl<SF, F, CS, S, T: 'static> XyWebRpcService<SF, F, CS, S, T>
where
    SF: SerdeFormat,
    F: for<'a> Fn(&'a mut Request, XyRpcChannel<SF, CS>) -> T,
    CS: RpcSchema,
    S: RpcSchema,
    RpcMsgHandlerWrapper<T>: RpcMsgHandler<S>,
{
    fn handle_data_stream(
        &mut self,
        mut req: Request,
        stream_id: Uuid,
    ) -> BoxFuture<'static, Result<Response, Infallible>> {
        let (read_read, read_write) = tokio::io::simplex(1024 * 8);
        let (write_read, write_write) = tokio::io::simplex(1024 * 8);
        let write = tokio_util::io::ReaderStream::new(write_read);

        let (channel, future) = ChannelBuilder::new(self.serde_format.clone())
            .call_and_serve(|channel| (self.f)(&mut req, channel))
            .build_from_tokio_read_write((read_read, write_write));
        tokio::spawn(future);
        self.channel_sender.send(channel).unwrap();

        let body = Body::from_stream(write);
        let mut response = Response::new(body);
        response.headers_mut().insert(
            XY_RPC_HEADER_KEY_STREAM_ID,
            HeaderValue::from_bytes(stream_id.to_string().as_bytes()).unwrap(),
        );
        self.streams.insert(stream_id, StreamInfo { write: read_write });
        async move { Ok(response) }.boxed()
    }

    fn handle_write_data(
        &self,
        req: Request,
        stream_id: Uuid,
    ) -> BoxFuture<'static, Result<Response, Infallible>> {
        let mut body = req.into_body().into_data_stream();

        if !self.streams.contains_key(&stream_id) {
            return async move {
                Ok(StatusCode::NOT_FOUND.into_response())
            }
            .boxed();
        }
        let streams = self.streams.clone();
        async move {
            let mut entry = match streams.get_mut(&stream_id) {
                Some(entry) => entry,
                None => return Ok(StatusCode::NOT_FOUND.into_response()),
            };
            while let Some(item) = body.next().await {
                if let Ok(bytes) = item {
                    let _ = entry.write.write_all(bytes.as_ref()).await;
                }
            }
            Ok(StatusCode::OK.into_response())
        }
        .boxed()
    }
}
