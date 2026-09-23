// The server prepends wasm-bindgen's no-modules glue to this processor body. Keeping the
// registration in one response avoids inconsistent imported-module handling across worklets.
const { initSync, RealtimeAudioProcessor } = wasm_bindgen;

class BinauralHrtfProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.processor = undefined;
    this.pendingHrir = undefined;
    this.failed = false;
    const initial = options.processorOptions;

    this.port.onmessage = (event) => {
      if (event.data?.type !== "hrir") return;
      if (this.processor) {
        this.setHrir(event.data.left, event.data.right);
      } else {
        this.pendingHrir = event.data;
      }
    };

    try {
      this.wasm = initSync({ module: initial.wasmModule });
      this.processor = new RealtimeAudioProcessor(
        initial.left,
        initial.right,
        initial.crossfadeSamples,
      );
      if (!this.processor.is_valid()) {
        throw new Error("The initial real-time HRIR is invalid");
      }
      this.blockSize = this.processor.block_size();
      this.inputPointer = this.processor.input_ptr();
      this.leftOutputPointer = this.processor.left_output_ptr();
      this.rightOutputPointer = this.processor.right_output_ptr();
      this.refreshMemoryViews();
      if (this.pendingHrir) {
        this.setHrir(this.pendingHrir.left, this.pendingHrir.right);
        this.pendingHrir = undefined;
      }
      this.port.postMessage({ type: "ready" });
    } catch (error) {
      this.reportError(error);
    }
  }

  setHrir(left, right) {
    try {
      if (!this.processor.set_hrir(left, right)) {
        throw new Error("A real-time HRIR update was invalid");
      }
    } catch (error) {
      this.reportError(error);
    }
  }

  reportError(error) {
    this.failed = true;
    this.port.postMessage({
      type: "error",
      message: error instanceof Error ? error.message : String(error),
    });
  }

  refreshMemoryViews() {
    const buffer = this.wasm.memory.buffer;
    if (this.inputView?.buffer === buffer) return;
    this.inputView = new Float32Array(buffer, this.inputPointer, this.blockSize);
    this.leftOutputView = new Float32Array(
      buffer,
      this.leftOutputPointer,
      this.blockSize,
    );
    this.rightOutputView = new Float32Array(
      buffer,
      this.rightOutputPointer,
      this.blockSize,
    );
  }

  process(inputs, outputs) {
    const output = outputs[0];
    const outputLeft = output?.[0];
    const outputRight = output?.[1];
    if (!outputLeft || !outputRight) return true;
    outputLeft.fill(0);
    outputRight.fill(0);

    const input = inputs[0]?.[0];
    if (!this.processor || this.failed || !input) return true;

    try {
      if (input.length !== this.blockSize) {
        if (!this.processor.set_block_size(input.length)) {
          throw new Error(`Invalid Web Audio render quantum: ${input.length} samples`);
        }
        this.blockSize = this.processor.block_size();
        this.inputPointer = this.processor.input_ptr();
        this.leftOutputPointer = this.processor.left_output_ptr();
        this.rightOutputPointer = this.processor.right_output_ptr();
        this.inputView = undefined;
        this.leftOutputView = undefined;
        this.rightOutputView = undefined;
      }
      this.refreshMemoryViews();
      this.inputView.set(input);
      if (!this.processor.process_queued_block()) {
        throw new Error("A real-time input block was invalid");
      }
      outputLeft.set(this.leftOutputView);
      outputRight.set(this.rightOutputView);
    } catch (error) {
      this.reportError(error);
    }
    return true;
  }
}

registerProcessor("binaural-hrtf-processor", BinauralHrtfProcessor);
