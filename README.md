# Binaural HRTF Explorer

A browser-oriented application for exploring binaural audio with measured head-related impulse responses (HRIRs). The repository implements the original MVP plus real-time source movement: a native spatial/DSP core, an offline dataset converter, a packaged MIT KEMAR dataset, Rust/WASM AudioWorklet processing, an interactive Rust/wgpu source-sphere scene, measurement/interpolation visualization, browser-local custom audio uploads, and a responsive interface served by Axum.

The browser page loads the real dataset into Rust/WASM, renders a pre-existing CC0 human head inside a spherical grid, supports orbit/zoom and ray-cast source dragging, and spatializes built-in or uploaded audio in real time. The source can move while audio is playing. Uploaded audio never leaves the browser.

## Quick start

Install Rust 1.85 or newer, then run the application from the repository root:

```bash
cargo run
```

Open <http://127.0.0.1:3000> in a WebGPU- or WebGL2-capable browser. Use headphones for the intended binaural effect. The generated browser files and KEMAR dataset are included in the repository, so no npm installation or separate frontend build is required for normal local use. Press `Ctrl+C` in the terminal to stop the server.

## Current architecture

```text
                    Browser
                       |
       +---------------+---------------+
       |               |               |
      DOM          Web Audio         Canvas
       |               |               |
       |       AudioWorklet/WASM    Rust/wgpu
       |               |               |
       +-------- JavaScript glue -------+
                       |
                 wasm-bindgen
                       |
             +---------+---------+
             |                   |
          HRTF core          Renderer
             |
     interpolation + stateful
       block convolution

                    Axum
                      |
              static/data serving
```

## Workspace packages

| Cargo package | Path | Target | Purpose |
| --- | --- | --- | --- |
| `binaural-explorer-server` | `server` | Runnable binary; default | Starts the local Axum server and serves the complete browser application. Run with `cargo run`. |
| `hrtf-convert` | `tools/hrtf-convert` | Runnable developer binary | Converts the official compact MIT KEMAR WAV archive into the runtime `.bhrtf` format. Run with `cargo run -p hrtf-convert -- <input> <output>`. |
| `hrtf` | `crates/hrtf` | Native library | Owns coordinate math, dataset validation, interpolation, convolution, and binaural rendering without browser dependencies. |
| `binaural-explorer-web` | `crates/web` | Rust/WASM library | Owns the browser-side dataset, source direction, camera, picking, head preprocessing, and wgpu renderer. |
| `binaural-audio-worklet` | `crates/audio-worklet` | Rust/WASM library | Exposes the stateful real-time convolver to the Web Audio `AudioWorklet`. |

Cargo's `-p` option selects workspace packages rather than filtering for executable targets, so its package listing includes the three library packages. They remain workspace members so `cargo test --workspace`, shared dependency versions, and workspace linting cover the complete application. The workspace's default member is the server, making plain `cargo run` the normal local startup command.

Outside the Cargo workspace, `frontend` contains the vanilla interface, Web Audio adapter, and generated `wasm-bindgen` artifacts. `docs/mit-kemar.md` records verified dataset metadata, conversion decisions, and attribution.

## Development

The minimum supported Rust version is 1.85.

```bash
cargo test --workspace
cargo run
```

Then open <http://127.0.0.1:3000>.

The application crates support Rust 1.85. Rebuilding the browser artifacts uses the current stable Rust toolchain because the build-time dependency graph of `wasm-bindgen-cli 0.2.128` currently requires Rust 1.88 or newer. Install the WASM target and a CLI version matching the workspace's `wasm-bindgen` crate, then run:

```bash
rustup toolchain install stable --profile minimal --target wasm32-unknown-unknown
cargo +stable install wasm-bindgen-cli --version 0.2.128 --locked
cargo +stable build -p binaural-explorer-web -p binaural-audio-worklet \
  --target wasm32-unknown-unknown --release
wasm-bindgen \
  --target web \
  --out-dir frontend/pkg \
  --out-name binaural_explorer_web \
  target/wasm32-unknown-unknown/release/binaural_explorer_web.wasm
wasm-bindgen \
  --target no-modules \
  --out-dir frontend/pkg \
  --out-name binaural_audio_worklet_nomodule \
  target/wasm32-unknown-unknown/release/binaural_audio_worklet.wasm
node tools/build-audio-worklet.mjs
```

