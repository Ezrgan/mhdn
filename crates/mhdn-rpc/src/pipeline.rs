use crate::client::RpcClient;
use crate::error::{Result, RpcError};
use crate::packet::{self, PacketType, MAX_PACKET_SIZE};
use crate::ReadReq;

impl RpcClient {
    pub fn read_many(&mut self, reqs: &mut [ReadReq<'_>]) -> Result<()> {
        if reqs.is_empty() {
            return Ok(());
        }

        #[derive(Clone, Copy)]
        struct Pending {
            id: u32,
            req_index: usize,
            addr: u32,
            len: usize,
        }

        let mut pending: Vec<Pending> = Vec::new();
        let mut next_req = 0usize;
        let mut completed = 0usize;

        while completed < reqs.len() {
            while next_req < reqs.len() && pending.len() < self.pipeline_window() {
                let len = reqs[next_req].buf.len();
                if len == 0 {
                    next_req += 1;
                    completed += 1;
                    continue;
                }
                let max = self.protocol().max_packet_data_size;
                if len > max {
                    return Err(RpcError::ReadTooLarge {
                        requested: len,
                        max,
                    });
                }
                let id = self.next_request_id();
                let addr = reqs[next_req].addr;
                let req_payload = [addr.to_le_bytes(), (len as u32).to_le_bytes()].concat();
                self.send_request(id, PacketType::ReadMemory, &req_payload)?;
                pending.push(Pending {
                    id,
                    req_index: next_req,
                    addr,
                    len,
                });
                next_req += 1;
            }

            if pending.is_empty() {
                break;
            }

            let mut buf = [0u8; MAX_PACKET_SIZE];
            let len = match self.recv_datagram(&mut buf) {
                Ok(n) => n,
                Err(e) if crate::client::is_timeout(&e) => {
                    return Err(RpcError::Timeout(self.request_timeout()));
                }
                Err(e) => return Err(e.into()),
            };
            if len == 0 {
                return Err(RpcError::InvalidResponse);
            }
            let (header, payload) = packet::split_datagram(&buf[..len])?;
            let version = self.protocol().version;
            if header.version != version {
                return Err(RpcError::VersionMismatch {
                    expected: version,
                    got: header.version,
                });
            }
            if header.packet_type != PacketType::ReadMemory {
                return Err(RpcError::TypeMismatch {
                    expected: PacketType::ReadMemory,
                    got: header.packet_type,
                });
            }
            let Some(pos) = pending.iter().position(|p| p.id == header.id) else {
                // A reply to an earlier, timed-out request.
                continue;
            };
            let p = pending.remove(pos);
            if payload.len() != p.len {
                return Err(RpcError::ReadFailed { addr: p.addr });
            }
            reqs[p.req_index].buf.copy_from_slice(payload);
            completed += 1;
        }

        Ok(())
    }
}
