# External check protocol · version 1

A check is an explicitly configured command, not a shell fragment. It can be
written in any language. The same result appears in CLI, JSON and TUI cards.

```json
{
  "schema_version": 1,
  "status": "ok",
  "summary": "No zombie processes",
  "metrics": [{"id": "zombies", "value": 0, "unit": "count"}]
}
```

Required fields are exactly `schema_version`, `status`, `summary` and `metrics`.
Status is `ok`, `warn`, `critical`, `unknown` or `error`. A metric has a unique,
nonempty `id`, a finite numeric `value` (or `null` when unknown), and a nonempty
`unit`. Control characters are rejected in metric names/units and removed from
displayed summaries. Use `bytes`, `cores`, `percent`, `count` or
`bytes/second` where applicable; custom units are allowed. The [JSON Schema](check.schema.json)
describes the payload; the runner additionally validates finite values and IDs.

```toml
[checks.zombies]
enabled = true
command = ["python3", "/absolute/path/check-zombies.py"]
interval_seconds = 10.0
timeout_seconds = 2.0
```

The command inherits its caller's working directory and environment. No shell,
tilde expansion or string interpolation is performed. Explicitly use
`["sh", "-c", "..."]` only when shell syntax is desired. The runner adds:

| Environment variable | Value |
| --- | --- |
| `URES_TARGET_USER` | Selected user label |
| `URES_TARGET_UID` | Numeric target UID; use this for process filtering |
| `URES_CONTEXT` | JSON object with schema version, timestamp, host, user, sample interval and summary |

Standard input is closed. Write exactly one JSON object to stdout and exit zero.
Stderr is diagnostic. Nonzero exit, malformed JSON, version mismatch, output
overflow or deadline expiration produces an `error` for that check. Stdout and
stderr are each bounded to 64 KiB. Numeric budgets and thresholds in the core
configuration do not change a plugin's status; a plugin owns its own assessment.

At most two checks run simultaneously. Every command runs in a monitor-owned
process group so timeout/cancellation can clean up its descendants without
signalling the target workload. Long-running services are not check commands.
Queries and TUI sampling continue while commands run. Results have update time
and age; overdue data is marked unknown/stale. One-shot CLI calls wait for the
first batch of results, bounded to 60 seconds overall; additional pending results
remain unknown. No historical storage or background daemon is installed.

External checks default to absent. `config check`, help and version never run
them. The [zombie-count example](../examples/check-zombies.py) uses only the Python
standard library and `ps`; it does not modify processes.
