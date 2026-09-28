//! Azahar/Citra RPC packet header and payload layout (see `dist/scripting/citra.py`).

use crate::error::{Result, RpcError};

/// Protocol version shared by request and response headers.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum bytes of payload after the 16-byte header.
pub const MAX_PACKET_DATA_SIZE: usize = 1024;

/// Maximum UDP datagram size accepted from the server.
pub const MAX_PACKET_SIZE: usize = MAX_PACKET_DATA_SIZE + 16;

const HEADER_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PacketType {
    ReadMemory = 1,
    WriteMemory = 2,
    ProcessList = 3,
    SetGetProcess = 4,
}

impl PacketType {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::ReadMemory),
            2 => Some(Self::WriteMemory),
            3 => Some(Self::ProcessList),
            4 => Some(Self::SetGetProcess),
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
        Self {
            version: PROTOCOL_VERSION,
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
pub fn validate_response(raw: &[u8], expected_id: u32, expected_type: PacketType) -> Result<&[u8]> {
    let (header, payload) = split_datagram(raw)?;
    if header.version != PROTOCOL_VERSION {
        return Err(RpcError::VersionMismatch {
            expected: PROTOCOL_VERSION,
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
        let payload = validate_response(&raw, 5, PacketType::SetGetProcess).unwrap();
        assert_eq!(payload, 123u32.to_le_bytes());
    }

    #[test]
    fn validate_response_id_mismatch() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&PacketHeader::new(5, PacketType::ReadMemory, 0).encode());
        assert!(matches!(
            validate_response(&raw, 6, PacketType::ReadMemory),
            Err(RpcError::IdMismatch { .. })
        ));
    }
}
