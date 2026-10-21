<div align="center">

# ures

### 从用户概览，找到占用资源的进程树。

![Version](https://img.shields.io/badge/version-0.1.0-8b5cf6)
![Rust](https://img.shields.io/badge/Rust-1.99-f97316?logo=rust)
![Linux](https://img.shields.io/badge/Linux-x86__64%20%7C%20aarch64-06b6d4)
![Static](https://img.shields.io/badge/binary-static-14b8a6)
![License](https://img.shields.io/badge/license-MIT-6366f1)

[English](README.md) · **简体中文**

<img src="docs/assets/tui-preview.svg" alt="ures 交互式资源面板" width="1100">

<sub>使用示例数据的交互界面。</sub>

</div>

`ures` 用于查看 **Linux 用户的资源被哪些进程、进程树或应用占用**。交互面板汇集 CPU、内存、配额、I/O 和 OOM 记录；`report` 可导出文本或 JSON，加 `-i` 可连续采样。

## 常用命令

```bash
ures                           # 打开当前用户的交互界面
ures -u alice --sort cpu        # 指定用户，交互界面按 CPU 排序
ures --theme light -i2          # 浅色交互界面，每两秒刷新
ures report                    # 一次性输出概览和默认视图
ures --view io -i2             # 在交互界面查看 I/O
ures report --view ps --sort mem -n10         # 单进程内存排行
ures report --view tree --pid 12345 --full    # 聚焦子树，保留完整命令行
ures report --view groups       # 按应用汇总
ures report --view io               # I/O、线程、FD 详情
ures report --view oom --since 24h            # 最近 24 小时内仍保留的 OOM 记录
ures report --view checks       # 内置与外部检查结果
ures config themes             # 查看内置主题
ures report --json             # JSON 快照
ures report -i2 --json --count 3 # 三条 NDJSON 快照
ures -h                        # 常用入口与参数
ures report --help             # 按用途分组的完整报告参数
```

`--view NAME` 选择配置中的视图，省略时使用 `default_view`。
交互终端使用 `ures`，管道和脚本使用 `ures report`。
完整参数见[命令接口说明](docs/cli.md)。

各级命令均可用 `-h` 查看简明帮助，用 `--help` 查看该层级全部参数；
TUI 快捷键在界面内按 `?` 查看。

CPU 以核为单位，内存优先使用 PSS。各指标的单位、覆盖标记和统计范围见
[指标与快照参考](docs/snapshots.md)。

## 构建与安装

[发布页面](https://github.com/flymin/ures/releases)提供 Linux x86_64/aarch64 musl 静态二进制。
核对 SHA256 后，将 `ures` 放到 PATH 中即可。运行需要 Linux `/proc`，不依赖 Python 或 Rust runtime；
cgroup 和内核日志按实际可读范围使用。

使用已有 Rust 1.99.0、rustfmt、Clippy 和对应 musl target：

```bash
git clone https://github.com/flymin/ures.git
cd ures
./build.sh

# 同时支持普通 Cargo 开发流程：
cargo test --locked
cargo run -- --help
```

`build.sh` 完成检查并将静态二进制写入 `target/<target>/release/ures`。
需要安装时，通过 `URES_INSTALL_DIR` 指定目标目录。构建和测试详情见[贡献指南](CONTRIBUTING.md)。

## 配置自己的视图

首次运行会创建带注释的配置文件，位置为 `${XDG_CONFIG_HOME}/ures/config.toml`，
默认使用 `~/.config/ures/config.toml`。可用 `ures config path` 查看路径。

```bash
ures config path
ures config show               # 合并所选 profile 后的配置
ures config check              # 校验所有 profile
ures --view compact
ures --profile lab
ures --config ./team.toml
```

用户文件只需写出修改部分。映射逐属性合并，数组整体替换；运行参数优先于所选 profile、全局配置和内置默认值。

```toml
[limits]
mode = "auto"                  # auto | manual | auto-or-manual

[profiles.lab.limits]
mode = "manual"
cpu_cores = 4.0
memory = "8GiB"

[views.tree]
columns = ["pid", "cpu", "mem", "self_mem", "threads", "command"]
sort = "mem"
top = 15

[[groups]]
name = "Workers"
match = 'sample_worker\.py|sample-watcher'
```

手动数值明确显示为 **configured budget（配置预算）**，不修改内核限制；探测到的内核上界仍独立保留。`auto-or-manual` 只在对应用户限制无法探测时使用预算；`auto` 不会悄悄使用配置中的数字。

视图支持 `tree`、`ps`、`groups`、`oom`、`checks`，可选显示名称 `name`，以及列、排序、行数、`include`/`exclude` 正则。显式配置 `[views]` 后，只显示其中列出的视图；内置视图键名仍会补齐省略的字段默认值，profile 可以覆盖或增加视图。树形筛选保留祖先上下文，祖先合计仍包含隐藏节点。应用规则按配置顺序首次匹配，否则按可执行文件名归组。列可修改标题、宽度和隐藏优先级；主题支持颜色名称、RGB 与索引色。详见[视图说明](docs/views.md)、[默认模板](config/default.toml)和[配置示例](examples/profiles.toml)。

## 交互操作

`ures` 为解析后的每个 `views` 条目生成一个 Tab。顶层 `tab_order` 数组决定顺序，未列出的视图按键名追加；默认顺序为 Tree、Processes、I/O、Groups、Compact、OOM、Checks。设置 `tab_order = []` 可使用字母顺序。有 `name` 就显示它，否则显示视图键名。`default_view` 决定起始视图，`--view` 可覆盖。每个视图采用各自的 `top`（`0` 表示全部），`--top` 可统一覆盖。切换时分别保留排序、搜索、折叠、选择和滚动位置；Tab 较多时会滚动到当前标签。

例如，以下配置只生成三个 Tab：

```toml
default_view = "workers"
tab_order = ["workers", "events", "health"]
[views.workers]
name = "Workers"
kind = "tree"
columns = ["pid", "cpu", "mem", "command"]
top = 0
[views.events]
name = "OOM history"
kind = "oom"
[views.health]
kind = "checks"
```

列宽和隐藏优先级可配置，命令列使用剩余空间；超长内容会按终端宽度截断。

内置 Dracula、Alucard 浅色和 Gruvbox 三套主题。CPU、内存和 I/O 使用不同颜色，
可选 rainbow 配色用于区分进程。`NO_COLOR` 和 `--color never` 可关闭颜色。例如：

```toml
[columns.read]
width = 12
[columns.write]
width = 12
[theme]
name = "dracula"
rainbow = true
# 仅在需要时覆盖某个颜色：
# cpu = "#50fa7b"
# selection_bg = "#44475a"
```

使用 `ures --theme light` 临时切换 Alucard Classic 米白底，或 `ures --theme gruvbox` 切换暖色 Gruvbox；持久选择写在 `[theme]` 的 `name` 字段。`--theme` 优先选择主题名称，显式配置的颜色覆盖仍然保留。`foreground = "reset"`、`background = "reset"` 可跟随终端原色。

profile 是用户定义的全局配置覆盖，可同时修改 limits、views、checks、采样间隔和 theme，
通过 `--profile work` 或顶层 `default_profile = "work"` 选择。
`[profiles.work.theme]` 可设置 `name = "gruvbox"` 等覆盖；
`ures --profile work --theme light` 保留 work 的其他设置并临时选择浅色主题。
详见[主题与 profile 参考](docs/themes.md)。

| 按键 | 功能 |
| --- | --- |
| Tab / Shift-Tab | 切换页面 |
| ↑↓ / j k | 移动与滚动 |
| PageUp / PageDown | 每次跳转十行 |
| Home / End | 第一行 / 最后一行 |
| 空格 / ← / → | 展开或折叠子树 |
| Enter | 进程详情及完整命令行 |
| c / m / i | 按 CPU / 内存 / 逻辑 I/O 排序 |
| / | 搜索当前视图内容；Enter 应用、Esc 清除 |
| p / r | 暂停显示 / 请求刷新 |
| R（Shift-r） | 切换本次会话的 rainbow 行色 |
| ? / q / Esc | 帮助 / 退出；弹窗内任意键关闭 |

## 扩展检查

外部命令返回带版本的 JSON：状态、说明和数值指标。扩展不要求特定语言或动态库 ABI。

```toml
[checks.zombies]
enabled = true
command = ["python3", "/path/to/ures/examples/check-zombies.py"]
interval_seconds = 10.0
timeout_seconds = 2.0
```

命令采用参数数组，不隐式经过 shell；通过 `URES_TARGET_USER`、`URES_TARGET_UID`、`URES_CONTEXT` 获得查询上下文。最多两个检查同时运行，每个有独立期限和 64 KiB 输出限制；失败、过期或不合法结果仅影响相应检查。只有配置中启用的扩展会执行。

详见[扩展协议](docs/extensions.md)、[JSON Schema](docs/check.schema.json)和[快照格式](docs/snapshots.md)。

## 参与开发与发布

[贡献指南](CONTRIBUTING.md)介绍构建和测试，[架构说明](docs/architecture.md)介绍模块划分。

Tagged release 提供 Linux x86_64/aarch64 静态包、SHA256 和依赖许可证清单。
[变更记录](CHANGELOG.md) · [MIT 许可证](LICENSE)。
