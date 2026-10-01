//! Interpolate the last few camera samples so a 30 FPS guest does not shimmer
//! on a faster host. Render at `now - display_latency` (one guest frame by default).

use std::collections::VecDeque;
use std::time::Duration;

use glam::{Mat4, Quat, Vec3, Vec4};

use crate::camera::{Camera, CameraParams};

/// One guest frame at 30 FPS. RAM is usually one or two frames ahead of the image.
pub const DEFAULT_DISPLAY_LATENCY: Duration = Duration::from_millis(33);

const CAPACITY: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFrame {
    pub host_us: u64,
    pub camera: Camera,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraBuffer {
    frames: VecDeque<CameraFrame>,
}

impl Default for CameraBuffer {
    fn default() -> Self {
        Self {
            frames: VecDeque::with_capacity(CAPACITY),
        }
    }
}

impl CameraBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn push(&mut self, frame: CameraFrame) {
        if self
            .frames
            .back()
            .is_some_and(|previous| frame.host_us < previous.host_us)
        {
            self.frames.clear();
        }
        if self
            .frames
            .back()
            .is_some_and(|previous| previous.host_us == frame.host_us)
        {
            self.frames.pop_back();
        }
        self.frames.push_back(frame);
        while self.frames.len() > CAPACITY {
            self.frames.pop_front();
        }
    }

    /// Camera at `host_us`, clamped to the stored window and lerped inside it.
    pub fn sample(&self, host_us: u64) -> Option<Camera> {
        let first = self.frames.front()?;
        if self.frames.len() == 1 || host_us <= first.host_us {
            return Some(first.camera);
        }
        let last = self.frames.back()?;
        if host_us >= last.host_us {
            return Some(last.camera);
        }

        let mut previous = first;
        for frame in self.frames.iter().skip(1) {
            if host_us <= frame.host_us {
                let span = frame.host_us - previous.host_us;
                let t = (host_us - previous.host_us) as f32 / span as f32;
                return Some(interpolate(previous.camera, frame.camera, t));
            }
            previous = frame;
        }
        Some(last.camera)
    }

    pub fn sample_for_render(&self, now_us: u64, latency: Duration) -> Option<Camera> {
        self.sample(render_timestamp(now_us, latency))
    }
}

pub fn render_timestamp(now_us: u64, latency: Duration) -> u64 {
    let latency_us = u64::try_from(latency.as_micros()).unwrap_or(u64::MAX);
    now_us.saturating_sub(latency_us)
}

fn interpolate(from: Camera, to: Camera, t: f32) -> Camera {
    let t = t.clamp(0.0, 1.0);
    match (from, to) {
        (Camera::Params(from), Camera::Params(to)) => Camera::Params(lerp_params(from, to, t)),
        (
            Camera::Matrices {
                view: view_from,
                projection: projection_from,
                rotation: rotation_from,
            },
            Camera::Matrices {
                view: view_to,
                projection: projection_to,
                rotation: rotation_to,
            },
        ) => Camera::Matrices {
            view: lerp_view(view_from, view_to, t),
            projection: lerp_columns(projection_from, projection_to, t),
            rotation: if t < 0.5 { rotation_from } else { rotation_to },
        },
        _ => {
            if t < 0.5 {
                from
            } else {
                to
            }
        }
    }
}

fn lerp_params(from: CameraParams, to: CameraParams, t: f32) -> CameraParams {
    let up = from.up.lerp(to.up, t);
    let up = if up.length_squared() > 1.0e-8 {
        up.normalize()
    } else {
        to.up
    };
    CameraParams {
        eye: from.eye.lerp(to.eye, t),
        target: from.target.lerp(to.target, t),
        up,
        fov_y_radians: from.fov_y_radians + (to.fov_y_radians - from.fov_y_radians) * t,
        near: from.near + (to.near - from.near) * t,
        far: from.far + (to.far - from.far) * t,
    }
}

fn lerp_view(from: Mat4, to: Mat4, t: f32) -> Mat4 {
    let Some((rotation_from, eye_from)) = camera_pose(from) else {
        return if t < 0.5 { from } else { to };
    };
    let Some((rotation_to, eye_to)) = camera_pose(to) else {
        return if t < 0.5 { from } else { to };
    };
    view_from_pose(
        rotation_from.slerp(rotation_to, t),
        eye_from.lerp(eye_to, t),
    )
}

/// View matrices are `R * translate(-eye)`, not a world-space rigid transform.
fn camera_pose(view: Mat4) -> Option<(Quat, Vec3)> {
    if !view.is_finite() || view.determinant().abs() < 1.0e-6 {
        return None;
    }
    let mut linear = view;
    linear.w_axis = Vec4::W;
    let rotation = Quat::from_mat4(&linear);
    let eye = view.inverse().transform_point3(Vec3::ZERO);
    if rotation.is_finite() && eye.is_finite() {
        Some((rotation, eye))
    } else {
        None
    }
}

fn view_from_pose(rotation: Quat, eye: Vec3) -> Mat4 {
    Mat4::from_quat(rotation) * Mat4::from_translation(-eye)
}

