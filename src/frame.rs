use crate::{RpcError, RpcOpId};
use bit_field::{BitArray, BitField};
use bytes::Bytes;
use futures_util::{AsyncReadExt, AsyncWriteExt};
use modular_bitfield::prelude::{B4, B12, B32};
use modular_bitfield::{BitfieldSpecifier, Specifier, bitfield};
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};

pub type RpcMsgSendId = u16;
pub type RpcMsgKind = u16;
#[derive(BitfieldSpecifier, Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
#[bits = 2]
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

// pub const TAG_BITS_COUNT: usize = 8;
// pub const RPC_MSG_ID_BITS_COUNT: usize = 12;
// pub const RPC_MSG_SEND_ID_BITS_COUNT: usize = 12;

// #[derive(Debug)]
pub struct RpcFrame {
    pub head: RpcFrameHead,
    pub payload: Bytes,
}

impl Debug for RpcFrame {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcFrame")
            .field("head", &self.head)
            .field("payload", &String::from_utf8_lossy(&self.payload))
            .finish()
    }
}

impl RpcFrame {
    pub fn out_stream_end(op: RpcOpId) -> Self {
        Self {
            head: RpcFrameHead::Rpc(
                RpcFrameHeadRpc::new()
                    .with_reply(true)
                    .with_msg_id(op.msg_id)
                    .with_msg_kind(op.msg_kind)
                    .with_stream(RpcStreamKind::StreamEnd)
                    .with_exclusive(false)
                    .with_payload_len(0),
            ),
            payload: Default::default(),
        }
    }
    pub fn cancel(op: RpcOpId) -> Self {
        Self {
            head: RpcFrameHead::CancelRpc(
                RpcFrameHeadCancelRpc::new()
                    .with_msg_id(op.msg_id)
                    .with_msg_kind(op.msg_kind),
            ),
            payload: Default::default(),
        }
    }
}

#[derive(BitfieldSpecifier)]
#[bits = 4]
pub enum RpcFrameKind {
    Msg,
    Rpc,
    CancelRpc,
}

#[bitfield(bits = 64)]
#[derive(Debug, Clone, PartialEq)]
pub struct RpcFrameHeadMsg {
    #[skip]
    __: B4,
    #[skip]
    __: B4,
    pub id: B12,
    #[skip]
    __: B12,
    pub payload_len: u32,
}

#[bitfield(bits = 64)]
#[derive(Debug, Clone, PartialEq)]
pub struct RpcFrameHeadRpc {
    #[skip]
    pub __: B4,
    pub reply: bool,
    pub exclusive: bool,
    pub stream: RpcStreamKind,
    pub msg_kind: B12,
    pub msg_id: B12,
    pub payload_len: u32,
}

impl RpcFrameHeadRpc {
    pub fn op_id(&self) -> RpcOpId {
        RpcOpId {
            msg_kind: self.msg_kind(),
            msg_id: self.msg_id(),
        }
    }
}

#[bitfield(bits = 64)]
#[derive(Debug, Clone, PartialEq)]
pub struct RpcFrameHeadCancelRpc {
    #[skip]
    __: B4,
    #[skip]
    __: B4,
    pub msg_kind: B12,
    pub msg_id: B12,
    __: B32,
}

impl RpcFrameHeadCancelRpc {
    pub fn op_id(&self) -> RpcOpId {
        RpcOpId {
            msg_kind: self.msg_kind(),
            msg_id: self.msg_id(),
        }
    }
}

pub type RpcFrameHeadBits = [u8; 8];

#[derive(Debug, Clone, PartialEq)]
pub enum RpcFrameHead {
    Msg(RpcFrameHeadMsg),
    Rpc(RpcFrameHeadRpc),
    CancelRpc(RpcFrameHeadCancelRpc),
}

impl RpcFrameHead {
    pub fn payload_len(&self) -> u32 {
        match &self {
            RpcFrameHead::Msg(n) => n.payload_len(),
            RpcFrameHead::Rpc(n) => n.payload_len(),
            RpcFrameHead::CancelRpc { .. } => 0,
        }
    }
}

impl From<RpcFrameHeadBits> for RpcFrameHead {
    fn from(bits: RpcFrameHeadBits) -> Self {
        let kind = RpcFrameKind::from_bytes(bits.get_bits(0..4)).unwrap();
        match kind {
            RpcFrameKind::Msg => RpcFrameHead::Msg(RpcFrameHeadMsg::from_bytes(bits)),
            RpcFrameKind::Rpc => RpcFrameHead::Rpc(RpcFrameHeadRpc::from_bytes(bits)),
            RpcFrameKind::CancelRpc => {
                RpcFrameHead::CancelRpc(RpcFrameHeadCancelRpc::from_bytes(bits))
            }
        }
    }
}

impl Into<RpcFrameHeadBits> for RpcFrameHead {
    fn into(self) -> RpcFrameHeadBits {
        match self {
            RpcFrameHead::Msg(head) => {
                let mut bits = head.into_bytes();
                bits.set_bits(0..4, RpcFrameKind::Msg as _);
                bits
            }
            RpcFrameHead::Rpc(head) => {
                let mut bits = head.into_bytes();
                bits.set_bits(0..4, RpcFrameKind::Rpc as _);
                bits
            }
            RpcFrameHead::CancelRpc(head) => {
                let mut bits = head.into_bytes();
                bits.set_bits(0..4, RpcFrameKind::CancelRpc as _);
                bits
            }
        }
    }
}

pub async fn write_frame(
    mut write: impl futures_util::AsyncWrite + Unpin,
    frame: RpcFrameHead,
) -> Result<(), RpcError> {
    // println!("write frame: {frame:#?}");
    let bits: RpcFrameHeadBits = frame.into();
    write.write_all(bits.as_slice()).await?;
    // println!("write frame end");
    Ok(())
}

pub async fn read_frame(
    mut read: impl futures_util::AsyncRead + Unpin,
) -> Option<Result<RpcFrameHead, RpcError>> {
    let mut bits = [0u8; 8];
    let r = read.read_exact(&mut bits[..]).await;
    // println!("r: {r:?}");
    println!("bits: {bits:?}");
    if let Err(err) = r {
        if matches!(err.kind(), std::io::ErrorKind::UnexpectedEof) && bits.iter().all(|b| *b == 0) {
            None
        } else {
            Some(Err(err.into()))
        }
    } else {
        Some(Ok(bits.into()))
    }
}

#[cfg(test)]
mod test {
    use crate::frame::{
        RpcFrameHead, RpcFrameHeadCancelRpc, RpcFrameHeadMsg, RpcFrameHeadRpc, RpcStreamKind,
    };

    #[test]
    pub fn test_frame() {
        let origin_head = RpcFrameHead::Rpc(
            RpcFrameHeadRpc::new()
                .with_msg_id(1)
                .with_payload_len(10)
                .with_msg_kind(1)
                .with_stream(RpcStreamKind::None)
                .with_exclusive(false)
                .with_reply(false),
        );
        let head: RpcFrameHead = origin_head.clone().into();
        assert_eq!(head, origin_head);

        let origin_head = RpcFrameHead::Msg(RpcFrameHeadMsg::new().with_id(1).with_payload_len(10));
        let head: RpcFrameHead = origin_head.clone().into();
        assert_eq!(head, origin_head);

        let origin_head = RpcFrameHead::CancelRpc(
            RpcFrameHeadCancelRpc::new()
                .with_msg_id(12)
                .with_msg_kind(13),
        );
        let head: RpcFrameHead = origin_head.clone().into();
        assert_eq!(head, origin_head);
    }
}
