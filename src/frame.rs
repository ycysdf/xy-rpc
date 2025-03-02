use crate::RpcError;
use bit_field::BitField;
use bytes::Bytes;
use futures_util::{AsyncReadExt, AsyncWriteExt};
use serde::{Deserialize, Serialize};

pub type RpcMsgSendId = u16;
pub type RpcMsgKind = u16;

#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
#[repr(u8)]
pub enum RpcStreamKind {
    None = 0,
    StreamStart = 1,
    StreamItem = 2,
    StreamEnd = 3,
}

impl Into<u8> for RpcStreamKind {
    fn into(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for RpcStreamKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::None,
            1 => Self::StreamStart,
            2 => Self::StreamItem,
            3 => Self::StreamEnd,
            _ => return Err(()),
        })
    }
}

pub const TAG_BITS_COUNT: usize = 8;
pub const RPC_MSG_ID_BITS_COUNT: usize = 12;
pub const RPC_MSG_SEND_ID_BITS_COUNT: usize = 12;

pub struct RpcFrame {
    pub head: RpcFrameHead,
    pub payload: Bytes,
}

#[derive(Debug,Serialize, Deserialize)]
pub enum RpcFrameHead {
    Msg {
        id: RpcMsgKind,
        payload_len: u32,
    },
    Rpc {
        reply: bool,
        exclusive: bool,
        stream: RpcStreamKind,
        id: RpcMsgKind,
        send_id: RpcMsgSendId,
        payload_len: u32,
    },
}

impl TryFrom<u64> for RpcFrameHead {
    type Error = RpcError;

    fn try_from(bits: u64) -> Result<Self, Self::Error> {
        match bits.get_bits(0..4) {
            0 => {
                let id_bits_range = TAG_BITS_COUNT..(TAG_BITS_COUNT + RPC_MSG_ID_BITS_COUNT);
                let id = bits.get_bits(id_bits_range.clone()) as _;
                // let msg_send_id = bits
                //     .get_bits(msg_id_bits_range.end..(msg_id_bits_range.end + RPC_MSG_SEND_ID_BITS_COUNT));
                // assert_eq!(msg_id_bits_range.end + RPC_MSG_SEND_ID_BITS_COUNT, 32);
                let payload_len = bits.get_bits(32..) as _;
                Ok(RpcFrameHead::Msg { id, payload_len })
            }
            1 => {
                let reply = bits.get_bit(5);
                let sequence = bits.get_bit(6);
                let stream: RpcStreamKind = (bits.get_bits(7..8) as u8).try_into().unwrap();
                let id_bits_range = TAG_BITS_COUNT..(TAG_BITS_COUNT + RPC_MSG_ID_BITS_COUNT);
                let id = bits.get_bits(id_bits_range.clone()) as _;
                let send_id = bits
                    .get_bits(id_bits_range.end..(id_bits_range.end + RPC_MSG_SEND_ID_BITS_COUNT))
                    as _;
                assert_eq!(id_bits_range.end + RPC_MSG_SEND_ID_BITS_COUNT, 32);

                let payload_len = bits.get_bits(32..) as _;
                Ok(RpcFrameHead::Rpc {
                    reply,
                    exclusive: sequence,
                    stream,
                    id,
                    send_id,
                    payload_len,
                })
            }
            _ => Err(RpcError::InvalidFrame),
        }
    }
}

impl Into<u64> for &RpcFrameHead {
    fn into(self) -> u64 {
        match self {
            RpcFrameHead::Msg { id, payload_len } => {
                let mut bits = 0u64;
                bits.set_bits(0..4, 0);
                bits.set_bits(
                    TAG_BITS_COUNT..(TAG_BITS_COUNT + RPC_MSG_ID_BITS_COUNT),
                    *id as u64,
                );
                bits.set_bits(32.., *payload_len as u64);
                bits
            }
            RpcFrameHead::Rpc {
                reply,
                exclusive: sequence,
                stream,
                id,
                send_id,
                payload_len,
            } => {
                let mut bits = 0u64;
                bits.set_bits(0..4, 1);
                bits.set_bit(5, *reply);
                bits.set_bit(6, *sequence);
                bits.set_bits(7..8, *stream as u8 as _);
                let id_bits_range = TAG_BITS_COUNT..(TAG_BITS_COUNT + RPC_MSG_ID_BITS_COUNT);
                bits.set_bits(TAG_BITS_COUNT..id_bits_range.end, *id as u64);
                bits.set_bits(
                    id_bits_range.end..(id_bits_range.end + RPC_MSG_SEND_ID_BITS_COUNT),
                    *send_id as u64,
                );
                bits.set_bits(32.., *payload_len as u64);
                bits
            }
        }
    }
}

pub async fn write_frame(
    mut write: impl futures_util::AsyncWrite + Unpin,
    frame: &RpcFrameHead,
) -> Result<(), RpcError> {
    // println!("write frame: {frame:#?}");
    let bits: u64 = frame.into();
    write.write_all(bits.to_be_bytes().as_slice()).await?;
    // println!("write frame end");
    Ok(())
}

pub async fn read_frame(
    mut read: impl futures_util::AsyncRead + Unpin,
) -> Option<Result<RpcFrameHead, RpcError>> {
    let mut bits = [0u8; 8];
    let r = read.read_exact(&mut bits[..]).await;
    let bits = u64::from_be_bytes(bits);
    if let Err(err) = r {
        if matches!(err.kind(), std::io::ErrorKind::UnexpectedEof) && bits == 0 {
            None
        } else {
            Some(Err(err.into()))
        }
    } else {
        Some(bits.try_into())
    }
}
