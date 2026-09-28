use std::net::SocketAddr;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use mhdn_rpc::{ReadReq, RpcClient};

#[derive(Parser)]
#[command(name = "mhdn-probe", about = "MHXX reverse-engineering CLI (see PLAN.md)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Measure Azahar RPC latency and throughput on localhost.
    BenchRpc {
        /// RPC server address (Azahar default: 127.0.0.1:45987).
        #[arg(long, default_value = "127.0.0.1:45987")]
        addr: SocketAddr,

        /// Guest address used for small reads (must be readable when a game is loaded).
        #[arg(long, default_value = "0x00100000")]
        read_addr: u32,

        /// Duration of the sustained throughput phase.
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> mhdn_rpc::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::BenchRpc {
            addr,
            read_addr,
            seconds,
        } => bench_rpc(addr, read_addr, seconds),
    }
}

fn bench_rpc(addr: SocketAddr, read_addr: u32, seconds: u64) -> mhdn_rpc::Result<()> {
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
            .map(|(buf, &addr)| ReadReq { addr, buf: buf.as_mut() })
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
    println!(
        "sustained {seconds}s: {total_reads} single reads ({req_per_s:.0} req/s)"
    );
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
