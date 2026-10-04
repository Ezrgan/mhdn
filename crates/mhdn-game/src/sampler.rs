//! Sampler thread. In a hunt it polls at 60 Hz; everywhere else, and when RPC fails, it backs off.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mhdn_rpc::MemorySource;

use crate::damage::DamageEvent;
use crate::model::Snapshot;
use crate::pipeline::Pipeline;
use crate::profile::Profile;
use crate::scene::Scene;
use crate::tap::PatchMemory;

pub fn sample_period(scene: Scene, failures: u32) -> Duration {
    if failures > 0 {
        let shift = failures.saturating_sub(1).min(4);
        return Duration::from_millis(250_u64 << shift);
    }
    if scene == Scene::InQuest {
        Duration::from_nanos(1_000_000_000 / 60)
    } else {
        Duration::from_millis(250)
    }
}

#[derive(Debug)]
pub struct EventQueue {
    events: Mutex<VecDeque<DamageEvent>>,
    notes: Mutex<VecDeque<String>>,
    capacity: usize,
    dropped: AtomicU64,
    /// 0 unknown, 1 fingerprint matched, 2 this build is not the profile.
    supported: AtomicU8,
    /// 0 not installed, 1 installed, 2 blocked by a foreign hook or an occupied cave.
    tap: AtomicU8,
    lost_tap: AtomicU64,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: Mutex::new(VecDeque::new()),
            notes: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
            dropped: AtomicU64::new(0),
            supported: AtomicU8::new(0),
            tap: AtomicU8::new(0),
            lost_tap: AtomicU64::new(0),
        }
    }

    pub fn push(&self, event: DamageEvent) {
        let mut queue = self.events.lock().expect("event queue");
        if queue.len() == self.capacity {
            queue.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        queue.push_back(event);
    }

    pub fn drain(&self) -> Vec<DamageEvent> {
        self.events.lock().expect("event queue").drain(..).collect()
    }

    pub fn push_note(&self, line: String) {
        let mut notes = self.notes.lock().expect("diag notes");
        if notes.len() == 256 {
            notes.pop_front();
        }
        notes.push_back(line);
    }

    pub fn drain_notes(&self) -> Vec<String> {
        self.notes.lock().expect("diag notes").drain(..).collect()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn set_supported(&self, ok: bool) {
        self.supported
            .store(if ok { 1 } else { 2 }, Ordering::Relaxed);
    }

    pub fn supported(&self) -> Option<bool> {
        match self.supported.load(Ordering::Relaxed) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }

    pub fn set_tap(&self, installed: bool, blocked: bool) {
        let state = if installed {
            1
        } else if blocked {
            2
        } else {
            0
        };
        self.tap.store(state, Ordering::Relaxed);
    }

    pub fn tap_installed(&self) -> bool {
        self.tap.load(Ordering::Relaxed) == 1
    }

    pub fn tap_blocked(&self) -> bool {
        self.tap.load(Ordering::Relaxed) == 2
    }

    pub fn set_lost_tap(&self, lost: u64) {
        self.lost_tap.store(lost, Ordering::Relaxed);
    }

    pub fn lost_tap(&self) -> u64 {
        self.lost_tap.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct Latest<T> {
    value: Mutex<Option<T>>,
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self {
            value: Mutex::new(None),
        }
    }
}

impl<T> Latest<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, value: T) {
        *self.value.lock().expect("snapshot") = Some(value);
    }

    pub fn take(&self) -> Option<T> {
        self.value.lock().expect("snapshot").take()
    }
}

/// Pipeline, memory and outputs of one session. The sampler thread and any [`Pump`]
/// share it, and whichever comes first once the period has elapsed takes the sample.
pub struct Sampler<M> {
    mem: M,
    profile: Profile,
    pipeline: Pipeline,
    snapshots: Arc<Latest<Snapshot>>,
    events: Arc<EventQueue>,
    started: Instant,
    last_tick: Option<Instant>,
    closed: bool,
    ticks: u64,
}

