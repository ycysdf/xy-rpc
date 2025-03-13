use crate::maybe_send::{AnyError, MaybeSend, MaybeSync};
use alloc::boxed::Box;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use core::mem::ManuallyDrop;
use serde::{Deserialize, Serialize};
use try_specialize::TrySpecialize;

pub trait SerdeFormat: MaybeSend + MaybeSync + Clone + 'static {
    fn serialize_to_buf<T>(&self, writer: &mut BytesMut, value: &T) -> Result<(), AnyError>
    where
        T: ?Sized + Serialize;
    fn serialize_to_writer_optimized<T>(
        &self,
        writer: &mut BytesMut,
        value: &T,
    ) -> Result<(), AnyError>
    where
        T: ?Sized + Serialize,
    {
        // return self.serialize_to_writer(writer, value);
        if let Some(bytes) = unsafe { value.try_specialize_ref_ignore_lifetimes::<Bytes>() } {
            writer.put_slice(bytes.chunk());
            Ok(())
        } else {
            self.serialize_to_buf(writer, value)
        }
    }
    // fn serialize_to_vec<T>(value: &T) -> Result<Vec<u8>,Box<dyn Error+MaybeSend>
    // where
    //     T: ?Sized + Serialize;
    fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> Result<T, AnyError>
    where
        T: ?Sized + Deserialize<'a>;

    fn deserialize_from_slice_optimized<'a, T>(
        &self,
        v: &'a impl AsRef<[u8]>,
    ) -> Result<T, AnyError>
    where
        T: ?Sized + Deserialize<'a>,
    {
        // return self.deserialize_from_slice(v);
        if try_specialize::type_eq_ignore_lifetimes::<T, Bytes>() {
            match unsafe { v.try_specialize_ref_ignore_lifetimes::<Bytes>() } {
                Some(bytes) => {
                    let bytes = ManuallyDrop::new(bytes.clone());
                    Ok(unsafe { core::mem::transmute_copy(&bytes) })
                }
                None => {
                    let bytes = ManuallyDrop::new(bytes::Bytes::copy_from_slice(v.as_ref()));
                    Ok(unsafe { core::mem::transmute_copy(&bytes) })
                }
            }
        } else {
            self.deserialize_from_slice(v.as_ref())
        }
    }
    // fn deserialize_from_reader<R, T>(&self, reader: R) -> Result<T,Box<dyn Error+MaybeSend>
    // where
    //     R: Read,
    //     T: DeserializeOwned;
}

// pub trait DynSerdeFormat: Send + Sync + Clone + 'static {
//     fn serialize_to_writer_dyn(&self, writer: &dyn Write, value: &dyn ) -> Result(),AnyError;
//     fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> Result<T,Box<dyn Error+MaybeSend>
//     where
//         T: Deserialize<'a>;
// }

#[cfg(feature = "format_json")]
#[derive(Clone, Default, Debug)]
pub struct JsonFormat;

#[cfg(feature = "format_json")]
impl SerdeFormat for JsonFormat {
    fn serialize_to_buf<T>(&self, writer: &mut BytesMut, value: &T) -> Result<(), AnyError>
    where
        T: ?Sized + Serialize,
    {
        Ok(serde_json::to_writer(writer.writer(), value)?)
    }

    // fn serialize_to_vec<T>(value: &T) -> Result<Vec<u8>,Box<dyn Error+MaybeSend>
    // where
    //     T: ?Sized + Serialize,
    // {
    //     Ok(serde_json::to_vec(value)?)
    // }

    fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> Result<T, AnyError>
    where
        T: Deserialize<'a>,
    {
        Ok(serde_json::from_slice(v)?)
    }

    // fn deserialize_from_reader<R, T>(&self, reader: R) -> Result<T,Box<dyn Error+MaybeSend>
    // where
    //     R: Read,
    //     T: DeserializeOwned,
    // {
    //     Ok(serde_json::from_reader(reader)?)
    // }
}

#[cfg(feature = "format_message_pack")]
#[derive(Clone, Debug)]
pub struct MessagePackFormat;

#[cfg(feature = "format_message_pack")]
impl SerdeFormat for MessagePackFormat {
    fn serialize_to_buf<T>(&self, writer: &mut BytesMut, value: &T) -> Result<(), AnyError>
    where
        T: ?Sized + Serialize,
    {
        Ok(rmp_serde::encode::write(&mut writer.writer(), value).map_err(|err| Box::new(err))?)
    }

    // fn serialize_to_vec<T>(value: &T) -> Result<Vec<u8>,Box<dyn Error+MaybeSend>
    // where
    //     T: ?Sized + Serialize,
    // {
    //     Ok(rmp_serde::to_vec(value).map_err(|err| Box::new(err))?)
    // }

    fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> Result<T, AnyError>
    where
        T: Deserialize<'a>,
    {
        Ok(rmp_serde::from_slice(v).map_err(|err| Box::new(err))?)
    }

    // fn deserialize_from_reader<R, T>(&self, reader: R) -> Result<T,Box<dyn Error+MaybeSend>
    // where
    //     R: Read,
    //     T: DeserializeOwned,
    // {
    //     Ok(rmp_serde::decode::from_read(reader).map_err(|err| Box::new(err))?)
    // }
}
// #[derive(From, Deref, DerefMut, Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
// pub struct RawBytes(pub Bytes);
