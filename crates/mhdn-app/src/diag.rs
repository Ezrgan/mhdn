//! Always-on diagnostic log. One file per run, one line per fact, flushed as it is written.
//!
//! The file answers, without a debugger: is the Azahar window found, is it fullscreen, is
//! the overlay parked, do hit events arrive, do numbers project off-screen, does present
//! fail. RPC reads are never logged one by one. They are counted and summed once a second.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::numbers::DrawStats;
use crate::session::{sample_rate, RateWindow, RpcMeter};

/// Stamped into the start line so a log says which build wrote it.
pub const BUILD: &str = "v0.3.4";
const SECOND: Duration = Duration::from_secs(1);

struct Sink {
    file: File,
    started: Instant,
    path: PathBuf,
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

/// Opens `diag-<unix>.log`. A failure leaves the app running without a log.
pub fn init() {
    let Some(dir) = mhdn_platform::log_dir() else {
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("diag-{unix}.log"));
    let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let _ = SINK.set(Mutex::new(Sink {
        file,
        started: Instant::now(),
        path,
    }));
    line(&format!("log opened unix={unix}"));
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        line(&format!("PANIC {info}"));
        previous(info);
    }));
}

/// What the settings window shows.
pub fn path_text() -> String {
    match SINK.get() {
        Some(sink) => sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .path
            .display()
            .to_string(),
        None => "unavailable".to_string(),
    }
}

/// Appends one line and hands it to the OS right away, so a crash keeps it.
pub fn line(text: &str) {
    let Some(sink) = SINK.get() else {
        return;
    };
    let mut sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let elapsed = sink.started.elapsed().as_secs_f64();
    let _ = sink
        .file
        .write_all(format!("[{elapsed:9.3}] {text}\n").as_bytes());
    let _ = sink.file.flush();
}

/// Everything counted in one second.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SecondStats {
    pub rpc_per_sec: f32,
    pub rpc_latency_ms: f32,
    pub snapshots: u32,
    pub events: u32,
    pub spawned: u32,
    pub alive: usize,
    /// Most numbers projected inside the screen in any one frame this second.
    pub proj_in: u32,
    /// Most numbers projected outside the screen in any one frame this second.
    pub proj_off: u32,
    /// Most numbers behind the camera in any one frame this second.
    pub behind: u32,
    /// Frames this second that had live numbers and no camera.
    pub nocam: u32,
    pub redraws: u32,
    /// Outdated, Lost or Validation from the surface.
    pub present_err: u32,
    /// Timeout or Occluded from the surface: the frame was skipped without an error.
    pub present_skip: u32,
    pub pumped: u64,
}

pub fn format_second(stats: &SecondStats) -> String {
    format!(
        "sec rpc_req_s={:.0} rpc_lat_ms={:.2} snaps={} events={} spawned={} alive={} proj_in={} proj_off={} behind={} nocam={} redraws={} present_err={} present_skip={} pumped={}",
        stats.rpc_per_sec,
        stats.rpc_latency_ms,
        stats.snapshots,
        stats.events,
        stats.spawned,
        stats.alive,
        stats.proj_in,
        stats.proj_off,
        stats.behind,
        stats.nocam,
        stats.redraws,
        stats.present_err,
        stats.present_skip,
        stats.pumped,
    )
}

pub struct Diag {
    stats: SecondStats,
    next: Instant,
    rate: RateWindow,
    last_pumped: u64,
    seen: HashMap<&'static str, String>,
    first_event: bool,
    first_present_error: bool,
}

