use crate::RpcError;
use crate::frame::RpcFrame;
use crate::maybe_send::MaybeSend;
use futures_util::{Sink, Stream};

pub trait RpcTransportStream:
    Stream<Item = Result<RpcFrame, RpcError>> + MaybeSend + 'static
{
}

impl<S> RpcTransportStream for S where
    S: ?Sized + Stream<Item = Result<RpcFrame, RpcError>> + MaybeSend + 'static
{
}

pub trait RpcTransportSink: Sink<RpcFrame, Error = RpcError> + MaybeSend + 'static {}

impl<S> RpcTransportSink for S where
    S: ?Sized + Sink<RpcFrame, Error = RpcError> + MaybeSend + 'static
{
}

pub trait RpcTransport: RpcTransportStream + RpcTransportSink {}

impl<S> RpcTransport for S where S: ?Sized + RpcTransportStream + RpcTransportSink {}
