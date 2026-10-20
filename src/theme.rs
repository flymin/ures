//! Built-in palettes and flat `[theme]` color overrides, independent of profiles.
use crate::Result;
use serde::{Deserialize, Serialize};

pub const DEFAULT: &str = "dracula";

pub struct Builtin {
    pub name: &'static str,
    pub description: &'static str,
    source: &'static str,
}

pub const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "dracula",
        description: "Dracula Classic — bright accents, charcoal background",
        source: include_str!("../config/themes/dracula.toml"),
    },
    Builtin {
        name: "light",
        description: "Alucard Classic — dark text, ivory background",
        source: include_str!("../config/themes/light.toml"),
    },
    Builtin {
        name: "gruvbox",
        description: "Gruvbox — warm text and bright accents",
        source: include_str!("../config/themes/gruvbox.toml"),
    },
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub accent: String,
    pub header: String,
    pub muted: String,
    pub warning: String,
    pub critical: String,
    pub cpu: String,
    pub memory: String,
    pub tree: String,
    pub read: String,
    pub write: String,
    pub selection_fg: String,
    pub selection_bg: String,
    pub rainbow: bool,
    pub rainbow_colors: Vec<String>,
}

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.iter().any(|theme| theme.name == name)
}

impl Theme {
    pub fn builtin(name: &str) -> Result<Self> {
        let entry = BUILTINS
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| {
                format!(
                    "unknown theme {name:?}; available: {}",
                    BUILTINS
                        .iter()
                        .map(|t| t.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        toml::from_str(entry.source).map_err(|e| format!("invalid built-in theme {name}: {e}"))
    }

    /// Resolve the chosen palette before applying explicit color overrides.
    /// `forced_name` changes only the selector, never discards user overrides.
    pub fn resolve(spec: &toml::Value, forced_name: Option<&str>) -> Result<Self> {
        let table = spec.as_table().ok_or("theme must be a table")?;
        let configured = match table.get("name") {
            Some(value) => value.as_str().ok_or("theme.name must be a string")?,
            None => DEFAULT,
        };
        let name = forced_name.unwrap_or(configured);
        let mut value = toml::Value::try_from(Self::builtin(name)?).map_err(|e| e.to_string())?;
        for (key, color) in table {
            if key != "name" {
                value
                    .as_table_mut()
                    .unwrap()
                    .insert(key.clone(), color.clone());
            }
        }
        let theme: Self = value
            .try_into()
            .map_err(|e| format!("invalid theme: {e}"))?;
        theme.validate()?;
        Ok(theme)
    }

    pub fn validate(&self) -> Result<()> {
        if !is_builtin(&self.name) {
            return Err(format!(
                "unknown theme {:?}; use ures config themes",
                self.name
            ));
        }
        if self.rainbow_colors.is_empty() {
            return Err("theme.rainbow_colors must not be empty".into());
        }
        for color in [
            &self.foreground,
            &self.background,
            &self.accent,
            &self.header,
            &self.muted,
            &self.warning,
            &self.critical,
            &self.cpu,
            &self.memory,
            &self.tree,
            &self.read,
            &self.write,
            &self.selection_fg,
            &self.selection_bg,
        ]
        .into_iter()
        .chain(&self.rainbow_colors)
        {
            color
                .parse::<ratatui::style::Color>()
                .map_err(|_| format!("unknown theme color: {color}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_palettes_are_unique_valid_and_match_their_names() {
        let mut names = std::collections::BTreeSet::new();
        for entry in BUILTINS {
            assert!(names.insert(entry.name));
            let theme = Theme::builtin(entry.name).unwrap();
            assert_eq!(theme.name, entry.name);
            theme.validate().unwrap();
        }
    }
}
