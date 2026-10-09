//! UDP fake Azahar RPC server for deterministic tests (not used in production).

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::packet::{
    PacketHeader, PacketType, RpcProtocol, MAX_PACKET_SIZE, PROTOCOL_VERSION_V1,
    PROTOCOL_VERSION_V2,
};

#[derive(Debug, Clone)]
pub struct FakeProcess {
    pub pid: u32,
    pub title_id: u64,
    pub name: [u8; 8],
}

pub struct FakeRpcServer {
    addr: std::net::SocketAddr,
    stop: Arc<Mutex<bool>>,
    handle: Option<thread::JoinHandle<()>>,
}

/// Mutable fake server configuration (returned from [`FakeRpcServer::bind`] for tests).
#[derive(Default)]
pub struct ServerState {
    pub memory: HashMap<u32, Vec<u8>>,
    pub processes: Vec<FakeProcess>,
    pub selected_pid: u32,
    pub latency: Duration,
    pub drop_replies: bool,
    pub empty_replies: bool,
    pub reorder_replies: bool,
    /// Count of ReadMemory requests that passed the version and size checks.
    pub read_requests: u32,
    /// Answer every request the way `ValidatePacket` fails: echoed header, zero-byte body.
    pub reject_all: bool,
    pending_replies: Vec<Vec<u8>>,
}

impl FakeRpcServer {
    /// Azahar 2125.x / 2126.1.x: `version <= 1`, 1024-byte payloads.
    pub fn bind() -> (Self, Arc<Mutex<ServerState>>) {
        Self::bind_protocol(RpcProtocol::V1)
    }

    /// `V1` matches Azahar through 2126.1.x. `V2` matches Azahar 2126.2 (`version == 2`).
    pub fn bind_protocol(protocol: RpcProtocol) -> (Self, Arc<Mutex<ServerState>>) {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = socket.local_addr().expect("addr");
        let state = Arc::new(Mutex::new(ServerState::default()));
        let stop = Arc::new(Mutex::new(false));
        let state_thread = Arc::clone(&state);
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || run_server(socket, state_thread, stop_thread, protocol));
        (
            Self {
                addr,
                stop,
                handle: Some(handle),
            },
            state,
        )
    }

    pub fn addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    pub fn shutdown(mut self) {
        *self.stop.lock().unwrap() = true;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for FakeRpcServer {
    fn drop(&mut self) {
        *self.stop.lock().unwrap() = true;
    }
}

fn run_server(
    socket: UdpSocket,
    state: Arc<Mutex<ServerState>>,
    stop: Arc<Mutex<bool>>,
    protocol: RpcProtocol,
) {
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok();
    let mut buf = [0u8; MAX_PACKET_SIZE];
    loop {
        if *stop.lock().unwrap() {
            break;
        }
        let (len, peer) = match socket.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => break,
        };
        if len > protocol.max_packet_size() {
            continue;
        }
        let raw = &buf[..len];
        let header = match PacketHeader::decode(raw) {
            Ok(h) => h,
            Err(_) => continue,
        };
        if raw.len() != 16 + header.data_size as usize {
            continue;
        }
        let payload = &raw[16..];
        let mut st = state.lock().unwrap();
        if st.latency > Duration::ZERO {
            thread::sleep(st.latency);
        }
        if st.drop_replies {
            continue;
        }
        if st.empty_replies {
            let _ = socket.send_to(&[], peer);
            continue;
        }
        let reply = build_reply(&mut st, protocol, header, payload);
        if st.reorder_replies {
            st.pending_replies.push(reply);
            if st.pending_replies.len() >= 2 {
                let second = st.pending_replies.pop().expect("len checked");
                let first = st.pending_replies.pop().expect("len checked");
                let _ = socket.send_to(&second, peer);
                let _ = socket.send_to(&first, peer);
            }
        } else {
            let _ = socket.send_to(&reply, peer);
        }
    }
}

/// Same start-address check as `RPCServer::HandleWriteMemory` in Azahar
/// (`src/core/rpc/rpc_server.cpp`). The New 3DS linear alias at `0x30000000`
/// is not in that list.
pub fn write_allowed(addr: u32) -> bool {
    const REGIONS: [(u32, u32); 4] = [
        (0x0010_0000, 0x0400_0000),
        (0x0800_0000, 0x1000_0000),
        (0x1400_0000, 0x1C00_0000),
        (0x1E80_0000, 0x1EC0_0000),
    ];
    REGIONS
        .iter()
        .any(|&(start, end)| addr >= start && addr <= end)
}

