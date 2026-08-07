# draco-oxide

[![Crates.io](https://img.shields.io/crates/v/draco-oxide?include_prereleases)](https://crates.io/crates/draco-oxide)
[![Documentation](https://docs.rs/draco-oxide/badge.svg)](https://docs.rs/draco-oxide)

`draco-oxide` is a high-performance [Draco](https://github.com/google/draco) codec written in pure Rust. It encodes and decodes triangular meshes (bitstream 2.2) and point clouds (bitstream 2.3, kd-tree method); legacy bitstream versions are not supported and are rejected with clear errors. Interoperability is continuously tested against the reference C++ implementation: streams encoded here decode in Google Draco, and vice versa.

The crates are published as `0.1.0-alpha` pre-releases; APIs may still move between alphas.

The workspace ships four crates:

| Crate | What it is |
| --- | --- |
| [`draco-oxide`](https://crates.io/crates/draco-oxide) | The main crate: encoder, OBJ and glTF/GLB I/O, and (by default) a re-export of the decoder as `draco_oxide::decode`. |
| [`draco-oxide-decoder`](https://crates.io/crates/draco-oxide-decoder) | The decoder alone. Depend on this directly for decode-only consumers such as WASM viewers. |
| [`draco-oxide-core`](https://crates.io/crates/draco-oxide-core) | The shared data model and codec primitives both sides build on. |
| [`draco-nd-vector`](https://crates.io/crates/draco-nd-vector) | Proc macros used by the core crate. |

## Encoder

The encoder is built to be robust and memory efficient. Every input mesh is sorted before compression so that the mesh is a disjoint union of manifolds with boundary, and the rest of the encoding pipeline never needs to handle the pathological cases, which are prone to bugs. This also allows a clean cascade of index maps that saves the expensive corner-indexed buffer per attribute. This index model is what keeps the encoder's memory footprint much below Google's original implementation's. See the [benchmark](bench/README.md) for concrete numbers on various datasets.

The same design removes the classic failure mode where a malformed mesh stalls the encoder. The mesh pre-sorting removes the case where an edge is shared by more than two faces, and the paths that build the corner table and corner-to-vertex maps admit no quadratic path.

Encode speed is at parity with Google's original C++ encoder at its default compression level, at nearly identical compression ratios. That, together with the bounded worst case, is why we recommend draco-oxide for encode-heavy pipelines: 3D Tiles composition, web game asset builds, and any batch pipeline where untrusted or messy meshes must not take down or stall a worker.

A glTF encoder is included: the transcoder rewrites `.glb`/`.gltf` assets with their mesh primitives Draco-compressed via the [`KHR_draco_mesh_compression`](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_draco_mesh_compression) extension.

## Decoder

The decoder focuses on correctness and performance. It includes a number of performance improvement ideas: lazy-or-shared walks of vertex attributes, lazy vertex-map construction, better instruction locality during the spirale-reversi walk, etc. It is roughly 1.3 to 1.9x faster than the reference decoder on real meshes while consuming 1.2 to 1.8x less peak memory (see the [benchmark](bench/README.md)). In the browser, Google's WASM decoder additionally pays its mandatory JS-glue overhead, which draco-oxide's Rust-native WASM avoids.

The decoder is designed to have no runtime configuration. Its configuration is feature-gated at compile time, so a build contains exactly the code paths it needs and the WASM module stays as small as possible:

- default features: the full dequantized path, returning original-format (float) attributes. The generic `decode()` returns the `Geometry` the stream declares; `decode_mesh()` is the typed convenience.
- `point-cloud`: adds `decode_point_cloud()`, `decode_point_cloud_portable()`, and the `Geometry::PointCloud` variant. Off by default in `draco-oxide-decoder`.
- `--no-default-features`: the portable tier, exposing `decode_mesh_portable()` and `decode_point_cloud_portable()`, which return integer attributes plus the reconstruction parameters, for consumers (e.g. GPU shaders) that reconstruct floats themselves.

## WebAssembly

The decoder is the WASM unit. Built for `wasm32-unknown-unknown` with the size-oriented profile and run through `wasm-opt -Oz`, the full-tier module is about 137 KB (53 KB gzipped) and the portable tier about 122 KB (49 KB gzipped); with point clouds they are 154 KB (60 KB) and 137 KB (55 KB). For comparison, Google's `draco_decoder_gltf.wasm` is 193 KB plus its mandatory JS glue. Every tier is kept green on wasm32 in CI, and module size is reported on every run.

## Benchmarks

Measured against Google Draco (level 7, matching settings) over the meshes in `tests/data`; both decoders decode the reference-encoded stream, so the decode comparison is over identical input. Method and full tables in [bench/README.md](bench/README.md). A snapshot:

| mesh | faces | oxide enc / Draco enc (ms) | oxide dec / Draco dec (ms) |
|---|--:|--:|--:|
| DragonAttenuation | 134,995 | 73.8 / 74.5 | 22.8 / 34.4 |
| FlightHelmet | 94,722 | 32.2 / 29.2 | 12.1 / 16.9 |
| PLATEAU city tile (LOD2) | 81,214 | 28.3 / 58.5 | 20.6 / 34.0 |
| bunny | 69,451 | 20.8 / 21.5 | 4.81 / 9.05 |
| Corset | 18,324 | 6.34 / 6.19 | 2.38 / 3.62 |

The PLATEAU row is a real 3D Tiles building tile (ECEF coordinates); draco-oxide also compresses it 1.8x smaller than the reference there, in part because its loader drops the degenerate sliver triangles such data carries.

## Getting Started

```sh
cargo add draco-oxide
```

### Encode an OBJ file

```rust
use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{Config, Encoder};
use draco_oxide::io::obj::load_obj;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mesh = load_obj("mesh.obj")?;

    // draco-oxide stream-writes to anything implementing
    // `draco_oxide::core::bit_coder::ByteWriter`; a Vec<u8> works.
    let mut buffer = Vec::new();
    Encoder::new().encode_mesh(mesh, &mut buffer, Config::default())?;

    std::fs::write("output.drc", &buffer)?;
    Ok(())
}
```

### Decode

```rust
use draco_oxide::decode::Decoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("input.drc")?;
    let mesh = Decoder::new().decode_mesh(&bytes)?;
    println!("{} faces", mesh.faces.len());
    Ok(())
}
```

Reuse one `Encoder`/`Decoder` instance across runs when processing many meshes. See [draco-oxide/examples](draco-oxide/examples/) for more, including the glTF transcoder.

### Configuration

The encoder configuration is built either through the `Config` builder methods (`with_attribute`, `with_edgebreaker`, ...) or deserialized from TOML. Per-attribute knobs cover the prediction scheme, prediction transform, quantization (explicit bits, max error against the observed range, or max error against a supplied domain), traversal, and the normal encoding mode. Invalid combinations are rejected by validation before anything is written.

### CLI

```bash
# compress input.obj into a draco file output.drc
cargo run --bin cli -- -i path/to/input.obj -o path/to/output.drc

# transcode input.glb into a Draco-compressed glb (KHR_draco_mesh_compression)
cargo run --bin cli -- --transcode -i path/to/input.glb -o path/to/output.glb
```

## Point clouds

Point clouds are compressed with the kd-tree method (bitstream 2.3), which carries every attribute in one integer kd-tree: float attributes are quantized, signed integers are shifted to unsigned, and unsigned integers are taken as they are. All seven compression levels are implemented, and for the levels the reference encoder can produce, draco-oxide emits byte-identical streams.

```rust
use draco_oxide::core::attribute::{Attribute, AttributeDomain, AttributeId, AttributeType};
use draco_oxide::core::types::{ConfigType, NdVector};
use draco_oxide::encode::{encode_point_cloud, PointCloudConfig};
use draco_oxide::{decode, PointCloud};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let positions: Vec<NdVector<3, f32>> = vec![
        NdVector::from([0.0, 0.0, 0.0]),
        NdVector::from([1.0, 2.0, 3.0]),
    ];
    let att = Attribute::from_without_removing_duplicates::<NdVector<3, f32>, 3>(
        AttributeId::new(0),
        positions,
        AttributeType::Position,
        AttributeDomain::Position,
        Vec::new(),
    );
    let point_cloud = PointCloud::new(vec![att])?;

    let mut buffer = Vec::new();
    encode_point_cloud(point_cloud, &mut buffer, PointCloudConfig::default())?;

    let decoded = decode::decode_point_cloud(&buffer)?;
    println!("{} points", decoded.num_points());
    Ok(())
}
```

As in the reference implementation, the kd-tree algorithm does not preserve point order.

## Not supported

- The sequential point-cloud method (`-cl 0` in the reference encoder); kd-tree covers every other level.
- Legacy Draco bitstreams (before 2.2). This is likely fine, as the 2.2 bitstream was released in 2018.

## Acknowledgements

- **Google Draco**, the original C++ implementation.
- Bench and test meshes from the [Khronos glTF Sample Models](https://github.com/KhronosGroup/glTF-Sample-Models) and the [Stanford 3D Scanning Repository](https://graphics.stanford.edu/data/3Dscanrep/); each data file carries its citation.
- PLATEAU bench meshes are derived from [Project PLATEAU](https://www.mlit.go.jp/plateau/) 3D city models by MLIT, Japan. 出典：「3D都市モデル（Project PLATEAU）千代田区」および「3D都市モデル（Project PLATEAU）新宿区（2025年度）」（国土交通省）を加工して作成

## Contact

Re:Earth core committers: [community@reearth.io](mailto:community@reearth.io)

## License

Licensed under either (at your discretion):

- Apache License, Version 2.0
   ([LICENSE_APACHE](LICENSE_APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license
   ([LICENSE_MIT](LICENSE_MIT) or http://opensource.org/licenses/MIT)