The final command combines the AudioWorklet prelude, `wasm-bindgen` no-modules glue, and processor body into `frontend/audio-worklet-bundle.js`. The generated JavaScript, WASM, and worklet bundle are checked in so `cargo run` works without a separate frontend toolchain.

Full validation:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p binaural-explorer-web -p binaural-audio-worklet \
  --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace
node --experimental-default-type=module --test frontend/audio.test.mjs
node --experimental-default-type=module --test frontend/audio-worklet.test.mjs
node --experimental-default-type=module --test frontend/static-assets.test.mjs
node tools/build-audio-worklet.mjs --check
```

## Coordinate convention

There is one canonical, right-handed convention throughout the project:

```text
+X = right
+Y = up
+Z = front
```

Azimuth is `0°` at the front, `+90°` at the right, `-90°` at the left, and `±180°` at the back. Elevation is `0°` on the horizontal plane, `+90°` above, and `-90°` below.

```text
x = cos(elevation) * sin(azimuth)
y = sin(elevation)
z = cos(elevation) * cos(azimuth)
```

Angles at the public API are degrees; trigonometric calculations use radians. Dataset directions are normalized once during construction. The selected MIT data uses the same front/right/up angular convention. Its mirrored azimuths are converted to canonical unit vectors offline.

## Graphics and sphere selection

The canvas is rendered by Rust/WASM using wgpu 30 and WGSL; there is no Three.js or JavaScript scene graph. The scene contains:

- a smooth neutral human head with a full cranium, nose, ears, and neck;
- a latitude/longitude source-sphere grid;
- all 710 actual KEMAR measurement directions as neutral markers;
- orange markers and guide lines for the active interpolation contributors;
- red +X/right, green +Y/up, and blue +Z/front orientation arrows with a matching legend;
- a bright blue selected-source sphere with an additive glow.

The head is the male mesh from Pistachio's CC0 **2 Human Head Basemeshes** asset. Cargo applies two Catmull–Clark subdivision levels, trims the lowest neck/shoulder region, aligns the visible ear centers with the source sphere's `Y = 0` center, and preprocesses the source OBJ into canonical `+Z`-facing triangles with area-weighted smooth normals. The browser does not contain a general-purpose model loader. The renderer reflects the lateral axis at the head-to-scene boundary because the anatomical left of a face looking toward the viewer appears on the viewer's right. Measurement points, contributors, source markers, picking, axes, and side-view presets use that same presentation transform. A neutral material and two-light WGSL shader reveal the facial shape without requiring textures. The model is visual only and never participates in acoustic processing. Source, license, checksum, and conversion details are recorded in [the head-model notes](docs/head-model.md).

Pointer positions are converted from canvas coordinates to normalized device coordinates. Rust inverts the camera view-projection matrix to construct a 3D ray, intersects that ray with the radius-1.5 source sphere, and normalizes the hit relative to the origin. Camera orbiting therefore changes only the view, never the physical source coordinate system. Left-clicking or left-dragging selects the source at most once per animation frame, right-dragging orbits, and the wheel zooms within bounded limits. On touch screens, one finger selects and moves the source; two fingers orbit around their centroid and pinch to zoom. Once a two-finger gesture begins, its remaining finger cannot accidentally reposition the source. The canvas suppresses its context menu and native touch navigation so scene gestures remain uninterrupted.

Front, back, left, right, top, and bottom buttons set canonical views without modifying the source direction. Reset returns to the initial front view and zoom distance. Camera preset and gesture mathematics remain in Rust; JavaScript only translates browser pointer events into coarse-grained WASM calls.

wgpu prefers the browser WebGPU backend. Because some browsers and headless environments expose `navigator.gpu` without a usable adapter, the build also includes wgpu's WebGPU capability detection and WebGL2 backend fallback. Both paths use the same Rust renderer and WGSL scene. WebGL2 is a compatibility path, not a replacement graphics architecture.

## Interface

The responsive vanilla HTML/CSS interface keeps the 3D view dominant and groups only the controls needed by the application: built-in/custom source selection, optional looping, file status, shared output volume, playback progress, azimuth/elevation, interpolation method and diagnostics, camera presets, scene-layer visibility, Play/Pause/Resume, and Stop. An information dialog beside the title summarizes the application, privacy behavior, headphone recommendation, and the head-relative direction convention. Azimuth and elevation can be changed with synchronized sliders or typed numeric values, while a compact strip above the scene selects the six canonical source directions without changing the camera. The right column also plots the current interpolated left and right HRIRs using one shared amplitude scale, preserving their visible relative level difference as the source moves. Looping is enabled by default so a source can be explored without repeatedly restarting a short clip. The always-visible timeline wraps against the source duration while looping, supports pointer and keyboard seeking, keeps a stable-width time label, resets without changing layout height on Stop, and uses the `AudioContext` clock rather than accumulating animation-frame deltas. Seeking recreates the one-shot Web Audio source at the requested offset while retaining the same browser-local mono buffer and real-time HRTF path; a seek performed while paused stays paused. Pause/resume uses the processing `AudioContext`, so the buffer source and Rust convolution state remain in place. A scene legend distinguishes measurements, contributors, and the selected source. Keyboard focus states and native labels are retained for basic accessibility.

Display options independently hide the measurement cloud, sphere grid, coordinate axes, or interpolation guides. The orange guides connect the selected source directly to the active contributor measurements. The selected source and active contributor markers remain visible for orientation. These flags live in the Rust renderer and only skip draw calls; changing them does not rebuild GPU resources or alter interpolation and audio state.

On wide screens the interface follows a three-column scientific-workstation layout: audio and display controls on the left, the visualization in the center, and position/camera controls on the right. It collapses to one column on narrower screens. When that narrow layout requires page scrolling, an unmodified wheel scrolls the page over the canvas and `Ctrl`+wheel zooms the scene; when the page fits, the wheel zooms directly.

The volume slider controls one Web Audio `GainNode` after stereo rendering. Its gain is applied equally to both output channels, so it does not alter interaural level differences. The default is a conservative `-6 dB`; the available range is `-30 dB` to `+12 dB`. Positive gain is deliberately permitted for quiet material, and the UI warns that it can clip.

## Runtime HRTF representation

`HrtfDataset` owns a sample rate and uniformly sized `HrirMeasurement` values. Each measurement contains a pre-normalized direction, derived canonical azimuth/elevation, and left/right `f32` impulse responses. Constructors reject zero/non-finite directions, zero sample rates, empty datasets, inconsistent HRIR lengths, and non-finite samples.

No original dataset format leaks into lookup, interpolation, or convolution code.

The runtime binary format uses explicit little-endian fields:

```text
magic[8] = "HRTFRT01"
version: u32
sample_rate: u32
hrir_length: u32
measurement_count: u32

