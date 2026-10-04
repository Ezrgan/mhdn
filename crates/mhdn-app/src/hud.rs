//! Debug crosses on the hunter and each monster, plus the top-screen rectangle.

#![forbid(unsafe_code)]

use glam::Vec3;
use mhdn_game::{CameraState, FovUnit, MonsterState, Scene, Snapshot};
use mhdn_proj::{project, Camera, CameraParams, EdgeMode, ScreenRect};
use mhdn_render::{cross, premul, stroke_rect, text_quads, Quad};

const ARM: f32 = 8.0;
const THICK: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudStats {
    pub scene: &'static str,
    pub guest_frame: u32,
    pub requests_per_sec: f32,
    pub rpc_latency_ms: f32,
    pub rpc_up: bool,
}

impl Default for HudStats {
    fn default() -> Self {
        Self {
            scene: "DISCONNECTED",
            guest_frame: 0,
            requests_per_sec: 0.0,
            rpc_latency_ms: 0.0,
            rpc_up: false,
        }
    }
}

pub fn scene_label(scene: Scene) -> &'static str {
    match scene {
        Scene::Disconnected => "DISCONNECTED",
        Scene::NoGame => "NOGAME",
        Scene::Village => "VILLAGE",
        Scene::Loading => "LOADING",
        Scene::InQuest => "INQUEST",
        Scene::QuestEnd => "QUESTEND",
    }
}

pub fn camera_from(state: &CameraState) -> Option<Camera> {
    let fov = match state.fov_unit {
        FovUnit::Deg => state.fov_y.to_radians(),
        FovUnit::Rad => state.fov_y,
    };
    let params = CameraParams::from_radians(
        Vec3::new(state.eye.x, state.eye.y, state.eye.z),
        Vec3::new(state.target.x, state.target.y, state.target.z),
        fov,
    );
    params.is_usable().then(|| Camera::from(params))
}

pub fn build_hud(
    snapshot: Option<&Snapshot>,
    top: ScreenRect,
    stats: &HudStats,
    text_scale: f32,
) -> Vec<Quad> {
    let frame_color = premul([0.25, 0.95, 0.45], 0.85);
    let monster_color = premul([1.0, 0.45, 0.12], 0.95);
    let hunter_color = premul([0.25, 0.85, 1.0], 0.95);
    let text_color = premul([1.0, 1.0, 1.0], 0.92);

    let mut quads = Vec::new();
    quads.extend(stroke_rect(
        top.x,
        top.y,
        top.width,
        top.height,
        THICK,
        frame_color,
    ));

    if let Some(snapshot) = snapshot {
        if let Some(camera) = snapshot.camera.as_ref().and_then(camera_from) {
            for monster in snapshot
                .monsters
                .iter()
                .filter(|monster| monster.visible && monster.hp > 0)
            {
                push_anchor(
                    &mut quads,
                    monster_world(monster),
                    &camera,
                    top,
                    monster_color,
                );
            }
            if let Some(hunter) = snapshot.hunter_pos {
                push_anchor(
                    &mut quads,
                    Vec3::new(hunter.x, hunter.y, hunter.z),
                    &camera,
                    top,
                    hunter_color,
                );
            }
        }
    }

    let rpc = if stats.rpc_up { "UP" } else { "DOWN" };
    let line = format!(
        "{}  F {}  {:.0} REQ/S  {:.1} MS  RPC {}",
        stats.scene, stats.guest_frame, stats.requests_per_sec, stats.rpc_latency_ms, rpc
    );
    quads.extend(text_quads(
        &line,
        top.x + 8.0,
        top.y + 8.0,
        text_scale.max(1.0),
        text_color,
    ));
    quads
}

fn monster_world(monster: &MonsterState) -> Vec3 {
    Vec3::new(monster.pos.x, monster.pos.y, monster.pos.z)
}

fn push_anchor(
    quads: &mut Vec<Quad>,
    world: Vec3,
    camera: &Camera,
    top: ScreenRect,
    color: [f32; 4],
) {
    if let Some(pos) = project(world, camera, top, EdgeMode::Hide).visible_pos() {
        quads.extend(cross(pos.x, pos.y, ARM, THICK, color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_game::{MonsterKey, MonsterState, Vec3 as GameVec3};

    fn hunt() -> Snapshot {
        Snapshot {
            guest_frame: 120,
            host_us: 1_000_000,
            scene: Scene::InQuest,
            camera: Some(CameraState {
                eye: GameVec3::new(0.0, 2.0, 12.0),
                target: GameVec3::new(0.0, 1.0, 0.0),
                fov_y: 50.0,
                fov_unit: FovUnit::Deg,
            }),
            hunter_pos: Some(GameVec3::new(1.5, 0.0, 0.0)),
            monsters: vec![MonsterState {
                key: MonsterKey {
                    struct_addr: 0x1000,
                    species: 1,
                    max_hp: 1000,
                    generation: 1,
                },
                hp: 800,
                pos: GameVec3::new(0.0, 1.0, 0.0),
                visible: true,
                large: true,
                poisoned: false,
                slot: 0,
            }],
        }
    }

    #[test]
    fn crosses_land_inside_the_top_screen() {
        let snapshot = hunt();
        let top = ScreenRect::new(0.0, 0.0, 400.0, 240.0);
        let stats = HudStats {
            scene: scene_label(snapshot.scene),
            guest_frame: snapshot.guest_frame,
            rpc_up: true,
            ..HudStats::default()
        };
        let quads = build_hud(Some(&snapshot), top, &stats, 2.0);
        let monster = premul([1.0, 0.45, 0.12], 0.95);
        let hunter = premul([0.25, 0.85, 1.0], 0.95);
        assert!(quads
            .iter()
            .any(|quad| quad.color == monster && top.contains(quad.x, quad.y)));
        assert!(quads
            .iter()
            .any(|quad| quad.color == hunter && top.contains(quad.x, quad.y)));
        assert!(quads.len() > 8);
    }

    #[test]
    fn a_dead_monster_gets_no_cross() {
        let mut snapshot = hunt();
        snapshot.monsters[0].hp = 0;
        let top = ScreenRect::new(0.0, 0.0, 400.0, 240.0);
        let quads = build_hud(Some(&snapshot), top, &HudStats::default(), 2.0);
        let monster = premul([1.0, 0.45, 0.12], 0.95);
        assert!(!quads.iter().any(|quad| quad.color == monster));
    }

    #[test]
    fn a_missing_snapshot_still_draws_the_screen_rect_and_status() {
        let top = ScreenRect::new(10.0, 12.0, 200.0, 120.0);
        let quads = build_hud(None, top, &HudStats::default(), 2.0);
        assert!(quads.iter().any(|quad| quad.x == 10.0 && quad.y == 12.0));
        assert!(quads.len() > 4);
    }
}
