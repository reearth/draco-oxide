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
Each codec encodes its own parse of the same OBJ. Every decode measurement
(speed and memory, both codecs) consumes the stream produced by the Google
Draco encoder, so the decoders are compared on identical input.

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
| DragonAttenuation (P, N, T) | 3.90 | 2.37 | 2.51 | 0.51 | 4.42 |
| FlightHelmet (P, N, T) | 3.89 | 2.25 | 2.39 | 1.57 | 4.00 |
| bldg_chiyoda_lod2 (P, N) | 6.28 | 3.87 | 4.13 | 0.58 | 11.14 |
| bunny (P, N) | 3.46 | 1.81 | 1.97 | 1.32 | 2.84 |
| Corset (P, N, T) | 4.22 | 2.34 | 2.53 | 1.67 | 3.43 |
| Duck (P, N, T) | 3.90 | 2.19 | 2.36 | 4.06 | 10.13 |
| torus (P) | 4.67 | 2.35 | 2.56 | 5.94 | 10.94 |
| bldg_894e93d9 (P, N) | 9.24 | 4.79 | 5.17 | 37.11 | 44.17 |
| sphere (P, N) | 14.62 | 4.83 | 6.88 | 86.84 | 120.83 |
| cube_quads (P, N, T) | 9.35 | 5.70 | 6.07 | 1238.33 | 1905.12 |

## Decode memory

![Decode memory](assets/decode-memory.svg)

All values are normalized by the decoded geometry size (the raw KB column of the results table): memory bytes per output byte. Peak RSS is measured the same way for both codecs (one decode in a fresh subprocess, `VmHWM` delta) and is directly comparable. The heap columns are exact allocation-event byte counts from a tracking allocator (oxide only): peak, time-weighted average, and RMS of live decode-window bytes; they exclude allocator overhead, so they read below RSS.

| mesh | oxide heap peak | oxide heap avg | oxide heap RMS | oxide peak RSS | Draco peak RSS |
|---|--:|--:|--:|--:|--:|
| DragonAttenuation (P, N, T) | 3.86 | 2.40 | 2.60 | 4.12 | 5.17 |
| FlightHelmet (P, N, T) | 3.81 | 2.45 | 2.62 | 3.84 | 4.74 |
| bldg_chiyoda_lod2 (P, N) | 11.59 | 7.66 | 8.48 | 11.67 | 14.51 |
| bunny (P, N) | 3.14 | 2.29 | 2.43 | 2.86 | 3.82 |
| Corset (P, N, T) | 4.25 | 2.73 | 2.93 | 3.55 | 5.98 |
| Duck (P, N, T) | 3.90 | 2.53 | 2.72 | 4.88 | 11.83 |
| torus (P) | 3.32 | 2.28 | 2.45 | 7.33 | 15.44 |
| bldg_894e93d9 (P, N) | 7.54 | 4.70 | 5.33 | 33.45 | 56.45 |
| sphere (P, N) | 12.97 | 7.64 | 8.92 | 86.09 | 159.34 |
| cube_quads (P, N, T) | 13.20 | 7.05 | 8.04 | 1357.40 | 2393.30 |

## Results

Each mesh name is annotated with the attributes it carries: P (position), N (normal), T (texture coordinate), C (color).

| mesh | faces | raw KB | oxide KB | Draco KB | oxide ratio | Draco ratio | oxide enc ms | Draco enc ms | oxide dec ms | Draco dec ms |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| DragonAttenuation (P, N, T) | 134995 | 4259.4 | 386.8 | 366.3 | 11.01 | 11.63 | 67.0 | 70.7 | 19.7 | 32.9 |
| FlightHelmet (P, N, T) | 94722 | 2622.7 | 159.2 | 155.5 | 16.48 | 16.87 | 28.8 | 27.0 | 10.2 | 15.9 |
| bldg_chiyoda_lod2 (P, N) | 81214 | 1752.1 | 298.5 | 542.3 | 5.87 | 3.23 | 26.0 | 53.3 | 17.1 | 31.3 |
| bunny (P, N) | 69451 | 1630.3 | 67.8 | 67.5 | 24.06 | 24.14 | 19.1 | 19.9 | 3.97 | 8.94 |
| Corset (P, N, T) | 18324 | 465.8 | 44.5 | 43.7 | 10.46 | 10.65 | 5.70 | 5.49 | 1.99 | 3.25 |
| Duck (P, N, T) | 4212 | 117.3 | 10.1 | 10.1 | 11.60 | 11.60 | 1.58 | 1.57 | 0.467 | 0.727 |
| torus (P) | 4095 | 72.0 | 2.4 | 2.4 | 29.62 | 29.62 | 0.551 | 0.426 | 0.112 | 0.152 |
| bldg_894e93d9 (P, N) | 696 | 15.3 | 3.2 | 3.0 | 4.83 | 5.18 | 0.217 | 0.200 | 0.068 | 0.112 |
| sphere (P, N) | 224 | 5.3 | 0.6 | 0.7 | 8.19 | 8.01 | 0.081 | 0.096 | 0.021 | 0.044 |
| cube_quads (P, N, T) | 12 | 0.3 | 0.2 | 0.2 | 1.54 | 1.84 | 0.024 | 0.010 | 0.005 | 0.007 |

Measurement details are in the [Method](#method) section above.
<!-- report:end -->
