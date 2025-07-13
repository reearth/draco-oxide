# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

draco-rs is a Rust implementation of Google's Draco mesh compression library for compressing and decompressing 3D geometric meshes and point clouds. This is currently a Work In Progress (WIP) - the encoder is functional while the decoder is largely incomplete.

## Workspace Structure

This is a Cargo workspace with three crates:
- **draco-rs/** - Main compression library (published as "draco" crate)
- **cli/** - Command-line interface (minimal implementation)
- **analyzer/** - Mesh analysis tool with HTML visualization reports

## Common Commands

### Building
```bash
# Build entire workspace
cargo build

# Build with evaluation features (required for analysis)
cargo build --features evaluation

# Build specific crate
cargo build -p draco
cargo build -p analyzer
```

### Testing
```bash
# Run all tests
cargo test

# Run tests with evaluation features
cargo test --features evaluation

# Run specific test suites
cargo test compatibility      # Basic encoding tests
cargo test integrated_tests  # Full encode/decode cycles
cargo test obj_reindexing     # OBJ file processing
cargo test eval              # Evaluation tests (requires --features evaluation)
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
Test meshes are located in `draco-rs/tests/data/`:
- bunny.obj, sphere.obj, tetrahedron.obj, triangle.obj, torus.obj

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

### Dependencies
- `tobj`: OBJ file parsing
- `faer`: Linear algebra operations
- `serde`: Serialization for configuration and evaluation data
- `nd_vector`: Custom macro for N-dimensional vectors

## Version Requirements
- Rust 1.84+ (specified in rust-toolchain.toml)
- Edition 2021