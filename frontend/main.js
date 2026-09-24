import init, { BinauralApp } from "/pkg/binaural_explorer_web.js?v=20260924-fft2";
import { BrowserAudio } from "/audio.js?v=20260924-fft2";
import { renderHrirPlot } from "/hrir-plot.js?v=20260924-fft2";

const elements = {
  status: document.querySelector("#status"),
  audioPanel: document.querySelector("#audio-source-panel"),
  canvas: document.querySelector("#scene"),
  signal: document.querySelector("#signal"),
  customSignal: document.querySelector("#custom-signal"),
  audioFile: document.querySelector("#audio-file"),
  fileStatus: document.querySelector("#file-status"),
  volume: document.querySelector("#volume"),
  volumeValue: document.querySelector("#volume-value"),
  volumeWarning: document.querySelector("#volume-warning"),
  loop: document.querySelector("#loop"),
  azimuth: document.querySelector("#azimuth"),
  elevation: document.querySelector("#elevation"),
  azimuthNumber: document.querySelector("#azimuth-number"),
  elevationNumber: document.querySelector("#elevation-number"),
  interpolationMethod: document.querySelector("#interpolation-method"),
  debug: document.querySelector("#debug"),
  hrirPlot: {
    leftPath: document.querySelector("#left-hrir-path"),
    rightPath: document.querySelector("#right-hrir-path"),
    length: document.querySelector("#hrir-length"),
    duration: document.querySelector("#hrir-duration"),
  },
  sourcePresets: [...document.querySelectorAll("[data-source-preset]")],
  cameraPresets: [...document.querySelectorAll("[data-camera-preset]")],
  displayLayers: [...document.querySelectorAll("[data-display-layer]")],
  play: document.querySelector("#play"),
  stop: document.querySelector("#stop"),
  playbackProgress: document.querySelector("#playback-progress"),
  playbackTime: document.querySelector("#playback-time"),
};

let app;
const browserAudio = new BrowserAudio();
let rendererReady = false;
let mousePointerState;
const touchPointers = new Map();
let touchGesture;
let touchGestureWasMulti = false;
let pendingSourcePointer;
let sourceFramePending = false;
let uploadedMono;
let fileLoadGeneration = 0;
let playbackProgressFrame;
let playbackDisplayDuration = 10;
let activePlayback;

const MAX_CUSTOM_DURATION_SECONDS = 60;
const MAX_CUSTOM_FILE_BYTES = 50 * 1024 * 1024;

function setEnabled(enabled) {
  for (const element of [
    elements.signal,
    elements.audioFile,
    elements.volume,
    elements.loop,
    elements.azimuth,
    elements.elevation,
    elements.azimuthNumber,
    elements.elevationNumber,
    elements.interpolationMethod,
    elements.play,
    ...elements.sourcePresets,
    ...elements.cameraPresets,
    ...elements.displayLayers,
  ]) {
    element.disabled = !enabled;
  }
}

function updateVolume() {
  const decibels = Number(elements.volume.value);
  browserAudio.setVolumeDecibels(decibels);
  elements.volumeValue.value = `${decibels > 0 ? "+" : ""}${decibels.toFixed(0)} dB`;
  elements.volumeValue.classList.toggle("positive-gain", decibels > 0);
  elements.volumeWarning.hidden = decibels <= 0;
}

function showError(error) {
  console.error(error);
  elements.status.textContent = `Error: ${error instanceof Error ? error.message : String(error)}`;
  elements.status.classList.add("error");
  elements.status.hidden = false;
  setEnabled(false);
}

function showPlaybackError(error) {
  console.error(error);
  stopPlayback();
  elements.status.textContent = `Playback error: ${error instanceof Error ? error.message : String(error)}`;
  elements.status.classList.add("error");
  elements.status.hidden = false;
}

function clearStatus() {
  elements.status.textContent = "";
  elements.status.classList.remove("error");
  elements.status.hidden = true;
}

function applySelection(selection, updateSliders = false, updateAngles = true) {
  const azimuth = selection.azimuth_degrees;
  const elevation = selection.elevation_degrees;
  const x = selection.x;
  const y = selection.y;
  const z = selection.z;
  const contributors = selection.contributor_summary;
  selection.free();

  if (updateSliders) {
    elements.azimuth.value = String(azimuth);
    elements.elevation.value = String(elevation);
  }
  if (updateAngles) {
    elements.azimuthNumber.value = azimuth.toFixed(1);
    elements.elevationNumber.value = elevation.toFixed(1);
  }
  elements.debug.textContent = [
    `XYZ: ${x.toFixed(3)}, ${y.toFixed(3)}, ${z.toFixed(3)}`,
    `Interpolation:\n${contributors}`,
  ].join("\n");
  syncCurrentHrir();
}

