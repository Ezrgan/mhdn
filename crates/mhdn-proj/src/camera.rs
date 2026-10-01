//! World-to-NDC projection from camera parameters or explicit matrices.
//!
//! MT Framework is Y-up and right-handed. The top 3DS screen is 400×240, so the
//! projection aspect is 5:3. Depth range does not affect x/y.

use glam::{Mat4, Vec2, Vec3, Vec4};

/// Top screen width in guest pixels.
pub const TOP_WIDTH: f32 = 400.0;
/// Top screen height in guest pixels.
pub const TOP_HEIGHT: f32 = 240.0;
/// Top screen width / height.
pub const TOP_ASPECT: f32 = TOP_WIDTH / TOP_HEIGHT;

const CLIP_EPSILON: f32 = 1.0e-4;

/// How a point that lands outside the top screen is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeMode {
    /// Drop the point.
    Hide,
    /// Pin the point to the screen rectangle.
    Clamp,
}

/// 90° fix applied in NDC when a GPU matrix includes the 3DS portrait rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NdcRotation {
    #[default]
    None,
    /// `(x, y) → (−y, x)`.
    Ccw90,
    /// `(x, y) → (y, −x)`.
    Cw90,
}

impl NdcRotation {
    pub fn apply(self, ndc: Vec2) -> Vec2 {
        match self {
            Self::None => ndc,
            Self::Ccw90 => Vec2::new(-ndc.y, ndc.x),
            Self::Cw90 => Vec2::new(ndc.y, -ndc.x),
        }
    }
}

/// Pick the rotation that brings `projected` closest to `expected`.
pub fn guess_ndc_rotation(projected: Vec2, expected: Vec2) -> NdcRotation {
    [NdcRotation::None, NdcRotation::Ccw90, NdcRotation::Cw90]
        .into_iter()
        .min_by(|left, right| {
            let left_d = left.apply(projected).distance_squared(expected);
            let right_d = right.apply(projected).distance_squared(expected);
            left_d.total_cmp(&right_d)
        })
        .unwrap_or_default()
}

/// Look-at camera. `fov_y_radians` is the full vertical field of view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraParams {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y_radians: f32,
    pub near: f32,
    pub far: f32,
}

impl CameraParams {
    pub fn from_degrees(eye: Vec3, target: Vec3, fov_y_degrees: f32) -> Self {
        Self::from_radians(eye, target, fov_y_degrees.to_radians())
    }

    pub fn from_radians(eye: Vec3, target: Vec3, fov_y_radians: f32) -> Self {
        Self {
            eye,
            target,
            up: Vec3::Y,
            fov_y_radians,
            near: 1.0,
            far: 1.0e6,
        }
    }

    pub fn is_usable(self) -> bool {
        let forward = self.target - self.eye;
        forward.length_squared() > 1.0e-8
            && self.up.length_squared() > 1.0e-8
            && forward.cross(self.up).length_squared() > 1.0e-8
            && self.fov_y_radians.is_finite()
            && self.fov_y_radians > 0.0
            && self.near.is_finite()
            && self.far.is_finite()
            && self.near > 0.0
            && self.far > self.near
    }

    pub fn view(self) -> Mat4 {
        Mat4::look_at_rh(self.eye, self.target, self.up)
    }

    pub fn projection(self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y_radians, TOP_ASPECT, self.near, self.far)
    }
}

/// Either reconstructed parameters or a view/projection pair already in memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Camera {
    Params(CameraParams),
    Matrices {
        view: Mat4,
        projection: Mat4,
        rotation: NdcRotation,
    },
}

impl From<CameraParams> for Camera {
    fn from(params: CameraParams) -> Self {
        Self::Params(params)
    }
}

/// Rectangle of the top screen in host pixels. Origin is the top-left, Y grows down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ScreenRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }
}

/// Where a world point lands on the host window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projected {
    Visible { x: f32, y: f32 },
    OffScreen { x: f32, y: f32 },
    Behind,
}

impl Projected {
    pub fn visible_pos(self) -> Option<Vec2> {
        match self {
            Self::Visible { x, y } => Some(Vec2::new(x, y)),
            Self::OffScreen { .. } | Self::Behind => None,
        }
    }
}

/// Project `world` onto `screen`. Points with clip `w ≤ ε` are behind the camera.
pub fn project(world: Vec3, camera: &Camera, screen: ScreenRect, edges: EdgeMode) -> Projected {
    let Some(ndc) = project_ndc(world, camera) else {
        return Projected::Behind;
    };
    if screen.width <= 0.0 || screen.height <= 0.0 {
        return Projected::Behind;
    }

    let inside = ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0;
    let ndc = if edges == EdgeMode::Clamp {
        Vec2::new(ndc.x.clamp(-1.0, 1.0), ndc.y.clamp(-1.0, 1.0))
    } else {
        ndc
    };
    let x = screen.x + (ndc.x + 1.0) * 0.5 * screen.width;
    let y = screen.y + (1.0 - ndc.y) * 0.5 * screen.height;
    if inside || edges == EdgeMode::Clamp {
        Projected::Visible { x, y }
    } else {
        Projected::OffScreen { x, y }
    }
}

