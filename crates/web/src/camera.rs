use glam::{Mat4, Vec3, Vec4};

pub const SOURCE_SPHERE_RADIUS: f32 = 1.5;

const MIN_DISTANCE: f32 = 2.4;
const MAX_DISTANCE: f32 = 8.0;
const MAX_PITCH_RADIANS: f32 = 1.45;
const DEFAULT_DISTANCE: f32 = 4.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraPreset {
    Front,
    Back,
    Left,
    Right,
    Top,
    Reset,
}

#[derive(Debug, Clone)]
pub struct OrbitCamera {
    yaw_radians: f32,
    pitch_radians: f32,
    distance: f32,
    aspect: f32,
}

impl OrbitCamera {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            yaw_radians: 0.0,
            pitch_radians: 0.0,
            distance: DEFAULT_DISTANCE,
            aspect: 1.0,
        }
    }

    pub fn set_aspect(&mut self, aspect: f32) {
        if aspect.is_finite() && aspect > 0.0 {
            self.aspect = aspect;
        }
    }

    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        self.yaw_radians -= delta_x * 0.008;
        self.pitch_radians =
            (self.pitch_radians + delta_y * 0.008).clamp(-MAX_PITCH_RADIANS, MAX_PITCH_RADIANS);
    }

    pub fn zoom(&mut self, wheel_delta: f32) {
        self.distance =
            (self.distance * (wheel_delta * 0.001).exp()).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    pub fn set_preset(&mut self, preset: CameraPreset) {
        let (yaw, pitch) = match preset {
            CameraPreset::Front | CameraPreset::Reset => (0.0, 0.0),
            CameraPreset::Back => (std::f32::consts::PI, 0.0),
            CameraPreset::Left => (-std::f32::consts::FRAC_PI_2, 0.0),
            CameraPreset::Right => (std::f32::consts::FRAC_PI_2, 0.0),
            CameraPreset::Top => (0.0, MAX_PITCH_RADIANS),
        };
        self.yaw_radians = yaw;
        self.pitch_radians = pitch;
        if preset == CameraPreset::Reset {
            self.distance = DEFAULT_DISTANCE;
        }
    }

    #[must_use]
    pub fn eye(&self) -> Vec3 {
        Vec3::new(
            self.distance * self.pitch_radians.cos() * self.yaw_radians.sin(),
            self.distance * self.pitch_radians.sin(),
            self.distance * self.pitch_radians.cos() * self.yaw_radians.cos(),
        )
    }

    #[must_use]
    pub fn view_projection(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye(), Vec3::ZERO, Vec3::Y);
        let projection = Mat4::perspective_rh(45_f32.to_radians(), self.aspect, 0.1, 100.0);
        projection * view
    }

    pub fn ray_from_canvas(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Result<Ray, CameraError> {
        if ![x, y, width, height].into_iter().all(f32::is_finite) || width <= 0.0 || height <= 0.0 {
            return Err(CameraError::InvalidViewport);
        }
        let ndc_x = x.mul_add(2.0 / width, -1.0);
        let ndc_y = 1.0 - y * 2.0 / height;
        let inverse = self.view_projection().inverse();
        let near = unproject(inverse * Vec4::new(ndc_x, ndc_y, 0.0, 1.0))?;
        let far = unproject(inverse * Vec4::new(ndc_x, ndc_y, 1.0, 1.0))?;
        let direction = (far - near)
            .try_normalize()
            .ok_or(CameraError::InvalidRay)?;
        Ok(Ray {
            origin: self.eye(),
            direction,
        })
    }
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self::new()
    }
}

