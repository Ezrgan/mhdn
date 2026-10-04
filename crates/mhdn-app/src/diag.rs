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
use mhdn_game::{DamageConfidence, DamageEvent, DamageKind, EventSource, MonsterState, Scene};

/// Stamped into the start line so a log says which build wrote it.
pub const BUILD: &str = concat!("v0.4.0-dev+", env!("MHDN_GIT_SHA"));
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
    /// Guest frame-counter delta across this second.
    pub guest_fps: u32,
    /// 1 while the damage tap is installed.
    pub tap_installed: u8,
    /// Tap ring entries dropped during this second.
    pub lost_tap: u64,
    pub residual_n: u32,
    pub residual_sum: u32,
    pub tap_n: u32,
    pub tap_sum: u32,
}

pub fn format_second(stats: &SecondStats) -> String {
    format!(
        "sec rpc_req_s={:.0} rpc_lat_ms={:.2} snaps={} events={} spawned={} alive={} proj_in={} proj_off={} behind={} nocam={} redraws={} present_err={} present_skip={} pumped={} guest_fps={} tap_installed={} lost_tap={} residual_n={} residual_sum={} tap_n={} tap_sum={}",
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
        stats.guest_fps,
        stats.tap_installed,
        stats.lost_tap,
        stats.residual_n,
        stats.residual_sum,
        stats.tap_n,
        stats.tap_sum,
    )
}

/// Guest frames advanced between two observations of the frame counter.
pub fn guest_fps(origin: Option<u32>, latest: Option<u32>) -> u32 {
    match (origin, latest) {
        (Some(origin), Some(latest)) => latest.wrapping_sub(origin),
        _ => 0,
    }
}

#[derive(Debug, Default)]
struct FrameWindow {
    origin: Option<u32>,
    latest: Option<u32>,
}

impl FrameWindow {
    fn note(&mut self, frame: u32) {
        if self.origin.is_none() {
            self.origin = Some(frame);
        }
        self.latest = Some(frame);
    }

    fn take(&mut self) -> u32 {
        let fps = guest_fps(self.origin, self.latest);
        self.origin = self.latest;
        fps
    }
}

pub fn format_dmg(event: &DamageEvent) -> String {
    let (species, generation, max_hp, mon) = match event.key {
        Some(key) => (
            key.species.to_string(),
            key.generation.to_string(),
            key.max_hp.to_string(),
            key.struct_addr,
        ),
        None => (
            "-".to_string(),
            "-".to_string(),
            "-".to_string(),
            event.monster,
        ),
    };
    let mut line = format!(
        "dmg source={} kind={} confidence={} amount={} mon=0x{mon:08X} species={species} gen={generation} max_hp={max_hp} hp_before={} hp_after={} frames={} lr=0x{:08X}",
        source_name(event.source),
        kind_name(event.kind),
        confidence_name(event.confidence),
        event.amount,
        opt_u32(event.hp_before),
        opt_u32(event.hp_after),
        opt_u32(event.frames_since),
        event.lr,
    );
    if let Some(sp) = event.tap_sp {
        let mut words = sp.to_vec();
        if let Some(hi) = event.tap_sp_hi {
            words.extend_from_slice(&hi);
        }
        let r3 = event.tap_r3.unwrap_or(0);
        line.push_str(&format!(" r3=0x{r3:08X} sp={}", format_words(&words)));
    }
    line
}

pub fn format_scene(scene: Scene, monsters: &[MonsterState]) -> String {
    let mut line = format!("scene {scene:?}");
    for monster in monsters {
        line.push_str(&format!(
            " slot={} addr=0x{:08X} species={} hp={} max_hp={} gen={}",
            monster.slot,
            monster.key.struct_addr,
            monster.key.species,
            monster.hp,
            monster.key.max_hp,
            monster.key.generation
        ));
    }
    line
}

pub fn scene_identity(scene: Scene, monsters: &[MonsterState]) -> String {
    let mut key = format!("{scene:?}");
    for monster in monsters {
        key.push_str(&format!(
            "|{}:{:08X}:{}:{}:{}",
            monster.slot,
            monster.key.struct_addr,
            monster.key.species,
            monster.key.max_hp,
            monster.key.generation
        ));
    }
    key
}

fn kind_name(kind: DamageKind) -> &'static str {
    match kind {
        DamageKind::Hit => "hit",
        DamageKind::Poison => "poison",
        DamageKind::Status => "status",
        DamageKind::Topple => "topple",
        DamageKind::Unknown => "unknown",
    }
}

