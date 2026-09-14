# Agent Workbench

本地 agent 工作进度可视化工作台。只读聚合 Claude Code、Codex、WorkBuddy 三家 agent 在本机留下的会话记录，按 agent → 项目 → 会话三级展示工作进度。

## 只读保证

本应用**绝不写入任何 agent 工作区**。三层保证：

1. 读 agent 数据一律经 `paths::open_readonly`，文件句柄本身不带写权限
2. 任何写操作先过 `paths::assert_app_owned`，目标不在应用数据目录内直接 panic（见 `paths.rs` 单测）
3. `tauri.conf.json` 不开放 fs / shell 权限，前端拿不到任何写能力

索引缓存只落在应用自己的目录：

- Windows `%LOCALAPPDATA%\AgentWorkbench\`
- macOS `~/Library/Application Support/AgentWorkbench/`

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
| 触达文件数 | ✅ | ❌ 不记录 | ✅ |
| 增删行数 | ✅ | ❌ | ❌ |
| 花费 | ✅ | ❌ | ❌ |
| git 分支 | ✅ | ❌ | ❌ |
| 模型 | ✅ | ✅ | ✅ |

三处解析上的坑，都已用单测钉住：

- **目录名编码有损**：`project-my-notes` 解码后会变成 `project\my\notes`，所以真实 cwd 一律从会话文件内容里 peek，解码只作兜底
- **WorkBuddy 标题**：真人输入被包在 8KB 注入上下文末尾的 `<user_query>` 里，必须优先识别该标签，否则标题会变成 `OS Version: win32 Shell: bash…`
- **盘符大小写**：Claude 记 `C:\`，WorkBuddy 记 `c:\`，不统一会让同一目录看起来像两个

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
cargo test                        # 只读守卫 + 解析启发式的单测
cargo run --example scan          # 列出本机所有 agent 和项目
cargo run --example scan -- full  # 连每场会话的统计一起列
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
src/                    Svelte 5 前端，无 UI 库、无图表库（图表手写 SVG，省体积）
  App.svelte            三栏骨架 + agent 侧栏
  lib/ProjectList.svelte
  lib/SessionList.svelte
src-tauri/
  src/model.rs          跨 agent 统一数据模型
  src/paths.rs          路径解析 + 只读守卫（含单测）
  src/commands.rs       暴露给前端的只读命令
  src/adapters/         每家 agent 一个实现 + 共用的标题提取启发式
  examples/scan.rs      命令行冒烟检查，不起窗口
```

分工约定：`list_projects` 只做目录枚举和 stat，保证首屏秒开；`list_sessions` 才逐行解析该项目的 JSONL，按需付费。
