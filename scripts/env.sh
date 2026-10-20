#!/usr/bin/env bash
# Source from the project build scripts. Uses an existing toolchain only.
set -euo pipefail
URES_VENV=${URES_VENV:-${VIRTUAL_ENV:-}}
if [[ -n "$URES_VENV" ]]; then
    test -x "$URES_VENV/bin/cargo" || { printf 'No cargo in URES_VENV=%s\n' "$URES_VENV" >&2; exit 1; }
    export PATH="$URES_VENV/bin:$PATH"
    export CARGO_HOME=${CARGO_HOME:-$URES_VENV/share/cargo}
fi
command -v cargo >/dev/null || { printf 'Rust is unavailable; set URES_VENV to an existing toolchain environment.\n' >&2; exit 1; }
URES_TOOLCHAIN=$(rustc --print sysroot)
URES_HOST=$(rustc -vV | sed -n 's/^host: //p')
export PATH="$URES_TOOLCHAIN/lib/rustlib/$URES_HOST/bin:$PATH"
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}
URES_TARGET=${URES_TARGET:-$(uname -m)-unknown-linux-musl}
case "$URES_TARGET" in x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;; *) printf 'Unsupported target: %s\n' "$URES_TARGET" >&2;exit 1;; esac
URES_PYTHON=${URES_PYTHON:-${URES_VENV:+$URES_VENV/bin/python}}
URES_PYTHON=${URES_PYTHON:-python3}
export URES_VENV URES_TOOLCHAIN URES_TARGET URES_PYTHON
