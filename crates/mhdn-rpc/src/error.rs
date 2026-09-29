use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("RPC response timed out after {0:?}")]
    Timeout(std::time::Duration),

    #[error("invalid or empty RPC response")]
    InvalidResponse,

    #[error("protocol version mismatch: expected {expected}, got {got}")]
    VersionMismatch { expected: u32, got: u32 },

    #[error("response id mismatch: expected {expected}, got {got}")]
    IdMismatch { expected: u32, got: u32 },

    #[error("response type mismatch: expected {expected:?}, got {got:?}")]
    TypeMismatch {
        expected: crate::packet::PacketType,
        got: crate::packet::PacketType,
    },

    #[error("response payload size mismatch: header says {declared}, payload is {actual}")]
    PayloadSizeMismatch { declared: u32, actual: usize },

    #[error("packet too short: {0} bytes")]
    PacketTooShort(usize),

    #[error("read size {requested} exceeds maximum chunk {max}")]
    ReadTooLarge { requested: usize, max: usize },

    #[error("memory read failed at address 0x{addr:08X}")]
    ReadFailed { addr: u32 },

    #[error("memory write was rejected or did not stick at address 0x{addr:08X}")]
    WriteRejected { addr: u32 },

    #[error("no process selected")]
    NoProcessSelected,

    #[error("pipelined read missing response for request id {0}")]
    MissingPipelinedResponse(u32),
}

pub type Result<T> = std::result::Result<T, RpcError>;
