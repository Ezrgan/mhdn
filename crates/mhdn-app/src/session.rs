//! RPC session. The sampler thread owns the socket; the UI only reads counters.

#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mhdn_game::{spawn, Latest, PatchMemory, Profile, Pump, Snapshot, TapError, MHXX_JP_TITLE_ID};
use mhdn_rpc::{MemorySource, RpcClient};

const RPC_ADDR: &str = "127.0.0.1:45987";

pub const PHASE_DOWN: u8 = 0;
pub const PHASE_WAITING: u8 = 1;
pub const PHASE_LIVE: u8 = 2;
pub const PHASE_UNSUPPORTED: u8 = 3;

#[derive(Debug)]
pub struct RpcMeter {
    reads: AtomicU64,
    nanos: AtomicU64,
    up: AtomicBool,
    phase: AtomicU8,
}

impl Default for RpcMeter {
    fn default() -> Self {
        Self {
            reads: AtomicU64::new(0),
            nanos: AtomicU64::new(0),
            up: AtomicBool::new(false),
            phase: AtomicU8::new(PHASE_WAITING),
        }
    }
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

    pub fn set_phase(&self, phase: u8) {
        self.phase.store(phase, Ordering::Relaxed);
    }

    pub fn phase(&self) -> u8 {
        self.phase.load(Ordering::Relaxed)
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

type PumpSlot = Arc<Mutex<Option<Pump>>>;

pub struct Session {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pump: PumpSlot,
}

impl Session {
    pub fn start(
        profile: Profile,
        snapshots: Arc<Latest<Snapshot>>,
        meter: Arc<RpcMeter>,
        events: Arc<mhdn_game::EventQueue>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let pump = PumpSlot::default();
        let slot = Arc::clone(&pump);
        let thread = thread::spawn(move || {
            run_until_stopped(profile, snapshots, meter, events, flag, slot);
        });
        Self {
            stop,
            thread: Some(thread),
            pump,
        }
    }

    /// Handle for sampling from the caller's thread while a game is attached.
    pub fn pump(&self) -> Option<Pump> {
        self.pump.lock().ok().and_then(|slot| slot.clone())
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
    events: Arc<mhdn_game::EventQueue>,
    stop: Arc<AtomicBool>,
    pump: PumpSlot,
) {
    while !stop.load(Ordering::Relaxed) {
        match connect() {
            Ok(client) => {
                meter.set_up(true);
                meter.set_phase(PHASE_LIVE);
                let mem = Metered {
                    client,
                    meter: Arc::clone(&meter),
                };
                let join = spawn(
                    mem,
                    profile.clone(),
                    Arc::clone(&snapshots),
                    Arc::clone(&events),
                    sampler_sleep(),
                );
                set_pump(&pump, Some(join.pump()));
                while !stop.load(Ordering::Relaxed) {
                    if events.supported() == Some(false) {
                        meter.set_phase(PHASE_UNSUPPORTED);
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                set_pump(&pump, None);
                drop(join);
                meter.set_up(false);
                if !stop.load(Ordering::Relaxed) {
                    meter.set_phase(PHASE_WAITING);
                }
            }
            Err(ConnectFail::Rpc) => {
                meter.set_up(false);
                meter.set_phase(PHASE_DOWN);
                thread::sleep(Duration::from_secs(1));
            }
            Err(ConnectFail::NoGame) => {
                meter.set_up(false);
                meter.set_phase(PHASE_WAITING);
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn set_pump(slot: &PumpSlot, pump: Option<Pump>) {
    if let Ok(mut slot) = slot.lock() {
        *slot = pump;
    }
}

/// The sampler owns its thread, so the QoS is raised on its first sleep.
fn sampler_sleep() -> impl FnMut(Duration) + Send + 'static {
    let mut raised = false;
    move |period| {
        if !raised {
            raised = true;
            if !mhdn_platform::raise_thread_qos() {
                eprintln!("mhdn: sampler thread kept its default QoS");
            }
        }
        thread::sleep(period);
    }
}

enum ConnectFail {
    Rpc,
    NoGame,
}

fn connect() -> Result<RpcClient, ConnectFail> {
    let addr = RPC_ADDR
        .parse()
        .expect("127.0.0.1:45987 is a valid socket address");
    let mut client =
        RpcClient::connect(addr, Duration::from_millis(50)).map_err(|_| ConnectFail::Rpc)?;
    let processes = client.list_processes().map_err(|_| ConnectFail::Rpc)?;
    let pid = processes
        .iter()
        .find(|process| process.title_id == MHXX_JP_TITLE_ID)
        .map(|process| process.pid)
        .ok_or(ConnectFail::NoGame)?;
    client.select_process(pid).map_err(|_| ConnectFail::Rpc)?;
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
