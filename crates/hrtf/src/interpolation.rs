use std::cmp::Ordering;

use glam::Vec3;
use rustfft::{FftPlannerScalar, num_complex::Complex32};

use crate::{HrtfDataset, HrtfError, angular_distance, coordinates::normalized};

const NEIGHBOR_COUNT: usize = 3;
const EXACT_MATCH_RADIANS: f32 = 1.0e-5;
const TRIANGLE_NEAREST_CANDIDATES: usize = 16;
const TRIANGLE_AZIMUTH_BINS: usize = 16;
const TRIANGLE_EPSILON: f32 = 1.0e-6;
const SPHERICAL_HARMONIC_COEFFICIENTS: usize = 16;
const SPHERICAL_HARMONIC_RIDGE: f32 = 1.0e-4;

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
        Ok(interpolate_time_aligned(
            dataset,
            Self::contributors(dataset, direction)?,
        ))
    }
}

/// Selects a local spherical triangle containing the target and uses spherical-area weights.
#[derive(Debug, Default, Clone, Copy)]
pub struct SphericalTriangleInterpolator;

impl SphericalTriangleInterpolator {
    /// Returns the vertices and normalized spherical barycentric weights of a local containing
    /// triangle. Candidate directions include both the nearest measurements and azimuthally
    /// distributed neighbors, which keeps polar and sparsely measured regions covered.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid direction, fewer than three measurements, or if no
    /// non-degenerate containing triangle can be found.
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
        let direction = normalized(direction)?;
        let distances = sorted_distances(dataset, direction)?;
        if distances[0].1 <= EXACT_MATCH_RADIANS {
            return Ok(vec![InterpolationContributor {
                measurement_index: distances[0].0,
                angular_distance_radians: distances[0].1,
                weight: 1.0,
            }]);
        }

        let candidates = triangle_candidate_indices(dataset, direction, &distances);
        let mut best: Option<(f32, [(usize, f32); NEIGHBOR_COUNT])> = None;
        for first in 0..candidates.len().saturating_sub(2) {
            for second in first + 1..candidates.len().saturating_sub(1) {
                for third in second + 1..candidates.len() {
                    let indices = [candidates[first], candidates[second], candidates[third]];
                    let vertices = indices.map(|index| dataset.measurements()[index].direction);
                    let Some((weights, area)) = spherical_triangle_weights(direction, vertices)
                    else {
                        continue;
                    };
                    if best.as_ref().is_none_or(|(best_area, _)| area < *best_area) {
                        best = Some((
                            area,
                            std::array::from_fn(|index| (indices[index], weights[index])),
                        ));
                    }
                }
            }
        }

        let (_, triangle) = best.ok_or(HrtfError::NoContainingTriangle)?;
        triangle
            .into_iter()
            .map(|(measurement_index, weight)| {
                Ok(InterpolationContributor {
                    measurement_index,
                    angular_distance_radians: angular_distance(
                        direction,
                        dataset.measurements()[measurement_index].direction,
                    )?,
                    weight,
                })
            })
            .collect()
    }
}

