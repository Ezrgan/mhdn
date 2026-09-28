mod attach;
mod dump;
mod error;
mod format;
mod parse;
mod scan;

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
    about = "MHXX reverse-engineering CLI (see PLAN.md phase 2)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
        Commands::Attach { conn } => cmd_attach(conn),
        Commands::Hexdump { conn, guest, len } => cmd_hexdump(conn, guest, len),
        Commands::Dump {
            conn,
            start,
            end,
            file,
        } => cmd_dump(conn, start, end, file),
        Commands::Scan { action } => cmd_scan(action),
        Commands::Peek { conn, ty, guest } => cmd_peek(conn, ty, guest),
        Commands::BenchRpc {
            addr,
            read_addr,
            seconds,
        } => bench_rpc(addr, read_addr, seconds),
    }
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
