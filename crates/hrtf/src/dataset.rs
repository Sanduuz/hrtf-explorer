use std::sync::OnceLock;

use glam::Vec3;

use crate::{
    HrtfError, coordinates::direction_to_spherical, coordinates::normalized,
    interpolation::SphericalHarmonicModel,
};

/// One measured pair of left/right head-related impulse responses.
#[derive(Debug, Clone, PartialEq)]
pub struct HrirMeasurement {
    pub direction: Vec3,
    pub azimuth_degrees: f32,
    pub elevation_degrees: f32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl HrirMeasurement {
    /// Constructs a measurement and normalizes its direction once on ingestion.
    ///
    /// # Errors
    ///
    /// Returns an error if the direction is zero/non-finite or any sample is non-finite.
    pub fn new(direction: Vec3, left: Vec<f32>, right: Vec<f32>) -> Result<Self, HrtfError> {
        let direction = normalized(direction)?;
        let spherical = direction_to_spherical(direction)?;
        if left.iter().chain(&right).any(|sample| !sample.is_finite()) {
            return Err(HrtfError::NonFiniteSample);
        }

        Ok(Self {
            direction,
            azimuth_degrees: spherical.azimuth_degrees,
            elevation_degrees: spherical.elevation_degrees,
            left,
            right,
        })
    }
}

/// Runtime HRTF data independent of its original on-disk format.
#[derive(Debug)]
pub struct HrtfDataset {
    sample_rate: u32,
    hrir_length: usize,
    measurements: Vec<HrirMeasurement>,
    spherical_harmonics: OnceLock<Result<SphericalHarmonicModel, HrtfError>>,
}

impl HrtfDataset {
    /// Validates a complete runtime dataset.
    ///
    /// # Errors
    ///
    /// Returns an error for a zero sample rate, no measurements, empty HRIRs, or channel
    /// lengths that differ between measurements.
    pub fn new(sample_rate: u32, measurements: Vec<HrirMeasurement>) -> Result<Self, HrtfError> {
        if sample_rate == 0 {
            return Err(HrtfError::InvalidSampleRate);
        }
        let Some(first) = measurements.first() else {
            return Err(HrtfError::EmptyDataset);
        };
        let hrir_length = first.left.len();
        if hrir_length == 0
            || measurements.iter().any(|measurement| {
                measurement.left.len() != hrir_length || measurement.right.len() != hrir_length
            })
        {
            return Err(HrtfError::InvalidHrirLength);
        }

        Ok(Self {
            sample_rate,
            hrir_length,
            measurements,
            spherical_harmonics: OnceLock::new(),
        })
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn hrir_length(&self) -> usize {
        self.hrir_length
    }

    #[must_use]
    pub fn measurements(&self) -> &[HrirMeasurement] {
        &self.measurements
    }

    pub(crate) fn spherical_harmonics(&self) -> Result<&SphericalHarmonicModel, HrtfError> {
        self.spherical_harmonics
            .get_or_init(|| SphericalHarmonicModel::fit(self))
            .as_ref()
            .map_err(Clone::clone)
    }
}

impl Clone for HrtfDataset {
    fn clone(&self) -> Self {
        Self {
            sample_rate: self.sample_rate,
            hrir_length: self.hrir_length,
            measurements: self.measurements.clone(),
            spherical_harmonics: OnceLock::new(),
        }
    }
}

impl PartialEq for HrtfDataset {
    fn eq(&self, other: &Self) -> bool {
        self.sample_rate == other.sample_rate
            && self.hrir_length == other.hrir_length
            && self.measurements == other.measurements
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_rejects_inconsistent_channel_lengths() {
        let measurement = HrirMeasurement::new(Vec3::Z, vec![1.0], vec![1.0, 0.0]).unwrap();
        assert_eq!(
            HrtfDataset::new(48_000, vec![measurement]),
            Err(HrtfError::InvalidHrirLength)
        );
    }

    #[test]
    fn measurement_normalizes_direction_and_derives_angles() {
        let measurement = HrirMeasurement::new(Vec3::X * 5.0, vec![1.0], vec![2.0]).unwrap();
        assert_eq!(measurement.direction, Vec3::X);
        assert!((measurement.azimuth_degrees - 90.0).abs() < 1.0e-5);
        assert!(measurement.elevation_degrees.abs() < 1.0e-5);
    }
}