impl HrirInterpolator for SphericalTriangleInterpolator {
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

/// Spherical-triangle interpolation with independent left/right arrival-time alignment.
///
/// Spatial contributors and weights are identical to [`SphericalTriangleInterpolator`]. Each
/// ear's responses are aligned before blending and its weighted delay is then restored, retaining
/// the interpolated interaural time difference while reducing temporal smearing.
#[derive(Debug, Default, Clone, Copy)]
pub struct TimeAlignedSphericalTriangleInterpolator;

impl TimeAlignedSphericalTriangleInterpolator {
    /// Uses the same containing triangle and spherical-area weights as
    /// [`SphericalTriangleInterpolator`].
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid direction or a dataset unsuitable for spherical-triangle
    /// interpolation.
    pub fn contributors(
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<Vec<InterpolationContributor>, HrtfError> {
        SphericalTriangleInterpolator::contributors(dataset, direction)
    }
}

impl HrirInterpolator for TimeAlignedSphericalTriangleInterpolator {
    fn interpolate(
        &self,
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<InterpolatedHrir, HrtfError> {
        Ok(interpolate_time_aligned(
            dataset,
            Self::contributors(dataset, direction)?,
        ))
    }
}

/// Interpolates minimum-phase magnitude responses and restores per-ear arrival delay.
///
/// The magnitude spectra of the three nearest HRIRs are blended logarithmically. A real-cepstrum
/// reconstruction produces one minimum-phase response per ear, after which the independently
/// weighted left/right peak delays are restored. Exact measurement hits remain unchanged.
#[derive(Debug, Default, Clone, Copy)]
pub struct MinimumPhaseInterpolator;

impl MinimumPhaseInterpolator {
    /// Uses the inverse-angular-distance contributors from [`NearestThreeInterpolator`].
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

impl HrirInterpolator for MinimumPhaseInterpolator {
    fn interpolate(
        &self,
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<InterpolatedHrir, HrtfError> {
        interpolate_minimum_phase(dataset, Self::contributors(dataset, direction)?)
    }
}

/// Interpolates HRIR magnitude and phase directly in the frequency domain.
///
/// Magnitudes are combined linearly and phases use a weighted circular mean, avoiding the
/// discontinuity at ±π. This intentionally retains measured phase rather than separating delay as
/// [`MinimumPhaseInterpolator`] does.
#[derive(Debug, Default, Clone, Copy)]
pub struct FrequencyDomainInterpolator;

impl FrequencyDomainInterpolator {
    /// Uses the inverse-angular-distance contributors from [`NearestThreeInterpolator`].
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

impl HrirInterpolator for FrequencyDomainInterpolator {
    fn interpolate(
        &self,
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<InterpolatedHrir, HrtfError> {
        Ok(interpolate_frequency_domain(
            dataset,
            Self::contributors(dataset, direction)?,
        ))
    }
}

/// Third-order real spherical-harmonic interpolation fitted over the complete dataset.
///
/// The global coefficient model is built lazily once per [`HrtfDataset`] and reused for every
/// direction. Evaluation therefore requires only 16 basis values per HRIR sample rather than a
/// new least-squares solve during pointer movement.
#[derive(Debug, Default, Clone, Copy)]
pub struct SphericalHarmonicInterpolator;

impl SphericalHarmonicInterpolator {
    /// Validates the direction and prepares the dataset's cached global model.
    ///
    /// A spherical-harmonic fit is global, so it does not expose a small set of local contributor
    /// markers or weights.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid direction, too few measurements, or a failed model fit.
    pub fn contributors(
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<Vec<InterpolationContributor>, HrtfError> {
        normalized(direction)?;
        dataset.spherical_harmonics()?;
        Ok(Vec::new())
    }
}

impl HrirInterpolator for SphericalHarmonicInterpolator {
    fn interpolate(
        &self,
        dataset: &HrtfDataset,
        direction: Vec3,
    ) -> Result<InterpolatedHrir, HrtfError> {
        let direction = normalized(direction)?;
        let (left, right) = dataset.spherical_harmonics()?.evaluate(direction);
        Ok(InterpolatedHrir {
            left,
            right,
            contributors: Vec::new(),
        })
    }
}

/// Cached coefficients for the fixed third-order real spherical-harmonic basis.
#[derive(Debug, Clone)]
pub(crate) struct SphericalHarmonicModel {
    hrir_length: usize,
    coefficients: Vec<f32>,
}

impl SphericalHarmonicModel {
    #[allow(clippy::needless_range_loop)] // Indexed loops directly express the small matrix solve.
    pub(crate) fn fit(dataset: &HrtfDataset) -> Result<Self, HrtfError> {
        if dataset.measurements().len() < SPHERICAL_HARMONIC_COEFFICIENTS {
            return Err(HrtfError::NotEnoughMeasurements {
                available: dataset.measurements().len(),
                required: SPHERICAL_HARMONIC_COEFFICIENTS,
            });
        }
        let output_width = dataset
            .hrir_length()
            .checked_mul(2)
            .ok_or(HrtfError::InvalidHrirLength)?;
        let mut gram = [[0.0; SPHERICAL_HARMONIC_COEFFICIENTS]; SPHERICAL_HARMONIC_COEFFICIENTS];
        let mut coefficients = vec![0.0; SPHERICAL_HARMONIC_COEFFICIENTS * output_width];

        for measurement in dataset.measurements() {
            let basis = real_spherical_harmonics(measurement.direction);
            for row in 0..SPHERICAL_HARMONIC_COEFFICIENTS {
                for column in 0..=row {
                    gram[row][column] += basis[row] * basis[column];
                }
                let coefficient_row = row * output_width;
                for (sample_index, sample) in measurement.left.iter().enumerate() {
                    coefficients[coefficient_row + sample_index] += basis[row] * sample;
                }
                for (sample_index, sample) in measurement.right.iter().enumerate() {
                    coefficients[coefficient_row + dataset.hrir_length() + sample_index] +=
                        basis[row] * sample;
                }
            }
        }
        for row in 0..SPHERICAL_HARMONIC_COEFFICIENTS {
            gram[row][row] += SPHERICAL_HARMONIC_RIDGE;
            for column in row + 1..SPHERICAL_HARMONIC_COEFFICIENTS {
                gram[row][column] = gram[column][row];
            }
        }

        let lower = cholesky(&gram)?;
        solve_cholesky(&lower, &mut coefficients, output_width);
        if coefficients
            .iter()
            .any(|coefficient| !coefficient.is_finite())
        {
            return Err(HrtfError::SphericalHarmonicFitFailed);
        }
        Ok(Self {
            hrir_length: dataset.hrir_length(),
            coefficients,
        })
    }

