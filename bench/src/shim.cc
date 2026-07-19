// C shim over libdraco so the benchmark can call Google Draco in-process and
// time it with the same harness as draco-oxide. The encoder settings mirror
// the draco_encoder CLI defaults: 11/10/8/8 quantization bits and compression
// level 7 (speed 3).

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <memory>

#include "draco/compression/decode.h"
#include "draco/compression/encode.h"
#include "draco/io/obj_decoder.h"
#include "draco/mesh/mesh.h"

extern "C" {

void* draco_bench_load_obj(const char* path) {
  draco::ObjDecoder obj;
  auto mesh = std::make_unique<draco::Mesh>();
  const draco::Status status = obj.DecodeFromFile(path, mesh.get());
  if (!status.ok()) {
    return nullptr;
  }
  return mesh.release();
}

void draco_bench_free_mesh(void* mesh) { delete static_cast<draco::Mesh*>(mesh); }

// Encodes `mesh`. When `out` is non-null the encoded bytes are copied into a
// malloc'd buffer the caller frees with draco_bench_free_buffer; a null `out`
// skips the copy so timed runs measure the encode alone. Returns 0 on success.
int draco_bench_encode(void* mesh, uint8_t** out, size_t* out_len) {
  auto* m = static_cast<draco::Mesh*>(mesh);
  draco::Encoder encoder;
  encoder.SetAttributeQuantization(draco::GeometryAttribute::POSITION, 11);
  encoder.SetAttributeQuantization(draco::GeometryAttribute::TEX_COORD, 10);
  encoder.SetAttributeQuantization(draco::GeometryAttribute::NORMAL, 8);
  encoder.SetAttributeQuantization(draco::GeometryAttribute::GENERIC, 8);
  encoder.SetSpeedOptions(3, 3);
  draco::EncoderBuffer buffer;
  const draco::Status status = encoder.EncodeMeshToBuffer(*m, &buffer);
  if (!status.ok()) {
    return 1;
  }
  if (out != nullptr) {
    *out_len = buffer.size();
    *out = static_cast<uint8_t*>(std::malloc(buffer.size()));
    std::memcpy(*out, buffer.data(), buffer.size());
  }
  return 0;
}

void draco_bench_free_buffer(uint8_t* buffer) { std::free(buffer); }

// Decodes an in-memory draco stream to a mesh (dequantized attributes, like a
// real consumer) and discards it. Returns 0 on success.
int draco_bench_decode(const uint8_t* data, size_t len) {
  draco::DecoderBuffer buffer;
  buffer.Init(reinterpret_cast<const char*>(data), len);
  draco::Decoder decoder;
  auto mesh = decoder.DecodeMeshFromBuffer(&buffer);
  return mesh.ok() ? 0 : 1;
}

}  // extern "C"
