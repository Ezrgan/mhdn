//! Turn damage events into world-anchored numbers and a hunt recount.

#![forbid(unsafe_code)]

use std::time::Duration;

use mhdn_fx::{HitKind, MagnitudeWindow, Pool, Spawn};
use mhdn_game::{Anchor, DamageEvent, DamageKind, MonsterState, Scene, HP_FROM_OBJECT};
use mhdn_proj::{project, Camera, EdgeMode, Projected, ScreenRect};
use mhdn_render::{glyph_quads, premul, Quad};

use crate::config::NumberAnchor;
use crate::settings::{CornerSettings, NumberSettings};

/// A forward jump larger than this is a save state, not a dropped sample.
const FRAME_JUMP: u32 = 90;
/// Game units are centimetres. A melee hit lands about this far in front of the hunter.
const WEAPON_REACH: f32 = 120.0;
/// Above the hunter's feet, so the number clears the hunter's own model.
const HIT_HEIGHT: f32 = 130.0;

pub struct CombatView {
    pool: Pool,
    magnitude: MagnitudeWindow,
    recount: Recount,
    seen: bool,
    last_scene: Scene,
    last_frame: u32,
    anchor: NumberAnchor,
    hunter: Option<[f32; 3]>,
}

impl Default for CombatView {
    fn default() -> Self {
        Self::new()
    }
}

impl CombatView {
    pub fn new() -> Self {
        Self {
            pool: Pool::new(),
            magnitude: MagnitudeWindow::new(),
            recount: Recount::default(),
            seen: false,
            last_scene: Scene::Disconnected,
            last_frame: 0,
            anchor: NumberAnchor::default(),
            hunter: None,
        }
    }

    pub fn set_anchor(&mut self, anchor: NumberAnchor) {
        self.anchor = anchor;
    }

    /// The local hunter's feet in the newest snapshot. `None` keeps numbers on the monster.
    pub fn set_hunter(&mut self, hunter: Option<[f32; 3]>) {
        self.hunter = hunter.filter(|pos| pos.iter().all(|value| value.is_finite()));
    }

    pub fn alive(&self) -> bool {
        self.pool.alive_count() > 0
    }

    pub fn clear(&mut self) {
        self.pool.clear();
        self.magnitude.clear();
        self.recount.clear();
    }

    /// Drop numbers when the hunt ends, the frame rewinds, or a save state jumps ahead.
    pub fn observe(&mut self, scene: Scene, frame: u32) -> Option<ResetReason> {
        if !self.seen {
            self.seen = true;
            self.last_scene = scene;
            self.last_frame = frame;
            return None;
        }
        let reason = reset_reason(self.last_scene, scene, self.last_frame, frame);
        if reason.is_some() {
            self.clear();
        }
        self.last_scene = scene;
        self.last_frame = frame;
        reason
    }

    /// One entry per event: the world anchor it spawned at, or `None` when it was dropped.
    pub fn ingest<F>(
        &mut self,
        events: &[DamageEvent],
        monsters: &[MonsterState],
        height: F,
    ) -> Ingested
    where
        F: Fn(u16, bool) -> f32,
    {
        if self.last_scene != Scene::InQuest {
            return Ingested {
                gated: true,
                anchors: vec![None; events.len()],
            };
        }
        let hunter = match self.anchor {
            NumberAnchor::Hunter => self.hunter,
            NumberAnchor::Monster => None,
        };
        let mut anchors = Vec::with_capacity(events.len());
        for event in events {
            if amount_above_max(event, monsters) {
                anchors.push(None);
                continue;
            }
            let world = anchor_world(event, monsters, &height, hunter);
            anchors.push(world);
            let Some(world) = world else {
                continue;
            };
            let style = self.magnitude.style(event.amount, hit_kind(event.kind));
            self.recount.add(event.amount);
            self.pool.spawn(Spawn {
                world,
                rgb: style.rgb,
                mag_scale: style.scale,
                amount: event.amount,
                seed: event_seed(event),
            });
        }
        Ingested {
            gated: false,
            anchors,
        }
    }

