# Changelog

## 0.1.1 — 2026-11-06

- Add cumulative Used, Shared, Compressed and Cache colors to the memory meter,
  preserving its slim line style and compact overview.
- Show per-class memory totals alongside the complete cgroup charge and process
  PSS/RSS total, using the available counters for the selected user.
- Include reported reclaimable slab in Cache and expose optional reclaimable-slab
  and zswap counters in snapshots, with compatibility for existing configurations.
- Add theme overrides for shared, compressed and cached memory.
- Install builds to `~/.local/bin` by default; `URES_INSTALL_DIR` selects another
  installation directory.

## 0.1.0 — 2026-10-21

Initial release of ures, a Linux resource monitor focused on a single user.

- Interactive dashboard with process trees, process rankings, application groups,
  I/O, OOM history and health checks.
- CPU and memory usage with cgroup v1/v2 limit detection and configurable budgets.
- CPU throttling, PSS/RSS memory, thread counts, file descriptors and I/O rates.
- Configurable views, tab names and order, columns, filters and profiles.
- Dracula, Alucard light and Gruvbox themes with color overrides.
- Text reports and JSON/NDJSON snapshots for scripts and logging.
- External checks through a versioned JSON protocol.
- Static Linux executables for x86_64 and aarch64, with SHA256 checksums and
  third-party license notices.
