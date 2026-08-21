# AGENTS.md

Guidance for AI coding agents working in this repository.

## Project Overview

draco-oxide is a high-performance Draco codec in pure Rust. Both the encoder
and the decoder are complete for triangular meshes on the current Draco
bitstream (2.2) and for point clouds on bitstream 2.3 (kd-tree method), and are
interoperability-tested against the reference C++ implementation in both
directions. Legacy bitstreams and the sequential point-cloud method are
rejected with dedicated errors.

## Workspace Structure

Published crates (`macro -> core -> {decoder, encoder}`), path-deps nested
under `draco-oxide/`:

- **draco-oxide/**: the main/encoder crate (published as "draco-oxide").
  Encoder (`src/encode/`), OBJ + glTF/GLB I/O (`src/io/`). Re-exports core as
  `draco_oxide::core` and, behind the default `decoder` feature, the decoder
  as `draco_oxide::decode`. Entry points: `encode::Encoder::encode_mesh` and
  `encode::Encoder::encode_point_cloud` (the free `encode_mesh()` and
  `encode_point_cloud()` are one-shot wrappers). Point-cloud encoding lives in
  `src/encode/point_cloud/` under its own `PointCloudConfig`, since none of the
  mesh knobs (connectivity, prediction schemes, traversal) apply.
- **draco-oxide/core/**: `draco-oxide-core`, shared by both sides. The
  geometry/attribute data model (`mesh`, `point_cloud`, `attribute`,
  `buffer`), numeric
  primitives (`types`: `NdVector`, typed index newtypes, numeric traits), and
  codec algorithms shared between encode and decode (`codec`:
  entropy/prediction/connectivity/header).
- **draco-oxide/decoder/**: `draco-oxide-decoder`, depends on core only; the
  lean WASM/embedded unit. Entry point: `Decoder`; the generic `decode()`
  returns the `Geometry` enum the stream declares; `decode_mesh()`,
  `decode_mesh_portable()`, `decode_point_cloud()`, and
  `decode_point_cloud_portable()` are the typed entry points (free-function
  wrappers of each exist). No runtime configuration: tiers are feature-gated
  (`dequantize` on by default; disable for the portable tier, which keeps the
  `_portable` entry points of both geometries; `point-cloud` off by default
  because it costs ~18 KB of linked WASM, and enabled by the `draco-oxide`
  crate; `rare-component-types` on by default, disable to reject i8/i16/i32
  attributes and drop their narrowing code).
- **draco-oxide/macros/draco-nd-vector/**: proc macros generating the
  low-dimensional vector ops used by core.
- **cli/**: the encode/transcode CLI app.
- **tests/**: unpublished integration-test crate holding the profile-test
  harness (`src/lib.rs`), its codegen `build.rs`, test data (`data/`), and
  the round-trip suites against Google Draco.
- **bench/**: unpublished benchmark against Google Draco (see below).
- **wasm-size-probe/**: minimal C-ABI surface over the decoder used to
  measure linked WASM module size per feature tier.
- **wasi-codec/**: unpublished file-in/file-out encode/decode binary over
  `draco-oxide`, built for `wasm32-wasip1` so the profile tests can run the
  codec under wasmtime (see Testing).

## The encoder's cascading index model

The encoder resolves every mesh into a cascade of index spaces up front, and
all later stages are plain array lookups along that cascade:

    corner -> point -> vertex (per attribute) -> unique attribute value

- Attribute values are deduplicated once at mesh build; points map to unique
  value indices per attribute.
- `encode/ds.rs::build_global_ds` sorts the mesh before anything else:
  `sort_mesh` splits every non-manifold point (minting points that alias the
  same values), so each point's corners form a single seam-connected sector
  and every downstream point-keyed map is a total function.
- Edge matching is a two-pass counting sort over edge endpoints; an edge
  whose face-adjacency count is not exactly 2, or whose two faces disagree on
  winding, becomes a boundary. Nothing in the encoder is super-linear, so
  pathological inputs (e.g. 425k faces sharing one edge) cannot stall it;
  `tests/profiles/pathological_*_timeout.toml` guard this with time ceilings.
- Per-attribute connectivity (`AttributeCornerTable`) is a view over the
  position corner table: shared outright when the attribute has no interior
  seams, materialized as one flat cut-opposite array when it does. This is
  the main reason encoder memory stays well below the reference
  implementation.

Consequences when editing: the vertex enumeration order and traversal
sequences are decoder-visible (the decoder derives the same orders), so
changes to walk order or vertex minting change the bitstream. The byte-stable
golden tests and the Draco round-trip suites catch this.

## The point-cloud codec

Point clouds use a different bitstream version (2.3) and a separate code path
from meshes; the two share only the attribute descriptors and the entropy
primitives.

- Only the kd-tree method is implemented. The reference selects it for every
  compression level except `-cl 0`, which picks the sequential method; that
  one is rejected (`Err::Unimplemented`).
- Every attribute rides one integer kd-tree of `sum(num_components)`
  dimensions. Floats are quantized (per-component minimum, one shared step
  from the largest extent), signed integers are shifted by their per-component
  minimum, unsigned integers are copied.
- Stream layout: header, optional metadata, `u32` point count, `u8` encoder
  count (always 1), the attribute descriptors, `u8` compression level, the
  kd-tree block, then the transform data (quantization parameters per float
  attribute, then the signed shifts).
- The kd-tree block is a bit length, a point count, and four independently
  framed sub-streams in order: numbers, remaining bits, axis, half. The
  compression level picks the coder for the numbers stream (direct for 0-1,
  binary rANS for 2-3, folded 32-context for 4-6) and whether the split axis
  is chosen adaptively (level 6 only).
- The algorithm does not preserve point order, so tests compare decoded clouds
  as point sets (symmetric Hausdorff distance), never index by index.
- Our encoder is byte-identical to the reference on every level the reference
  can emit. That depends on matching `std::partition`'s two-ended scan, so it
  is a useful debugging signal but not something to assert in tests.

## Testing

```bash
scripts/build-draco.sh   # once: build the Google Draco reference for round trips
cargo test --workspace   # everything
cargo test -p tests      # integration: profiles, round trips, compatibility
```

### Profile tests (preferred)

New tests should be declarative TOML profiles in `tests/profiles/*.toml`;
write Rust tests only when the harness cannot express the case. A profile is
a list of operations run in order in a scratch dir:

- `DracoOxideEncode` / `DracoOxideDecode`: our codec; `[operations.cfg...]`
  is the encoder's TOML config surface (see `encode/config_spec.rs`).
- `DracoEncode` / `DracoDecode`: the reference binaries (`cfg` takes
  `compression_level`, `position_quantization`, `metadata`, ...).
- `Comparison`: `L2Norm` (max) or `Ssim` (min, `color_by` Geometry/Uv/...)
  between two files.
- `Validation`: format sanity of an output.
- `timeout_secs` on an operation enforces a time ceiling.

`tests/build.rs` generates one `#[test]` per profile; the test name is the
file stem. Test data lives in `tests/data/`; derived data must carry the
citation its license requires in the file header (see existing headers).

### Profile tests on WASM

The same profiles run with draco-oxide on a 32-bit WASM target, which is
where `usize`-width bugs surface:

```bash
rustup target add wasm32-wasip1     # once; also install wasmtime
cargo build -p wasi-codec --target wasm32-wasip1 --release
DRACO_OXIDE_WASM=target/wasm32-wasip1/release/wasi-codec.wasm \
  cargo test -p tests --test integrated_tests
```

`DRACO_OXIDE_WASM` reroutes every `DracoOxideEncode` / `DracoOxideDecode`
through the module under `wasmtime` (`WASMTIME=<path>` if it is not on
PATH; a relative module path is taken from the workspace root); the
reference-binary operations and comparisons are unchanged.
`cargo test -p draco-oxide-core -p draco-oxide-decoder --target wasm32-wasip1`
runs the unit tests on the same target via the runner in
`.cargo/config.toml`. CI runs both.

## Bench

`cargo run -p bench --release` benchmarks both codecs (speed, compression
ratio, peak RSS, and exact heap tracking for oxide) over a fixed subset of
`tests/data`, renders SVGs into `bench/assets/`, and splices the report into
`bench/README.md` between the report markers. `--local` also benches OBJs in
the git-ignored `tests/data/local/`. Fairness rules: every timed run
constructs a fresh `Encoder`/`Decoder` inside the measured region, so
cross-run resource reuse can never flatter the numbers, and every decode
measurement (both codecs) consumes the stream the reference encoder
produced, so the decoders are compared on identical input. Helper binaries:
`profile_encode` (callgrind-friendly encode timing), `time_decode`,
`dump_drc`.

## Code Quality

```bash
cargo fmt
cargo clippy    # MSRV 1.84
cargo deny check
```

CI (`.github/workflows/tests.yml`) enforces fmt, warnings-as-errors, clippy,
wasm32 builds of both decoder tiers with module-size reporting, and the full
test suite against a cached Google Draco build. `release.yml` publishes the
four crates to crates.io in dependency order when a `v*` tag matching the
workspace version is pushed (requires the `CARGO_REGISTRY_TOKEN` secret).

## Conventions

- Doc comments state what an item does, concisely. No development narrative should be written.
- Inline comments only for safety contracts or invariants.
- Encoder configuration changes must keep `Config::validate` exhaustive:
  every selectable combination either works end-to-end (with a profile test)
  or is rejected before anything is written.
