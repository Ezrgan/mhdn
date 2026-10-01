mod attach;
mod dump;
mod error;
mod findmat;
mod fingerprint;
mod format;
mod parse;
mod ptrscan;
mod record;
mod scan;
mod tap_cmd;
mod watch;

use std::net::SocketAddr;
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use mhdn_rpc::{ReadReq, RpcClient};

use crate::error::Result;
use crate::format::PeekType;
use crate::parse::{parse_hex_u32, parse_hex_u64, parse_range};
use crate::scan::ValueType;

#[derive(Parser)]
#[command(
    name = "mhdn-probe",
    about = "CLI for live memory analysis and diagnostics against a running Azahar instance"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show title id, update TMD version, and `.text` xxh3 fingerprints.
    GameInfo {
        #[command(flatten)]
        conn: ConnArgs,
        /// Update TMD (`0004000e/00197100/content/*.tmd`). Defaults to the Azahar path on macOS.
        #[arg(long)]
        tmd: Option<PathBuf>,
        /// Profile to compare fingerprints against.
        #[arg(long)]
        profile: Option<PathBuf>,
        /// How many 4 KiB `.text` windows to hash when the profile has none.
        #[arg(long, default_value_t = 8)]
        windows: u32,
    },
    /// List Azahar processes and select MHXX.
    Attach {
        #[command(flatten)]
        conn: ConnArgs,
    },
    /// Hex dump of guest memory.
    Hexdump {
        #[command(flatten)]
        conn: ConnArgs,
        /// Guest address (`0x00100000`).
        #[arg(value_parser = parse_hex_u32, value_name = "ADDR")]
        guest: u32,
        /// Number of bytes to read.
        len: usize,
    },
    /// Dump a guest address range to a raw file (pair it with `--base` later).
    Dump {
        #[command(flatten)]
        conn: ConnArgs,
        /// First guest address to include.
        #[arg(value_parser = parse_hex_u32, value_name = "START")]
        start: u32,
        /// Exclusive end address.
        #[arg(value_parser = parse_hex_u32, value_name = "END")]
        end: u32,
        /// Output path. Raw bytes, no header. `*.bin` is gitignored.
        file: PathBuf,
    },
    /// Cheat-engine style value scan. Candidates are stored in a session file.
    Scan {
        #[command(subcommand)]
        action: ScanCmd,
    },
    /// Print frame count and size of an `.mhrec` file.
    RecInfo { file: PathBuf },
    /// Record profile fields at a fixed rate into a versioned `.mhrec` file.
    Record {
        #[command(flatten)]
        conn: ConnArgs,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 60)]
        hz: u32,
        /// How long to record. The phase-2 check is 120 seconds of combat under 10 MB.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
    /// Find view and projection matrix candidates in a dump.
    Findmat {
        dump: PathBuf,
        #[arg(long, value_parser = parse_hex_u32)]
        base: u32,
        /// Second dump (camera moved). Hits that did not change are dropped.
        #[arg(long)]
        diff: Option<PathBuf>,
        #[arg(long, default_value_t = 32)]
        limit: usize,
    },
    /// Find pointer chains from a static range to a target address, using a dump.
    Ptrscan {
        #[arg(value_parser = parse_hex_u32, value_name = "TARGET")]
        target: u32,
        /// Flat dump produced by `dump` or GDB `dump memory`.
        #[arg(long)]
        dump: PathBuf,
        /// Guest address of byte 0 in the dump.
        #[arg(long, value_parser = parse_hex_u32)]
        base: u32,
        #[arg(long, default_value_t = 4)]
        depth: u32,
        #[arg(long, default_value = "0x2000", value_parser = parse_hex_u32)]
        max_offset: u32,
        /// Where the chain root must live, e.g. `0x00100000-0x00E00000`.
        #[arg(long, value_parser = parse_range)]
        static_range: Range<u32>,
        #[arg(long, default_value_t = 64)]
        limit: usize,
    },
    /// Resolve `BASE+OFF+OFF` against the live process.
    Ptrverify {
        #[command(flatten)]
        conn: ConnArgs,
        #[arg(value_parser = ptrscan::parse_path, value_name = "PATH")]
        path: ptrscan::PointerPath,
        #[arg(long, value_parser = parse_hex_u32)]
        expect: Option<u32>,
    },
    /// Print bytes and floats that change between samples.
    Watch {
        #[command(flatten)]
        conn: ConnArgs,
        #[arg(value_parser = parse_hex_u32, value_name = "ADDR")]
        guest: u32,
        len: usize,
        #[arg(long, default_value_t = 30)]
        hz: u32,
        /// Stop after this many seconds. `0` watches until Ctrl-C.
        #[arg(long, default_value_t = 0)]
        seconds: u64,
    },
    /// Read one typed value.
    Peek {
        #[command(flatten)]
        conn: ConnArgs,
        /// `u8`, `u16`, `u32`, `f32`, or `vec3`.
        #[arg(value_parser = PeekType::parse)]
        ty: PeekType,
        #[arg(value_parser = parse_hex_u32, value_name = "ADDR")]
        guest: u32,
    },
    /// Install, remove, or read the damage-tap hook (plan 2.20).
    Tap {
        #[command(subcommand)]
        action: TapCmd,
    },
    /// Measure Azahar RPC latency and throughput on localhost.
    BenchRpc {
        /// RPC server address (Azahar default: 127.0.0.1:45987).
        #[arg(long, default_value = "127.0.0.1:45987")]
        addr: SocketAddr,
        /// Guest address used for small reads (must be readable when a game is loaded).
        #[arg(long, default_value = "0x00100000", value_parser = parse_hex_u32)]
        read_addr: u32,
        /// Duration of the sustained throughput phase.
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
}

