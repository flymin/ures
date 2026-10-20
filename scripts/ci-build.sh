#!/usr/bin/env bash
set -euo pipefail
URES_PROJECT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$URES_PROJECT/target/ci}
export URES_INSTALL_DIR=${URES_INSTALL_DIR:-$CARGO_TARGET_DIR/install}
URES_PYTHON=${URES_PYTHON:-python3}
cd "$URES_PROJECT"
"$URES_PYTHON" -B scripts/release.py metadata
bash build.sh
"$URES_PYTHON" -B scripts/release.py verify --binary "$URES_INSTALL_DIR/ures" --target "$URES_TARGET"
"$URES_PYTHON" -B scripts/release.py package --binary "$URES_INSTALL_DIR/ures" --target "$URES_TARGET" --destination dist