    pub fn alive_count(&self) -> usize {
        self.pool.alive_count()
    }

    pub fn recount_total(&self) -> u32 {
        self.recount.total
    }

    /// Where the live numbers project right now. Same test as `number_quads`.
    pub fn draw_stats(&self, camera: &Camera, top: ScreenRect) -> DrawStats {
        let mut stats = DrawStats::default();
        for live in self.pool.live() {
            if live.pose.alpha <= 0.0 {
                continue;
            }
            let world = glam::Vec3::new(live.world[0], live.world[1], live.world[2]);
            match project(world, camera, top, EdgeMode::Hide) {
                Projected::Visible { .. } => stats.drawn += 1,
                Projected::OffScreen { .. } => stats.offscreen += 1,
                Projected::Behind => stats.behind += 1,
            }
        }
        stats
    }

    pub fn tick(&mut self, dt: Duration) {
        let ms = dt.as_secs_f32() * 1000.0;
        self.pool.tick(ms);
        if self.last_scene == Scene::InQuest {
            self.recount.tick(ms);
        }
    }

    /// Hidden categories draw nothing. The category is read from the number's spawn color
    /// each frame, so a settings change also reaches numbers that are already flying.
    pub fn number_quads(
        &self,
        camera: &Camera,
        top: ScreenRect,
        number_px: f32,
        settings: &NumberSettings,
    ) -> Vec<Quad> {
        let mut quads = Vec::new();
        for live in self.pool.live() {
            if live.pose.alpha <= 0.0 {
                continue;
            }
            let Some(rgb) = settings.color_for(live.rgb) else {
                continue;
            };
            let world = glam::Vec3::new(live.world[0], live.world[1], live.world[2]);
            let Some(pos) = project(world, camera, top, EdgeMode::Hide).visible_pos() else {
                continue;
            };
            let px = (number_px * live.pose.scale * live.mag_scale).max(8.0);
            let color = premul(rgb, live.pose.alpha);
            let glyphs = glyph_quads(live.text, 0.0, 0.0, px, color);
            if glyphs.is_empty() {
                continue;
            }
            let min_x = glyphs.iter().map(|quad| quad.x).fold(f32::MAX, f32::min);
            let max_x = glyphs
                .iter()
                .map(|quad| quad.x + quad.w)
                .fold(0.0, f32::max);
            let dx = pos.x + live.scatter - (min_x + max_x) * 0.5;
            let dy = pos.y - live.pose.rise_px - px;
            for mut glyph in glyphs {
                glyph.x += dx;
                glyph.y += dy;
                quads.push(glyph);
            }
        }
        quads
    }

