use crate::{Result, config, engine, render, util};
use std::collections::BTreeSet;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub const HELP: &str = r#"ures — Linux resources, focused on one user

Usage: ures [COMMAND] [OPTIONS]

Open the interactive TUI when no command is given.
Commands:
  report                   Print a text or JSON snapshot
  config ACTION            Inspect or validate configuration
  help [COMMAND [ACTION]]  Show detailed help
"#;

struct HelpOption {
    flag: &'static str,
    usage: &'static str,
    description: &'static str,
}

const HELP_OPTIONS: &[HelpOption] = &[
    HelpOption {
        flag: "--user",
        usage: "-u, --user USER",
        description: "User name or UID (default: current user)",
    },
    HelpOption {
        flag: "--view",
        usage: "    --view KEY",
        description: "Configured view key (default: default_view)",
    },
    HelpOption {
        flag: "--sort",
        usage: "-s, --sort FIELD",
        description: "mem | cpu | io | threads | fds (default: view config)",
    },
    HelpOption {
        flag: "--top",
        usage: "-n, --top N",
        description: "Maximum rows; 0 means all (default: view config)",
    },
    HelpOption {
        flag: "--pid",
        usage: "    --pid PID",
        description: "Focus a tree on a process belonging to the selected user",
    },
    HelpOption {
        flag: "--since",
        usage: "    --since TIME",
        description: "Retained OOM records to display (text/TUI only)",
    },
    HelpOption {
        flag: "--interval",
        usage: "-i, --interval TIME",
        description: "Time between snapshots (seconds or s/m/h/d)",
    },
    HelpOption {
        flag: "--sample",
        usage: "    --sample TIME",
        description: "Initial CPU sampling interval (default: config)",
    },
    HelpOption {
        flag: "--count",
        usage: "    --count N",
        description: "Stop after N snapshots (requires -i)",
    },
    HelpOption {
        flag: "--theme",
        usage: "    --theme NAME",
        description: "Built-in theme; list with ures config themes",
    },
    HelpOption {
        flag: "--ascii",
        usage: "    --ascii",
        description: "ASCII tree and gauges",
    },
    HelpOption {
        flag: "--color",
        usage: "    --color MODE",
        description: "auto | always | never (default: auto)",
    },
    HelpOption {
        flag: "--json",
        usage: "    --json",
        description: "Complete JSON snapshot; NDJSON when repeated",
    },
    HelpOption {
        flag: "--full",
        usage: "    --full",
        description: "Complete command lines in text output",
    },
    HelpOption {
        flag: "--width",
        usage: "    --width N",
        description: "Text width (default: terminal, COLUMNS, or 120)",
    },
    HelpOption {
        flag: "--config",
        usage: "    --config FILE",
        description: "Use this TOML configuration file",
    },
    HelpOption {
        flag: "--profile",
        usage: "    --profile NAME",
        description: "Apply a named configuration profile",
    },
    HelpOption {
        flag: "--help",
        usage: "-h, --help",
        description: "Brief help with -h; full help with --help",
    },
    HelpOption {
        flag: "--version",
        usage: "-V, --version",
        description: "Show version",
    },
];

fn option_group(output: &mut String, heading: &str, flags: &[&str]) {
    output.push_str(&format!("\n{heading}:\n"));
    for flag in flags {
        let option = HELP_OPTIONS
            .iter()
            .find(|option| option.flag == *flag)
            .unwrap();
        output.push_str(&format!("  {:<22} {}\n", option.usage, option.description));
    }
}

pub fn help(topic: Option<&str>) -> Result<String> {
    help_for(topic, None, true)
}