fn source_name(source: EventSource) -> &'static str {
    match source {
        EventSource::Tap => "tap",
        EventSource::Passive => "passive",
        EventSource::Plugin => "plugin",
    }
}

fn confidence_name(confidence: DamageConfidence) -> &'static str {
    match confidence {
        DamageConfidence::Exact => "exact",
        DamageConfidence::HpDelta => "hp_delta",
        DamageConfidence::AggregatedHpDelta => "aggregated_hp_delta",
    }
}

fn opt_u32(value: Option<u32>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "-".to_string(),
    }
}

fn format_words(words: &[u32]) -> String {
    words
        .iter()
        .map(|word| format!("0x{word:08X}"))
        .collect::<Vec<_>>()
        .join(",")
}

pub struct Diag {
    stats: SecondStats,
    next: Instant,
    rate: RateWindow,
    last_pumped: u64,
    seen: HashMap<&'static str, String>,
    scene_key: String,
    frames: FrameWindow,
    lost_seen: u64,
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
            scene_key: String::new(),
            frames: FrameWindow::default(),
            lost_seen: 0,
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

    pub fn note_guest_frame(&mut self, frame: u32) {
        self.frames.note(frame);
    }

    pub fn note_damage(&mut self, source: EventSource, amount: u32) {
        match source {
            EventSource::Passive => {
                self.stats.residual_n = self.stats.residual_n.saturating_add(1);
                self.stats.residual_sum = self.stats.residual_sum.saturating_add(amount);
            }
            EventSource::Tap => {
                self.stats.tap_n = self.stats.tap_n.saturating_add(1);
                self.stats.tap_sum = self.stats.tap_sum.saturating_add(amount);
            }
            EventSource::Plugin => {}
        }
    }

    /// `Some` when the scene or the monster list identity changed. Current HP is listed, not compared.
    pub fn scene_change(&mut self, scene: Scene, monsters: &[MonsterState]) -> Option<String> {
        let identity = scene_identity(scene, monsters);
        if self.scene_key == identity {
            return None;
        }
        self.scene_key = identity;
        Some(format_scene(scene, monsters))
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
    pub fn tick(
        &mut self,
        now: Instant,
        meter: &RpcMeter,
        alive: usize,
        pumped: u64,
        tap_installed: bool,
        lost_tap: u64,
    ) {
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
        stats.guest_fps = self.frames.take();
        stats.tap_installed = u8::from(tap_installed);
        stats.lost_tap = lost_tap.saturating_sub(self.lost_seen);
        self.lost_seen = lost_tap;
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
            guest_fps: 60,
            tap_installed: 1,
            lost_tap: 2,
            residual_n: 1,
            residual_sum: 14,
            tap_n: 3,
            tap_sum: 40,
        };
        assert_eq!(
            format_second(&stats),
            "sec rpc_req_s=412 rpc_lat_ms=0.31 snaps=58 events=2 spawned=1 alive=3 proj_in=2 proj_off=1 behind=0 nocam=4 redraws=60 present_err=5 present_skip=6 pumped=7 guest_fps=60 tap_installed=1 lost_tap=2 residual_n=1 residual_sum=14 tap_n=3 tap_sum=40"
        );
    }

    #[test]
    fn an_idle_second_still_prints_every_field() {
        let text = format_second(&SecondStats::default());
        assert!(text.starts_with("sec rpc_req_s=0 rpc_lat_ms=0.00 snaps=0"));
        assert!(text.ends_with(
            "guest_fps=0 tap_installed=0 lost_tap=0 residual_n=0 residual_sum=0 tap_n=0 tap_sum=0"
        ));
        assert_eq!(text.split(' ').count(), 22);
    }

    #[test]
    fn guest_fps_is_the_frame_counter_delta() {
        assert_eq!(guest_fps(Some(1_000), Some(1_060)), 60);
        assert_eq!(guest_fps(Some(0xFFFF_FFF0), Some(8)), 24);
        assert_eq!(guest_fps(None, Some(10)), 0);
        assert_eq!(guest_fps(Some(10), None), 0);
        let mut window = FrameWindow::default();
        assert_eq!(window.take(), 0);
        window.note(100);
        window.note(130);
        assert_eq!(window.take(), 30);
        window.note(190);
        assert_eq!(window.take(), 60);
        assert_eq!(window.take(), 0);
    }

