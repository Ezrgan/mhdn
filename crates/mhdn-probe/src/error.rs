use std::io;

use mhdn_rpc::RpcError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error(transparent)]
    Rpc(#[from] RpcError),

    #[error(transparent)]
    Profile(#[from] mhdn_game::ProfileError),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("{0}")]
    Msg(String),
}

impl ProbeError {
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Msg(message.into())
    }
}

pub type Result<T> = std::result::Result<T, ProbeError>;
