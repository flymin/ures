use crate::{
    config,
    model::{Report, Status},
    oom,
    procfs::{self, Totals},
    util,
};
use std::collections::{BTreeMap, BTreeSet};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub full: bool,
    pub ascii: bool,
    pub pid: Option<u32>,
    pub search: String,
    pub collapsed: BTreeSet<u32>,
}
#[derive(Clone, Debug)]
pub struct Row {
    pub key: String,
    pub pid: Option<u32>,
    pub totals: Totals,
    pub own_mem: u64,
    pub own_fallback: bool,
    pub command: String,
    pub context: bool,
    pub has_children: bool,
}
#[derive(Clone, Debug)]
pub struct Table {
    pub header: String,
    pub rows: Vec<Row>,
    pub lines: Vec<String>,
    pub total_rows: usize,
    pub notes: Vec<String>,
    /// Visible fields and their reserved terminal-cell widths.
    pub columns: Vec<(String, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableLayout {
    /// Compact CLI output measured from the current rows.
    Compact,
    /// Stable TUI columns; only viewport/configuration changes affect widths.
    Stable,
}

/// Terminal cells as painted by Ratatui, whose buffer advances per grapheme.
/// Whole-string widths can merge ligatures across grapheme boundaries.
pub fn cell_width(s: &str) -> usize {
    s.graphemes(true).map(UnicodeWidthStr::width).sum()
}

pub fn fit(s: &str, width: usize, ascii: bool) -> String {
    if cell_width(s) <= width {
        return s.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mark = if ascii { "~" } else { "…" };
    let mut result = String::new();
    let mut used = 0;
    for grapheme in s.graphemes(true) {
        let size = UnicodeWidthStr::width(grapheme);
        if used + size > width - 1 {
            break;
        }
        result.push_str(grapheme);
        used += size;
    }
    result.push_str(mark);
    result
}
fn pad(s: &str, width: usize, right: bool) -> String {
    let spaces = " ".repeat(width.saturating_sub(cell_width(s)));
    if right {
        format!("{spaces}{s}")
    } else {
        format!("{s}{spaces}")
    }
}

fn score(total: &Totals, sort: &str) -> f64 {
    match sort {
        "cpu" => total.cpu,
        "io" => total
            .read_bps
            .zip(total.write_bps)
            .map_or(-1.0, |(r, w)| r + w),
        "threads" => total.threads.map_or(-1.0, |v| v as f64),
        "fds" => total.fds.map_or(-1.0, |v| v as f64),
        _ => total.mem as f64,
    }
}

fn ancestor_context(visible: &mut BTreeSet<u32>, mut parent: impl FnMut(u32) -> Option<u32>) {
    for mut pid in visible.clone() {
        while let Some(ancestor) = parent(pid) {
            // Already-visible ancestors are either expanded by this walk or
            // visited by the outer loop. This also terminates cycles.
            if !visible.insert(ancestor) {
                break;
            }
            pid = ancestor;
        }
    }
}

pub fn rows(
    report: &Report,
    config: &config::Config,
    view: &config::View,
    options: &Options,
) -> Vec<Row> {
    let processes = &report.processes;
    let include = view
        .include
        .as_deref()
        .map(|p| regex::Regex::new(p).expect("validated regex"));
    let exclude = view
        .exclude
        .as_deref()
        .map(|p| regex::Regex::new(p).expect("validated regex"));
    let search = options.search.to_lowercase();
    let matches = |pid: &u32| {
        let command = processes[pid].command(true);
        include.as_ref().is_none_or(|p| p.is_match(&command))
            && exclude.as_ref().is_none_or(|p| !p.is_match(&command))
            && (search.is_empty()
                || command.to_lowercase().contains(&search)
                || pid.to_string().contains(&search))
    };
    if view.kind == "groups" {
        let rules: Vec<_> = config
            .groups
            .iter()
            .map(|g| {
                (
                    &g.name,
                    regex::Regex::new(&g.pattern).expect("validated regex"),
                )
            })
            .collect();
        let mut groups: BTreeMap<String, Totals> = BTreeMap::new();
        for p in processes.values().filter(|p| matches(&p.pid)) {
            let command = p.command(true);
            let name = rules
                .iter()
                .find(|(_, r)| r.is_match(&command))
                .map_or_else(|| p.group(), |(n, _)| (*n).clone());
            groups.entry(name).or_default().add(&Totals::own(p));
        }
        let mut result: Vec<_> = groups
            .into_iter()
            .map(|(name, total)| Row {
                key: name.clone(),
                pid: None,
                own_mem: total.mem,
                own_fallback: total.fallback > 0,
                totals: total,
                command: name,
                context: false,
                has_children: false,
            })
            .collect();
        result.sort_by(|a, b| {
            score(&b.totals, &view.sort)
                .total_cmp(&score(&a.totals, &view.sort))
                .then(a.key.cmp(&b.key))
        });
        return result;
    }
    let make = |pid: u32, totals: Totals, command: String, context: bool, has_children: bool| Row {
        key: format!("{pid}:{}", processes[&pid].start),
        pid: Some(pid),
        totals,
        own_mem: processes[&pid].mem(),
        own_fallback: processes[&pid].pss.is_none(),
        command,
        context,
        has_children,
    };
    if view.kind == "ps" {
        let mut result: Vec<_> = processes
            .values()
            .filter(|p| matches(&p.pid))
            .map(|p| make(p.pid, Totals::own(p), p.command(options.full), false, false))
            .collect();
        result.sort_by(|a, b| {
            score(&b.totals, &view.sort)
                .total_cmp(&score(&a.totals, &view.sort))
                .then(a.pid.cmp(&b.pid))
        });
        return result;
    }
    let children = procfs::children(processes);
    let totals = procfs::all_totals(processes);
    let mut visible: BTreeSet<_> = processes
        .keys()
        .filter(|pid| matches(pid))
        .copied()
        .collect();
    // Preserve ancestor context. Their inclusive totals deliberately include hidden descendants.
    ancestor_context(&mut visible, |pid| {
        procfs::parent(processes.get(&pid)?, processes)
    });
    let sort = |ids: &mut Vec<u32>| {
        ids.sort_by(|a, b| {
            score(&totals[b], &view.sort)
                .total_cmp(&score(&totals[a], &view.sort))
                .then(a.cmp(b))
        })
    };
    let mut roots: Vec<_> = if let Some(pid) = options.pid {
        if processes.contains_key(&pid) {
            vec![pid]
        } else {
            Vec::new()
        }
    } else {
        processes
            .values()
            .filter(|p| procfs::parent(p, processes).is_none())
            .map(|p| p.pid)
            .collect()
    };
    sort(&mut roots);
    let mut pending: Vec<_> = roots
        .into_iter()
        .rev()
        .map(|p| (p, String::new(), true, 0))
        .collect();
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    while let Some((pid, prefix, last, depth)) = pending.pop() {
        if !seen.insert(pid) || !visible.contains(&pid) {
            continue;
        }
        let mut kids: Vec<_> = children
            .get(&pid)
            .into_iter()
            .flatten()
            .copied()
            .filter(|p| *p != pid && visible.contains(p))
            .collect();
        sort(&mut kids);
        let is_collapsed = options.collapsed.contains(&pid);
        let branch = if depth == 0 {
            ""
        } else if options.ascii {
            if last { "`-" } else { "|-" }
        } else if last {
            "└─"
        } else {
            "├─"
        };
        let command = format!(
            "{prefix}{branch}{}{}",
            if kids.is_empty() {
                ""
            } else if is_collapsed {
                "[+] "
            } else {
                "[-] "
            },
            processes[&pid].command(options.full)
        );
        result.push(make(
            pid,
            totals[&pid].clone(),
            command,
            !matches(&pid),
            !kids.is_empty(),
        ));
        if is_collapsed {
            continue;
        }
        let next = if depth == 0 {
            prefix
        } else {
            format!(
                "{prefix}{}",
                if last || options.ascii { "  " } else { "│ " }
            )
        };
        // Cap indentation, not traversal, so deeply nested trees stay readable.
        let next = if depth >= 12 {
            if options.ascii {
                "... ".into()
            } else {
                "⋮ ".into()
            }
        } else {
            next
        };
        let n = kids.len();
        for (i, kid) in kids.into_iter().enumerate().rev() {
            pending.push((kid, next.clone(), i == n - 1, depth + 1));
        }
    }
    result
}

pub fn value(row: &Row, field: &str) -> String {
    let total = &row.totals;
    let count = |value: Option<u64>, read: usize| {
        value.map_or_else(
            || "?".into(),
            |v| format!("{v}{}", if read < total.count { "~" } else { "" }),
        )
    };
    let rate = |value: Option<f64>| {
        value.map_or_else(
            || "?".into(),
            |v| {
                format!(
                    "{}{}",
                    util::size(v as u64),
                    if total.io_partial || total.io_read < total.count {
                        "~"
                    } else {
                        ""
                    }
                )
            },
        )
    };
    match field {
        "pid" => row.pid.map_or_else(|| "-".into(), |v| v.to_string()),
        "cpu" => format!("{:.2}", total.cpu / 100.0),
        "mem" => format!(
            "{}{}",
            util::size(total.mem),
            if total.fallback > 0 { "*" } else { "" }
        ),
        "self_mem" => format!(
            "{}{}",
            util::size(row.own_mem),
            if row.own_fallback { "*" } else { "" }
        ),
        "rss" => util::size(total.rss),
        "processes" => total.count.to_string(),
        "threads" => count(total.threads, total.threads_read),
        "fds" => count(total.fds, total.fds_read),
        "read" => rate(total.read_bps),
        "write" => rate(total.write_bps),
        "command" => row.command.clone(),
        _ => "?".into(),
    }
}

pub fn title(field: &str, kind: &str) -> &'static str {
    match field {
        "pid" => "PID",
        "cpu" => "CPU(c)",
        "mem" if kind == "tree" => "TREE MEM",
        "mem" => "MEM(PSS)",
        "self_mem" => "SELF MEM",
        "rss" => "RSS",
        "processes" => "PROCS",
        "threads" => "THR",
        "fds" => "FD",
        "read" => "READ/s",
        "write" => "WRITE/s",
        "command" => "COMMAND",
        _ => "?",
    }
}

pub fn table(
    report: &Report,
    config: &config::Config,
    view: &config::View,
    options: &Options,
    width: usize,
) -> Table {
    table_with_layout(report, config, view, options, width, TableLayout::Compact)
}

pub fn table_with_layout(
    report: &Report,
    config: &config::Config,
    view: &config::View,
    options: &Options,
    width: usize,
    layout: TableLayout,
) -> Table {
    let mut rows = rows(report, config, view, options);
    let total_rows = rows.len();
    if view.top > 0 {
        rows.truncate(view.top);
    }
    let mut fields = view.columns.clone();
    let mut widths = BTreeMap::new();
    let mut titles = BTreeMap::new();
    for field in &fields {
        let column = config.columns.get(field).cloned().unwrap_or_default();
        let label = column
            .title
            .clone()
            .unwrap_or_else(|| title(field, &view.kind).into());
        let natural = match layout {
            TableLayout::Compact => rows
                .iter()
                .map(|row| cell_width(value(row, field).as_str()))
                .max()
                .unwrap_or(0),
            TableLayout::Stable => column.width.unwrap_or(match field.as_str() {
                "pid" => 7,
                "cpu" => 8,
                "mem" | "self_mem" | "rss" | "read" | "write" => 11,
                "command" => 12,
                _ => 7,
            }),
        }
        .max(cell_width(label.as_str()));
        let default_max = match layout {
            TableLayout::Stable => width.max(1),
            TableLayout::Compact if field == "command" => 80,
            TableLayout::Compact => 18,
        };
        // User minima override compact defaults when space permits. No field
        // can occupy more than the viewport, even with an enormous minimum.
        let max = column
            .max_width
            .unwrap_or(default_max.max(column.min_width))
            .min(width.max(1));
        widths.insert(field.clone(), natural.max(column.min_width).min(max));
        titles.insert(field.clone(), label);
    }
    let total = |fields: &Vec<String>, widths: &BTreeMap<String, usize>| {
        fields.iter().map(|f| widths[f]).sum::<usize>() + fields.len().saturating_sub(1) * 2
    };
    while total(&fields, &widths) > width {
        let optional = fields
            .iter()
            .filter(|f| config.columns.get(*f).is_some_and(|c| c.optional))
            .min_by_key(|f| config.columns[*f].priority)
            .cloned();
        if let Some(field) = optional {
            fields.retain(|f| f != &field);
            continue;
        }
        let shrink = fields
            .iter()
            .filter(|f| {
                widths[*f]
                    > config
                        .columns
                        .get(*f)
                        .map_or(if f.as_str() == "command" { 12 } else { 4 }, |c| {
                            c.min_width
                        })
            })
            .max_by_key(|f| widths[*f])
            .cloned();
        let Some(field) = shrink else {
            break;
        };
        *widths.get_mut(&field).unwrap() -= 1;
    }
    if layout == TableLayout::Stable && fields.iter().any(|f| f == "command") {
        let spare = width.saturating_sub(total(&fields, &widths));
        let column = config.columns.get("command").cloned().unwrap_or_default();
        let reserved = widths["command"];
        widths.insert(
            "command".into(),
            (reserved + spare).min(column.max_width.unwrap_or(width)),
        );
    }
    let make_line = |values: Vec<String>| {
        let line = fields
            .iter()
            .zip(values)
            .map(|(f, text)| {
                if options.full && f == "command" {
                    text
                } else {
                    pad(
                        &fit(&text, widths[f], options.ascii),
                        widths[f],
                        f != "command",
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        if options.full {
            line.trim_end().to_owned()
        } else {
            fit(line.trim_end(), width, options.ascii)
        }
    };
    let header = make_line(fields.iter().map(|f| titles[f].clone()).collect());
    let lines = rows
        .iter()
        .map(|row| make_line(fields.iter().map(|f| value(row, f)).collect()))
        .collect();
    let mut notes = vec![format!(
        "{} of {total_rows} rows | sorted by {} | CPU in cores | MEM=PSS; *=RSS fallback; ~=partial",
        rows.len(),
        view.sort
    )];
    if options
        .pid
        .is_some_and(|pid| !report.processes.contains_key(&pid))
    {
        notes.push("Selected PID is no longer visible for this user.".into());
    }
    Table {
        header,
        rows,
        lines,
        total_rows,
        notes,
        columns: fields.iter().map(|f| (f.clone(), widths[f])).collect(),
    }
}

pub(crate) fn gauge(ratio: Option<f64>, ascii: bool) -> String {
    let Some(ratio) = ratio else {
        return if ascii {
            "[----------]"
        } else {
            "[··········]"
        }
        .into();
    };
    let filled = (ratio.clamp(0.0, 1.0) * 10.0).round() as usize;
    format!(
        "[{}{}]",
        if ascii { "#" } else { "━" }.repeat(filled),
        if ascii { "-" } else { "·" }.repeat(10 - filled)
    )
}

pub fn summary(report: &Report, ascii: bool) -> Vec<String> {
    let s = &report.summary;
    let cpu = s.cpu_limit.value.map(|v| s.cpu_cores / v);
    let mem = s.memory_limit.value.map(|v| s.memory_bytes as f64 / v);
    let p = &s.processes;
    let mut lines = vec![
        format!(
            "ures  {}@{}  uid={}  sample={:.2}s  {}",
            report.user.name,
            report.host,
            report.user.uid,
            report.sample_seconds,
            util::timestamp(report.unix_seconds, false)
        ),
        format!(
            "CPU  {}  {:.2} / {} cores  {}  [{}]",
            gauge(cpu, ascii),
            s.cpu_cores,
            s.cpu_limit
                .value
                .map_or_else(|| "?".into(), |v| format!("{v:.2}")),
            cpu.map_or_else(|| "n/a".into(), |v| format!("{:.1}%", v * 100.0)),
            s.cpu_limit.source
        ),
        format!(
            "MEM  {}  {} / {}  {}  [{}]",
            gauge(mem, ascii),
            util::size(s.memory_bytes),
            s.memory_limit
                .value
                .map_or_else(|| "?".into(), |v| util::size(v as u64)),
            mem.map_or_else(|| "n/a".into(), |v| format!("{:.1}%", v * 100.0)),
            s.memory_limit.source
        ),
        format!(
            "USER {} processes | MEM accounting: {} | process PSS/RSS {} ({}/{} PSS readable)",
            p.count,
            s.memory_source,
            util::size(p.mem),
            p.count - p.fallback,
            p.count
        ),
        format!(
            "CPU throttle {}/{} periods, {}ms | OOM {} cumulative, {} new this sample",
            s.throttled_periods
                .map_or_else(|| "?".into(), |v| v.to_string()),
            s.periods.map_or_else(|| "?".into(), |v| v.to_string()),
            s.throttle_ms
                .map_or_else(|| "?".into(), |v| format!("{v:.1}")),
            report
                .account
                .oom
                .map_or_else(|| "?".into(), |v| v.to_string()),
            s.oom_new.map_or_else(|| "?".into(), |v| v.to_string())
        ),
    ];
    if report.account.memory.is_some() {
        let show = |v: Option<u64>| v.map_or_else(|| "?".into(), util::size);
        lines.push(format!(
            "CHARGE anon {} | shared {} | file cache {} | peak {}",
            show(report.account.anon),
            show(report.account.shmem),
            show(report.account.file),
            show(report.account.peak)
        ));
    }
    if s.cpu_limit.source == "configured budget" || s.memory_limit.source == "configured budget" {
        lines.push(format!(
            "DETECTED kernel bounds: CPU {} cores; MEM {}",
            report
                .account
                .cores
                .map_or_else(|| "?".into(), |v| format!("{v:.2}")),
            report.account.limit.map_or_else(|| "?".into(), util::size)
        ));
    }
    lines
}

struct TextRow {
    content: String,
    display: String,
}
impl TextRow {
    fn plain(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            content: text.clone(),
            display: text,
        }
    }
}

// Filter complete content before field clipping, row limits and viewport fitting.
fn text_lines(
    rows: Vec<TextRow>,
    view: Option<&config::View>,
    search: &str,
    width: usize,
    ascii: bool,
) -> Vec<String> {
    let include = view
        .and_then(|v| v.include.as_ref())
        .map(|v| regex::Regex::new(v).expect("validated view"));
    let exclude = view
        .and_then(|v| v.exclude.as_ref())
        .map(|v| regex::Regex::new(v).expect("validated view"));
    let search = search.to_lowercase();
    let limit = view
        .filter(|v| v.top > 0)
        .map_or(usize::MAX, |v| v.top.saturating_add(1));
    rows.into_iter()
        .enumerate()
        .filter(|(index, row)| {
            *index == 0
                || (row.content.to_lowercase().contains(&search)
                    && include.as_ref().is_none_or(|r| r.is_match(&row.content))
                    && exclude.as_ref().is_none_or(|r| !r.is_match(&row.content)))
        })
        .take(limit)
        .map(|(_, row)| fit(&row.display, width, ascii))
        .collect()
}

/// Shared CLI/TUI rendering for configured OOM and check views.
pub fn text_view(
    report: &Report,
    view: &config::View,
    since: Option<f64>,
    search: &str,
    width: usize,
    ascii: bool,
) -> Vec<String> {
    let rows = match view.kind.as_str() {
        "oom" => oom_rows(report, since, 0, ascii),
        "checks" => check_rows(report, ascii),
        _ => return Vec::new(),
    };
    text_lines(rows, Some(view), search, width, ascii)
}

pub fn oom_lines(
    report: &Report,
    since: Option<f64>,
    top: usize,
    width: usize,
    ascii: bool,
) -> Vec<String> {
    text_lines(oom_rows(report, since, top, ascii), None, "", width, ascii)
}

fn oom_rows(report: &Report, since: Option<f64>, top: usize, ascii: bool) -> Vec<TextRow> {
    let mut lines = vec![TextRow::plain(
        "TIME (local, offset)        KERNEL PID  COMMAND            RSS        SHMEM",
    )];
    let events = oom::since(&report.oom.events, since);
    let n = if top == 0 { events.len() } else { top };
    for event in events.into_iter().take(n) {
        let line = |command: &str| {
            format!(
                "{}  {:>10}  {}  {:>9}  {:>9}",
                util::timestamp(event.unix_seconds, false),
                event.kernel_pid,
                pad(command, 18, false),
                event.rss_bytes.map_or_else(|| "?".into(), util::size),
                event.shmem_bytes.map_or_else(|| "?".into(), util::size)
            )
        };
        lines.push(TextRow {
            content: line(&event.command),
            display: line(&fit(&event.command, 18, ascii)),
        });
    }
    if lines.len() == 1 {
        lines.push(TextRow::plain(
            "No matching retained OOM kill records. This does not prove OOM never occurred.",
        ));
    }
    if let Some(warning) = &report.oom.warning {
        lines.push(TextRow::plain(warning));
    }
    lines
}

pub fn check_lines(report: &Report, width: usize, ascii: bool) -> Vec<String> {
    text_lines(check_rows(report, ascii), None, "", width, ascii)
}

fn check_rows(report: &Report, ascii: bool) -> Vec<TextRow> {
    let mut lines = vec![TextRow::plain("STATUS   CHECK             RESULT")];
    for check in &report.checks {
        let line = |id: &str| {
            format!(
                "{:<8} {} {}{}",
                check.status.label(),
                pad(id, 17, false),
                check.summary,
                if check.age_seconds >= 1.0 {
                    format!(" (age {:.0}s)", check.age_seconds)
                } else {
                    String::new()
                }
            )
        };
        lines.push(TextRow {
            content: line(&check.id),
            display: line(&fit(&check.id, 17, ascii)),
        });
        if !check.metrics.is_empty() {
            lines.push(TextRow::plain(format!(
                "         {}",
                check
                    .metrics
                    .iter()
                    .map(|m| format!(
                        "{}={} {}",
                        m.id,
                        m.value.map_or_else(|| "?".into(), |v| format!("{v:.2}")),
                        m.unit
                    ))
                    .collect::<Vec<_>>()
                    .join(" | ")
            )));
        }
    }
    lines
}

pub fn colorize(text: &str, status: Status, config: &config::Config, color: bool) -> String {
    if !color {
        return text.into();
    }
    let color = match status {
        Status::Ok => &config.theme.accent,
        Status::Warn => &config.theme.warning,
        Status::Critical | Status::Error => &config.theme.critical,
        Status::Unknown => &config.theme.muted,
    };
    let parsed: ratatui::style::Color = color.parse().unwrap_or_default();
    match parsed {
        ratatui::style::Color::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m{text}\x1b[0m"),
        ratatui::style::Color::Indexed(n) => format!("\x1b[38;5;{n}m{text}\x1b[0m"),
        _ => {
            let ansi = match parsed {
                ratatui::style::Color::Red => 31,
                ratatui::style::Color::Green => 32,
                ratatui::style::Color::Yellow => 33,
                ratatui::style::Color::Blue => 34,
                ratatui::style::Color::Magenta => 35,
                ratatui::style::Color::Cyan => 36,
                ratatui::style::Color::White => 97,
                ratatui::style::Color::LightRed => 91,
                ratatui::style::Color::LightGreen => 92,
                ratatui::style::Color::LightYellow => 93,
                ratatui::style::Color::LightBlue => 94,
                ratatui::style::Color::LightMagenta => 95,
                ratatui::style::Color::LightCyan => 96,
                ratatui::style::Color::Reset => 39,
                ratatui::style::Color::Black => 30,
                ratatui::style::Color::DarkGray => 90,
                _ => 37,
            };
            format!("\x1b[{ansi}m{text}\x1b[0m")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestor_context_visits_deep_chains_without_repeated_walks() {
        const N: u32 = 4096;
        for mut visible in [
            (1..=N).collect::<BTreeSet<_>>(),
            [N / 2, N].into(),
            [N].into(),
        ] {
            let mut lookups = 0;
            ancestor_context(&mut visible, |pid| {
                lookups += 1;
                (pid > 1).then(|| pid - 1)
            });
            assert_eq!(visible, (1..=N).collect());
            assert!(lookups <= 2 * N);
        }
        let mut visible = [2].into();
        ancestor_context(&mut visible, |pid| match pid {
            1 => Some(2),
            2 => Some(3),
            3 => Some(1),
            _ => None,
        });
        assert_eq!(visible, [1, 2, 3].into());
    }

    #[test]
    fn unicode_truncation_preserves_display_width() {
        assert_eq!(fit("你好 world", 6, false), "你好 …");
        assert!(UnicodeWidthStr::width(fit("进程🙂 something", 8, false).as_str()) <= 8);
        assert_eq!(fit("abcdef", 1, true), "~");
        assert_eq!(fit("👩‍💻👩‍💻👩‍💻", 5, false), "👩‍💻👩‍💻…");
        assert_eq!(fit("e\u{301}🇨🇳👩‍💻tail", 4, false), "e\u{301}🇨🇳…");
        assert_eq!(cell_width("لالالاX"), 7);
        assert_eq!(fit("لالالاX", 4, false), "لال…");
        for width in 0..10 {
            assert!(cell_width(&fit("لالالاX", width, false)) <= width);
        }
    }
}