    #[test]
    fn damage_lines_name_the_source_and_the_sample_hp() {
        use mhdn_game::{Anchor, DamageConfidence, DamageKind, EventSource, MonsterKey};

        let tap = DamageEvent {
            seq: 1,
            guest_frame: 10,
            monster: 0x0820_0000,
            amount: 42,
            lr: 0x008B_A260,
            kind: DamageKind::Hit,
            source: EventSource::Tap,
            confidence: DamageConfidence::Exact,
            key: Some(MonsterKey {
                struct_addr: 0x300E_0E38,
                species: 30,
                max_hp: 720,
                generation: 1,
            }),
            anchor: Anchor::Unknown,
            part_hp: Some(90),
            hp_before: Some(700),
            hp_after: Some(658),
            frames_since: Some(1),
            tap_r3: Some(0x0812_3456),
            tap_sp: Some([1, 0x5A, 0x082C_E744, 0x2A, 0x0E]),
            tap_sp_hi: None,
        };
        assert_eq!(
            format_dmg(&tap),
            "dmg source=tap kind=hit confidence=exact amount=42 mon=0x300E0E38 species=30 gen=1 max_hp=720 hp_before=700 hp_after=658 frames=1 lr=0x008BA260 r3=0x08123456 sp=0x00000001,0x0000005A,0x082CE744,0x0000002A,0x0000000E"
        );

        let mut passive = tap.clone();
        passive.source = EventSource::Passive;
        passive.confidence = DamageConfidence::AggregatedHpDelta;
        passive.amount = 682;
        passive.lr = 0;
        passive.key = Some(MonsterKey {
            struct_addr: 0x300E_0E38,
            species: 30,
            max_hp: 720,
            generation: 2,
        });
        passive.hp_before = Some(720);
        passive.hp_after = Some(38);
        passive.frames_since = Some(12);
        passive.tap_r3 = None;
        passive.tap_sp = None;
        assert_eq!(
            format_dmg(&passive),
            "dmg source=passive kind=hit confidence=aggregated_hp_delta amount=682 mon=0x300E0E38 species=30 gen=2 max_hp=720 hp_before=720 hp_after=38 frames=12 lr=0x00000000"
        );

        let mut unknown = tap;
        unknown.source = EventSource::Plugin;
        unknown.key = None;
        unknown.monster = 0x0820_0000;
        unknown.amount = 8;
        unknown.lr = 0;
        unknown.hp_before = None;
        unknown.hp_after = None;
        unknown.frames_since = None;
        unknown.tap_r3 = None;
        unknown.tap_sp = None;
        assert_eq!(
            format_dmg(&unknown),
            "dmg source=plugin kind=hit confidence=exact amount=8 mon=0x08200000 species=- gen=- max_hp=- hp_before=- hp_after=- frames=- lr=0x00000000"
        );

        let mut status = unknown;
        status.kind = DamageKind::Status;
        assert!(format_dmg(&status).contains("kind=status"));
        status.kind = DamageKind::Topple;
        assert!(format_dmg(&status).contains("kind=topple"));
        status.kind = DamageKind::Poison;
        assert!(format_dmg(&status).contains("kind=poison"));
    }

    #[test]
    fn scene_lines_list_monsters_and_ignore_current_hp() {
        use mhdn_game::{MonsterKey, MonsterState, Scene, Vec3};

        let monster = |hp, generation| MonsterState {
            key: MonsterKey {
                struct_addr: 0x300E_0E38,
                species: 14,
                max_hp: 920,
                generation,
            },
            hp,
            pos: Vec3::new(0.0, 0.0, 0.0),
            visible: true,
            large: true,
            poisoned: false,
            slot: 0,
        };
        assert_eq!(format_scene(Scene::Village, &[]), "scene Village");
        assert_eq!(
            format_scene(Scene::InQuest, &[monster(830, 1)]),
            "scene InQuest slot=0 addr=0x300E0E38 species=14 hp=830 max_hp=920 gen=1"
        );
        assert_eq!(
            scene_identity(Scene::InQuest, &[monster(830, 1)]),
            scene_identity(Scene::InQuest, &[monster(100, 1)])
        );
        assert_ne!(
            scene_identity(Scene::InQuest, &[monster(830, 1)]),
            scene_identity(Scene::InQuest, &[monster(830, 2)])
        );
        let mut diag = Diag::new();
        assert!(diag.scene_change(Scene::Village, &[]).is_some());
        assert!(diag.scene_change(Scene::Village, &[]).is_none());
        assert!(diag
            .scene_change(Scene::InQuest, &[monster(830, 1)])
            .is_some());
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

    #[test]
    fn the_build_stamp_names_this_dev_series() {
        assert!(BUILD.starts_with("v0.4.0-dev+"));
        assert_ne!(BUILD, "v0.4.0-dev+");
    }
}
