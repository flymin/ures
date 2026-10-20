# Views and TUI tabs

The TUI reads its tabs exclusively from the resolved `views` map. Top-level
`tab_order` lists keys in the desired order; unlisted views follow in key order.
The default is Tree, Processes, I/O, Groups, Compact, OOM, Checks. Use an empty
array for alphabetical order. Duplicate or unknown keys are rejected, including
in unselected profiles.
There are no additional built-in pages. Each entry has a unique key, an optional
`name` label and a `kind`: `tree`, `ps`, `groups`, `oom` or `checks`. Omit `name`
to display the key. Labels may contain Unicode; blank/control-character labels
and keys are rejected. Labels need not be unique because state belongs to keys.

```toml
default_view = "workers"
tab_order = ["workers", "disk", "events", "health"]

[views.workers]
name = "Workers"
kind = "tree"
columns = ["pid", "cpu", "mem", "self_mem", "command"]
sort = "mem"
top = 0
include = 'python|torchrun'

[views.disk]
name = "Disk I/O"
kind = "ps"
columns = ["pid", "read", "write", "command"]
sort = "io"

[views.events]
name = "OOM history"
kind = "oom"

[views.health]
kind = "checks"
```

This defines exactly four tabs. Tree/ps/groups require nonempty, unique valid
columns. OOM/checks do not require columns; they use the same configuration,
selection and tab state mechanism with their own content renderer. `sort` defaults
to `mem`, `top` to `20`; sorting keys apply to process tables. `top = 0` shows all
rows. OOM/checks search and include/exclude regexes filter complete content before
the row limit and display clipping (including check metric lines). Resizing never
changes which content matches. OOM time filtering uses `--since`.
Column `min_width` applies to both CLI and TUI, overriding implicit compact
width caps when space permits. An explicit `max_width` remains authoritative;
fields and rows are clipped when the viewport cannot fit the configured minima.
`--full` requests complete command text, bypassing command and final row clipping.
`ures --view events` and `ures --view health` open those TUI tabs. Use
`ures report --view events` or `ures report --view health` for one text report,
and `ures report --view NAME -i2` for repeated reports.
Both the TUI and `report --view NAME` select an exact resolved view key,
never a `kind` or display label. View keys remain unrestricted: a key named
`ps` may have `kind = "tree"`. Missing keys fail with a pointer to `[views]`
in the configuration; there is no search by kind or builtin CLI fallback.

`default_view` chooses the first active tab; `ures --view disk` overrides it.
It also selects the view for `report` when `--view` is omitted.
An explicitly invalid `default_view` is rejected. If a custom view set omits
`default_view` and does not contain the inherited default, its first ordered key is used.
`--top` overrides row limits in every tab, while `--sort` changes the initial view.
Returning to a tab restores its sort, search, collapsed PIDs, selected process
identity and scroll position. Multiple views of the same kind remain independent.
Pause and rainbow are session-wide settings. Hidden tabs keep state; they do not
start additional samplers. I/O/FD/thread collection follows the active view and
enabled checks. Narrow terminals scroll the tab strip to keep the selected label
visible, with Unicode-safe truncation for a label wider than the viewport.

## Defaults and compatibility

- Without a root `views` table, the full embedded view set is loaded.
- With an explicit root `views` table, only its keys are loaded. A partial builtin
  entry, such as `[views.io]`, inherits that entry's field defaults, while its
  omitted `name` falls back to `io`. Custom process views must supply their kind
  and columns. Empty view sets, unknown kinds and invalid columns are rejected.
- If `tab_order` is omitted, the inherited default order is filtered to the
  selected view set, with custom keys appended alphabetically. Explicit orders
  must reference configured view keys, rather than display names. Profiles may
  override the array with `[profiles.NAME] tab_order = [...]` (on separate lines).
- Profiles keep the established patch semantics: they can modify or add views
  to the selected root set. All profiles are validated even when unselected.
- Existing schema-1 configurations remain valid without `name` or new theme
  settings. An old five-view config produces those five tabs; OOM/Checks appear
  only after adding entries for them. Existing files are never rewritten.
- To add OOM and Checks to an existing config, append `[views.oom]` with
  `kind = "oom"` and `[views.checks]` with `kind = "checks"`. Add `name` as desired.
  Reports use those same keys with `ures report --view oom` or
  `ures report --view checks`; absent keys are unavailable in both interfaces.

See [default.toml](../config/default.toml) for the complete template and
[README.zh-CN.md](../README.zh-CN.md#交互操作) for Chinese usage examples.