#[derive(Subcommand)]
enum TapCmd {
    /// Verify the original words, write the stub, then the hook branch.
    Install {
        #[command(flatten)]
        conn: ConnArgs,
    },
    /// Restore the original instruction at the hook.
    Uninstall {
        #[command(flatten)]
        conn: ConnArgs,
    },
    /// Print ring entries published by the stub.
    Events {
        #[command(flatten)]
        conn: ConnArgs,
    },
    /// Stay attached and append new hits. One process, one socket.
    Follow {
        #[command(flatten)]
        conn: ConnArgs,
        /// Boss HP address for this boot, if already resolved.
        #[arg(long, value_parser = parse_hex_u32)]
        hp: Option<u32>,
        /// Append-only log. Defaults to `dumps/tap-live.log`.
        #[arg(long)]
        log: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ScanCmd {
    /// First pass: `unknown` or an exact value.
    New {
        #[command(flatten)]
        conn: ConnArgs,
        #[arg(value_parser = ValueType::parse)]
        ty: ValueType,
        /// `unknown` or the exact value to keep (`100`, `0x64`, `1.5`).
        query: String,
        /// Guest range `START-END` (end exclusive), e.g. `0x08000000-0x09000000`.
        #[arg(long, value_parser = parse_range)]
        range: Range<u32>,
        #[arg(long, default_value = scan::DEFAULT_SESSION)]
        session: PathBuf,
    },
    /// Narrow the session with eq, ne, gt, lt, inc, dec, changed, or unchanged.
    Next {
        #[command(flatten)]
        conn: ConnArgs,
        filter: String,
        value: Option<String>,
        #[arg(long, default_value = scan::DEFAULT_SESSION)]
        session: PathBuf,
    },
    /// Print candidates from the session (last scanned value, no RPC read).
    List {
        #[arg(long, default_value = scan::DEFAULT_SESSION)]
        session: PathBuf,
        #[arg(long, default_value_t = 32)]
        limit: usize,
    },
}

#[derive(clap::Args)]
struct ConnArgs {
    /// Azahar RPC address.
    #[arg(long, default_value = "127.0.0.1:45987")]
    addr: SocketAddr,
    /// Guest title id to attach, in hex. Default is MHXX JP.
    #[arg(long, default_value = "0x0004000000197100", value_parser = parse_hex_u64)]
    title_id: u64,
    /// Per-request RPC timeout in milliseconds.
    #[arg(long, default_value_t = 200)]
    timeout_ms: u64,
}

impl ConnArgs {
    fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::GameInfo {
            conn,
            tmd,
            profile,
            windows,
        } => cmd_game_info(conn, tmd, profile, windows),
        Commands::Attach { conn } => cmd_attach(conn),
        Commands::Hexdump { conn, guest, len } => cmd_hexdump(conn, guest, len),
        Commands::Dump {
            conn,
            start,
            end,
            file,
        } => cmd_dump(conn, start, end, file),
        Commands::Scan { action } => cmd_scan(action),
        Commands::RecInfo { file } => cmd_rec_info(file),
        Commands::Record {
            conn,
            profile,
            out,
            hz,
            seconds,
        } => cmd_record(conn, profile, out, hz, seconds),
        Commands::Findmat {
            dump,
            base,
            diff,
            limit,
        } => cmd_findmat(dump, base, diff, limit),
        Commands::Ptrscan {
            target,
            dump,
            base,
            depth,
            max_offset,
            static_range,
            limit,
        } => cmd_ptrscan(target, dump, base, depth, max_offset, static_range, limit),
        Commands::Ptrverify { conn, path, expect } => cmd_ptrverify(conn, path, expect),
        Commands::Watch {
            conn,
            guest,
            len,
            hz,
            seconds,
        } => cmd_watch(conn, guest, len, hz, seconds),
        Commands::Peek { conn, ty, guest } => cmd_peek(conn, ty, guest),
        Commands::Tap { action } => match action {
            TapCmd::Install { conn } => tap_cmd::install(conn.addr, conn.title_id, conn.timeout()),
            TapCmd::Uninstall { conn } => {
                tap_cmd::uninstall(conn.addr, conn.title_id, conn.timeout())
            }
            TapCmd::Events { conn } => tap_cmd::events(conn.addr, conn.title_id, conn.timeout()),
            TapCmd::Follow { conn, hp, log } => tap_cmd::follow(
                conn.addr,
                conn.title_id,
                conn.timeout(),
                hp,
                log.unwrap_or_else(|| PathBuf::from("dumps/tap-live.log")),
            ),
        },
        Commands::BenchRpc {
            addr,
            read_addr,
            seconds,
        } => bench_rpc(addr, read_addr, seconds),
    }
}

fn cmd_game_info(
    conn: ConnArgs,
    tmd: Option<PathBuf>,
    profile_path: Option<PathBuf>,
    windows: u32,
) -> Result<()> {
    let tmd_path = tmd.or_else(fingerprint::default_azahar_update_tmd);
    match tmd_path {
        Some(path) => match fingerprint::read_tmd_title_version(&path) {
            Ok(version) => println!(
                "update title version {version} (0x{version:04X}) from {}",
                path.display()
            ),
            Err(err) => println!("tmd: {err}"),
        },
        None => println!("tmd: not found (pass --tmd)"),
    }

    let profile = match profile_path {
        Some(path) => Some(mhdn_game::Profile::load(path)?),
        None => None,
    };

    match attach::attach(conn.addr, conn.title_id, conn.timeout()) {
        Ok(mut attached) => {
            println!(
                "process pid={} title_id=0x{:016X} name={}",
                attached.selected.pid,
                attached.selected.title_id,
                attached.selected.name_str()
            );
            let windows = match &profile {
                Some(profile) if !profile.meta.fingerprint.is_empty() => profile
                    .meta
                    .fingerprint
                    .iter()
                    .map(|window| (window.addr, window.len))
                    .collect::<Vec<_>>(),
                _ => fingerprint::text_windows(windows),
            };
            let hashes = fingerprint::hash_windows(&mut attached.client, &windows)?;
            for ((addr, len), (_, hash)) in windows.iter().zip(&hashes) {
                println!("xxh3 0x{addr:08X} len={len} 0x{hash:016X}");
            }
            if let Some(profile) = &profile {
                let observed: Vec<_> = hashes
                    .iter()
                    .map(|&(addr, xxh3)| mhdn_game::ObservedWindow { addr, xxh3 })
                    .collect();
                match mhdn_game::select(
                    std::slice::from_ref(profile),
                    attached.selected.title_id,
                    &observed,
                ) {
                    Some(selection) => println!("profile {} ({:?})", profile.id(), selection.kind),
                    None => println!("profile {} does not match this process", profile.id()),
                }
            }
        }
        Err(err) => println!("rpc: {err}"),
    }
    Ok(())
}

fn cmd_attach(conn: ConnArgs) -> Result<()> {
    let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
    println!("MHXX JP title_id=0x{:016X}", attach::MHXX_JP_TITLE_ID);
    print!("{}", attach::format_process_list(&attached.processes));
    println!(
        "selected pid={} title_id=0x{:016X} name={}",
        attached.selected.pid,
        attached.selected.title_id,
        attached.selected.name_str()
    );
    let confirmed = attached.client.selected_process()?;
    println!("RPC selected pid={confirmed}");
    Ok(())
}

fn cmd_hexdump(conn: ConnArgs, addr: u32, len: usize) -> Result<()> {
    if len == 0 {
        return Err(error::ProbeError::msg("length must be greater than 0"));
    }
    if len > 1024 * 1024 {
        return Err(error::ProbeError::msg(
            "hexdump refuses lengths above 1 MiB; use `dump` for larger regions",
        ));
    }
    let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
    let mut buf = vec![0u8; len];
    attached.client.read(addr, &mut buf)?;
    println!("{}", format::hexdump(addr, &buf));
    Ok(())
}

fn cmd_dump(conn: ConnArgs, start: u32, end: u32, file: PathBuf) -> Result<()> {
    let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
    let stats = dump::dump_to_file(&mut attached.client, start, end, &file)?;
    println!(
        "wrote {} bytes to {} in {:.3?} (guest 0x{start:08X}..0x{end:08X})",
        stats.bytes,
        file.display(),
        stats.elapsed
    );
    println!("reload with FileMemorySource base 0x{start:08X}");
    println!("faster path: Azahar GDB stub, see docs/RE_NOTES.md#memory-dumps");
    Ok(())
}

fn cmd_scan(action: ScanCmd) -> Result<()> {
    match action {
        ScanCmd::New {
            conn,
            ty,
            query,
            range,
            session,
        } => {
            let query = if query.eq_ignore_ascii_case("unknown") {
                scan::InitialQuery::Unknown
            } else {
                scan::InitialQuery::Equal(
                    scan::parse_scan_value(ty, &query).map_err(error::ProbeError::msg)?,
                )
            };
            let len = (range.end - range.start) as usize;
            if len > 128 * 1024 * 1024 {
                return Err(error::ProbeError::msg(
                    "scan range is above 128 MiB; narrow --range",
                ));
            }
            let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
            let data = dump::read_range(&mut attached.client, range.start, range.end)?;
            let snapshot = scan::scan_new(range, &data, ty, query)?;
            scan::save_snapshot(&session, &snapshot)?;
            println!(
                "{} candidates in 0x{:08X}..0x{:08X} → {}",
                snapshot.candidates.len(),
                snapshot.range.start,
                snapshot.range.end,
                session.display()
            );
        }
        ScanCmd::Next {
            conn,
            filter,
            value,
            session,
        } => {
            let previous = scan::load_snapshot(&session)?;
            let filter = scan::Filter::parse(&filter, previous.ty, value.as_deref())
                .map_err(error::ProbeError::msg)?;
            let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
            let data = dump::read_range(
                &mut attached.client,
                previous.range.start,
                previous.range.end,
            )?;
            let snapshot = scan::scan_next(&previous, &data, filter)?;
            scan::save_snapshot(&session, &snapshot)?;
            println!(
                "{} candidates (was {}) → {}",
                snapshot.candidates.len(),
                previous.candidates.len(),
                session.display()
            );
        }
        ScanCmd::List { session, limit } => {
            let snapshot = scan::load_snapshot(&session)?;
            println!(
                "{} candidates, type {:?}, range 0x{:08X}..0x{:08X}",
                snapshot.candidates.len(),
                snapshot.ty,
                snapshot.range.start,
                snapshot.range.end
            );
            for candidate in snapshot.candidates.iter().take(limit) {
                println!(
                    "0x{:08X} = {}",
                    candidate.addr,
                    scan::format_value(snapshot.ty, candidate.prev)
                );
            }
            if snapshot.candidates.len() > limit {
                println!(
                    "… {} more (raise --limit)",
                    snapshot.candidates.len() - limit
                );
            }
        }
    }
    Ok(())
}

fn cmd_rec_info(file: PathBuf) -> Result<()> {
    let bytes = std::fs::read(&file)?;
    let recording = record::decode(&bytes)?;
    let monsters: usize = recording
        .frames
        .iter()
        .map(|frame| frame.monsters.len())
        .sum();
    println!(
        "{}: {} frames at {} Hz, {monsters} monster samples, {} bytes, last guest_frame={} scene={}",
        recording.profile_id,
        recording.frames.len(),
        recording.hz,
        bytes.len(),
        recording
            .frames
            .last()
            .map(|frame| frame.guest_frame)
            .unwrap_or(0),
        recording
            .frames
            .last()
            .map(|frame| frame.scene)
            .unwrap_or(u32::MAX)
    );
    Ok(())
}

fn cmd_record(
    conn: ConnArgs,
    profile_path: PathBuf,
    out: PathBuf,
    hz: u32,
    seconds: u64,
) -> Result<()> {
    let profile = mhdn_game::Profile::load(&profile_path)?;
    let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
    println!(
        "recording {} at {hz} Hz for {seconds}s → {}",
        profile.id(),
        out.display()
    );
    let pending = profile.unresolved_fields();
    if !pending.is_empty() {
        println!("unresolved fields stay empty: {}", pending.join(", "));
    }
    let recording = record::record_for(&mut attached.client, &profile, hz, seconds)?;
    let bytes = record::encode(&recording)?;
    std::fs::write(&out, &bytes)?;
    println!(
        "wrote {} frames, {} bytes ({:.2} MB)",
        recording.frames.len(),
        bytes.len(),
        bytes.len() as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}

fn cmd_findmat(dump_path: PathBuf, base: u32, diff: Option<PathBuf>, limit: usize) -> Result<()> {
    let image = std::fs::read(&dump_path)?;
    let other = match diff {
        Some(path) => {
            let bytes = std::fs::read(&path)?;
            if bytes.len() != image.len() {
                return Err(error::ProbeError::msg(
                    "diff dump must be the same size as the first dump",
                ));
            }
            Some(bytes)
        }
        None => None,
    };
    let hits = findmat::find_matrices(base, &image, other.as_deref());
    println!("{} matrix candidate(s)", hits.len());
    for hit in hits.iter().take(limit) {
        println!("0x{:08X}  {:?}", hit.addr, hit.kind);
    }
    if hits.len() > limit {
        println!("… {} more (raise --limit)", hits.len() - limit);
    }
    Ok(())
}

fn cmd_ptrscan(
    target: u32,
    dump_path: PathBuf,
    base: u32,
    depth: u32,
    max_offset: u32,
    static_range: Range<u32>,
    limit: usize,
) -> Result<()> {
    if depth == 0 || depth > 6 {
        return Err(error::ProbeError::msg(
            "ptrscan --depth must be between 1 and 6",
        ));
    }
    let image = std::fs::read(&dump_path)?;
    if image.len() > 256 * 1024 * 1024 {
        return Err(error::ProbeError::msg("dump is above 256 MiB"));
    }
    let image_end = base.saturating_add(image.len() as u32);
    if static_range.start >= image_end || static_range.end <= base {
        eprintln!(
            "warning: static range 0x{:08X}..0x{:08X} does not overlap the dump 0x{base:08X}..0x{image_end:08X}",
            static_range.start, static_range.end
        );
    }
    let found = ptrscan::scan_pointers(
        base,
        &image,
        target,
        &ptrscan::PtrScanConfig {
            depth,
            max_offset,
            static_range,
            max_paths: limit,
        },
    );
    println!("{} path(s) to 0x{target:08X}", found.len());
    for path in &found {
        let resolved = ptrscan::resolve_in_image(base, &image, path);
        println!(
            "{}  => {}",
            ptrscan::format_path(path),
            resolved
                .map(|addr| format!("0x{addr:08X}"))
                .unwrap_or_else(|| "unresolved".to_string())
        );
    }
    Ok(())
}

fn cmd_ptrverify(conn: ConnArgs, path: ptrscan::PointerPath, expect: Option<u32>) -> Result<()> {
    let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
    let resolved = ptrscan::resolve_memory(&mut attached.client, &path)?;
    println!("{} => 0x{resolved:08X}", ptrscan::format_path(&path));
    if let Some(expected) = expect {
        if resolved != expected {
            return Err(error::ProbeError::msg(format!(
                "path resolved to 0x{resolved:08X}, expected 0x{expected:08X}"
            )));
        }
        println!("matches expected 0x{expected:08X}");
    }
    Ok(())
}

fn cmd_watch(conn: ConnArgs, guest: u32, len: usize, hz: u32, seconds: u64) -> Result<()> {
    if len == 0 || len > 4096 {
        return Err(error::ProbeError::msg(
            "watch length must be between 1 and 4096 bytes",
        ));
    }
    if hz == 0 || hz > 120 {
        return Err(error::ProbeError::msg(
            "watch --hz must be between 1 and 120",
        ));
    }
    let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
    let mut prev = vec![0u8; len];
    attached.client.read(guest, &mut prev)?;
    println!(
        "watching 0x{guest:08X} + {len} bytes at {hz} Hz{}",
        if seconds == 0 {
            " (Ctrl-C to stop)".to_string()
        } else {
            format!(" for {seconds}s")
        }
    );
    let period = Duration::from_secs_f64(1.0 / f64::from(hz));
    let deadline = (seconds > 0).then(|| Instant::now() + Duration::from_secs(seconds));
    let mut next = vec![0u8; len];
    loop {
        if deadline.is_some_and(|end| Instant::now() >= end) {
            break;
        }
        std::thread::sleep(period);
        attached.client.read(guest, &mut next)?;
        let bytes = watch::changed_bytes(guest, &prev, &next)?;
        let floats = watch::changed_f32(guest, &prev, &next)?;
        print!("{}", watch::format_diff(&bytes, &floats, 48));
        prev.copy_from_slice(&next);
    }
    Ok(())
}

fn cmd_peek(conn: ConnArgs, ty: PeekType, addr: u32) -> Result<()> {
    let mut attached = attach::attach(conn.addr, conn.title_id, conn.timeout())?;
    let mut buf = vec![0u8; ty.size()];
    attached.client.read(addr, &mut buf)?;
    println!("{}", format::format_peek(ty, addr, &buf));
    Ok(())
}

fn bench_rpc(addr: SocketAddr, read_addr: u32, seconds: u64) -> Result<()> {
    let mut client = RpcClient::connect(addr, Duration::from_millis(20))?;
    let _ = client.list_processes()?;

    println!("Azahar RPC bench → {addr}");
    println!("read_addr = 0x{read_addr:08X}");

    let samples = 500usize;
    let mut lat_4 = Vec::with_capacity(samples);
    for _ in 0..samples {
        let t0 = Instant::now();
        let _ = client.read_u32(read_addr)?;
        lat_4.push(t0.elapsed());
    }
    report_latency("read 4 B", &lat_4);

    let mut buf_1k = vec![0u8; 1024];
    let mut lat_1k = Vec::with_capacity(samples);
    for _ in 0..samples {
        let t0 = Instant::now();
        client.read(read_addr, &mut buf_1k)?;
        lat_1k.push(t0.elapsed());
    }
    report_latency("read 1 KiB", &lat_1k);

    let mut bufs: Vec<[u8; 4]> = (0..32).map(|_| [0u8; 4]).collect();
    let addrs: Vec<u32> = (0..32).map(|i| read_addr.wrapping_add(i * 4)).collect();
    let mut lat_batch = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mut reqs: Vec<ReadReq<'_>> = bufs
            .iter_mut()
            .zip(addrs.iter())
            .map(|(buf, &guest)| ReadReq {
                addr: guest,
                buf: buf.as_mut(),
            })
            .collect();
        let t0 = Instant::now();
        client.read_many(&mut reqs)?;
        lat_batch.push(t0.elapsed());
    }
    report_latency("batch 32 × 4 B (pipelined)", &lat_batch);

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut total_reads = 0u64;
    while Instant::now() < deadline {
        let _ = client.read_u32(read_addr)?;
        total_reads += 1;
    }
    let req_per_s = total_reads as f64 / seconds as f64;
    println!("sustained {seconds}s: {total_reads} single reads ({req_per_s:.0} req/s)");
    println!("Note: compare Azahar FPS/title bar before and during this test.");
    println!("Document results in docs/RE_NOTES.md#rpc-bench");

    Ok(())
}

fn report_latency(label: &str, samples: &[Duration]) {
    let mut sorted: Vec<Duration> = samples.to_vec();
    sorted.sort();
    let p50 = percentile(&sorted, 0.50);
    let p99 = percentile(&sorted, 0.99);
    println!(
        "{label}: p50 = {:.3?}, p99 = {:.3?} (n = {})",
        p50,
        p99,
        samples.len()
    );
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