impl Diag {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            stats: SecondStats::default(),
            next: now + SECOND,
            rate: RateWindow::new(now),
            last_pumped: 0,
            seen: HashMap::new(),
            first_event: false,
            first_present_error: false,
        }
    }

    pub fn snapshot(&mut self) {
        self.stats.snapshots += 1;
    }

    pub fn events(&mut self, popped: usize) {
        self.stats.events += u32::try_from(popped).unwrap_or(u32::MAX);
    }

    pub fn spawned(&mut self, count: usize) {
        self.stats.spawned += u32::try_from(count).unwrap_or(u32::MAX);
    }

    pub fn redraw(&mut self) {
        self.stats.redraws += 1;
    }

    /// `None` is a frame with live numbers and no usable camera.
    pub fn projected(&mut self, drawn: Option<DrawStats>) {
        match drawn {
            Some(drawn) => {
                self.stats.proj_in = self.stats.proj_in.max(drawn.drawn);
                self.stats.proj_off = self.stats.proj_off.max(drawn.offscreen);
                self.stats.behind = self.stats.behind.max(drawn.behind);
            }
            None => self.stats.nocam += 1,
        }
    }

    pub fn present(&mut self, errors: u32, skipped: u32, first_error: Option<String>) {
        self.stats.present_err += errors;
        self.stats.present_skip += skipped;
        if let Some(error) = first_error {
            if !self.first_present_error {
                self.first_present_error = true;
                line(&format!("present error (first) {error}"));
            }
        }
    }

    /// True once, for the first damage event this run.
    pub fn first_event(&mut self) -> bool {
        !std::mem::replace(&mut self.first_event, true)
    }

    /// Logs `key value` when `value` differs from the last one logged for `key`.
    pub fn changed(&mut self, key: &'static str, value: String) {
        if let Some(text) = self.change_line(key, value) {
            line(&text);
        }
    }

    fn change_line(&mut self, key: &'static str, value: String) -> Option<String> {
        if self.seen.get(key) == Some(&value) {
            return None;
        }
        let text = format!("{key} {value}");
        self.seen.insert(key, value);
        Some(text)
    }

    /// Writes the one-second line when a second has passed.
    pub fn tick(&mut self, now: Instant, meter: &RpcMeter, alive: usize, pumped: u64) {
        if now < self.next {
            return;
        }
        self.next = now + SECOND;
        sample_rate(&mut self.rate, meter, now);
        let mut stats = std::mem::take(&mut self.stats);
        stats.rpc_per_sec = self.rate.requests_per_sec;
        stats.rpc_latency_ms = self.rate.latency_ms;
        stats.alive = alive;
        stats.pumped = pumped.saturating_sub(self.last_pumped);
        self.last_pumped = pumped;
        line(&format_second(&stats));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_one_second_line_has_a_fixed_field_order() {
        let stats = SecondStats {
            rpc_per_sec: 412.4,
            rpc_latency_ms: 0.314,
            snapshots: 58,
            events: 2,
            spawned: 1,
            alive: 3,
            proj_in: 2,
            proj_off: 1,
            behind: 0,
            nocam: 4,
            redraws: 60,
            present_err: 5,
            present_skip: 6,
            pumped: 7,
        };
        assert_eq!(
            format_second(&stats),
            "sec rpc_req_s=412 rpc_lat_ms=0.31 snaps=58 events=2 spawned=1 alive=3 proj_in=2 proj_off=1 behind=0 nocam=4 redraws=60 present_err=5 present_skip=6 pumped=7"
        );
    }

    #[test]
    fn an_idle_second_still_prints_every_field() {
        let text = format_second(&SecondStats::default());
        assert!(text.starts_with("sec rpc_req_s=0 rpc_lat_ms=0.00 snaps=0"));
        assert_eq!(text.split(' ').count(), 15);
    }

    #[test]
    fn a_change_is_logged_once_until_the_value_moves() {
        let mut diag = Diag::new();
        assert_eq!(
            diag.change_line("azahar", "lost".to_string()).as_deref(),
            Some("azahar lost")
        );
        assert_eq!(diag.change_line("azahar", "lost".to_string()), None);
        assert_eq!(
            diag.change_line("azahar", "found".to_string()).as_deref(),
            Some("azahar found")
        );
    }

    #[test]
    fn the_first_damage_event_is_flagged_once() {
        let mut diag = Diag::new();
        assert!(diag.first_event());
        assert!(!diag.first_event());
    }
}
