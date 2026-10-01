//! `mhdn-probe tap install|uninstall|events` (plan 2.20).

use mhdn_game::{
    tap_install, tap_read_events, tap_uninstall, InstallOutcome, PatchMemory, TapError, TapEvent,
    ENTRY_SIZE, RING_ADDR, WRITE_SEQ_ADDR,
};
use mhdn_rpc::RpcClient;

use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::thread;
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
            "seq={} damage={} r1={} monster=0x{:08X} lr=0x{:08X} sp=[0x{:08X}, 0x{:08X}, 0x{:08X}, 0x{:08X}, 0x{:08X}]",
            event.seq,
            event.damage(),
            event.r1,
            event.monster,
            event.lr,
            event.stack[0],
            event.stack[1],
            event.stack[2],
            event.stack[3],
            event.stack[4],
        );
    }
    Ok(())
}

/// One process, one socket. Reads `write_seq` about ten times a second and only
/// fetches ring slots that are new.
pub fn follow(
    addr: SocketAddr,
    title_id: u64,
    timeout: Duration,
    hp_addr: Option<u32>,
    log_path: PathBuf,
) -> Result<()> {
    let mut attached = attach::attach(addr, title_id, timeout)?;
    let mut mem = RpcMem {
        client: &mut attached.client,
    };
    if let Some(parent) = log_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let mut last_seq = 0u32;
    let mut last_hp: Option<u32> = None;
    let mut hits = 0u32;
    let mut sum = 0i64;
    println!("tap follow → {}", log_path.display());
    writeln!(log, "# follow start").ok();
    loop {
        match read_u32(&mut mem, WRITE_SEQ_ADDR) {
            Ok(write_seq) if write_seq != last_seq => {
                if last_seq != 0 && write_seq.wrapping_sub(last_seq) > 64 {
                    let line = format!("lost seq {last_seq}..={write_seq}");
                    println!("{line}");
                    writeln!(log, "{line}").ok();
                }
                let start = write_seq.saturating_sub(64).max(last_seq);
                for seq in (start + 1)..=write_seq {
                    let slot = RING_ADDR + (seq % 64) * ENTRY_SIZE;
                    let mut buf = [0u8; ENTRY_SIZE as usize];
                    if mem.read(slot, &mut buf).is_err() {
                        continue;
                    }
                    let Some(event) = TapEvent::decode(&buf) else {
                        continue;
                    };
                    if event.seq != seq {
                        continue;
                    }
                    hits += 1;
                    sum += i64::from(event.damage());
                    let line = format!(
                        "seq={} damage={} r1={} monster=0x{:08X} lr=0x{:08X} sp=[0x{:08X}, 0x{:08X}, 0x{:08X}, 0x{:08X}, 0x{:08X}]",
                        event.seq,
                        event.damage(),
                        event.r1,
                        event.monster,
                        event.lr,
                        event.stack[0],
                        event.stack[1],
                        event.stack[2],
                        event.stack[3],
                        event.stack[4],
                    );
                    println!("{line}");
                    writeln!(log, "{line}").ok();
                }
                last_seq = write_seq;
                log.flush().ok();
            }
            Ok(_) => {}
            Err(err) => {
                println!("events error {err}");
                thread::sleep(Duration::from_millis(200));
                continue;
            }
        }
        if let Some(hp_addr) = hp_addr {
            match read_u32(&mut mem, hp_addr) {
                Ok(hp) if last_hp != Some(hp) => {
                    let line = format!("hp={hp} hits={hits} sum={sum}");
                    println!("{line}");
                    writeln!(log, "{line}").ok();
                    log.flush().ok();
                    last_hp = Some(hp);
                }
                Ok(_) => {}
                Err(err) => println!("hp error {err}"),
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn read_u32(mem: &mut RpcMem<'_>, addr: u32) -> std::result::Result<u32, TapError> {
    let mut buf = [0u8; 4];
    mem.read(addr, &mut buf)?;
    Ok(u32::from_le_bytes(buf))
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
