#!/usr/bin/env bash
#
# Fetch and build Google Draco's reference `draco_decoder`/`draco_encoder` for
# round-trip tests against draco-oxide. Prints the decoder path on the last line.
#
# Idempotent (no-op if the decoder exists; set FORCE=1 to rebuild), so it's cheap
# to call on every CI run paired with a cache on third_party/draco. Override the
# pinned version with DRACO_TAG=<tag>; draco-oxide emits bitstream 2.2 for meshes
# and 2.3 for point clouds, both of which 1.5.x decodes.

set -euo pipefail

DRACO_TAG="${DRACO_TAG:-1.5.7}"

# Repo root is the parent of this script's directory (<root>/scripts/).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

DRACO_SRC="$ROOT_DIR/third_party/draco"
DRACO_BUILD="$DRACO_SRC/_build"
DECODER_BIN="$DRACO_BUILD/draco_decoder"

if [[ -x "$DECODER_BIN" && "${FORCE:-0}" != "1" ]]; then
  echo "draco_decoder already built (set FORCE=1 to rebuild)" >&2
  echo "$DECODER_BIN"
  exit 0
fi

# Shallow-clone the pinned tag if we don't already have the source.
if [[ ! -d "$DRACO_SRC/.git" ]]; then
  echo "Cloning google/draco @ $DRACO_TAG ..." >&2
  rm -rf "$DRACO_SRC"
  git clone --depth 1 --branch "$DRACO_TAG" \
    https://github.com/google/draco.git "$DRACO_SRC"
fi

echo "Configuring and building draco (decoder + encoder, Release) ..." >&2
cmake -S "$DRACO_SRC" -B "$DRACO_BUILD" -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build "$DRACO_BUILD" --target draco_decoder draco_encoder --parallel

echo "Built draco_decoder at: $DECODER_BIN" >&2
echo "$DECODER_BIN"