function syncCurrentHrir() {
  const hrir = app.current_hrir();
  const left = hrir.left;
  const right = hrir.right;
  hrir.free();
  renderHrirPlot(elements.hrirPlot, left, right, app.sample_rate());
  browserAudio.setHrir(left, right);
}

function updateDirection() {
  applySelection(app.set_direction(
    Number(elements.azimuth.value),
    Number(elements.elevation.value),
  ));
}

function updateDirectionFromNumbers() {
  const azimuth = Number(elements.azimuthNumber.value);
  const elevation = Number(elements.elevationNumber.value);
  if (!Number.isFinite(azimuth) || !Number.isFinite(elevation)) {
    elements.azimuthNumber.value = Number(elements.azimuth.value).toFixed(1);
    elements.elevationNumber.value = Number(elements.elevation.value).toFixed(1);
    return;
  }
  const clampedAzimuth = Math.max(-180, Math.min(180, azimuth));
  const clampedElevation = Math.max(-90, Math.min(90, elevation));
  elements.azimuth.value = String(clampedAzimuth);
  elements.elevation.value = String(clampedElevation);
  updateDirection();
}

function resizeRenderer() {
  if (!rendererReady) return;
  const bounds = elements.canvas.getBoundingClientRect();
  if (bounds.width > 0 && bounds.height > 0) {
    app.resize_renderer(bounds.width, bounds.height, window.devicePixelRatio || 1);
  }
}

function stopPlayback() {
  stopPlaybackProgress();
  browserAudio.stop();
  activePlayback = undefined;
  elements.stop.disabled = true;
  setPlayButton("play");
}

function setPlayButton(mode) {
  const labels = {
    play: "▶ Play",
    pause: "⏸ Pause",
    resume: "▶ Resume",
  };
  elements.play.textContent = labels[mode];
}

function formatPlaybackTime(seconds) {
  const wholeSeconds = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(wholeSeconds / 60);
  return `${minutes}:${String(wholeSeconds % 60).padStart(2, "0")}`;
}

function renderPlaybackProgress(position, duration) {
  playbackDisplayDuration = duration;
  elements.playbackProgress.max = duration;
  elements.playbackProgress.value = position;
  elements.playbackTime.value = `${formatPlaybackTime(position)} / ${formatPlaybackTime(duration)}`;
}

function updatePlaybackProgress() {
  const state = browserAudio.playbackState();
  if (!state) return;
  renderPlaybackProgress(state.position, state.duration);
  playbackProgressFrame = requestAnimationFrame(updatePlaybackProgress);
}

function startPlaybackProgress() {
  if (playbackProgressFrame !== undefined) cancelAnimationFrame(playbackProgressFrame);
  elements.playbackProgress.disabled = false;
  updatePlaybackProgress();
}

function finishPlaybackProgress(duration) {
  if (playbackProgressFrame !== undefined) cancelAnimationFrame(playbackProgressFrame);
  playbackProgressFrame = undefined;
  elements.playbackProgress.disabled = true;
  renderPlaybackProgress(duration, duration);
}

function stopPlaybackProgress() {
  if (playbackProgressFrame !== undefined) cancelAnimationFrame(playbackProgressFrame);
  playbackProgressFrame = undefined;
  elements.playbackProgress.disabled = true;
  renderPlaybackProgress(0, playbackDisplayDuration);
}

function clearUploadedAudio(message, isError = false) {
  uploadedMono = undefined;
  elements.customSignal.disabled = true;
  if (elements.signal.value === "custom") elements.signal.value = "pink-noise";
  elements.fileStatus.textContent = message;
  elements.fileStatus.classList.toggle("file-error", isError);
}

