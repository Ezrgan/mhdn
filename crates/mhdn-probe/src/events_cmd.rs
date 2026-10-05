//! `mhdn-probe events`: sample the hunt and print one line per damage event.

use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use mhdn_game::{PatchMemory, Pipeline, Profile, TapError};
use mhdn_rpc::{MemorySource, RpcClient, RpcError};

use crate::attach;
use crate::error::{ProbeError, Result};

pub fn follow(
    addr: SocketAddr,
    title_id: u64,
    timeout: Duration,
    profile_path: &Path,
    seconds: u64,
) -> Result<()> {
    let profile = Profile::load(profile_path).map_err(|err| ProbeError::msg(err.to_string()))?;
    let mut attached = attach::attach(addr, title_id, timeout)?;
    let mut session = Session {
        client: &mut attached.client,
    };
    let mut pipeline = Pipeline::from_env();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds);
    println!("sampling {} for {seconds}s", profile.id());
    while Instant::now() < deadline {
        let host_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        match pipeline.poll(&mut session, &profile, host_us) {
            Ok(sample) => {
                for event in sample.events {
                    println!("{}", format_event(&event));
                }
            }
            Err(err) => eprintln!("sample error: {err}"),
        }
        let _ = pipeline.maintain_tap(&mut session, &profile);
        let period = mhdn_game::sample_period(pipeline.scene(), pipeline.failures());
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        std::thread::sleep(period.min(left));
    }
    pipeline.shutdown_tap(&mut session);
    if pipeline.lost_tap() > 0 {
        eprintln!("lost tap entries: {}", pipeline.lost_tap());
    }
    Ok(())
}

fn format_event(event: &mhdn_game::DamageEvent) -> String {
    let anchor = match event.anchor {
        mhdn_game::Anchor::Unknown => "anchor=?".to_string(),
        mhdn_game::Anchor::World(pos) => format!("anchor=({:.1},{:.1},{:.1})", pos.x, pos.y, pos.z),
    };
    let part = event
        .part_hp
        .map(|hp| format!(" part_hp={hp}"))
        .unwrap_or_default();
    format!(
        "frame={} {:?} amount={} monster=0x{:08X} {:?} {:?} {anchor}{part}",
        event.guest_frame, event.source, event.amount, event.monster, event.confidence, event.kind,
    )
}

struct Session<'a> {
    client: &'a mut RpcClient,
}

impl MemorySource for Session<'_> {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> std::result::Result<(), RpcError> {
        self.client.read(addr, buf)
    }
}

impl PatchMemory for Session<'_> {
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
