#!/usr/bin/env bash
set -euo pipefail
URES_PROJECT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$URES_PROJECT/target/ci}
cd "$URES_PROJECT"
source scripts/env.sh
"$URES_PYTHON" -B scripts/release.py metadata
bash build.sh
"$URES_PYTHON" -B scripts/release.py package --binary "$CARGO_TARGET_DIR/$URES_TARGET/release/ures" --target "$URES_TARGET" --destination dist
