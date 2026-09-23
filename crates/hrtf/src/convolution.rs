use crate::{HrtfError, InterpolatedHrir};

const DEFAULT_TARGET_PEAK: f32 = 0.95;

pub trait Convolver {
    /// Computes a complete linear convolution.
    ///
    /// # Errors
    ///
    /// Returns an error for empty input or non-finite samples.
    fn convolve(&self, signal: &[f32], impulse: &[f32]) -> Result<Vec<f32>, HrtfError>;
}

/// Straightforward O(NM) convolution, appropriate for MVP clips and short HRIRs.
#[derive(Debug, Default, Clone, Copy)]
pub struct TimeDomainConvolver;

impl Convolver for TimeDomainConvolver {
    fn convolve(&self, signal: &[f32], impulse: &[f32]) -> Result<Vec<f32>, HrtfError> {
        if signal.is_empty() || impulse.is_empty() {
            return Err(HrtfError::EmptySignal);
        }
        if signal
            .iter()
            .chain(impulse)
            .any(|sample| !sample.is_finite())
        {
            return Err(HrtfError::NonFiniteSample);
        }

        let mut output = vec![0.0; signal.len() + impulse.len() - 1];
        for (signal_index, &signal_sample) in signal.iter().enumerate() {
            for (impulse_index, &impulse_sample) in impulse.iter().enumerate() {
                output[signal_index + impulse_index] += signal_sample * impulse_sample;
            }
        }
        Ok(output)
    }
}

/// Stateful direct-form stereo convolution for fixed-size real-time audio blocks.
///
/// The input history is shared by both ears. HRIR changes are output-equivalent to
/// crossfading two convolvers because convolution is linear, so only one history ring is needed.
#[derive(Debug, Clone)]
pub struct RealtimeBinauralConvolver {
    history: Vec<f32>,
    write_index: usize,
    current_left: Vec<f32>,
    current_right: Vec<f32>,
    target_left: Vec<f32>,
    target_right: Vec<f32>,
    fade_length_samples: usize,
    fade_position: usize,
}

impl RealtimeBinauralConvolver {
    /// Creates a stateful convolver with an initial stereo HRIR.
    ///
    /// # Errors
    ///
    /// Returns an error if the HRIR channels are empty, differ in length, or contain non-finite
    /// samples. A zero fade length is accepted and makes subsequent HRIR changes immediate.
    pub fn new(left: &[f32], right: &[f32], fade_length_samples: usize) -> Result<Self, HrtfError> {
        validate_hrir_channels(left, right)?;
        Ok(Self {
            history: vec![0.0; left.len()],
            write_index: 0,
            current_left: left.to_vec(),
            current_right: right.to_vec(),
            target_left: left.to_vec(),
            target_right: right.to_vec(),
            fade_length_samples,
            fade_position: fade_length_samples,
        })
    }

    /// Retargets the convolver while preserving its input history.
    ///
    /// If another transition is active, its current effective HRIR becomes the start of the new
    /// transition. This keeps rapid pointer updates continuous.
    ///
    /// # Errors
    ///
    /// Returns an error if either channel is invalid or differs from the configured HRIR length.
    pub fn set_hrir(&mut self, left: &[f32], right: &[f32]) -> Result<(), HrtfError> {
        validate_hrir_channels(left, right)?;
        if left.len() != self.history.len() {
            return Err(HrtfError::InvalidHrirLength);
        }

        self.materialize_current_transition();
        self.target_left.copy_from_slice(left);
        self.target_right.copy_from_slice(right);
        if self.fade_length_samples == 0 {
            self.current_left.copy_from_slice(left);
            self.current_right.copy_from_slice(right);
        }
        self.fade_position = 0;
        Ok(())
    }

    /// Processes one mono block and returns equally sized left/right output blocks.
    ///
    /// # Errors
    ///
    /// Returns an error if an input sample is non-finite.
    pub fn process_block(&mut self, input: &[f32]) -> Result<(Vec<f32>, Vec<f32>), HrtfError> {
        let mut left = vec![0.0; input.len()];
        let mut right = vec![0.0; input.len()];
        self.process_block_into(input, &mut left, &mut right)?;
        Ok((left, right))
    }