async function loadAudioFile(file) {
  const generation = ++fileLoadGeneration;
  stopPlayback();
  elements.play.disabled = true;
  elements.fileStatus.classList.remove("file-error");
  elements.fileStatus.textContent = `Decoding ${file.name}…`;

  try {
    const processingRate = app.sample_rate();
    const { samples, channelCount } = await browserAudio.decodeFile(
      file,
      processingRate,
      MAX_CUSTOM_DURATION_SECONDS,
      MAX_CUSTOM_FILE_BYTES,
    );
    if (generation !== fileLoadGeneration) return;

    uploadedMono = samples;
    elements.customSignal.disabled = false;
    elements.signal.value = "custom";
    elements.fileStatus.textContent = `${file.name} · ${channelCount} channel${channelCount === 1 ? "" : "s"} → mono · ${(samples.length / processingRate).toFixed(2)} s at ${processingRate} Hz`;
    clearStatus();
  } catch (error) {
    if (generation !== fileLoadGeneration) return;
    console.error(error);
    clearUploadedAudio(
      `Could not load ${file.name}. ${error instanceof Error ? error.message : "The browser may not support this codec."}`,
      true,
    );
    clearStatus();
  } finally {
    if (generation === fileLoadGeneration) elements.play.disabled = false;
  }
}

async function startSpatialPlayback(playback, startOffsetSeconds = 0) {
  await browserAudio.playSpatializedMono(
    playback.mono,
    playback.sampleRate,
    playback.loop,
    () => {
      if (activePlayback !== playback) return;
      finishPlaybackProgress(playback.duration);
      activePlayback = undefined;
      elements.stop.disabled = true;
      setPlayButton("play");
    },
    showPlaybackError,
    startOffsetSeconds,
  );
  startPlaybackProgress();
  elements.stop.disabled = false;
  setPlayButton("pause");
}

async function play() {
  stopPlayback();
  elements.play.disabled = true;
  elements.play.setAttribute("aria-busy", "true");
  clearStatus();

  try {
    let mono;
    if (elements.signal.value === "custom") {
      if (!uploadedMono) throw new Error("Choose and decode a custom audio file first");
      mono = uploadedMono;
    } else {
      const duration = elements.signal.value === "click" ? 0.5 : 10.0;
      mono = app.generate_test_signal(elements.signal.value, duration);
    }
    const sampleRate = app.sample_rate();
    activePlayback = {
      mono,
      sampleRate,
      duration: mono.length / sampleRate,
      loop: elements.loop.checked,
    };
    await startSpatialPlayback(activePlayback);
  } finally {
    elements.play.disabled = false;
    elements.play.removeAttribute("aria-busy");
  }
}

async function seekPlayback(requestedOffset) {
  const playback = activePlayback;
  if (!playback) return;
  const remainPaused = browserAudio.isPlaybackPaused();
  const lastSampleOffset = Math.max(0, playback.duration - 1 / playback.sampleRate);
  const offset = Math.min(Math.max(0, requestedOffset), lastSampleOffset);
  await startSpatialPlayback(playback, offset);
  if (remainPaused) await pausePlayback();
}

async function pausePlayback() {
  if (!activePlayback) return;
  if (playbackProgressFrame !== undefined) cancelAnimationFrame(playbackProgressFrame);
  playbackProgressFrame = undefined;
  await browserAudio.pausePlayback();
  const state = browserAudio.playbackState();
  if (state) renderPlaybackProgress(state.position, state.duration);
  setPlayButton("resume");
}

async function resumePlayback() {
  if (!activePlayback) return;
  await browserAudio.resumePlayback();
  startPlaybackProgress();
  setPlayButton("pause");
}

async function togglePlayback() {
  if (!activePlayback) {
    await play();
  } else if (browserAudio.isPlaybackPaused()) {
    await resumePlayback();
  } else {
    await pausePlayback();
  }
}

async function start() {
  try {
    await init({ module_or_path: "/pkg/binaural_explorer_web_bg.wasm?v=20260924-fft2" });
    app = new BinauralApp();
    const response = await fetch("/assets/mit-kemar.bhrtf");
    if (!response.ok) {
      throw new Error(`HRTF request failed with HTTP ${response.status}`);
    }
    app.load_hrtf(new Uint8Array(await response.arrayBuffer()));
    browserAudio.configureSampleRate(app.sample_rate());
    await app.initialize_renderer(elements.canvas);
    rendererReady = true;
    resizeRenderer();
    const probe = app.process_audio(new Float32Array([1]));
    const probeLength = probe.left.length;
    const probeChannelsMatch = probeLength === probe.right.length;
    probe.free();
    if (probeLength !== 128 || !probeChannelsMatch) {
      throw new Error("Rust/WASM DSP startup check returned invalid stereo buffers");
    }
    updateDirection();
    updateVolume();
    setEnabled(true);
    clearStatus();
  } catch (error) {
    showError(error);
  }
}