    /// `px` is the glyph height in physical pixels.
    pub fn recount_quads(&self, top: ScreenRect, px: f32, corner: &CornerSettings) -> Vec<Quad> {
        if self.recount.total == 0 {
            return Vec::new();
        }
        let Some(line) = corner.line(self.recount.total, self.recount.dps()) else {
            return Vec::new();
        };
        let margin = px * 0.5;
        glyph_quads(
            &line,
            top.x + margin,
            top.y + top.height - px - margin,
            px,
            premul([1.0, 1.0, 1.0], 0.9),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ingested {
    /// The view was not in a hunt, so nothing spawned.
    pub gated: bool,
    pub anchors: Vec<Option<[f32; 3]>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrawStats {
    pub drawn: u32,
    pub offscreen: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    LeftQuest,
    Rewind,
    Jump(u32),
}

pub fn reset_reason(prev: Scene, next: Scene, prev_frame: u32, frame: u32) -> Option<ResetReason> {
    if prev == Scene::InQuest && next != Scene::InQuest {
        return Some(ResetReason::LeftQuest);
    }
    if prev == Scene::InQuest && next == Scene::InQuest {
        if frame < prev_frame {
            return Some(ResetReason::Rewind);
        }
        let jump = frame.wrapping_sub(prev_frame);
        if jump > FRAME_JUMP {
            return Some(ResetReason::Jump(jump));
        }
    }
    None
}

/// An exact contact point wins. Otherwise, with a hunter, the number goes between the
/// hunter and the monster at weapon reach. Without one, it sits above the monster.
pub fn anchor_world<F>(
    event: &DamageEvent,
    monsters: &[MonsterState],
    height: F,
    hunter: Option<[f32; 3]>,
) -> Option<[f32; 3]>
where
    F: Fn(u16, bool) -> f32,
{
    if let Anchor::World(pos) = event.anchor {
        if pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite() {
            return Some([pos.x, pos.y, pos.z]);
        }
    }
    let monster = find_monster(event, monsters)?;
    let body = [monster.pos.x, monster.pos.y, monster.pos.z];
    if let Some(hunter) = hunter {
        return Some(near_hunter(hunter, body));
    }
    let rise = height(monster.key.species, monster.large);
    Some([body[0], body[1] + rise, body[2]])
}

/// On the ground line from the hunter toward the monster, never past halfway.
fn near_hunter(hunter: [f32; 3], monster: [f32; 3]) -> [f32; 3] {
    let dx = monster[0] - hunter[0];
    let dz = monster[2] - hunter[2];
    let dist = (dx * dx + dz * dz).sqrt();
    let step = if dist > 1.0 {
        WEAPON_REACH.min(dist * 0.5) / dist
    } else {
        0.0
    };
    [
        hunter[0] + dx * step,
        hunter[1] + HIT_HEIGHT,
        hunter[2] + dz * step,
    ]
}

fn amount_above_max(event: &DamageEvent, monsters: &[MonsterState]) -> bool {
    if let Some(monster) = find_monster(event, monsters) {
        return event.amount > monster.key.max_hp;
    }
    event.key.is_some_and(|key| event.amount > key.max_hp)
}

fn find_monster<'a>(event: &DamageEvent, monsters: &'a [MonsterState]) -> Option<&'a MonsterState> {
    if let Some(key) = event.key {
        if let Some(monster) = monsters.iter().find(|monster| {
            monster.key.struct_addr == key.struct_addr && monster.key.generation == key.generation
        }) {
            return Some(monster);
        }
    }
    monsters.iter().find(|monster| {
        monster.key.struct_addr == event.monster
            || monster.key.struct_addr == event.monster.wrapping_add(HP_FROM_OBJECT)
    })
}

fn hit_kind(kind: DamageKind) -> HitKind {
    match kind {
        DamageKind::Poison => HitKind::Poison,
        DamageKind::Hit | DamageKind::Unknown => HitKind::Hit,
    }
}

fn event_seed(event: &DamageEvent) -> u32 {
    event
        .seq
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(event.amount)
        .wrapping_add(event.monster)
}

#[derive(Debug, Clone, Copy, Default)]
struct Recount {
    total: u32,
    elapsed_ms: f32,
}

impl Recount {
    fn add(&mut self, amount: u32) {
        self.total = self.total.saturating_add(amount);
    }

    fn tick(&mut self, dt_ms: f32) {
        if self.total > 0 && dt_ms > 0.0 {
            self.elapsed_ms += dt_ms;
        }
    }

    fn dps(&self) -> f32 {
        if self.elapsed_ms < 250.0 {
            0.0
        } else {
            self.total as f32 / (self.elapsed_ms / 1000.0)
        }
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_fx::{LIFE_MS, ORANGE, POISON, SCATTER_PX};
    use mhdn_game::{
        CameraState, DamageConfidence, EventSource, FovUnit, MonsterKey, Vec3 as GameVec3,
    };
    use mhdn_proj::ScreenRect;

    fn monster(addr: u32, large: bool) -> MonsterState {
        MonsterState {
            key: MonsterKey {
                struct_addr: addr,
                species: 7,
                max_hp: if large { 3000 } else { 80 },
                generation: 1,
            },
            hp: 100,
            pos: GameVec3::new(0.0, 1.0, 0.0),
            visible: true,
            large,
            poisoned: false,
        }
    }

    fn event(amount: u32, anchor: Anchor, kind: DamageKind) -> DamageEvent {
        DamageEvent {
            seq: amount,
            guest_frame: 10,
            monster: 0x1000,
            amount,
            lr: 0,
            kind,
            source: EventSource::Passive,
            confidence: DamageConfidence::HpDelta,
            key: Some(MonsterKey {
                struct_addr: 0x1000,
                species: 7,
                max_hp: 3000,
                generation: 1,
            }),
            anchor,
            part_hp: None,
        }
    }

    fn hunt_camera() -> Camera {
        crate::hud::camera_from(&CameraState {
            eye: GameVec3::new(0.0, 2.0, 12.0),
            target: GameVec3::new(0.0, 1.0, 0.0),
            fov_y: 50.0,
            fov_unit: FovUnit::Deg,
        })
        .expect("camera")
    }

    #[test]
    fn a_bone_anchor_wins_and_a_passive_hit_rises_by_species_height() {
        let monsters = [monster(0x1000, true)];
        let bone = anchor_world(
            &event(
                12,
                Anchor::World(GameVec3::new(1.0, 2.0, 3.0)),
                DamageKind::Hit,
            ),
            &monsters,
            |_, _| 150.0,
            Some([500.0, 0.0, 0.0]),
        );
        assert_eq!(bone, Some([1.0, 2.0, 3.0]));
        let body = anchor_world(
            &event(12, Anchor::Unknown, DamageKind::Hit),
            &monsters,
            |species, large| {
                assert_eq!(species, 7);
                assert!(large);
                150.0
            },
            None,
        );
        assert_eq!(body, Some([0.0, 151.0, 0.0]));
    }

    #[test]
    fn with_a_hunter_the_number_sits_at_weapon_reach_toward_the_monster() {
        let monsters = [monster(0x1000, true)];
        let hit = event(12, Anchor::Unknown, DamageKind::Hit);
        let far = anchor_world(&hit, &monsters, |_, _| 150.0, Some([1000.0, 50.0, 0.0]));
        assert_eq!(far, Some([1000.0 - WEAPON_REACH, 50.0 + HIT_HEIGHT, 0.0]));

        let close = anchor_world(&hit, &monsters, |_, _| 150.0, Some([0.0, 0.0, 100.0]));
        assert_eq!(close, Some([0.0, HIT_HEIGHT, 50.0]));

        let on_top = anchor_world(&hit, &monsters, |_, _| 150.0, Some([0.0, 0.0, 0.0]));
        assert_eq!(on_top, Some([0.0, HIT_HEIGHT, 0.0]));
    }

    #[test]
    fn the_monster_mode_ignores_the_hunter() {
        let monsters = [monster(0x1000, true)];
        let hit = event(5, Anchor::Unknown, DamageKind::Hit);
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 1);
        view.set_hunter(Some([1000.0, 0.0, 0.0]));
        let near = view.ingest(std::slice::from_ref(&hit), &monsters, |_, _| 150.0);
        assert_eq!(near.anchors, vec![Some([880.0, HIT_HEIGHT, 0.0])]);

        view.set_anchor(NumberAnchor::Monster);
        let body = view.ingest(std::slice::from_ref(&hit), &monsters, |_, _| 150.0);
        assert_eq!(body.anchors, vec![Some([0.0, 151.0, 0.0])]);

        view.set_anchor(NumberAnchor::Hunter);
        view.set_hunter(Some([f32::NAN, 0.0, 0.0]));
        let fallback = view.ingest(&[hit], &monsters, |_, _| 150.0);
        assert_eq!(fallback.anchors, vec![Some([0.0, 151.0, 0.0])]);
    }

    #[test]
    fn hits_become_quads_above_the_monster_and_sum_into_the_recount() {
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 10);
        let monsters = [monster(0x1000, true)];
        view.ingest(
            &[event(
                40,
                Anchor::World(GameVec3::new(0.0, 1.0, 0.0)),
                DamageKind::Hit,
            )],
            &monsters,
            |_, _| 10.0,
        );
        assert_eq!(view.recount.total, 40);
        let top = ScreenRect::new(0.0, 0.0, 800.0, 480.0);
        let quads = view.number_quads(&hunt_camera(), top, 36.0, &NumberSettings::default());
        assert!(!quads.is_empty());
        assert!(quads.iter().all(|quad| top.contains(quad.x, quad.y)));
        let recount = view.recount_quads(top, 44.0, &CornerSettings::default());
        assert!(recount.len() > 4);
        assert!(recount.iter().all(|quad| top.contains(quad.x, quad.y)));
    }

    fn spawn_one(view: &mut CombatView, amount: u32, kind: DamageKind) {
        view.ingest(
            &[event(
                amount,
                Anchor::World(GameVec3::new(0.0, 1.0, 0.0)),
                kind,
            )],
            &[monster(0x1000, true)],
            |_, _| 10.0,
        );
    }

    #[test]
    fn a_hidden_category_produces_no_quad_but_still_counts_in_the_recount() {
        let top = ScreenRect::new(0.0, 0.0, 800.0, 480.0);
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 10);
        spawn_one(&mut view, 3, DamageKind::Poison);
        let shown = NumberSettings::default();
        assert!(!view
            .number_quads(&hunt_camera(), top, 36.0, &shown)
            .is_empty());

        let mut hidden = NumberSettings::default();
        hidden.poison.show = false;
        assert!(view
            .number_quads(&hunt_camera(), top, 36.0, &hidden)
            .is_empty());
        assert_eq!(view.recount.total, 3);

        // The same poison number comes back as soon as the setting does.
        assert!(!view
            .number_quads(&hunt_camera(), top, 36.0, &shown)
            .is_empty());
    }

