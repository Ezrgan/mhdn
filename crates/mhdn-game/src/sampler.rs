//! Sampler thread. In a hunt it polls at 60 Hz; everywhere else, and when RPC fails, it backs off.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
    capacity: usize,
    dropped: AtomicU64,
    /// 0 unknown, 1 fingerprint matched, 2 this build is not the profile.
    supported: AtomicU8,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
            dropped: AtomicU64::new(0),
            supported: AtomicU8::new(0),
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

pub struct SamplerJoin {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SamplerJoin {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
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
    let thread = thread::spawn(move || run_session(mem, profile, flag, snapshots, events, sleep));
    SamplerJoin {
        stop,
        thread: Some(thread),
    }
}

pub fn run_session<M, S>(
    mut mem: M,
    profile: Profile,
    stop: Arc<AtomicBool>,
    snapshots: Arc<Latest<Snapshot>>,
    events: Arc<EventQueue>,
    mut sleep: S,
) where
    M: MemorySource + PatchMemory,
    S: FnMut(Duration),
{
    let mut pipeline = Pipeline::new();
    let mut host_us = 0u64;
    while !stop.load(Ordering::Relaxed) {
        let period = sample_period(pipeline.scene(), pipeline.failures());
        if let Ok(sample) = pipeline.poll(&mut mem, &profile, host_us) {
            if let Some(snapshot) = sample.snapshot {
                snapshots.publish(snapshot);
            }
            for event in sample.events {
                events.push(event);
            }
        }
        let _ = pipeline.maintain_tap(&mut mem, &profile);
        if let Some(ok) = pipeline.supported() {
            events.set_supported(ok);
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
        sleep(period);
        host_us = host_us.saturating_add(u64::try_from(period.as_micros()).unwrap_or(u64::MAX));
    }
    pipeline.shutdown_tap(&mut mem);
}

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
            run_session(mem, profile, flag, snapshots, events, move |_| {
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
