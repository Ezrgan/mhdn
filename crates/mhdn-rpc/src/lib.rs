//! Client for Azahar's UDP RPC server (`127.0.0.1:45987`).

#![forbid(unsafe_code)]

mod client;
mod error;
mod packet;

pub use client::{ProcessInfo, ReadReq, RpcClient, DEFAULT_PIPELINE_WINDOW, DEFAULT_REQUEST_TIMEOUT};
pub use error::{Result, RpcError};
pub use packet::{
    PacketHeader, PacketType, MAX_PACKET_DATA_SIZE, MAX_PACKET_SIZE, PROTOCOL_VERSION,
};