    #[test]
    fn hiding_one_category_leaves_the_others_on_screen() {
        let top = ScreenRect::new(0.0, 0.0, 800.0, 480.0);
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 10);
        spawn_one(&mut view, 3, DamageKind::Poison);
        spawn_one(&mut view, 40, DamageKind::Hit);
        let all = view
            .number_quads(&hunt_camera(), top, 36.0, &NumberSettings::default())
            .len();
        let mut hidden = NumberSettings::default();
        hidden.poison.show = false;
        let without_poison = view.number_quads(&hunt_camera(), top, 36.0, &hidden).len();
        assert!(without_poison > 0);
        assert!(without_poison < all);
    }

    #[test]
    fn a_recolor_reaches_a_number_that_is_already_flying() {
        let top = ScreenRect::new(0.0, 0.0, 800.0, 480.0);
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 10);
        spawn_one(&mut view, 3, DamageKind::Poison);
        let mut green = NumberSettings::default();
        green.poison.rgb = [0.0, 1.0, 0.0];
        let quads = view.number_quads(&hunt_camera(), top, 36.0, &green);
        assert!(quads
            .iter()
            .all(|quad| quad.color[0] == 0.0 && quad.color[1] > 0.0));
    }

    #[test]
    fn the_corner_hides_when_both_parts_are_off() {
        let top = ScreenRect::new(0.0, 0.0, 800.0, 480.0);
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 10);
        spawn_one(&mut view, 40, DamageKind::Hit);
        let mut corner = CornerSettings::default();
        let both = view.recount_quads(top, 44.0, &corner).len();
        corner.show_dps = false;
        let total_only = view.recount_quads(top, 44.0, &corner).len();
        assert!(total_only > 0 && total_only < both);
        corner.show_total = false;
        assert!(view.recount_quads(top, 44.0, &corner).is_empty());
    }

    #[test]
    fn scatter_stays_inside_twelve_pixels() {
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 1);
        let monsters = [monster(0x1000, true)];
        for amount in 1..30 {
            view.ingest(
                &[event(amount, Anchor::Unknown, DamageKind::Hit)],
                &monsters,
                |_, _| 1.0,
            );
        }
        for live in view.pool.live() {
            assert!(live.scatter.abs() <= SCATTER_PX);
        }
    }

    #[test]
    fn leaving_the_quest_or_rewinding_drops_numbers() {
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 100);
        view.ingest(
            &[event(
                8,
                Anchor::World(GameVec3::new(0.0, 1.0, 0.0)),
                DamageKind::Hit,
            )],
            &[monster(0x1000, true)],
            |_, _| 1.0,
        );
        assert!(view.alive());
        view.observe(Scene::QuestEnd, 110);
        assert!(!view.alive());
        assert_eq!(view.recount.total, 0);

        view.observe(Scene::InQuest, 200);
        view.ingest(
            &[event(
                9,
                Anchor::World(GameVec3::new(0.0, 1.0, 0.0)),
                DamageKind::Hit,
            )],
            &[monster(0x1000, true)],
            |_, _| 1.0,
        );
        view.observe(Scene::InQuest, 50);
        assert!(!view.alive());
    }

    #[test]
    fn resets_name_their_reason_and_gated_events_report_no_anchor() {
        let mut view = CombatView::new();
        assert_eq!(view.observe(Scene::Village, 1), None);
        let hit = event(5, Anchor::Unknown, DamageKind::Hit);
        let gated = view.ingest(
            std::slice::from_ref(&hit),
            &[monster(0x1000, true)],
            |_, _| 1.0,
        );
        assert!(gated.gated);
        assert_eq!(gated.anchors, vec![None]);

        assert_eq!(view.observe(Scene::InQuest, 10), None);
        let spawned = view.ingest(&[hit], &[monster(0x1000, true)], |_, _| 1.0);
        assert_eq!(spawned.anchors, vec![Some([0.0, 2.0, 0.0])]);
        assert_eq!(
            view.observe(Scene::InQuest, 200),
            Some(ResetReason::Jump(190))
        );
        assert_eq!(view.observe(Scene::InQuest, 100), Some(ResetReason::Rewind));
        assert_eq!(
            view.observe(Scene::Loading, 101),
            Some(ResetReason::LeftQuest)
        );
    }

    #[test]
    fn poison_is_purple_and_a_late_spike_is_orange() {
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 1);
        let monsters = [monster(0x1000, true)];
        view.ingest(
            &[event(3, Anchor::Unknown, DamageKind::Poison)],
            &monsters,
            |_, _| 1.0,
        );
        let poison = view.pool.live().next().expect("poison");
        assert_eq!(poison.rgb, POISON);
        for amount in 1..=20 {
            view.ingest(
                &[event(amount, Anchor::Unknown, DamageKind::Hit)],
                &monsters,
                |_, _| 1.0,
            );
        }
        view.ingest(
            &[event(500, Anchor::Unknown, DamageKind::Hit)],
            &monsters,
            |_, _| 1.0,
        );
        let orange = view
            .pool
            .live()
            .find(|live| live.text == "500")
            .expect("spike");
        assert_eq!(orange.rgb, ORANGE);
        assert!((orange.mag_scale - 1.25).abs() < 0.001);
    }

    #[test]
    fn an_amount_above_max_hp_never_reaches_the_recount_or_the_magnitude_window() {
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 10);
        let monsters = [monster(0x1000, true)];
        let at_cap = event(
            3000,
            Anchor::World(GameVec3::new(0.0, 1.0, 0.0)),
            DamageKind::Hit,
        );
        view.ingest(std::slice::from_ref(&at_cap), &monsters, |_, _| 10.0);
        assert_eq!(view.recount.total, 3000);
        assert_eq!(view.magnitude.len(), 1);

        let huge = event(
            3001,
            Anchor::World(GameVec3::new(0.0, 1.0, 0.0)),
            DamageKind::Hit,
        );
        let ingested = view.ingest(std::slice::from_ref(&huge), &monsters, |_, _| 10.0);
        assert_eq!(ingested.anchors, vec![None]);
        assert_eq!(view.recount.total, 3000);
        assert_eq!(view.magnitude.len(), 1);
        assert_eq!(view.alive_count(), 1);
    }

    #[test]
    fn a_number_fades_out_instead_of_sticking() {
        let mut view = CombatView::new();
        view.observe(Scene::InQuest, 1);
        view.ingest(
            &[event(4, Anchor::Unknown, DamageKind::Hit)],
            &[monster(0x1000, true)],
            |_, _| 1.0,
        );
        view.tick(Duration::from_millis(LIFE_MS as u64));
        assert!(!view.alive());
    }
}
