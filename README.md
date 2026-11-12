# Agent Workbench

一个本地桌面应用，把 **Claude Code**、**Codex**、**WorkBuddy** 三家 AI 编程 agent 在你机器上留下的会话记录聚合起来，按「agent → 项目 → 会话」三级展示工作进度。

**完全只读**：不联网、不上传，也绝不写入任何 agent 的工作目录。

基于 [Tauri 2](https://tauri.app/) + [Svelte 5](https://svelte.dev/) 构建，安装包约 1–1.5 MB。

## 功能

- **会话总览**：每场会话的标题、轮数、工具调用次数、模型、花费、增删行数、git 分支，以及是否仍在进行中
- **活动热力图**：近 26 周按天的活动量，跨天会话按事件时间逐条归档
- **成果盘点**：项目内文件改动排行、各会话的贡献排行
- **计划 / 待办**：汇总三类来源
  - 项目里的计划文档（`PLAN.md`、`TODO.md`、`ROADMAP.md` 等）
  - agent 自己产出的计划（Claude 的计划模式、`TodoWrite` 清单等）
  - 源码注释里的 `TODO` / `FIXME` / `XXX` / `HACK`（手动触发扫描）
- **全局搜索**：跨所有 agent、所有项目搜索会话内容
- **实时刷新**：监听 agent 目录，会话有新内容时自动更新
- **增量缓存**：只重新解析变化过的文件，二次打开约快 13 倍

## 支持的 agent

| Agent | 读取位置 |
|---|---|
| Claude Code | `~/.claude/projects/`、`~/.claude/plans/` |
| Codex | `~/.codex/sessions/`、`~/.codex/archived_sessions/` |
| WorkBuddy | `~/.workbuddy/projects/`、`~/.workbuddy/sessions/` |

各家记录的字段不同，卡片上显示 `—` 表示该 agent 本身不提供这项数据，不是解析失败。字段对照表见 [docs/DESIGN.md](docs/DESIGN.md#各家能提供的字段不一样)。

同一个目录被多个 agent 用过时，会在每个 agent 下各出现一次。这是有意的设计，不做跨 agent 合并。

## 平台支持

| 平台 | 状态 |
|---|---|
| Windows 10 / 11 (x64) | ✅ |
| macOS 11+ (Apple Silicon) | ✅ |
| macOS (Intel) / Linux | 未测试，欢迎反馈 |

## 安装

暂无预编译版本，需要从源码构建（见下文）。

构建出的安装包**未签名**，首次运行时：

- **Windows**：SmartScreen 会弹出提示，点「更多信息 → 仍要运行」
- **macOS**：Gatekeeper 会拦截，在 Finder 中右键 → 打开；或执行
  `xattr -dr com.apple.quarantine "/Applications/Agent Workbench.app"`

## 从源码构建

### 准备环境

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install) 1.77+
- Windows：VS 2022 Build Tools（勾选「使用 C++ 的桌面开发」）
- macOS：Xcode Command Line Tools（`xcode-select --install`）

Rust 只在编译时需要，运行时不需要。两个平台需要各自在本机构建，不能交叉编译。

### 开发

```bash
npm install
npm run tauri dev
```

### 打包

```bash
npm run tauri build            # Windows → src-tauri/target/release/bundle/nsis/
CI=true npm run tauri build    # macOS   → src-tauri/target/release/bundle/dmg/
```

macOS 上 `CI=true` 不能省：否则打 DMG 时会卡在 Finder 摆放图标的步骤，等到 AppleEvent 超时而失败。原因见 [docs/DESIGN.md](docs/DESIGN.md#开发)。

### 测试

```bash
cd src-tauri
cargo test                     # 只读守卫 + 解析逻辑单测
cargo run --example scan       # 不开窗口，直接打印本机的解析结果
cd .. && node --test tests/*.test.mjs  # 前端测试
```

`scan` 还支持 `full`、`outcome`、`heat`、`plans`、`todos`、`find <关键词>` 等子命令，见 [`src-tauri/examples/scan.rs`](src-tauri/examples/scan.rs)。

## 隐私与安全

这个应用读的是你最敏感的工作数据，所以只读保证由代码结构强制，而不是靠约定：

- 读取 agent 数据的文件句柄一律以只读方式打开
- 所有写盘操作只有一个出口，并断言目标位于应用自己的数据目录内；有单测扫描源码，防止新代码绕过
- 二进制根本没有编译进 Tauri 的 fs / shell 插件，前端拿不到任何写文件的能力
- 不发起任何网络请求

**应用会在本地写一份缓存**，用来加速二次打开：

- Windows：`%LOCALAPPDATA%\AgentWorkbench\parse-cache.json`
- macOS：`~/Library/Application Support/AgentWorkbench/parse-cache.json`

缓存是**明文**，其中包含每场会话的首条 prompt 和触达过的文件路径。左侧栏底部会显示缓存大小，可一键清空；源文件删除后，对应的缓存条目也会在下次启动时清掉。

**源码 TODO 扫描**是唯一会读取 agent 目录之外文件的功能。它只扫描出现在 agent 项目列表里的目录，且必须手动点按钮才会执行。

完整的威胁模型与实现细节见 [docs/DESIGN.md](docs/DESIGN.md#只读保证)。

## 项目结构

```
src/                 Svelte 5 前端（无 UI 库、无图表库）
src-tauri/src/
  adapters/          每家 agent 一个适配器
  commands.rs        暴露给前端的只读命令
  index.rs           增量解析缓存
  paths.rs           路径解析 + 只读守卫
  repo.rs            计划文档发现 + 源码 TODO 扫描
  watcher.rs         agent 目录监听
src-tauri/examples/  命令行冒烟检查
docs/DESIGN.md       设计说明、数据格式细节、踩过的坑
PLAN.md              后续计划与已知限制
```

## 参与贡献

欢迎提 Issue 和 PR，尤其是：

- 新 agent 的适配器：实现 `adapters/` 下的 trait 即可，各家只需实现 `parse_project` 一个方法
- Intel Mac / Linux 上的实测反馈
- Codex `update_plan` 的真实数据样本：目前的解析是按常见格式写的，还没用真实数据验证过

改动适配器之前请先读 [docs/DESIGN.md](docs/DESIGN.md)，里面记录了各家数据格式的坑。改动后请先跑 `cargo test` 和 `cargo run --example scan`；如果改了缓存里某个字段的统计口径，记得把 `index.rs` 里的 `VERSION` 加 1。

## 许可证

[MIT](LICENSE)

本项目与 Anthropic、OpenAI 及 WorkBuddy 均无关联。各产品名称归其所有者所有。
