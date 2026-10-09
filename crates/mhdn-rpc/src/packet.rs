//! Azahar/Citra RPC packet header and payload layout (see Azahar `src/core/rpc/packet.h`).

use crate::error::{Result, RpcError};

/// RPC protocol v1 (`CURRENT_VERSION` in Azahar 2125.x and 2126.1.x).
pub const PROTOCOL_VERSION_V1: u32 = 1;
/// RPC protocol v2 (Azahar 2126.2, `CURRENT_VERSION = 2`).
/// A later header number that does not change the RPC body keeps this payload size.
pub const PROTOCOL_VERSION_V2: u32 = 2;

/// Legacy alias: default on-the-wire version before auto-detection (v1).
pub const PROTOCOL_VERSION: u32 = PROTOCOL_VERSION_V1;

/// Max payload bytes after the 16-byte header (Azahar v1).
pub const MAX_PACKET_DATA_SIZE_V1: usize = 1024;
/// Max payload bytes after the 16-byte header (Azahar 2126.2).
pub const MAX_PACKET_DATA_SIZE_V2: usize = 32 * 1024;

/// Largest payload this client accepts (v2 ceiling).
pub const MAX_PACKET_DATA_SIZE: usize = MAX_PACKET_DATA_SIZE_V2;

const HEADER_SIZE: usize = 16;

/// Negotiated RPC dialect (version + per-datagram payload limit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpcProtocol {
    pub version: u32,
    pub max_packet_data_size: usize,
}

impl RpcProtocol {
    pub const V1: Self = Self {
        version: PROTOCOL_VERSION_V1,
        max_packet_data_size: MAX_PACKET_DATA_SIZE_V1,
    };

    pub const V2: Self = Self {
        version: PROTOCOL_VERSION_V2,
        max_packet_data_size: MAX_PACKET_DATA_SIZE_V2,
    };

    pub fn max_packet_size(self) -> usize {
        HEADER_SIZE + self.max_packet_data_size
    }

    pub fn max_write_chunk(self) -> usize {
        self.max_packet_data_size.saturating_sub(8)
    }

    /// Max process entries in one ProcessList reply (Azahar `MAX_PROCESSES_IN_LIST`).
    pub fn max_processes_in_list(self) -> usize {
        (self.max_packet_data_size - 4) / 0x14
    }

    /// Version 1 keeps the 1024-byte datagram. Any higher header number uses the
    /// 2126.2 body layout until a trace shows the RPC itself changed.
    pub fn for_version(version: u32) -> Self {
        if version <= PROTOCOL_VERSION_V1 {
            Self::V1
        } else if version == PROTOCOL_VERSION_V2 {
            Self::V2
        } else {
            Self {
                version,
                max_packet_data_size: MAX_PACKET_DATA_SIZE_V2,
            }
        }
    }
}

/// Maximum UDP datagram size accepted from the server (v2 ceiling).
pub const MAX_PACKET_SIZE: usize = HEADER_SIZE + MAX_PACKET_DATA_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PacketType {
    ReadMemory = 1,
    WriteMemory = 2,
    ProcessList = 3,
    SetGetProcess = 4,
    /// Azahar 2126.2+ only (not used by mhdn).
    TakeScreenshot = 5,
    ReadScreenshot = 6,
    GetPerfStats = 7,
}

impl PacketType {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::ReadMemory),
            2 => Some(Self::WriteMemory),
            3 => Some(Self::ProcessList),
            4 => Some(Self::SetGetProcess),
            5 => Some(Self::TakeScreenshot),
            6 => Some(Self::ReadScreenshot),
            7 => Some(Self::GetPerfStats),
            _ => None,
        }
    }
}

/// 16-byte header: `version, id, type, data_size` (all little-endian u32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    pub version: u32,
    pub id: u32,
    pub packet_type: PacketType,
    pub data_size: u32,
}

impl PacketHeader {
    pub fn new(id: u32, packet_type: PacketType, data_size: u32) -> Self {
        Self::new_with_version(PROTOCOL_VERSION_V1, id, packet_type, data_size)
    }

    pub fn new_with_version(
        version: u32,
        id: u32,
        packet_type: PacketType,
        data_size: u32,
    ) -> Self {
        Self {
            version,
            id,
            packet_type,
            data_size,
        }
    }

    pub fn encode(self) -> [u8; HEADER_SIZE] {
        let mut out = [0u8; HEADER_SIZE];
        out[0..4].copy_from_slice(&self.version.to_le_bytes());
        out[4..8].copy_from_slice(&self.id.to_le_bytes());
        out[8..12].copy_from_slice(&(self.packet_type as u32).to_le_bytes());
        out[12..16].copy_from_slice(&self.data_size.to_le_bytes());
        out
    }

    pub fn decode(raw: &[u8]) -> Result<Self> {
        if raw.len() < HEADER_SIZE {
            return Err(RpcError::PacketTooShort(raw.len()));
        }
        let version = u32::from_le_bytes(raw[0..4].try_into().expect("slice"));
        let id = u32::from_le_bytes(raw[4..8].try_into().expect("slice"));
        let ty_raw = u32::from_le_bytes(raw[8..12].try_into().expect("slice"));
        let data_size = u32::from_le_bytes(raw[12..16].try_into().expect("slice"));
        let packet_type = PacketType::from_u32(ty_raw).ok_or(RpcError::InvalidResponse)?;
        Ok(Self {
            version,
            id,
            packet_type,
            data_size,
        })
    }
}

