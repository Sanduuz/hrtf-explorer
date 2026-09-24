/** Browser-only audio decoding, resampling, and device playback. */
const MIN_VOLUME_DECIBELS = -60;
const MAX_VOLUME_DECIBELS = 12;

export class BrowserAudio {
  constructor() {
    this.context = undefined;
    this.outputGain = undefined;
    this.playingSource = undefined;
    this.activeWorklet = undefined;
    this.workletModulePromise = undefined;
    this.processingSampleRate = undefined;
    this.deviceHrir = undefined;
    this.volumeDecibels = -6;
    this.playbackStartedAt = undefined;
    this.playbackDuration = undefined;
    this.playbackLoops = false;
    this.playbackPaused = false;
  }

  configureSampleRate(sampleRate) {
    if (!Number.isInteger(sampleRate) || sampleRate <= 0) {
      throw new Error("Audio processing sample rate must be a positive integer");
    }
    if (this.context && this.context.sampleRate !== sampleRate) {
      throw new Error("AudioContext was already created at a different sample rate");
    }
    this.processingSampleRate = sampleRate;
  }

  getContext() {
    if (!this.context) {
      if (!this.processingSampleRate) {
        throw new Error("Configure the HRTF sample rate before creating browser audio");
      }
      this.context = new AudioContext({ sampleRate: this.processingSampleRate });
      if (this.context.sampleRate !== this.processingSampleRate) {
        throw new Error(
          `Browser created a ${this.context.sampleRate} Hz AudioContext; ${this.processingSampleRate} Hz is required`,
        );
      }
      this.outputGain = this.context.createGain();
      this.outputGain.gain.value = decibelsToGain(this.volumeDecibels);
      this.outputGain.connect(this.context.destination);
    }
    return this.context;
  }

  setVolumeDecibels(decibels) {
    if (
      !Number.isFinite(decibels)
      || decibels < MIN_VOLUME_DECIBELS
      || decibels > MAX_VOLUME_DECIBELS
    ) {
      throw new Error(
        `Volume must be between ${MIN_VOLUME_DECIBELS} dB and +${MAX_VOLUME_DECIBELS} dB`,
      );
    }
    this.volumeDecibels = decibels;
    if (this.outputGain) {
      this.outputGain.gain.setValueAtTime(
        decibelsToGain(decibels),
        this.context.currentTime,
      );
    }
  }

  stop() {
    const source = this.playingSource;
    const worklet = this.activeWorklet;
    this.playingSource = undefined;
    this.activeWorklet = undefined;
    this.playbackStartedAt = undefined;
    this.playbackDuration = undefined;
    this.playbackLoops = false;
    this.playbackPaused = false;
    if (source) {
      try {
        source.stop();
      } catch (error) {
        if (error?.name !== "InvalidStateError") throw error;
      }
      source.disconnect();
    }
    if (worklet) worklet.disconnect();
  }

  setHrir(left, right) {
    if (!(left instanceof Float32Array) || !(right instanceof Float32Array)) {
      throw new Error("HRIR channels must be Float32Array values");
    }
    if (left.length === 0 || left.length !== right.length) {
      throw new Error("HRIR channels must be non-empty and equally sized");
    }
    this.deviceHrir = mapFrontFacingSceneToHeadphones(left, right);
    if (this.activeWorklet) {
      this.activeWorklet.port.postMessage({
        type: "hrir",
        left: this.deviceHrir.left,
        right: this.deviceHrir.right,
      });
    }
  }

  async decodeFile(file, targetRate, maximumDurationSeconds, maximumFileBytes) {
    validateAudioFile(file, maximumFileBytes);
    const context = this.getContext();
    const decoded = await context.decodeAudioData(await file.arrayBuffer());
    if (!Number.isFinite(decoded.duration) || decoded.duration <= 0) {
      throw new Error("Decoded audio is empty");
    }
    if (decoded.duration > maximumDurationSeconds) {
      throw new Error(
        `Audio is ${decoded.duration.toFixed(1)} seconds; the current limit is ${maximumDurationSeconds} seconds`,
      );
    }
    const channelCount = decoded.numberOfChannels;
    const decodedRate = decoded.sampleRate;
    const mono = downmixToMono(decoded);
    const samples = await resampleMono(mono, decodedRate, targetRate);
    return { samples, channelCount, decodedRate };
  }

