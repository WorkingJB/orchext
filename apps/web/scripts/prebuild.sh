#!/usr/bin/env bash
# Rebuild the wasm artefact only when `wasm-pack` is on PATH.
#
# Local dev and CI have wasm-pack installed → fresh build runs and
# the committed apps/web/src/wasm/ output is verified by CI's diff
# check.
#
# Vercel's build image has no Rust toolchain → script no-ops and the
# committed wasm artefact is bundled as-is.

set -euo pipefail

if command -v wasm-pack >/dev/null 2>&1; then
  exec npm run build:wasm
fi

echo "wasm-pack not on PATH; using committed apps/web/src/wasm artefacts."
