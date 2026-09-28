//! UDP fake Azahar RPC server for deterministic tests (not used in production).

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::packet::{PacketHeader, PacketType, MAX_PACKET_SIZE};

const PROCESS_ENTRY_SIZE: usize = 0x14;

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

#[derive(Default)]
struct ServerState {
    memory: HashMap<u32, Vec<u8>>,
    processes: Vec<FakeProcess>,
    selected_pid: u32,
    latency: Duration,
    drop_replies: bool,
    empty_replies: bool,
    reorder_replies: bool,
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

fn run_server(
    socket: UdpSocket,
    state: Arc<Mutex<ServerState>>,
    stop: Arc<Mutex<bool>>,
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
                    st.selected_pid =
                        u32::from_le_bytes(payload[4..8].try_into().expect("slice"));
                }
            }
            out_payload.extend_from_slice(&st.selected_pid.to_le_bytes());
        }
        PacketType::ReadMemory => {
            if payload.len() >= 8 {
                let addr =
                    u32::from_le_bytes(payload[0..4].try_into().expect("slice"));
                let size =
                    u32::from_le_bytes(payload[4..8].try_into().expect("slice")) as usize;
                out_payload.resize(size, 0);
                if let Some(bytes) = st.memory.get(&addr) {
                    let copy_len = size.min(bytes.len());
                    out_payload[..copy_len].copy_from_slice(&bytes[..copy_len]);
                }
            }
        }
        PacketType::WriteMemory => {}
    }
    let hdr = PacketHeader::new(header.id, header.packet_type, out_payload.len() as u32);
    let mut datagram = Vec::with_capacity(16 + out_payload.len());
    datagram.extend_from_slice(&hdr.encode());
    datagram.extend_from_slice(&out_payload);
    datagram
}
