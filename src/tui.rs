use crate::{Result, cli, config, engine, model::Report, render, util};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal;
use std::sync::atomic::Ordering;
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// Share terminal initialization/restoration with alternate frontends. Unwinding
/// runs the same restoration guard as the interactive application.
pub fn with_terminal(
    operation: impl FnOnce(&mut ratatui::DefaultTerminal) -> Result<()>,
) -> Result<()> {
    // Initialization can enable raw mode and then fail while writing/querying
    // the terminal. Install cleanup before the first fallible operation.
    let _restore = Restore;
    let mut terminal = ratatui::try_init().map_err(|e| e.to_string())?;
    operation(&mut terminal)
}

struct ViewState {
    view: config::View,
    selected: usize,
    key: Option<String>,
    collapsed: BTreeSet<u32>,
    search: String,
    state: ListState,
}

struct App {
    tabs: Vec<String>,
    retained_views: BTreeMap<String, ViewState>,
    page: usize,
    view: config::View,
    options: cli::Options,
    selected: usize,
    key: Option<String>,
    collapsed: BTreeSet<u32>,
    search: String,
    editing: bool,
    paused: bool,
    help: bool,
    detail: Option<String>,
    report: Option<Report>,
    table: Option<render::Table>,
    state: ListState,
    color: bool,
    rainbow: bool,
}

// A deterministic hash keeps colors tied to PID + start time (or group name),
// including across filtering, folding, sorting and application restarts.
fn identity_color<'a>(key: &str, palette: &'a [String]) -> &'a str {
    let hash = key.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    &palette[(hash % palette.len() as u64) as usize]
}

fn take_cells(text: &str, width: usize) -> (&str, &str) {
    let mut used = 0;
    for (offset, grapheme) in text.grapheme_indices(true) {
        used += render::cell_width(grapheme);
        if used > width {
            return text.split_at(offset);
        }
    }
    (text, "")
}

