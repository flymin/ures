#!/usr/bin/env bash
# An existing musl Rust host also needs its own libc/libgcc for procedural macros.
URES_REAL_LLD=$(command -v rust-lld)
URES_HOST_GCC="$URES_TOOLCHAIN/lib/libgcc_s.so.1"
URES_HOST_LIBC="${URES_VENV:-/nonexistent}/opt/musl/usr/lib/${URES_HOST%%-*}-linux-musl/libc.so"
if [[ "$URES_HOST" == *-linux-musl && -f "$URES_HOST_GCC" && -f "$URES_HOST_LIBC" ]]; then
    URES_HOST_LIB="$CARGO_TARGET_DIR/.host/lib"
    mkdir -p "$URES_HOST_LIB" "$CARGO_TARGET_DIR/.host/bin"
    ln -sf "$URES_HOST_GCC" "$URES_HOST_LIB/libgcc_s.so"
    ln -sf "$URES_HOST_LIBC" "$URES_HOST_LIB/libc.so"
    cat > "$CARGO_TARGET_DIR/.host/bin/rust-lld" <<'EOF'
#!/usr/bin/env bash
for ures_arg in "$@"; do
    if [[ "$ures_arg" == -shared ]]; then exec "$URES_REAL_LLD" "$@" "-L$URES_HOST_LIB"; fi
done
exec "$URES_REAL_LLD" "$@"
EOF
    chmod +x "$CARGO_TARGET_DIR/.host/bin/rust-lld"
    export URES_REAL_LLD URES_HOST_LIB
    export PATH="$CARGO_TARGET_DIR/.host/bin:$PATH"
    URES_HOST_KEY=${URES_HOST^^};URES_HOST_KEY=${URES_HOST_KEY//-/_}
    export "CARGO_TARGET_${URES_HOST_KEY}_LINKER=$CARGO_TARGET_DIR/.host/bin/rust-lld"
fi
