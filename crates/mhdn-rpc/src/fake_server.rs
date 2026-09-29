//! UDP fake Azahar RPC server for deterministic tests (not used in production).

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::packet::{PacketHeader, PacketType, MAX_PACKET_SIZE};

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
    pending_replies: Vec<Vec<u8>>,
}

impl FakeRpcServer {
    pub fn bind() -> (Self, Arc<Mutex<ServerState>>) {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = socket.local_addr().expect("addr");
        let state = Arc::new(Mutex::new(ServerState::default()));
        let stop = Arc::new(Mutex::new(false));
        let state_thread = Arc::clone(&state);
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || run_server(socket, state_thread, stop_thread));
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

fn run_server(socket: UdpSocket, state: Arc<Mutex<ServerState>>, stop: Arc<Mutex<bool>>) {
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
        let raw = &buf[..len];
        let header = match PacketHeader::decode(raw) {
            Ok(h) => h,
            Err(_) => continue,
        };
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
        let reply = build_reply(&mut st, header, payload);
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

fn build_reply(st: &mut ServerState, header: PacketHeader, payload: &[u8]) -> Vec<u8> {
    let mut out_payload = Vec::new();
    match header.packet_type {
        PacketType::ProcessList => {
            let start = if payload.len() >= 4 {
                u32::from_le_bytes(payload[0..4].try_into().expect("slice")) as usize
            } else {
                0
            };
            let slice = st.processes.get(start..).unwrap_or(&[]);
            let count = slice.len().min(51);
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
        PacketType::ReadMemory => {
            if payload.len() >= 8 {
                let addr = u32::from_le_bytes(payload[0..4].try_into().expect("slice"));
                let size = u32::from_le_bytes(payload[4..8].try_into().expect("slice")) as usize;
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
        PacketType::WriteMemory => {
            if payload.len() >= 8 {
                let addr = u32::from_le_bytes(payload[0..4].try_into().expect("slice"));
                let size = u32::from_le_bytes(payload[4..8].try_into().expect("slice")) as usize;
                let data = &payload[8..];
                if data.len() >= size && write_allowed(addr) {
                    apply_write(&mut st.memory, addr, &data[..size]);
                }
            }
            // Azahar sends an empty payload for both a landed write and a rejected one.
        }
    }
    let hdr = PacketHeader::new(header.id, header.packet_type, out_payload.len() as u32);
    let mut datagram = Vec::with_capacity(16 + out_payload.len());
    datagram.extend_from_slice(&hdr.encode());
    datagram.extend_from_slice(&out_payload);
    datagram
}