elements.azimuth.addEventListener("input", updateDirection);
elements.elevation.addEventListener("input", updateDirection);
elements.azimuthNumber.addEventListener("change", updateDirectionFromNumbers);
elements.elevationNumber.addEventListener("change", updateDirectionFromNumbers);
elements.interpolationMethod.addEventListener("change", () => {
  try {
    const selection = app.set_interpolation_method(elements.interpolationMethod.value);
    applySelection(selection, false, false);
  } catch (error) {
    showError(error);
  }
});
for (const input of [elements.azimuthNumber, elements.elevationNumber]) {
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") input.blur();
  });
}
elements.play.addEventListener("click", () => togglePlayback().catch(showError));
elements.stop.addEventListener("click", () => {
  stopPlayback();
  clearStatus();
});
elements.playbackProgress.addEventListener("pointerdown", () => {
  if (playbackProgressFrame !== undefined) cancelAnimationFrame(playbackProgressFrame);
  playbackProgressFrame = undefined;
});
elements.playbackProgress.addEventListener("input", () => {
  if (!activePlayback) return;
  renderPlaybackProgress(Number(elements.playbackProgress.value), activePlayback.duration);
});
elements.playbackProgress.addEventListener("change", () => {
  seekPlayback(Number(elements.playbackProgress.value)).catch(showPlaybackError);
});
elements.volume.addEventListener("input", updateVolume);
elements.audioFile.addEventListener("change", () => {
  const [file] = elements.audioFile.files;
  if (file) {
    loadAudioFile(file).catch((error) => {
      console.error(error);
      clearUploadedAudio("Unexpected error while loading custom audio.", true);
      elements.play.disabled = false;
    });
  } else {
    fileLoadGeneration += 1;
    clearUploadedAudio("No file selected. Choose or drop browser-supported audio, up to 60 seconds.");
  }
});

let fileDragDepth = 0;

function isFileDrag(event) {
  return [...(event.dataTransfer?.types ?? [])].includes("Files");
}

elements.audioPanel.addEventListener("dragenter", (event) => {
  if (elements.audioFile.disabled || !isFileDrag(event)) return;
  event.preventDefault();
  fileDragDepth += 1;
  elements.audioPanel.classList.add("drop-active");
});
elements.audioPanel.addEventListener("dragover", (event) => {
  if (elements.audioFile.disabled || !isFileDrag(event)) return;
  event.preventDefault();
  event.dataTransfer.dropEffect = "copy";
});
elements.audioPanel.addEventListener("dragleave", () => {
  fileDragDepth = Math.max(0, fileDragDepth - 1);
  if (fileDragDepth === 0) elements.audioPanel.classList.remove("drop-active");
});
elements.audioPanel.addEventListener("drop", (event) => {
  if (elements.audioFile.disabled || !isFileDrag(event)) return;
  event.preventDefault();
  fileDragDepth = 0;
  elements.audioPanel.classList.remove("drop-active");
  const [file] = event.dataTransfer.files;
  if (file) loadAudioFile(file).catch(showPlaybackError);
});

function queueSourceSelection(clientX, clientY) {
  if (!rendererReady) return;
  pendingSourcePointer = { x: clientX, y: clientY };
  if (sourceFramePending) return;
  sourceFramePending = true;
  requestAnimationFrame(() => {
    sourceFramePending = false;
    const pointer = pendingSourcePointer;
    pendingSourcePointer = undefined;
    if (!pointer) return;
    const bounds = elements.canvas.getBoundingClientRect();
    try {
      const selection = app.select_canvas_position(
        pointer.x - bounds.left,
        pointer.y - bounds.top,
        bounds.width,
        bounds.height,
      );
      applySelection(selection, true);
    } catch (error) {
      console.debug("Pointer is outside the source sphere", error);
    }
  });
}

function currentTouchGesture() {
  const [first, second] = [...touchPointers.values()];
  if (!first || !second) return undefined;
  return {
    x: (first.x + second.x) * 0.5,
    y: (first.y + second.y) * 0.5,
    distance: Math.hypot(second.x - first.x, second.y - first.y),
  };
}

function releasePointerCapture(event) {
  if (elements.canvas.hasPointerCapture(event.pointerId)) {
    elements.canvas.releasePointerCapture(event.pointerId);
  }
}

function finishTouch(event) {
  touchPointers.delete(event.pointerId);
  if (touchPointers.size < 2) touchGesture = undefined;
  if (touchPointers.size === 0) touchGestureWasMulti = false;
  releasePointerCapture(event);
}

