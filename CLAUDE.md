# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

draco-rs is a Rust implementation of Google's Draco mesh compression library for compressing and decompressing 3D geometric meshes and point clouds. This is currently a Work In Progress (WIP) - the encoder is functional while the decoder is largely incomplete.

## Workspace Structure

This is a Cargo workspace with four crates:
- **draco-oxide/** - Main compression library (published as the "draco-oxide" crate). Ships with no build script.
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
cargo build -p draco-oxide
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

### Core Data Structures
- **Mesh**: Central data structure in `core/mesh/` containing faces and attributes
- **Attributes**: Vertex data (position, normal, texture coords) managed in `core/attribute/`
- **Corner Table**: Topological representation for mesh connectivity in `core/corner_table/`

### Compression Pipeline
1. **Connectivity Compression**: Uses Edgebreaker algorithm (`encode/connectivity/edgebreaker.rs`)
2. **Attribute Compression**: Prediction transforms and quantization (`encode/attribute/`)
3. **Entropy Coding**: rANS (range Asymmetric Numeral Systems) in `encode/entropy/`

### Key Modules
- **encode/**: Complete encoding pipeline (functional)
- **decode/**: Decoding pipeline (mostly incomplete/commented out)
- **shared/**: Common algorithms and data structures
- **io/**: File format support (OBJ, STL, partial glTF)
- **utils/**: Bit manipulation and geometric utilities

## Features and Configuration

### Cargo Features
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