fn apply_write(memory: &mut HashMap<u32, Vec<u8>>, addr: u32, data: &[u8]) {
    let existing = memory.iter_mut().find(|(base, buf)| {
        let base = **base;
        (addr >= base && (addr as usize) < (base as usize).saturating_add(buf.len()))
            || addr == base.wrapping_add(buf.len() as u32)
    });
    if let Some((base, buf)) = existing {
        let off = (addr - *base) as usize;
        let end = off + data.len();
        if end > buf.len() {
            buf.resize(end, 0);
        }
        buf[off..end].copy_from_slice(data);
        return;
    }
    memory.insert(addr, data.to_vec());
}

/// Azahar ≤2126.1 accepts `version <= 1`. Azahar 2126.2 accepts only `version == 2`.
/// Both require a known opcode and at least two `u32` arguments. Failure is an empty body.
fn request_accepted(protocol: RpcProtocol, header: &PacketHeader, payload: &[u8]) -> bool {
    let version_ok = match protocol.version {
        PROTOCOL_VERSION_V1 => header.version <= PROTOCOL_VERSION_V1,
        PROTOCOL_VERSION_V2 => header.version == PROTOCOL_VERSION_V2,
        _ => false,
    };
    if !version_ok || payload.len() != header.data_size as usize || header.data_size < 8 {
        return false;
    }
    matches!(
        header.packet_type,
        PacketType::ReadMemory
            | PacketType::WriteMemory
            | PacketType::ProcessList
            | PacketType::SetGetProcess
    )
}

fn encode_reply(version: u32, id: u32, packet_type: PacketType, payload: &[u8]) -> Vec<u8> {
    let hdr = PacketHeader::new_with_version(version, id, packet_type, payload.len() as u32);
    let mut datagram = Vec::with_capacity(16 + payload.len());
    datagram.extend_from_slice(&hdr.encode());
    datagram.extend_from_slice(payload);
    datagram
}

fn build_reply(
    st: &mut ServerState,
    protocol: RpcProtocol,
    header: PacketHeader,
    payload: &[u8],
) -> Vec<u8> {
    if st.reject_all || !request_accepted(protocol, &header, payload) {
        return encode_reply(header.version, header.id, header.packet_type, &[]);
    }
    let mut out_payload = Vec::new();
    match header.packet_type {
        PacketType::ProcessList => {
            let start = if payload.len() >= 4 {
                u32::from_le_bytes(payload[0..4].try_into().expect("slice")) as usize
            } else {
                0
            };
            let slice = st.processes.get(start..).unwrap_or(&[]);
            let count = slice.len().min(protocol.max_processes_in_list());
            out_payload.extend_from_slice(&(count as u32).to_le_bytes());
            for p in &slice[..count] {
                out_payload.extend_from_slice(&p.pid.to_le_bytes());
                out_payload.extend_from_slice(&p.title_id.to_le_bytes());
                out_payload.extend_from_slice(&p.name);
            }
        }
        PacketType::SetGetProcess => {
            if payload.len() >= 8 {
                let op = u32::from_le_bytes(payload[0..4].try_into().expect("slice"));
                if op == 1 {
                    st.selected_pid = u32::from_le_bytes(payload[4..8].try_into().expect("slice"));
                }
            }
            out_payload.extend_from_slice(&st.selected_pid.to_le_bytes());
        }
        PacketType::ReadMemory if payload.len() >= 8 => {
            let addr = u32::from_le_bytes(payload[0..4].try_into().expect("slice"));
            let size = u32::from_le_bytes(payload[4..8].try_into().expect("slice")) as usize;
            if size > 0 && size <= protocol.max_packet_data_size {
                st.read_requests = st.read_requests.wrapping_add(1);
                out_payload.resize(size, 0);
                if let Some(bytes) = st.memory.get(&addr) {
                    let copy_len = size.min(bytes.len());
                    out_payload[..copy_len].copy_from_slice(&bytes[..copy_len]);
                } else {
                    for (base, bytes) in &st.memory {
                        let base = *base;
                        if addr >= base {
                            let off = (addr - base) as usize;
                            if off < bytes.len() {
                                let copy_len = size.min(bytes.len() - off);
                                out_payload[..copy_len]
                                    .copy_from_slice(&bytes[off..off + copy_len]);
                                break;
                            }
                        }
                    }
                }
            }
        }
        // Azahar sends an empty payload for both a landed write and a rejected one.
        PacketType::WriteMemory if payload.len() >= 8 => {
            let addr = u32::from_le_bytes(payload[0..4].try_into().expect("slice"));
            let size = u32::from_le_bytes(payload[4..8].try_into().expect("slice")) as usize;
            let data = &payload[8..];
            if size > 0
                && size <= protocol.max_write_chunk()
                && data.len() >= size
                && write_allowed(addr)
            {
                apply_write(&mut st.memory, addr, &data[..size]);
            }
        }
        _ => {}
    }
    encode_reply(header.version, header.id, header.packet_type, &out_payload)
}
