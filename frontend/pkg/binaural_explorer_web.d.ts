/* tslint:disable */
/* eslint-disable */

/**
 * Long-lived browser application state. The HRTF data is owned only by Rust.
 */
export class BinauralApp {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Returns the interpolated HRIR for the current source direction.
     *
     * This coarse-grained snapshot is sent only when direction changes, rather than crossing the
     * JS/WASM boundary for individual audio samples.
     *
     * # Errors
     *
     * Returns a JavaScript error before dataset loading or if interpolation fails.
     */
    current_hrir(): BrowserHrir;
    /**
     * Generates a deterministic built-in mono test signal at the dataset sample rate.
     *
     * Supported kinds are `pink-noise` and `click`.
     *
     * # Errors
     *
     * Returns a JavaScript error for an unknown kind, invalid duration, or before dataset loading.
     */
    generate_test_signal(kind: string, duration_seconds: number): Float32Array;
    /**
     * Initializes the Rust/wgpu scene on the supplied browser canvas.
     *
     * # Errors
     *
     * Returns a JavaScript error when no compatible WebGPU adapter/device or canvas surface is
     * available, or when GPU initialization fails.
     */
    initialize_renderer(canvas: HTMLCanvasElement): Promise<void>;
    is_hrtf_loaded(): boolean;
    /**
     * Parses and retains one complete runtime HRTF dataset.
     *
     * # Errors
     *
     * Returns a JavaScript error if the dataset bytes fail native validation.
     */
    load_hrtf(bytes: Uint8Array): void;
    /**
     * Returns the number of measured HRIR directions in the loaded dataset.
     *
     * # Errors
     *
     * Returns a JavaScript error before a dataset has been loaded.
     */
    measurement_count(): number;
    /**
     * Applies a combined orbit/pinch gesture with one GPU redraw.
     *
     * # Errors
     *
     * Returns a JavaScript error for non-finite deltas, before renderer initialization, or if
     * drawing fails.
     */
    navigate_camera(delta_x: number, delta_y: number, wheel_delta: number): void;
    constructor();
    /**
     * Orbits the camera by a browser pointer delta in CSS pixels.
     *
     * # Errors
     *
     * Returns a JavaScript error before renderer initialization or if drawing fails.
     */
    orbit_camera(delta_x: number, delta_y: number): void;
    /**
     * Interpolates the current HRIR and renders a complete mono buffer to stereo.
     *
     * Input samples must be at [`Self::sample_rate`].
     *
     * # Errors
     *
     * Returns a JavaScript error before dataset loading or for invalid/empty samples.
     */
    process_audio(mono_samples: Float32Array): ProcessedAudio;
    /**
     * Reports the active wgpu browser backend for diagnostics.
     *
     * # Errors
     *
     * Returns a JavaScript error before renderer initialization.
     */
    renderer_backend(): string;
    /**
     * Resizes the GPU surface to match its CSS dimensions and device pixel ratio.
     *
     * # Errors
     *
     * Returns a JavaScript error before renderer initialization or if drawing fails.
     */
    resize_renderer(css_width: number, css_height: number, device_pixel_ratio: number): void;
    /**
     * Returns the processing sample rate from the loaded dataset.
     *
     * # Errors
     *
     * Returns a JavaScript error before a dataset has been loaded.
     */
    sample_rate(): number;
    /**
     * Casts a camera ray through a CSS-space canvas point and selects the source sphere hit.
     *
     * # Errors
     *
     * Returns a JavaScript error for invalid canvas dimensions, a missed sphere, renderer state,
     * or interpolation failure.
     */
    select_canvas_position(x: number, y: number, css_width: number, css_height: number): SelectionInfo;
    /**
     * Returns the current direction and interpolation diagnostics.
     *
     * # Errors
     *
     * Returns a JavaScript error before dataset loading or if interpolation fails.
     */
    selection_info(): SelectionInfo;
    /**
     * Applies a canonical camera view without changing the physical source direction.
     *
     * Supported presets are `front`, `back`, `left`, `right`, `top`, `bottom`, and `reset`.
     *
     * # Errors
     *
     * Returns a JavaScript error for an unknown preset, before renderer initialization, or if
     * drawing fails.
     */
    set_camera_preset(preset: string): void;
    /**
     * Updates the physical source direction without regard to camera state.
     *
     * # Errors
     *
     * Returns a JavaScript error for non-finite angles or before dataset loading.
     */
    set_direction(azimuth_degrees: number, elevation_degrees: number): SelectionInfo;
    /**
     * Shows or hides one independently rendered scene layer.
     *
     * Supported layers are `measurements`, `grid`, `axes`, and `contributor-guides`.
     * The head, selected source, and contributor markers always remain visible.
     *
     * # Errors
     *
     * Returns a JavaScript error for an unknown layer, before renderer initialization, or if
     * drawing fails.
     */
    set_display_layer(layer: string, visible: boolean): void;
    /**
     * Selects the HRIR interpolation strategy and refreshes its diagnostics.
     *
     * Supported methods are `nearest-neighbor`, `nearest-three`, `time-aligned-three`,
     * `spherical-triangle`, `time-aligned-spherical-triangle`, and `minimum-phase`.
     *
     * # Errors
     *
     * Returns a JavaScript error for an unknown method, before dataset loading, or if rendering
     * the updated contributors fails.
     */
    set_interpolation_method(method: string): SelectionInfo;
    /**
     * Moves the physical source to a canonical head-relative direction.
     *
     * Supported presets are `front`, `back`, `left`, `right`, `top`, and `bottom`.
     *
     * # Errors
     *
     * Returns a JavaScript error for an unknown preset or before dataset loading.
     */
    set_source_preset(preset: string): SelectionInfo;
    /**
     * Zooms the orbit camera by a browser wheel delta.
     *
     * # Errors
     *
     * Returns a JavaScript error before renderer initialization or if drawing fails.
     */
    zoom_camera(wheel_delta: number): void;
}