fn lerp_columns(from: Mat4, to: Mat4, t: f32) -> Mat4 {
    Mat4::from_cols(
        from.x_axis.lerp(to.x_axis, t),
        from.y_axis.lerp(to.y_axis, t),
        from.z_axis.lerp(to.z_axis, t),
        from.w_axis.lerp(to.w_axis, t),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{project, EdgeMode, Projected};
    use crate::layout::{resolve, LayoutInput};
    use glam::Vec3;

    fn still() -> Camera {
        Camera::Params(CameraParams::from_degrees(
            Vec3::new(0.0, 170.0, 500.0),
            Vec3::new(0.0, 170.0, 0.0),
            50.0,
        ))
    }

    fn push_still(buffer: &mut CameraBuffer, host_us: u64) {
        buffer.push(CameraFrame {
            host_us,
            camera: still(),
        });
    }

    #[test]
    fn a_still_camera_does_not_shimmer() {
        let mut buffer = CameraBuffer::new();
        push_still(&mut buffer, 0);
        push_still(&mut buffer, 33_000);
        push_still(&mut buffer, 66_000);

        let screen = resolve(&LayoutInput::large_bottom_right(1920, 1080))
            .top
            .to_screen();
        let point = Vec3::new(120.0, 200.0, 40.0);
        let mut positions = Vec::new();
        for now in [50_000_u64, 66_000, 82_000] {
            let camera = buffer
                .sample_for_render(now, DEFAULT_DISPLAY_LATENCY)
                .unwrap();
            let Projected::Visible { x, y } = project(point, &camera, screen, EdgeMode::Hide)
            else {
                panic!("point should stay on screen");
            };
            positions.push((x, y));
        }
        for (x, y) in &positions[1..] {
            assert!((x - positions[0].0).abs() < 1.0, "{positions:?}");
            assert!((y - positions[0].1).abs() < 1.0, "{positions:?}");
        }
    }

    #[test]
    fn eye_and_target_are_lerped() {
        let mut buffer = CameraBuffer::new();
        buffer.push(CameraFrame {
            host_us: 0,
            camera: Camera::Params(CameraParams::from_degrees(
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::new(0.0, 0.0, 0.0),
                50.0,
            )),
        });
        buffer.push(CameraFrame {
            host_us: 100_000,
            camera: Camera::Params(CameraParams::from_degrees(
                Vec3::new(0.0, 0.0, 20.0),
                Vec3::new(0.0, 0.0, 10.0),
                60.0,
            )),
        });
        let Camera::Params(mid) = buffer.sample(50_000).unwrap() else {
            panic!("params stay params");
        };
        assert!((mid.eye.z - 15.0).abs() < 1.0e-4);
        assert!((mid.target.z - 5.0).abs() < 1.0e-4);
        assert!((mid.fov_y_radians - 55.0_f32.to_radians()).abs() < 1.0e-4);
    }

    #[test]
    fn view_matrix_slerp_matches_the_midpoint_pose() {
        let from = CameraParams::from_degrees(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 50.0);
        let to = CameraParams::from_degrees(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO, 50.0);
        let expected = CameraParams::from_degrees(Vec3::new(5.0, 0.0, 5.0), Vec3::ZERO, 50.0);
        let mut buffer = CameraBuffer::new();
        buffer.push(CameraFrame {
            host_us: 0,
            camera: Camera::Matrices {
                view: from.view(),
                projection: from.projection(),
                rotation: Default::default(),
            },
        });
        buffer.push(CameraFrame {
            host_us: 100_000,
            camera: Camera::Matrices {
                view: to.view(),
                projection: to.projection(),
                rotation: Default::default(),
            },
        });
        let Camera::Matrices { view, .. } = buffer.sample(50_000).unwrap() else {
            panic!("matrices stay matrices");
        };
        let delta = view - expected.view();
        let error = delta.x_axis.length()
            + delta.y_axis.length()
            + delta.z_axis.length()
            + delta.w_axis.length();
        assert!(error < 1.0e-3, "{error}");
    }

    #[test]
    fn a_backwards_timestamp_resets_the_buffer() {
        let mut buffer = CameraBuffer::new();
        push_still(&mut buffer, 10_000);
        push_still(&mut buffer, 20_000);
        push_still(&mut buffer, 5_000);
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.sample(0).unwrap(), still());
    }

    #[test]
    fn render_time_defaults_to_one_guest_frame() {
        assert_eq!(
            render_timestamp(100_000, DEFAULT_DISPLAY_LATENCY),
            100_000 - 33_000
        );
        assert_eq!(render_timestamp(10, Duration::from_millis(33)), 0);
    }

    #[test]
    fn only_three_samples_are_kept() {
        let mut buffer = CameraBuffer::new();
        for index in 0..5 {
            buffer.push(CameraFrame {
                host_us: index * 10_000,
                camera: Camera::Params(CameraParams::from_degrees(
                    Vec3::new(0.0, 0.0, index as f32),
                    Vec3::new(0.0, 0.0, -1.0),
                    50.0,
                )),
            });
        }
        assert_eq!(buffer.len(), 3);
        let Camera::Params(camera) = buffer.sample(0).unwrap() else {
            panic!("params");
        };
        // Samples 0 and 1 were dropped, so a time before the window clamps to eye.z = 2.
        assert!((camera.eye.z - 2.0).abs() < 1.0e-4);
    }
}
