# Architecture

```text
Linux proc/cgroup/log sources      Configured check commands
             │                               │
      identity + sampling             bounded worker pool
             │                               │
      disjoint process totals ──────── check results
             │                               │
             └──────── versioned Report ───────┘
                              │
                 CLI / NDJSON / interactive TUI
```

- `procfs` reads selected-user records, samples CPU and live-thread I/O by
  `(id, starttime)`, and derives inclusive totals in linear time.
- `cgroup` maps memberships through mount roots, recognizes user account domains,
  and keeps local constraints separate from effective ancestor bounds. It never
  sums overlapping groups or guesses that a shared container is a user allocation.
- `config` loads and validates a partial TOML patch plus an optional named profile.
  The embedded template is the source of default settings.
- `cli` resolves presentation explicitly: the default/TUI, one `report` (or repeated
  snapshots with `-i`), or configuration inspection. Bare root selects the TUI.
  Both interfaces select exact configured view keys; collection/rendering use
  the selected kind without hidden fallback views. A shared mode resolver
  validates option applicability and drives
  sampling/termination; stdout redirection never silently changes the default mode.
- `theme` owns the built-in palette registry and embedded `config/themes/*.toml`
  files. Config/profile patches retain sparse theme overrides until the final
  selector is known; palette defaults are resolved only afterward. `--theme`
  changes the selector without discarding explicit color overrides. Profiles
  remain user-defined patches of global configuration, independent of the registry.
- `engine` owns sample baselines and emits typed reports. The TUI worker replaces
  its latest snapshot rather than accumulating an unbounded queue.
- `checks` applies builtin thresholds and schedules at most two external commands.
  `oom` reads available current-boot log backends and attributes records only from
  explicit UID or a matching OOM context.
- `render` builds presentation rows, compact CLI tables and stable TUI tables.
  TUI widths depend on configuration and viewport rather than sampled values;
  the command column receives unused space. `tui` colors semantic overview spans
  and deterministic process-identity PID/command colors, independently of collection.
  Metrics retain semantic colors; selection explicitly overrides foreground and
  background for every cell, without per-span reverse-video backgrounds.
  It consumes reports outside the sampler thread; raw terminal lifetime has a
  restoration guard.
- TUI tabs come only from resolved view keys, with optional display labels and
  a validated `tab_order` array that controls their display order.
  Process, OOM and check views share per-key sort/filter/folding/selection/scroll
  state. Rendering and key capabilities depend on view kind, never page index.

New builtin metrics should extend the typed collection/coverage model first,
then analysis and presentation. Keep collectors injectable through data fixtures
and pure parsing functions. A scripting extension usually needs only the
[check protocol](extensions.md), not a new builtin or Rust ABI.

Kernel semantics: [proc/PSS](https://www.kernel.org/doc/html/latest/filesystems/proc.html),
[cgroup v2](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html),
[process I/O](https://man7.org/linux/man-pages/man5/proc_pid_io.5.html).
The per-thread I/O path avoids the process-level aggregation of waited-for
children; exit between samples still causes an unobserved tail.

Tests must remain read-only with respect to real workloads. Cgroup limit and OOM
tests use temporary fixtures, not writes to `/sys` or memory exhaustion. CLI
tests use temporary configuration homes; PTY tests check restoration after q,
Ctrl-C, SIGTERM and an induced test panic.
