# Agent Workbench

本地 agent 工作进度可视化工作台。只读聚合 Claude Code、Codex、WorkBuddy 三家 agent 在本机留下的会话记录，按 agent → 项目 → 会话三级展示工作进度。

未完成事项和已知限制见 [PLAN.md](PLAN.md)。

## 只读保证

本应用**绝不写入任何 agent 工作区**。三层保证：

1. 读 agent 数据一律经 `paths::open_readonly`，文件句柄本身不带写权限
2. 任何写操作只能走 `paths::write_app_file` / `rename_app_file` / `remove_app_file`，它们内部先过 `assert_app_owned`，目标不在应用数据目录内直接 panic
3. `Cargo.toml` 根本没引入 `tauri-plugin-fs` / `tauri-plugin-shell`——不是「没开权限」，是**写文件的代码压根没被编译进二进制**，前端无论如何拿不到写能力

第 2 层原先只是「记得调用断言」的约定：`std::fs::write` 在 crate 里任何地方都能直接调，而守卫单测只验「守卫函数判得对不对」，验不了「是否所有写操作都过了守卫」。现在写操作收成 `paths.rs` 的唯一出口，并由 `写操作只能出现在_paths_rs` 这条单测扫源码做静态兜底：任何绕过封装直接写盘的新代码都会让 `cargo test` 变红。

读侧有一条对称的守卫 `paths::resolve_under`：凡是拿**会话文件内容里读出来的名字**去拼路径（如 `~/.claude/plans/<slug>.md` 的 slug），必须先过它——否则 slug 为 `../../../Desktop/机密` 就能把任意文件读出来渲染进计划面板。

### 缓存里有什么、在哪、怎么清

解析缓存只落在应用自己的目录：

- Windows `%LOCALAPPDATA%\AgentWorkbench\parse-cache.json`
- macOS `~/Library/Application Support/AgentWorkbench/parse-cache.json`

⚠️ 这个文件里存的是**明文**的解析结果，含每场会话的首条 prompt（已剥掉注入的上下文块）和触达过的文件路径。只读承诺管的是「不写 agent 目录」，不等于可以把 agent 工作区里的内容无限期复制到用户不知情的新位置，所以：

- 启动时剔除源文件已不存在的条目
- 条目数有硬上限（会话 4000 / 源码文件 40000），超了按 mtime 淘汰最旧的
- 左侧栏底部显示缓存占用，一键清空（清完只是下次打开项目慢一点，不丢任何数据）

文件监听（`watcher.rs`）只订阅文件系统事件，不读也不写被监听的文件；真正的重新解析发生在前端收到通知后主动重新调命令时，届时仍走 `open_readonly`。

⚠️ **读取范围有一个例外**：源码 TODO 扫描（`repo.rs`）会读项目工作目录下的文件。仍然只读、不违反需求 2，但它超出了三个 agent 数据目录，所以受白名单和手动触发两道约束——详见下方「项目源码里的 TODO 标记」。

### 威胁模型：会话文件的内容被视为可信输入

TODO 扫描的白名单校验「目标必须出现在某个 agent 的项目列表里」，而项目列表里的路径来自各 adapter 的 `peek_cwd`，也就是**被解析的会话文件自身的内容**。同理，计划文件的 slug 也来自会话文件。

这道白名单挡的是「前端传来的任意路径」，**不是「被篡改的会话文件」**：能写 `~/.claude/projects/**/*.jsonl` 的人，本来就能在你的机器上做更多事。`resolve_under` 之类的守卫是纵深防御（会话文件会被复制、同步、从他处拷入，不该毫无校验地当路径用），不是把这条信任边界移走。

## 增量缓存

全量解析本机数据约 860 ms，缓存后 64 ms（**13.5 倍**）。缓存以 `(绝对路径, 文件大小, mtime)` 为键，三者全都没变才复用——agent 写会话文件必然改变大小和 mtime，不会漏更新。