// Ratatui measures each Span as a whole string when positioning later spans.
// Split only context-sensitive ligatures so its span and buffer widths agree.
fn cell_line(mut line: Line<'static>) -> Line<'static> {
    line.spans = line
        .spans
        .into_iter()
        .flat_map(|span| {
            if span.width() == render::cell_width(span.content.as_ref()) {
                vec![span]
            } else {
                span.content
                    .graphemes(true)
                    .map(|g| Span::styled(g.to_owned(), span.style))
                    .collect()
            }
        })
        .collect();
    line
}

impl App {
    fn new(
        config: &config::Config,
        options: cli::Options,
        mut view: config::View,
        color: bool,
    ) -> Self {
        if let Some(top) = options.top {
            view.top = top;
        }
        let tabs = config.view_keys();
        let page = tabs
            .iter()
            .position(|key| key == &config.default_view)
            .unwrap_or(0);
        Self {
            tabs,
            retained_views: BTreeMap::new(),
            page,
            view,
            options,
            selected: 0,
            key: None,
            collapsed: BTreeSet::new(),
            search: String::new(),
            editing: false,
            paused: false,
            help: false,
            detail: None,
            report: None,
            table: None,
            state: ListState::default(),
            color,
            rainbow: config.theme.rainbow,
        }
    }
    fn label(&self) -> &str {
        self.view.label(&self.tabs[self.page])
    }
    fn visible_tabs(&self, config: &config::Config, width: usize) -> (Vec<String>, usize) {
        let labels: Vec<_> = self
            .tabs
            .iter()
            .map(|key| {
                render::fit(
                    config.views[key].label(key),
                    width.saturating_sub(2),
                    config.ascii,
                )
            })
            .collect();
        let mut start = 0;
        let used = |start: usize| {
            labels[start..=self.page]
                .iter()
                .map(|s| render::cell_width(s.as_str()) + 3)
                .sum::<usize>()
                .saturating_sub(1)
        };
        while start < self.page && used(start) > width {
            start += 1;
        }
        // Ratatui truncates titles at the right edge. Scrolling this window
        // ensures the selected tab remains visible even with many custom tabs.
        (labels.into_iter().skip(start).collect(), self.page - start)
    }
    fn style(&self, color: &str) -> Style {
        if self.color {
            Style::default().fg(color.parse().unwrap_or(Color::Reset))
        } else {
            Style::default()
        }
    }
    fn base_style(&self, config: &config::Config) -> Style {
        if self.color {
            self.style(&config.theme.foreground).bg(config
                .theme
                .background
                .parse()
                .unwrap_or(Color::Reset))
        } else {
            Style::default()
        }
    }
    fn pressure_style(&self, config: &config::Config, check: &str, ratio: Option<f64>) -> Style {
        let theme = &config.theme;
        let thresholds = config.checks.get(check);
        let percent = ratio.map(|v| v * 100.0);
        let reached = |threshold: Option<f64>| {
            percent
                .zip(threshold)
                .is_some_and(|(value, bound)| value >= bound)
        };
        let color = if reached(thresholds.and_then(|c| c.critical)) {
            &theme.critical
        } else if reached(thresholds.and_then(|c| c.warn)) {
            &theme.warning
        } else if check == "cpu" {
            &theme.cpu
        } else {
            &theme.memory
        };
        self.style(color).add_modifier(Modifier::BOLD)
    }
    fn summary(&self, config: &config::Config, compact: bool) -> Vec<Line<'static>> {
        let Some(report) = &self.report else {
            return vec![Line::from("Collecting a user resource snapshot…")];
        };
        let s = &report.summary;
        let theme = &config.theme;
        let muted = self.style(&theme.muted);
        let cpu = s.cpu_limit.value.map(|v| s.cpu_cores / v);
        let mem = s.memory_limit.value.map(|v| s.memory_bytes as f64 / v);
        let cpu_style = self.pressure_style(config, "cpu", cpu);
        let mem_style = self.pressure_style(config, "memory", mem);
        if compact {
            return vec![Line::from(vec![
                Span::styled(format!("CPU {:.2}c", s.cpu_cores), cpu_style),
                Span::styled(" · ", muted),
                Span::styled(format!("MEM {}", util::size(s.memory_bytes)), mem_style),
                Span::styled(
                    format!(" · {} procs", s.processes.count),
                    self.style(&theme.tree),
                ),
            ])];
        }
        let mut lines = Vec::new();
        for (label, ratio, usage, limit, source, style) in [
            (
                "CPU",
                cpu,
                format!("{:.2}", s.cpu_cores),
                s.cpu_limit
                    .value
                    .map_or_else(|| "?".into(), |v| format!("{v:.2} cores")),
                &s.cpu_limit.source,
                cpu_style,
            ),
            (
                "MEM",
                mem,
                util::size(s.memory_bytes),
                s.memory_limit
                    .value
                    .map_or_else(|| "?".into(), |v| util::size(v as u64)),
                &s.memory_limit.source,
                mem_style,
            ),
        ] {
            let label_color = if label == "CPU" {
                &theme.cpu
            } else {
                &theme.memory
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{label}  "),
                    self.style(label_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(render::gauge(ratio, config.ascii), style),
                Span::styled(format!("  {usage} / {limit}  "), style),
                Span::styled(
                    ratio.map_or_else(|| "n/a".into(), |v| format!("{:.1}%", v * 100.0)),
                    style,
                ),
                Span::styled(format!("  [{source}]"), muted),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled(
                "USER ",
                self.style(&theme.tree).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} processes", s.processes.count),
                self.style(&theme.tree),
            ),
            Span::styled(
                format!(" | MEM accounting: {} | process PSS/RSS ", s.memory_source),
                muted,
            ),
            Span::styled(util::size(s.processes.mem), self.style(&theme.memory)),
            Span::styled(
                format!(
                    " ({}/{} PSS readable)",
                    s.processes.count - s.processes.fallback,
                    s.processes.count
                ),
                muted,
            ),
        ]));
        let number = |value: Option<u64>| value.map_or_else(|| "?".into(), |v| v.to_string());
        let throttle = if s.throttled_periods.is_some_and(|v| v > 0) {
            &theme.warning
        } else {
            &theme.muted
        };
        let oom = if s.oom_new.is_some_and(|v| v > 0) {
            &theme.critical
        } else {
            &theme.header
        };
        lines.push(Line::from(vec![
            Span::styled("CPU throttle ", self.style(&theme.cpu)),
            Span::styled(
                format!(
                    "{}/{} periods, {}ms",
                    number(s.throttled_periods),
                    number(s.periods),
                    s.throttle_ms
                        .map_or_else(|| "?".into(), |v| format!("{v:.1}"))
                ),
                self.style(throttle),
            ),
            Span::styled(" | ", muted),
            Span::styled(
                format!(
                    "OOM {} cumulative, {} new this sample",
                    number(report.account.oom),
                    number(s.oom_new)
                ),
                self.style(oom),
            ),
        ]));
        if report.account.memory.is_some() {
            let mut spans = vec![Span::styled("CHARGE ", muted)];
            for (i, (label, value, color)) in [
                ("anon", report.account.anon, &theme.cpu),
                ("shared", report.account.shmem, &theme.tree),
                ("file cache", report.account.file, &theme.warning),
                ("peak", report.account.peak, &theme.memory),
            ]
            .into_iter()
            .enumerate()
            {
                if i > 0 {
                    spans.push(Span::styled(" | ", muted));
                }
                spans.push(Span::styled(format!("{label} "), muted));
                spans.push(Span::styled(
                    value.map_or_else(|| "?".into(), util::size),
                    self.style(color),
                ));
            }
            lines.push(Line::from(spans));
        }
        // Keep the independently detected bounds visible for configured budgets.
        lines.extend(
            render::summary(report, config.ascii)
                .into_iter()
                .filter(|line| line.starts_with("DETECTED "))
                .map(|line| Line::styled(line, muted)),
        );
        lines
    }
    fn row_line(&self, index: usize, text: String, config: &config::Config) -> Line<'static> {
        let theme = &config.theme;
        let Some((table, row)) = self
            .table
            .as_ref()
            .and_then(|table| table.rows.get(index).map(|row| (table, row)))
        else {
            let color = if text.starts_with("CRIT") || text.starts_with("ERROR") {
                &theme.critical
            } else if text.starts_with("WARN") {
                &theme.warning
            } else {
                &theme.muted
            };
            return cell_line(Line::styled(text, self.style(color)));
        };
        let base = if self.rainbow {
            identity_color(&row.key, &theme.rainbow_colors)
        } else {
            &theme.tree
        };
        let mut rest = text.as_str();
        let mut spans = Vec::new();
        for (field, width) in &table.columns {
            let (cell, tail) = take_cells(rest, *width);
            rest = tail;
            // Metric colors express their meaning consistently; identity colors
            // belong to PID/command cells rather than recoloring every metric.
            let color = match field.as_str() {
                "cpu" => &theme.cpu,
                "mem" | "self_mem" | "rss" => &theme.memory,
                "read" => &theme.read,
                "write" => &theme.write,
                "pid" | "command" | "processes" if self.rainbow => base,
                "command" => &theme.tree,
                _ => &theme.muted,
            };
            let style = if row.context {
                self.style(color).add_modifier(Modifier::DIM)
            } else {
                self.style(color)
            };
            if field == "command" && self.view.kind == "tree" {
                let guide_width = self
                    .report
                    .as_ref()
                    .and_then(|r| row.pid.and_then(|pid| r.processes.get(&pid)))
                    .and_then(|p| row.command.strip_suffix(&p.command(false)))
                    .map_or(0, render::cell_width);
                let (guide, command) = take_cells(cell, guide_width);
                spans.push(Span::styled(guide.to_owned(), self.style(&theme.tree)));
                spans.push(Span::styled(command.to_owned(), style));
            } else {
                spans.push(Span::styled(cell.to_owned(), style));
            }
            let (gap, tail) = take_cells(rest, 2);
            spans.push(Span::raw(gap.to_owned()));
            rest = tail;
        }
        cell_line(Line::from(spans))
    }
    fn body(&mut self, config: &config::Config, width: usize) -> (String, Vec<String>) {
        let Some(report) = &self.report else {
            return (
                "Collecting".into(),
                vec!["Sampling the selected user; collecting a CPU baseline…".into()],
            );
        };
        if self.view.kind == "oom" {
            self.table = None;
            return (
                format!("{} · historical kernel PIDs", self.label()),
                render::text_view(
                    report,
                    &self.view,
                    self.options.since,
                    &self.search,
                    width,
                    config.ascii,
                ),
            );
        }
        if self.view.kind == "checks" {
            self.table = None;
            return (
                format!("{} · builtins and configured extensions", self.label()),
                render::text_view(report, &self.view, None, &self.search, width, config.ascii),
            );
        }
        let table = render::table_with_layout(
            report,
            config,
            &self.view,
            &render::Options {
                full: false,
                ascii: config.ascii,
                pid: if self.view.kind == "tree" {
                    self.options.pid
                } else {
                    None
                },
                search: self.search.clone(),
                collapsed: self.collapsed.clone(),
            },
            width,
            render::TableLayout::Stable,
        );
        if let Some(key) = &self.key
            && let Some(i) = table.rows.iter().position(|r| &r.key == key)
        {
            self.selected = i;
        }
        self.selected = self.selected.min(table.rows.len().saturating_sub(1));
        self.key = table.rows.get(self.selected).map(|r| r.key.clone());
        let title = format!(
            "{} · {} rows · sort {}",
            self.label(),
            table.total_rows,
            self.view.sort
        );
        let lines = std::iter::once(table.header.clone())
            .chain(table.lines.clone())
            .collect();
        self.table = Some(table);
        (title, lines)
    }
    fn draw(&mut self, frame: &mut Frame, config: &config::Config) {
        let area = frame.area();
        frame.render_widget(Block::default().style(self.base_style(config)), area);
        if area.width < 24 || area.height < 8 {
            frame.render_widget(
                Paragraph::new("ures: enlarge the terminal (24×8 minimum)"),
                area,
            );
            return;
        }
        let height = if area.height >= 24 {
            8
        } else if area.height >= 16 {
            6
        } else {
            3
        };
        let sections = Layout::vertical([
            Constraint::Length(height),
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(2),
        ])
        .split(area);
        let title = self.report.as_ref().map_or_else(
            || " ures · sampling ".into(),
            |r| format!(" ures · {}@{} · uid {} ", r.user.name, r.host, r.user.uid),
        );
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title)
            .border_style(self.style(&config.theme.accent));
        let summary = self.summary(config, height == 3);
        frame.render_widget(
            Paragraph::new(Text::from(summary)).block(block),
            sections[0],
        );
        let (labels, selected_tab) = self.visible_tabs(config, sections[1].width as usize);
        frame.render_widget(
            Tabs::new(labels.into_iter().map(|label| cell_line(Line::from(label))))
                .select(selected_tab)
                .highlight_style(
                    self.style(&config.theme.header)
                        .add_modifier(Modifier::BOLD),
                ),
            sections[1],
        );
        let body_block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.style(&config.theme.muted));
        let inner = body_block.inner(sections[2]);
        let (title, mut body) = self.body(config, inner.width as usize);
        let body_block = body_block.title(format!(" {title} "));
        frame.render_widget(body_block, sections[2]);
        if !body.is_empty() {
            let header = body.remove(0);
            let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
            frame.render_widget(
                Paragraph::new(header).style(
                    self.style(&config.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                parts[0],
            );
            self.selected = self.selected.min(body.len().saturating_sub(1));
            self.state
                .select((!body.is_empty()).then_some(self.selected));
            let items: Vec<_> = body
                .into_iter()
                .enumerate()
                .map(|(index, line)| ListItem::new(self.row_line(index, line, config)))
                .collect();
            let selection = if self.color {
                self.style(&config.theme.selection_fg)
                    .bg(config.theme.selection_bg.parse().unwrap_or(Color::Reset))
                    .remove_modifier(Modifier::REVERSED | Modifier::DIM)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .remove_modifier(Modifier::DIM)
            };
            let list = List::new(items).highlight_style(selection);
            frame.render_stateful_widget(list, parts[1], &mut self.state);
        }
        let age = self
            .report
            .as_ref()
            .map_or(0, |r| (util::unix_now() - r.unix_seconds).max(0));
        let footer = if self.editing {
            format!(
                "Search: {}_  (Enter apply · Esc clear)\nMEM=PSS, *=RSS fallback, ~=partial. Tree totals overlap.",
                self.search
            )
        } else {
            format!(
                "Tab pages · ↑↓ move · Space fold · Enter detail · / search · ? help · q quit\n{} age {age}s · c/m/i sort · p pause · r refresh · R rainbow:{} · *=RSS ~=partial",
                if self.paused { "PAUSED" } else { "LIVE" },
                if self.rainbow && self.color {
                    "on"
                } else {
                    "off"
                }
            )
        };
        frame.render_widget(
            Paragraph::new(footer).style(self.style(&config.theme.muted)),
            sections[3],
        );
        if self.help {
            self.popup(frame,"Keys & accounting","Tab / Shift-Tab  switch pages\nUp/down or j/k   select a row\nPageUp/PageDown  jump ten rows\nHome / End       first / last row\nSpace/left/right fold a tree branch\nEnter            process details (process views)\nc / m / i        sort process views: CPU / memory / I/O\n/                search current view content\np                pause/resume displayed data\nr                request a fresh sample\nR (Shift-r)      toggle stable rainbow row colors\nq / Esc          quit (outside popups/search)\nAny key          close help or details\n\nCPU(c) is cores, not whole-machine percent.\nTree CPU/MEM include descendants; SELF MEM is root only.\nPSS apportions shared pages; * marks RSS fallback.\n? is unavailable; ~ is partial. OOM PIDs are historical.\nThis interface never signals or modifies target processes.", config);
        }
        if let Some(key) = &self.detail {
            let text=self.report.as_ref().and_then(|r|r.processes.values().find(|p|format!("{}:{}",p.pid,p.start)==*key).map(|p|{
                let total=crate::procfs::subtree(p.pid,&r.processes,&crate::procfs::children(&r.processes));
                format!("PID {}  PPID {}  state {}  start ticks {}\nOwn CPU {:.2} cores  subtree CPU {:.2} cores\nOwn PSS/RSS {}  subtree {}  RSS {}\nThreads {:?}  FD {:?}\n\n{}\n\nCPU cgroup: {}\nMemory cgroup: {}\nSample {} · collection {:.1}ms\n\nAny key closes this panel.",p.pid,p.ppid,p.state,p.start,p.cpu/100.0,total.cpu/100.0,util::size(p.mem()),util::size(total.mem),util::size(p.rss),p.threads,p.fds,p.command(true),r.cpu_cgroup.as_deref().unwrap_or("not detected"),r.memory_cgroup.as_deref().unwrap_or("not detected"),r.timestamp,r.collection_ms)
            })).unwrap_or_else(||"The selected process has exited or changed identity.\nEsc closes this panel.".into());
            self.popup(frame, "Process details", &text, config);
        }
    }
    fn popup(&self, frame: &mut Frame, title: &str, text: &str, config: &config::Config) {
        let area = frame.area();
        let rect = Rect::new(
            area.x + area.width / 20,
            area.y + 1,
            area.width.saturating_sub(area.width / 10),
            area.height.saturating_sub(2),
        );
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(text)
                .style(self.base_style(config))
                .wrap(Wrap { trim: false })
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .title(format!(" {title} "))
                        .border_style(self.style(&config.theme.accent)),
                ),
            rect,
        );
    }
    fn set_page(&mut self, page: usize, config: &config::Config) {
        if page == self.page {
            return;
        }
        self.retained_views.insert(
            self.tabs[self.page].clone(),
            ViewState {
                view: self.view.clone(),
                selected: self.selected,
                key: self.key.take(),
                collapsed: std::mem::take(&mut self.collapsed),
                search: std::mem::take(&mut self.search),
                state: std::mem::take(&mut self.state),
            },
        );
        self.page = page;
        let saved = self
            .retained_views
            .remove(&self.tabs[page])
            .unwrap_or_else(|| {
                let mut view = config.views[&self.tabs[page]].clone();
                if let Some(top) = self.options.top {
                    view.top = top;
                }
                ViewState {
                    view,
                    selected: 0,
                    key: None,
                    collapsed: BTreeSet::new(),
                    search: String::new(),
                    state: ListState::default(),
                }
            });
        self.view = saved.view;
        self.selected = saved.selected;
        self.key = saved.key;
        self.collapsed = saved.collapsed;
        self.search = saved.search;
        self.state = saved.state;
        self.editing = false;
        self.detail = None;
        self.table = None;
    }
}