impl<M: MemorySource + PatchMemory> Sampler<M> {
    pub fn new(
        mem: M,
        profile: Profile,
        snapshots: Arc<Latest<Snapshot>>,
        events: Arc<EventQueue>,
    ) -> Self {
        Self {
            mem,
            profile,
            pipeline: Pipeline::from_env(),
            snapshots,
            events,
            started: Instant::now(),
            last_tick: None,
            closed: false,
            ticks: 0,
        }
    }

    pub fn period(&self) -> Duration {
        sample_period(self.pipeline.scene(), self.pipeline.failures())
    }

    /// Time left until the next sample is due. Zero when it is due now.
    pub fn wait(&self, now: Instant) -> Duration {
        match self.last_tick {
            Some(last) => self
                .period()
                .saturating_sub(now.saturating_duration_since(last)),
            None => Duration::ZERO,
        }
    }

    /// Samples once if the period has elapsed. Returns true when it sampled.
    pub fn tick_if_due(&mut self, now: Instant) -> bool {
        if self.closed || !self.wait(now).is_zero() {
            return false;
        }
        self.tick(now);
        true
    }

    fn tick(&mut self, now: Instant) {
        self.last_tick = Some(now);
        self.ticks += 1;
        let host_us =
            u64::try_from(now.saturating_duration_since(self.started).as_micros()).unwrap_or(0);
        if let Ok(sample) = self.pipeline.poll(&mut self.mem, &self.profile, host_us) {
            if let Some(snapshot) = sample.snapshot {
                self.snapshots.publish(snapshot);
            }
            for event in sample.events {
                self.events.push(event);
            }
            for line in self.pipeline.drain_diag_lines() {
                self.events.push_note(line);
            }
        }
        let _ = self.pipeline.maintain_tap(&mut self.mem, &self.profile);
        if let Some(ok) = self.pipeline.supported() {
            self.events.set_supported(ok);
        }
        self.events
            .set_tap(self.pipeline.tap_installed(), self.pipeline.tap_blocked());
        self.events.set_lost_tap(self.pipeline.lost_tap());
    }

    /// A healthy hunt, the only state where a caller outside the sampler thread may sample.
    fn pumpable(&self) -> bool {
        !self.closed && self.pipeline.failures() == 0 && self.pipeline.scene() == Scene::InQuest
    }

    /// Removes the tap. Later ticks, from any caller, do nothing.
    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            self.pipeline.shutdown_tap(&mut self.mem);
        }
    }
}

trait Step: Send {
    fn pump(&mut self, now: Instant) -> bool;
    fn ticks(&self) -> u64;
}

impl<M: MemorySource + PatchMemory + Send> Step for Sampler<M> {
    fn pump(&mut self, now: Instant) -> bool {
        self.pumpable() && self.tick_if_due(now)
    }

    fn ticks(&self) -> u64 {
        self.ticks
    }
}

/// Lets a thread the OS keeps on schedule, such as the one presenting frames,
/// take samples when the sampler thread is being woken late.
#[derive(Clone)]
pub struct Pump {
    step: Arc<Mutex<dyn Step>>,
}

impl Pump {
    /// Samples if a hunt is running and the period has elapsed. Never waits for the lock.
    pub fn pump(&self) -> bool {
        match self.step.try_lock() {
            Ok(mut step) => step.pump(Instant::now()),
            Err(_) => false,
        }
    }

    /// Samples taken so far, by any caller.
    pub fn ticks(&self) -> u64 {
        self.step.lock().map(|step| step.ticks()).unwrap_or(0)
    }
}

pub struct SamplerJoin {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pump: Pump,
}

impl SamplerJoin {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn pump(&self) -> Pump {
        self.pump.clone()
    }
}

impl Drop for SamplerJoin {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn spawn<M, S>(
    mem: M,
    profile: Profile,
    snapshots: Arc<Latest<Snapshot>>,
    events: Arc<EventQueue>,
    sleep: S,
) -> SamplerJoin
where
    M: MemorySource + PatchMemory + Send + 'static,
    S: FnMut(Duration) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let shared = Arc::new(Mutex::new(Sampler::new(mem, profile, snapshots, events)));
    let step: Arc<Mutex<dyn Step>> = shared.clone();
    let thread = thread::spawn(move || run_session(&shared, &flag, sleep));
    SamplerJoin {
        stop,
        thread: Some(thread),
        pump: Pump { step },
    }
}

