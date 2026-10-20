# Snapshot format · version 1

`ures report --json` writes one JSON object. `ures report --json -i2` writes one complete
object per line (NDJSON), without terminal escapes or table truncation.
Use `--count N` to stop the stream after N snapshots. JSON output requires an explicit command; plain `ures` always opens the TUI.
Migrate old root JSON invocations to `ures report --json`.
It retains the complete selected-user process set and retained OOM source records;
view regexes, row limits and `--since` affect human presentation. JSON consumers
can filter the explicit event timestamps without losing the original records.

| Field | Meaning |
| --- | --- |
| `schema_version`, `version` | Data schema and package version |
| `sample_id` | Monotonic sequence within this invocation |
| `timestamp`, `unix_seconds` | Sample start, RFC3339 UTC and Unix seconds |
| `host`, `user` | Host and selected `{name, uid}` |
| `sample_seconds`, `collection_ms` | CPU/process sampling interval and collection cost |
| `summary` | User usage, selected limits, process totals and event deltas |
| `account` | Raw normalized cgroup counters/constraints, when available |
| `cpu_cgroup`, `memory_cgroup` | Exact accounting domains, or null |
| `processes` | Object keyed by PID; each record also contains its start tick |
| `checks` | Builtin and external check results, with source/time/age |
| `oom` | Retained current-boot events, readable sources and optional warning |
| `warnings` | Measurement scope or coverage limitations |

Summary CPU uses `cpu_cores`; memory uses `memory_bytes`. Each limit has a
numeric `value` (cores or bytes respectively), `source` and `accounting` scope.
Configured budgets are distinct from detected kernel bounds in `account`.
Process `cpu_percent` uses 100% per core; `rss_bytes`/`pss_bytes` are bytes.
Process totals `cpu` use the same percent convention; `mem` and `rss` are bytes.
I/O fields ending `_bps` are bytes/second. Thread/FD values are counts.

`summary.processes` is a disjoint sum of visible selected-user processes, excluding
the collector itself. `fallback`, `threads_read`, `fds_read` and `io_read` describe
coverage; `io_partial` marks missing task data. Inclusive tree totals are derived
for presentation and must not be added together. PSS totals need not equal a
cgroup's charged memory. Raw cgroup memory values are bytes, `cores` and
`own_cores` are core bounds, and CPU timing fields explicitly end in `_ns`.
`own_*` records constraints on the selected group; effective `cores`/`limit` can
also reflect stricter ancestors. An inherited shared-only bound is not selected
as a personal quota.

Absent/inaccessible metrics are null, not zero. Some derived totals may contain
only a readable subset; use coverage fields before interpreting them as complete.
PID/TID start ticks use the kernel's boot-relative clock ticks. Exited tasks and
counter resets cannot create a negative rate or inherit a previous PID's baseline.

OOM events use `kernel_pid`, deliberately distinct from live process IDs. Logs
may belong to a host PID namespace, and retained events can be fewer than a
cgroup's cumulative count. Event `source` indicates dmesg or journal.

Package updates may add optional fields within schema 1. Consumers should ignore
unknown fields and check `schema_version`; incompatible meaning changes require
a new schema version. The Rust library exposes the same typed report for custom
frontends; deserializing a report does not resume its sampler state.