pub fn require_terminal() -> Result<()> {
    if !std::io::stdout().is_terminal()
        || !std::io::stdin().is_terminal()
        || std::env::var("TERM").is_ok_and(|s| s == "dumb")
    {
        return Err(
            "interactive mode requires terminal input and output; use ures report for text or ures report --json for scripts".into(),
        );
    }
    Ok(())
}

pub fn run(
    user: util::User,
    config: config::Config,
    options: cli::Options,
    view: config::View,
) -> Result<()> {
    require_terminal()?;
    let worker = engine::Worker::spawn(user, config.clone(), engine::features(&config, &view));
    let _restore = Restore;
    let mut terminal = ratatui::try_init().map_err(|e| e.to_string())?;
    let color = cli::color(&options);
    let mut app = App::new(&config, options, view, color);
    let mut dirty = true;
    let mut second = util::unix_now();
    loop {
        if util::STOP.load(Ordering::Relaxed) {
            break;
        }
        if !app.paused {
            let latest = worker.latest.lock().map_err(|e| e.to_string())?;
            match latest.as_ref() {
                Some(Err(error)) => return Err(error.clone()),
                Some(Ok(report))
                    if app
                        .report
                        .as_ref()
                        .is_none_or(|old| old.sample_id != report.sample_id) =>
                {
                    let same_identity = |pid: &u32| {
                        app.report
                            .as_ref()
                            .and_then(|old| old.processes.get(pid))
                            .zip(report.processes.get(pid))
                            .is_some_and(|(a, b)| a.start == b.start)
                    };
                    app.collapsed.retain(same_identity);
                    for saved in app.retained_views.values_mut() {
                        saved.collapsed.retain(same_identity);
                    }
                    app.report = Some(report.clone());
                    dirty = true;
                }
                _ => {}
            }
        }
        if second != util::unix_now() {
            second = util::unix_now();
            dirty = true;
        }
        if dirty {
            terminal
                .draw(|frame| app.draw(frame, &config))
                .map_err(|e| e.to_string())?;
            dirty = false;
        }
        if !event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            continue;
        }
        match event::read().map_err(|e| e.to_string())? {
            Event::Resize(_, _) => dirty = true,
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                dirty = true;
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    util::SIGNAL.store(libc::SIGINT, Ordering::Relaxed);
                    util::STOP.store(true, Ordering::Relaxed);
                    break;
                }
                if app.help {
                    app.help = false;
                    continue;
                }
                if app.detail.is_some() {
                    app.detail = None;
                    continue;
                }
                if app.editing {
                    match key.code {
                        KeyCode::Esc => {
                            app.editing = false;
                            app.search.clear();
                        }
                        KeyCode::Enter => app.editing = false,
                        KeyCode::Backspace => {
                            app.search.pop();
                        }
                        KeyCode::Char(c) => {
                            app.search.push(c);
                        }
                        _ => {}
                    }
                    app.selected = 0;
                    app.key = None;
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('?') => app.help = true,
                    KeyCode::Char('/') => app.editing = true,
                    KeyCode::Char('p') => app.paused = !app.paused,
                    KeyCode::Char('R') => app.rainbow = !app.rainbow,
                    KeyCode::Char('r') => {
                        let _ = worker.tx.send(engine::Control::Refresh);
                    }
                    KeyCode::Tab | KeyCode::BackTab => {
                        let count = app.tabs.len();
                        let page = (app.page
                            + if key.code == KeyCode::Tab {
                                1
                            } else {
                                count - 1
                            })
                            % count;
                        app.set_page(page, &config);
                        let _ = worker.tx.send(engine::Control::Features(engine::features(
                            &config, &app.view,
                        )));
                        let _ = worker.tx.send(engine::Control::Refresh);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.selected = app.selected.saturating_sub(1);
                        app.key = None;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        app.selected += 1;
                        app.key = None;
                    }
                    KeyCode::PageDown => {
                        app.selected += 10;
                        app.key = None;
                    }
                    KeyCode::PageUp => {
                        app.selected = app.selected.saturating_sub(10);
                        app.key = None;
                    }
                    KeyCode::Home => {
                        app.selected = 0;
                        app.key = None;
                    }
                    KeyCode::End => {
                        app.selected = usize::MAX / 2;
                        app.key = None;
                    }
                    KeyCode::Char('c' | 'm' | 'i') if app.view.is_table() => {
                        app.view.sort = match key.code {
                            KeyCode::Char('c') => "cpu",
                            KeyCode::Char('i') => "io",
                            _ => "mem",
                        }
                        .into();
                        let _ = worker.tx.send(engine::Control::Features(engine::features(
                            &config, &app.view,
                        )));
                        app.selected = 0;
                        app.key = None;
                    }
                    KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
                        if app.view.kind == "tree" =>
                    {
                        if let Some(row) = app.table.as_ref().and_then(|t| t.rows.get(app.selected))
                            && row.has_children
                            && let Some(pid) = row.pid
                            && !app.collapsed.remove(&pid)
                        {
                            app.collapsed.insert(pid);
                        }
                    }
                    KeyCode::Enter => {
                        app.detail = app
                            .table
                            .as_ref()
                            .and_then(|t| t.rows.get(app.selected))
                            .filter(|r| r.pid.is_some())
                            .map(|r| r.key.clone());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(config: &config::Config, color: bool) -> App {
        let mut app = App::new(
            config,
            cli::Options::default(),
            config.views[&config.default_view].clone(),
            color,
        );
        app.report = Some(crate::test_support::report());
        app
    }
    #[test]
    fn tui_handles_tiny_and_wide_test_backends() {
        for (width, height) in [(20, 6), (24, 8), (60, 16), (80, 24), (160, 40)] {
            let backend = ratatui::backend::TestBackend::new(width, height);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let config = config::Config::builtin();
            let mut app = app(&config, false);
            terminal.draw(|frame| app.draw(frame, &config)).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("ures"));
        }
    }

    #[test]
    fn grapheme_columns_preserve_following_metrics_and_styled_text() {
        let config = config::Config::parse(
            "default_view='ps'\n[views.ps]\ncolumns=['command','pid','cpu']\n[columns.command]\nwidth=10\nmax_width=10",
            None,
        ).unwrap();
        let mut app = app(&config, true);
        let report = app.report.as_mut().unwrap();
        report.processes.retain(|pid, _| *pid == 2000);
        for (command, symbol) in [("👩‍💻👩‍💻👩‍💻", "👩‍💻"), ("لالالالالا", "ل")]
        {
            app.report
                .as_mut()
                .unwrap()
                .processes
                .get_mut(&2000)
                .unwrap()
                .args = vec![command.into()];
            app.body(&config, 60);
            let table = app.table.as_ref().unwrap();
            let original = &table.lines[0];
            let styled = app.row_line(0, original.clone(), &config);
            assert_eq!(
                styled
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>(),
                *original
            );
            assert_eq!(styled.width(), render::cell_width(original));
            // List is the production row widget; Paragraph exercises the same
            // content through its separate grapheme rendering path.
            for list in [false, true] {
                let backend = ratatui::backend::TestBackend::new(60, 1);
                let mut terminal = ratatui::Terminal::new(backend).unwrap();
                terminal
                    .draw(|frame| {
                        if list {
                            frame.render_widget(
                                List::new([ListItem::new(styled.clone())]),
                                frame.area(),
                            );
                        } else {
                            frame.render_widget(Paragraph::new(styled.clone()), frame.area());
                        }
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                assert_eq!(buffer[(4, 0)].symbol(), symbol);
                assert_eq!(
                    (15..19)
                        .map(|x| buffer[(x, 0)].symbol())
                        .collect::<String>(),
                    "2000"
                );
                assert_eq!(
                    (25..29)
                        .map(|x| buffer[(x, 0)].symbol())
                        .collect::<String>(),
                    "0.50"
                );
                assert_eq!(
                    buffer[(25, 0)].fg,
                    config.theme.cpu.parse::<Color>().unwrap()
                );
            }
        }
        let label = cell_line(Line::from("لالالالالا"));
        assert_eq!(label.width(), 10);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(30, 1)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(Tabs::new([label, Line::from("Next")]), frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(
            (14..18)
                .map(|x| buffer[(x, 0)].symbol())
                .collect::<String>(),
            "Next"
        );
        assert_eq!(take_cells("e\u{301}🇨🇳👩‍💻 end", 3), ("e\u{301}🇨🇳", "👩‍💻 end"));
    }

    #[test]
    fn overview_has_distinct_semantic_colors_and_honors_thresholds() {
        let config = config::Config::builtin();
        let app = app(&config, true);
        let lines = app.summary(&config, false);
        let color = |value: &str| value.parse::<Color>().unwrap();
        assert_eq!(lines[0].spans[0].style.fg, Some(color(&config.theme.cpu)));
        assert_eq!(
            lines[1].spans[0].style.fg,
            Some(color(&config.theme.memory))
        );
        assert_eq!(
            lines[1].spans[1].style.fg,
            Some(color(&config.theme.critical))
        );
        assert_eq!(lines[2].spans[0].style.fg, Some(color(&config.theme.tree)));
        assert!(
            lines[3]
                .spans
                .iter()
                .any(|s| s.style.fg == Some(color(&config.theme.warning)))
        );
        let backend = ratatui::backend::TestBackend::new(110, 28);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut app = app;
        terminal.draw(|frame| app.draw(frame, &config)).unwrap();
        for expected in [
            &config.theme.cpu,
            &config.theme.memory,
            &config.theme.critical,
            &config.theme.tree,
        ] {
            assert!(
                terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .any(|c| c.fg == color(expected))
            );
        }
    }

    #[test]
    fn rainbow_tracks_identity_across_sort_filter_fold_and_toggle() {
        let config = config::Config::builtin();
        let mut app = app(&config, true);
        let colors = |app: &mut App| -> BTreeMap<String, Option<Color>> {
            app.body(&config, 110);
            let table = app.table.as_ref().unwrap();
            table
                .rows
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    (
                        row.key.clone(),
                        app.row_line(i, table.lines[i].clone(), &config).spans[0]
                            .style
                            .fg,
                    )
                })
                .collect()
        };
        let original = colors(&mut app);
        assert!(
            original
                .values()
                .map(|color| format!("{color:?}"))
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        );
        app.view.sort = "cpu".into();
        assert_eq!(original, colors(&mut app));
        app.search = "sample_worker".into();
        for (key, color) in colors(&mut app) {
            assert_eq!(original[&key], color);
        }
        app.search.clear();
        app.collapsed.insert(1000);
        for (key, color) in colors(&mut app) {
            assert_eq!(original[&key], color);
        }
        app.rainbow = false;
        let table = app.table.as_ref().unwrap();
        let line = app.row_line(0, table.lines[0].clone(), &config);
        assert_eq!(line.spans[0].style.fg, config.theme.muted.parse().ok());
        assert!(
            line.spans
                .iter()
                .any(|s| s.style.fg == config.theme.cpu.parse().ok())
        );
    }

    #[test]
    fn color_disabled_removes_every_foreground_color() {
        let config = config::Config::builtin();
        let mut app = app(&config, false);
        let backend = ratatui::backend::TestBackend::new(110, 28);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.draw(frame, &config)).unwrap();
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
        );
    }

    #[test]
    fn configured_tabs_include_every_kind_and_no_unconfigured_pages() {
        let config = config::Config::parse(
            r#"
default_view = "workers"
tab_order = ["workers", "io", "compact", "events", "health"]
[views.workers]
name = "Workers"
kind = "tree"
columns = ["pid", "mem", "command"]
[views.io]
name = "Disk I/O"
[views.compact]
[views.events]
name = "Kernel kills"
kind = "oom"
[views.health]
kind = "checks"
"#,
            None,
        )
        .unwrap();
        let mut app = app(&config, true);
        assert_eq!(
            app.tabs,
            vec!["workers", "io", "compact", "events", "health"]
        );
        assert_eq!(app.label(), "Workers");
        assert_eq!(
            app.visible_tabs(&config, 160).0,
            vec!["Workers", "Disk I/O", "compact", "Kernel kills", "health"]
        );
        for page in 0..app.tabs.len() {
            app.set_page(page, &config);
            let (_, lines) = app.body(&config, 100);
            match app.view.kind.as_str() {
                "oom" => {
                    assert!(lines[0].starts_with("TIME"));
                    assert!(app.table.is_none());
                }
                "checks" => {
                    assert!(lines[0].starts_with("STATUS"));
                    assert!(app.table.is_none());
                }
                _ => {
                    assert!(app.table.is_some());
                }
            }
            if app.tabs[page] == "io" {
                let features = engine::features(&config, &app.view);
                assert!(features.io && features.threads && features.fds);
                assert!(lines[0].contains("READ/s"));
            }
        }
        for width in [24, 40, 80] {
            let (labels, selected) = app.visible_tabs(&config, width);
            assert_eq!(labels[selected], app.label());
            let through_selected = labels[..=selected]
                .iter()
                .map(|s| render::cell_width(s.as_str()) + 3)
                .sum::<usize>()
                - 1;
            assert!(through_selected <= width);
        }
    }

    #[test]
    fn same_kind_views_restore_sort_search_fold_selection_and_scroll_separately() {
        let config = config::Config::parse(
            r#"
default_view = "one"
[views.one]
kind = "tree"
columns = ["pid", "mem", "command"]
top = 7
[views.two]
kind = "tree"
columns = ["pid", "cpu", "command"]
top = 13
"#,
            None,
        )
        .unwrap();
        let mut app = app(&config, false);
        app.view.sort = "cpu".into();
        app.search = "python".into();
        app.collapsed.insert(1000);
        app.key = Some("1001:20".into());
        app.selected = 1;
        *app.state.offset_mut() = 5;
        app.set_page(1, &config);
        assert!(app.search.is_empty() && app.collapsed.is_empty());
        assert_eq!(app.view.top, 13);
        app.view.sort = "io".into();
        app.search = "sample-watcher".into();
        app.collapsed.insert(2000);
        app.selected = 2;
        *app.state.offset_mut() = 3;
        app.set_page(0, &config);
        assert_eq!(app.view.top, 7);
        assert_eq!(app.view.sort, "cpu");
        assert_eq!(app.search, "python");
        assert_eq!(app.key.as_deref(), Some("1001:20"));
        assert_eq!(app.selected, 1);
        assert_eq!(app.state.offset(), 5);
        assert_eq!(app.collapsed, BTreeSet::from([1000]));
        app.set_page(1, &config);
        assert_eq!(app.view.sort, "io");
        assert_eq!(app.search, "sample-watcher");
        assert_eq!(app.selected, 2);
        assert_eq!(app.state.offset(), 3);
        assert_eq!(app.collapsed, BTreeSet::from([2000]));
    }

    #[test]
    fn one_custom_tab_and_long_unicode_tab_names_remain_usable() {
        let config = config::Config::parse(
            r#"
[views.only]
name = "只有一个很长的自定义视图名称需要省略"
kind = "checks"
"#,
            None,
        )
        .unwrap();
        let mut app = app(&config, false);
        assert_eq!(app.tabs, vec!["only"]);
        app.set_page((app.page + 1) % app.tabs.len(), &config);
        let (labels, selected) = app.visible_tabs(&config, 24);
        assert_eq!(selected, 0);
        assert!(labels[0].ends_with('…'));
        assert!(render::cell_width(labels[0].as_str()) <= 22);
    }

    #[test]
    fn metric_colors_remain_semantic_with_rainbow_enabled() {
        let config = config::Config::builtin();
        let mut app = app(&config, true);
        app.body(&config, 110);
        let table = app.table.as_ref().unwrap();
        for (index, text) in table.lines.iter().enumerate() {
            let line = app.row_line(index, text.clone(), &config);
            assert_eq!(line.spans[2].style.fg, config.theme.cpu.parse().ok());
            assert_eq!(line.spans[4].style.fg, config.theme.memory.parse().ok());
        }
        let page = app.tabs.iter().position(|key| key == "io").unwrap();
        app.set_page(page, &config);
        app.body(&config, 110);
        let table = app.table.as_ref().unwrap();
        let line = app.row_line(0, table.lines[0].clone(), &config);
        assert_eq!(line.spans[2].style.fg, config.theme.read.parse().ok());
        assert_eq!(line.spans[4].style.fg, config.theme.write.parse().ok());
        assert_ne!(line.spans[2].style.fg, line.spans[4].style.fg);
    }

    #[test]
    fn selection_background_is_continuous_across_cells_gaps_and_trailing_space() {
        for theme in crate::theme::BUILTINS {
            let config = config::Config::parse_with_theme("", None, Some(theme.name)).unwrap();
            let mut app = app(&config, true);
            let width = 110;
            let backend = ratatui::backend::TestBackend::new(width, 24);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let bg = config.theme.selection_bg.parse::<Color>().unwrap();
            let fg = config.theme.selection_fg.parse::<Color>().unwrap();
            for selected in [0, 1, 3] {
                app.selected = selected;
                app.key = None;
                terminal.draw(|frame| app.draw(frame, &config)).unwrap();
                let buffer = terminal.backend().buffer();
                let cells = buffer.content();
                let marked: Vec<_> = cells
                    .iter()
                    .enumerate()
                    .filter(|(_, cell)| cell.bg == bg)
                    .collect();
                assert_eq!(marked.len(), usize::from(width - 2));
                let y = marked[0].0 / usize::from(width);
                for x in 1..usize::from(width - 1) {
                    let cell = &cells[y * usize::from(width) + x];
                    assert_eq!(cell.bg, bg, "gap at {x},{y}");
                    assert_eq!(cell.fg, fg, "foreground at {x},{y}");
                    assert!(!cell.modifier.intersects(Modifier::REVERSED | Modifier::DIM));
                }
            }
            // Optional exact-cell export for manual UI review; regular tests do not
            // write artifacts. Renderers can consume this without a terminal parser.
            if let Ok(path) = std::env::var("URES_TUI_PREVIEW") {
                let path = std::path::PathBuf::from(path);
                let path = if theme.name == crate::theme::DEFAULT {
                    path
                } else {
                    path.with_extension(format!("{}.json", theme.name))
                };
                let buffer = terminal.backend().buffer();
                let cells: Vec<_> = buffer
                .content()
                .iter()
                .map(|cell| {
                    serde_json::json!({
                        "text": cell.symbol(), "fg": cell.fg.to_string(), "bg": cell.bg.to_string(),
                        "bold": cell.modifier.contains(Modifier::BOLD),
                    })
                })
                .collect();
                std::fs::write(
                path,
                serde_json::to_vec(&serde_json::json!({"width":width,"height":24,"cells":cells,
                    "foreground":config.theme.foreground,"background":config.theme.background,
                    "critical":config.theme.critical,"warning":config.theme.warning,
                    "cpu":config.theme.cpu,"muted":config.theme.muted,"selection_bg":config.theme.selection_bg}))
                    .unwrap(),
            )
            .unwrap();
            }
        }
    }

    #[test]
    fn themes_cover_blank_cells_and_popup_backgrounds() {
        for theme in crate::theme::BUILTINS {
            let config = config::Config::parse_with_theme("", None, Some(theme.name)).unwrap();
            let mut app = app(&config, true);
            let bg = config.theme.background.parse::<Color>().unwrap();
            let selection = config.theme.selection_bg.parse::<Color>().unwrap();
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 24)).unwrap();
            for help in [false, true] {
                app.help = help;
                terminal.draw(|frame| app.draw(frame, &config)).unwrap();
                assert!(
                    terminal.backend().buffer().content().iter().all(|cell| {
                        (cell.bg == bg || cell.bg == selection) && cell.fg != Color::Reset
                    }),
                    "{}, help={help}",
                    theme.name
                );
            }
        }
    }
}