/// NDC in `[-1, 1]²`, after any matrix rotation fix. `None` when the point is behind the camera.
pub fn project_ndc(world: Vec3, camera: &Camera) -> Option<Vec2> {
    let (view, projection, rotation) = match *camera {
        Camera::Params(params) => {
            if !params.is_usable() {
                return None;
            }
            (params.view(), params.projection(), NdcRotation::None)
        }
        Camera::Matrices {
            view,
            projection,
            rotation,
        } => (view, projection, rotation),
    };
    let clip = projection * view * Vec4::new(world.x, world.y, world.z, 1.0);
    if !clip.w.is_finite() || clip.w <= CLIP_EPSILON {
        return None;
    }
    let ndc = Vec2::new(clip.x / clip.w, clip.y / clip.w);
    if !ndc.is_finite() {
        return None;
    }
    Some(rotation.apply(ndc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn center_cam(fov_deg: f32) -> Camera {
        Camera::Params(CameraParams::from_degrees(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -1.0),
            fov_deg,
        ))
    }

    #[test]
    fn vertical_fov_matches_the_live_projection_scale() {
        let camera = CameraParams::from_degrees(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), 50.0);
        let y_scale = camera.projection().y_axis.y;
        let expected = 1.0 / (25.0_f32.to_radians().tan());
        assert!(
            (y_scale - expected).abs() < 1.0e-5,
            "{y_scale} vs {expected}"
        );
        let x_scale = camera.projection().x_axis.x;
        assert!((x_scale - expected / TOP_ASPECT).abs() < 1.0e-5);
    }

    #[test]
    fn optical_axis_is_the_screen_center() {
        let camera =
            CameraParams::from_degrees(Vec3::new(0.0, 10.0, 20.0), Vec3::new(0.0, 10.0, 0.0), 50.0);
        let ndc = project_ndc(camera.target, &Camera::Params(camera)).unwrap();
        assert!(ndc.length() < 1.0e-4, "{ndc}");

        let screen = ScreenRect::new(100.0, 50.0, 400.0, 240.0);
        let Projected::Visible { x, y } =
            project(camera.target, &camera.into(), screen, EdgeMode::Hide)
        else {
            panic!("target should be visible");
        };
        assert!((x - 300.0).abs() < 1.0e-3);
        assert!((y - 170.0).abs() < 1.0e-3);
    }

    #[test]
    fn points_behind_the_camera_are_dropped() {
        let camera = center_cam(50.0);
        assert!(project_ndc(Vec3::new(0.0, 0.0, 5.0), &camera).is_none());
        let screen = ScreenRect::new(0.0, 0.0, 400.0, 240.0);
        assert_eq!(
            project(Vec3::new(0.0, 0.0, 5.0), &camera, screen, EdgeMode::Hide),
            Projected::Behind
        );
    }

    #[test]
    fn ninety_degree_fov_hits_the_ndc_edge() {
        let camera = center_cam(90.0);
        let ndc = project_ndc(Vec3::new(0.0, 10.0, -10.0), &camera).unwrap();
        assert!(ndc.x.abs() < 1.0e-4, "{ndc}");
        assert!((ndc.y - 1.0).abs() < 1.0e-4, "{ndc}");

        let edge_x = 10.0 * TOP_ASPECT;
        let ndc = project_ndc(Vec3::new(edge_x, 0.0, -10.0), &camera).unwrap();
        assert!((ndc.x - 1.0).abs() < 1.0e-4, "{ndc}");
        assert!(ndc.y.abs() < 1.0e-4, "{ndc}");
    }

    #[test]
    fn offscreen_points_hide_or_clamp() {
        let camera = center_cam(50.0);
        // Far above the 50° frustum at z = -10.
        let world = Vec3::new(0.0, 100.0, -10.0);
        let screen = ScreenRect::new(0.0, 0.0, 400.0, 240.0);
        assert!(matches!(
            project(world, &camera, screen, EdgeMode::Hide),
            Projected::OffScreen { .. }
        ));
        let Projected::Visible { x, y } = project(world, &camera, screen, EdgeMode::Clamp) else {
            panic!("clamp should keep the point");
        };
        assert!((x - 200.0).abs() < 1.0e-2);
        assert!((y - 0.0).abs() < 1.0e-2);
    }

    #[test]
    fn matrix_path_matches_parameters() {
        let params =
            CameraParams::from_degrees(Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 2.0, 0.0), 50.0);
        let world = Vec3::new(4.0, 2.0, 1.0);
        let from_params = project_ndc(world, &Camera::Params(params)).unwrap();
        let from_matrix = project_ndc(
            world,
            &Camera::Matrices {
                view: params.view(),
                projection: params.projection(),
                rotation: NdcRotation::None,
            },
        )
        .unwrap();
        assert!((from_params - from_matrix).length() < 1.0e-5);
    }

    #[test]
    fn rotation_fix_and_guess() {
        let point = Vec2::new(0.5, 0.0);
        assert_eq!(NdcRotation::Cw90.apply(point), Vec2::new(0.0, -0.5));
        assert_eq!(NdcRotation::Ccw90.apply(point), Vec2::new(0.0, 0.5));
        assert_eq!(
            guess_ndc_rotation(point, Vec2::new(0.0, -0.5)),
            NdcRotation::Cw90
        );
        assert_eq!(guess_ndc_rotation(point, point), NdcRotation::None);
    }

    #[test]
    fn degenerate_camera_does_not_project() {
        let camera = Camera::Params(CameraParams::from_degrees(Vec3::ZERO, Vec3::ZERO, 50.0));
        assert!(project_ndc(Vec3::new(0.0, 0.0, -1.0), &camera).is_none());
    }
}
