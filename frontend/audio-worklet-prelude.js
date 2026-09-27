// Chromium does not expose TextDecoder in AudioWorkletGlobalScope. wasm-bindgen only
// needs this fallback when formatting a defensive error, not during normal DSP.
if (typeof TextDecoder === "undefined") {
  globalThis.TextDecoder = class {
    decode(bytes) {
      if (!bytes) return "";
      let text = "";
      for (const byte of bytes) text += String.fromCharCode(byte);
      return text;
    }
  };
}
