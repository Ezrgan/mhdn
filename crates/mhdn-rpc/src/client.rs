use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use crate::error::{Result, RpcError};
use crate::packet::{
    self, PacketHeader, PacketType, RpcProtocol, MAX_PACKET_SIZE, PROTOCOL_VERSION_V1,
    PROTOCOL_VERSION_V2,
};

/// One process entry returned by [`RpcClient::list_processes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub title_id: u64,
    pub name: [u8; 8],
}

impl ProcessInfo {
    pub fn name_str(&self) -> &str {
        let end = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        std::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

/// A single memory read in a pipelined batch ([`RpcClient::read_many`]).
pub struct ReadReq<'a> {
    pub addr: u32,
    pub buf: &'a mut [u8],
}

/// Default in-flight request window for pipelining.
pub const DEFAULT_PIPELINE_WINDOW: usize = 16;

/// Per-request timeout when waiting for a UDP reply.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_millis(20);

const PROCESS_ENTRY_SIZE: usize = 0x14;

pub struct RpcClient {
    socket: UdpSocket,
    server: SocketAddr,
    timeout: Duration,
    protocol: RpcProtocol,
    next_id: u32,
    pipeline_window: usize,
}

impl RpcClient {
    pub fn connect(addr: SocketAddr, timeout: Duration) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect(addr)?;
        socket.set_read_timeout(Some(timeout))?;
        let protocol = detect_protocol(&socket, timeout)?;
        socket.set_read_timeout(Some(timeout))?;
        Ok(Self {
            socket,
            server: addr,
            timeout,
            protocol,
            next_id: 1,
            pipeline_window: DEFAULT_PIPELINE_WINDOW,
        })
    }

    /// Negotiated dialect. Version 1 is Azahar through 2126.1. Version 2 is 2126.2.
    /// A higher number is a later build that kept the same RPC body.
    pub fn protocol(&self) -> RpcProtocol {
        self.protocol
    }

    pub fn set_pipeline_window(&mut self, window: usize) {
        self.pipeline_window = window.max(1);
    }

    pub fn server_addr(&self) -> SocketAddr {
        self.server
    }

    pub(crate) fn pipeline_window(&self) -> usize {
        self.pipeline_window
    }

