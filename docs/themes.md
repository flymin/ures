# Themes

The default maps the open-source **Dracula Classic** palette to resource metrics:
green CPU, yellow memory, cyan reads, orange writes and red critical values.
Body text uses `#f8f8f2` on `#282a36`. Secondary text and tree guides use a lighter
foreground/background mix (`#c4c5c3`) instead of the upstream comment color, so
labels and footers remain readable. Purple is limited to identity colors.

The TUI paints the full foreground/background, including empty cells and popups.
Selected rows use explicit colors across columns, gaps and line ends. `NO_COLOR`
and `--color never` preserve terminal defaults. Set either `theme.foreground`
or `theme.background` to `"reset"` to inherit that terminal color individually.

| Selection | Palette | Appearance |
| --- | --- | --- |
| `ures` | Dracula Classic | Bright accents, charcoal background |
| `ures --theme light` | Alucard Classic | Dark text, ivory background |
| `ures --theme gruvbox` | Gruvbox dark / bright accents | Warm text and colors |

List the embedded choices with `ures config themes`. This command reads no config
and executes no checks. Palette data lives in `config/themes/*.toml` and is
embedded in the binary; runtime theme files are not required.

Persist the selected theme in `[theme]`. Leave colors unspecified unless you
want an explicit override; all other colors come from the chosen palette:

```toml
[theme]
name = "light"
# Optional overrides:
# cpu = "#14710a"
# selection_bg = "#cfcfde"
# rainbow = false
```

`--theme NAME` temporarily changes the selector. Explicit global/profile colors
still override that palette, including when `--theme` is used. `config show`
displays the fully resolved colors for debugging; it is not the sparse template.
Copying all those colors into `[theme]` explicitly fixes every color, so a later
theme switch will keep them. The default template only sets `name`.

## Profiles override global configuration

There are no built-in theme profiles. Profiles are exclusively user-defined
patches of global settings: limits, checks, views, columns, intervals, themes,
and so on. `--profile work` takes precedence over root `default_profile = "work"`.
For example:

```toml
default_profile = "work"

[theme]
name = "dracula"

[profiles.work]
interval_seconds = 1.0

[profiles.work.limits]
mode = "auto-or-manual"
cpu_cores = 4.0
memory = "8GiB"

[profiles.work.theme]
name = "gruvbox"
rainbow = false
```

`ures --profile work --theme light` keeps work's interval, limits and rainbow
setting, but selects the light palette. A user-defined profile may share a theme's
name; selecting that theme never selects the profile.

| Field | Precedence, highest first |
| --- | --- |
| Theme name | `--theme` → selected profile's `theme.name` → global `theme.name` → `dracula` |
| Individual color / rainbow setting | Selected profile's override → global override → selected built-in palette |
| Other global settings | Matching CLI option → selected profile → global config → built-in defaults |

Maps merge and arrays replace. Every user profile is validated, even when not
selected. Unknown theme names, colors, profile names and misspelled keys fail
with an error. To add a built-in palette, create its TOML under `config/themes`,
register it in `src/theme.rs`, and run the registry and TUI tests.

## Migration

- Replace old `ures --profile light` / `--profile gruvbox` theme shortcuts with
  `--theme light` / `--theme gruvbox`.
- Replace an implicit theme selection such as root `default_profile = "light"`
  with `[theme] name = "light"` (on separate lines). Keep `default_profile` when
  it selects one of your own declared global override profiles.
- Explicitly declared user profiles continue to work, including profiles with
  a `[profiles.NAME.theme]` override. No profile names are reserved for themes.
- Legacy flat `[theme]` color overrides without `name` still apply to Dracula.

## Upstream references

The palettes were compared using their original GitHub repositories rather than
mixing unrelated individual colors:

- [Dracula Classic and Alucard OSS palettes](https://github.com/dracula/dracula-theme#color-palette-oss):
  default and light colors, including foreground/background and selection tokens.
- [Gruvbox palette definitions](https://github.com/morhetz/gruvbox/blob/master/colors/gruvbox.vim#L78):
  warm alternative using the bright metric colors and light text tokens.
- [Catppuccin](https://github.com/catppuccin/catppuccin): reviewed as a pastel alternative;
  the default uses Dracula's stronger accents for this dashboard.

These are adaptations of upstream palettes, not official ports. The regular and
secondary text contrast is checked against each built-in theme's background. TUI tests
also check full-frame/popup coverage, monochrome rendering and continuous selection.