  async playStereo(left, right, sampleRate, onEnded) {
    if (left.length !== right.length || left.length === 0) {
      throw new Error("Rendered stereo buffers have invalid dimensions");
    }
    this.stop();
    const context = this.getContext();
    await context.resume();
    const buffer = context.createBuffer(2, left.length, sampleRate);
    const playback = mapFrontFacingSceneToHeadphones(left, right);
    buffer.copyToChannel(playback.left, 0);
    buffer.copyToChannel(playback.right, 1);

    const source = context.createBufferSource();
    source.buffer = buffer;
    source.connect(this.outputGain);
    source.addEventListener("ended", () => {
      if (this.playingSource === source) {
        this.playingSource = undefined;
        onEnded();
      }
    });
    this.playingSource = source;
    source.start();
  }

  async playSpatializedMono(
    mono,
    sampleRate,
    loop,
    onEnded,
    onError,
    startOffsetSeconds = 0,
  ) {
    if (!(mono instanceof Float32Array) || mono.length === 0) {
      throw new Error("Real-time playback requires a non-empty mono Float32Array");
    }
    if (!this.deviceHrir) {
      throw new Error("Select a source direction before starting playback");
    }
    const sourceDuration = mono.length / sampleRate;
    validatePlaybackOffset(startOffsetSeconds, sourceDuration);
    this.stop();
    const context = this.getContext();
    if (sampleRate !== context.sampleRate) {
      throw new Error(`Audio is ${sampleRate} Hz but the processing context is ${context.sampleRate} Hz`);
    }
    await context.resume();
    const workletWasmModule = await this.loadWorkletModule(context);

    const crossfadeSamples = Math.max(1, Math.round(context.sampleRate * 0.03));
    const worklet = new AudioWorkletNode(context, "binaural-hrtf-processor", {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [2],
      channelCount: 1,
      channelCountMode: "explicit",
      processorOptions: {
        wasmModule: workletWasmModule,
        left: this.deviceHrir.left,
        right: this.deviceHrir.right,
        crossfadeSamples,
      },
    });
    await waitForWorkletReady(worklet);

    const tailLength = loop ? 0 : this.deviceHrir.left.length - 1;
    const buffer = context.createBuffer(1, mono.length + tailLength, sampleRate);
    buffer.copyToChannel(mono, 0);
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.loop = loop;
    source.connect(worklet);
    worklet.connect(this.outputGain);
    let processingFailed = false;
    const reportProcessingError = (error) => {
      if (processingFailed) return;
      processingFailed = true;
      onError(error);
    };
    worklet.port.addEventListener("message", (event) => {
      if (event.data?.type === "error") {
        reportProcessingError(new Error(event.data.message));
      }
    });
    worklet.addEventListener("processorerror", () => {
      reportProcessingError(new Error("The AudioWorklet processor stopped unexpectedly"));
    });
    worklet.port.start();
    source.addEventListener("ended", () => {
      if (this.playingSource === source) {
        this.playingSource = undefined;
        this.activeWorklet = undefined;
        this.playbackStartedAt = undefined;
        this.playbackPaused = false;
        source.disconnect();
        worklet.disconnect();
        onEnded();
      }
    });
    this.playingSource = source;
    this.activeWorklet = worklet;
    this.playbackStartedAt = context.currentTime - startOffsetSeconds;
    this.playbackDuration = sourceDuration;
    this.playbackLoops = loop;
    this.playbackPaused = false;
    source.start(0, startOffsetSeconds);
  }

  async pausePlayback() {
    if (!this.playingSource || this.playbackPaused) return false;
    await this.context.suspend();
    this.playbackPaused = true;
    return true;
  }

  async resumePlayback() {
    if (!this.playingSource || !this.playbackPaused) return false;
    await this.context.resume();
    this.playbackPaused = false;
    return true;
  }

  isPlaybackPaused() {
    return this.playbackPaused;
  }

  playbackState() {
    if (
      !this.playingSource
      || this.playbackStartedAt === undefined
      || this.playbackDuration === undefined
    ) {
      return undefined;
    }
    const elapsed = Math.max(0, this.context.currentTime - this.playbackStartedAt);
    return {
      position: calculatePlaybackPosition(elapsed, this.playbackDuration, this.playbackLoops),
      duration: this.playbackDuration,
      loop: this.playbackLoops,
      paused: this.playbackPaused,
    };
  }

