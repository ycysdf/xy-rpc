#![allow(dead_code)]

pub use send_sync::*;

#[cfg(feature = "send_sync")]
mod send_sync {
    pub use core::marker::Send as MaybeSend;
    pub use core::marker::Sync as MaybeSync;
}

#[cfg(not(feature = "send_sync"))]
mod send_sync {
    pub unsafe trait MaybeSend {}

    unsafe impl<T> MaybeSend for T where T: ?Sized {}

    pub unsafe trait MaybeSync {}

    unsafe impl<T> MaybeSync for T where T: ?Sized {}
}

#[cfg(feature = "send_sync")]
pub type BoxedFutureMaybeLocal<'a, T> = futures_util::future::BoxFuture<'a, T>;

#[cfg(not(feature = "send_sync"))]
pub type BoxedFutureMaybeLocal<'a, T> = futures_util::future::LocalBoxFuture<'a, T>;
#[cfg(feature = "send_sync")]
pub type BoxedStreamMaybeLocal<'a, T> = futures_util::stream::BoxStream<'a, T>;

// #[cfg(not(feature = "send_sync"))]
// pub type BoxedStreamMaybeLocal<'a, T> = futures_util::stream::LocalBoxStream<'a, T>;
