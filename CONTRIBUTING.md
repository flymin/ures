# Contributing

Keep the package, documentation and distribution version at **0.1.0** during
this ongoing iteration. Do not bump it unless the user explicitly requests a
version change or release. Record changes under 0.1.0 in the changelog.

Use Rust 1.99.0 with rustfmt and Clippy. Normal `cargo test --locked` and
`cargo clippy --locked --all-targets -- -D warnings` work with an existing host
toolchain. `build.sh` additionally builds a native musl executable, verifies it,
runs CLI/PTY smoke checks and installs it. It does not install toolchains.

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
python3 -B scripts/check_privacy.py
python3 -B tests/smoke.py target/debug/ures
```

Update the English and Chinese READMEs when behavior changes. Keep examples,
screenshots and fixtures fictional. Do not include account paths, local build
logs, credentials, real process arguments or compiler caches in commits.
See [publication privacy](docs/privacy.md) for artifact checks and the external
private-term file. Update `release-files.txt` when adding release documentation.

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
needed to regenerate it. Select your project's existing compiler/Python setup.
The export also writes `.light.json` and `.gruvbox.json` siblings for the built-in
alternatives; their previews use the same cells, including painted backgrounds.
