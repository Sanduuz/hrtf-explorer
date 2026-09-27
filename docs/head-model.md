# Human head model

The scene uses the male mesh from **2 Human Head Basemeshes**, created by
OpenGameArt user Pistachio.

- Source page: <https://opengameart.org/content/2-human-head-basemeshes>
- Original file: <https://opengameart.org/sites/default/files/human_head_basemesh_male.obj>
- License: [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/)
- Downloaded OBJ SHA-256: `712a3178ae870ce4639b2e14ca1e4d3d7d5a9376eeaea0a4ede7231aacd1b72a`
- Repository copy: `crates/web/assets/head-male.obj`

The source is a neutral, untextured OBJ subdivision base mesh with 398 control positions. It includes
the cranium, face, ears, and neck. The head is visual orientation context only and
does not participate in HRTF processing or acoustic simulation.

## Build-time preprocessing

`crates/web/build.rs` converts the OBJ before compiling the renderer:

1. Parse positions and polygon indices.
2. Apply two Catmull–Clark subdivision levels, including boundary-aware vertex rules.
3. Clip the lowest 43% of the bust extent, removing the shoulders while retaining a
   short neck and a clean shared boundary.
4. Detect the symmetric outer-ear silhouette, then apply a small calibrated upward
   presentation offset so the visible center of each ear meets scene `Y = 0` and
   the center of the source sphere.
5. Triangulate the resulting polygons with a triangle fan.
6. Drop zero-area source triangles.
7. Center the mesh laterally and in depth, then scale it to 1.25 scene units tall.
8. Preserve the source orientation, where the face points toward canonical `+Z`.
9. Generate area-weighted smooth vertex normals.
10. Write a versioned, little-endian `HMESH001` vertex stream into Cargo's
   generated output directory.

The WASM module embeds that generated stream. No OBJ parser, source text, texture,
or model-loading JavaScript is shipped to the browser. Rust validates the embedded
header, dimensions, positions, and normals before creating the GPU buffer.