/// Validate a full datagram and split header from payload.
pub fn split_datagram(raw: &[u8]) -> Result<(PacketHeader, &[u8])> {
    if raw.is_empty() {
        return Err(RpcError::InvalidResponse);
    }
    let header = PacketHeader::decode(raw)?;
    let payload = &raw[HEADER_SIZE..];
    if payload.len() != header.data_size as usize {
        return Err(RpcError::PayloadSizeMismatch {
            declared: header.data_size,
            actual: payload.len(),
        });
    }
    Ok((header, payload))
}

/// Validate response header against the outstanding request.
pub fn validate_response(
    raw: &[u8],
    protocol: RpcProtocol,
    expected_id: u32,
    expected_type: PacketType,
) -> Result<&[u8]> {
    let (header, payload) = split_datagram(raw)?;
    if header.version != protocol.version {
        return Err(RpcError::VersionMismatch {
            expected: protocol.version,
            got: header.version,
        });
    }
    if header.id != expected_id {
        return Err(RpcError::IdMismatch {
            expected: expected_id,
            got: header.id,
        });
    }
    if header.packet_type != expected_type {
        return Err(RpcError::TypeMismatch {
            expected: expected_type,
            got: header.packet_type,
        });
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = PacketHeader::new(42, PacketType::ReadMemory, 8);
        let encoded = h.encode();
        let decoded = PacketHeader::decode(&encoded).unwrap();
        assert_eq!(decoded, h);
    }

    #[test]
    fn split_valid_datagram() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&PacketHeader::new(1, PacketType::ProcessList, 4).encode());
        raw.extend_from_slice(&7u32.to_le_bytes());
        let (h, payload) = split_datagram(&raw).unwrap();
        assert_eq!(h.data_size, 4);
        assert_eq!(payload, 7u32.to_le_bytes());
    }

    #[test]
    fn rejects_payload_size_mismatch() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&PacketHeader::new(1, PacketType::ReadMemory, 8).encode());
        raw.extend_from_slice(&[1, 2, 3]);
        assert!(matches!(
            split_datagram(&raw),
            Err(RpcError::PayloadSizeMismatch { .. })
        ));
    }

    #[test]
    fn rejects_unknown_type() {
        let mut raw = [0u8; 16];
        raw[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            PacketHeader::decode(&raw),
            Err(RpcError::InvalidResponse)
        ));
    }

    #[test]
    fn validate_response_ok() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&PacketHeader::new(5, PacketType::SetGetProcess, 4).encode());
        raw.extend_from_slice(&123u32.to_le_bytes());
        let payload =
            validate_response(&raw, RpcProtocol::V1, 5, PacketType::SetGetProcess).unwrap();
        assert_eq!(payload, 123u32.to_le_bytes());
    }

    #[test]
    fn validate_response_id_mismatch() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&PacketHeader::new(5, PacketType::ReadMemory, 0).encode());
        assert!(matches!(
            validate_response(&raw, RpcProtocol::V1, 6, PacketType::ReadMemory),
            Err(RpcError::IdMismatch { .. })
        ));
    }

    #[test]
    fn versions_match_azahar_packet_h() {
        // 2126.1: CURRENT_VERSION = 1, MAX_PACKET_DATA_SIZE = 1024.
        // 2126.2: CURRENT_VERSION = 2, MAX_PACKET_DATA_SIZE = 32 * 1024.
        // MAX_PROCESSES_IN_LIST = (MAX_PACKET_DATA_SIZE - sizeof(u32)) / sizeof(ProcessInfo).
        assert_eq!(PROTOCOL_VERSION_V1, 1);
        assert_eq!(MAX_PACKET_DATA_SIZE_V1, 1024);
        assert_eq!(RpcProtocol::V1.max_processes_in_list(), 51);
        assert_eq!(RpcProtocol::V1.max_write_chunk(), 1024 - 8);
        assert_eq!(PROTOCOL_VERSION_V2, 2);
        assert_eq!(MAX_PACKET_DATA_SIZE_V2, 32 * 1024);
        assert_eq!(
            RpcProtocol::V2.max_processes_in_list(),
            (32 * 1024 - 4) / 0x14
        );
        assert_eq!(RpcProtocol::V2.max_write_chunk(), 32 * 1024 - 8);
        assert_eq!(RpcProtocol::for_version(1), RpcProtocol::V1);
        assert_eq!(RpcProtocol::for_version(2), RpcProtocol::V2);
        assert_eq!(RpcProtocol::for_version(5).version, 5);
        assert_eq!(
            RpcProtocol::for_version(5).max_packet_data_size,
            MAX_PACKET_DATA_SIZE_V2
        );
        assert_eq!(PacketType::ReadMemory as u32, 1);
        assert_eq!(PacketType::WriteMemory as u32, 2);
        assert_eq!(PacketType::ProcessList as u32, 3);
        assert_eq!(PacketType::SetGetProcess as u32, 4);
    }
}
