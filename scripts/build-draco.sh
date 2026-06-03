#!/usr/bin/env bash
#
# Fetch and build Google Draco's reference tools (`draco_decoder` and
# `draco_encoder`) from source, for round-trip / benchmark tests against
# draco-oxide.
#
# Idempotent: if the decoder binary already exists this is a no-op, so it is
# cheap to call on every CI run (pair it with a cache on third_party/draco).
# Set FORCE=1 to rebuild from scratch.
#
# The pinned version can be overridden with DRACO_TAG=<tag>. draco-oxide emits
# Draco mesh bitstream version 2.2, which 1.5.x decodes.
#
# On success the path to the built draco_decoder is printed on the last line.

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
