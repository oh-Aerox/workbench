# Agent Workbench

本地 agent 工作进度可视化工作台。只读聚合 Claude Code、Codex、WorkBuddy 三家 agent 在本机留下的会话记录，按 agent → 项目 → 会话三级展示工作进度。

需求原文见 [require.md](require.md)，未完成事项和已知限制见 [PLAN.md](PLAN.md)。

## 只读保证

本应用**绝不写入任何 agent 工作区**。三层保证：

1. 读 agent 数据一律经 `paths::open_readonly`，文件句柄本身不带写权限
2. 任何写操作先过 `paths::assert_app_owned`，目标不在应用数据目录内直接 panic（见 `paths.rs` 单测）
3. `tauri.conf.json` 不开放 fs / shell 权限，前端拿不到任何写能力

解析缓存只落在应用自己的目录：

- Windows `%LOCALAPPDATA%\AgentWorkbench\parse-cache.json`
- macOS `~/Library/Application Support/AgentWorkbench/parse-cache.json`

文件监听（`watcher.rs`）只订阅文件系统事件，不读也不写被监听的文件；真正的重新解析发生在前端收到通知后主动重新调命令时，届时仍走 `open_readonly`。

## 增量缓存

全量解析本机数据约 860 ms，缓存后 64 ms（**13.5 倍**）。缓存以 `(绝对路径, 文件大小, mtime)` 为键，三者全都没变才复用——agent 写会话文件必然改变大小和 mtime，不会漏更新。

**没有用 SQLite**：rusqlite(bundled) 会给二进制加约 1.5–2 MB，而数据规模只是几十场会话、秒级解析。为需求 5（体积尽可能小）这笔不划算，JSON 缓存同样是增量且零新依赖。全局搜索同理走内存扫描，没引全文索引。

⚠️ 改动任何 adapter 的统计口径后，必须给 `index.rs` 的 `VERSION` +1，否则旧缓存会以新字段含义被读出来。版本对不上时缓存整个丢弃重建。

## 数据来源

| Agent | 路径 | 说明 |
|---|---|---|
| Claude Code | `~/.claude/projects/<cwd编码>/<sessionId>.jsonl` | 含 `ai-title`（现成标题）、`cost-state`（花费/增删行数）、`file-history-*`（触达文件） |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`、`~/.codex/archived_sessions/`、`~/.codex/session_index.jsonl` | 按日期分目录，项目归属需从 `session_meta.cwd` 反查 |
| WorkBuddy | `~/.workbuddy/projects/<cwd编码>/<sessionId>.jsonl`、`~/.workbuddy/sessions/<pid>.json` | 与 Claude 同形但 schema 不同；`sessions/*.json` 有心跳，可直接判定进行中 |

三家在 Windows 和 macOS 上都落在用户主目录下的同名隐藏目录，路径解析无需按平台分支。

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

只从 agent 自己的记录里读，**不扫描项目源码**——那会把应用的读取范围从三个 agent 数据目录扩大到用户全部项目的源码树。

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
```

出包：

```bash
npm run tauri build
```

产物：Windows `src-tauri/target/release/bundle/nsis/`，macOS `src-tauri/target/release/bundle/dmg/`。

### 构建环境

编译期需要 Rust；**运行期不需要**，安装包里是编译好的原生二进制。

- Windows：Rust（MSVC 工具链）+ VS 2022 Build Tools（C++ 工作负载）
- macOS (Apple Silicon)：Rust + Xcode Command Line Tools

两个平台需各自本地出包，无法交叉编译。

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
  lib/Markdown.svelte      极简 markdown 渲染（不引库、不用 @html）
  lib/SessionList.svelte   按时间倒序的会话卡片
  lib/SearchResults.svelte 搜索结果 + 关键词高亮
src-tauri/
  src/model.rs             跨 agent 统一数据模型
  src/paths.rs             路径解析 + 只读守卫（含单测）
  src/index.rs             增量解析缓存（唯一写磁盘处）
  src/watcher.rs           agent 目录监听，去抖后通知前端
  src/commands.rs          暴露给前端的只读命令
  src/adapters/            每家 agent 一个实现 + 聚合逻辑
  examples/scan.rs         命令行冒烟检查，不起窗口
```

三条约定：

- **按需解析**：`list_projects` 只做目录枚举和 stat，保证首屏秒开；`parse_project` 才逐行解析该项目的 JSONL
- **adapter 只实现一个方法**：各家只需实现 `parse_project`，`list_sessions`（时间序）、`project_outcome`（成果聚合）、`daily_activity`（热力图）都由 trait 默认方法从它派生
- **跨天活动必须逐事件归档**：会话可能跨多天（本机最长一场跨 4 天），热力图不能用起止时间推算，要在解析时按事件时间戳逐条归到本地日期
