//! Coarse-grained WASM bindings for the browser audio proof of concept.

use hrtf::{
    HrirInterpolator, HrtfDataset, InterpolationContributor, NearestThreeInterpolator, Vec3,
    direction_to_spherical, render_binaural, spherical_to_direction,
};
use wasm_bindgen::prelude::*;

#[cfg(any(target_arch = "wasm32", test))]
mod camera;
#[cfg(any(target_arch = "wasm32", test))]
mod head;
#[cfg(target_arch = "wasm32")]
mod renderer;

const DEFAULT_AZIMUTH_DEGREES: f32 = 37.2;
const DEFAULT_ELEVATION_DEGREES: f32 = 14.7;
const MAX_TEST_DURATION_SECONDS: f32 = 10.0;
const MAX_TEST_SAMPLES: f64 = 1_000_000.0;

/// Long-lived browser application state. The HRTF data is owned only by Rust.
#[wasm_bindgen]
pub struct BinauralApp {
    dataset: Option<HrtfDataset>,
    direction: Vec3,
    #[cfg(target_arch = "wasm32")]
    renderer: Option<renderer::Renderer>,
}

#[wasm_bindgen]
impl BinauralApp {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self {
            dataset: None,
            direction: spherical_to_direction(DEFAULT_AZIMUTH_DEGREES, DEFAULT_ELEVATION_DEGREES),
            #[cfg(target_arch = "wasm32")]
            renderer: None,
        }
    }

    /// Parses and retains one complete runtime HRTF dataset.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error if the dataset bytes fail native validation.
    pub fn load_hrtf(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.dataset = Some(HrtfDataset::from_runtime_bytes(bytes).map_err(js_error)?);
        Ok(())
    }

    #[must_use]
    pub fn is_hrtf_loaded(&self) -> bool {
        self.dataset.is_some()
    }

    /// Returns the processing sample rate from the loaded dataset.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before a dataset has been loaded.
    pub fn sample_rate(&self) -> Result<u32, JsError> {
        Ok(self.dataset()?.sample_rate())
    }

    /// Returns the number of measured HRIR directions in the loaded dataset.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before a dataset has been loaded.
    pub fn measurement_count(&self) -> Result<u32, JsError> {
        u32::try_from(self.dataset()?.measurements().len())
            .map_err(|_| JsError::new("HRTF dataset has too many measurements"))
    }

    /// Updates the physical source direction without regard to camera state.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error for non-finite angles or before dataset loading.
    pub fn set_direction(
        &mut self,
        azimuth_degrees: f32,
        elevation_degrees: f32,
    ) -> Result<SelectionInfo, JsError> {
        if !azimuth_degrees.is_finite() || !elevation_degrees.is_finite() {
            return Err(JsError::new("source angles must be finite"));
        }
        self.direction = spherical_to_direction(azimuth_degrees, elevation_degrees);
        let (selection, contributors) = selection_state(self.dataset()?, self.direction)?;
        #[cfg(target_arch = "wasm32")]
        if let Some(renderer) = &mut self.renderer {
            renderer
                .set_selection(self.direction, &contributors)
                .map_err(js_error)?;
            renderer.render().map_err(js_error)?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        drop(contributors);
        Ok(selection)
    }

    /// Returns the current direction and interpolation diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before dataset loading or if interpolation fails.
    pub fn selection_info(&self) -> Result<SelectionInfo, JsError> {
        selection_state(self.dataset()?, self.direction).map(|(selection, _)| selection)
    }

    /// Generates a deterministic built-in mono test signal at the dataset sample rate.
    ///
    /// Supported kinds are `pink-noise` and `click`.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error for an unknown kind, invalid duration, or before dataset loading.
    pub fn generate_test_signal(
        &self,
        kind: &str,
        duration_seconds: f32,
    ) -> Result<Vec<f32>, JsError> {
        if !duration_seconds.is_finite()
            || duration_seconds <= 0.0
            || duration_seconds > MAX_TEST_DURATION_SECONDS
        {
            return Err(JsError::new(
                "test duration must be finite and between 0 and 10 seconds",
            ));
        }
        let sample_count = f64::from(duration_seconds) * f64::from(self.sample_rate()?);
        if sample_count > MAX_TEST_SAMPLES {
            return Err(JsError::new("test signal would contain too many samples"));
        }
        // The preceding finite, positive, and upper-bound checks make this conversion safe.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sample_count = sample_count.round() as usize;
        match kind {
            "pink-noise" => Ok(generate_pink_noise(sample_count)),
            "click" => Ok(generate_click(sample_count)),
            _ => Err(JsError::new("unknown test signal")),
        }
    }

    /// Interpolates the current HRIR and renders a complete mono buffer to stereo.
    ///
    /// Input samples must be at [`Self::sample_rate`].
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before dataset loading or for invalid/empty samples.
    pub fn process_audio(&self, mono_samples: &[f32]) -> Result<ProcessedAudio, JsError> {
        let hrir = self.interpolated_hrir()?;
        let rendered = render_binaural(mono_samples, &hrir).map_err(js_error)?;
        Ok(ProcessedAudio {
            left: rendered.left,
            right: rendered.right,
            applied_gain: rendered.applied_gain,
        })
    }

    /// Returns the interpolated HRIR for the current source direction.
    ///
    /// This coarse-grained snapshot is sent only when direction changes, rather than crossing the
    /// JS/WASM boundary for individual audio samples.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before dataset loading or if interpolation fails.
    pub fn current_hrir(&self) -> Result<BrowserHrir, JsError> {
        let hrir = self.interpolated_hrir()?;
        Ok(BrowserHrir {
            left: hrir.left,
            right: hrir.right,
        })
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BinauralApp {
    /// Initializes the Rust/wgpu scene on the supplied browser canvas.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error when no compatible WebGPU adapter/device or canvas surface is
    /// available, or when GPU initialization fails.
    pub async fn initialize_renderer(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), JsError> {
        let measurement_directions = self
            .dataset()?
            .measurements()
            .iter()
            .map(|measurement| measurement.direction)
            .collect::<Vec<_>>();
        let (_, contributors) = selection_state(self.dataset()?, self.direction)?;
        let renderer = renderer::Renderer::new(
            canvas,
            self.direction,
            &measurement_directions,
            &contributors,
        )
        .await
        .map_err(js_error)?;
        self.renderer = Some(renderer);
        Ok(())
    }

    /// Resizes the GPU surface to match its CSS dimensions and device pixel ratio.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before renderer initialization or if drawing fails.
    pub fn resize_renderer(
        &mut self,
        css_width: f32,
        css_height: f32,
        device_pixel_ratio: f32,
    ) -> Result<(), JsError> {
        self.renderer_mut()?
            .resize(css_width, css_height, device_pixel_ratio)
            .map_err(js_error)
    }

    /// Orbits the camera by a browser pointer delta in CSS pixels.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before renderer initialization or if drawing fails.
    pub fn orbit_camera(&mut self, delta_x: f32, delta_y: f32) -> Result<(), JsError> {
        self.renderer_mut()?
            .orbit(delta_x, delta_y)
            .map_err(js_error)
    }

    /// Zooms the orbit camera by a browser wheel delta.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before renderer initialization or if drawing fails.
    pub fn zoom_camera(&mut self, wheel_delta: f32) -> Result<(), JsError> {
        self.renderer_mut()?.zoom(wheel_delta).map_err(js_error)
    }

    /// Applies a canonical camera view without changing the physical source direction.
    ///
    /// Supported presets are `front`, `back`, `left`, `right`, `top`, and `reset`.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error for an unknown preset, before renderer initialization, or if
    /// drawing fails.
    pub fn set_camera_preset(&mut self, preset: &str) -> Result<(), JsError> {
        let preset = match preset {
            "front" => camera::CameraPreset::Front,
            "back" => camera::CameraPreset::Back,
            "left" => camera::CameraPreset::Left,
            "right" => camera::CameraPreset::Right,
            "top" => camera::CameraPreset::Top,
            "reset" => camera::CameraPreset::Reset,
            _ => return Err(JsError::new("unknown camera preset")),
        };
        self.renderer_mut()?
            .set_camera_preset(preset)
            .map_err(js_error)
    }

    /// Shows or hides one independently rendered scene layer.
    ///
    /// Supported layers are `measurements`, `grid`, `axes`, and `contributor-guides`.
    /// The head, selected source, and contributor markers always remain visible.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error for an unknown layer, before renderer initialization, or if
    /// drawing fails.
    pub fn set_display_layer(&mut self, layer: &str, visible: bool) -> Result<(), JsError> {
        let layer = match layer {
            "measurements" => renderer::DisplayLayer::Measurements,
            "grid" => renderer::DisplayLayer::Grid,
            "axes" => renderer::DisplayLayer::Axes,
            "contributor-guides" => renderer::DisplayLayer::ContributorGuides,
            _ => return Err(JsError::new("unknown display layer")),
        };
        self.renderer_mut()?
            .set_display_layer(layer, visible)
            .map_err(js_error)
    }

    /// Applies a combined orbit/pinch gesture with one GPU redraw.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error for non-finite deltas, before renderer initialization, or if
    /// drawing fails.
    pub fn navigate_camera(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        wheel_delta: f32,
    ) -> Result<(), JsError> {
        self.renderer_mut()?
            .navigate_camera(delta_x, delta_y, wheel_delta)
            .map_err(js_error)
    }

    /// Reports the active wgpu browser backend for diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error before renderer initialization.
    pub fn renderer_backend(&self) -> Result<String, JsError> {
        Ok(self.renderer()?.backend_name().to_owned())
    }

    /// Casts a camera ray through a CSS-space canvas point and selects the source sphere hit.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error for invalid canvas dimensions, a missed sphere, renderer state,
    /// or interpolation failure.
    pub fn select_canvas_position(
        &mut self,
        x: f32,
        y: f32,
        css_width: f32,
        css_height: f32,
    ) -> Result<SelectionInfo, JsError> {
        let direction = self
            .renderer_mut()?
            .pick_direction(x, y, css_width, css_height)
            .map_err(js_error)?;
        self.direction = direction;
        let (selection, contributors) = selection_state(self.dataset()?, direction)?;
        self.renderer_mut()?
            .set_selection(direction, &contributors)
            .map_err(js_error)?;
        self.renderer_mut()?.render().map_err(js_error)?;
        Ok(selection)
    }
}

#[cfg(target_arch = "wasm32")]
impl BinauralApp {
    fn renderer(&self) -> Result<&renderer::Renderer, JsError> {
        self.renderer
            .as_ref()
            .ok_or_else(|| JsError::new("3D renderer is not initialized"))
    }

    fn renderer_mut(&mut self) -> Result<&mut renderer::Renderer, JsError> {
        self.renderer
            .as_mut()
            .ok_or_else(|| JsError::new("3D renderer is not initialized"))
    }
}

fn selection_state(
    dataset: &HrtfDataset,
    direction: Vec3,
) -> Result<(SelectionInfo, Vec<(Vec3, f32)>), JsError> {
    let spherical = direction_to_spherical(direction).map_err(js_error)?;
    let contributors =
        NearestThreeInterpolator::contributors(dataset, direction).map_err(js_error)?;
    let contributor_summary = contributor_summary(&contributors);
    let visuals = contributors
        .iter()
        .map(|contributor| {
            (
                dataset.measurements()[contributor.measurement_index].direction,
                contributor.weight,
            )
        })
        .collect();
    Ok((
        SelectionInfo {
            azimuth_degrees: spherical.azimuth_degrees,
            elevation_degrees: spherical.elevation_degrees,
            x: direction.x,
            y: direction.y,
            z: direction.z,
            contributor_summary,
        },
        visuals,
    ))
}

fn contributor_summary(contributors: &[InterpolationContributor]) -> String {
    contributors
        .iter()
        .map(|contributor| {
            format!(
                "#{}  w={:.3}  Δ={:.2}°",
                contributor.measurement_index,
                contributor.weight,
                contributor.angular_distance_radians.to_degrees(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Default for BinauralApp {
    fn default() -> Self {
        Self::new()
    }
}

impl BinauralApp {
    fn dataset(&self) -> Result<&HrtfDataset, JsError> {
        self.dataset
            .as_ref()
            .ok_or_else(|| JsError::new("HRTF dataset is not loaded"))
    }

    fn interpolated_hrir(&self) -> Result<hrtf::InterpolatedHrir, JsError> {
        NearestThreeInterpolator
            .interpolate(self.dataset()?, self.direction)
            .map_err(js_error)
    }
}

/// Interpolated HRIR snapshot used to retarget the real-time audio worklet.
#[wasm_bindgen]
pub struct BrowserHrir {
    left: Vec<f32>,
    right: Vec<f32>,
}

#[wasm_bindgen]
impl BrowserHrir {
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn left(&self) -> Vec<f32> {
        self.left.clone()
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn right(&self) -> Vec<f32> {
        self.right.clone()
    }
}

/// Browser-facing direction and contributor diagnostics.
#[wasm_bindgen]
pub struct SelectionInfo {
    azimuth_degrees: f32,
    elevation_degrees: f32,
    x: f32,
    y: f32,
    z: f32,
    contributor_summary: String,
}

#[wasm_bindgen]
impl SelectionInfo {
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn azimuth_degrees(&self) -> f32 {
        self.azimuth_degrees
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn elevation_degrees(&self) -> f32 {
        self.elevation_degrees
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn x(&self) -> f32 {
        self.x
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn y(&self) -> f32 {
        self.y
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn z(&self) -> f32 {
        self.z
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn contributor_summary(&self) -> String {
        self.contributor_summary.clone()
    }
}

/// Complete rendered stereo buffers returned in one WASM call.
#[wasm_bindgen]
pub struct ProcessedAudio {
    left: Vec<f32>,
    right: Vec<f32>,
    applied_gain: f32,
}

#[wasm_bindgen]
impl ProcessedAudio {
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn left(&self) -> Vec<f32> {
        self.left.clone()
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn right(&self) -> Vec<f32> {
        self.right.clone()
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn applied_gain(&self) -> f32 {
        self.applied_gain
    }
}

fn js_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

fn generate_click(sample_count: usize) -> Vec<f32> {
    let mut samples = vec![0.0; sample_count];
    if let Some(first) = samples.first_mut() {
        *first = 0.7;
    }
    samples
}

fn generate_pink_noise(sample_count: usize) -> Vec<f32> {
    let mut state = 0x1234_5678_u32;
    let (mut b0, mut b1, mut b2) = (0.0_f32, 0.0_f32, 0.0_f32);
    (0..sample_count)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let bytes = state.to_le_bytes();
            let random = u16::from_le_bytes([bytes[2], bytes[3]]);
            let white = (f32::from(random) / f32::from(u16::MAX)).mul_add(2.0, -1.0);
            b0 = 0.997_65_f32.mul_add(b0, white * 0.099_046);
            b1 = 0.963_f32.mul_add(b1, white * 0.296_516_4);
            b2 = 0.57_f32.mul_add(b2, white * 1.052_691_3);
            (b0 + b1 + b2 + white * 0.1848) * 0.05
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packaged_app() -> BinauralApp {
        let mut app = BinauralApp::new();
        app.load_hrtf(include_bytes!("../../../frontend/assets/mit-kemar.bhrtf"))
            .unwrap();
        app
    }

    #[test]
    fn click_is_a_bounded_single_impulse() {
        assert_eq!(generate_click(4), vec![0.7, 0.0, 0.0, 0.0]);
        assert!(generate_click(0).is_empty());
    }

    #[test]
    fn pink_noise_is_deterministic_finite_and_non_silent() {
        let first = generate_pink_noise(1_000);
        let second = generate_pink_noise(1_000);
        assert_eq!(first, second);
        assert!(first.iter().all(|sample| sample.is_finite()));
        assert!(first.iter().any(|sample| sample.abs() > 0.001));
    }

    #[test]
    fn packaged_dataset_reports_measurements_and_three_default_contributors() {
        let app = packaged_app();

        assert_eq!(app.measurement_count().unwrap(), 710);
        let selection = app.selection_info().unwrap();
        assert_eq!(selection.contributor_summary.lines().count(), 3);
        assert!(selection.contributor_summary.contains("w="));
        assert!(selection.contributor_summary.contains("Δ="));
    }

    #[test]
    fn direction_selection_drives_mirrored_binaural_rendering() {
        let mut app = packaged_app();

        let right_selection = app.set_direction(90.0, 0.0).unwrap();
        assert!(right_selection.x > 0.999);
        assert_eq!(right_selection.contributor_summary.lines().count(), 1);
        let source_on_right = app.process_audio(&[1.0]).unwrap();

        let left_selection = app.set_direction(-90.0, 0.0).unwrap();
        assert!(left_selection.x < -0.999);
        assert_eq!(left_selection.contributor_summary.lines().count(), 1);
        let source_on_left = app.process_audio(&[1.0]).unwrap();

        assert_eq!(source_on_right.left.len(), 128);
        assert_eq!(source_on_right.right.len(), 128);
        assert_ne!(source_on_right.left, source_on_right.right);
        assert_eq!(source_on_right.left, source_on_left.right);
        assert_eq!(source_on_right.right, source_on_left.left);
    }

    #[test]
    fn substantially_different_selections_produce_different_stereo_results() {
        let mut app = packaged_app();
        app.set_direction(0.0, 0.0).unwrap();
        let front = app.process_audio(&[1.0]).unwrap();
        app.set_direction(120.0, 30.0).unwrap();
        let rear_side = app.process_audio(&[1.0]).unwrap();

        let absolute_difference: f32 = front
            .left
            .iter()
            .chain(&front.right)
            .zip(rear_side.left.iter().chain(&rear_side.right))
            .map(|(front, rear_side)| (front - rear_side).abs())
            .sum();
        assert!(absolute_difference > 0.1);
    }
}
