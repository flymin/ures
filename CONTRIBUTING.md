# Contributing

Use Rust 1.99.0 with rustfmt and Clippy. Normal `cargo test --locked` and
`cargo clippy --locked --all-targets -- -D warnings` work with an existing host
toolchain. `build.sh` additionally builds a native musl executable, verifies it,
and runs CLI/PTY smoke checks. Its output is `target/<target>/release/ures`, or the
equivalent path under `CARGO_TARGET_DIR`. Set `URES_TARGET` to select the target
and `URES_PYTHON` to select a Python 3.11+ interpreter. Toolchains are selected
through `PATH` or `rustup`; `CARGO_BUILD_JOBS` controls build parallelism.
Installation is optional: set `URES_INSTALL_DIR` to an explicit destination.

Keep collection, sampling, analysis and rendering separate. New metrics must
describe their unit, scope, availability and freshness. Unknown values must not
be silently converted into zero. Avoid reading expensive per-process files
unless the selected view or checks need them.

Use `(PID/TID, starttime)` identities. Keep user totals distinct from inclusive
tree rows. Do not sum overlapping cgroups, RSS shared pages, or waited-child I/O
into an apparently exact total. Add a deterministic fixture for each accounting
or parser fix; use mock commands for failures and PTYs for terminal behavior.
Do not trigger real OOM, change kernel settings or signal target processes in tests.

Before proposing a change:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 -B -m unittest discover -s tests -p 'test_*.py'
python3 -B tests/smoke.py target/debug/ures
```

Update the English and Chinese READMEs when behavior changes. Release archive
contents are listed in `release-files.txt`; update it when adding documentation.

Version the extension and snapshot protocols independently from package releases.
Document incompatible data changes and update protocol tests. Release tags must
equal `v` followed by `Cargo.toml`'s package version. CI builds both native Linux
architectures before publishing a complete release; partial archives are never
promoted as a finished release.

For TUI color/layout changes, export the actual test-backend cells and render
them for review (the fixture uses fictional processes and users):

```bash
mkdir -p .logs
TZ=UTC URES_TUI_PREVIEW="$PWD/.logs/tui-preview.json" cargo test selection_background_is_continuous
python3 scripts/render-tui-preview.py .logs/tui-preview.json docs/assets/tui-preview.svg
```

The selection regression checks every cell, including gaps and trailing space.
The SVG renderer uses only the Python standard library; no image packages are
needed to regenerate it.
The export also writes `.light.json` and `.gruvbox.json` siblings for the built-in
alternatives; their previews use the same cells, including painted backgrounds.
