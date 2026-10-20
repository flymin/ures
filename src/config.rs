pub use crate::theme::Theme;
use crate::{Result, util};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const TEMPLATE: &str = include_str!("../config/default.toml");
pub const FIELDS: &[&str] = &[
    "pid",
    "cpu",
    "mem",
    "self_mem",
    "rss",
    "processes",
    "threads",
    "fds",
    "read",
    "write",
    "command",
];
pub const BUILTINS: &[&str] = &["cpu", "memory", "oom", "io", "threads", "fds"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub mode: String,
    pub cpu_cores: Option<f64>,
    pub memory: Option<String>,
    pub cpu_cgroup: Option<String>,
    pub memory_cgroup: Option<String>,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
            cpu_cores: None,
            memory: None,
            cpu_cgroup: None,
            memory_cgroup: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Check {
    pub enabled: bool,
    pub warn: Option<f64>,
    pub critical: Option<f64>,
    pub command: Vec<String>,
    pub interval_seconds: f64,
    pub timeout_seconds: f64,
}
impl Default for Check {
    fn default() -> Self {
        Self {
            enabled: true,
            warn: None,
            critical: None,
            command: Vec::new(),
            interval_seconds: 10.0,
            timeout_seconds: 2.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    #[serde(default)]
    pub name: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default = "default_sort")]
    pub sort: String,
    #[serde(default = "default_top")]
    pub top: usize,
    pub include: Option<String>,
    pub exclude: Option<String>,
}
fn default_sort() -> String {
    "mem".into()
}
fn default_top() -> usize {
    20
}
impl View {
    pub fn is_table(&self) -> bool {
        matches!(self.kind.as_str(), "tree" | "ps" | "groups")
    }
    pub fn label<'a>(&'a self, key: &'a str) -> &'a str {
        self.name.as_deref().unwrap_or(key)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Column {
    pub title: Option<String>,
    /// Preferred reserved width in the TUI; independent of sampled values.
    pub width: Option<usize>,
    pub min_width: usize,
    pub max_width: Option<usize>,
    pub optional: bool,
    pub priority: u32,
}
impl Default for Column {
    fn default() -> Self {
        Self {
            title: None,
            width: None,
            min_width: 1,
            max_width: None,
            optional: false,
            priority: 50,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub name: String,
    #[serde(rename = "match")]
    pub pattern: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub default_view: String,
    #[serde(default)]
    pub tab_order: Vec<String>,
    pub default_profile: Option<String>,
    pub sample_seconds: f64,
    pub interval_seconds: f64,
    pub ascii: bool,
    pub limits: Limits,
    pub checks: BTreeMap<String, Check>,
    pub views: BTreeMap<String, View>,
    #[serde(default)]
    pub columns: BTreeMap<String, Column>,
    pub theme: Theme,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub profiles: BTreeMap<String, toml::Table>,
}

pub fn merge(base: &mut toml::Value, patch: toml::Value) {
    match (base, patch) {
        (toml::Value::Table(base), toml::Value::Table(patch)) => {
            for (key, value) in patch {
                if let Some(old) = base.get_mut(&key) {
                    merge(old, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, patch) => *base = patch,
    }
}

fn finite_positive(value: f64, label: &str) -> Result<()> {
    if value.is_finite() && (0.05..=86400.0).contains(&value) {
        Ok(())
    } else {
        Err(format!("{label} must be finite and between 0.05 and 86400"))
    }
}

impl Config {
    pub fn parse(text: &str, profile: Option<&str>) -> Result<Self> {
        Self::parse_with_theme(text, profile, None)
    }

    pub fn parse_with_theme(
        text: &str,
        profile: Option<&str>,
        theme: Option<&str>,
    ) -> Result<Self> {
        let mut value: toml::Value = toml::from_str(TEMPLATE).expect("embedded config");
        let patch: toml::Value = toml::from_str(text).map_err(|e| format!("invalid TOML: {e}"))?;
        // An explicit views table selects the available views. Preserve builtin
        // field defaults for legacy partial overrides, without adding tabs the
        // user did not select or inventing display names they did not specify.
        if let Some(views) = patch.get("views").and_then(toml::Value::as_table) {
            let mut selected = toml::Table::new();
            for key in views.keys() {
                if let Some(default) = value["views"].get(key) {
                    let mut default = default.clone();
                    default.as_table_mut().unwrap().remove("name");
                    selected.insert(key.clone(), default);
                }
            }
            value["views"] = toml::Value::Table(selected);
            // Keep inherited ordering applicable to a selected subset. Explicit
            // user orders are validated strictly instead of silently filtered.
            if patch.get("tab_order").is_none() {
                value["tab_order"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|key| key.as_str().is_some_and(|key| views.contains_key(key)));
            }
        }
        merge(&mut value, patch.clone());
        if patch.get("default_view").is_none()
            && value["views"]
                .get(value["default_view"].as_str().unwrap())
                .is_none()
            && let Some(key) = value["tab_order"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .find(|key| value["views"].get(*key).is_some())
                .map(str::to_owned)
                .or_else(|| {
                    value["views"]
                        .as_table()
                        .and_then(|v| v.keys().next())
                        .cloned()
                })
        {
            value["default_view"] = toml::Value::String(key);
        }
        let base = Self::decode(&value, None)?;
        // Validate every profile, even when another is selected.
        for (name, patch) in &base.profiles {
            if patch.contains_key("profiles") || patch.contains_key("default_profile") {
                return Err(format!(
                    "profile {name}: cannot redefine profiles or default_profile"
                ));
            }
            let mut candidate = value.clone();
            merge(&mut candidate, toml::Value::Table(patch.clone()));
            Self::decode(&candidate, None).map_err(|e| format!("profile {name}: {e}"))?;
        }
        if let Some(name) = profile.or(base.default_profile.as_deref()) {
            let patch = base
                .profiles
                .get(name)
                .ok_or_else(|| profile_error("profile", name))?;
            merge(&mut value, toml::Value::Table(patch.clone()));
        }
        Self::decode(&value, theme)
    }

    fn decode(value: &toml::Value, theme: Option<&str>) -> Result<Self> {
        let mut value = value.clone();
        let resolved = Theme::resolve(
            value.get("theme").ok_or("missing theme configuration")?,
            theme,
        )?;
        value["theme"] = toml::Value::try_from(resolved).map_err(|e| e.to_string())?;
        let config: Self = value
            .try_into()
            .map_err(|e| format!("invalid config: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn builtin() -> Self {
        Self::parse("", None).expect("embedded config")
    }

    /// Explicit keys first, then unlisted views in deterministic key order.
    pub fn view_keys(&self) -> Vec<String> {
        let listed: BTreeSet<_> = self.tab_order.iter().collect();
        self.tab_order
            .iter()
            .chain(self.views.keys().filter(|key| !listed.contains(key)))
            .cloned()
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported schema_version {}; expected 1",
                self.schema_version
            ));
        }
        if self.views.is_empty() {
            return Err("views must contain at least one view".into());
        }
        if !self.views.contains_key(&self.default_view) {
            return Err(format!("unknown default_view: {}", self.default_view));
        }
        let mut seen = BTreeSet::new();
        for key in &self.tab_order {
            if !self.views.contains_key(key) {
                return Err(format!("tab_order: unknown view {key:?}"));
            }
            if !seen.insert(key) {
                return Err(format!("tab_order: duplicate view {key:?}"));
            }
        }
        if let Some(profile) = &self.default_profile
            && !self.profiles.contains_key(profile)
        {
            return Err(profile_error("default_profile", profile));
        }
        finite_positive(self.sample_seconds, "sample_seconds")?;
        finite_positive(self.interval_seconds, "interval_seconds")?;
        if !matches!(
            self.limits.mode.as_str(),
            "auto" | "manual" | "auto-or-manual"
        ) {
            return Err("limits.mode: expected auto, manual or auto-or-manual".into());
        }
        if let Some(cpu) = self.limits.cpu_cores
            && (!cpu.is_finite() || cpu <= 0.0)
        {
            return Err("limits.cpu_cores must be positive and finite".into());
        }
        if let Some(memory) = &self.limits.memory {
            util::bytes(memory)?;
        }
        if self.limits.mode == "manual"
            && self.limits.cpu_cores.is_none()
            && self.limits.memory.is_none()
        {
            return Err("manual limits need cpu_cores or memory".into());
        }
        for (key, path) in [
            ("cpu_cgroup", &self.limits.cpu_cgroup),
            ("memory_cgroup", &self.limits.memory_cgroup),
        ] {
            if let Some(path) = path
                && !Path::new(path).is_absolute()
            {
                return Err(format!("limits.{key} must be an absolute path"));
            }
        }
        for (name, check) in &self.checks {
            if name.is_empty() || name.chars().any(char::is_control) {
                return Err(
                    "check names must be nonempty and contain no control characters".into(),
                );
            }
            if check.command.is_empty() && !BUILTINS.contains(&name.as_str()) {
                return Err(format!("check {name}: unknown builtin; provide command"));
            }
            if check.command.iter().any(|s| s.contains('\0'))
                || check.command.first().is_some_and(String::is_empty)
            {
                return Err(format!("check {name}: invalid command"));
            }
            finite_positive(
                check.interval_seconds,
                &format!("checks.{name}.interval_seconds"),
            )?;
            finite_positive(
                check.timeout_seconds,
                &format!("checks.{name}.timeout_seconds"),
            )?;
            for value in [check.warn, check.critical].into_iter().flatten() {
                if !value.is_finite() || value < 0.0 {
                    return Err(format!(
                        "check {name}: thresholds must be nonnegative and finite"
                    ));
                }
            }
            if let (Some(w), Some(c)) = (check.warn, check.critical)
                && w > c
            {
                return Err(format!("check {name}: warn must not exceed critical"));
            }
        }
        for (name, view) in &self.views {
            if name.trim().is_empty()
                || name.chars().any(char::is_control)
                || view
                    .name
                    .as_ref()
                    .is_some_and(|v| v.trim().is_empty() || v.chars().any(char::is_control))
            {
                return Err(format!(
                    "view {name:?}: key and display name must be nonempty and contain no control characters"
                ));
            }
            if !matches!(
                view.kind.as_str(),
                "tree" | "ps" | "groups" | "oom" | "checks"
            ) {
                return Err(format!(
                    "view {name}: kind must be tree, ps, groups, oom or checks"
                ));
            }
            if !["cpu", "mem", "io", "threads", "fds"].contains(&view.sort.as_str()) {
                return Err(format!("view {name}: unknown sort {}", view.sort));
            }
            let mut seen = BTreeSet::new();
            if view.is_table() && view.columns.is_empty() {
                return Err(format!("view {name}: columns must not be empty"));
            }
            for field in &view.columns {
                if !FIELDS.contains(&field.as_str()) || !seen.insert(field) {
                    return Err(format!("view {name}: unknown or duplicate column {field}"));
                }
            }
            for pattern in [&view.include, &view.exclude].into_iter().flatten() {
                regex::Regex::new(pattern).map_err(|e| format!("view {name}: {e}"))?;
            }
        }
        for (field, col) in &self.columns {
            if !FIELDS.contains(&field.as_str()) {
                return Err(format!("unknown column: {field}"));
            }
            if col.min_width == 0
                || col.max_width.is_some_and(|v| v < col.min_width)
                || col
                    .width
                    .is_some_and(|v| v < col.min_width || col.max_width.is_some_and(|max| v > max))
            {
                return Err(format!("column {field}: invalid widths"));
            }
            if col
                .title
                .as_ref()
                .is_some_and(|t| t.is_empty() || t.chars().any(char::is_control))
            {
                return Err(format!("column {field}: invalid title"));
            }
        }
        for group in &self.groups {
            if group.name.is_empty() || group.name.chars().any(char::is_control) {
                return Err("group name must be nonempty and contain no control characters".into());
            }
            regex::Regex::new(&group.pattern).map_err(|e| format!("group {}: {e}", group.name))?;
        }
        self.theme.validate()?;
        Ok(())
    }
}

pub fn default_path() -> Result<PathBuf> {
    default_path_from(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}
pub fn default_path_from(
    xdg: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Result<PathBuf> {
    if let Some(xdg) = xdg.filter(|s| !s.is_empty() && Path::new(s).is_absolute()) {
        return Ok(Path::new(xdg).join("ures/config.toml"));
    }
    home.filter(|s| !s.is_empty())
        .map(|p| Path::new(p).join(".config/ures/config.toml"))
        .ok_or_else(|| "HOME is unset; provide --config FILE".into())
}

pub fn create_default(path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("config needs a parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".ures-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(TEMPLATE.as_bytes())?;
        file.sync_all()?;
        match std::fs::hard_link(&temp, path) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            result => result,
        }
    })();
    let _ = std::fs::remove_file(temp);
    result
}

fn profile_error(field: &str, name: &str) -> String {
    let hint = if crate::theme::is_builtin(name) {
        format!(
            "; {name:?} is a built-in theme: use --theme {name} or [theme] name = {name:?}, or define your own [profiles.{name}]"
        )
    } else {
        String::new()
    };
    format!("unknown {field}: {name}{hint}")
}

pub fn load(
    explicit: Option<&Path>,
    profile: Option<&str>,
    theme: Option<&str>,
    create: bool,
) -> Result<Config> {
    let path = match explicit
        .map(Path::to_owned)
        .map(Ok)
        .unwrap_or_else(default_path)
    {
        Ok(path) => path,
        Err(error) if create => {
            eprintln!("ures: {error}; using built-in config");
            return Config::parse_with_theme("", profile, theme);
        }
        Err(error) => return Err(error),
    };
    if create
        && explicit.is_none()
        && !path.exists()
        && let Err(error) = create_default(&path)
    {
        eprintln!(
            "ures: cannot create {}: {error}; using built-in config",
            path.display()
        );
        return Config::parse_with_theme("", profile, theme);
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if explicit.is_none() && !create && e.kind() == std::io::ErrorKind::NotFound => {
            String::new()
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    Config::parse_with_theme(&text, profile, theme).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn themes_and_user_profiles_are_independent_and_overrides_have_clear_precedence() {
        let text = r##"
default_profile = "work"
[theme]
name = "gruvbox"
cpu = "#123456"
[profiles.work]
interval_seconds = 3.0
[profiles.work.limits]
mode = "manual"
cpu_cores = 2.0
[profiles.work.theme]
name = "light"
write = "#abcdef"
"##;
        let config = Config::parse(text, None).unwrap();
        assert_eq!(config.theme.name, "light");
        assert_eq!(
            config.theme.background,
            Theme::builtin("light").unwrap().background
        );
        assert_eq!(config.theme.cpu, "#123456");
        assert_eq!(config.theme.write, "#abcdef");
        assert_eq!(config.interval_seconds, 3.0);
        assert_eq!(config.limits.cpu_cores, Some(2.0));
        let config = Config::parse_with_theme(text, None, Some("dracula")).unwrap();
        assert_eq!(config.theme.name, "dracula");
        assert_eq!(
            config.theme.background,
            Theme::builtin("dracula").unwrap().background
        );
        assert_eq!(config.theme.cpu, "#123456");
        assert_eq!(config.theme.write, "#abcdef");
        assert_eq!(config.interval_seconds, 3.0);
        assert!(Config::builtin().profiles.is_empty());
        let error = Config::parse("", Some("light")).unwrap_err();
        assert!(error.contains("--theme light"));
        assert!(
            Config::parse("default_profile='light'", None)
                .unwrap_err()
                .contains("--theme light")
        );
        let user = "[profiles.light]\ninterval_seconds=7.0";
        assert_eq!(
            Config::parse(user, Some("light")).unwrap().interval_seconds,
            7.0
        );
        assert_eq!(
            Config::parse_with_theme(user, None, Some("light"))
                .unwrap()
                .interval_seconds,
            2.0
        );
        assert_eq!(
            Config::parse(user, Some("light")).unwrap().theme.name,
            "dracula"
        );
    }

    #[test]
    fn theme_selectors_typos_and_unselected_profile_overrides_are_validated() {
        for text in [
            "[theme]\nname='missing'",
            "[theme]\nname=42",
            "theme='light'",
            "[theme]\ncolours='red'",
            "[profiles.bad.theme]\nname='missing'",
            "[profiles.bad.theme]\ncpu='invalid-color'",
        ] {
            assert!(
                Config::parse_with_theme(text, None, Some("light")).is_err(),
                "{text}"
            );
        }
        assert!(
            Config::parse_with_theme("", None, Some("missing"))
                .unwrap_err()
                .contains("available:")
        );
        let config = Config::parse("[theme]\naccent='blue'", None).unwrap();
        assert_eq!(config.theme.name, "dracula");
        assert_eq!(config.theme.accent, "blue");
    }
    #[test]
    fn profile_patches_and_array_replacement_are_predictable() {
        let config=Config::parse("[profiles.lab.limits]\nmode='manual'\ncpu_cores=4.0\nmemory='8GiB'\n[profiles.lab.views.tree]\ncolumns=['pid','command']",Some("lab")).unwrap();
        assert_eq!(config.limits.cpu_cores, Some(4.0));
        assert_eq!(config.views["tree"].columns, vec!["pid", "command"]);
        assert!(config.checks["cpu"].enabled);
    }
    #[test]
    fn explicit_views_select_tabs_and_legacy_overrides_keep_field_defaults() {
        let config =
            Config::parse("[views.io]\nname='Disk'\n[views.tree]\nsort='cpu'", None).unwrap();
        assert_eq!(
            config.views.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["io", "tree"]
        );
        assert_eq!(config.views["io"].label("io"), "Disk");
        assert_eq!(config.views["tree"].label("tree"), "tree");
        assert_eq!(config.views["tree"].kind, "tree");
        assert!(config.views["io"].columns.contains(&"read".into()));
        let config = Config::parse("[views.custom]\nkind='oom'", None).unwrap();
        assert_eq!(config.default_view, "custom");
        assert_eq!(config.views.len(), 1);
        let config = Config::parse(
            "[views.base]\nkind='checks'\n[profiles.extra.views.events]\nkind='oom'",
            Some("extra"),
        )
        .unwrap();
        assert_eq!(config.views.len(), 2);
    }

    #[test]
    fn invalid_tab_names_kinds_and_default_selection_are_rejected() {
        for text in [
            "[views]",
            "[views.custom]\nkind='ps'",
            "[views.custom]\nkind='unknown'",
            "[views.tree]\nname='  '",
            "[views.tree]\nname=42",
            "[views.tree]\nname=\"bad\\nname\"",
            "[views.'']\nkind='oom'",
            "default_view='missing'\n[views.custom]\nkind='oom'",
        ] {
            assert!(Config::parse(text, None).is_err(), "{text}");
        }
    }

    #[test]
    fn tab_order_supports_partial_lists_profiles_and_selected_view_sets() {
        let config = Config::parse("tab_order=['c']\n[views.a]\nkind='checks'\n[views.b]\nkind='oom'\n[views.c]\nkind='checks'", None).unwrap();
        assert_eq!(config.view_keys(), vec!["c", "a", "b"]);
        assert_eq!(config.default_view, "c");
        let text = "tab_order=['io','tree']\n[views.io]\n[views.tree]\n[profiles.flip]\ntab_order=['tree','io']";
        assert_eq!(
            Config::parse(text, None).unwrap().view_keys(),
            vec!["io", "tree"]
        );
        assert_eq!(
            Config::parse(text, Some("flip")).unwrap().view_keys(),
            vec!["tree", "io"]
        );
        assert_eq!(
            Config::parse("[views.io]\n[views.tree]", None)
                .unwrap()
                .view_keys(),
            vec!["tree", "io"]
        );
        assert_eq!(
            Config::parse("tab_order=[]\n[views.io]\n[views.tree]", None)
                .unwrap()
                .view_keys(),
            vec!["io", "tree"]
        );
        assert_eq!(
            Config::builtin().view_keys(),
            vec!["tree", "ps", "io", "groups", "compact", "oom", "checks"]
        );
    }

    #[test]
    fn invalid_tab_orders_and_selection_colors_are_rejected() {
        for text in [
            "tab_order=['tree','tree']",
            "tab_order=['missing']",
            "tab_order='tree'",
            "tab_order=[1]",
            "[profiles.bad]\ntab_order=['missing']",
            "[theme]\nselection_bg='invalid-color'",
            "[theme]\nselection_fg='invalid-color'",
            "[theme]\nforeground='invalid-color'",
            "[theme]\nbackground='invalid-color'",
        ] {
            assert!(Config::parse(text, None).is_err(), "{text}");
        }
    }

    #[test]
    fn complete_v010_config_is_compatible_without_adding_tabs() {
        let config =
            Config::parse(include_str!("../tests/fixtures/config-v0.1.0.toml"), None).unwrap();
        assert_eq!(config.views.len(), 5);
        assert_eq!(config.default_view, "tree");
        assert!(config.views.values().all(|v| v.name.is_none()));
        assert!(config.theme.rainbow);
    }
    #[test]
    fn builtin_theme_text_stays_readable_against_its_background() {
        let luminance = |hex: &str| {
            let channels: Vec<f64> = [1, 3, 5]
                .iter()
                .map(|offset| {
                    let c =
                        u8::from_str_radix(&hex[*offset..*offset + 2], 16).unwrap() as f64 / 255.0;
                    if c <= 0.04045 {
                        c / 12.92
                    } else {
                        ((c + 0.055) / 1.055).powf(2.4)
                    }
                })
                .collect();
            channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
        };
        for theme in crate::theme::BUILTINS {
            let config = Config::parse_with_theme("", None, Some(theme.name)).unwrap();
            let bg = luminance(&config.theme.background);
            for text in [
                &config.theme.foreground,
                &config.theme.muted,
                &config.theme.tree,
            ] {
                let fg = luminance(text);
                let ratio = (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05);
                assert!(ratio >= 4.5, "{}: {text} contrast={ratio}", theme.name);
            }
        }
    }
    #[test]
    fn invalid_unselected_profile_and_typo_are_rejected() {
        for text in [
            "sample_seconds=nan",
            "typo=1",
            "[profiles.bad.limits]\ncpu_cores=-1.0",
            "[views.tree]\ncolumns=['pid','pid']",
            "[checks.mine]\nenabled=true",
        ] {
            assert!(Config::parse(text, None).is_err(), "{text}");
        }
    }
    #[test]
    fn config_path_matches_xdg_convention() {
        assert_eq!(
            default_path_from(Some("/xdg".as_ref()), Some("/home/u".as_ref())).unwrap(),
            PathBuf::from("/xdg/ures/config.toml")
        );
        assert_eq!(
            default_path_from(Some("relative".as_ref()), Some("/home/u".as_ref())).unwrap(),
            PathBuf::from("/home/u/.config/ures/config.toml")
        );
    }
    #[test]
    fn legacy_theme_configs_merge_new_defaults_and_validate_customizations() {
        let config = Config::parse("[theme]\naccent='blue'", None).unwrap();
        assert!(config.theme.rainbow);
        assert_eq!(config.theme.accent, "blue");
        assert!(!config.theme.rainbow_colors.is_empty());
        assert!(
            !Config::parse("[theme]\nrainbow=false", None)
                .unwrap()
                .theme
                .rainbow
        );
        for text in [
            "[theme]\nrainbow_colors=[]",
            "[theme]\nrainbow_colors=['invalid-color']",
            "[theme]\ncpu='invalid-color'",
            "[columns.pid]\nwidth=0",
            "[columns.read]\nwidth=12\nmax_width=8",
        ] {
            assert!(Config::parse(text, None).is_err(), "{text}");
        }
    }
    #[test]
    fn simultaneous_creation_never_replaces_existing_file() {
        let dir = std::env::temp_dir().join(format!("ures-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| create_default(&path).unwrap());
            }
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
        std::fs::write(&path, "# mine").unwrap();
        create_default(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# mine");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
