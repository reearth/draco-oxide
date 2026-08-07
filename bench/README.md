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
| DragonAttenuation (P, N, T) | 3.90 | 2.53 | 2.69 | 0.53 | 4.01 |
| FlightHelmet (P, N, T) | 3.89 | 2.43 | 2.58 | 1.48 | 3.71 |
| bldg_chiyoda_lod2 (P, N) | 6.74 | 4.07 | 4.37 | 0.63 | 11.35 |
| bunny (P, N) | 3.46 | 1.93 | 2.10 | 1.29 | 3.37 |
| Corset (P, N, T) | 4.22 | 2.48 | 2.69 | 1.94 | 3.43 |
| Duck (P, N, T) | 3.90 | 2.33 | 2.53 | 4.43 | 10.68 |
| torus (P) | 4.67 | 2.44 | 2.65 | 6.11 | 10.44 |
| bldg_894e93d9 (P, N) | 9.58 | 5.12 | 5.53 | 21.17 | 48.35 |
| sphere (P, N) | 15.16 | 5.76 | 7.36 | 74.76 | 120.83 |
| cube_quads (P, N, T) | 9.74 | 5.88 | 6.29 | 1214.51 | 1905.12 |

## Decode memory

![Decode memory](assets/decode-memory.svg)

All values are normalized by the decoded geometry size (the raw KB column of the results table): memory bytes per output byte. Peak RSS is measured the same way for both codecs (one decode in a fresh subprocess, `VmHWM` delta) and is directly comparable. The heap columns are exact allocation-event byte counts from a tracking allocator (oxide only): peak, time-weighted average, and RMS of live decode-window bytes; they exclude allocator overhead, so they read below RSS.

| mesh | oxide heap peak | oxide heap avg | oxide heap RMS | oxide peak RSS | Draco peak RSS |
|---|--:|--:|--:|--:|--:|
| DragonAttenuation (P, N, T) | 3.86 | 2.43 | 2.62 | 4.14 | 5.42 |
| FlightHelmet (P, N, T) | 3.81 | 2.48 | 2.66 | 3.91 | 4.85 |
| bldg_chiyoda_lod2 (P, N) | 11.59 | 7.84 | 8.63 | 12.04 | 14.15 |
| bunny (P, N) | 3.14 | 2.32 | 2.46 | 2.88 | 4.61 |
| Corset (P, N, T) | 4.25 | 2.80 | 3.01 | 4.18 | 6.46 |
| Duck (P, N, T) | 3.90 | 2.56 | 2.75 | 6.51 | 11.83 |
| torus (P) | 3.32 | 2.41 | 2.56 | 8.11 | 14.55 |
| bldg_894e93d9 (P, N) | 7.54 | 4.85 | 5.44 | 37.64 | 60.64 |
| sphere (P, N) | 12.96 | 7.77 | 8.96 | 74.01 | 147.26 |
| cube_quads (P, N, T) | 13.13 | 6.98 | 7.95 | 976.37 | 2393.30 |

## Results

Each mesh name is annotated with the attributes it carries: P (position), N (normal), T (texture coordinate), C (color).

| mesh | faces | raw KB | oxide KB | Draco KB | oxide ratio | Draco ratio | oxide enc ms | Draco enc ms | oxide dec ms | Draco dec ms |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| DragonAttenuation (P, N, T) | 134995 | 4259.4 | 387.3 | 366.3 | 11.00 | 11.63 | 73.8 | 74.5 | 22.8 | 34.4 |
| FlightHelmet (P, N, T) | 94722 | 2622.7 | 159.2 | 155.5 | 16.47 | 16.87 | 32.2 | 29.2 | 12.1 | 16.9 |
| bldg_chiyoda_lod2 (P, N) | 81214 | 1752.1 | 298.5 | 542.3 | 5.87 | 3.23 | 28.3 | 58.5 | 20.6 | 34.0 |
| bunny (P, N) | 69451 | 1630.3 | 69.5 | 67.5 | 23.46 | 24.14 | 20.8 | 21.5 | 4.81 | 9.05 |
| Corset (P, N, T) | 18324 | 465.8 | 44.9 | 43.7 | 10.38 | 10.65 | 6.34 | 6.19 | 2.38 | 3.62 |
| Duck (P, N, T) | 4212 | 117.3 | 11.2 | 10.1 | 10.43 | 11.60 | 1.53 | 1.46 | 0.460 | 0.830 |
| torus (P) | 4095 | 72.0 | 3.2 | 2.4 | 22.78 | 29.62 | 0.581 | 0.417 | 0.121 | 0.152 |
| bldg_894e93d9 (P, N) | 696 | 15.3 | 3.7 | 3.0 | 4.08 | 5.18 | 0.251 | 0.213 | 0.082 | 0.125 |
| sphere (P, N) | 224 | 5.3 | 1.9 | 0.7 | 2.79 | 8.01 | 0.114 | 0.091 | 0.022 | 0.044 |
| cube_quads (P, N, T) | 12 | 0.3 | 0.2 | 0.2 | 1.54 | 1.84 | 0.016 | 0.011 | 0.005 | 0.008 |

Measurement details are in the [Method](#method) section above.
<!-- report:end -->