    /// Processes one mono block into caller-owned output buffers without allocating.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer lengths differ or an input sample is non-finite.
    pub fn process_block_into(
        &mut self,
        input: &[f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), HrtfError> {
        if input.len() != left.len() || input.len() != right.len() {
            return Err(HrtfError::InvalidSignalLength);
        }
        if input.iter().any(|sample| !sample.is_finite()) {
            return Err(HrtfError::NonFiniteSample);
        }
        for ((&sample, left_sample), right_sample) in
            input.iter().zip(left.iter_mut()).zip(right.iter_mut())
        {
            (*left_sample, *right_sample) = self.process_sample(sample);
        }
        Ok(())
    }

    fn process_sample(&mut self, sample: f32) -> (f32, f32) {
        self.history[self.write_index] = sample;
        let current_left = convolve_history(&self.history, self.write_index, &self.current_left);
        let current_right = convolve_history(&self.history, self.write_index, &self.current_right);

        let is_fading = self.fade_position < self.fade_length_samples;
        let output = if is_fading {
            let target_left = convolve_history(&self.history, self.write_index, &self.target_left);
            let target_right =
                convolve_history(&self.history, self.write_index, &self.target_right);
            self.fade_position += 1;
            let mix = fade_mix(self.fade_position, self.fade_length_samples);
            (
                mix.mul_add(target_left - current_left, current_left),
                mix.mul_add(target_right - current_right, current_right),
            )
        } else {
            (current_left, current_right)
        };

        if is_fading && self.fade_position == self.fade_length_samples {
            self.current_left.copy_from_slice(&self.target_left);
            self.current_right.copy_from_slice(&self.target_right);
        }
        self.write_index = (self.write_index + 1) % self.history.len();
        output
    }

    fn materialize_current_transition(&mut self) {
        if self.fade_position >= self.fade_length_samples || self.fade_length_samples == 0 {
            return;
        }
        let mix = fade_mix(self.fade_position, self.fade_length_samples);
        for (current, target) in self.current_left.iter_mut().zip(&self.target_left) {
            *current = mix.mul_add(*target - *current, *current);
        }
        for (current, target) in self.current_right.iter_mut().zip(&self.target_right) {
            *current = mix.mul_add(*target - *current, *current);
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn fade_mix(position: usize, length: usize) -> f32 {
    // Audio fades are only a few thousand samples long; all relevant values are exactly
    // representable by f32, while usize keeps indexing and completion checks straightforward.
    position as f32 / length as f32
}

fn validate_hrir_channels(left: &[f32], right: &[f32]) -> Result<(), HrtfError> {
    if left.is_empty() || left.len() != right.len() {
        return Err(HrtfError::InvalidHrirLength);
    }
    if left.iter().chain(right).any(|sample| !sample.is_finite()) {
        return Err(HrtfError::NonFiniteSample);
    }
    Ok(())
}

fn convolve_history(history: &[f32], newest_index: usize, impulse: &[f32]) -> f32 {
    impulse
        .iter()
        .enumerate()
        .map(|(tap, coefficient)| {
            let history_index = (newest_index + history.len() - tap) % history.len();
            history[history_index] * coefficient
        })
        .sum()
}

/// Rendered stereo data with the shared anti-clipping gain that was applied.
#[derive(Debug, Clone, PartialEq)]
pub struct BinauralOutput {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub applied_gain: f32,
}

/// Convolves mono audio with both HRIR channels and applies one shared gain.
///
/// # Errors
///
/// Returns an error if the source or either HRIR channel is empty or contains non-finite samples.
pub fn render_binaural(
    mono_signal: &[f32],
    hrir: &InterpolatedHrir,
) -> Result<BinauralOutput, HrtfError> {
    render_binaural_with(&TimeDomainConvolver, mono_signal, hrir, DEFAULT_TARGET_PEAK)
}

fn render_binaural_with(
    convolver: &impl Convolver,
    mono_signal: &[f32],
    hrir: &InterpolatedHrir,
    target_peak: f32,
) -> Result<BinauralOutput, HrtfError> {
    let mut left = convolver.convolve(mono_signal, &hrir.left)?;
    let mut right = convolver.convolve(mono_signal, &hrir.right)?;
    let peak = left
        .iter()
        .chain(&right)
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    let applied_gain = if peak > target_peak {
        target_peak / peak
    } else {
        1.0
    };
    if applied_gain < 1.0 {
        left.iter_mut()
            .chain(&mut right)
            .for_each(|sample| *sample *= applied_gain);
    }

    Ok(BinauralOutput {
        left,
        right,
        applied_gain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hrir(left: Vec<f32>, right: Vec<f32>) -> InterpolatedHrir {
        InterpolatedHrir {
            left,
            right,
            contributors: Vec::new(),
        }
    }

    #[test]
    fn impulse_convolution_reproduces_impulse_response() {
        let impulse_response = [0.25, -0.5, 0.125];
        assert_eq!(
            TimeDomainConvolver
                .convolve(&[1.0], &impulse_response)
                .unwrap(),
            impulse_response
        );
    }

    #[test]
    fn convolution_has_expected_values_and_length() {
        let result = TimeDomainConvolver
            .convolve(&[1.0, 2.0], &[3.0, 4.0, 5.0])
            .unwrap();
        assert_eq!(result, vec![3.0, 10.0, 13.0, 10.0]);
        assert_eq!(result.len(), 2 + 3 - 1);
    }

    #[test]
    fn binaural_render_preserves_left_right_order() {
        let result = render_binaural(&[1.0], &hrir(vec![0.25, 0.0], vec![0.0, 0.5])).unwrap();
        assert_eq!(result.left, vec![0.25, 0.0]);
        assert_eq!(result.right, vec![0.0, 0.5]);
    }

    #[test]
    fn normalization_uses_one_gain_for_both_channels() {
        let result = render_binaural(&[1.0], &hrir(vec![2.0], vec![1.0])).unwrap();
        assert!((result.left[0] - 0.95).abs() < 1.0e-6);
        assert!((result.right[0] - 0.475).abs() < 1.0e-6);
        assert!((result.applied_gain - 0.475).abs() < 1.0e-6);
    }

    #[test]
    fn empty_and_non_finite_inputs_are_rejected() {
        assert_eq!(
            TimeDomainConvolver.convolve(&[], &[1.0]),
            Err(HrtfError::EmptySignal)
        );
        assert_eq!(
            TimeDomainConvolver.convolve(&[f32::NAN], &[1.0]),
            Err(HrtfError::NonFiniteSample)
        );
    }

    #[test]
    fn realtime_blocks_match_continuous_time_domain_convolution() {
        let input = [1.0, 2.0, 3.0, 4.0];
        let impulse = [0.5, -0.25, 0.125];
        let expected = TimeDomainConvolver.convolve(&input, &impulse).unwrap();
        let mut realtime = RealtimeBinauralConvolver::new(&impulse, &impulse, 8).unwrap();

        let (first, _) = realtime.process_block(&input[..2]).unwrap();
        let (second, _) = realtime.process_block(&input[2..]).unwrap();
        let (tail, _) = realtime.process_block(&[0.0, 0.0]).unwrap();
        let actual = first
            .into_iter()
            .chain(second)
            .chain(tail)
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn realtime_convolver_preserves_stereo_order() {
        let mut realtime = RealtimeBinauralConvolver::new(&[1.0, 0.0], &[0.0, 1.0], 4).unwrap();
        let (left, right) = realtime.process_block(&[1.0, 0.0]).unwrap();
        assert_eq!(left, vec![1.0, 0.0]);
        assert_eq!(right, vec![0.0, 1.0]);
    }

    #[test]
    fn realtime_hrir_change_crossfades_and_reaches_target() {
        let mut realtime = RealtimeBinauralConvolver::new(&[1.0], &[2.0], 4).unwrap();
        realtime.set_hrir(&[5.0], &[10.0]).unwrap();
        let (left, right) = realtime.process_block(&[1.0; 5]).unwrap();

        assert_eq!(left, vec![2.0, 3.0, 4.0, 5.0, 5.0]);
        assert_eq!(right, vec![4.0, 6.0, 8.0, 10.0, 10.0]);
    }

    #[test]
    fn realtime_retarget_starts_from_active_transition() {
        let mut realtime = RealtimeBinauralConvolver::new(&[0.0], &[0.0], 4).unwrap();
        realtime.set_hrir(&[4.0], &[8.0]).unwrap();
        let (before, _) = realtime.process_block(&[1.0, 1.0]).unwrap();
        assert_eq!(before, vec![1.0, 2.0]);

        realtime.set_hrir(&[6.0], &[12.0]).unwrap();
        let (after, _) = realtime.process_block(&[1.0; 4]).unwrap();
        assert_eq!(after, vec![3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn realtime_convolver_rejects_invalid_hrirs_and_samples() {
        assert_eq!(
            RealtimeBinauralConvolver::new(&[], &[], 4).unwrap_err(),
            HrtfError::InvalidHrirLength
        );
        let mut realtime = RealtimeBinauralConvolver::new(&[1.0], &[1.0], 4).unwrap();
        assert_eq!(
            realtime.set_hrir(&[1.0, 2.0], &[1.0, 2.0]),
            Err(HrtfError::InvalidHrirLength)
        );
        assert_eq!(
            realtime.process_block(&[f32::NAN]),
            Err(HrtfError::NonFiniteSample)
        );
        assert_eq!(
            realtime.process_block_into(&[1.0], &mut [], &mut []),
            Err(HrtfError::InvalidSignalLength)
        );
    }
}
