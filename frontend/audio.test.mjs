import assert from "node:assert/strict";
import test from "node:test";

import {
  BrowserAudio,
  calculatePlaybackPosition,
  decibelsToGain,
  mapFrontFacingSceneToHeadphones,
  validateAudioFile,
  validatePlaybackOffset,
} from "./audio.js";

test("front-facing presentation swaps the completed stereo channels", () => {
  const left = new Float32Array([1, 2]);
  const right = new Float32Array([3, 4]);
  const playback = mapFrontFacingSceneToHeadphones(left, right);

  assert.strictEqual(playback.left, right);
  assert.strictEqual(playback.right, left);
});

test("positive volume gain is supported through +12 dB", () => {
  const audio = new BrowserAudio();

  audio.setVolumeDecibels(12);
  assert.equal(audio.volumeDecibels, 12);
  assert.ok(Math.abs(decibelsToGain(6.0206) - 2) < 1e-5);
  assert.throws(() => audio.setVolumeDecibels(12.1), /between -60 dB and \+12 dB/);
});

test("real-time HRIR updates are cached in face-on device order", () => {
  const audio = new BrowserAudio();
  const headLeft = new Float32Array([1, 2]);
  const headRight = new Float32Array([3, 4]);

  audio.configureSampleRate(44_100);
  audio.setHrir(headLeft, headRight);

  assert.equal(audio.processingSampleRate, 44_100);
  assert.strictEqual(audio.deviceHrir.left, headRight);
  assert.strictEqual(audio.deviceHrir.right, headLeft);
});

test("custom audio files are bounded before browser decoding", () => {
  const file = (size) => ({ size, arrayBuffer: async () => new ArrayBuffer(size) });

  assert.doesNotThrow(() => validateAudioFile(file(1024), 2048));
  assert.throws(() => validateAudioFile(file(0), 2048), /empty/);
  assert.throws(() => validateAudioFile(file(4096), 2048), /current limit/);
  assert.throws(() => validateAudioFile({}, 2048), /valid local audio file/);
  assert.throws(() => validateAudioFile(file(1024), 0), /positive integer/);
});

test("playback position clamps or wraps using the Web Audio clock", () => {
  assert.equal(calculatePlaybackPosition(2.5, 10, false), 2.5);
  assert.equal(calculatePlaybackPosition(12, 10, false), 10);
  assert.equal(calculatePlaybackPosition(12.5, 10, true), 2.5);
  assert.throws(() => calculatePlaybackPosition(-1, 10, false), /finite and positive/);
  assert.throws(() => calculatePlaybackPosition(1, 0, false), /finite and positive/);
});

test("playback seek offsets stay inside the source", () => {
  assert.doesNotThrow(() => validatePlaybackOffset(0, 10));
  assert.doesNotThrow(() => validatePlaybackOffset(9.99, 10));
  assert.throws(() => validatePlaybackOffset(-1, 10), /within the source duration/);
  assert.throws(() => validatePlaybackOffset(10, 10), /within the source duration/);
});