    fn evaluate(&self, direction: Vec3) -> (Vec<f32>, Vec<f32>) {
        let basis = real_spherical_harmonics(direction);
        let output_width = self.hrir_length * 2;
        let mut left = vec![0.0; self.hrir_length];
        let mut right = vec![0.0; self.hrir_length];
        for (basis_index, basis_value) in basis.into_iter().enumerate() {
            let row = basis_index * output_width;
            for sample in 0..self.hrir_length {
                left[sample] += basis_value * self.coefficients[row + sample];
                right[sample] += basis_value * self.coefficients[row + self.hrir_length + sample];
            }
        }
        (left, right)
    }
}

#[allow(clippy::many_single_char_names)] // Cartesian names make the basis definition auditable.
fn real_spherical_harmonics(direction: Vec3) -> [f32; SPHERICAL_HARMONIC_COEFFICIENTS] {
    let Vec3 { x, y, z } = direction;
    [
        0.282_095,
        0.488_603 * y,
        0.488_603 * z,
        0.488_603 * x,
        1.092_548 * x * y,
        1.092_548 * y * z,
        0.315_392 * (3.0 * z * z - 1.0),
        1.092_548 * x * z,
        0.546_274 * (x * x - y * y),
        0.590_044 * y * (3.0 * x * x - y * y),
        2.890_611 * x * y * z,
        0.457_046 * y * (5.0 * z * z - 1.0),
        0.373_176 * z * (5.0 * z * z - 3.0),
        0.457_046 * x * (5.0 * z * z - 1.0),
        1.445_306 * z * (x * x - y * y),
        0.590_044 * x * (x * x - 3.0 * y * y),
    ]
}

#[allow(clippy::needless_range_loop)] // Indexed loops directly express Cholesky factorization.
fn cholesky(
    matrix: &[[f32; SPHERICAL_HARMONIC_COEFFICIENTS]; SPHERICAL_HARMONIC_COEFFICIENTS],
) -> Result<[[f32; SPHERICAL_HARMONIC_COEFFICIENTS]; SPHERICAL_HARMONIC_COEFFICIENTS], HrtfError> {
    let mut lower = [[0.0; SPHERICAL_HARMONIC_COEFFICIENTS]; SPHERICAL_HARMONIC_COEFFICIENTS];
    for row in 0..SPHERICAL_HARMONIC_COEFFICIENTS {
        for column in 0..=row {
            let mut value = matrix[row][column];
            for inner in 0..column {
                value -= lower[row][inner] * lower[column][inner];
            }
            if row == column {
                if !value.is_finite() || value <= f32::EPSILON {
                    return Err(HrtfError::SphericalHarmonicFitFailed);
                }
                lower[row][column] = value.sqrt();
            } else {
                lower[row][column] = value / lower[column][column];
            }
        }
    }
    Ok(lower)
}

#[allow(clippy::needless_range_loop)] // Indexed loops solve all HRIR samples against one matrix.
fn solve_cholesky(
    lower: &[[f32; SPHERICAL_HARMONIC_COEFFICIENTS]; SPHERICAL_HARMONIC_COEFFICIENTS],
    right_hand_sides: &mut [f32],
    output_width: usize,
) {
    for row in 0..SPHERICAL_HARMONIC_COEFFICIENTS {
        for output in 0..output_width {
            let mut value = right_hand_sides[row * output_width + output];
            for inner in 0..row {
                value -= lower[row][inner] * right_hand_sides[inner * output_width + output];
            }
            right_hand_sides[row * output_width + output] = value / lower[row][row];
        }
    }
    for row in (0..SPHERICAL_HARMONIC_COEFFICIENTS).rev() {
        for output in 0..output_width {
            let mut value = right_hand_sides[row * output_width + output];
            for inner in row + 1..SPHERICAL_HARMONIC_COEFFICIENTS {
                value -= lower[inner][row] * right_hand_sides[inner * output_width + output];
            }
            right_hand_sides[row * output_width + output] = value / lower[row][row];
        }
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

fn triangle_candidate_indices(
    dataset: &HrtfDataset,
    direction: Vec3,
    distances: &[(usize, f32)],
) -> Vec<usize> {
    let mut candidates = distances
        .iter()
        .take(TRIANGLE_NEAREST_CANDIDATES)
        .map(|(index, _)| *index)
        .collect::<Vec<_>>();
    let reference = if direction.y.abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let tangent_x = direction.cross(reference).normalize();
    let tangent_y = direction.cross(tangent_x).normalize();
    let mut bins = [None; TRIANGLE_AZIMUTH_BINS];

    for (index, measurement) in dataset.measurements().iter().enumerate() {
        let x = measurement.direction.dot(tangent_x);
        let y = measurement.direction.dot(tangent_y);
        if x.mul_add(x, y * y) <= TRIANGLE_EPSILON {
            continue;
        }
        let angle = y.atan2(x) + std::f32::consts::PI;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            clippy::cast_sign_loss
        )]
        // The fixed bin count is small and the angle has already been bounded to one turn.
        let bin = ((angle / std::f32::consts::TAU * TRIANGLE_AZIMUTH_BINS as f32) as usize)
            .min(TRIANGLE_AZIMUTH_BINS - 1);
        let distance = direction.dot(measurement.direction).clamp(-1.0, 1.0).acos();
        if bins[bin].is_none_or(|(_, best_distance)| distance < best_distance) {
            bins[bin] = Some((index, distance));
        }
    }
    for (index, _) in bins.into_iter().flatten() {
        if !candidates.contains(&index) {
            candidates.push(index);
        }
    }
    candidates
}

fn spherical_triangle_weights(
    target: Vec3,
    vertices: [Vec3; NEIGHBOR_COUNT],
) -> Option<([f32; NEIGHBOR_COUNT], f32)> {
    let [first, second, third] = vertices;
    if !same_spherical_side(first, second, target, third)
        || !same_spherical_side(second, third, target, first)
        || !same_spherical_side(third, first, target, second)
    {
        return None;
    }

    let area = spherical_triangle_area(first, second, third);
    if !area.is_finite() || !(TRIANGLE_EPSILON..std::f32::consts::TAU).contains(&area) {
        return None;
    }
    let sub_areas = [
        spherical_triangle_area(target, second, third),
        spherical_triangle_area(target, third, first),
        spherical_triangle_area(target, first, second),
    ];
    let sum = sub_areas.iter().sum::<f32>();
    if !sum.is_finite() || sum <= TRIANGLE_EPSILON || (sum - area).abs() > 1.0e-3 {
        return None;
    }
    Some((sub_areas.map(|sub_area| sub_area / sum), area))
}

fn same_spherical_side(edge_start: Vec3, edge_end: Vec3, target: Vec3, opposite: Vec3) -> bool {
    let normal = edge_start.cross(edge_end);
    let target_side = normal.dot(target);
    let opposite_side = normal.dot(opposite);
    opposite_side.abs() > TRIANGLE_EPSILON && target_side * opposite_side >= -TRIANGLE_EPSILON
}

fn spherical_triangle_area(first: Vec3, second: Vec3, third: Vec3) -> f32 {
    let numerator = first.dot(second.cross(third)).abs();
    let denominator = 1.0 + first.dot(second) + second.dot(third) + third.dot(first);
    2.0 * numerator.atan2(denominator).abs()
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

fn interpolate_time_aligned(
    dataset: &HrtfDataset,
    contributors: Vec<InterpolationContributor>,
) -> InterpolatedHrir {
    if contributors.len() == 1 {
        return interpolate_contributors(dataset, contributors);
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

    InterpolatedHrir {
        left: interpolate_time_aligned_channel(&left_responses, dataset.hrir_length()),
        right: interpolate_time_aligned_channel(&right_responses, dataset.hrir_length()),
        contributors,
    }
}

fn interpolate_minimum_phase(
    dataset: &HrtfDataset,
    contributors: Vec<InterpolationContributor>,
) -> Result<InterpolatedHrir, HrtfError> {
    if contributors.len() == 1 {
        return Ok(interpolate_contributors(dataset, contributors));
    }

    let left_responses = weighted_responses(dataset, &contributors, |measurement| {
        measurement.left.as_slice()
    });
    let right_responses = weighted_responses(dataset, &contributors, |measurement| {
        measurement.right.as_slice()
    });

    Ok(InterpolatedHrir {
        left: interpolate_minimum_phase_channel(&left_responses, dataset.hrir_length())?,
        right: interpolate_minimum_phase_channel(&right_responses, dataset.hrir_length())?,
        contributors,
    })
}

fn interpolate_frequency_domain(
    dataset: &HrtfDataset,
    contributors: Vec<InterpolationContributor>,
) -> InterpolatedHrir {
    if contributors.len() == 1 {
        return interpolate_contributors(dataset, contributors);
    }

    let left_responses = weighted_responses(dataset, &contributors, |measurement| {
        measurement.left.as_slice()
    });
    let right_responses = weighted_responses(dataset, &contributors, |measurement| {
        measurement.right.as_slice()
    });

    InterpolatedHrir {
        left: interpolate_frequency_domain_channel(&left_responses, dataset.hrir_length()),
        right: interpolate_frequency_domain_channel(&right_responses, dataset.hrir_length()),
        contributors,
    }
}

fn weighted_responses<'a>(
    dataset: &'a HrtfDataset,
    contributors: &[InterpolationContributor],
    channel: impl Fn(&'a crate::HrirMeasurement) -> &'a [f32],
) -> Vec<(&'a [f32], f32)> {
    contributors
        .iter()
        .map(|contributor| {
            (
                channel(&dataset.measurements()[contributor.measurement_index]),
                contributor.weight,
            )
        })
        .collect()
}

fn interpolate_minimum_phase_channel(
    responses: &[(&[f32], f32)],
    output_length: usize,
) -> Result<Vec<f32>, HrtfError> {
    let fft_length = output_length
        .checked_mul(2)
        .and_then(usize::checked_next_power_of_two)
        .ok_or(HrtfError::InvalidHrirLength)?;
    // The fixed 256-point transform is inexpensive, and the scalar planner avoids browser-specific
    // SIMD feature detection and its unsupported-instruction trap paths.
    let mut planner = FftPlannerScalar::<f32>::new();
    let forward = planner.plan_fft_forward(fft_length);
    let inverse = planner.plan_fft_inverse(fft_length);
    let mut weighted_log_magnitude = vec![0.0; fft_length];

    for (response, weight) in responses {
        let mut spectrum = vec![Complex32::ZERO; fft_length];
        for (bin, sample) in spectrum.iter_mut().zip(*response) {
            bin.re = *sample;
        }
        forward.process(&mut spectrum);
        for (weighted, bin) in weighted_log_magnitude.iter_mut().zip(spectrum) {
            *weighted += weight * bin.norm().max(1.0e-12).ln();
        }
    }

    let mut cepstrum = weighted_log_magnitude
        .into_iter()
        .map(|value| Complex32::new(value, 0.0))
        .collect::<Vec<_>>();
    inverse.process(&mut cepstrum);
    #[allow(clippy::cast_precision_loss)]
    let fft_scale = fft_length as f32;
    for coefficient in &mut cepstrum {
        *coefficient /= fft_scale;
    }
    for coefficient in &mut cepstrum[1..fft_length / 2] {
        *coefficient *= 2.0;
    }
    for coefficient in &mut cepstrum[fft_length / 2 + 1..] {
        *coefficient = Complex32::ZERO;
    }

    forward.process(&mut cepstrum);
    for bin in &mut cepstrum {
        *bin = Complex32::from_polar(bin.re.exp(), bin.im);
    }
    inverse.process(&mut cepstrum);
    let minimum_phase = cepstrum
        .into_iter()
        .take(output_length)
        .map(|sample| sample.re / fft_scale)
        .collect::<Vec<_>>();
    let delay = responses
        .iter()
        .map(|(response, weight)| weight * peak_delay(response))
        .sum::<f32>();
    let mut source_position = -delay;
    Ok((0..output_length)
        .map(|_| {
            let sample = linear_sample(&minimum_phase, source_position);
            source_position += 1.0;
            sample
        })
        .collect())
}

fn interpolate_frequency_domain_channel(
    responses: &[(&[f32], f32)],
    output_length: usize,
) -> Vec<f32> {
    let mut planner = FftPlannerScalar::<f32>::new();
    let forward = planner.plan_fft_forward(output_length);
    let inverse = planner.plan_fft_inverse(output_length);
    let mut magnitudes = vec![0.0; output_length];
    let mut phase_vectors = vec![Complex32::ZERO; output_length];
    let mut complex_fallback = vec![Complex32::ZERO; output_length];

    for (response, weight) in responses {
        let mut spectrum = response
            .iter()
            .map(|sample| Complex32::new(*sample, 0.0))
            .collect::<Vec<_>>();
        forward.process(&mut spectrum);
        for (((magnitude, phase_vector), fallback), bin) in magnitudes
            .iter_mut()
            .zip(&mut phase_vectors)
            .zip(&mut complex_fallback)
            .zip(spectrum)
        {
            let bin_magnitude = bin.norm();
            *magnitude += weight * bin_magnitude;
            *fallback += weight * bin;
            if bin_magnitude > 1.0e-12 {
                *phase_vector += weight * bin / bin_magnitude;
            }
        }
    }

    let mut spectrum = magnitudes
        .into_iter()
        .zip(phase_vectors)
        .zip(complex_fallback)
        .map(|((magnitude, phase_vector), fallback)| {
            let phase = if phase_vector.norm_sqr() > 1.0e-12 {
                phase_vector.arg()
            } else if fallback.norm_sqr() > 1.0e-12 {
                fallback.arg()
            } else {
                0.0
            };
            Complex32::from_polar(magnitude, phase)
        })
        .collect::<Vec<_>>();
    inverse.process(&mut spectrum);
    #[allow(clippy::cast_precision_loss)]
    let scale = output_length as f32;
    spectrum
        .into_iter()
        .map(|sample| sample.re / scale)
        .collect()
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
    fn spherical_triangle_uses_normalized_area_weights() {
        let dataset = HrtfDataset::new(
            48_000,
            [(Vec3::X, 1.0), (Vec3::Y, 4.0), (Vec3::Z, 7.0)]
                .into_iter()
                .map(|(direction, sample)| {
                    HrirMeasurement::new(direction, vec![sample], vec![sample * 10.0]).unwrap()
                })
                .collect(),
        )
        .unwrap();

        let result = SphericalTriangleInterpolator
            .interpolate(&dataset, Vec3::ONE.normalize())
            .unwrap();

        assert_eq!(result.contributors.len(), 3);
        assert!(
            result
                .contributors
                .iter()
                .all(|entry| entry.weight.is_finite() && entry.weight > 0.0)
        );
        assert!(
            (result
                .contributors
                .iter()
                .map(|entry| entry.weight)
                .sum::<f32>()
                - 1.0)
                .abs()
                < 1.0e-6
        );
        assert!(
            result
                .contributors
                .iter()
                .all(|entry| (entry.weight - 1.0 / 3.0).abs() < 1.0e-6)
        );
        assert!((result.left[0] - 4.0).abs() < 1.0e-6);
        assert!((result.right[0] - 40.0).abs() < 1.0e-5);
    }

    #[test]
    fn spherical_triangle_crosses_the_azimuth_seam() {
        let measurements = [(170.0, -10.0), (-170.0, -10.0), (180.0, 20.0)]
            .into_iter()
            .map(|(azimuth, elevation)| {
                HrirMeasurement::new(
                    spherical_to_direction(azimuth, elevation),
                    vec![1.0],
                    vec![1.0],
                )
                .unwrap()
            })
            .collect();
        let dataset = HrtfDataset::new(48_000, measurements).unwrap();

        let contributors = SphericalTriangleInterpolator::contributors(
            &dataset,
            spherical_to_direction(180.0, 0.0),
        )
        .unwrap();

        assert_eq!(contributors.len(), 3);
        assert!(contributors.iter().all(|entry| entry.weight >= 0.0));
        assert!((contributors.iter().map(|entry| entry.weight).sum::<f32>() - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn spherical_triangle_preserves_an_exact_measurement() {
        let dataset = synthetic_dataset();
        let result = SphericalTriangleInterpolator
            .interpolate(&dataset, Vec3::X)
            .unwrap();

        assert_eq!(result.left, dataset.measurements()[1].left);
        assert_eq!(result.right, dataset.measurements()[1].right);
        assert_eq!(result.contributors.len(), 1);
    }

    #[test]
    fn time_aligned_spherical_triangle_reduces_impulse_smearing() {
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

        let direct = SphericalTriangleInterpolator
            .interpolate(&dataset, direction)
            .unwrap();
        let aligned = TimeAlignedSphericalTriangleInterpolator
            .interpolate(&dataset, direction)
            .unwrap();

        assert_eq!(aligned.contributors, direct.contributors);
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
    fn time_aligned_spherical_triangle_preserves_an_exact_measurement() {
        let dataset = synthetic_dataset();
        let result = TimeAlignedSphericalTriangleInterpolator
            .interpolate(&dataset, Vec3::X)
            .unwrap();

        assert_eq!(result.left, dataset.measurements()[1].left);
        assert_eq!(result.right, dataset.measurements()[1].right);
        assert_eq!(result.contributors.len(), 1);
    }

    #[test]
    fn minimum_phase_interpolation_restores_weighted_ear_delays() {
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

        let result = MinimumPhaseInterpolator
            .interpolate(&dataset, Vec3::ONE.normalize())
            .unwrap();
        let left_peak = result
            .left
            .iter()
            .enumerate()
            .max_by(|left, right| {
                left.1
                    .abs()
                    .partial_cmp(&right.1.abs())
                    .unwrap_or(Ordering::Equal)
            })
            .unwrap();
        let right_peak = result
            .right
            .iter()
            .enumerate()
            .max_by(|left, right| {
                left.1
                    .abs()
                    .partial_cmp(&right.1.abs())
                    .unwrap_or(Ordering::Equal)
            })
            .unwrap();

        assert_eq!(left_peak.0, 3);
        assert_eq!(right_peak.0, 4);
        assert!((*left_peak.1 - 1.0).abs() < 1.0e-5);
        assert!((*right_peak.1 - 1.0).abs() < 1.0e-5);
        assert!(result.left.iter().all(|sample| sample.is_finite()));
        assert!(result.right.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn minimum_phase_interpolation_preserves_an_exact_measurement() {
        let dataset = synthetic_dataset();
        let result = MinimumPhaseInterpolator
            .interpolate(&dataset, Vec3::X)
            .unwrap();

        assert_eq!(result.left, dataset.measurements()[1].left);
        assert_eq!(result.right, dataset.measurements()[1].right);
        assert_eq!(result.contributors.len(), 1);
    }

    #[test]
    fn frequency_domain_interpolation_reconstructs_shared_hrirs() {
        let left = vec![0.25, -0.5, 0.75, 0.125, 0.0, 0.0, 0.0, 0.0];
        let right = vec![-0.125, 0.25, 0.5, -0.25, 0.0, 0.0, 0.0, 0.0];
        let dataset = HrtfDataset::new(
            48_000,
            [Vec3::X, Vec3::Y, Vec3::Z]
                .into_iter()
                .map(|direction| {
                    HrirMeasurement::new(direction, left.clone(), right.clone()).unwrap()
                })
                .collect(),
        )
        .unwrap();

        let result = FrequencyDomainInterpolator
            .interpolate(&dataset, Vec3::ONE.normalize())
            .unwrap();

        assert_eq!(result.contributors.len(), 3);
        for (actual, expected) in result.left.iter().zip(left) {
            assert!((actual - expected).abs() < 1.0e-5);
        }
        for (actual, expected) in result.right.iter().zip(right) {
            assert!((actual - expected).abs() < 1.0e-5);
        }
    }

    #[test]
    fn frequency_domain_interpolation_handles_different_measured_phases() {
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
        let frequency = FrequencyDomainInterpolator
            .interpolate(&dataset, direction)
            .unwrap();

        assert_eq!(frequency.left.len(), dataset.hrir_length());
        assert_eq!(frequency.right.len(), dataset.hrir_length());
        assert!(frequency.left.iter().all(|sample| sample.is_finite()));
        assert!(frequency.right.iter().all(|sample| sample.is_finite()));
        assert_ne!(frequency.left, direct.left);
        assert_ne!(frequency.right, direct.right);
    }

    #[test]
    fn frequency_domain_interpolation_preserves_an_exact_measurement() {
        let dataset = synthetic_dataset();
        let result = FrequencyDomainInterpolator
            .interpolate(&dataset, Vec3::X)
            .unwrap();

        assert_eq!(result.left, dataset.measurements()[1].left);
        assert_eq!(result.right, dataset.measurements()[1].right);
        assert_eq!(result.contributors.len(), 1);
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // The synthetic degree values are small exact integers.
    fn spherical_harmonic_fit_reconstructs_low_order_directional_fields() {
        let mut measurements = Vec::new();
        for elevation in [-60.0, -30.0, 0.0, 30.0, 60.0] {
            for azimuth in (0..360).step_by(30) {
                let direction = spherical_to_direction(azimuth as f32, elevation);
                let left = vec![
                    0.3 + 0.2 * direction.x - 0.1 * direction.y + 0.05 * direction.z,
                    -0.2 + 0.15 * direction.y,
                ];
                let right = vec![
                    -0.1 + 0.25 * direction.z,
                    0.4 - 0.1 * direction.x + 0.2 * direction.y,
                ];
                measurements.push(HrirMeasurement::new(direction, left, right).unwrap());
            }
        }
        let dataset = HrtfDataset::new(48_000, measurements).unwrap();
        let target = spherical_to_direction(43.0, 17.0);
        let result = SphericalHarmonicInterpolator
            .interpolate(&dataset, target)
            .unwrap();
        let expected_left = [
            0.3 + 0.2 * target.x - 0.1 * target.y + 0.05 * target.z,
            -0.2 + 0.15 * target.y,
        ];
        let expected_right = [
            -0.1 + 0.25 * target.z,
            0.4 - 0.1 * target.x + 0.2 * target.y,
        ];

        assert!(result.contributors.is_empty());
        for (actual, expected) in result.left.iter().zip(expected_left) {
            assert!((actual - expected).abs() < 1.0e-4);
        }
        for (actual, expected) in result.right.iter().zip(expected_right) {
            assert!((actual - expected).abs() < 1.0e-4);
        }
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // The synthetic degree values are small exact integers.
    fn spherical_harmonic_model_is_cached_per_dataset() {
        let measurements = (0..SPHERICAL_HARMONIC_COEFFICIENTS)
            .map(|index| {
                let azimuth = index as f32 * 137.5;
                let elevation = -70.0 + index as f32 * 140.0 / 15.0;
                HrirMeasurement::new(
                    spherical_to_direction(azimuth, elevation),
                    vec![index as f32],
                    vec![-(index as f32)],
                )
                .unwrap()
            })
            .collect();
        let dataset = HrtfDataset::new(48_000, measurements).unwrap();

        let first = std::ptr::from_ref(dataset.spherical_harmonics().unwrap());
        let second = std::ptr::from_ref(dataset.spherical_harmonics().unwrap());

        assert_eq!(first, second);
    }

    #[test]
    fn spherical_harmonic_fit_requires_enough_measurements() {
        let dataset = synthetic_dataset();
        assert_eq!(
            SphericalHarmonicInterpolator.interpolate(&dataset, Vec3::Z),
            Err(HrtfError::NotEnoughMeasurements {
                available: 4,
                required: SPHERICAL_HARMONIC_COEFFICIENTS,
            })
        );
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