    pub(crate) fn request_timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn next_request_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id == 0 {
            self.next_id = 1;
        }
        id
    }

    pub(crate) fn send_request(
        &mut self,
        id: u32,
        packet_type: PacketType,
        payload: &[u8],
    ) -> Result<()> {
        let header = PacketHeader::new_with_version(
            self.protocol.version,
            id,
            packet_type,
            payload.len() as u32,
        );
        let mut datagram = Vec::with_capacity(16 + payload.len());
        datagram.extend_from_slice(&header.encode());
        datagram.extend_from_slice(payload);
        self.socket.send(&datagram)?;
        Ok(())
    }

    /// Replies to requests that already timed out stay queued on the socket. Skip them, or every
    /// later request would read its predecessor's reply.
    fn recv_response(&mut self, expected_id: u32, expected_type: PacketType) -> Result<Vec<u8>> {
        let mut buf = [0u8; MAX_PACKET_SIZE];
        for _ in 0..MAX_STALE_REPLIES {
            let len = match self.socket.recv(&mut buf) {
                Ok(n) => n,
                Err(e) if is_timeout(&e) => return Err(RpcError::Timeout(self.timeout)),
                Err(e) => return Err(e.into()),
            };
            if len == 0 {
                return Err(RpcError::InvalidResponse);
            }
            match packet::validate_response(&buf[..len], self.protocol, expected_id, expected_type)
            {
                Ok(payload) => return Ok(payload.to_vec()),
                Err(RpcError::IdMismatch { .. }) => continue,
                Err(err) => return Err(err),
            }
        }
        Err(RpcError::Timeout(self.timeout))
    }

    fn exchange(
        &mut self,
        packet_type: PacketType,
        payload: &[u8],
        retries: u32,
    ) -> Result<Vec<u8>> {
        let id = self.next_request_id();
        let mut attempts = 0;
        loop {
            self.send_request(id, packet_type, payload)?;
            match self.recv_response(id, packet_type) {
                Ok(data) => return Ok(data),
                Err(e) if attempts < retries && should_retry(&e) => {
                    attempts += 1;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub fn list_processes(&mut self) -> Result<Vec<ProcessInfo>> {
        let mut processes = Vec::new();
        let mut read_processes = 0u32;
        loop {
            let req_payload = read_processes
                .to_le_bytes()
                .into_iter()
                .chain(0x7FFF_FFFFu32.to_le_bytes());
            let payload: Vec<u8> = req_payload.collect();
            let reply = self.exchange(PacketType::ProcessList, &payload, 1)?;
            if reply.len() < 4 {
                break;
            }
            let read_count = u32::from_le_bytes(reply[0..4].try_into().expect("slice"));
            if read_count == 0 {
                break;
            }
            let entries = &reply[4..];
            let expected_len = read_count as usize * PROCESS_ENTRY_SIZE;
            if entries.len() < expected_len {
                return Err(RpcError::InvalidResponse);
            }
            for i in 0..read_count as usize {
                let off = i * PROCESS_ENTRY_SIZE;
                let chunk = &entries[off..off + PROCESS_ENTRY_SIZE];
                let pid = u32::from_le_bytes(chunk[0..4].try_into().expect("slice"));
                let title_id = u64::from_le_bytes(chunk[4..12].try_into().expect("slice"));
                let mut name = [0u8; 8];
                name.copy_from_slice(&chunk[12..20]);
                processes.push(ProcessInfo {
                    pid,
                    title_id,
                    name,
                });
            }
            read_processes += read_count;
        }
        Ok(processes)
    }

    pub fn select_process(&mut self, pid: u32) -> Result<()> {
        let payload = [1u32.to_le_bytes(), pid.to_le_bytes()].concat();
        self.exchange(PacketType::SetGetProcess, &payload, 1)?;
        Ok(())
    }

    pub fn selected_process(&mut self) -> Result<u32> {
        let payload = [0u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
        let reply = self.exchange(PacketType::SetGetProcess, &payload, 1)?;
        if reply.len() < 4 {
            return Err(RpcError::InvalidResponse);
        }
        Ok(u32::from_le_bytes(reply[0..4].try_into().expect("slice")))
    }

    pub fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let mut offset = 0usize;
        let mut cur_addr = addr;
        while offset < buf.len() {
            let chunk = (buf.len() - offset).min(self.protocol.max_packet_data_size);
            let req = [cur_addr.to_le_bytes(), (chunk as u32).to_le_bytes()].concat();
            let reply = self
                .exchange(PacketType::ReadMemory, &req, 1)
                .map_err(|e| match e {
                    RpcError::InvalidResponse | RpcError::ReadFailed { .. } => e,
                    other => other,
                })?;
            if reply.len() != chunk {
                return Err(RpcError::ReadFailed { addr: cur_addr });
            }
            buf[offset..offset + chunk].copy_from_slice(&reply);
            offset += chunk;
            cur_addr += chunk as u32;
        }
        Ok(())
    }

    pub(crate) fn recv_datagram(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.socket.recv(buf)
    }

    /// Write `data` at `addr`. Azahar replies with an empty payload both when the
    /// write lands and when it rejects the address, so this reads the bytes back
    /// and returns [`RpcError::WriteRejected`] if they do not match.
    ///
    /// Each datagram carries at most [`MAX_PACKET_DATA_SIZE`] minus the 8-byte
    /// address and size header.
    pub fn write(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        let chunk_max = self.protocol.max_write_chunk();
        let mut offset = 0usize;
        let mut cur_addr = addr;
        while offset < data.len() {
            let chunk = (data.len() - offset).min(chunk_max);
            let mut payload = Vec::with_capacity(8 + chunk);
            payload.extend_from_slice(&cur_addr.to_le_bytes());
            payload.extend_from_slice(&(chunk as u32).to_le_bytes());
            payload.extend_from_slice(&data[offset..offset + chunk]);
            let reply = self.exchange(PacketType::WriteMemory, &payload, 1)?;
            if !reply.is_empty() {
                return Err(RpcError::InvalidResponse);
            }
            offset += chunk;
            cur_addr = cur_addr.wrapping_add(chunk as u32);
        }
        let mut got = vec![0u8; data.len()];
        self.read(addr, &mut got)?;
        if got != data {
            return Err(RpcError::WriteRejected { addr });
        }
        Ok(())
    }

    pub fn read_u32(&mut self, addr: u32) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.read(addr, &mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    pub fn read_f32x3(&mut self, addr: u32) -> Result<[f32; 3]> {
        let mut buf = [0u8; 12];
        self.read(addr, &mut buf)?;
        Ok([
            f32::from_le_bytes(buf[0..4].try_into().expect("slice")),
            f32::from_le_bytes(buf[4..8].try_into().expect("slice")),
            f32::from_le_bytes(buf[8..12].try_into().expect("slice")),
        ])
    }
}

/// Upper bound on stale datagrams drained while waiting for one reply.
const MAX_STALE_REPLIES: usize = 256;

/// macOS reports an expired `SO_RCVTIMEO` as `WouldBlock`, Linux and Windows as `TimedOut`.
pub(crate) fn is_timeout(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn should_retry(err: &RpcError) -> bool {
    matches!(
        err,
        RpcError::Timeout(_) | RpcError::InvalidResponse | RpcError::Io(_)
    )
}

/// Smallest ProcessList body Azahar writes on success: a `u32` count, even when the count is 0.
const MIN_PROCESS_LIST_REPLY: usize = 4;

/// Last header number tried when a rejection echoes our version instead of naming its own.
const MAX_PROBED_VERSION: u32 = 16;

const PROBE_ID: u32 = 0x7FFF_FFFE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeReply {
    /// ProcessList body of at least 4 bytes, header version equal to the one we sent.
    Accepted,
    /// Same id, `packet_size == 0`. `version` is whatever the server wrote in the header.
    Rejected { version: u32 },
}

/// Pick the header version the server answers with a real process list.
///
/// Azahar 2126.2 (`src/core/rpc/udp_server.cpp` `SendReply`, tag 2126.2) copies the
/// request `PacketHeader` and `SetPacketDataSize` only writes `packet_size`. A
/// failed `ValidatePacket` (`rpc_server.cpp`) therefore echoes the version the
/// client sent, with an empty body. It does not write `CURRENT_VERSION`.
///
/// Version 2 is tried first. A timeout means nothing is listening, so no other
/// version is tried. A list body selects version 2 and stops. An empty body is a
/// rejection, not "no games".
///
/// If that rejection's header version is neither the one we sent nor 0, the server
/// is naming its own version: retry that value once and do not sweep. Otherwise
/// try version 1, then 3, 4, … through 16, and stop at the first list body.
fn detect_protocol(socket: &UdpSocket, timeout: Duration) -> Result<RpcProtocol> {
    match probe_process_list(socket, PROTOCOL_VERSION_V2, timeout)? {
        ProbeReply::Accepted => return Ok(RpcProtocol::for_version(PROTOCOL_VERSION_V2)),
        ProbeReply::Rejected { version } if version != PROTOCOL_VERSION_V2 && version != 0 => {
            return accept_announced_version(socket, version, timeout);
        }
        ProbeReply::Rejected { .. } => {}
    }
    for version in std::iter::once(PROTOCOL_VERSION_V1).chain(3..=MAX_PROBED_VERSION) {
        if probe_process_list(socket, version, timeout)? == ProbeReply::Accepted {
            return Ok(RpcProtocol::for_version(version));
        }
    }
    Err(RpcError::InvalidResponse)
}

/// One retry with the version a rejection header named. A second miss does not sweep.
fn accept_announced_version(
    socket: &UdpSocket,
    version: u32,
    timeout: Duration,
) -> Result<RpcProtocol> {
    match probe_process_list(socket, version, timeout)? {
        ProbeReply::Accepted => Ok(RpcProtocol::for_version(version)),
        ProbeReply::Rejected { .. } => Err(RpcError::InvalidResponse),
    }
}

fn probe_process_list(socket: &UdpSocket, version: u32, timeout: Duration) -> Result<ProbeReply> {
    let payload: Vec<u8> = [0u32.to_le_bytes(), 0x7FFF_FFFFu32.to_le_bytes()].concat();
    let header = PacketHeader::new_with_version(
        version,
        PROBE_ID,
        PacketType::ProcessList,
        payload.len() as u32,
    );
    let mut datagram = Vec::with_capacity(16 + payload.len());
    datagram.extend_from_slice(&header.encode());
    datagram.extend_from_slice(&payload);
    socket.send(&datagram)?;

    let mut buf = [0u8; MAX_PACKET_SIZE];
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(RpcError::Timeout(timeout));
        }
        socket.set_read_timeout(Some(remaining))?;
        let len = match socket.recv(&mut buf) {
            Ok(n) => n,
            Err(e) if is_timeout(&e) => return Err(RpcError::Timeout(timeout)),
            Err(e) => return Err(e.into()),
        };
        if len == 0 {
            continue;
        }
        let (header, payload) = packet::split_datagram(&buf[..len])?;
        if header.id != PROBE_ID {
            continue;
        }
        if header.packet_type != PacketType::ProcessList {
            return Err(RpcError::TypeMismatch {
                expected: PacketType::ProcessList,
                got: header.packet_type,
            });
        }
        if payload.len() >= MIN_PROCESS_LIST_REPLY && header.version == version {
            return Ok(ProbeReply::Accepted);
        }
        if payload.is_empty() {
            return Ok(ProbeReply::Rejected {
                version: header.version,
            });
        }
        return Err(RpcError::InvalidResponse);
    }
}