for each measurement:
    direction_x, direction_y, direction_z: f32
    left[hrir_length]: f32
    right[hrir_length]: f32
```

The parser checks the magic, version, dimensions, exact byte length, finite samples, and valid non-zero direction vectors before constructing `HrtfDataset`.

## Interpolation

The interpolation dropdown switches between eight Rust implementations:

- **Nearest neighbor** selects the closest measured direction and uses its HRIR unchanged. It is spatially discontinuous but does not blend impulse arrival times.
- **3-point linear** is the default and directly combines corresponding HRIR samples.
- **Time-aligned 3-point** uses the same neighbors and weights, estimates each ear's peak arrival with sub-sample parabolic refinement, aligns responses before mixing, and restores the weighted left/right delays independently. This reduces multi-peak smearing while retaining interpolated interaural delay. Exact measurement hits bypass shifting and remain bit-identical.
- **Spherical triangle** finds a small measured triangle containing the requested direction and derives normalized weights from the three spherical sub-triangle areas. It respects the actual measurement layout instead of always choosing the three closest points. Candidate vertices combine nearby measurements with directionally distributed neighbors so the sparse lower cap remains covered without maintaining a global triangulation.
- **Aligned spherical** uses the same containing triangle and spherical-area weights, then applies the per-ear arrival-time alignment used by the time-aligned 3-point method. This combines layout-aware spatial interpolation with reduced temporal smearing.
- **Minimum phase** uses the three nearest inverse-distance contributors, interpolates their log-magnitude spectra, reconstructs a minimum-phase HRIR through the real cepstrum, and restores the weighted peak-arrival delay independently for each ear. This separates spectral shape from interaural delay instead of blending the original phase responses.
- **Frequency domain** transforms the three nearest HRIRs, interpolates spectral magnitude linearly and phase with a circular mean, and reconstructs each ear through an inverse FFT. Circular phase interpolation handles the ±π boundary, but phase cancellation can still make this method less stable than minimum-phase interpolation.
- **Spherical harmonics** fits a third-order, 16-coefficient real spherical-harmonic model to every left/right HRIR sample across the complete dataset. A small ridge term stabilizes the least-squares solve. The model is cached after its first use, so subsequent source movement only evaluates the basis and coefficients. This is a deliberately smooth global approximation and does not have three local contributors to highlight.

The linear and time-aligned 3-point methods share the same spatial selection:

1. Normalize the requested direction.
2. Calculate clamped unit-vector angular distances, which naturally handle azimuth wrapping.
3. Select the three nearest measurements.
4. Normalize inverse-angular-distance weights.
5. Apply the same weights sample-by-sample to both HRIR channels.

An effectively exact match returns that measurement with weight 1, avoiding division by zero. The active contributor indices, angular distances, and weights are displayed in the debug panel; their directions are highlighted in orange in the 3D scene. Changing methods immediately updates those visuals, the HRIR plot, and the live AudioWorklet target filter.

Both spherical methods use local containment and spherical-area coordinates, but deliberately avoid the extra machinery of a precomputed global spherical Delaunay mesh. The direct variant combines corresponding HRIR samples; the aligned variant separates and restores the left/right delays around that combination.

Direct sample-wise HRIR interpolation is intentionally approximate: neighboring responses can have different impulse arrival times, so combining them can smear temporal and spectral structure. Time alignment reduces that problem but uses peak arrival as a deliberately simple delay estimate. Minimum-phase interpolation avoids blending measured phase, although its peak-based delay estimate remains intentionally simple. Frequency-domain interpolation retains measured phase but can encounter ambiguity when contributor phases oppose one another. The low-order spherical-harmonic model strongly smooths spatial detail and directly models time-domain HRIR samples; higher orders or frequency-domain spherical-harmonic coefficients would preserve more detail at greater computational and storage cost.

## Graphics-to-DSP integration

Canvas selections are ray-cast in Rust and update the one canonical source direction owned by `BinauralApp`. That direction immediately drives the selected-source marker, contributor lookup, contributor diagnostics, and a coarse-grained interpolated HRIR snapshot. JavaScript does not select measurements or interpolate samples.

During playback, each direction change sends one 128-sample stereo HRIR pair to the audio worklet. It never sends individual samples across the main-thread boundary. The worklet's dedicated Rust/WASM processor retains the mono input history and transitions to the target HRIR over 30 ms. Retargeting during an active transition first materializes its current effective filter, avoiding discontinuities during rapid dragging. Native integration tests use the packaged KEMAR data to verify that opposite left/right selections swap stereo HRIR channels and that substantially different directions produce different rendered signals.

The HRTF core, compact dataset, plots, and headphone output retain canonical anatomical ear order end to end: left HRIR goes to the left headphone and right HRIR goes to the right headphone. Face-on mirroring is handled only by the renderer's head-to-scene coordinate transform, so it cannot reverse audio channels. Thus a source at the head's left appears on the viewer's right in the front view and is still heard through the listener's left side.

## Convolution and levels

`TimeDomainConvolver` implements complete direct `O(NM)` linear convolution for tests and offline use. `RealtimeBinauralConvolver` implements the same direct convolution over successive Web Audio render quanta using a persistent circular history buffer. The 128-tap HRIRs are short enough for this initial real-time method; FFT/partitioned convolution remains a future optimization.

Offline binaural rendering can inspect a completed output and apply one shared peak gain. Real-time playback has no full-buffer lookahead and therefore does not peak-normalize in the worklet. One shared Web Audio `GainNode` follows the stereo worklet, preserving interaural level differences; positive user gain can clip, as the UI warns.

## HRTF dataset status

The bundled [MIT compact KEMAR dataset](docs/mit-kemar.md) contains 710 directions at 44.1 kHz and 128 samples per ear. It was measured by Bill Gardner and Keith Martin at the MIT Media Lab. The compact responses are stereo, retain interaural delay, and were equalized for the measurement loudspeaker.

The official ZIP contains 368 16-bit PCM WAV files spanning azimuth 0°–180°. `hrtf-convert` validates these files, converts samples to `f32`, and reconstructs the other hemisphere by mirroring azimuth and swapping ear channels. Median-plane positions are not duplicated. The resulting asset is `frontend/assets/mit-kemar.bhrtf`.

The data is Copyright 1994 MIT Media Laboratory, provided without usage restrictions with a request to cite its authors in research or commercial applications. Exact source URLs, archive/runtime checksums, format differences, processing history, and reproduction commands are recorded in [the dataset notes](docs/mit-kemar.md).

## Sample-rate policy

The canonical processing rate is the dataset's native 44,100 Hz. The browser explicitly requests a 44.1 kHz `AudioContext` and rejects a mismatched context rather than reinterpreting HRIR samples. Built-in signals are generated at 44.1 kHz in Rust. Uploaded media enters that context through a `MediaElementAudioSourceNode`, so Web Audio resamples it to the context rate before it reaches the HRTF worklet. The browser handles final conversion from the processing context to the physical audio device.

## Browser audio and privacy

The current page generates complete mono test signals in Rust/WASM or accepts a browser-supported audio file through `<input type="file">`. Built-in signals use an `AudioBufferSourceNode`. Uploaded files receive a local object URL and stream through an `HTMLAudioElement` plus `MediaElementAudioSourceNode`; the full recording is not decoded into a JavaScript sample array. The worklet's explicit mono, speaker-interpreted input makes Web Audio downmix stereo and multichannel media sensibly before Rust convolution. Both source paths deliver render quanta through the same worklet. JavaScript writes each quantum into reusable WASM input/output views, makes one processing call, and copies the stereo result to Web Audio. Steady-state processing performs no Rust allocation and creates no result objects; there are no per-sample JS/WASM calls. If a browser changes its render-quantum length, the worklet resizes and reacquires those buffers once, then resumes the same allocation-free steady state.

The frontend build combines the audio processor and `wasm-bindgen`'s `no-modules` glue into one self-contained static worklet script. This avoids inconsistent imported-module registration in browser audio-rendering scopes and removes the need for server-side script assembly. The bundle also includes a tiny ASCII `TextDecoder` fallback for `wasm-bindgen`'s defensive error path because Chromium does not expose `TextDecoder` inside `AudioWorkletGlobalScope`; normal DSP does not use that fallback.

Uploaded files remain local and are addressed through revocable browser object URLs. They are never sent to Axum, persisted server-side, submitted to analytics, or sent to remote DSP. Files can be selected with the native picker or dropped onto the Audio Source panel. Codec support follows the browser's media-element implementation rather than claiming universal format support. File-size, metadata, format, and playback errors are shown without disabling the built-in signals.

Custom recordings have no application-level duration limit. Encoded files are limited to 2 GiB as a defensive browser-input bound. Streaming avoids the duration-proportional decoded `Float32Array` and `AudioBuffer` allocations used by the earlier 60-second implementation. Selecting another file or clearing the input revokes the previous object URL.

## Static hosting

`frontend/` is a self-contained static site after the WASM modules and AudioWorklet bundle have been built. HTML references are relative to `index.html`, while JavaScript resolves datasets, WASM modules, and the worklet relative to its own `import.meta.url`. The same files therefore work at a domain root or under a project subpath such as `https://example.github.io/binaural-explorer/`.