/**
 * Interpolated HRIR snapshot used to retarget the real-time audio worklet.
 */
export class BrowserHrir {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    readonly left: Float32Array;
    readonly right: Float32Array;
}

/**
 * Complete rendered stereo buffers returned in one WASM call.
 */
export class ProcessedAudio {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    readonly applied_gain: number;
    readonly left: Float32Array;
    readonly right: Float32Array;
}

/**
 * Browser-facing direction and contributor diagnostics.
 */
export class SelectionInfo {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    readonly azimuth_degrees: number;
    readonly contributor_summary: string;
    readonly elevation_degrees: number;
    readonly x: number;
    readonly y: number;
    readonly z: number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_binauralapp_free: (a: number, b: number) => void;
    readonly __wbg_browserhrir_free: (a: number, b: number) => void;
    readonly __wbg_processedaudio_free: (a: number, b: number) => void;
    readonly __wbg_selectioninfo_free: (a: number, b: number) => void;
    readonly binauralapp_current_hrir: (a: number) => [number, number, number];
    readonly binauralapp_generate_test_signal: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly binauralapp_initialize_renderer: (a: number, b: any) => any;
    readonly binauralapp_is_hrtf_loaded: (a: number) => number;
    readonly binauralapp_load_hrtf: (a: number, b: number, c: number) => [number, number];
    readonly binauralapp_measurement_count: (a: number) => [number, number, number];
    readonly binauralapp_navigate_camera: (a: number, b: number, c: number, d: number) => [number, number];
    readonly binauralapp_new: () => number;
    readonly binauralapp_orbit_camera: (a: number, b: number, c: number) => [number, number];
    readonly binauralapp_process_audio: (a: number, b: number, c: number) => [number, number, number];
    readonly binauralapp_renderer_backend: (a: number) => [number, number, number, number];
    readonly binauralapp_resize_renderer: (a: number, b: number, c: number, d: number) => [number, number];
    readonly binauralapp_sample_rate: (a: number) => [number, number, number];
    readonly binauralapp_select_canvas_position: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly binauralapp_selection_info: (a: number) => [number, number, number];
    readonly binauralapp_set_camera_preset: (a: number, b: number, c: number) => [number, number];
    readonly binauralapp_set_direction: (a: number, b: number, c: number) => [number, number, number];
    readonly binauralapp_set_display_layer: (a: number, b: number, c: number, d: number) => [number, number];
    readonly binauralapp_set_interpolation_method: (a: number, b: number, c: number) => [number, number, number];
    readonly binauralapp_set_source_preset: (a: number, b: number, c: number) => [number, number, number];
    readonly binauralapp_zoom_camera: (a: number, b: number) => [number, number];
    readonly browserhrir_left: (a: number) => [number, number];
    readonly browserhrir_right: (a: number) => [number, number];
    readonly processedaudio_applied_gain: (a: number) => number;
    readonly processedaudio_left: (a: number) => [number, number];
    readonly processedaudio_right: (a: number) => [number, number];
    readonly selectioninfo_azimuth_degrees: (a: number) => number;
    readonly selectioninfo_contributor_summary: (a: number) => [number, number];
    readonly selectioninfo_elevation_degrees: (a: number) => number;
    readonly selectioninfo_x: (a: number) => number;
    readonly selectioninfo_y: (a: number) => number;
    readonly selectioninfo_z: (a: number) => number;
    readonly wasm_bindgen_196a8f8e05f9fa6c___convert__closures_____invoke___js_sys_762fa09176666909___Function_fn_wasm_bindgen_196a8f8e05f9fa6c___JsValue_____wasm_bindgen_196a8f8e05f9fa6c___sys__Undefined___js_sys_762fa09176666909___Function_fn_wasm_bindgen_196a8f8e05f9fa6c___JsValue_____wasm_bindgen_196a8f8e05f9fa6c___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_196a8f8e05f9fa6c___convert__closures_____invoke___wasm_bindgen_196a8f8e05f9fa6c___JsValue__core_f0fd674eaa06beef___result__Result_____wasm_bindgen_196a8f8e05f9fa6c___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_196a8f8e05f9fa6c___convert__closures_____invoke___wasm_bindgen_196a8f8e05f9fa6c___sys__JsNullable_wgpu_63018cc362eee2b0___backend__webgpu__webgpu_sys__gen_GpuError__GpuError___core_f0fd674eaa06beef___result__Result_____wasm_bindgen_196a8f8e05f9fa6c___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_196a8f8e05f9fa6c___convert__closures_____invoke___wasm_bindgen_196a8f8e05f9fa6c___sys__JsNullable_wgpu_63018cc362eee2b0___backend__webgpu__webgpu_sys__gen_GpuError__GpuError___core_f0fd674eaa06beef___result__Result_____wasm_bindgen_196a8f8e05f9fa6c___JsError___true__35: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_196a8f8e05f9fa6c___convert__closures_____invoke___wasm_bindgen_196a8f8e05f9fa6c___sys__JsNullable_wgpu_63018cc362eee2b0___backend__webgpu__webgpu_sys__gen_GpuError__GpuError___core_f0fd674eaa06beef___result__Result_____wasm_bindgen_196a8f8e05f9fa6c___JsError___true__36: (a: number, b: number, c: any) => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
