use crate::RpcError;
use crate::frame::RpcFrame;
use crate::maybe_send::{MaybeSend, MaybeSync};
use core::fmt::Debug;
use futures_util::{Sink, TryStream};

pub trait RpcTransportStream:
    TryStream<Ok = RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + 'static>
    + MaybeSend
    + 'static
{
}

impl<S> RpcTransportStream for S where
    S: ?Sized
        + TryStream<Ok = RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + 'static>
        + MaybeSend
        + 'static
{
}

pub trait RpcTransportSink:
    Sink<RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + 'static>
    + MaybeSend
    + 'static
{
}

impl<S> RpcTransportSink for S where
    S: ?Sized
        + Sink<RpcFrame, Error: Into<RpcError> + Debug + MaybeSend + 'static>
        + MaybeSend
        + 'static
{
}

pub trait RpcTransport: RpcTransportStream + RpcTransportSink {}

impl<S> RpcTransport for S where S: ?Sized + RpcTransportStream + RpcTransportSink {}
