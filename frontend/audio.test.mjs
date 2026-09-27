import assert from "node:assert/strict";
import test from "node:test";

import {
  BrowserAudio,
  calculatePlaybackPosition,
  decibelsToGain,
  mapHeadRelativeHrirToHeadphones,
  validateAudioFile,
  validateMediaDuration,
  validatePlaybackOffset,
} from "./audio.js";

test("head-relative HRIRs preserve anatomical headphone channels", () => {
  const left = new Float32Array([1, 2]);
  const right = new Float32Array([3, 4]);
  const playback = mapHeadRelativeHrirToHeadphones(left, right);

  assert.strictEqual(playback.left, left);
  assert.strictEqual(playback.right, right);
});

test("positive volume gain is supported through +12 dB", () => {
  const audio = new BrowserAudio();

  audio.setVolumeDecibels(12);
  assert.equal(audio.volumeDecibels, 12);
  assert.ok(Math.abs(decibelsToGain(6.0206) - 2) < 1e-5);
  assert.throws(() => audio.setVolumeDecibels(12.1), /between -60 dB and \+12 dB/);
});

test("real-time HRIR updates retain head-relative device order", () => {
  const audio = new BrowserAudio();
  const headLeft = new Float32Array([1, 2]);
  const headRight = new Float32Array([3, 4]);

  audio.configureSampleRate(44_100);
  audio.setHrir(headLeft, headRight);

  assert.equal(audio.processingSampleRate, 44_100);
  assert.strictEqual(audio.deviceHrir.left, headLeft);
  assert.strictEqual(audio.deviceHrir.right, headRight);
});

test("streamed custom audio files are bounded without reading their bytes", () => {
  const file = (size) => ({ size, arrayBuffer: async () => new ArrayBuffer(size) });

  assert.doesNotThrow(() => validateAudioFile(file(1024), 2048));
  assert.doesNotThrow(() => validateAudioFile(file(1536 * 1024 * 1024), 2 * 1024 * 1024 * 1024));
  assert.throws(() => validateAudioFile(file(0), 2048), /empty/);
  assert.throws(() => validateAudioFile(file(4096), 2048), /current limit/);
  assert.throws(() => validateAudioFile({}, 2048), /valid local audio file/);
  assert.throws(() => validateAudioFile(file(1024), 0), /positive integer/);
});

test("streaming metadata requires a finite positive duration", () => {
  assert.doesNotThrow(() => validateMediaDuration(30 * 60));
  assert.throws(() => validateMediaDuration(0), /valid audio duration/);
  assert.throws(() => validateMediaDuration(Number.POSITIVE_INFINITY), /valid audio duration/);
});

test("streaming playback progress follows media currentTime", () => {
  const audio = new BrowserAudio();
  audio.playingMedia = { currentTime: 123.5 };
  audio.playbackDuration = 1800;
  audio.playbackLoops = false;
  audio.playbackPaused = true;

  assert.deepEqual(audio.playbackState(), {
    position: 123.5,
    duration: 1800,
    loop: false,
    paused: true,
  });
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