elements.canvas.addEventListener("pointerdown", (event) => {
  if (!rendererReady) return;
  if (event.pointerType === "touch") {
    event.preventDefault();
    elements.canvas.setPointerCapture(event.pointerId);
    touchPointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (touchPointers.size >= 2) {
      touchGestureWasMulti = true;
      pendingSourcePointer = undefined;
      touchGesture = currentTouchGesture();
    } else if (!touchGestureWasMulti) {
      queueSourceSelection(event.clientX, event.clientY);
    }
    return;
  }
  const mode = event.button === 0 ? "source" : event.button === 2 ? "orbit" : undefined;
  if (!mode) return;
  event.preventDefault();
  elements.canvas.setPointerCapture(event.pointerId);
  mousePointerState = { pointerId: event.pointerId, mode, x: event.clientX, y: event.clientY };
  if (mode === "source") queueSourceSelection(event.clientX, event.clientY);
});
elements.canvas.addEventListener("pointermove", (event) => {
  if (event.pointerType === "touch") {
    if (!touchPointers.has(event.pointerId)) return;
    touchPointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (touchPointers.size >= 2) {
      const nextGesture = currentTouchGesture();
      if (touchGesture && nextGesture) {
        const zoomDelta = touchGesture.distance > 1 && nextGesture.distance > 1
          ? Math.log(touchGesture.distance / nextGesture.distance) * 1000
          : 0;
        app.navigate_camera(
          nextGesture.x - touchGesture.x,
          nextGesture.y - touchGesture.y,
          zoomDelta,
        );
      }
      touchGesture = nextGesture;
    } else if (!touchGestureWasMulti) {
      queueSourceSelection(event.clientX, event.clientY);
    }
    return;
  }
  if (
    !mousePointerState
    || mousePointerState.pointerId !== event.pointerId
    || !elements.canvas.hasPointerCapture(event.pointerId)
  ) return;
  if (mousePointerState.mode === "source") {
    if (event.buttons & 1) queueSourceSelection(event.clientX, event.clientY);
    return;
  }
  const deltaX = event.clientX - mousePointerState.x;
  const deltaY = event.clientY - mousePointerState.y;
  mousePointerState.x = event.clientX;
  mousePointerState.y = event.clientY;
  if (event.buttons & 2) app.orbit_camera(deltaX, deltaY);
});
elements.canvas.addEventListener("pointerup", (event) => {
  if (event.pointerType === "touch") {
    finishTouch(event);
    return;
  }
  if (!mousePointerState || mousePointerState.pointerId !== event.pointerId) return;
  mousePointerState = undefined;
  releasePointerCapture(event);
});
elements.canvas.addEventListener("pointercancel", (event) => {
  if (event.pointerType === "touch") {
    finishTouch(event);
  } else if (mousePointerState?.pointerId === event.pointerId) {
    mousePointerState = undefined;
    releasePointerCapture(event);
  }
});
elements.canvas.addEventListener("lostpointercapture", (event) => {
  if (event.pointerType === "touch") {
    touchPointers.delete(event.pointerId);
    if (touchPointers.size < 2) touchGesture = undefined;
    if (touchPointers.size === 0) touchGestureWasMulti = false;
  } else if (mousePointerState?.pointerId === event.pointerId) {
    mousePointerState = undefined;
  }
});
elements.canvas.addEventListener("contextmenu", (event) => event.preventDefault());
elements.canvas.addEventListener("wheel", (event) => {
  if (!rendererReady) return;
  const pageCanScroll = document.documentElement.scrollHeight > window.innerHeight + 1;
  if (pageCanScroll && !event.ctrlKey) return;
  event.preventDefault();
  app.zoom_camera(event.deltaY);
}, { passive: false });
new ResizeObserver(resizeRenderer).observe(elements.canvas);

for (const button of elements.cameraPresets) {
  button.addEventListener("click", () => {
    try {
      app.set_camera_preset(button.dataset.cameraPreset);
    } catch (error) {
      showError(error);
    }
  });
}

for (const button of elements.sourcePresets) {
  button.addEventListener("click", () => {
    try {
      applySelection(app.set_source_preset(button.dataset.sourcePreset), true);
    } catch (error) {
      showError(error);
    }
  });
}

for (const checkbox of elements.displayLayers) {
  checkbox.addEventListener("change", () => {
    try {
      app.set_display_layer(checkbox.dataset.displayLayer, checkbox.checked);
    } catch (error) {
      showError(error);
    }
  });
}

start();