**没有用 SQLite**：rusqlite(bundled) 会给二进制加约 1.5–2 MB，而数据规模只是几十场会话、秒级解析。为需求 5（体积尽可能小）这笔不划算，JSON 缓存同样是增量且零新依赖。全局搜索同理走内存扫描，没引全文索引。

⚠️ 改动任何 adapter 的统计口径后，必须给 `index.rs` 的 `VERSION` +1，否则旧缓存会以新字段含义被读出来。版本对不上时缓存整个丢弃重建。

⚠️ **只有「文件内容的函数」才能进缓存**。「进行中」标志（`running`）是 `now - mtime` 的函数，而缓存的失效条件恰恰是 mtime 变化——会话结束后文件不再变化，缓存永远命中，解析当时算出的 `true` 就被永久固化，还随缓存落盘、重启也不恢复。它现在在 `adapters::cached_collect` 里按当前 mtime 现算，回归测试见 `进行中标志不吃缓存`。以后再加这类时间派生字段，一律照此处理。

⚠️ 缓存的清理（`prune_missing`，对每个条目做一次 `exists()`）**只在加载时跑一次**。`flush` 是每解析完一个项目就调一次的，把清理放进去会变成 O(项目数 × 条目数) 次 stat：实测把一次全局搜索从 131 ms 拖到 21 s。

## 数据来源

