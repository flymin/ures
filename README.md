<div align="center">

# ures

Fixtures and previews use fictional data. See [publication privacy](docs/privacy.md)
before sharing runtime output or building a release.

### Follow the user. Find the process tree.

![Version](https://img.shields.io/badge/version-0.1.0-8b5cf6)
![Rust](https://img.shields.io/badge/Rust-1.99-f97316?logo=rust)
![Linux](https://img.shields.io/badge/Linux-x86__64%20%7C%20aarch64-06b6d4)
![Static](https://img.shields.io/badge/binary-static-14b8a6)
![License](https://img.shields.io/badge/license-MIT-6366f1)

**English** · [简体中文](README.zh-CN.md)

<img src="docs/assets/tui-preview.svg" alt="ures TUI: ordered tabs, semantic CPU/memory colors and a continuous selected row" width="1100">

<sub>Illustrative data. CPU is measured in cores; tree totals overlap.</sub>

</div>

`ures` answers a focused question: **which process, process tree, or application is consuming this user's resources?** It keeps quota pressure, CPU throttling and OOM evidence beside the processes that explain them. `ures` opens the interactive TUI by default; `report` exports text or JSON, with `-i` for repeated snapshots. All modes share the same collection and analysis code.

Development continues at **0.1.0**; routine iterations do not change the version.

| Find out | Command |
| --- | --- |
| Open the interactive dashboard | `ures` |
| Print the trees using the most memory | `ures report --view tree` |
| Which process uses CPU? | `ures report --view ps --sort cpu` |
| What is another user running? | `ures -u alice` |
| Which application is responsible? | `ures report --view groups` |
| Is it doing I/O or holding many FDs? | `ures report --view io` |
| Was something OOM-killed? | `ures report --view oom --since 24h` |
| Explore a configured view | `ures --view io` |

## Get started

```bash
ures                           # Open the TUI
ures --theme light -i2          # Light TUI, refreshed every two seconds
ures report                    # Print the overview and default view once
ures --view io -i2             # Explore I/O interactively
ures report --view tree --pid 12345 --full    # Focus a subtree; retain complete commands
ures report --view ps --sort mem -n10         # Single-process memory ranking
ures report --json             # A versioned machine-readable snapshot
ures report -i2 --json --count 3 # Three NDJSON snapshots
ures -h                        # Main commands and common options
ures report --help             # All report options, grouped by purpose
ures config path               # Find your configuration
ures config themes             # List built-in themes
```

`ures report --view NAME` selects an exact configured view key; `report`
without `--view` uses `default_view`. Add `-i` for repeated reports. Use `ures report --json` for script output. In a pipe, use `ures report` (add `-i` to repeat):
bare `ures` requires terminal input and output and explains this before creating
configuration. See the [command reference and migration notes](docs/cli.md).

Use `-h` for brief help or `--help` for all options at the current command/action.
Press `?` inside the TUI for key bindings.

CPU columns use **cores**, so `1.00` means one CPU. Tree CPU and TREE MEM include descendants; SELF MEM belongs to the root process. **Do not sum tree rows.** Memory defaults to PSS, which apportions shared pages; `*` marks RSS fallback, `~` marks partial coverage, and `?` means unavailable.

The overview uses a matching user cgroup when one is identifiable. Its charged memory includes cache and can differ from process PSS. Detection supports cgroup v1/v2, systemd user slices, login-user groups, namespace mount roots, inherited constraints and cpusets. A shared host/container limit is not presented as a personal allocation. Mixed scopes remain explicit; custom layouts can supply controller paths.

## Install and develop

Download the release archive for `x86_64-unknown-linux-musl` or `aarch64-unknown-linux-musl`, verify `SHA256SUMS`, then install the `ures` executable into a directory on `PATH`. Runtime collection needs Linux `/proc`; cgroup and kernel-log access are optional. No Python or Rust runtime is required. Directory-backed user names use the host `id` command if static libc cannot resolve NSS; numeric UID and current-user queries remain available without it.

Build with an existing **Rust 1.99.0** toolchain, rustfmt, Clippy and the matching musl target:

```bash
cd ~/local/src/ures
./build.sh

# Select an existing compiler environment explicitly:
URES_VENV="/path/to/existing-toolchain" ./build.sh

# Standard Cargo workflows are supported too:
cargo test --locked
cargo run -- --help
```

`build.sh` checks formatting, tests, Clippy, static linkage and CLI/PTY behavior before atomically installing `~/local/bin/ures`. Override `URES_INSTALL_DIR` or `URES_TARGET` as needed. It uses one build job by default, records sanitized output in `.logs/build.log`, remaps private compiler paths and cleans its own temporary build directory. An explicitly supplied `CARGO_TARGET_DIR` is preserved. Toolchain setup and environment files are never changed by the script.

## Make the view yours

The first normal invocation creates a fully commented TOML file at `${XDG_CONFIG_HOME}/ures/config.toml`, falling back to `${HOME}/.config/ures/config.toml` when XDG is absent, empty or relative. Existing files are never overwritten. Help, version and `config` commands do not create it or execute checks.

```bash
ures config show               # Effective configuration, including selected profile
ures config check              # Validate all profiles without executing checks
ures --view compact
ures --profile lab
ures --config ./team.toml
```

Partial configuration is enough. Maps merge by property; arrays replace. Runtime options override the selected profile, then global values, then built-ins.

```toml
# auto | manual | auto-or-manual
[limits]
mode = "auto"

# Explicitly select this budget with --profile lab.
[profiles.lab.limits]
mode = "manual"
cpu_cores = 4.0
memory = "8GiB"

[views.tree]
columns = ["pid", "cpu", "mem", "self_mem", "threads", "command"]
sort = "mem"
top = 15

[[groups]]
name = "Workers"
match = 'sample_worker\.py|sample-watcher'
```

Manual values are **configured budgets**, not kernel settings. Detected kernel bounds remain available independently. `auto-or-manual` uses a budget only when the corresponding user limit cannot be detected; plain `auto` does not silently use configured numbers.

Views choose `tree`, `ps`, `groups`, `oom` or `checks`, an optional display `name`, fields, sort, row count, and optional `include`/`exclude` regexes. An explicit `[views]` selects the available tabs; builtin view keys retain omitted field defaults. Profiles patch/add to that selection. Tree filtering retains ancestor context; inclusive totals still include hidden descendants. Ordered application rules use first-match wins, followed by executable-name grouping. Columns can change titles, widths and hiding priority. Themes support Ratatui color names, RGB hex colors and indexed colors. See the [view reference](docs/views.md), [complete template](config/default.toml) and [profile examples](examples/profiles.toml).

CPU, memory and OOM checks are enabled by default. I/O, threads and FDs are collected when a view needs them or their check is enabled. Thresholds are percentages for CPU/memory, and raw units for other numeric checks. OOM reports new kills separately from cumulative counters; an old kill does not keep the check critical forever.

Kernel logs are read only when the OOM check is enabled or an OOM view is needed:
any configured TUI tab, or the selected report view.

## Interactive mode

`ures` creates one tab for every resolved `views` entry. A top-level
`tab_order` array controls their order; unlisted views follow in key order.
The default template orders Tree, Processes, I/O, Groups, Compact, OOM, Checks.
Use `tab_order = []` for alphabetical order. `name` supplies the label; without
it the key is displayed. `default_view`
selects the initial tab, overridden by `--view`. Each view honors `top` (`0` for
all rows); `--top` overrides it across tabs. Switching tabs restores each view's
sort, search, folded branches, selection and scroll position. Long tab lists
scroll to keep the selected tab visible.

For example, this configuration creates exactly three tabs:

```toml
default_view = "workers"
tab_order = ["workers", "events", "health"]
[views.workers]
name = "Workers"
kind = "tree"
columns = ["pid", "cpu", "mem", "command"]
top = 0
[views.events]
name = "OOM history"
kind = "oom"
[views.health]
kind = "checks"
```

Column widths stay fixed between samples, sorting and filtering. PID reserves
seven characters and I/O rates eleven, including coverage markers. Commands use
the remaining width; long values use a Unicode ellipsis (`~` with `--ascii`).
Resizing the terminal can hide optional columns or compress the layout.

The default adapts the [Dracula Classic palette](https://github.com/dracula/dracula-theme#color-palette-oss)
with brighter secondary text, explicit foreground/background colors and a continuous
selection. CPU is green, memory yellow, reads cyan and writes orange. Rainbow colors
are enabled by default for PID/command cells, tied to process identity or group
name; metrics retain their semantic colors. Tree guides have their own color;
ancestor context stays dim. Selected rows use one continuous background and
foreground across cells, separators and trailing space. `NO_COLOR` and
`--color never` disable colors. Add overrides to your existing config, for example:

```toml
[columns.read]
width = 12
[columns.write]
width = 12
[theme]
name = "dracula"
rainbow = true
# Optional explicit overrides:
# cpu = "#50fa7b"
# selection_bg = "#44475a"
```

Use `ures --theme light` for Alucard Classic (ivory background), or
`ures --theme gruvbox` for warm Gruvbox colors. Persist the choice with
`[theme] name = "light"` (on separate lines). `--theme` overrides the selected
name; explicitly configured colors still override the palette. `foreground` and
`background` can be `"reset"` to inherit terminal defaults.

Profiles are user-defined patches of **all global settings**, including limits,
views, checks, intervals and theme settings. Select one with `--profile work`
or top-level `default_profile = "work"`. Built-in themes do not create profiles.
For example, `[profiles.work.theme] name = "gruvbox"` can accompany a work
profile's other overrides. `ures --profile work --theme light` retains those
other overrides while selecting the light palette. See [theme/profile precedence
and migration](docs/themes.md).

| Key | Action |
| --- | --- |
| Tab / Shift-Tab | Switch pages |
| ↑↓ / j k | Select and scroll |
| PageUp / PageDown | Jump ten rows |
| Home / End | First / last row |
| Space / ← / → | Fold or unfold a tree |
| Enter | Complete process details |
| c / m / i | Sort CPU / memory / logical I/O |
| / | Search current view content; Enter applies, Esc clears |
| p / r | Pause the display / request a fresh sample |
| R (Shift-r) | Toggle rainbow row colors for this session |
| ? / q / Esc | Help / exit; any key closes help or details |

Sampling and extensions run outside the input loop. A paused view retains its snapshot and age. Process identity includes its start time so a reused PID does not inherit a selection or CPU baseline. The interface is read-only and restores terminal mode on normal exit, Ctrl-C, termination and panic.

## Extend the checks

An external command returns a versioned JSON object containing status, summary and numeric metrics. No plugin runtime or dynamic library ABI is required.

```toml
[checks.zombies]
enabled = true
command = ["python3", "/path/to/ures/examples/check-zombies.py"]
interval_seconds = 10.0
timeout_seconds = 2.0
```

Commands are argument arrays, with no implicit shell. The runner supplies `URES_TARGET_USER`, `URES_TARGET_UID` and `URES_CONTEXT`. At most two commands run concurrently; each has a deadline and 64 KiB output bounds. Failed, stale or malformed results remain local to that check. Only configured, enabled extensions execute.

See the [check protocol](docs/extensions.md), [JSON Schema](docs/check.schema.json) and [snapshot format](docs/snapshots.md). CLI snapshots and TUI cards use the same check results.

## Measurement boundaries

- CPU is a sample delta, not lifetime `ps %CPU`. Short-lived processes can disappear between samples; cgroup accounting may capture work absent from the process list.
- PSS coverage and RSS fallback are explicit. Process memory, cgroup charge and inherited shared bounds have different scopes.
- Logical I/O is counted from observed thread deltas to avoid importing waited-for child history. It is a visible-task lower bound; block-storage counters do not measure complete NFS or disk throughput. FD sums count descriptors, not unique files.
- OOM history comes from available current-boot dmesg/journal records and can be truncated or inaccessible. Cumulative cgroup counts can outlive retained events. Kernel PIDs are historical and may belong to a different namespace.
- No root access is required for ordinary use. Inaccessible data stays unknown; the tool does not invoke sudo.

## Contribute and release

Start with [CONTRIBUTING.md](CONTRIBUTING.md) and the [architecture](docs/architecture.md). Tests use deterministic process/cgroup fixtures, real CLI smoke checks and pseudo-terminals; they do not trigger actual OOMs or modify quotas. The TUI preview is exported from the actual test backend; `cargo run --example preview` provides the corresponding fictional CLI session.

CI tests native Linux x86_64/aarch64 musl targets and produces archives with SHA256 checksums and third-party license notices. Matching `v<package-version>` tags enable the prepared release workflow. [CHANGELOG](CHANGELOG.md) · [MIT license](LICENSE).
