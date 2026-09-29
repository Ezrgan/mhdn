//! `mhdn-probe tap install|uninstall|events` (plan 2.20).

use mhdn_game::{
    tap_install, tap_read_events, tap_uninstall, InstallOutcome, PatchMemory, TapError,
};
use mhdn_rpc::RpcClient;

use std::net::SocketAddr;
use std::time::Duration;

use crate::attach;
use crate::error::{ProbeError, Result};

pub fn install(addr: SocketAddr, title_id: u64, timeout: Duration) -> Result<()> {
    let mut attached = attach::attach(addr, title_id, timeout)?;
    let mut mem = RpcMem {
        client: &mut attached.client,
    };
    match tap_install(&mut mem).map_err(probe_tap)? {
        InstallOutcome::Installed => println!("damage tap installed"),
        InstallOutcome::AlreadyInstalled => println!("damage tap already installed"),
    }
    Ok(())
}

pub fn uninstall(addr: SocketAddr, title_id: u64, timeout: Duration) -> Result<()> {
    let mut attached = attach::attach(addr, title_id, timeout)?;
    let mut mem = RpcMem {
        client: &mut attached.client,
    };
    tap_uninstall(&mut mem).map_err(probe_tap)?;
    println!("damage tap uninstalled");
    Ok(())
}

pub fn events(addr: SocketAddr, title_id: u64, timeout: Duration) -> Result<()> {
    let mut attached = attach::attach(addr, title_id, timeout)?;
    let mut mem = RpcMem {
        client: &mut attached.client,
    };
    let events = tap_read_events(&mut mem).map_err(probe_tap)?;
    if events.is_empty() {
        println!("no tap events");
        return Ok(());
    }
    for event in events {
        println!(
            "seq={} damage={} r1={} monster=0x{:08X} lr=0x{:08X}",
            event.seq,
            event.damage(),
            event.r1,
            event.monster,
            event.lr
        );
    }
    Ok(())
}

struct RpcMem<'a> {
    client: &'a mut RpcClient,
}

impl PatchMemory for RpcMem<'_> {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> std::result::Result<(), TapError> {
        self.client
            .read(addr, buf)
            .map_err(|err| TapError::Memory(err.to_string()))
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> std::result::Result<(), TapError> {
        self.client
            .write(addr, data)
            .map_err(|err| TapError::Memory(err.to_string()))
    }
}

fn probe_tap(err: TapError) -> ProbeError {
    ProbeError::msg(err.to_string())
}
