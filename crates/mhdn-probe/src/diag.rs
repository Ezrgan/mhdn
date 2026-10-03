//! `mhdn-probe diag`: read-only trace of a live hunt. Never installs or removes the tap.
//!
//! Writes `trace.tsv` plus `objs/<frame>_<hp>.bin` dumps of live monsters for offline offset scans.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use mhdn_game::{Anchor, DamageEvent, Pipeline, Profile, Snapshot};
use mhdn_rpc::{MemorySource, RpcClient, RpcError};

use crate::attach;
use crate::dump::read_range;
use crate::error::{ProbeError, Result};

const POLL: Duration = Duration::from_millis(16);
const STATE_EVERY: Duration = Duration::from_millis(100);
const DUMP_EVERY: Duration = Duration::from_millis(500);
/// Covers the slot object (HP − 0x1418) through species (HP + 0x5A18).
const DUMP_BEFORE_HP: u32 = 0x1800;
const DUMP_AFTER_HP: u32 = 0x5C00;

#[derive(Debug, Default)]
struct Totals {
    polls: u64,
    torn: u64,
    errors: u64,
    events: u64,
    dumps: u64,
}

pub fn run(
    addr: SocketAddr,
    title_id: u64,
    timeout: Duration,
    profile_path: &Path,
    seconds: u64,
    out: &Path,
) -> Result<()> {
    let profile = Profile::load(profile_path).map_err(|err| ProbeError::msg(err.to_string()))?;
    let mut attached = attach::attach(addr, title_id, timeout)?;
    let mut mem = Session {
        client: &mut attached.client,
    };
    let objs = out.join("objs");
    fs::create_dir_all(&objs)?;
    let mut trace = BufWriter::new(File::create(out.join("trace.tsv"))?);
    writeln!(
        trace,
        "# mhdn-probe diag {} seconds={seconds} dump=hp-0x{DUMP_BEFORE_HP:X}..hp+0x{DUMP_AFTER_HP:X}",
        profile.id()
    )?;

    let mut pipeline = Pipeline::new();
    let mut totals = Totals::default();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds);
    let mut next_state = started;
    let mut next_dump = started;
    println!(
        "diag: tracing {} for {seconds}s into {}",
        profile.id(),
        out.display()
    );
    while Instant::now() < deadline {
        let now = Instant::now();
        let t_ms = now.duration_since(started).as_millis();
        let host_us = u64::try_from(now.duration_since(started).as_micros()).unwrap_or(u64::MAX);
        totals.polls += 1;
        match pipeline.poll(&mut mem, &profile, host_us) {
            Ok(sample) => {
                if sample.torn {
                    totals.torn += 1;
                }
                for event in &sample.events {
                    totals.events += 1;
                    writeln!(trace, "{}", event_line(t_ms, event))?;
                }
                if let Some(snapshot) = sample.snapshot.as_ref() {
                    if now >= next_state {
                        next_state = now + STATE_EVERY;
                        write_state(&mut trace, t_ms, snapshot)?;
                    }
                    if now >= next_dump {
                        next_dump = now + DUMP_EVERY;
                        totals.dumps += dump_monsters(&mut mem, &objs, snapshot)?;
                    }
                }
            }
            Err(err) => {
                totals.errors += 1;
                writeln!(trace, "X\t{t_ms}\t{err}")?;
            }
        }
        std::thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
    writeln!(
        trace,
        "# polls={} torn={} errors={} events={} dumps={} lost_tap={}",
        totals.polls,
        totals.torn,
        totals.errors,
        totals.events,
        totals.dumps,
        pipeline.lost_tap()
    )?;
    trace.flush()?;
    println!(
        "diag: polls={} torn={} errors={} events={} dumps={}",
        totals.polls, totals.torn, totals.errors, totals.events, totals.dumps
    );
    Ok(())
}

fn write_state(trace: &mut impl Write, t_ms: u128, snapshot: &Snapshot) -> Result<()> {
    let camera = snapshot
        .camera
        .as_ref()
        .map(|cam| {
            format!(
                "{:.1},{:.1},{:.1}|{:.1},{:.1},{:.1}|{:.3}",
                cam.eye.x,
                cam.eye.y,
                cam.eye.z,
                cam.target.x,
                cam.target.y,
                cam.target.z,
                cam.fov_y
            )
        })
        .unwrap_or_else(|| "-".to_string());
    let hunter = snapshot
        .hunter_pos
        .map(|pos| format!("{:.1},{:.1},{:.1}", pos.x, pos.y, pos.z))
        .unwrap_or_else(|| "-".to_string());
    writeln!(
        trace,
        "S\t{t_ms}\t{}\t{:?}\tcam={camera}\thunter={hunter}\tmonsters={}",
        snapshot.guest_frame,
        snapshot.scene,
        snapshot.monsters.len()
    )?;
    for monster in &snapshot.monsters {
        writeln!(
            trace,
            "M\t{t_ms}\t{}\t0x{:08X}\tsp={}\thp={}/{}\tvis={}\tlarge={}\tpos={:.1},{:.1},{:.1}",
            snapshot.guest_frame,
            monster.key.struct_addr,
            monster.key.species,
            monster.hp,
            monster.key.max_hp,
            u8::from(monster.visible),
            u8::from(monster.large),
            monster.pos.x,
            monster.pos.y,
            monster.pos.z
        )?;
    }
    Ok(())
}

fn dump_monsters(mem: &mut dyn MemorySource, objs: &Path, snapshot: &Snapshot) -> Result<u64> {
    let mut written = 0;
    for monster in snapshot.monsters.iter().filter(|monster| monster.hp > 0) {
        let hp = monster.key.struct_addr;
        let start = hp.wrapping_sub(DUMP_BEFORE_HP);
        let Ok(bytes) = read_range(mem, start, hp.wrapping_add(DUMP_AFTER_HP)) else {
            continue;
        };
        let name = format!("{:08}_{hp:08X}.bin", snapshot.guest_frame);
        fs::write(objs.join(name), bytes)?;
        written += 1;
    }
    Ok(written)
}

fn event_line(t_ms: u128, event: &DamageEvent) -> String {
    let anchor = match event.anchor {
        Anchor::Unknown => "-".to_string(),
        Anchor::World(pos) => format!("{:.1},{:.1},{:.1}", pos.x, pos.y, pos.z),
    };
    format!(
        "E\t{t_ms}\t{}\t{:?}\tamount={}\tmonster=0x{:08X}\tkey={}\tanchor={anchor}\tseq={}",
        event.guest_frame,
        event.source,
        event.amount,
        event.monster,
        event
            .key
            .map(|key| format!("0x{:08X}", key.struct_addr))
            .unwrap_or_else(|| "-".to_string()),
        event.seq
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
