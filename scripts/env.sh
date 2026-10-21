#!/usr/bin/env bash
# Shared build settings. Cargo and Rust are selected through PATH or rustup.
set -euo pipefail
command -v cargo >/dev/null || { printf 'cargo is required on PATH.\n' >&2; exit 1; }
command -v rustc >/dev/null || { printf 'rustc is required on PATH.\n' >&2; exit 1; }
URES_TOOLCHAIN=$(rustc --print sysroot)
URES_HOST=$(rustc -vV | sed -n 's/^host: //p')
export PATH="$URES_TOOLCHAIN/lib/rustlib/$URES_HOST/bin:$PATH"
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
URES_TARGET=${URES_TARGET:-$(uname -m)-unknown-linux-musl}
case "$URES_TARGET" in x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;; *) printf 'Unsupported target: %s\n' "$URES_TARGET" >&2;exit 1;; esac
URES_PYTHON=${URES_PYTHON:-python3}
export URES_TOOLCHAIN URES_TARGET URES_PYTHON
