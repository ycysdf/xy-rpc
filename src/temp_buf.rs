use bytes::BytesMut;

#[cfg(feature = "std")]
std::thread_local! {
    pub static BUF: core::cell::RefCell<BytesMut> = core::cell::RefCell::new(BytesMut::with_capacity(1024 * 8));
}

#[cfg(feature = "std")]
pub fn with_buf<U>(f: impl FnOnce(&mut BytesMut) -> U) -> U {
    BUF.with_borrow_mut(|buf| {
        buf.clear();
        f(buf)
    })
}

#[cfg(not(feature = "std"))]
pub fn with_buf<U>(f: impl FnOnce(&mut BytesMut) -> U) -> U {
    let mut bytes_mut = BytesMut::new();
    f(&mut bytes_mut)
}
