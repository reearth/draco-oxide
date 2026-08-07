# Benchmark

Benchmark of draco-oxide against Google Draco over the meshes in `tests/data`,
measuring compression ratio, encode/decode speed, and memory use. Each run
renders SVG charts into `assets/` and splices the report into this README,
below the [Method](#method) section.

## How to run it

```bash
scripts/build-draco.sh                    # once: build libdraco
cargo run -p bench --release
```

`scripts/build-draco.sh` builds the Google Draco checkout the bench links
against; override its location with `DRACO_SRC_DIR` / `DRACO_BUILD_DIR`.

You can also benchmark your own data. Place the OBJ files in `tests/data/local`,
then

```bash
cargo run -p bench --release -- --local   # also bench OBJs in tests/data/local/
```

Currently we support OBJ format only.

## Method

The input meshes are the OBJ files in `tests/data/`, and "raw" refers to the
uncompressed geometry both codecs start from: every attribute's unique values
plus 32-bit face indices. With `--local`, the OBJ files in the git-ignored
`tests/data/local/` directory are benched as well, and the report notes such
runs.

Both codecs run in-process on in-memory data and share the same timing
harness: each measurement is the median wall time over repeated runs after a
warmup. They also run with matching settings: edgebreaker connectivity, 11-bit
positions, 10-bit texture coordinates, and 8-bit octahedral normals (Draco at
compression level 7, its CLI default).
draco-oxide encodes with `encode()` under `Config::default()` and
decodes with `decode()` back to original-format floats. Google Draco is
`libdraco` called through a C shim, encoding with `Encoder::EncodeMeshToBuffer`
under the CLI-default options and decoding with `Decoder::DecodeMeshFromBuffer`.
Each codec encodes its own parse of the same OBJ.

Speed is reported as throughput over the raw geometry size, in MB/s consumed
by the encoder and MB/s produced by the decoder (1 MB = 10^6 bytes).
Compression ratio is raw bytes divided by compressed bytes, so both codecs
share the same raw baseline.

Memory is measured for one encode and one decode of each mesh. Peak RSS is the
`VmHWM` growth of the operation in a fresh subprocess per codec, with the
kernel's high-water mark reset via `/proc/self/clear_refs` after the input OBJ
or stream has been loaded, so only what the operation itself adds counts. The
oxide heap columns come from a counting global allocator: live bytes relative
to the start of the operation, integrated over time at every allocation event.
All memory values are reported as ratios to the raw geometry size.

<!-- report:start -->
## Compression

![Compression ratio](assets/compression-ratio.svg)

## Speed

![Encode speed](assets/encode-speed.svg)

![Decode speed](assets/decode-speed.svg)

## Encode memory

![Encode memory](assets/encode-memory.svg)

All values are normalized by the raw geometry size (the raw KB column of the results table) and measure memory on top of the already-loaded input mesh: memory bytes per input byte. Peak RSS is measured the same way for both codecs (one encode in a fresh subprocess that has loaded the OBJ, `VmHWM` delta) and is directly comparable. The heap columns are exact allocation-event byte counts from a tracking allocator (oxide only): peak, time-weighted average, and RMS of live encode-window bytes; they exclude allocator overhead, so they read below RSS.

| mesh | oxide heap peak | oxide heap avg | oxide heap RMS | oxide peak RSS | Draco peak RSS |
|---|--:|--:|--:|--:|--:|
| DragonAttenuation (P, N, T) | 3.90 | 2.53 | 2.70 | 0.46 | 4.39 |
| bunny (P, N) | 3.46 | 1.93 | 2.09 | 1.16 | 2.54 |
| Duck (P, N, T) | 3.90 | 2.32 | 2.52 | 0.48 | 10.13 |
| torus (P) | 4.67 | 2.36 | 2.59 | 2.89 | 10.44 |
| bldg_894e93d9 (P, N) | 9.21 | 5.22 | 5.60 | 13.07 | 44.17 |
| sphere (P, N) | 15.16 | 5.93 | 7.53 | 12.08 | 120.83 |
| cube_quads (P, N, T) | 9.86 | 5.94 | 6.36 | 595.35 | 2036.09 |

## Decode memory

![Decode memory](assets/decode-memory.svg)

All values are normalized by the decoded geometry size (the raw KB column of the results table): memory bytes per output byte. Peak RSS is measured the same way for both codecs (one decode in a fresh subprocess, `VmHWM` delta) and is directly comparable. The heap columns are exact allocation-event byte counts from a tracking allocator (oxide only): peak, time-weighted average, and RMS of live decode-window bytes; they exclude allocator overhead, so they read below RSS.

| mesh | oxide heap peak | oxide heap avg | oxide heap RMS | oxide peak RSS | Draco peak RSS |
|---|--:|--:|--:|--:|--:|
| DragonAttenuation (P, N, T) | 3.73 | 2.53 | 2.72 | 3.57 | 5.37 |
| bunny (P, N) | 3.05 | 2.32 | 2.47 | 2.82 | 4.34 |
| Duck (P, N, T) | 3.84 | 2.72 | 2.90 | 6.75 | 12.38 |
| torus (P) | 3.32 | 2.37 | 2.57 | 9.00 | 15.44 |
| bldg_894e93d9 (P, N) | 6.68 | 4.39 | 4.83 | 32.93 | 56.45 |
| sphere (P, N) | 9.19 | 5.08 | 6.03 | 83.07 | 159.34 |
| cube_quads (P, N, T) | 11.87 | 6.02 | 7.03 | 964.47 | 2583.81 |

## Results

Each mesh name is annotated with the attributes it carries: P (position), N (normal), T (texture coordinate), C (color).

| mesh | faces | raw KB | oxide KB | Draco KB | oxide ratio | Draco ratio | oxide enc ms | Draco enc ms | oxide dec ms | Draco dec ms |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| DragonAttenuation (P, N, T) | 134995 | 4259.4 | 387.3 | 366.3 | 11.00 | 11.63 | 79.1 | 80.4 | 20.8 | 36.1 |
| bunny (P, N) | 69451 | 1630.3 | 67.7 | 67.5 | 24.08 | 24.14 | 22.0 | 21.1 | 5.11 | 9.38 |
| Duck (P, N, T) | 4212 | 117.3 | 11.0 | 10.1 | 10.70 | 11.60 | 1.67 | 1.59 | 0.438 | 0.810 |
| torus (P) | 4095 | 72.0 | 3.2 | 2.4 | 22.78 | 29.62 | 0.590 | 0.423 | 0.123 | 0.152 |
| bldg_894e93d9 (P, N) | 696 | 15.3 | 3.9 | 3.0 | 3.96 | 5.18 | 0.258 | 0.232 | 0.088 | 0.124 |
| sphere (P, N) | 224 | 5.3 | 1.9 | 0.7 | 2.79 | 8.01 | 0.114 | 0.091 | 0.022 | 0.044 |
| cube_quads (P, N, T) | 12 | 0.3 | 0.2 | 0.2 | 1.54 | 1.84 | 0.025 | 0.029 | 0.005 | 0.008 |

Measurement details are in the [Method](#method) section above.
<!-- report:end -->