fn unproject(point: Vec4) -> Result<Vec3, CameraError> {
    if !point.is_finite() || point.w.abs() <= f32::EPSILON {
        return Err(CameraError::InvalidRay);
    }
    Ok(point.truncate() / point.w)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn sphere_intersection_direction(
        self,
        center: Vec3,
        radius: f32,
    ) -> Result<Vec3, CameraError> {
        if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return Err(CameraError::InvalidSphere);
        }
        let offset = self.origin - center;
        let half_b = offset.dot(self.direction);
        let c = offset.length_squared() - radius * radius;
        let discriminant = half_b.mul_add(half_b, -c);
        if discriminant < 0.0 {
            return Err(CameraError::SphereMiss);
        }
        let root = discriminant.sqrt();
        let near = -half_b - root;
        let far = -half_b + root;
        let distance = if near > 0.0 {
            near
        } else if far > 0.0 {
            far
        } else {
            return Err(CameraError::SphereMiss);
        };
        ((self.origin + self.direction * distance) - center)
            .try_normalize()
            .ok_or(CameraError::InvalidRay)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraError {
    InvalidRay,
    InvalidSphere,
    InvalidViewport,
    SphereMiss,
}

impl std::fmt::Display for CameraError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRay => write!(formatter, "camera produced an invalid ray"),
            Self::InvalidSphere => write!(formatter, "source sphere is invalid"),
            Self::InvalidViewport => write!(formatter, "canvas dimensions are invalid"),
            Self::SphereMiss => write!(formatter, "pointer ray did not hit the source sphere"),
        }
    }
}

impl std::error::Error for CameraError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_pointer_hits_canonical_front() {
        let mut camera = OrbitCamera::new();
        camera.set_aspect(800.0 / 600.0);
        let ray = camera.ray_from_canvas(400.0, 300.0, 800.0, 600.0).unwrap();
        let direction = ray
            .sphere_intersection_direction(Vec3::ZERO, SOURCE_SPHERE_RADIUS)
            .unwrap();
        assert!(direction.abs_diff_eq(Vec3::Z, 1.0e-4));
    }

    #[test]
    fn right_side_pointer_hits_positive_x() {
        let camera = OrbitCamera::new();
        let ray = camera.ray_from_canvas(650.0, 300.0, 800.0, 600.0).unwrap();
        let direction = ray
            .sphere_intersection_direction(Vec3::ZERO, SOURCE_SPHERE_RADIUS)
            .unwrap();
        assert!(direction.x > 0.0);
        assert!(direction.z > 0.0);
    }

    #[test]
    fn ray_sphere_returns_nearest_forward_hit_and_rejects_miss() {
        let hit = Ray {
            origin: Vec3::new(0.0, 0.0, 4.0),
            direction: Vec3::NEG_Z,
        }
        .sphere_intersection_direction(Vec3::ZERO, 1.5)
        .unwrap();
        assert!(hit.abs_diff_eq(Vec3::Z, 1.0e-6));

        let miss = Ray {
            origin: Vec3::new(0.0, 0.0, 4.0),
            direction: Vec3::X,
        }
        .sphere_intersection_direction(Vec3::ZERO, 1.5);
        assert_eq!(miss, Err(CameraError::SphereMiss));
    }

    #[test]
    fn orbit_and_zoom_remain_bounded() {
        let mut camera = OrbitCamera::new();
        camera.orbit(0.0, 100_000.0);
        assert!(camera.eye().y < camera.distance);
        camera.zoom(-100_000.0);
        assert!((camera.eye().length() - MIN_DISTANCE).abs() < 1.0e-5);
        camera.zoom(100_000.0);
        assert!((camera.eye().length() - MAX_DISTANCE).abs() < 1.0e-5);
    }

    #[test]
    fn presets_use_canonical_views_and_reset_distance() {
        let mut camera = OrbitCamera::new();
        camera.set_preset(CameraPreset::Back);
        assert!(camera.eye().normalize().abs_diff_eq(Vec3::NEG_Z, 1.0e-5));
        camera.set_preset(CameraPreset::Left);
        assert!(camera.eye().normalize().abs_diff_eq(Vec3::NEG_X, 1.0e-5));
        camera.set_preset(CameraPreset::Right);
        assert!(camera.eye().normalize().abs_diff_eq(Vec3::X, 1.0e-5));
        camera.set_preset(CameraPreset::Top);
        assert!(camera.eye().y > 0.99 * camera.eye().length());

        camera.zoom(500.0);
        camera.set_preset(CameraPreset::Front);
        assert!(camera.eye().normalize().abs_diff_eq(Vec3::Z, 1.0e-5));
        assert!((camera.eye().length() - DEFAULT_DISTANCE).abs() > 1.0e-3);
        camera.set_preset(CameraPreset::Reset);
        assert!((camera.eye().length() - DEFAULT_DISTANCE).abs() < 1.0e-5);
    }
}
