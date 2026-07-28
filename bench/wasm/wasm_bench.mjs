// WASM decode benchmark: instantiates the probe module fresh per mesh, warms
// the V8 tiering up, then reports the median decode time and the linear-memory
// high-water mark.
//
// Usage: node wasm_bench.mjs <probe.wasm> <mesh.drc> [<mesh.drc> ...]

import fs from "fs";

const [wasmPath, ...drcs] = process.argv.slice(2);
const wasmBytes = fs.readFileSync(wasmPath);

// Copies the input into a fresh region of the instance's memory.
function stage(memory, data) {
  const pages = Math.ceil(data.length / 65536) + 1;
  const base = memory.grow(pages) * 65536;
  new Uint8Array(memory.buffer, base, data.length).set(data);
  return base;
}

for (const drc of drcs) {
  const data = fs.readFileSync(drc);

  // Memory: a fresh instance and exactly one decode, so the linear-memory
  // growth is one decode's high-water mark, not accumulated fragmentation.
  const memInst = (await WebAssembly.instantiate(wasmBytes, {})).instance;
  const memBase = memInst.exports.memory.buffer.byteLength + data.length;
  const memPtr = stage(memInst.exports.memory, data);
  memInst.exports.probe_decode(memPtr, data.length);
  const peakMB0 =
    (memInst.exports.memory.buffer.byteLength - memBase) / 1e6;

  // Timing: a second fresh instance, warmed up.
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});
  const { memory, probe_decode } = instance.exports;
  const base = stage(memory, data);

  const faces = probe_decode(base, data.length);
  if (faces < 0n) {
    console.error(`${drc}: decode failed`);
    continue;
  }

  // Warmup so TurboFan tier-up happens before measurement.
  const w0 = performance.now();
  while (performance.now() - w0 < 1000) probe_decode(base, data.length);

  const samples = [];
  const start = performance.now();
  while (
    samples.length < 7 ||
    (performance.now() - start < 2000 && samples.length < 400)
  ) {
    const t = performance.now();
    probe_decode(base, data.length);
    samples.push(performance.now() - t);
  }
  samples.sort((a, b) => a - b);
  const median = samples[samples.length >> 1];

  const name = drc.split("/").pop();
  console.log(
    `${name}: faces=${faces} median=${median.toFixed(3)}ms ` +
      `samples=${samples.length} peak-linear-mem=${peakMB0.toFixed(1)}MB`
  );
}
