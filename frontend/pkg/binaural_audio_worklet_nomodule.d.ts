declare namespace wasm_bindgen {
    /* tslint:disable */
    /* eslint-disable */

    /**
     * Long-lived real-time DSP state owned by the browser audio rendering thread.
     */
    export class RealtimeAudioProcessor {
        free(): void;
        [Symbol.dispose](): void;
        /**
         * Current Web Audio render quantum size.
         */
        block_size(): number;
        /**
         * Byte offset of the fixed mono input block in WASM linear memory.
         */
        input_ptr(): number;
        /**
         * Reports whether the initial HRIR passed validation.
         */
        is_valid(): boolean;
        /**
         * Byte offset of the fixed left output block in WASM linear memory.
         */
        left_output_ptr(): number;
        /**
         * Creates the processor with an initial device-ordered stereo HRIR.
         */
        constructor(left: Float32Array, right: Float32Array, fade_length_samples: number);
        /**
         * Processes the queued fixed-size input block without allocating.
         */
        process_queued_block(): boolean;
        /**
         * Byte offset of the fixed right output block in WASM linear memory.
         */
        right_output_ptr(): number;
        /**
         * Resizes the reusable input/output blocks for a browser render-quantum change.
         *
         * The normal 128-sample path never calls this after initialization. A browser that changes
         * quantum size pays for allocation once at the transition, then returns to allocation-free
         * block processing.
         */
        set_block_size(block_size: number): boolean;
        /**
         * Begins a smooth transition to a new device-ordered HRIR, returning false on invalid input.
         */
        set_hrir(left: Float32Array, right: Float32Array): boolean;
    }

}
declare type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

declare interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_realtimeaudioprocessor_free: (a: number, b: number) => void;
    readonly realtimeaudioprocessor_block_size: (a: number) => number;
    readonly realtimeaudioprocessor_input_ptr: (a: number) => number;
    readonly realtimeaudioprocessor_is_valid: (a: number) => number;
    readonly realtimeaudioprocessor_left_output_ptr: (a: number) => number;
    readonly realtimeaudioprocessor_new: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly realtimeaudioprocessor_process_queued_block: (a: number) => number;
    readonly realtimeaudioprocessor_right_output_ptr: (a: number) => number;
    readonly realtimeaudioprocessor_set_block_size: (a: number, b: number) => number;
    readonly realtimeaudioprocessor_set_hrir: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_start: () => void;
}

declare type SyncInitInput = BufferSource | WebAssembly.Module;

declare namespace wasm_bindgen {
    /**
     * Instantiates the given `module`, which can either be bytes or
     * a precompiled `WebAssembly.Module`.
     *
     * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
     *
     * @returns {InitOutput}
     */
    export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;
}

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
declare function wasm_bindgen (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
