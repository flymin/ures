# Command interface

| Goal | Command | Behavior |
| --- | --- | --- |
| Explore interactively | `ures` | Open the TUI at the configured interval |
| Choose a starting tab | `ures --view io` | Open that configured TUI view |
| Change the palette | `ures --theme light` | Temporarily choose a built-in theme |
| Print the default view once | `ures report` | Overview and configured `default_view`, then exit |
| Print a configured view once | `ures report --view compact` | Any view, including OOM/checks |
| Export one snapshot | `ures report --json` | One complete JSON object |
| Export a bounded stream | `ures report --json -i2 --count 3` | Three NDJSON records |
| Discover config actions | `ures config` | Group help, without loading configuration |
| Inspect configuration | `ures config show` | Effective TOML, without collecting resources |
| Learn one action | `ures help config show` | Same as `ures config show --help` |

Bare `ures` always opens the TUI and requires terminal input and output and a
`TERM` other than `dumb`. In a pipe use `ures report` or `ures report --json`.
The TUI preflight fails before creating configuration or collecting resources.

`report` prints once by default. Add `-i TIME` for repeated snapshots and
`--count N` to stop after N snapshots. Repeated text redraws in a terminal and
appends in a pipe/file. JSON is always undecorated, one complete object per line
when repeated. See [the snapshot schema](snapshots.md). For interactive monitoring,
use the TUI; repeated reports remain useful for scripts and logging.

The TUI uses `interval_seconds` from configuration/profile unless `-i` is given.
TIME values accept seconds, including decimals, or `s`/`m`/`h`/`d` suffixes.
Refresh/sample intervals must be 0.05 seconds to 24 hours; `--since` accepts
positive durations up to 365 days. `--sample` sets the initial CPU interval.

Use configured view keys for every report:

```bash
ures report --view tree --pid 12345 --full
ures report --view ps --sort cpu -n10
ures report --view oom --since 24h
ures report --view checks
```

Both `ures --view NAME` and `ures report --view NAME` look up exact keys in
the resolved `views` map. `kind` chooses the renderer/metrics; `name` is only a
Tab label. Neither is used as an alternative lookup key. A missing key is an
error, with no search by kind or builtin fallback. Keys are defined in `[views]`
in the configuration. Omit `--view` to use `default_view`. The template provides
the example keys above; custom configurations may rename or omit them.

Resource options work before or after the command: `--user`, `--config`,
`--profile`, `--theme`, `--view`, `--sort`, `--top`, `--pid`, `--since`, `--sample`,
`--ascii` and `--color`. `--pid` requires a tree as the initial view. In the TUI,
`--top` applies across tabs and `--sort` changes the starting tab. Tabs still
come exclusively from `views`/`tab_order` and retain independent state.

`--width`/`--full` belong to text reports, and `--count` to repeated text/JSON
output. The TUI rejects them. JSON rejects CLI display-format flags `--full`,
`--width`, `--ascii` and `--color`. Use Enter in a process view for full details;
`?` shows every TUI key, including PageUp/PageDown, Home/End and view search.

## Configuration and help

Bare `ures config` shows action help; it does not implicitly dump TOML.

| Action | Result | Applicable options |
| --- | --- | --- |
| `config path` | Path, without reading/validating the file | `--config` |
| `config themes` | Embedded theme names, without loading config | None |
| `config show` | Effective TOML | Config overrides below |
| `config check` | Validate file, all profiles and proposed overrides | Config overrides below |

`show`/`check` accept `--config`, `--profile`, `--theme`, `--view`, `--sort`,
`--top`, `-i`/`--interval`, `--sample` and `--ascii`. Overrides are resolved in
memory without writing the file. Other options are rejected, including an
explicit default such as `--color auto`. Configuration commands, help and
version never create files or execute resource checks.

Help follows the same hierarchy as the commands:

| Need | Command |
| --- | --- |
| Main commands and common TUI options | `ures -h` |
| All TUI options, grouped by purpose | `ures --help` |
| Common report options and examples | `ures report -h` |
| All report options and output rules | `ures report --help` |
| Configuration actions | `ures config -h` |
| Common options for an action | `ures config show -h` |
| All options for an action, including overrides | `ures config show --help` |
| Interactive key bindings | Press `?` inside the TUI |

`-h` gives brief help; `--help` gives complete help for the selected command or
action. Small pages such as `config path` show the same help for both.
`ures help report` and `ures help config show` are equivalent to command/action
`--help`; append `-h` for the brief version. `ures help --help` explains help
syntax. Unknown commands/actions/flags are rejected even when requesting help.

## Migration during 0.1.0

| Removed or changed entry | Use instead |
| --- | --- |
| `ures tui` | `ures` |
| `ures watch` | `ures` for interaction; `ures report -i2` for automation |
| `ures --json` | `ures report --json` |
| `--limit N` | `--top N` or `-nN` |
| `ures tree` / `ps` / `groups` / `oom` / `checks` | `ures report --view NAME` with an existing config key |
| Bare `ures config` producing TOML | `ures config show` |
| `ures config views` | Read `views` and `tab_order` in the configuration |

Conventional short options remain. Existing configuration
files are never rewritten; command syntax selects presentation, and profiles
override settings. Normal completion/help/version exit with status 0, usage or
configuration errors with status 2, and signal exits with 128 + signal number.
The package version remains 0.1.0.
