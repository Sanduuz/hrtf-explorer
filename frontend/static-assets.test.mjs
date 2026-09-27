import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { expectedAudioWorkletBundle } from "../tools/build-audio-worklet.mjs";

const frontend = new URL("./", import.meta.url);

test("browser assets do not escape a project-site subpath", async () => {
  const [index, main, audio] = await Promise.all([
    readFile(new URL("index.html", frontend), "utf8"),
    readFile(new URL("main.js", frontend), "utf8"),
    readFile(new URL("audio.js", frontend), "utf8"),
  ]);

  assert.doesNotMatch(index, /(?:href|src)="\/(?!\/)/);
  assert.doesNotMatch(main, /["']\/(?:assets|audio|hrir|main|pkg|styles)/);
  assert.doesNotMatch(audio, /["']\/(?:audio|pkg)/);

  const moduleUrl = new URL("https://example.github.io/binaural-explorer/main.js");
  assert.equal(
    new URL("./assets/mit-kemar.bhrtf", moduleUrl).href,
    "https://example.github.io/binaural-explorer/assets/mit-kemar.bhrtf",
  );
  assert.equal(
    new URL("./pkg/module.wasm", moduleUrl).href,
    "https://example.github.io/binaural-explorer/pkg/module.wasm",
  );
});

test("checked-in AudioWorklet bundle matches its source files", async () => {
  const [actual, expected] = await Promise.all([
    readFile(new URL("audio-worklet-bundle.js", frontend), "utf8"),
    expectedAudioWorkletBundle(),
  ]);
  assert.equal(actual, expected);
  assert.match(actual, /let wasm_bindgen/);
  assert.match(actual, /registerProcessor\("binaural-hrtf-processor"/);
});
