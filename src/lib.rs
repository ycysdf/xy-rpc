#![no_std]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub use bytes;
pub use auto_enums::enum_derive;
pub use flume;
#[cfg(feature = "rt_tokio_without_send_sync")]
pub mod tokio;

pub use xy_rpc_macro as macros;

mod channel;
mod channel_builder;
#[cfg(feature = "rt_compio")]
pub mod compio;
#[cfg(feature = "duplex")]
pub mod duplex;
pub mod formats;
mod frame;
mod handle_rpc;
pub mod maybe_send;
pub mod read_stream;
pub mod temp_buf;
mod transport;
mod future_stream_with_sender;
pub use future_stream_with_sender::*;

pub use channel_builder::*;
pub use transport::*;

pub use handle_rpc::*;

pub use channel::*;

pub use xy_rpc_macro::rpc_service;

pub type EmptyStream = maybe_send::BoxedStreamMaybeLocal<'static, Result<(), RpcError>>;
