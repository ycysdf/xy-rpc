use serde::{Deserialize, Serialize};
use std::io;
use std::io::Write;

pub trait SerdeFormat: Send + Sync + Clone + 'static {
    fn serialize_to_writer<W, T>(&self, writer: W, value: &T) -> io::Result<()>
    where
        W: Write,
        T: ?Sized + Serialize;
    // fn serialize_to_vec<T>(value: &T) -> io::Result<Vec<u8>>
    // where
    //     T: ?Sized + Serialize;
    fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> io::Result<T>
    where
        T: Deserialize<'a>;
    // fn deserialize_from_reader<R, T>(&self, reader: R) -> io::Result<T>
    // where
    //     R: Read,
    //     T: DeserializeOwned;
}

#[cfg(feature = "format_json")]
#[derive(Clone, Default, Debug)]
pub struct JsonFormat;

#[cfg(feature = "format_json")]
impl SerdeFormat for JsonFormat {
    fn serialize_to_writer<W, T>(&self, writer: W, value: &T) -> io::Result<()>
    where
        W: Write,
        T: ?Sized + Serialize,
    {
        Ok(serde_json::to_writer(writer, value)?)
    }

    // fn serialize_to_vec<T>(value: &T) -> io::Result<Vec<u8>>
    // where
    //     T: ?Sized + Serialize,
    // {
    //     Ok(serde_json::to_vec(value)?)
    // }

    fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> io::Result<T>
    where
        T: Deserialize<'a>,
    {
        Ok(serde_json::from_slice(v)?)
    }

    // fn deserialize_from_reader<R, T>(&self, reader: R) -> io::Result<T>
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
    fn serialize_to_writer<W, T>(&self, mut writer: W, value: &T) -> io::Result<()>
    where
        W: Write,
        T: ?Sized + Serialize,
    {
        Ok(rmp_serde::encode::write(&mut writer, value).map_err(|err| io::Error::other(err))?)
    }

    // fn serialize_to_vec<T>(value: &T) -> io::Result<Vec<u8>>
    // where
    //     T: ?Sized + Serialize,
    // {
    //     Ok(rmp_serde::to_vec(value).map_err(|err| io::Error::other(err))?)
    // }

    fn deserialize_from_slice<'a, T>(&self, v: &'a [u8]) -> io::Result<T>
    where
        T: Deserialize<'a>,
    {
        Ok(rmp_serde::from_slice(v).map_err(|err| io::Error::other(err))?)
    }

    // fn deserialize_from_reader<R, T>(&self, reader: R) -> io::Result<T>
    // where
    //     R: Read,
    //     T: DeserializeOwned,
    // {
    //     Ok(rmp_serde::decode::from_read(reader).map_err(|err| io::Error::other(err))?)
    // }
}