fn help_for(topic: Option<&str>, action: Option<&str>, detailed: bool) -> Result<String> {
    let report = match topic {
        None => false,
        Some("report") => true,
        Some("config") => return config_help(action, detailed),
        Some("help") => return Ok("Usage: ures help [COMMAND [ACTION]]\n\nShow full help for the selected command or config action.\nEquivalent to appending --help; add -h for brief help.\n\nOptions:\n  -h, --help             Explain the help command\n\nExamples:\n  ures help report\n  ures help config show\n".into()),
        Some(topic) => return Err(format!("unknown help topic: {topic}; use ures -h")),
    };
    let mut output = if report {
        "Usage: ures report [OPTIONS]\n\nPrint the overview and selected view once, then exit.\nAdd -i TIME to repeat; --count N limits the number of snapshots.\n".into()
    } else {
        HELP.to_owned()
    };
    if detailed {
        option_group(
            &mut output,
            "Selection",
            &["--user", "--view", "--sort", "--top", "--pid", "--since"],
        );
        let refresh = if report {
            &["--interval", "--sample", "--count"][..]
        } else {
            &["--interval", "--sample"][..]
        };
        option_group(&mut output, "Refresh", refresh);
        if report {
            option_group(
                &mut output,
                "Output",
                &["--json", "--full", "--width", "--ascii", "--color"],
            );
            option_group(
                &mut output,
                "Configuration",
                &["--config", "--profile", "--theme"],
            );
        } else {
            option_group(
                &mut output,
                "Appearance",
                &["--theme", "--ascii", "--color"],
            );
            option_group(&mut output, "Configuration", &["--config", "--profile"]);
        }
        option_group(&mut output, "Help", &["--help", "--version"]);
        output.push_str("\nViews: --view uses an exact key from [views] in your configuration.\nTIME: decimal seconds or s/m/h/d; refresh/sample 0.05s to 24h; --since >0 to 365d.\n");
        if report {
            output.push_str("Repeated text redraws in a terminal and appends when redirected.\nJSON includes the complete user snapshot; --view does not limit its processes.\n--full, --width, --ascii and --color cannot be combined with --json.\n");
        } else {
            output.push_str(
                "The TUI uses the configured refresh interval; press ? inside for key bindings.\n--sort sets the starting tab's order; --top limits rows across all tabs.\n",
            );
        }
    } else {
        let common = if report {
            &[
                "--user",
                "--view",
                "--json",
                "--interval",
                "--count",
                "--help",
                "--version",
            ][..]
        } else {
            &[
                "--user",
                "--view",
                "--theme",
                "--interval",
                "--config",
                "--profile",
                "--help",
                "--version",
            ][..]
        };
        option_group(&mut output, "Common options", common);
    }
    output.push_str("\nExamples:\n");
    if report {
        output
            .push_str("  ures report --view tree --sort mem\n  ures report --json -i2 --count 3\n");
    } else {
        output.push_str("  ures --view io -i2\n  ures report --json\n");
    }
    if !detailed {
        let command = if report { "ures report" } else { "ures" };
        output.push_str(&format!("\nAll options: {command} --help."));
        if !report {
            output.push_str(" TUI key bindings: ?.");
        }
        output.push('\n');
    }
    Ok(output)
}

const CONFIG_OVERRIDES: &[&str] = &[
    "--config",
    "--profile",
    "--theme",
    "--view",
    "--sort",
    "--top",
    "--interval",
    "--sample",
    "--ascii",
];

fn config_flags(action: Option<&str>) -> &'static [&'static str] {
    match action {
        Some("path") => &["--config"],
        Some("show" | "check") => CONFIG_OVERRIDES,
        _ => &[],
    }
}

fn config_help(action: Option<&str>, detailed: bool) -> Result<String> {
    let Some(action) = action else {
        return Ok("Usage: ures config ACTION [OPTIONS]\n\nInspect or validate configuration without running resource checks.\nActions:\n  show     Effective TOML configuration\n  check    Validate configuration and all profiles\n  themes   Built-in themes\n  path     Configuration file path\n\nHelp:\n  -h, --help             Show this help\n  -V, --version          Show version\n\nUse ures config ACTION -h for common options, --help for all options.\n".into());
    };
    let description = match action {
        "path" => "Print the configuration path without reading or validating it.",
        "themes" => "List embedded theme names without reading or validating configuration.",
        "show" => "Print effective TOML configuration, including selected profile and overrides.",
        "check" => "Validate configuration and every profile without executing checks.",
        _ => {
            return Err(format!(
                "unknown config action: {action}; use ures config --help"
            ));
        }
    };
    let mut output = format!("Usage: ures config {action} [OPTIONS]\n\n{description}\n");
    let overrides = matches!(action, "show" | "check");
    let common = if overrides {
        &["--config", "--profile", "--theme"][..]
    } else {
        config_flags(Some(action))
    };
    if !common.is_empty() {
        option_group(&mut output, "Configuration", common);
    }
    if detailed && overrides {
        option_group(
            &mut output,
            "View overrides",
            &["--view", "--sort", "--top", "--ascii"],
        );
        option_group(
            &mut output,
            "Sampling overrides",
            &["--interval", "--sample"],
        );
        output.push_str("\nTIME: decimal seconds or s/m/h/d; refresh/sample 0.05s to 24h.\n");
    }
    option_group(&mut output, "Help", &["--help", "--version"]);
    if !detailed && overrides {
        output.push_str(&format!(
            "\nUse ures config {action} --help for view and sampling overrides.\n"
        ));
    }
    Ok(output)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Tui,
    Report,
    Stream,
    Config,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub command: Option<String>,
    pub action: Option<String>,
    pub user: Option<String>,
    pub config: Option<PathBuf>,
    pub profile: Option<String>,
    pub theme: Option<String>,
    pub view: Option<String>,
    pub sort: Option<String>,
    pub top: Option<usize>,
    pub interval: Option<f64>,
    pub sample: Option<f64>,
    pub count: Option<usize>,
    pub pid: Option<u32>,
    pub since: Option<f64>,
    pub full: bool,
    pub json: bool,
    pub ascii: bool,
    pub color: String,
    pub width: Option<usize>,
    pub help: bool,
    pub long_help: bool,
    pub version: bool,
    pub supplied: BTreeSet<String>,
}

