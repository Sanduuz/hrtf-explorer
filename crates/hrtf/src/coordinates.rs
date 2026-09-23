use glam::Vec3;

use crate::HrtfError;

/// Azimuth and elevation in degrees in the project's canonical convention.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SphericalDirection {
    pub azimuth_degrees: f32,
    pub elevation_degrees: f32,
}

/// Converts canonical azimuth/elevation angles to a unit direction vector.
#[must_use]
pub fn spherical_to_direction(azimuth_degrees: f32, elevation_degrees: f32) -> Vec3 {
    let theta = azimuth_degrees.to_radians();
    let phi = elevation_degrees.to_radians();
    let cos_phi = phi.cos();

    Vec3::new(cos_phi * theta.sin(), phi.sin(), cos_phi * theta.cos()).normalize()
}

/// Converts a non-zero direction vector to canonical azimuth/elevation.
///
/// Azimuth is returned in the range `[-180, 180]`; elevation is in
/// `[-90, 90]`.
///
/// # Errors
///
/// Returns [`HrtfError::InvalidDirection`] for a zero or non-finite vector.
pub fn direction_to_spherical(direction: Vec3) -> Result<SphericalDirection, HrtfError> {
    let direction = normalized(direction)?;
    Ok(SphericalDirection {
        azimuth_degrees: direction.x.atan2(direction.z).to_degrees(),
        elevation_degrees: direction.y.clamp(-1.0, 1.0).asin().to_degrees(),
    })
}

/// Returns the shortest angular distance in radians between two directions.
///
/// # Errors
///
/// Returns [`HrtfError::InvalidDirection`] if either vector is zero or non-finite.
pub fn angular_distance(first: Vec3, second: Vec3) -> Result<f32, HrtfError> {
    let first = normalized(first)?;
    let second = normalized(second)?;
    Ok(first.dot(second).clamp(-1.0, 1.0).acos())
}

pub(crate) fn normalized(direction: Vec3) -> Result<Vec3, HrtfError> {
    if !direction.is_finite() || direction.length_squared() <= f32::EPSILON {
        return Err(HrtfError::InvalidDirection);
    }
    Ok(direction.normalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1.0e-5;

    fn assert_vec3_close(actual: Vec3, expected: Vec3) {
        assert!(
            actual.abs_diff_eq(expected, EPSILON),
            "actual={actual:?}, expected={expected:?}"
        );
    }

    #[test]
    fn cardinal_angles_map_to_canonical_axes() {
        assert_vec3_close(spherical_to_direction(0.0, 0.0), Vec3::Z);
        assert_vec3_close(spherical_to_direction(90.0, 0.0), Vec3::X);
        assert_vec3_close(spherical_to_direction(-90.0, 0.0), Vec3::NEG_X);
        assert_vec3_close(spherical_to_direction(0.0, 90.0), Vec3::Y);
        assert_vec3_close(spherical_to_direction(0.0, -90.0), Vec3::NEG_Y);
        assert_vec3_close(spherical_to_direction(180.0, 0.0), Vec3::NEG_Z);
    }

    #[test]
    fn spherical_round_trip_handles_azimuth_wrap() {
        for (azimuth, elevation) in [
            (0.0, 0.0),
            (37.2, 14.7),
            (-179.5, -40.0),
            (179.5, 60.0),
            (-90.0, 0.0),
        ] {
            let result = direction_to_spherical(spherical_to_direction(azimuth, elevation))
                .expect("generated direction is valid");
            assert!((result.azimuth_degrees - azimuth).abs() < 1.0e-3);
            assert!((result.elevation_degrees - elevation).abs() < 1.0e-3);
        }
    }

    #[test]
    fn angular_distance_has_known_values() {
        assert!(angular_distance(Vec3::Z, Vec3::Z).unwrap() < EPSILON);
        assert!(
            (angular_distance(Vec3::Z, Vec3::X).unwrap() - std::f32::consts::FRAC_PI_2).abs()
                < EPSILON
        );
        assert!(
            (angular_distance(Vec3::Z, Vec3::NEG_Z).unwrap() - std::f32::consts::PI).abs()
                < EPSILON
        );
    }

    #[test]
    fn invalid_directions_are_rejected() {
        assert_eq!(
            direction_to_spherical(Vec3::ZERO),
            Err(HrtfError::InvalidDirection)
        );
        assert_eq!(
            angular_distance(Vec3::NAN, Vec3::X),
            Err(HrtfError::InvalidDirection)
        );
    }
}
