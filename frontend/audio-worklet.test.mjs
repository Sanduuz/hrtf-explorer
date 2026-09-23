import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const glue = readFileSync(new URL(
  "./pkg/binaural_audio_worklet_nomodule.js",
  import.meta.url,
), "utf8");
const context = vm.createContext({
  console,
  FinalizationRegistry,
  Float32Array,
  Symbol,
  TextDecoder,
  Uint8Array,
  WebAssembly,
});
vm.runInContext(`${glue}\nglobalThis.workletWasm = wasm_bindgen;`, context);
const { initSync, RealtimeAudioProcessor } = context.workletWasm;

const wasmBytes = readFileSync(new URL(
  "./pkg/binaural_audio_worklet_nomodule_bg.wasm",
  import.meta.url,
));
const wasm = initSync(new WebAssembly.Module(wasmBytes));

function processorViews(processor) {
  const length = processor.block_size();
  return {
    input: new Float32Array(wasm.memory.buffer, processor.input_ptr(), length),
    left: new Float32Array(wasm.memory.buffer, processor.left_output_ptr(), length),
    right: new Float32Array(wasm.memory.buffer, processor.right_output_ptr(), length),
  };
}

test("generated worklet WASM preserves history between render quanta", () => {
  const processor = new RealtimeAudioProcessor(
    new Float32Array([1, 0.5]),
    new Float32Array([2, 0.25]),
    4,
  );

  let views = processorViews(processor);
  views.input.fill(0);
  views.input[views.input.length - 1] = 1;
  assert.equal(processor.process_queued_block(), true);
  assert.equal(views.left[views.left.length - 1], 1);
  assert.equal(views.right[views.right.length - 1], 2);

  views.input.fill(0);
  assert.equal(processor.process_queued_block(), true);
  assert.equal(views.left[0], 0.5);
  assert.equal(views.right[0], 0.25);
  processor.free();
});

test("generated worklet WASM crossfades HRIR updates", () => {
  const processor = new RealtimeAudioProcessor(
    new Float32Array([0]),
    new Float32Array([0]),
    2,
  );
  processor.set_hrir(new Float32Array([2]), new Float32Array([4]));
  const views = processorViews(processor);
  views.input.fill(1);
  assert.equal(processor.process_queued_block(), true);

  assert.deepEqual([...views.left.slice(0, 3)], [1, 2, 2]);
  assert.deepEqual([...views.right.slice(0, 3)], [2, 4, 4]);
  processor.free();
});

test("generated worklet WASM adapts its reusable render-quantum buffers", () => {
  const processor = new RealtimeAudioProcessor(
    new Float32Array([1]),
    new Float32Array([2]),
    4,
  );

  assert.equal(processor.set_block_size(64), true);
  const views = processorViews(processor);
  assert.equal(views.input.length, 64);
  views.input.fill(0);
  views.input[63] = 0.25;
  assert.equal(processor.process_queued_block(), true);
  assert.equal(views.left[63], 0.25);
  assert.equal(views.right[63], 0.5);
  assert.equal(processor.set_block_size(0), false);
  processor.free();
});