impl Options {
    pub fn mode(&self) -> Mode {
        match self.command.as_deref() {
            Some("config") => Mode::Config,
            None => Mode::Tui,
            _ if self.interval.is_some() => Mode::Stream,
            _ => Mode::Report,
        }
    }
}

pub fn parse(args: &[String]) -> Result<Options> {
    let mut result = Options {
        color: "auto".into(),
        ..Options::default()
    };
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if !raw.starts_with('-') {
            if result.command.as_deref() == Some("config")
                && result.action.is_none()
                && ["path", "show", "check", "themes"].contains(&raw.as_str())
            {
                result.action = Some(raw.clone());
            } else if result.command.is_none() && ["report", "config"].contains(&raw.as_str()) {
                result.command = Some(raw.clone());
            } else {
                return Err(format!("unexpected argument: {raw}"));
            }
            i += 1;
            continue;
        }
        let (flag, attached) = if raw.starts_with("--") {
            raw.split_once('=')
                .map_or((raw.as_str(), None), |(k, v)| (k, Some(v.to_owned())))
        } else if raw.len() > 2
            && raw
                .get(..2)
                .is_some_and(|prefix| ["-u", "-s", "-n", "-i"].contains(&prefix))
        {
            (&raw[..2], Some(raw[2..].to_owned()))
        } else {
            (raw.as_str(), None)
        };
        let takes_value = match flag {
            "--full" | "--json" | "--ascii" | "-h" | "--help" | "-V" | "--version" => false,
            "-u" | "--user" | "--config" | "--profile" | "--theme" | "--view" | "-s" | "--sort"
            | "-n" | "--top" | "-i" | "--interval" | "--sample" | "--count" | "--pid"
            | "--since" | "--width" | "--color" => true,
            _ => return Err(format!("unknown option: {flag}")),
        };
        let value = if takes_value {
            if let Some(value) = attached {
                value
            } else {
                i += 1;
                args.get(i)
                    .cloned()
                    .ok_or_else(|| format!("{flag} requires a value"))?
            }
        } else {
            if attached.is_some() {
                return Err(format!("{flag} takes no value"));
            }
            String::new()
        };
        result.supplied.insert(
            match flag {
                "-u" => "--user",
                "-s" => "--sort",
                "-n" => "--top",
                "-i" => "--interval",
                "-h" => "--help",
                "-V" => "--version",
                _ => flag,
            }
            .into(),
        );
        match flag {
            "-u" | "--user" => result.user = Some(value),
            "--config" => result.config = Some(value.into()),
            "--profile" => result.profile = Some(value),
            "--theme" => result.theme = Some(value),
            "--view" => result.view = Some(value),
            "-s" | "--sort" => {
                if !["mem", "cpu", "io", "threads", "fds"].contains(&value.as_str()) {
                    return Err(format!("invalid sort: {value}"));
                }
                result.sort = Some(value);
            }
            "-n" | "--top" => {
                result.top = Some(
                    value
                        .parse()
                        .map_err(|_| "--top requires a nonnegative integer")?,
                )
            }
            "-i" | "--interval" => result.interval = Some(util::duration(&value)?),
            "--sample" => result.sample = Some(util::duration(&value)?),
            "--count" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| "--count requires a positive integer")?;
                if n == 0 {
                    return Err("--count must be positive".into());
                }
                result.count = Some(n);
            }
            "--pid" => {
                result.pid = Some(value.parse().map_err(|_| "--pid requires a positive PID")?)
            }
            "--since" => result.since = Some(util::duration(&value)?),
            "--width" => {
                let n: usize = value.parse().map_err(|_| "--width requires an integer")?;
                if !(20..=10000).contains(&n) {
                    return Err("--width must be between 20 and 10000".into());
                }
                result.width = Some(n);
            }
            "--color" => {
                if !["auto", "always", "never"].contains(&value.as_str()) {
                    return Err("--color must be auto, always or never".into());
                }
                result.color = value;
            }
            "--full" => result.full = true,
            "--json" => result.json = true,
            "--ascii" => result.ascii = true,
            "-h" | "--help" => {
                result.help = true;
                result.long_help |= flag == "--help";
            }
            "-V" | "--version" => result.version = true,
            _ => return Err(format!("unknown option: {flag}")),
        }
        i += 1;
    }
    if result.help || result.version {
        return Ok(result);
    }
    match result.mode() {
        Mode::Tui => {
            if result.json || result.count.is_some() || result.width.is_some() || result.full {
                return Err(
                    "--json, --count, --width and --full require text/JSON output; use ures report"
                        .into(),
                );
            }
        }
        Mode::Report if result.count.is_some() => {
            return Err("--count requires --interval for repeated text/JSON output".into());
        }
        Mode::Config => {
            if let Some(flag) = result
                .supplied
                .iter()
                .find(|flag| !config_flags(result.action.as_deref()).contains(&flag.as_str()))
            {
                return Err(format!(
                    "{flag} does not apply to this config action; choose an action with ures config --help"
                ));
            }
        }
        _ => {}
    }
    if result.json
        && (result.full
            || result.width.is_some()
            || result.ascii
            || result.supplied.contains("--color"))
    {
        return Err("--full, --width, --ascii and --color do not apply to JSON output".into());
    }
    if result.pid == Some(0) {
        return Err("--pid must be positive".into());
    }
    Ok(result)
}

