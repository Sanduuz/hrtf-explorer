use std::cmp::Ordering;

use glam::Vec3;

use crate::{HrtfDataset, HrtfError, angular_distance, coordinates::normalized};

const NEIGHBOR_COUNT: usize = 3;
const EXACT_MATCH_RADIANS: f32 = 1.0e-5;

/// A selected measurement and its normalized contribution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterpolationContributor {
    pub measurement_index: usize,
    pub angular_distance_radians: f32,
    pub weight: f32,
}

/// Interpolated left/right HRIR and diagnostic spatial weights.
#[derive(Debug, Clone, PartialEq)]
pub struct InterpolatedHrir {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub contributors: Vec<InterpolationContributor>,
}

pub trait HrirInterpolator {
    /// Interpolates a stereo HRIR for `direction`.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid direction or a dataset unsuitable for the method.
    fn interpolate(
        &self,
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<InterpolatedHrir, HrtfError>;
}

/// Selects the single closest measured direction without blending HRIR samples.
#[derive(Debug, Default, Clone, Copy)]
pub struct NearestNeighborInterpolator;

impl NearestNeighborInterpolator {
    /// Returns the closest measurement with weight one.
    ///
    /// # Errors
    ///
    /// Returns an error if the direction is invalid.
    pub fn contributors(
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<Vec<InterpolationContributor>, HrtfError> {
        let nearest = sorted_distances(dataset, direction)?
            .into_iter()
            .next()
            .ok_or(HrtfError::EmptyDataset)?;
        Ok(vec![InterpolationContributor {
            measurement_index: nearest.0,
            angular_distance_radians: nearest.1,
            weight: 1.0,
        }])
    }
}

impl HrirInterpolator for NearestNeighborInterpolator {
    fn interpolate(
        &self,
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<InterpolatedHrir, HrtfError> {
        Ok(interpolate_contributors(
            dataset,
            Self::contributors(dataset, direction)?,
        ))
    }
}

/// Three nearest unit vectors, weighted by inverse angular distance.
#[derive(Debug, Default, Clone, Copy)]
pub struct NearestThreeInterpolator;

impl NearestThreeInterpolator {
    /// Performs only spatial selection and weighting, useful to render debug markers.
    ///
    /// # Errors
    ///
    /// Returns an error if the direction is invalid or the dataset has fewer than three
    /// measurements.
    pub fn contributors(
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<Vec<InterpolationContributor>, HrtfError> {
        if dataset.measurements().len() < NEIGHBOR_COUNT {
            return Err(HrtfError::NotEnoughMeasurements {
                available: dataset.measurements().len(),
                required: NEIGHBOR_COUNT,
            });
        }
        let distances = sorted_distances(dataset, direction)?;

        if distances[0].1 <= EXACT_MATCH_RADIANS {
            return Ok(vec![InterpolationContributor {
                measurement_index: distances[0].0,
                angular_distance_radians: distances[0].1,
                weight: 1.0,
            }]);
        }

        let inverse_sum: f32 = distances[..NEIGHBOR_COUNT]
            .iter()
            .map(|(_, distance)| distance.recip())
            .sum();
        Ok(distances[..NEIGHBOR_COUNT]
            .iter()
            .map(|&(index, distance)| InterpolationContributor {
                measurement_index: index,
                angular_distance_radians: distance,
                weight: distance.recip() / inverse_sum,
            })
            .collect())
    }
}

impl HrirInterpolator for NearestThreeInterpolator {
    fn interpolate(
        &self,
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<InterpolatedHrir, HrtfError> {
        Ok(interpolate_contributors(
            dataset,
            Self::contributors(dataset, direction)?,
        ))
    }
}

fn sorted_distances(
    dataset: &HrtfDataset,
    direction: Vec3,
) -> Result<Vec<(usize, f32)>, HrtfError> {
    let direction = normalized(direction)?;
    let mut distances = dataset
        .measurements()
        .iter()
        .enumerate()
        .map(|(index, measurement)| {
            angular_distance(direction, measurement.direction).map(|distance| (index, distance))
        })
        .collect::<Result<Vec<_>, _>>()?;
    distances.sort_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal));
    Ok(distances)
}

fn interpolate_contributors(
    dataset: &HrtfDataset,
    contributors: Vec<InterpolationContributor>,
) -> InterpolatedHrir {
    let mut left = vec![0.0; dataset.hrir_length()];
    let mut right = vec![0.0; dataset.hrir_length()];

    for contributor in &contributors {
        let measurement = &dataset.measurements()[contributor.measurement_index];
        for (output, sample) in left.iter_mut().zip(&measurement.left) {
            *output += contributor.weight * sample;
        }
        for (output, sample) in right.iter_mut().zip(&measurement.right) {
            *output += contributor.weight * sample;
        }
    }

    InterpolatedHrir {
        left,
        right,
        contributors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HrirMeasurement, spherical_to_direction};

    fn synthetic_dataset() -> HrtfDataset {
        let specs = [
            (0.0, vec![1.0, 2.0], vec![10.0, 20.0]),
            (90.0, vec![3.0, 4.0], vec![30.0, 40.0]),
            (-90.0, vec![5.0, 6.0], vec![50.0, 60.0]),
            (180.0, vec![7.0, 8.0], vec![70.0, 80.0]),
        ];
        HrtfDataset::new(
            48_000,
            specs
                .into_iter()
                .map(|(azimuth, left, right)| {
                    HrirMeasurement::new(spherical_to_direction(azimuth, 0.0), left, right).unwrap()
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn exact_position_returns_exact_measurement() {
        let dataset = synthetic_dataset();
        let result = NearestThreeInterpolator
            .interpolate(&dataset, Vec3::X)
            .unwrap();
        assert_eq!(result.left, vec![3.0, 4.0]);
        assert_eq!(result.right, vec![30.0, 40.0]);
        assert_eq!(result.contributors.len(), 1);
        assert_eq!(result.contributors[0].measurement_index, 1);
        assert!((result.contributors[0].weight - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn nearest_neighbor_returns_one_unmodified_measurement() {
        let dataset = synthetic_dataset();
        let result = NearestNeighborInterpolator
            .interpolate(&dataset, spherical_to_direction(80.0, 5.0))
            .unwrap();

        assert_eq!(result.left, vec![3.0, 4.0]);
        assert_eq!(result.right, vec![30.0, 40.0]);
        assert_eq!(result.contributors.len(), 1);
        assert_eq!(result.contributors[0].measurement_index, 1);
        assert!((result.contributors[0].weight - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn nearest_three_are_selected_across_azimuth_wrap() {
        let dataset = synthetic_dataset();
        let contributors =
            NearestThreeInterpolator::contributors(&dataset, spherical_to_direction(-179.0, 0.0))
                .unwrap();
        assert_eq!(contributors[0].measurement_index, 3);
        assert!(
            !contributors
                .iter()
                .any(|entry| entry.measurement_index == 0)
        );
    }

    #[test]
    fn weights_are_finite_non_negative_and_normalized() {
        let dataset = synthetic_dataset();
        let contributors =
            NearestThreeInterpolator::contributors(&dataset, spherical_to_direction(40.0, 20.0))
                .unwrap();
        assert_eq!(contributors.len(), 3);
        assert!(
            contributors
                .iter()
                .all(|entry| entry.weight.is_finite() && entry.weight > 0.0)
        );
        assert!((contributors.iter().map(|entry| entry.weight).sum::<f32>() - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn sample_wise_interpolation_matches_weighted_sum() {
        let dataset = synthetic_dataset();
        let result = NearestThreeInterpolator
            .interpolate(&dataset, spherical_to_direction(45.0, 0.0))
            .unwrap();
        let expected_left_first: f32 = result
            .contributors
            .iter()
            .map(|entry| dataset.measurements()[entry.measurement_index].left[0] * entry.weight)
            .sum();
        assert!((result.left[0] - expected_left_first).abs() < 1.0e-6);
    }

    #[test]
    fn fewer_than_three_measurements_is_rejected() {
        let measurements = [Vec3::Z, Vec3::X]
            .map(|direction| HrirMeasurement::new(direction, vec![1.0], vec![1.0]).unwrap())
            .to_vec();
        let dataset = HrtfDataset::new(48_000, measurements).unwrap();
        assert_eq!(
            NearestThreeInterpolator::contributors(&dataset, Vec3::Z),
            Err(HrtfError::NotEnoughMeasurements {
                available: 2,
                required: 3
            })
        );
    }
}
