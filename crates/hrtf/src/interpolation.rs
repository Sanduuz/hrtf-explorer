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

/// Three-neighbor interpolation that aligns each ear's peak arrival before blending.
///
/// Left and right delays are estimated and restored independently, retaining the interpolated
/// interaural time difference. Exact measurement hits bypass shifting and return the original
/// samples unchanged.
#[derive(Debug, Default, Clone, Copy)]
pub struct TimeAlignedNearestThreeInterpolator;

impl TimeAlignedNearestThreeInterpolator {
    /// Uses the same inverse-angular-distance spatial contributors as
    /// [`NearestThreeInterpolator`].
    ///
    /// # Errors
    ///
    /// Returns an error if the direction is invalid or the dataset has fewer than three
    /// measurements.
    pub fn contributors(
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<Vec<InterpolationContributor>, HrtfError> {
        NearestThreeInterpolator::contributors(dataset, direction)
    }
}

impl HrirInterpolator for TimeAlignedNearestThreeInterpolator {
    fn interpolate(
        &self,
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<InterpolatedHrir, HrtfError> {
        let contributors = Self::contributors(dataset, direction)?;
        if contributors.len() == 1 {
            return Ok(interpolate_contributors(dataset, contributors));
        }

        let left_responses = contributors
            .iter()
            .map(|contributor| {
                (
                    dataset.measurements()[contributor.measurement_index]
                        .left
                        .as_slice(),
                    contributor.weight,
                )
            })
            .collect::<Vec<_>>();
        let right_responses = contributors
            .iter()
            .map(|contributor| {
                (
                    dataset.measurements()[contributor.measurement_index]
                        .right
                        .as_slice(),
                    contributor.weight,
                )
            })
            .collect::<Vec<_>>();

        Ok(InterpolatedHrir {
            left: interpolate_time_aligned_channel(&left_responses, dataset.hrir_length()),
            right: interpolate_time_aligned_channel(&right_responses, dataset.hrir_length()),
            contributors,
        })
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

fn interpolate_time_aligned_channel(responses: &[(&[f32], f32)], output_length: usize) -> Vec<f32> {
    let delays = responses
        .iter()
        .map(|(response, _)| peak_delay(response))
        .collect::<Vec<_>>();
    let interpolated_delay = responses
        .iter()
        .zip(&delays)
        .map(|((_, weight), delay)| weight * delay)
        .sum::<f32>();
    let mut aligned = vec![0.0; output_length];

    for ((response, weight), delay) in responses.iter().zip(&delays) {
        let mut source_position = *delay;
        for output in &mut aligned {
            *output += weight * linear_sample(response, source_position);
            source_position += 1.0;
        }
    }

    let mut source_position = -interpolated_delay;
    (0..output_length)
        .map(|_| {
            let sample = linear_sample(&aligned, source_position);
            source_position += 1.0;
            sample
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)] // Runtime HRIR lengths are format-limited u32 values.
fn peak_delay(response: &[f32]) -> f32 {
    let (peak_index, peak) = response
        .iter()
        .enumerate()
        .map(|(index, sample)| (index, sample.abs()))
        .max_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal))
        .unwrap_or((0, 0.0));
    if peak == 0.0 || peak_index == 0 || peak_index + 1 >= response.len() {
        return peak_index as f32;
    }

    let before = response[peak_index - 1].abs();
    let after = response[peak_index + 1].abs();
    let denominator = before - 2.0 * peak + after;
    let fractional = if denominator.abs() > f32::EPSILON {
        (0.5 * (before - after) / denominator).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    peak_index as f32 + fractional
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)] // Bounds checks make the f32-to-index conversion non-negative and in range.
fn linear_sample(samples: &[f32], position: f32) -> f32 {
    if position < 0.0 || position > (samples.len() - 1) as f32 {
        return 0.0;
    }
    let lower = position.floor() as usize;
    let upper = (lower + 1).min(samples.len() - 1);
    let fraction = position - lower as f32;
    samples[lower] * (1.0 - fraction) + samples[upper] * fraction
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
    fn time_alignment_combines_delayed_impulses_without_three_separate_peaks() {
        fn impulse(index: usize) -> Vec<f32> {
            let mut response = vec![0.0; 8];
            response[index] = 1.0;
            response
        }

        let dataset = HrtfDataset::new(
            48_000,
            [(Vec3::X, 1, 2), (Vec3::Y, 3, 4), (Vec3::Z, 5, 6)]
                .into_iter()
                .map(|(direction, left, right)| {
                    HrirMeasurement::new(direction, impulse(left), impulse(right)).unwrap()
                })
                .collect(),
        )
        .unwrap();
        let direction = Vec3::ONE.normalize();

        let direct = NearestThreeInterpolator
            .interpolate(&dataset, direction)
            .unwrap();
        let aligned = TimeAlignedNearestThreeInterpolator
            .interpolate(&dataset, direction)
            .unwrap();

        assert_eq!(
            direct.left.iter().filter(|sample| **sample > 0.3).count(),
            3
        );
        assert!(aligned.left[3] > 0.99);
        assert!(aligned.right[4] > 0.99);
        assert_eq!(
            aligned.left.iter().filter(|sample| **sample > 0.01).count(),
            1
        );
        assert_eq!(
            aligned
                .right
                .iter()
                .filter(|sample| **sample > 0.01)
                .count(),
            1
        );
    }

    #[test]
    fn time_alignment_preserves_an_exact_measurement_bit_for_bit() {
        let dataset = synthetic_dataset();
        let aligned = TimeAlignedNearestThreeInterpolator
            .interpolate(&dataset, Vec3::X)
            .unwrap();

        assert_eq!(aligned.left, dataset.measurements()[1].left);
        assert_eq!(aligned.right, dataset.measurements()[1].right);
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
