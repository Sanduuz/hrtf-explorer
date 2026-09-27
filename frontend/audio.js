/** Browser-only local-media streaming and device playback. */
const MIN_VOLUME_DECIBELS = -60;
const MAX_VOLUME_DECIBELS = 12;

export class BrowserAudio {
  constructor() {
    this.context = undefined;
    this.outputGain = undefined;
    this.playingSource = undefined;
    this.playingMedia = undefined;
    this.activeWorklet = undefined;
    this.mediaEndTimer = undefined;
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
    const media = this.playingMedia;
    const worklet = this.activeWorklet;
    if (this.mediaEndTimer !== undefined) clearTimeout(this.mediaEndTimer);
    this.playingSource = undefined;
    this.playingMedia = undefined;
    this.activeWorklet = undefined;
    this.mediaEndTimer = undefined;
    this.playbackStartedAt = undefined;
    this.playbackDuration = undefined;
    this.playbackLoops = false;
    this.playbackPaused = false;
    if (media) {
      media.pause();
      media.onended = null;
      media.onerror = null;
      try {
        media.currentTime = 0;
      } catch {
        // A media element without metadata cannot seek, but can still be disconnected safely.
      }
    } else if (source) {
      try {
        source.stop();
      } catch (error) {
        if (error?.name !== "InvalidStateError") throw error;
      }
    }
    if (source) source.disconnect();
    if (worklet) worklet.disconnect();
  }

  setHrir(left, right) {
    if (!(left instanceof Float32Array) || !(right instanceof Float32Array)) {
      throw new Error("HRIR channels must be Float32Array values");
    }
    if (left.length === 0 || left.length !== right.length) {
      throw new Error("HRIR channels must be non-empty and equally sized");
    }
    this.deviceHrir = mapHeadRelativeHrirToHeadphones(left, right);
    if (this.activeWorklet) {
      this.activeWorklet.port.postMessage({
        type: "hrir",
        left: this.deviceHrir.left,
        right: this.deviceHrir.right,
      });
    }
  }

  async createMediaAsset(file, maximumFileBytes) {
    validateAudioFile(file, maximumFileBytes);
    const url = URL.createObjectURL(file);
    const media = new Audio();
    media.preload = "metadata";
    media.src = url;
    try {
      await waitForMediaMetadata(media);
      validateMediaDuration(media.duration);
      return {
        duration: media.duration,
        element: media,
        fileName: file.name || "Local audio",
        released: false,
        sourceNode: undefined,
        url,
      };
    } catch (error) {
      media.removeAttribute("src");
      media.load();
      URL.revokeObjectURL(url);
      throw error;
    }
  }

  releaseMediaAsset(asset) {
    if (!asset || asset.released) return;
    asset.released = true;
    if (this.playingMedia === asset.element) this.stop();
    asset.element.pause();
    asset.sourceNode?.disconnect();
    asset.element.removeAttribute("src");
    asset.element.load();
    URL.revokeObjectURL(asset.url);
  }

