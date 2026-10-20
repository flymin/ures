<div align="center">

# ures

测试、示例和预览均使用虚构数据。分享运行输出或制作发布包前，请参阅
[开源发布隐私说明](docs/privacy.md)。

### 从用户概览，找到占用资源的进程树。

![Version](https://img.shields.io/badge/version-0.1.0-8b5cf6)
![Rust](https://img.shields.io/badge/Rust-1.99-f97316?logo=rust)
![Linux](https://img.shields.io/badge/Linux-x86__64%20%7C%20aarch64-06b6d4)
![Static](https://img.shields.io/badge/binary-static-14b8a6)
![License](https://img.shields.io/badge/license-MIT-6366f1)

[English](README.md) · **简体中文**

<img src="docs/assets/tui-preview.svg" alt="ures TUI：自定义 Tab 顺序、CPU 与内存语义配色、连续的选中行底色" width="1100">

<sub>示意数据。CPU 单位是核；不同树节点的合计存在包含关系。</sub>

</div>

`ures` 专注回答：**某个用户的哪个进程、进程树或应用占用了资源？** 它把用户配额、CPU 限流和 OOM 证据放在进程列表旁，默认打开交互 TUI；`report` 导出文本或 JSON，加 `-i` 可连续采样，两种入口共用统计模型。

当前持续迭代固定使用 **0.1.0**，日常修改不推进版本号。

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

`ures report --view NAME` 严格按配置中的视图键名选择，不按 `kind` 或 Tab 显示名称查找；
省略 `--view` 时使用 `default_view`，加 `-i` 可持续输出。
JSON 导出使用 `ures report --json`；管道或重定向中使用 `ures report`，加 `-i` 可连续输出；
直接 `ures` 需要交互终端，不满足时会先给出用法提示，不创建配置。
`--width`、`--full` 用于文本输出，`--count` 用于持续输出；TUI 会明确拒绝这些无效参数。
完整用法与迁移规则见[命令接口说明](docs/cli.md)。

各级命令均可用 `-h` 查看简明帮助，用 `--help` 查看该层级全部参数；
TUI 快捷键在界面内按 `?` 查看。

CPU 列以 **核** 为单位，`1.00` 代表一个 CPU。树中的 CPU、TREE MEM 包含后代；SELF MEM 只属于当前进程。**不能把树形表格的各行相加。** 内存优先使用按比例分摊共享页的 PSS；`*` 代表回退到 RSS，`~` 表示统计不完整，`?` 表示不可读。

概览优先使用可识别的用户 cgroup。cgroup 内存包含文件缓存，和进程 PSS 合计不同。工具支持 cgroup v1/v2、systemd 用户 slice、登录节点用户组、容器挂载根、祖先限制和 cpuset；不会把共享主机或容器的上界当作个人分配额度。特殊目录结构可通过配置指定控制器路径。

## 构建与安装

发布包提供 Linux x86_64/aarch64 musl 静态二进制。核对 SHA256 后，将 `ures` 安装到 PATH 中即可。普通采集不依赖 Python、Rust runtime 或 root 权限；cgroup 和内核日志按实际可读范围使用。静态 libc 无法解析目录服务账号时，工具通过系统 `id` 解析用户名；没有该命令时仍支持数字 UID 和当前用户查询。

使用已有 Rust 1.99.0、rustfmt、Clippy 和对应 musl target：

```bash
cd ~/local/src/ures
./build.sh

# 工具链位于已有环境时，可指定其路径（请替换为本机路径）：
URES_VENV="/path/to/existing-toolchain" ./build.sh

# 同时支持普通 Cargo 开发流程：
cargo test --locked
cargo run -- --help
```

`build.sh` 检查格式、测试、Clippy、静态链接和 CLI/伪终端行为，通过后原子安装到 `~/local/bin/ures`。可设置 `URES_INSTALL_DIR`、`URES_TARGET`。默认使用一个构建任务；匿名化日志保存在 `.logs/build.log`，二进制内重映射本地编译路径。脚本清理自己创建的临时构建目录；明确指定的 `CARGO_TARGET_DIR` 会保留。构建不会安装工具链或修改环境配置。

## 配置自己的视图

默认配置位置：`${XDG_CONFIG_HOME}/ures/config.toml`；XDG 未设置、为空或不是绝对路径时使用 `~/.config/ures/config.toml`。首次正常运行生成完整注释模板，已有文件不会覆盖。帮助、版本和 `config` 子命令不创建文件，也不执行检查脚本。

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

默认启用 CPU、内存、OOM 检查；I/O、线程和 FD 在视图需要或检查启用时采集。CPU/内存阈值采用参考上限百分比，其他检查使用原始计量单位。OOM 区分本轮新增和历史累计，不会因曾经发生过 OOM 一直显示严重状态。

只有启用 OOM 检查，或 TUI 中存在 OOM 视图、报告选中 OOM 视图时，才读取内核日志。

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

列宽不会随刷新、排序和筛选变化：PID 预留 7 位，读写速率预留 11 列（含不完整统计标记），命令列使用剩余宽度。超长内容显示省略号，`--ascii` 使用 `~`。调整窗口大小时，界面会隐藏可选列或压缩布局。

默认配色基于 [Dracula Classic](https://github.com/dracula/dracula-theme#color-palette-oss)，提高了辅助文字亮度，并明确设置前景、背景。CPU 使用亮绿，内存使用亮黄，读取为青色、写入为橙色；顶部压力值跟随告警阈值变色。默认开启 PID/命令的 rainbow 颜色，绑定进程身份或应用组名，指标颜色始终表达相同含义。树形连线独立着色，上下文祖先行弱化显示；选中行采用统一前景、底色，覆盖列、列间空格和行尾。支持 `NO_COLOR` 和 `--color never`。可以在已有配置中加入或修改：

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

profile 仅代表用户定义的全局配置覆盖，可同时修改 limits、views、checks、采样间隔和 theme，通过 `--profile work` 或顶层 `default_profile = "work"` 选择。内置主题不再自动创建同名 profile；用户自己的 `[profiles.work.theme]` 仍可设置 `name = "gruvbox"` 等覆盖。`ures --profile work --theme light` 会保留 work 的其他设置并临时选择浅色主题。详见[主题、profile 优先级与迁移说明](docs/themes.md)。

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

采样和外部检查在输入循环之外运行；暂停显示会保留旧快照并显示数据年龄。进程身份包含启动时间，避免 PID 重用继承旧进程的选择状态或 CPU 基线。界面只读，正常退出、Ctrl-C、终止和 panic 都会恢复终端。

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

## 统计边界与开发

- CPU 采用采样增量；短命进程可能在两次采样间消失，cgroup 与可见进程合计可能不同。
- PSS、RSS、cgroup 内存和祖先共享上界具有不同范围，界面明确标注，不直接混加。
- I/O 汇总使用可见线程增量，避免引入已回收子进程的历史累计；退出任务可能留下未观测尾部，因此不代表完整磁盘/NFS 吞吐。FD 合计是描述符数，不是唯一文件数。
- OOM 使用当前启动周期内仍保留且可访问的日志，可能截断或不可读；累计计数可能大于保留事件数。日志 PID 是历史内核 PID，可能来自另一命名空间。
- 无权限数据保持未知，工具不会调用 sudo。

开发入口：[贡献指南](CONTRIBUTING.md)、[架构](docs/architecture.md)、[变更记录](CHANGELOG.md)。测试使用确定性进程/cgroup 数据、CLI 和伪终端，不触发真实 OOM 或修改配额。`cargo run --example preview` 可重现文档中的虚构场景。

CI 为两个 Linux 架构构建静态包，生成 SHA256 和依赖许可证清单；与包版本匹配的 `v<version>` tag 可触发已准备的发布流程。[MIT 许可证](LICENSE)。
