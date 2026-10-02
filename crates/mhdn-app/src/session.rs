//! RPC session. The sampler thread owns the socket; the UI only reads counters.

#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mhdn_game::{spawn, Latest, PatchMemory, Profile, Snapshot, TapError, MHXX_JP_TITLE_ID};
use mhdn_rpc::{MemorySource, RpcClient, RpcError};

const RPC_ADDR: &str = "127.0.0.1:45987";

#[derive(Debug, Default)]
pub struct RpcMeter {
    reads: AtomicU64,
    nanos: AtomicU64,
    up: AtomicBool,
}

impl RpcMeter {
    pub fn record(&self, elapsed: Duration) {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.nanos
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
    }

    pub fn set_up(&self, up: bool) {
        self.up.store(up, Ordering::Relaxed);
    }

    pub fn is_up(&self) -> bool {
        self.up.load(Ordering::Relaxed)
    }

    fn reads(&self) -> u64 {
        self.reads.load(Ordering::Relaxed)
    }

    fn nanos(&self) -> u64 {
        self.nanos.load(Ordering::Relaxed)
    }
}

/// Turns the cumulative counters into a short-window req/s and mean latency.
#[derive(Debug)]
pub struct RateWindow {
    last_reads: u64,
    last_nanos: u64,
    last_at: Instant,
    pub requests_per_sec: f32,
    pub latency_ms: f32,
}

impl RateWindow {
    pub fn new(now: Instant) -> Self {
        Self {
            last_reads: 0,
            last_nanos: 0,
            last_at: now,
            requests_per_sec: 0.0,
            latency_ms: 0.0,
        }
    }
}

pub fn sample_rate(window: &mut RateWindow, meter: &RpcMeter, now: Instant) {
    let elapsed = now.saturating_duration_since(window.last_at).as_secs_f32();
    if elapsed < 0.25 {
        return;
    }
    let reads = meter.reads();
    let nanos = meter.nanos();
    let delta_reads = reads.saturating_sub(window.last_reads);
    let delta_nanos = nanos.saturating_sub(window.last_nanos);
    window.requests_per_sec = delta_reads as f32 / elapsed;
    if delta_reads > 0 {
        window.latency_ms = (delta_nanos as f32 / delta_reads as f32) / 1_000_000.0;
    }
    window.last_reads = reads;
    window.last_nanos = nanos;
    window.last_at = now;
}

pub struct Session {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    pub fn start(profile: Profile, snapshots: Arc<Latest<Snapshot>>, meter: Arc<RpcMeter>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = thread::spawn(move || run_until_stopped(profile, snapshots, meter, flag));
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_until_stopped(
    profile: Profile,
    snapshots: Arc<Latest<Snapshot>>,
    meter: Arc<RpcMeter>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        match connect() {
            Ok(client) => {
                meter.set_up(true);
                let mem = Metered {
                    client,
                    meter: Arc::clone(&meter),
                };
                let join = spawn(
                    mem,
                    profile.clone(),
                    Arc::clone(&snapshots),
                    events(),
                    thread::sleep,
                );
                while !stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(100));
                }
                drop(join);
                meter.set_up(false);
            }
            Err(_) => {
                meter.set_up(false);
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn events() -> Arc<mhdn_game::EventQueue> {
    Arc::new(mhdn_game::EventQueue::new(64))
}

fn connect() -> Result<RpcClient, RpcError> {
    let addr = RPC_ADDR
        .parse()
        .expect("127.0.0.1:45987 is a valid socket address");
    let mut client = RpcClient::connect(addr, Duration::from_millis(50))?;
    let processes = client.list_processes()?;
    let pid = processes
        .iter()
        .find(|process| process.title_id == MHXX_JP_TITLE_ID)
        .map(|process| process.pid)
        .ok_or(RpcError::InvalidResponse)?;
    client.select_process(pid)?;
    Ok(client)
}

struct Metered {
    client: RpcClient,
    meter: Arc<RpcMeter>,
}

impl MemorySource for Metered {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> mhdn_rpc::Result<()> {
        let started = Instant::now();
        let result = self.client.read(addr, buf);
        self.meter.record(started.elapsed());
        result
    }
}

impl PatchMemory for Metered {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), TapError> {
        MemorySource::read(self, addr, buf).map_err(|err| TapError::Memory(err.to_string()))
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), TapError> {
        let started = Instant::now();
        let result = self
            .client
            .write(addr, data)
            .map_err(|err| TapError::Memory(err.to_string()));
        self.meter.record(started.elapsed());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_window_reports_requests_and_mean_latency() {
        let meter = RpcMeter::default();
        meter.record(Duration::from_micros(500));
        meter.record(Duration::from_micros(1500));
        let mut window = RateWindow::new(Instant::now() - Duration::from_millis(500));
        sample_rate(&mut window, &meter, Instant::now());
        assert!(window.requests_per_sec > 0.0);
        assert!(window.latency_ms > 0.4 && window.latency_ms < 1.6);
    }
}
