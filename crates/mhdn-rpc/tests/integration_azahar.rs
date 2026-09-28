//! Manual integration test against a running Azahar instance with RPC enabled.
//!
//! Run with: `cargo test -p mhdn-rpc --test integration_azahar -- --ignored --nocapture`

use std::net::SocketAddr;
use std::time::Duration;

use mhdn_rpc::RpcClient;

const MHXX_JP_TITLE_ID: u64 = 0x0004_0000_0019_7100;

#[test]
#[ignore = "requires Azahar with RPC server enabled on 127.0.0.1:45987"]
fn lists_processes_and_finds_mhxx() {
    let addr: SocketAddr = "127.0.0.1:45987".parse().expect("addr");
    let mut client =
        RpcClient::connect(addr, Duration::from_secs(2)).expect("connect to Azahar RPC");
    let processes = client.list_processes().expect("process list");
    for p in &processes {
        println!(
            "pid={} title_id=0x{:016X} name={}",
            p.pid,
            p.title_id,
            p.name_str()
        );
    }
    let mhxx = processes
        .iter()
        .find(|p| p.title_id == MHXX_JP_TITLE_ID)
        .expect("MHXX JP process not found — start the game in Azahar");
    println!("found MHXX at pid {}", mhxx.pid);
}
