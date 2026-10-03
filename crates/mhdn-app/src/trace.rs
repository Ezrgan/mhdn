//! Opt-in diagnostics. `MHDN_TRACE=1` writes one line per event and reset, and a summary per second.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mhdn_game::{Anchor, DamageEvent};

use crate::config::config_dir;
use crate::numbers::{DrawStats, Ingested, ResetReason};

const SUMMARY_EVERY: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
struct Counters {
    snapshots: u32,
    events: u32,
    spawned: u32,
    dropped: u32,
    gated: u32,
    draws: u32,
    no_camera: u32,
    drawn: u32,
    offscreen: u32,
    behind: u32,
}

pub struct Trace {
    out: BufWriter<File>,
    started: Instant,
    next_summary: Instant,
    counters: Counters,
}

impl Trace {
    pub fn from_env() -> Option<Self> {
        let enabled =
            std::env::var("MHDN_TRACE").is_ok_and(|value| value != "0" && !value.is_empty());
        if !enabled {
            return None;
        }
        let dir = config_dir().join("logs");
        if let Err(err) = fs::create_dir_all(&dir) {
            eprintln!("mhdn: trace: {err}");
            return None;
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let path = dir.join(format!("trace-{stamp}.log"));
        match File::create(&path) {
            Ok(file) => {
                eprintln!("mhdn: trace -> {}", path.display());
                let now = Instant::now();
                Some(Self {
                    out: BufWriter::new(file),
                    started: now,
                    next_summary: now + SUMMARY_EVERY,
                    counters: Counters::default(),
                })
            }
            Err(err) => {
                eprintln!("mhdn: trace: {err}");
                None
            }
        }
    }

    fn t_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    pub fn snapshot(&mut self) {
        self.counters.snapshots += 1;
    }

    pub fn reset(&mut self, reason: ResetReason, frame: u32) {
        let t = self.t_ms();
        let _ = writeln!(self.out, "R\t{t}\t{reason:?}\tframe={frame}");
    }

    /// `screen` describes where a world anchor lands on the top screen right now.
    pub fn ingest<F>(&mut self, events: &[DamageEvent], ingested: &Ingested, screen: F)
    where
        F: Fn([f32; 3]) -> String,
    {
        let t = self.t_ms();
        for (event, world) in events.iter().zip(&ingested.anchors) {
            self.counters.events += 1;
            let anchor = match event.anchor {
                Anchor::Unknown => "-".to_string(),
                Anchor::World(pos) => format!("{:.1},{:.1},{:.1}", pos.x, pos.y, pos.z),
            };
            let outcome = if ingested.gated {
                self.counters.gated += 1;
                "gated".to_string()
            } else if let Some(world) = world {
                self.counters.spawned += 1;
                format!(
                    "spawn world={:.1},{:.1},{:.1} screen={}",
                    world[0],
                    world[1],
                    world[2],
                    screen(*world)
                )
            } else {
                self.counters.dropped += 1;
                "dropped".to_string()
            };
            let _ = writeln!(
                self.out,
                "E\t{t}\tframe={}\t{:?}\tamount={}\tmonster=0x{:08X}\tkey={}\tanchor={anchor}\t{outcome}",
                event.guest_frame,
                event.source,
                event.amount,
                event.monster,
                event
                    .key
                    .map(|key| format!("0x{:08X}", key.struct_addr))
                    .unwrap_or_else(|| "-".to_string()),
            );
        }
    }

    /// `None` means the frame had no usable camera.
    pub fn draw(&mut self, stats: Option<DrawStats>) {
        self.counters.draws += 1;
        match stats {
            Some(stats) => {
                self.counters.drawn += stats.drawn;
                self.counters.offscreen += stats.offscreen;
                self.counters.behind += stats.behind;
            }
            None => self.counters.no_camera += 1,
        }
    }

    pub fn due(&self) -> bool {
        Instant::now() >= self.next_summary
    }

    pub fn summary(&mut self, context: &str) {
        self.next_summary = Instant::now() + SUMMARY_EVERY;
        let t = self.t_ms();
        let c = std::mem::take(&mut self.counters);
        let _ = writeln!(
            self.out,
            "W\t{t}\tsnaps={}\tevents={}\tspawned={}\tdropped={}\tgated={}\tdraws={}\tnocam={}\tdrawn={}\toff={}\tbehind={}\t{}",
            c.snapshots,
            c.events,
            c.spawned,
            c.dropped,
            c.gated,
            c.draws,
            c.no_camera,
            c.drawn,
            c.offscreen,
            c.behind,
            context
        );
        let _ = self.out.flush();
    }
}
