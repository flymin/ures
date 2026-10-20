#!/usr/bin/env bash
set -euo pipefail
URES_PROJECT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
URES_INSTALL_DIR=${URES_INSTALL_DIR:-$HOME/local/bin}
source "$URES_PROJECT/scripts/env.sh"
if [[ -z ${CARGO_TARGET_DIR:-} ]]; then
    CARGO_TARGET_DIR=$(mktemp -d /tmp/ures-build.XXXXXX)
    trap 'rm -rf -- "$CARGO_TARGET_DIR"' EXIT
fi
export CARGO_TARGET_DIR URES_PROJECT URES_INSTALL_DIR
export TZ=UTC PYTHONDONTWRITEBYTECODE=1
mkdir -p "$URES_PROJECT/.logs"
exec > >("$URES_PYTHON" -u "$URES_PROJECT/scripts/sanitize_log.py" | tee "$URES_PROJECT/.logs/build.log") 2>&1
cd "$URES_PROJECT"
source scripts/prepare-host.sh
URES_FLAGS=( -C link-self-contained=yes -C target-feature=+crt-static
    "--remap-path-prefix=$HOME=/home/user"
    "--remap-path-prefix=$CARGO_HOME=/tooling/cargo"
    "--remap-path-prefix=$URES_TOOLCHAIN=/tooling/rust"
    "--remap-path-prefix=$URES_PROJECT=/workspace/ures"
    "--remap-path-prefix=$CARGO_TARGET_DIR=/build/ures" )
printf -v CARGO_ENCODED_RUSTFLAGS '%s\x1f' "${URES_FLAGS[@]}"
export CARGO_ENCODED_RUSTFLAGS=${CARGO_ENCODED_RUSTFLAGS%$'\x1f'}
rustc --version
"$URES_PYTHON" -B scripts/check_privacy.py
bash -n build.sh scripts/*.sh
"$URES_PYTHON" -B -m unittest discover -s tests -p 'test_*.py' -v
cargo fmt --check
cargo test --locked --target "$URES_TARGET" -- --test-threads=2
cargo clippy --locked --target "$URES_TARGET" --all-targets -- -D warnings
cargo build --locked --release --target "$URES_TARGET"
cargo build --locked --target "$URES_TARGET" --example terminal-panic
URES_BINARY="$CARGO_TARGET_DIR/$URES_TARGET/release/ures"
file "$URES_BINARY"
"$URES_PYTHON" -B scripts/release.py verify --binary "$URES_BINARY" --target "$URES_TARGET"
"$URES_PYTHON" -B scripts/check_privacy.py --binary "$URES_BINARY"
URES_LINKAGE=$(ldd "$URES_BINARY" 2>&1 || true)
printf '%s\n' "$URES_LINKAGE"
if [[ "$URES_LINKAGE" == *'=>'* ]]; then printf 'Refusing a dynamically linked binary.\n' >&2;exit 1;fi
"$URES_BINARY" --version
"$URES_PYTHON" -B tests/smoke.py "$URES_BINARY" "$CARGO_TARGET_DIR/$URES_TARGET/debug/examples/terminal-panic"
mkdir -p "$URES_INSTALL_DIR"
install -m 755 "$URES_BINARY" "$URES_INSTALL_DIR/.ures.installing"
mv -f -- "$URES_INSTALL_DIR/.ures.installing" "$URES_INSTALL_DIR/ures"
printf 'Installed %s/ures\n' "$URES_INSTALL_DIR"