  async playStereo(left, right, sampleRate, onEnded) {
    if (left.length !== right.length || left.length === 0) {
      throw new Error("Rendered stereo buffers have invalid dimensions");
    }
    this.stop();
    const context = this.getContext();
    await context.resume();
    const buffer = context.createBuffer(2, left.length, sampleRate);
    const playback = mapHeadRelativeHrirToHeadphones(left, right);
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
    const worklet = await this.createSpatialWorklet(context, onError);

    const tailLength = loop ? 0 : this.deviceHrir.left.length - 1;
    const buffer = context.createBuffer(1, mono.length + tailLength, sampleRate);
    buffer.copyToChannel(mono, 0);
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.loop = loop;
    source.connect(worklet);
    worklet.connect(this.outputGain);
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

  async playSpatializedMedia(
    asset,
    loop,
    onEnded,
    onError,
    startOffsetSeconds = 0,
  ) {
    if (!asset?.element || asset.released || !Number.isFinite(asset.duration)) {
      throw new Error("Choose a valid local streaming audio file");
    }
    if (!this.deviceHrir) {
      throw new Error("Select a source direction before starting playback");
    }
    validatePlaybackOffset(startOffsetSeconds, asset.duration);
    this.stop();
    const context = this.getContext();
    await context.resume();
    const worklet = await this.createSpatialWorklet(context, onError);
    asset.sourceNode ??= context.createMediaElementSource(asset.element);
    const source = asset.sourceNode;
    const media = asset.element;
    media.loop = loop;
    media.currentTime = startOffsetSeconds;
    source.connect(worklet);
    worklet.connect(this.outputGain);

    media.onended = () => {
      if (this.playingMedia !== media) return;
      const tailMilliseconds = Math.ceil(
        (this.deviceHrir.left.length - 1) / context.sampleRate * 1000,
      );
      this.mediaEndTimer = setTimeout(() => {
        if (this.playingMedia !== media) return;
        this.playingSource = undefined;
        this.playingMedia = undefined;
        this.activeWorklet = undefined;
        this.mediaEndTimer = undefined;
        this.playbackStartedAt = undefined;
        this.playbackPaused = false;
        media.onended = null;
        media.onerror = null;
        source.disconnect();
        worklet.disconnect();
        onEnded();
      }, tailMilliseconds);
    };
    media.onerror = () => onError(new Error(mediaErrorMessage(media)));

    this.playingSource = source;
    this.playingMedia = media;
    this.activeWorklet = worklet;
    this.playbackStartedAt = undefined;
    this.playbackDuration = asset.duration;
    this.playbackLoops = loop;
    this.playbackPaused = false;
    try {
      await media.play();
    } catch (error) {
      if (this.playingMedia === media) this.stop();
      throw new Error(`Could not start streaming playback: ${errorMessage(error)}`);
    }
  }

  async pausePlayback() {
    if (!this.playingSource || this.playbackPaused) return false;
    if (this.playingMedia) {
      this.playingMedia.pause();
    } else {
      await this.context.suspend();
    }
    this.playbackPaused = true;
    return true;
  }

  async resumePlayback() {
    if (!this.playingSource || !this.playbackPaused) return false;
    if (this.playingMedia) {
      await this.playingMedia.play();
    } else {
      await this.context.resume();
    }
    this.playbackPaused = false;
    return true;
  }

  isPlaybackPaused() {
    return this.playbackPaused;
  }

  playbackState() {
    if (this.playingMedia && this.playbackDuration !== undefined) {
      return {
        position: Math.min(this.playingMedia.currentTime, this.playbackDuration),
        duration: this.playbackDuration,
        loop: this.playbackLoops,
        paused: this.playbackPaused,
      };
    }
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

  async createSpatialWorklet(context, onError) {
    const workletWasmModule = await this.loadWorkletModule(context);
    const crossfadeSamples = Math.max(1, Math.round(context.sampleRate * 0.03));
    const worklet = new AudioWorkletNode(context, "binaural-hrtf-processor", {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [2],
      channelCount: 1,
      channelCountMode: "explicit",
      channelInterpretation: "speakers",
      processorOptions: {
        wasmModule: workletWasmModule,
        left: this.deviceHrir.left,
        right: this.deviceHrir.right,
        crossfadeSamples,
      },
    });
    await waitForWorkletReady(worklet);
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
    return worklet;
  }

  async loadWorkletModule(context) {
    if (!context.audioWorklet) {
      throw new Error("This browser does not support AudioWorklet");
    }
    this.workletModulePromise ??= Promise.all([
      context.audioWorklet.addModule("/audio-worklet.js?v=20260927-stream1"),
      fetch("/pkg/binaural_audio_worklet_nomodule_bg.wasm?v=20260927-stream1").then(async (response) => {
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

export function validateMediaDuration(duration) {
  if (!Number.isFinite(duration) || duration <= 0) {
    throw new Error("The browser could not determine a valid audio duration");
  }
}

function waitForMediaMetadata(media) {
  return new Promise((resolve, reject) => {
    const cleanup = () => {
      clearTimeout(timeout);
      media.removeEventListener("loadedmetadata", handleMetadata);
      media.removeEventListener("error", handleError);
    };
    const handleMetadata = () => {
      cleanup();
      resolve();
    };
    const handleError = () => {
      cleanup();
      reject(new Error(mediaErrorMessage(media)));
    };
    const timeout = setTimeout(() => {
      cleanup();
      reject(new Error("Timed out while reading local audio metadata"));
    }, 15_000);
    media.addEventListener("loadedmetadata", handleMetadata);
    media.addEventListener("error", handleError);
    if (media.readyState >= 1) {
      handleMetadata();
    } else {
      media.load();
    }
  });
}

function mediaErrorMessage(media) {
  const messages = {
    1: "Loading the local audio file was aborted",
    2: "A browser error interrupted local audio loading",
    3: "The browser could not decode this audio file",
    4: "The browser does not support this audio format",
  };
  return messages[media.error?.code] ?? "The browser could not load this audio file";
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
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
 * The renderer handles the face-on visual reflection. HRIR channels are already
 * anatomical/head-relative and therefore map directly to the matching headphone.
 */
export function mapHeadRelativeHrirToHeadphones(left, right) {
  return { left, right };
}
