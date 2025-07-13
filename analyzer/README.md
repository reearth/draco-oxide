# Draco Mesh Analyzer

A tool for analyzing Draco compression performance and generating HTML reports.

## Usage

```bash
cargo run --bin analyzer -- --original path/to/mesh.obj
cargo run --bin analyzer -- --original path/to/model.glb
```

## Output

Creates timestamped output directory in `analyzer/outputs/` with:
- **index.html**: Interactive analysis report
- **Compressed files**: Draco-compressed versions
- **Evaluation data**: JSON metrics
- **Comparison files**: Decompressed versions for quality analysis

## Supported Formats

- **Input**: OBJ, glTF, GLB
- **Output**: DRC, HTML reports with compression metrics

## Building

```bash
cargo build --bin analyzer --features evaluation
```

## Features

- Compression ratio analysis
- Quality metrics (L2 norm distance)
- File size comparisons
- Interactive HTML reports with detailed breakdowns