  async loadWorkletModule(context) {
    if (!context.audioWorklet) {
      throw new Error("This browser does not support AudioWorklet");
    }
    this.workletModulePromise ??= Promise.all([
      context.audioWorklet.addModule("/audio-worklet.js?v=20260924-fft3"),
      fetch("/pkg/binaural_audio_worklet_nomodule_bg.wasm?v=20260924-fft3").then(async (response) => {
        if (!response.ok) {
          throw new Error(`Audio DSP WASM request failed with HTTP ${response.status}`);
        }
        if (WebAssembly.compileStreaming) {
          return WebAssembly.compileStreaming(Promise.resolve(response));
        }
        return WebAssembly.compile(await response.arrayBuffer());
      }),
    ]).then(([, wasmModule]) => wasmModule);
    return this.workletModulePromise;
  }
}

export function validateAudioFile(file, maximumFileBytes) {
  if (!file || typeof file.arrayBuffer !== "function") {
    throw new Error("Choose a valid local audio file");
  }
  if (!Number.isSafeInteger(maximumFileBytes) || maximumFileBytes <= 0) {
    throw new Error("Maximum audio file size must be a positive integer");
  }
  if (!Number.isSafeInteger(file.size) || file.size <= 0) {
    throw new Error("The selected audio file is empty");
  }
  if (file.size > maximumFileBytes) {
    throw new Error(
      `Audio file is ${formatMebibytes(file.size)} MiB; the current limit is ${formatMebibytes(maximumFileBytes)} MiB`,
    );
  }
}

function formatMebibytes(bytes) {
  return (bytes / (1024 * 1024)).toFixed(1);
}

function waitForWorkletReady(worklet) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("AudioWorklet initialization timed out")), 5000);
    const handleMessage = (event) => {
      if (event.data?.type === "ready") {
        clearTimeout(timeout);
        worklet.port.removeEventListener("message", handleMessage);
        resolve();
      } else if (event.data?.type === "error") {
        clearTimeout(timeout);
        worklet.port.removeEventListener("message", handleMessage);
        reject(new Error(event.data.message));
      }
    };
    worklet.port.addEventListener("message", handleMessage);
    worklet.port.start();
  });
}

export function decibelsToGain(decibels) {
  return 10 ** (decibels / 20);
}

export function calculatePlaybackPosition(elapsed, duration, loop) {
  if (!Number.isFinite(elapsed) || elapsed < 0 || !Number.isFinite(duration) || duration <= 0) {
    throw new Error("Playback timing values must be finite and positive");
  }
  return loop ? elapsed % duration : Math.min(elapsed, duration);
}

export function validatePlaybackOffset(offset, duration) {
  if (
    !Number.isFinite(offset)
    || offset < 0
    || !Number.isFinite(duration)
    || duration <= 0
    || offset >= duration
  ) {
    throw new Error("Playback offset must be within the source duration");
  }
}

/**
 * The visual head faces the viewer, making its anatomical lateral axis appear
 * mirrored. Keep the dataset and Rust DSP head-relative, and adapt only the
 * final device channels so a marker on the visible right is heard on the right.
 */
export function mapFrontFacingSceneToHeadphones(left, right) {
  return { left: right, right: left };
}

export function downmixToMono(buffer) {
  if (buffer.numberOfChannels < 1 || buffer.length < 1) {
    throw new Error("Decoded audio is empty");
  }
  const mono = new Float32Array(buffer.length);
  const gain = 1 / buffer.numberOfChannels;
  for (let channel = 0; channel < buffer.numberOfChannels; channel += 1) {
    const samples = buffer.getChannelData(channel);
    for (let index = 0; index < samples.length; index += 1) {
      mono[index] += samples[index] * gain;
    }
  }
  return mono;
}

export async function resampleMono(samples, sourceRate, targetRate) {
  if (sourceRate === targetRate) return samples;
  const targetLength = Math.max(1, Math.round(samples.length * targetRate / sourceRate));
  const offline = new OfflineAudioContext(1, targetLength, targetRate);
  const input = offline.createBuffer(1, samples.length, sourceRate);
  input.copyToChannel(samples, 0);
  const source = offline.createBufferSource();
  source.buffer = input;
  source.connect(offline.destination);
  source.start();
  const rendered = await offline.startRendering();
  return rendered.getChannelData(0).slice();
}
