# Changelog

Command interface: bare `ures` opens the TUI; `report` exports a snapshot,
with `-i` for repeated text/NDJSON output. Remove redundant `watch`/`tui` commands, the hidden `--limit` alias and the
implicit root JSON mode; export with `ures report --json`. Bare `config` lists
actions; per-action help and strict option applicability avoid ignored flags.
Remove kind-based CLI shortcuts and their builtin fallbacks; both TUI and
reports select exact configured view keys via --view, or use default_view. Add command-specific help and early
non-terminal guidance; reject options that have no effect in the chosen mode.

Publication privacy: replace environment-derived fixtures and previews with
fictional data; add a non-disclosing source/binary/archive privacy gate, explicit
release inputs and Cargo source includes; withhold live smoke-test captures and
upload only the expected release artifacts. Version remains 0.1.0.

## 0.1.0 — ongoing development

The package version stays at 0.1.0 throughout this iteration.

- Brief `-h` and complete `--help` at each command/action level, with options
  grouped by purpose and limited to the selected entry point.
- Remove the redundant `config views` action; views and tab order remain in
  configuration and its effective `config show` output.
- Skip kernel-log commands when the OOM check is disabled and no available
  TUI tab (or selected report view) needs OOM history.
- Recheck process/thread identities around UID, resource and I/O reads, including
  the CPU baseline; retain processes with non-UTF-8 names via lossy decoding.
- Stop ancestor-context walks at already included nodes to avoid quadratic
  rendering work for deep process trees.
- Independent built-in themes via `[theme] name`, per-color overrides and
  `--theme`; profiles exclusively contain user-defined global config overrides.
- `ures config themes` discovery, explicit selector/color precedence and
  migration hints for former implicit theme profiles.
- Dracula Classic default colors with brighter secondary text, explicit frame
  foreground/background, Alucard light and Gruvbox alternative themes.
- Matching popup backgrounds, text contrast checks and actual TUI previews.
- Shared CLI/TUI filters and row limits for OOM and check views.
- Account-domain effective cpuset bounds, sanitized executable group labels,
  and external check freshness measured from worker completion.
- Required nullable metric values and target-filtered offline release packaging.
- Grapheme-aware truncation and TUI column styling preserve joined emoji and
  following metric values in custom column orders.
- Terminal restoration also covers failures during partial initialization.
- CLI and TUI use exact configured view keys, including keys that differ from
  their kind. Missing views fail without silently choosing a builtin.
- OOM/check filtering uses complete content before field/viewport clipping;
  headers and empty states also respect narrow output widths.
- Unicode OOM commands and check names use terminal-cell padding so following
  result and memory columns stay aligned.
- Parentheses and UID-like text in killed task names cannot truncate OOM
  commands or misattribute records to another user; context-like task names
  preserve the matching UID context for legacy records.
- Consistent grapheme-cell measurements and span positioning preserve metrics
  after contextual Unicode ligatures, including Arabic commands and labels.
- Configured column minima override implicit compact CLI caps when space is
  available; viewport bounds prevent oversized padding allocations.

- Configurable top-level `tab_order`, profile overrides, deterministic ordering
  of unlisted views and validation of duplicate/unknown keys.
- Default tab order: Tree, Processes, I/O, Groups, Compact, OOM, Checks.
- Green CPU, yellow memory, cyan reads and orange writes; balanced identity colors
  for PID/command cells with consistent metric colors.
- Continuous selection foreground/background across cells, gaps and line ends;
  configurable selection colors and a matching light theme.

- Fully configuration-driven TUI tabs, including I/O, Compact, custom views,
  OOM and Checks; optional display names and independent per-view state.
- Explicit view sets select available tabs; legacy partial builtin overrides
  still inherit field defaults and old configs require no new fields.
- Stable TUI column reserves for PIDs, rates and other metrics; elastic command
  width and Unicode-safe ellipsis instead of layout changes between samples.
- Configurable TUI column `width`, in addition to existing minimum/maximum widths.
- Distinct overview colors for CPU, memory, accounting, throttling and OOM;
  pressure colors follow configured warning and critical thresholds.
- Harmonious process-identity rainbow colors, separately colored tree guides,
  a Shift-R toggle and monochrome support. Existing configs inherit new defaults.

- User-scoped CPU and memory overview with automatic limits and manual budgets.
- Inclusive process trees, single-process rankings and configurable application groups.
- CPU throttling, current-boot OOM history, logical/storage I/O, threads and FD metrics.
- TOML profiles, views, columns, regex filters and terminal themes.
- Text/NDJSON reports and a read-only interactive TUI.
- Versioned external checks with bounded concurrency, output and execution time.
- Static Linux x86_64/aarch64 build and release workflows, fixtures and PTY tests.