| Agent | 路径 | 说明 |
|---|---|---|
| Claude Code | `~/.claude/projects/<cwd编码>/<sessionId>.jsonl` | 含 `ai-title`（现成标题）、`cost-state`（花费/增删行数）、`file-history-*`（触达文件） |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`、`~/.codex/archived_sessions/`、`~/.codex/session_index.jsonl` | 按日期分目录，项目归属需从 `session_meta.cwd` 反查 |
| WorkBuddy | `~/.workbuddy/projects/<cwd编码>/<sessionId>.jsonl`、`~/.workbuddy/sessions/<pid>.json` | 与 Claude 同形但 schema 不同；`sessions/*.json` 有心跳，可直接判定进行中 |

三家在 Windows 和 macOS 上都落在用户主目录下的同名隐藏目录，**根目录**解析无需按平台分支。

但 **cwd 编码成目录名的方式，三家在 macOS 上并不一致**（实机确认）：

| Agent | macOS 上 `/Users/a/x` 编码成 |
|---|---|
| Claude Code | `-Users-a-x`　保留了开头的 `/` |
| WorkBuddy | `Users-a-x`　　开头的 `/` 被直接吃掉 |
| Codex | 不编码，按日期分目录 |

所以 `decode_project_dir` 不能只认前导 `-`：少了它的名字在 Windows 分支里会被当成相对路径解成 `Users\a\x`。兜底解码现在按 `#[cfg(unix)]` 分平台还原——同一台机器上不会混两个平台产出的目录名，这个假设是安全的。

不过这条兜底路径平时走不到：三家 adapter 都优先 `peek_cwd()` 从会话文件内容里读真实 cwd，只有文件损坏或为空时才落到解码。本机 25 个项目全部走的 peek。

### 各家能提供的字段不一样

卡片上显示 `—` 不是解析失败，是那家 agent 本来就没记这个数据：

| 字段 | Claude | Codex | WorkBuddy |
|---|:--:|:--:|:--:|
| 会话标题 | ✅ `ai-title` | ✅ `thread_name` | ⚠️ 取首条 prompt |
| 轮数 / 工具调用 | ✅ | ✅ | ✅ |
| 文件改动排行 | ✅ | ❌ 不记录 | ✅ 仅版本号 |
| 增删行数 | ✅ | ❌ | ❌ |
| 花费 | ✅ | ❌ | ❌ |
| git 分支 | ✅ | ❌ | ❌ |
| 模型 | ✅ | ✅ | ✅ |
| 计划 / 待办 | ✅ | ⚠️ 未验证 | ❌ 无此工具 |

## 计划 / 待办从哪来

「计划」面板合并三类来源，**按快慢分成自动加载与手动触发两档**：

| 来源 | 加载方式 | 耗时 |
|---|---|---|
| 项目里的计划文档（`PLAN.md` 等） | 切项目自动 | 毫秒级，只走两层目录 |
| agent 产出的计划（`ExitPlanMode` 等） | 切项目自动 | 走会话解析缓存 |
| 代码注释里的 TODO 标记 | 按钮触发 | 大项目首次几十秒 |

分档是必须的：本机最大的项目扫一遍源码树要 85 秒，若和计划文档绑在一起，看一眼 `PLAN.md` 就得等一分半钟。

### 一、项目里的计划文档（自动加载）

根目录及下一层里名为 `PLAN` / `TODO` / `ROADMAP` / `BACKLOG` / `TASKS` / `MILESTONES` 的 `.md`、`.txt`，以及名字含「待办」「计划」「路线」的文档。整份读进来渲染，并抽出 `- [ ]` 勾选项算完成度。

`README.md` 不算——它是说明不是计划。

### 二、agent 自己产出的计划（自动加载）

| 来源 | 说明 |
|---|---|
| `ExitPlanMode` 的 `input.plan` | 计划模式产出的 markdown 全文，质量最高 |
| `~/.claude/plans/<slug>.md` | 独立计划文件，会话记录里的 `slug` 字段负责关联 |
| `TodoWrite` 的 `input.todos` | `[{content, status}]` 结构化清单 |
| Codex `update_plan` | ⚠️ **本机数据里没出现过这个工具，字段形状按常见约定写、未经真实数据验证**，解析刻意做得宽容，对不上就安静跳过 |
| WorkBuddy | 工具集里没有计划/待办类工具（实测只有 Bash/Glob/Write/Read/PowerShell/Grep 等），恒为空 |

两个要点：

- **只认真正的复选框** `- [ ]` / `- [x]`。计划正文里的普通 `-` 列表大多是选型说明和约束条件，当成待办会满屏噪声
- **按正文去重**。`ExitPlanMode` 和 `plans/<slug>.md` 通常是同一份内容（前者就是把后者提交上去的），不能按 slug 比对 `source`——`ExitPlanMode` 的 source 里没有 slug

### 三、代码注释里的 TODO 标记（手动触发）

遍历整棵源码树找 `TODO` / `FIXME` / `XXX` / `HACK`（见 `repo.rs` 的 `scan_project`）。

**这是本应用唯一读取 agent 数据目录之外文件的功能**，仍然只读，但读取范围扩大到了项目源码树，所以有三道约束：

1. **白名单**：`project_todos` 命令只接受出现在某个 agent 项目列表里的路径。前端传来的路径不可信——没有这道校验，任何能调到该命令的地方都能让应用遍历机器上任意目录
2. **手动触发**：本机 eshop 项目 7162 个文件首次扫描耗时 **85 秒**（绝大部分花在逐文件读取上，Windows Defender 实时扫描会显著放大）。切项目就自动扫会像卡死，所以做成按钮
3. **跑在阻塞线程池**：命令是 `async` + `spawn_blocking`，不占 UI 线程

性能闸门：忽略 `node_modules`/`target`/`dist` 等依赖与构建产物目录及所有隐藏目录、跳过 >512 KB 和含 NUL 字节的文件、遍历 6 万文件或命中 3000 条即截断。配合按 (大小, mtime) 的逐文件缓存，重扫只读变化过的文件（eshop 85s → 1.2s）。

代价是缓存文件会变大：**每个扫过的文件都要存一条记录，包括没有 TODO 的**。本机扫完全部项目后 `parse-cache.json` 从 46 KB 涨到 1.5 MB。这笔开销省不掉——不存空结果，下次重扫就得重读全部文件，等于回到 85 秒。

匹配规则上的两个决定：

- **要求标记是独立单词**，前后不能紧邻字母数字。否则 `TODOS_TABLE`、`XXXX` 会误判——本机 eshop 上这条规则滤掉了 56 处误报（352 → 296）
- **大小写敏感**，只认全大写。小写 `todo` 在正常英文散文里太常见，不敏感会被噪声淹没

⚠️ 噪声仍然存在且无法根治：eshop 的 296 处里有 102 处来自 `inc/class/phpQuery/phpQuery.php` 这个内嵌的第三方库。它不在 `vendor/` 之类的常规目录名下，靠目录名规则识别不出来。

Codex 没有文件历史追踪，成果盘点对它的文件清单永远是空的；WorkBuddy 只有全量快照没有增量记录，改动次数只能取快照里的 `version`。

解析上的坑，都已用单测钉住：

- **目录名编码有损**：`project-my-notes` 解码后会变成 `project\my\notes`，所以真实 cwd 一律从会话文件内容里 peek，解码只作兜底
- **WorkBuddy 标题**：真人输入被包在 8KB 注入上下文末尾的 `<user_query>` 里，必须优先识别该标签，否则标题会变成 `OS Version: win32 Shell: bash…`
- **盘符大小写**：Claude 记 `C:\`，WorkBuddy 记 `c:\`，不统一会让同一目录看起来像两个
- **文件路径混用相对与绝对**：`trackedFileBackups` 的键对项目内文件是**相对路径**，只有项目外的文件（`~/.claude/plans`、临时目录）才是绝对路径。不先判断就比对前缀，会把所有文件都误判成「项目外」
- **改动次数不能累加**：`file-history-snapshot` 里的 `trackedFileBackups` 是全量映射、每次快照重复出现，累加会把次数放大数倍。取 `version` 与 `file-history-delta` 计数的较大者

## 按 agent 划分

同一个工作目录可能被多个 agent 跑过。工作台**按 agent 划分，不做跨 agent 合并**——同一项目在不同 agent 下各出现一次是预期行为，不是 bug。

## 开发

```bash
npm install
npm run tauri dev
```

改完 adapter 先跑冒烟检查，不起窗口直接看解析结果：

```bash
cd src-tauri
cargo test                           # 只读守卫 + 解析启发式的单测
cargo run --example scan             # 列出本机所有 agent 和项目
cargo run --example scan -- full     # 连每场会话的统计一起列
cargo run --example scan -- outcome  # 列每个项目的成果盘点
cargo run --example scan -- heat     # 按天活动量 + 缓存冷热耗时对比
cargo run --example scan -- find xxx # 全局搜索
cargo run --example scan -- plans    # 列提取到的计划 / 待办
cargo run --example scan -- todos    # 扫项目源码的 TODO 标记（含冷热耗时）
```

出包：

```bash
npm run tauri build            # Windows
CI=true npm run tauri build    # macOS，CI=true 不能省，原因见下
```

产物：Windows `src-tauri/target/release/bundle/nsis/`，macOS `src-tauri/target/release/bundle/dmg/`。

⚠️ **macOS 上 `CI=true` 是必需的**，否则打 DMG 必挂在最后一步：

```
execution error: “Finder”遇到一个错误：AppleEvent已超时。 (-1712)
failed to bundle project: error running bundle_dmg.sh
```

`.app` 其实已经编译好了，挂掉的是 `create-dmg` 用 AppleScript 驱动 Finder 摆图标位置的**美化**步骤——终端进程没有「自动化控制 Finder」权限时它会一直等到超时。Tauri 在检测到 `CI` 环境变量时会给 `bundle_dmg.sh` 传 `--skip-jenkins` 跳过这段。跳过后 DMG 内容完全一样（app + `/Applications` 软链 + 卷图标），只是打开时图标按 Finder 默认排布。

要拿到摆好位的 DMG，得去「系统设置 → 隐私与安全性 → 自动化」给终端勾上 Finder，然后不带 `CI` 重跑。对内自用不值得折腾。

### 构建环境

编译期需要 Rust；**运行期不需要**，安装包里是编译好的原生二进制。

- Windows：Rust（MSVC 工具链）+ VS 2022 Build Tools（C++ 工作负载）
- macOS (Apple Silicon)：Rust + Xcode Command Line Tools（`xcode-select --install`，不需要完整 Xcode）

两个平台需各自本地出包，无法交叉编译。

两边实测产物体积：

| 平台 | 安装包 | 安装后 |
|---|--:|--:|
| Windows (NSIS) | 1.04 MB | — |
| macOS (DMG, arm64) | 1.5 MB | 3.5 MB（`.app`） |

macOS 产物是 arm64 单架构（`lipo -archs` 确认），不是 universal——需求只要 M 芯片，打 universal 会让体积翻倍，与需求 5 冲突。`LSMinimumSystemVersion` 设为 `11.0`：Tauri 默认值 10.13 是 Intel 时代的，arm64 本身就要 macOS 11 起步。

### 运行期依赖

- Windows：WebView2 Runtime。Win11 预装；Win10 随 Edge 推送，绝大多数机器已有。安装包配置为 `downloadBootstrapper`，缺失时自动下载，不增加包体积
- macOS：WKWebView 为系统组件，无需处理

### 签名

当前未签名。Windows 首次运行会弹 SmartScreen（点「仍要运行」）；macOS 会被 Gatekeeper 拦截（右键→打开，或 `xattr -dr com.apple.quarantine`）。对外分发需自行配置代码签名证书与公证。

## 图标

```bash
python scripts/make-icon.py
npx tauri icon src-tauri/icons/source.png
```

## 结构

```
src/                       Svelte 5 前端，无 UI 库、无图表库（热力图和条形图手写）
  App.svelte               三栏骨架 + agent 侧栏 + 搜索框
  lib/ProjectList.svelte   项目列表
  lib/DetailPane.svelte    右栏路由：搜索结果 / 项目详情 / agent 概览
  lib/AgentOverview.svelte agent 概览：热力图 + 活跃统计
  lib/Heatmap.svelte       近 26 周日历热力图
  lib/OutcomeView.svelte   成果盘点：文件改动排行 + 会话贡献排行
  lib/PlansView.svelte     计划 / 待办清单
  lib/RepoTodos.svelte     代码 TODO 标记分区（手动触发扫描）
  lib/Markdown.svelte      极简 markdown 渲染（不引库、不用 @html）
  lib/SessionList.svelte   按时间倒序的会话卡片
  lib/SearchResults.svelte 搜索结果 + 关键词高亮
src-tauri/
  src/model.rs             跨 agent 统一数据模型
  src/paths.rs             路径解析 + 只读守卫（含单测）
  src/index.rs             增量解析缓存（唯一写磁盘处）
  src/watcher.rs           agent 目录监听，去抖后通知前端
  src/repo.rs              计划文档发现 + 源码 TODO 扫描（唯一读 agent 目录之外文件处）
  src/commands.rs          暴露给前端的只读命令
  src/adapters/            每家 agent 一个实现 + 聚合逻辑
  examples/scan.rs         命令行冒烟检查，不起窗口
```

三条约定：

- **按需解析**：`list_projects` 只做目录枚举和 stat，保证首屏秒开；`parse_project` 才逐行解析该项目的 JSONL
- **adapter 只实现一个方法**：各家只需实现 `parse_project`，`list_sessions`（时间序）、`project_outcome`（成果聚合）、`daily_activity`（热力图）都由 trait 默认方法从它派生
- **跨天活动必须逐事件归档**：会话可能跨多天（本机最长一场跨 4 天），热力图不能用起止时间推算，要在解析时按事件时间戳逐条归到本地日期
