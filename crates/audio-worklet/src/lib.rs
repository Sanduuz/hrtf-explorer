//! Minimal WASM boundary for stateful convolution inside an `AudioWorkletProcessor`.

use hrtf::RealtimeBinauralConvolver;
use wasm_bindgen::prelude::*;

const INITIAL_RENDER_QUANTUM_SIZE: usize = 128;
const MAX_RENDER_QUANTUM_SIZE: usize = 16_384;

/// Long-lived real-time DSP state owned by the browser audio rendering thread.
#[wasm_bindgen]
pub struct RealtimeAudioProcessor {
    convolver: Option<RealtimeBinauralConvolver>,
    input: Vec<f32>,
    left_output: Vec<f32>,
    right_output: Vec<f32>,
}

#[wasm_bindgen]
impl RealtimeAudioProcessor {
    /// Creates the processor with an initial device-ordered stereo HRIR.
    ///
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(left: &[f32], right: &[f32], fade_length_samples: usize) -> Self {
        Self {
            convolver: RealtimeBinauralConvolver::new(left, right, fade_length_samples).ok(),
            input: vec![0.0; INITIAL_RENDER_QUANTUM_SIZE],
            left_output: vec![0.0; INITIAL_RENDER_QUANTUM_SIZE],
            right_output: vec![0.0; INITIAL_RENDER_QUANTUM_SIZE],
        }
    }

    /// Reports whether the initial HRIR passed validation.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.convolver.is_some()
    }

    /// Begins a smooth transition to a new device-ordered HRIR, returning false on invalid input.
    pub fn set_hrir(&mut self, left: &[f32], right: &[f32]) -> bool {
        self.convolver
            .as_mut()
            .is_some_and(|convolver| convolver.set_hrir(left, right).is_ok())
    }

    /// Current Web Audio render quantum size.
    #[must_use]
    pub fn block_size(&self) -> usize {
        self.input.len()
    }

    /// Resizes the reusable input/output blocks for a browser render-quantum change.
    ///
    /// The normal 128-sample path never calls this after initialization. A browser that changes
    /// quantum size pays for allocation once at the transition, then returns to allocation-free
    /// block processing.
    pub fn set_block_size(&mut self, block_size: usize) -> bool {
        if block_size == 0 || block_size > MAX_RENDER_QUANTUM_SIZE {
            return false;
        }
        if block_size != self.input.len() {
            self.input.resize(block_size, 0.0);
            self.left_output.resize(block_size, 0.0);
            self.right_output.resize(block_size, 0.0);
        }
        true
    }

    /// Byte offset of the fixed mono input block in WASM linear memory.
    #[must_use]
    pub fn input_ptr(&mut self) -> usize {
        self.input.as_mut_ptr() as usize
    }

    /// Byte offset of the fixed left output block in WASM linear memory.
    #[must_use]
    pub fn left_output_ptr(&mut self) -> usize {
        self.left_output.as_mut_ptr() as usize
    }

    /// Byte offset of the fixed right output block in WASM linear memory.
    #[must_use]
    pub fn right_output_ptr(&mut self) -> usize {
        self.right_output.as_mut_ptr() as usize
    }

    /// Processes the queued fixed-size input block without allocating.
    pub fn process_queued_block(&mut self) -> bool {
        self.convolver.as_mut().is_some_and(|convolver| {
            convolver
                .process_block_into(&self.input, &mut self.left_output, &mut self.right_output)
                .is_ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_processes_stereo_without_swapping_channels() {
        let mut processor = RealtimeAudioProcessor::new(&[1.0], &[2.0], 16);
        assert!(processor.is_valid());
        processor.input[0] = 0.5;
        assert!(processor.process_queued_block());
        assert!((processor.left_output[0] - 0.5).abs() < f32::EPSILON);
        assert!((processor.right_output[0] - 1.0).abs() < f32::EPSILON);
        assert!(
            processor.left_output[1..]
                .iter()
                .all(|sample| sample.abs() < f32::EPSILON)
        );
    }

    #[test]
    fn wrapper_reports_invalid_data_without_a_string_error_boundary() {
        let mut processor = RealtimeAudioProcessor::new(&[], &[], 16);
        assert!(!processor.is_valid());
        assert!(!processor.process_queued_block());
        assert!(!processor.set_hrir(&[1.0], &[1.0]));
    }

    #[test]
    fn wrapper_adapts_to_render_quantum_changes() {
        let mut processor = RealtimeAudioProcessor::new(&[1.0], &[2.0], 16);
        assert!(processor.set_block_size(64));
        assert_eq!(processor.block_size(), 64);
        processor.input[63] = 0.25;
        assert!(processor.process_queued_block());
        assert!((processor.left_output[63] - 0.25).abs() < f32::EPSILON);
        assert!((processor.right_output[63] - 0.5).abs() < f32::EPSILON);

        assert!(!processor.set_block_size(0));
        assert!(!processor.set_block_size(MAX_RENDER_QUANTUM_SIZE + 1));
        assert_eq!(processor.block_size(), 64);
    }
}
