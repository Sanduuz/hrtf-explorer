import assert from "node:assert/strict";
import test from "node:test";

import { createHrirPath, sharedAbsolutePeak } from "./hrir-plot.js";

test("HRIR plots share one peak scale across both ears", () => {
  const left = new Float32Array([0, 0.25, -0.5]);
  const right = new Float32Array([0, 1, -0.75]);

  assert.equal(sharedAbsolutePeak(left, right), 1);
  assert.match(createHrirPath(left, 1), /^M0\.00 56\.00 L130\.00 43\.50/);
  assert.match(createHrirPath(right, 1), /^M0\.00 56\.00 L130\.00 6\.00/);
});

test("empty and silent HRIRs produce an empty path without invalid coordinates", () => {
  assert.equal(createHrirPath(new Float32Array(), 1), "");
  assert.equal(createHrirPath(new Float32Array([0, 0]), 0), "");
});