pub fn width(explicit: Option<usize>) -> usize {
    if let Some(width) = explicit {
        return width;
    }
    if std::io::stdout().is_terminal() {
        let mut size = std::mem::MaybeUninit::<libc::winsize>::uninit();
        // SAFETY: ioctl writes the winsize buffer on success.
        if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } == 0 {
            // SAFETY: ioctl succeeded.
            let columns = unsafe { size.assume_init() }.ws_col as usize;
            if columns > 0 {
                return columns;
            }
        }
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(120)
}

pub fn color(options: &Options) -> bool {
    options.color == "always"
        || (options.color == "auto"
            && std::io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none()
            && std::env::var("TERM").is_ok_and(|t| t != "dumb"))
}

fn write(text: &str) -> Result<bool> {
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

pub fn run(args: Vec<String>) -> Result<()> {
    if args.first().is_some_and(|s| s == "help") {
        let topics: Vec<_> = args[1..]
            .iter()
            .map(String::as_str)
            .filter(|arg| !matches!(*arg, "-h" | "--help"))
            .collect();
        let (topic, action) = match topics.as_slice() {
            [] if args.len() > 1 => (Some("help"), None),
            [] => (None, None),
            [topic] => (Some(*topic), None),
            ["config", action] => (Some("config"), Some(*action)),
            _ => return Err("usage: ures help [COMMAND [ACTION]]".into()),
        };
        let detailed =
            !args.iter().any(|arg| arg == "-h") || args.iter().any(|arg| arg == "--help");
        write(&help_for(topic, action, detailed)?)?;
        return Ok(());
    }
    let options = parse(&args)?;
    if options.help {
        write(&help_for(
            options.command.as_deref(),
            options.action.as_deref(),
            options.long_help,
        )?)?;
        return Ok(());
    }
    if options.version {
        write(&format!("ures {}\n", crate::VERSION))?;
        return Ok(());
    }
    let mode = options.mode();
    if mode == Mode::Tui {
        crate::tui::require_terminal()?;
    }
    let is_config = mode == Mode::Config;
    if is_config && options.action.is_none() {
        write(&config_help(None, true)?)?;
        return Ok(());
    }
    if is_config && options.action.as_deref() == Some("themes") {
        write(
            &crate::theme::BUILTINS
                .iter()
                .map(|theme| format!("{:<12} {}\n", theme.name, theme.description))
                .collect::<String>(),
        )?;
        return Ok(());
    }
    if is_config && options.action.as_deref() == Some("path") {
        let path = options
            .config
            .clone()
            .map(Ok)
            .unwrap_or_else(config::default_path)?;
        write(&format!("{}\n", path.display()))?;
        return Ok(());
    }
    let mut config = config::load(
        options.config.as_deref(),
        options.profile.as_deref(),
        options.theme.as_deref(),
        !is_config,
    )?;
    if let Some(interval) = options.interval {
        config.interval_seconds = interval;
    }
    if let Some(sample) = options.sample {
        config.sample_seconds = sample;
    }
    config.ascii |= options.ascii;
    let view_name = options
        .view
        .clone()
        .unwrap_or_else(|| config.default_view.clone());
    let mut view =
        config.views.get(&view_name).cloned().ok_or_else(|| {
            format!("unknown view: {view_name}; check [views] in your configuration")
        })?;
    if let Some(sort) = &options.sort {
        view.sort = sort.clone();
    }
    if let Some(top) = options.top {
        view.top = top;
    }
    if options.pid.is_some() && view.kind != "tree" {
        return Err("--pid requires a tree view".into());
    }
    config.views.insert(view_name.clone(), view.clone());
    config.default_view = view_name;
    config.validate()?;
    if is_config {
        let output = match options
            .action
            .as_deref()
            .expect("config action handled above")
        {
            "show" => toml::to_string_pretty(&config).map_err(|e| e.to_string())?,
            "check" => "Configuration OK (including all profiles). No checks executed.\n".into(),
            _ => unreachable!(),
        };
        write(&output)?;
        return Ok(());
    }
    let user = util::user(options.user.as_deref())?;
    let features = engine::features(&config, &view);
    if mode == Mode::Tui {
        return crate::tui::run(user, config, options, view);
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let need_oom = engine::needs_oom(&config, std::iter::once(&view));
    let mut engine = engine::Engine::new(user, config.clone(), features, need_oom, cancelled)?;
    util::sleep(Duration::from_secs_f64(config.sample_seconds));
    let mut count = 0;
    loop {
        if util::STOP.load(Ordering::Relaxed) {
            break;
        }
        let mut report = engine.sample();
        if mode == Mode::Report {
            engine.wait_checks(&mut report);
        }
        let output = if options.json {
            format!(
                "{}\n",
                serde_json::to_string(&report).map_err(|e| e.to_string())?
            )
        } else {
            let width = width(options.width);
            let mut lines = Vec::new();
            let mut headings = render::summary(&report, config.ascii);
            for (i, line) in headings.iter_mut().enumerate() {
                *line = render::fit(line, width, config.ascii);
                let status = match i {
                    1 => report
                        .checks
                        .iter()
                        .find(|c| c.id == "cpu")
                        .map_or(crate::model::Status::Ok, |c| c.status),
                    2 => report
                        .checks
                        .iter()
                        .find(|c| c.id == "memory")
                        .map_or(crate::model::Status::Ok, |c| c.status),
                    _ => crate::model::Status::Unknown,
                };
                *line = render::colorize(line, status, &config, color(&options));
            }
            lines.extend(headings);
            lines.push(String::new());
            match view.kind.as_str() {
                "oom" | "checks" => lines.extend(render::text_view(
                    &report,
                    &view,
                    options.since,
                    "",
                    width,
                    config.ascii,
                )),
                _ => {
                    let table = render::table(
                        &report,
                        &config,
                        &view,
                        &render::Options {
                            full: options.full,
                            ascii: config.ascii,
                            pid: options.pid,
                            ..render::Options::default()
                        },
                        width,
                    );
                    lines.push(render::colorize(
                        &table.header,
                        crate::model::Status::Ok,
                        &config,
                        color(&options),
                    ));
                    lines.extend(table.lines);
                    lines.push(String::new());
                    for note in table.notes {
                        lines.push(render::fit(&note, width, config.ascii));
                    }
                    if need_oom && !report.oom.events.is_empty() {
                        lines.push(String::new());
                        lines.push(render::fit(
                            "RECENT OOM (retained kernel records)",
                            width,
                            config.ascii,
                        ));
                        lines.extend(render::oom_lines(
                            &report,
                            options.since,
                            3,
                            width,
                            config.ascii,
                        ));
                    }
                }
            }
            for warning in &report.warnings {
                lines.push(render::fit(
                    &format!("Note: {warning}"),
                    width,
                    config.ascii,
                ));
            }
            format!("{}\n", lines.join("\n"))
        };
        if mode == Mode::Stream
            && !options.json
            && std::io::stdout().is_terminal()
            && std::env::var("TERM").is_ok_and(|s| s != "dumb")
            && !write("\x1b[H\x1b[2J")?
        {
            break;
        }
        if !write(&output)? {
            break;
        }
        count += 1;
        if mode == Mode::Report || options.count.is_some_and(|n| count >= n) {
            break;
        }
        util::sleep(Duration::from_secs_f64(config.interval_seconds));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(args: &[&str]) -> Result<Options> {
        parse(&args.iter().map(|s| (*s).into()).collect::<Vec<_>>())
    }

    #[test]
    fn explicit_modes_keep_default_interactive_and_scripts_predictable() {
        for args in [vec![], vec!["-i2"], vec!["--view", "io"]] {
            assert_eq!(options(&args).unwrap().mode(), Mode::Tui, "{args:?}");
        }
        for args in [vec!["report"], vec!["report", "--json"]] {
            assert_eq!(options(&args).unwrap().mode(), Mode::Report, "{args:?}");
        }
        for args in [
            vec!["report", "-i2"],
            vec!["report", "--view", "tree", "-i2"],
            vec!["report", "--json", "-i2", "--count", "2"],
        ] {
            assert_eq!(options(&args).unwrap().mode(), Mode::Stream, "{args:?}");
        }
        assert_eq!(
            options(&["config", "show", "-i2"]).unwrap().mode(),
            Mode::Config
        );
    }

    #[test]
    fn rejects_ignored_output_options_and_invalid_mode_combinations() {
        for args in [
            vec!["--count", "2", "-i2"],
            vec!["--full"],
            vec!["--width", "80"],
            vec!["tui", "--json"],
            vec!["tui"],
            vec!["--json"],
            vec!["report", "--limit", "2"],
            vec!["report", "--json", "--width", "80"],
            vec!["report", "--count", "2"],
            vec!["config", "--json"],
            vec!["config", "--user", "alice"],
            vec!["watch"],
            vec!["watch", "--json", "-i2"],
            vec!["unknown"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
        assert!(
            options(&["--unknown"])
                .unwrap_err()
                .contains("unknown option")
        );
    }

    #[test]
    fn help_topics_skip_values_and_describe_the_requested_command() {
        for (args, topic) in [
            (vec!["--help"], None),
            (vec!["--profile", "report", "--help"], None),
            (vec!["--config", "tree", "report", "--help"], Some("report")),
            (vec!["--user=alice", "report", "--help"], Some("report")),
            (vec!["config", "check", "--help"], Some("config")),
        ] {
            let args: Vec<_> = args.into_iter().map(String::from).collect();
            let options = parse(&args).unwrap();
            assert_eq!(options.command.as_deref(), topic);
            assert!(options.help);
        }
        assert!(help(Some("watch")).is_err());
        assert!(help(Some("tui")).is_err());
        assert!(help(Some("report")).unwrap().contains("then exit"));
        assert!(help(Some("config")).unwrap().contains("Effective TOML"));
        assert!(help(Some("unknown")).is_err());
        for command in ["tree", "ps", "groups", "oom", "checks"] {
            assert!(options(&[command]).is_err());
            assert!(options(&[command, "--help"]).is_err());
            assert!(help(Some(command)).is_err());
        }
        assert!(
            config_help(Some("path"), true)
                .unwrap()
                .contains("without reading")
        );
        assert!(
            !config_help(Some("themes"), true)
                .unwrap()
                .contains("--config")
        );
        for action in ["show", "check"] {
            assert!(!config_help(Some(action), true).unwrap().contains("--full"));
        }
        for args in [
            vec!["config", "typo", "--help"],
            vec!["--unknown", "report", "--help"],
        ] {
            assert!(options(&args).is_err());
        }
    }

    fn help_flags(text: &str) -> BTreeSet<&str> {
        text.lines()
            .filter(|line| line.starts_with("  ") && line.trim_start().starts_with('-'))
            .filter_map(|line| line.split_whitespace().find(|word| word.starts_with("--")))
            .collect()
    }

    #[test]
    fn brief_help_is_small_and_full_help_only_lists_applicable_options() {
        let all: BTreeSet<_> = HELP_OPTIONS.iter().map(|option| option.flag).collect();
        for topic in [None, Some("report")] {
            let brief = help_for(topic, None, false).unwrap();
            let full = help_for(topic, None, true).unwrap();
            assert!(brief.lines().count() <= 28, "{topic:?}");
            assert!(brief.contains("Common options:\n"));
            assert!(!brief.contains("--sample"));
            assert!(full.contains("Selection:\n"));
            assert!(full.contains("Refresh:\n"));
            assert!(help_flags(&brief).is_subset(&help_flags(&full)));
            let mut expected = all.clone();
            if topic.is_none() {
                for flag in ["--json", "--full", "--width", "--count"] {
                    expected.remove(flag);
                }
            }
            assert_eq!(help_flags(&full), expected, "{topic:?}");
            // Each option appears in exactly one group.
            let option_lines = full
                .lines()
                .filter(|line| line.starts_with("  ") && line.trim_start().starts_with('-'))
                .count();
            assert_eq!(option_lines, expected.len());
        }
        for action in ["path", "show", "check", "themes"] {
            let brief = config_help(Some(action), false).unwrap();
            let full = config_help(Some(action), true).unwrap();
            let mut expected: BTreeSet<_> = config_flags(Some(action)).iter().copied().collect();
            expected.extend(["--help", "--version"]);
            assert_eq!(help_flags(&full), expected, "{action}");
            assert!(help_flags(&brief).is_subset(&expected));
            assert!(brief.lines().count() <= 16, "{action}");
        }
    }

    #[test]
    fn help_detail_does_not_change_option_validation_or_value_parsing() {
        for (args, detailed) in [
            (vec!["-h"], false),
            (vec!["--help"], true),
            (vec!["--help", "-h"], true),
            (vec!["-h", "--help"], true),
            (vec!["--config", "--help", "-h"], false),
            (vec!["--config=--help", "-h"], false),
        ] {
            let parsed = options(&args).unwrap();
            assert!(parsed.help);
            assert_eq!(parsed.long_help, detailed, "{args:?}");
        }
        for flag in ["-h", "--help"] {
            for args in [vec!["--unknown", flag], vec!["config", "typo", flag]] {
                assert!(options(&args).is_err(), "{args:?}");
            }
        }
    }

    #[test]
    fn config_actions_reject_even_explicit_default_no_effect_flags() {
        for args in [
            vec!["config", "--profile", "work"],
            vec!["config", "path", "--profile", "work"],
            vec!["config", "themes", "--config", "fixture.toml"],
            vec!["config", "show", "--color", "auto"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
        for args in [
            vec!["config"],
            vec!["config", "path", "--config", "fixture.toml"],
            vec!["config", "check", "-i2", "--ascii"],
            vec!["config", "show", "--theme", "light", "-n2"],
        ] {
            assert!(options(&args).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn removed_config_views_action_is_rejected_even_with_help() {
        for args in [
            vec!["config", "views"],
            vec!["config", "views", "-h"],
            vec!["config", "views", "--help"],
            vec!["config", "views", "--profile", "work"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
        for detailed in [false, true] {
            assert!(help_for(Some("config"), Some("views"), detailed).is_err());
            let group = config_help(None, detailed).unwrap();
            assert!(
                !group
                    .lines()
                    .any(|line| line.trim_start().starts_with("views "))
            );
        }
    }

    #[test]
    fn parses_compact_and_long_options_without_shell_expansion() {
        let args = [
            "report",
            "-ualice",
            "-scpu",
            "-n0",
            "-i2",
            "--count=2",
            "--config=/tmp/$(touch bad)",
        ];
        let parsed = parse(&args.map(String::from)).unwrap();
        assert_eq!(parsed.user.as_deref(), Some("alice"));
        assert_eq!(parsed.top, Some(0));
        assert_eq!(parsed.interval, Some(2.0));
        assert_eq!(parsed.config, Some(PathBuf::from("/tmp/$(touch bad)")));
        for args in [
            vec!["--count", "0"],
            vec!["--sample", "NaN"],
            vec!["--pid", "0"],
            vec!["--sort", "invalid"],
            vec!["--json=yes"],
        ] {
            assert!(parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }
}
