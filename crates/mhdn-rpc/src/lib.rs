//! Client for Azahar's UDP RPC server (`127.0.0.1:45987`).

#![forbid(unsafe_code)]

mod client;
mod error;
mod memory_source;
mod packet;
mod pipeline;

#[doc(hidden)]
pub mod fake_server;

pub use client::{
    ProcessInfo, ReadReq, RpcClient, DEFAULT_PIPELINE_WINDOW, DEFAULT_REQUEST_TIMEOUT,
};
pub use error::{Result, RpcError};
pub use memory_source::{read_dump_range, FileMemorySource, MemorySource, ReplaySource};
pub use packet::{
    PacketHeader, PacketType, RpcProtocol, MAX_PACKET_DATA_SIZE, MAX_PACKET_SIZE, PROTOCOL_VERSION,
};
