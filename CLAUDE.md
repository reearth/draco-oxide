# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

draco-rs is a Rust implementation of Google's Draco mesh compression library for compressing and decompressing 3D geometric meshes and point clouds. This is currently a Work In Progress (WIP) - the encoder is functional while the decoder is largely incomplete.

## Workspace Structure

The library is split into four published crates (`macro → core → {decoder, encoder}`) plus internal tooling. Path-deps are nested under `draco-oxide/`:
- **draco-oxide/** - The main/encoder crate (published as "draco-oxide"). Contains the encoder (`src/encode/`), file-format I/O (`src/io/`), and evaluation (`src/eval.rs`). Depends on core; re-exports it as `draco_oxide::core`, and behind the default `decoder` feature re-exports the decoder as `draco_oxide::decode`. Ships with no build script.
- **draco-oxide/core/** - `draco-oxide-core`: the crate both encoder and decoder depend on. Holds the geometry/attribute data model (`mesh`, `attribute`, `corner_table`, `buffer`, `point_cloud`), numeric primitives (`types`: `NdVector`, index types, numeric traits), the codec algorithms shared between encode and decode (`codec`: entropy/prediction/connectivity/header), and `utils`. No prelude — items are `pub` and reached by canonical path.
- **draco-oxide/decoder/** - `draco-oxide-decoder`: the decoder (WIP, mostly commented out), depends on core only. The lean WASM/embedded unit — a decode-only consumer depends on this directly and never links the encoder.
- **draco-oxide/macros/draco-nd-vector/** - proc-macro crate generating low-dimensional vector ops; used only by core. Reads `DRACO_OXIDE_MAX_VECTOR_DIM` via its build script.
- **cli/** - Command-line interface (minimal implementation)
- **analyzer/** - Mesh analysis tool with HTML visualization reports
- **tests/** - Internal (unpublished) integration-test crate. Holds the declarative TOML-profile test harness (`src/lib.rs`), the codegen `build.rs`, test data, and the round-trip tests against Google Draco. Its integration tests live at `tests/tests/` (Cargo requires integration tests in a `tests/` subdir of the package).

## Common Commands

### Building
```bash
# Build entire workspace
cargo build

# Build with evaluation features (required for analysis)
cargo build --features evaluation

# Build specific crate
cargo build -p draco-oxide          # encoder (+ core, + decoder via default feature)
cargo build -p draco-oxide-core     # shared core, standalone
cargo build -p draco-oxide-decoder  # decoder (lean: core + decode, no encoder)
cargo build -p draco-oxide --no-default-features  # encoder-only (drops the decoder)
cargo build -p analyzer
```

### Testing
```bash
# Run all tests (every workspace crate)
cargo test

# Integration tests (profiles, round-trip, compatibility) live in the `tests` crate.
# The `evaluation` feature is defined there and forwards to draco-oxide/evaluation.
cargo test -p tests
cargo test -p tests --features evaluation   # also runs the eval test

# Run specific test suites within the tests crate
cargo test -p tests --test compatibility       # Basic encoding test
cargo test -p tests --test integrated_tests    # TOML-profile-driven tests + compatibility/eval
cargo test -p tests --test draco_decode        # Google Draco round-trip smoke test
```

### Code Quality
```bash
cargo fmt       # Format code
cargo clippy    # Run lints (configured with MSRV 1.84)
cargo deny check # Check licenses and dependencies
```

## Architecture Overview

Module paths below are in `draco-oxide-core` (`draco-oxide/core/src/`) unless prefixed `encode/`/`io/`/`eval.rs` (the `draco-oxide` encoder crate) or `decode/` (the `draco-oxide-decoder` crate).

### Core Data Structures (in `draco-oxide-core`)
- **Mesh**: Central data structure in `mesh/` containing faces and attributes
- **Attributes**: Vertex data (position, normal, texture coords) managed in `attribute/`
- **Corner Table**: Topological representation for mesh connectivity in `corner_table/`
- **Numeric primitives**: `NdVector`, index types (`PointIdx`, `CornerIdx`, …), and numeric traits in `types` (`types.rs`)

### Compression Pipeline
1. **Connectivity Compression**: Edgebreaker algorithm (encoder `encode/connectivity/edgebreaker.rs`)
2. **Attribute Compression**: Prediction transforms and quantization (encoder `encode/attribute/`)
3. **Entropy Coding**: rANS (range Asymmetric Numeral Systems) — shared primitives in core `codec/entropy/`; encoder-side writers in `encode/entropy/`

### Crate layout
- **draco-oxide-core**: shared types + algorithms — the data model plus `codec/` (the prediction/entropy/connectivity/header algorithms shared by encode and decode) and `utils/`
- **draco-oxide (encoder)**: `encode/` (complete, functional), `io/` (OBJ + partial glTF), `eval.rs`; depends on core, re-exports `draco_oxide::core` and (default `decoder` feature) `draco_oxide::decode`
- **draco-oxide-decoder**: `decode/` (mostly incomplete/commented out); depends on core only

## Features and Configuration

### Cargo Features (on the `draco-oxide` crate)
- `decoder` (default): re-exports `draco-oxide-decoder` as `draco_oxide::decode`. Disable with `--no-default-features` for an encoder-only build; size-sensitive consumers should instead depend on `draco-oxide-decoder` directly.
- `evaluation`: Enables compression analysis and metrics generation
- `debug_format`: Additional debug output formatting

### Test Data
Test meshes are located in `tests/data/` (in the `tests` crate):
- bunny.obj, sphere.obj, tetrahedron.obj, cube_quads.obj, punctured_sphere.obj, torus.obj

### Analysis and Evaluation
When using `--features evaluation`, you can:
- Generate detailed compression metrics
- Compare L2 norm distances between original and compressed meshes
- Create HTML visualization reports via the analyzer tool

## Development Notes

### Current Limitations
- Decoder implementation is incomplete (most functionality commented out)
- CLI tool has minimal functionality
- File format support limited to OBJ/STL with partial glTF

### Testing Patterns
Tests typically follow this pattern:
1. Load test mesh from `tests/data/` using `tobj`
2. Convert to internal `Mesh` structure using `MeshBuilder`
3. Encode using `encode()` function with configuration
4. For evaluation tests, use `EvalWriter` to capture metrics

## Version Requirements
- Rust 1.84+ (specified in rust-toolchain.toml)
- Edition 2021