Axum remains the convenient local development server, but it is not part of the deployed audio path and is not required by a static host. A future GitHub Pages workflow can publish the contents of `frontend/` directly after running the documented build commands. Use HTTP or HTTPS rather than opening `index.html` through `file://`, because browser security rules restrict WASM, AudioWorklet, media, and GPU features in local-file contexts.

## License

Original source code in this repository is available under the [MIT License](LICENSE), copyright the Binaural HRTF Explorer contributors.

Bundled third-party material is not relicensed under the project's MIT License:

- The compact KEMAR measurements are Copyright 1994 MIT Media Laboratory. MIT provides the data without restrictions on use provided that Bill Gardner and Keith Martin are cited when it is used in research or commercial applications. The project retains that citation and records the conversion history in [the dataset notes](docs/mit-kemar.md).
- The bundled male head basemesh by OpenGameArt user Pistachio is available under [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/). Its provenance and modifications are documented in [the head-model notes](docs/head-model.md).
- Rust dependencies and the generated JavaScript/WASM artifacts retain their respective permissive licenses. See [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES) and the locked dependency versions in `Cargo.lock`.

Redistributions should include both `LICENSE` and `THIRD_PARTY_NOTICES`.

## Current limitations

- The bundled HRTF is a non-individualized KEMAR measurement and may localize differently for each listener/headphone combination.
- Custom audio is limited to 2 GiB of encoded input and browser-supported streaming codecs; practical seek behavior can vary by codec and browser.
- The bundled head is a subdivided generic basemesh rather than a photorealistic scan.
- The current server embeds the UI, generated WASM, and runtime dataset; general static-file serving is not needed yet.
