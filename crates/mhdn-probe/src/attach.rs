use std::net::SocketAddr;
use std::time::Duration;

use mhdn_rpc::{ProcessInfo, RpcClient};

use crate::error::{ProbeError, Result};

pub const MHXX_JP_TITLE_ID: u64 = 0x0004_0000_0019_7100;

pub struct Attached {
    pub client: RpcClient,
    pub processes: Vec<ProcessInfo>,
    pub selected: ProcessInfo,
}

pub fn attach(addr: SocketAddr, title_id: u64, timeout: Duration) -> Result<Attached> {
    let mut client = RpcClient::connect(addr, timeout)?;
    let processes = client.list_processes()?;
    let selected = processes
        .iter()
        .find(|process| process.title_id == title_id)
        .cloned()
        .ok_or_else(|| {
            ProbeError::msg(format!(
                "no process with title_id 0x{title_id:016X} (RPC {addr}). Start MHXX in Azahar with the RPC server enabled."
            ))
        })?;
    client.select_process(selected.pid)?;
    Ok(Attached {
        client,
        processes,
        selected,
    })
}

pub fn format_process_list(processes: &[ProcessInfo]) -> String {
    let mut out = String::new();
    for process in processes {
        out.push_str(&format!(
            "pid={} title_id=0x{:016X} name={}\n",
            process.pid,
            process.title_id,
            process.name_str()
        ));
    }
    out
}