pub fn run_session<M, S>(sampler: &Mutex<Sampler<M>>, stop: &AtomicBool, mut sleep: S)
where
    M: MemorySource + PatchMemory,
    S: FnMut(Duration),
{
    while !stop.load(Ordering::Relaxed) {
        let wait = {
            let mut sampler = sampler.lock().expect("sampler");
            let now = Instant::now();
            sampler.tick_if_due(now);
            sampler.wait(Instant::now()).max(MIN_SLEEP)
        };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        sleep(wait);
    }
    sampler.lock().expect("sampler").close();
}

/// Keeps the thread from spinning when another caller has just sampled.
const MIN_SLEEP: Duration = Duration::from_millis(1);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::damage::{DamageConfidence, DamageKind, EventSource};
    use crate::tap::HOOK_ADDR;
    use mhdn_rpc::MemorySource;

    fn event(seq: u32) -> DamageEvent {
        DamageEvent {
            seq,
            guest_frame: 0,
            monster: 0,
            amount: 1,
            lr: 0,
            kind: DamageKind::Hit,
            source: EventSource::Passive,
            confidence: DamageConfidence::HpDelta,
            key: None,
            anchor: crate::model::Anchor::Unknown,
            part_hp: None,
            hp_before: None,
            hp_after: None,
            frames_since: None,
            tap_r3: None,
            tap_sp: None,
            tap_sp_hi: None,
        }
    }

    #[test]
    fn period_backs_off_outside_a_hunt_and_when_rpc_fails() {
        assert_eq!(
            sample_period(Scene::InQuest, 0),
            Duration::from_nanos(1_000_000_000 / 60)
        );
        assert_eq!(sample_period(Scene::Village, 0), Duration::from_millis(250));
        assert_eq!(sample_period(Scene::InQuest, 1), Duration::from_millis(250));
        assert_eq!(
            sample_period(Scene::InQuest, 5),
            Duration::from_millis(4000)
        );
        assert_eq!(
            sample_period(Scene::InQuest, 9),
            Duration::from_millis(4000)
        );
    }

    #[test]
    fn a_full_queue_drops_the_oldest_event() {
        let queue = EventQueue::new(2);
        queue.push(event(1));
        queue.push(event(2));
        queue.push(event(3));
        assert_eq!(queue.dropped(), 1);
        let drained = queue.drain();
        assert_eq!(drained[0].seq, 2);
        assert_eq!(drained[1].seq, 3);
    }

    #[test]
    fn notes_drain_in_order() {
        let queue = EventQueue::new(2);
        queue.push_note("hp_bad".to_string());
        queue.push_note("dmg_drop".to_string());
        assert_eq!(
            queue.drain_notes(),
            vec!["hp_bad".to_string(), "dmg_drop".to_string()]
        );
        assert!(queue.drain_notes().is_empty());
    }

    #[test]
    fn the_session_installs_the_tap_and_removes_it_on_exit() {
        use crate::sparse::SparseMemory;
        use crate::support::{live_profile, place_monster, stage_hunt};
        use crate::tap::{hook_branch, EXPECTED_HOOK, EXPECTED_HP_STORE, EXPECTED_NEXT};
        use std::sync::Mutex as StdMutex;

        let mut profile = live_profile();
        profile.meta.fingerprint.clear();
        let mut image = SparseMemory::new();
        stage_hunt(&mut image, &profile, 1, true, false);
        place_monster(&mut image, 0, 500, 500, 1, [0.0, 0.0, 0.0], 0);
        image.write_u32(HOOK_ADDR, EXPECTED_HOOK);
        image.write_u32(0x008D_03EC, EXPECTED_NEXT);
        image.write_u32(0x008D_03FC, EXPECTED_HP_STORE);
        let shared = Arc::new(StdMutex::new(image));
        let saw = Arc::new(AtomicBool::new(false));
        let sleeps = Arc::new(AtomicU64::new(0));
        let mem = SharedMem(Arc::clone(&shared));
        let snapshots = Arc::new(Latest::new());
        let events = Arc::new(EventQueue::new(8));
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let saw_flag = Arc::clone(&saw);
        let sleep_count = Arc::clone(&sleeps);
        let image_for_sleep = Arc::clone(&shared);
        let thread = thread::spawn(move || {
            let sampler = StdMutex::new(Sampler::new(mem, profile, snapshots, events));
            run_session(&sampler, &flag, move |wait| {
                thread::sleep(wait);
                let n = sleep_count.fetch_add(1, Ordering::Relaxed);
                if n == 3 {
                    let hook = image_for_sleep
                        .lock()
                        .expect("mem")
                        .read_u32(HOOK_ADDR)
                        .unwrap_or(0);
                    saw_flag.store(hook == hook_branch(), Ordering::Relaxed);
                }
                if n >= 4 {
                    stop.store(true, Ordering::Relaxed);
                }
            });
        });
        thread.join().unwrap();
        assert!(saw.load(Ordering::Relaxed), "tap branch was not installed");
        let restored = shared_hook(&shared);
        assert_eq!(restored, EXPECTED_HOOK);
    }

    fn sampler_for(hunting: bool) -> Sampler<crate::sparse::SparseMemory> {
        use crate::sparse::SparseMemory;
        use crate::support::{live_profile, place_monster, stage_hunt};

        let mut profile = live_profile();
        profile.meta.fingerprint.clear();
        let mut image = SparseMemory::new();
        stage_hunt(&mut image, &profile, 1, false, false);
        if hunting {
            place_monster(&mut image, 0, 500, 500, 1, [0.0, 0.0, 0.0], 0);
        }
        Sampler::new(
            image,
            profile,
            Arc::new(Latest::new()),
            Arc::new(EventQueue::new(8)),
        )
    }

    #[test]
    fn a_sample_is_taken_once_per_period_and_never_after_close() {
        let mut sampler = sampler_for(true);
        let t0 = Instant::now();
        assert!(sampler.tick_if_due(t0));
        assert!(!sampler.tick_if_due(t0 + Duration::from_millis(1)));
        assert!(sampler.tick_if_due(t0 + Duration::from_secs(1)));
        sampler.close();
        assert!(!sampler.tick_if_due(t0 + Duration::from_secs(5)));
        assert_eq!(sampler.ticks, 2);
    }

    #[test]
    fn the_pump_samples_only_in_a_hunt() {
        let mut village = sampler_for(false);
        let mut hunt = sampler_for(true);
        let mut now = Instant::now();
        for _ in 0..8 {
            village.tick_if_due(now);
            hunt.tick_if_due(now);
            now += Duration::from_secs(1);
        }
        assert!(!Step::pump(&mut village, now));
        assert!(Step::pump(&mut hunt, now));
        hunt.close();
        assert!(!Step::pump(&mut hunt, now + Duration::from_secs(1)));
    }

    fn shared_hook(mem: &Arc<std::sync::Mutex<crate::sparse::SparseMemory>>) -> u32 {
        mem.lock().expect("mem").read_u32(HOOK_ADDR).unwrap_or(0)
    }

    struct SharedMem(Arc<std::sync::Mutex<crate::sparse::SparseMemory>>);

    impl MemorySource for SharedMem {
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> mhdn_rpc::Result<()> {
            let mut guard = self.0.lock().expect("mem");
            MemorySource::read(&mut *guard, addr, buf)
        }
    }

    impl PatchMemory for SharedMem {
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), crate::tap::TapError> {
            let mut guard = self.0.lock().expect("mem");
            PatchMemory::read(&mut *guard, addr, buf)
        }

        fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), crate::tap::TapError> {
            self.0.lock().expect("mem").write(addr, data)
        }
    }
}
