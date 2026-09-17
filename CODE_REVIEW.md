# Agent Workbench 代码审查报告

## 当前复核结论（2026-09-17）

**尚未全部解决。** 本次复核对象是当前工作区中的未提交修复，不是下方原始报告的 `main (e0aa072)`。原始报告保留作问题来源；当前状态、严重程度与修复安排以本节和 [PLAN.md](PLAN.md) 为准。

当前未发现仍未处理的原报告“严重”级核心缺陷。**两个 P1（#9、#12）已修复**；剩余 **#16 为中，#11、#13 及 #7 的剩余性能优化为低**。严重程度按实际影响和触发条件评定，与 PLAN 的执行优先级分别记录。

### 17 项状态对照

| 原编号 | 问题 | 当前状态 | 剩余严重程度 |
|---|---|---|---|
| #1 | running 永久吃缓存 | 已修复：`cached_collect` 返回前重算 | — |
| #2 | slug 路径穿越 | 已修复：`resolve_under` 校验，读取走 `open_readonly` | — |
| #3 | 非 UTF-8 行截断解析 | 已修复：使用 `LossyLines` | — |
| #4 | 计划快照重复 | 已修复：Codex / Claude 共享快照折叠；Claude 真实 TodoWrite 样本验证仍待完成 | — |
| #5 | Unicode 偏移导致 panic | 已修复：摘要和相对路径不再用错误字节偏移切片 | — |
| #6 | 写操作只靠约定 | 已按建议增加封装和源码扫描单测；该检查不是完备的静态分析 | — |
| #7 | Codex 重复 peek | 核心问题已修复：cwd 按指纹缓存；仍有重复遍历与名称索引读取 | 低 |
| #8 | 重 IO 命令占主线程 | 已修复：同步命令加 async 标记，TODO 扫描保留 spawn_blocking | — |
| #9 | 缓存无上限、无清理入口 | 已修复：清空不主动重建，等待在途查询结束，查询后刷新大小并防止旧响应覆盖 | — |
| #10 | Claude prompt 未剥注入 | 已修复：应用 `strip_injected`，与 WorkBuddy 同口径 | — |
| #11 | 编码文件被静默跳过 | 部分修复：GBK 可扫描 ASCII 标记但中文乱码；UTF-16 仍可能被判为二进制跳过 | 低 |
| #12 | 白名单路径比对偏宽 | 已修复：比较真实目录 canonicalize 结果，不做小写化或分隔符替换兜底 | — |
| #13 | 断言副作用 / TOCTOU | 部分修复：断言不再建目录；检查与实际操作之间仍有竞态窗口 | 低 |
| #14 | 去抖无限等待 | 已修复：增加 5 秒绝对上限 | — |
| #15 | Mutex 中毒后静默失效 | 已修复：取回中毒锁内的数据继续使用 | — |
| #16 | 按下标保存折叠状态 | 原下标问题已改，但引入默认计划无法收起的回归 | 中 |
| #17 | 重复剥离文件后缀 | 已修复：改为 `strip_suffix` | — |

### 两个 P1 的修复记录

**#12 · 已修复（修复前：中，P1）**

- 位置：`src-tauri/src/adapters/mod.rs::same_path`，由 `commands.rs::ensure_known_project` 调用。
- 白名单现在只比较实际存在目录的 `canonicalize` 结果；不存在、非目录或无法解析时拒绝。大小写和链接解析交给文件系统，不再将 `/repo/A` 与 `/repo/a` 无条件等同，也不在 Unix 上替换反斜杠。
- 回归测试覆盖不存在目录、普通文件、目录别名 `src/..`、Windows 分隔符，以及当前文件系统的大小写行为。Windows 本机通过；macOS / 大小写敏感卷尚未实机复验。会话内容仍属于 README 声明的信任边界。

**#9 · 已修复（修复前：中，P1）**

- 清空后不再调用 `selectAgent`；同时清除前端源码扫描缓存。后续数据查询完成时通知界面重新读取缓存大小，使用请求序号防止旧大小响应覆盖新值；大小读取失败显示“未知”，不冒充空缓存。
- 所有会写缓存的后端查询命令持共享操作锁，清空持独占锁，等待已在执行的查询及落盘结束后删除缓存；删除错误返回界面。落盘持缓存锁完成临时文件写入和改名，避免多个查询同时操作同一临时文件。
- 回归测试覆盖清空不触发解析、查询后大小通知、响应乱序与读取失败、清空等待在途查询。后续新的查询或文件监听刷新仍可合法重建缓存，这不是“关闭缓存”设置。
- 已有条目上限和启动淘汰有效；当前仍存明文 prompt / 计划内容，选择的是原报告允许的“上限 + 清理 + 告知”方案，不能宣称已消除内容落盘。

### 剩余问题的影响与验收条件

**#16 · 中 · 默认展开的计划无法收起（优先级 P2）**

- 位置：`src/lib/PlansView.svelte::isOpen` / `toggle`。
- 空集合同时代表“尚未操作”和“全部收起”。默认卡片首次点击只把 key 加入集合；再次点击删掉 key 后又触发默认展开。已用当前函数复现连续两次点击仍展开。在只有一份计划时用户无法收起它，属于常见操作的确定性回归。
- 验收：区分初始默认状态和用户主动收起状态；覆盖单计划首次点击收起、再次展开、全部收起，以及列表刷新/重排后状态不串到另一计划。

**#11 · 低 · UTF-16 跳过与 GBK 中文乱码（优先级 P2）**

- 位置：`src-tauri/src/repo.rs::read_text`。
- `from_utf8_lossy` 前仍有零字节检查，常见 UTF-16 文本会提前返回 `None`；GBK 的 ASCII TODO 可命中，但中文说明不能正确恢复。影响特定编码项目的扫描完整性，不影响 UTF-8 主路径，也不造成写入或崩溃。
- 验收：覆盖带 BOM 的 UTF-16 LE/BE、GBK 中文说明和真实二进制文件；正确解码，或明确报告不支持的编码及跳过数量，不能继续静默当空结果。

**#13 · 低 · 路径检查与写操作间的竞态（优先级 P3）**

- 位置：`src-tauri/src/paths.rs::write_app_file` / `rename_app_file` / `remove_app_file`。
- 断言副作用已解决，但检查后仍按路径发起文件操作。利用剩余窗口需要本地进程能替换相关目录/链接并命中操作时机；本次未进行竞态攻击复现。按当前本地应用威胁模型评为低，不能因此宣称写入侧已消除 TOCTOU。
- 验收：若实施加固，采用能约束目录身份及链接解析的文件操作并做平台验证；若接受风险，单独记录风险接受与前提，保持“未消除”状态，不标为已修复。

**#7 · 低 · 主要瓶颈已处理，重复遍历仍可优化（优先级 P2）**

- 位置：`src-tauri/src/adapters/codex.rs::parse_project`。
- cwd 已走指纹缓存，不再按项目重复打开会话文件读取头部；但每个项目仍重新 `collect_rollouts()`，并重读 `session_index.jsonl`。数据量增长时仍有额外目录遍历、stat 和索引读取开销，不应再沿用原先“peek 完全不走缓存”的描述。
- 验收：用多项目、多会话数据测量热缓存扫描；必要时在一次批量查询内共享 rollout 分组和名称索引，确认统计与项目归属不变。

### 验证记录与范围

- 2026-09-17 两个 P1 修复后实际执行：`cargo test --manifest-path src-tauri/Cargo.toml`，**45 项通过**；`node --test tests/cache-ui.test.mjs`，**3 项通过**；`npm run build`，**通过**。修复前已用回归测试复现清空重新解析、清空不等待查询及白名单接受不存在目录。
- 提取当前计划面板函数执行，确认默认计划初始展开、点击一次及两次后仍展开。UTF-16 零字节条件通过构造字节样本检查，其余遗漏由当前代码调用链确认。
- 未重新执行 macOS 出包、大小写敏感卷实机复验、完整 GUI 冒烟、Clippy 或 TOCTOU 攻击验证；测试通过不代表剩余 P2/P3 问题已修复。

---

## 原始审查报告（历史基线，以下问题描述不是当前修复状态）

> 审查对象：main (e0aa072)，约 2900 行 Rust + 12 个 Svelte 组件
> 方式：静态阅读（git show / git grep / git ls-tree），未编译、未跑测试、未改任何文件
> 中继：WorkBuddy -> Claude Code (session 952a91d1)，花费 $2.01，19 轮

---

RELAY-01 报告，全程只读（`git show` / `git grep` / `git ls-tree`），未修改、创建或删除任何文件。

**审查对象**：`main`（`e0aa072`），不是你当前检出的 `feat/workbench-p0-p1`（该分支缺 macOS 那次提交里对 `paths.rs` / `index.rs` 的修改）。约 2900 行 Rust + 12 个 Svelte 组件，全部通读。未编译、未跑测试（只读约束），结论来自静态阅读。

## 先说结论：只读守卫的实际强度

三层里**第 3 层最硬，第 1 层名不副实，第 2 层只是约定**：

- **第 3 层（能力面）真实可靠，且比文档说的更强**：`Cargo.toml` 里根本没有引入 `tauri-plugin-fs` / `tauri-plugin-shell`，仓库里也没有 `capabilities/` 目录。不是"没开权限"，是**写文件的代码压根没被编译进二进制**。前端无论如何拿不到写能力。
- **第 1 层有例外**：`claude.rs:229` 用 `std::fs::read_to_string` 读 `~/.claude/plans/*.md`，绕过了 `open_readonly`。这条不构成写风险（`read_to_string` 本来就只读），但"一律走 open_readonly"的说法不成立。
- **第 2 层不是结构性保证**：`assert_app_owned` 靠开发者记得调用。`std::fs::write` 在 crate 里任何地方都能直接调。目前确实只有 `index.rs` 写盘且调了守卫，所以**当下是对的**，但没有任何机制阻止下一次提交绕过它。4 条守卫单测测的是"守卫函数本身判得对不对"，不是"是否所有写操作都过了守卫"——后者无测试覆盖。

守卫函数本身的逻辑我没找到可绕过的缺口：`normalize()` 对最深已存在祖先做 `canonicalize` 再拼尾段，规避了 Windows `\\?\` 前缀不一致的经典坑；`..` 被组件级拒绝而不是靠字符串比对；符号链接会在 `canonicalize` 阶段被解析到真实位置，指向 `~/.claude` 的链接会被拦下。

---

## 问题清单

### 严重

**1. `running`（进行中）标志在解析时算一次就被写进缓存，之后永久错误**

- 位置：`src-tauri/src/adapters/claude.rs:463`、`codex.rs:292`、`workbuddy.rs:280`，配合 `src-tauri/src/index.rs:84`
- 问题：`running: now_ms() - last_write < RUNNING_WINDOW_MS` 在 `parse_session` 里计算，结果作为 `ParsedSession` 的一部分进缓存。缓存的失效条件是 `(size, mtime)` 变化——而会话结束后文件不再变化，缓存永远命中，`get_or_parse` 直接 `return Some(e.parsed.clone())`，`running` 不重算。
- 影响：**任何一场"用户在 agent 干活时打开看过"的会话，会永久显示"进行中"**。缓存落盘到 `parse-cache.json`，重启应用也不会恢复，而 `index::clear()` 是死代码（见问题 9），用户没有任何手段清掉。这是必然触发、不可自愈的错误状态，且恰好命中本应用的核心卖点（进度可视化）。
- 建议：`running` 是时间派生量，不该进缓存。从 `SessionSummary` 里移除，改在 `list_sessions` / `aggregate` 返回前按 `mtime_ms(path)` 现算；或把它从 `ParsedSession` 提到缓存层之外，缓存只存 `last_write`。

**2. `plan_from_file` 用会话文件里的 `slug` 直接拼路径，存在路径穿越**

- 位置：`src-tauri/src/adapters/claude.rs:227-229`，输入来自 `claude.rs:330-336`
- 问题：`slug` 从 JSONL 的 `"slug"` 字段原样读出，未做任何字符校验，直接 `agent_root.join("plans").join(format!("{slug}.md"))`。`slug` 为 `../../../Desktop/机密` 时会读到 `~/Desktop/机密.md`，内容作为"计划正文"完整渲染进计划面板。
- 影响：信息泄露。前置条件是攻击者能控制 `~/.claude/projects/**/*.jsonl` 的内容——门槛不低，但会话文件是会被复制、同步、从他处拷入的数据，不该当成可信输入。另外注意：`repo.rs` 的白名单机制明确声明"这是唯一读 agent 目录之外文件的地方"，而这条路径是第二处，且无任何校验，与文档相矛盾。
- 建议：校验 `slug` 只含 `[A-Za-z0-9._-]` 且不等于 `.`/`..`（拒绝任何路径分隔符），或对拼出的路径做一次与写入侧对称的 `assert_under(plans_dir)` 读侧断言。

**3. `map_while(Result::ok)` 会在第一个非 UTF-8 行处静默终止整个文件的解析**

- 位置：`claude.rs:52`、`claude.rs:303`、`codex.rs:79`、`codex.rs:102`、`codex.rs:199`、`workbuddy.rs:44`、`workbuddy.rs:182`
- 问题：`BufRead::lines()` 遇到无效 UTF-8 会 yield `Err`，`map_while(Result::ok)` 把它变成 `None` 从而**结束迭代**——不是跳过这一行，是丢弃该行之后的**全部内容**。`claude.rs:307` 的注释"单行解析失败就跳过"只对 JSON 语法错误成立（那条走 `continue`），对 IO/编码错误不成立。
- 影响：一次编码损坏（并发写入撕裂、混入非 UTF-8 字节）就让整场会话的轮数、工具调用、花费、文件清单全部少算，**而且错误结果会连同指纹一起写进缓存固化下来**，除非文件再次变化否则永远不会重算。失败完全静默，UI 上看不出区别。在 `peek_meta` / `peek_cwd` 里则表现为读不到 `cwd`，回退到有损的 `decode_project_dir`，项目路径显示错误。
- 建议：改成 `filter_map(Result::ok)`（跳过坏行、继续读后面）。若要保留"尾部半行"的语义，可以改为按 `read_until(b'\n')` 读字节再 `String::from_utf8_lossy`。

### 中

**4. Codex 的 `update_plan` 快照零去重，且 Claude 的 `TodoWrite` 有同样的缺陷**

- 位置：`src-tauri/src/adapters/codex.rs:226-233` + `adapters/mod.rs:33-50`
- 问题：`response_item` 每命中一次 `function_call`，`plan_from_call` 就无条件 `plans.push(p)`。`project_plans` 只做时间排序，没有任何折叠。`PLAN.md` 已记录此事（MaxKB / canpay-web 实测同一会话出现 3–5 份）。
- 影响：同一场会话产出一串只差勾选进度的"任务计划"卡片，计划面板信噪比崩坏。补充一点 PLAN.md 没写的：**`claude.rs:191-219` 的 `TodoWrite` 分支结构完全相同**——每次 `TodoWrite` 调用也 push 一条 `title: "任务清单"`。本机因为没有真实 `TodoWrite` 数据所以没暴露，一旦有数据就是同样的雪崩。修去重时应当一并覆盖，别只修 Codex。
- 建议：不要在 `plan_from_call` / `plan_from_tool` 层去重（那里看不到全局）。在 `parse_session` 收尾处按 `(kind, source)` 分组，同组只保留 `at` 最大的一条；或保留全部但给 `PlanEntry` 加 `supersedes: usize`，前端折叠成一条带"演进 N 次"角标。现有的按正文去重（`claude.rs:431-439`）不适用，因为这两类 entry 的 `body` 都是 `None`。

**5. `snippet()` 用 `to_lowercase()` 的字节偏移去切原串，可触发 panic；`panic = "abort"` 会让整个应用直接退出**

- 位置：`src-tauri/src/commands.rs:181-190`
- 问题：`byte_pos` 是在 `lower` 里找到的偏移，却被用来切 `text`（`text[..byte_pos]`）。`to_lowercase()` 不保长：`İ`(U+0130, 2 字节) 小写化成 `i̇`(3 字节)。偏移一旦错位并落在多字节字符中间，切片直接 panic。可复现输入：会话文本 `"İ中"`，搜索 `中` → `lower.find` 返回 3，而 `text` 的第 3 字节在 `中` 的内部 → panic。
- 影响：`Cargo.toml:29` 设了 `panic = "abort"`，release 下没有 unwind，**应用整体崩溃退出**，且触发源是用户输入的搜索词。即便不 panic，偏移错位也会让摘要窗口取错位置。
- 建议：改成在原串上定位：`text.char_indices()` 配合逐字符小写比较，或用 `text.to_lowercase()` 的**字符**索引（`lower.chars()` 逐个数）而不是字节索引。同类隐患还有 `adapters/mod.rs:166-173` 的 `relative_to`，它用 `dir`（原串）的长度去切 `path`，而是否匹配是用小写串判定的——两者大小写形态不同且含非 ASCII 时长度会对不上。

**6. 只读守卫缺少强制机制，靠约定维持**

- 位置：`src-tauri/src/paths.rs:1-6`（三层声明）、`index.rs:154`
- 问题：见开头的结论。第 2 层是"记得调用"，第 1 层已经有一处例外，且没有任何静态检查阻止后续代码直接 `std::fs::write`。
- 影响：这是本应用的**最高优先级需求（需求 2）**，却是全项目唯一没有自动化兜底的不变量。一次疏忽的提交就能破坏它，而现有 4 条单测不会失败。
- 建议：(a) 把 `fs::write` / `fs::rename` / `fs::remove_file` 封进 `paths.rs` 的 `write_app_file()` / `remove_app_file()`，内部先断言后操作，成为唯一出口；(b) 加 `clippy.toml` 的 `disallowed-methods` 禁掉 `std::fs::write` 等，或在 CI 里加一条 `grep` 断言"`fs::write` 只在 paths.rs 出现"；(c) 补一条单测覆盖"读侧路径也不越界"，把问题 2 一并纳入。

**7. Codex adapter 的 `peek_cwd` 呈 O(项目数 × 会话数) 重复执行，且完全不走缓存**

- 位置：`src-tauri/src/adapters/codex.rs:162-170`、`123-138`
- 问题：`parse_project` 每次都 `collect_rollouts()` 拿到**全部** rollout，再对每一个调 `peek_cwd` 过滤。而 `parse_all`（`mod.rs:28-30`）= `list_projects()`（已经 peek 了一遍全部）+ 对每个项目调一次 `parse_project`（每次又 peek 全部）。P 个项目 N 个 rollout → `N + P×N` 次文件打开。`thread_names()` 也是每个项目重读一次 `session_index.jsonl`。
- 影响：按 PLAN.md 记录的 Mac 实测数据（9 项目 / 27 rollout），一次热力图刷新就是约 270 次冗余文件打开。真正耗时的全量解析有 `index` 缓存，而 `peek_cwd` **没有**——缓存暖了之后它反而成为 Codex 路径的主要开销，且随数据增长呈平方级放大。
- 建议：把 `rollout 路径 → cwd` 的映射按 `(size, mtime)` 加进 `index`（复用现成的 `get_or_scan_file` 模式），或在 `CodexAdapter` 上加一次性构建的 `cwd → Vec<PathBuf>` 分组，让 `list_projects` 和 `parse_project` 共用。

**8. 除 `project_todos` 外所有命令都是同步的，跑在主线程上**

- 位置：`src-tauri/src/commands.rs` 全部 `#[tauri::command]`（只有 `project_todos:75` 是 `async` + `spawn_blocking`）
- 问题：Tauri v2 里同步命令在主线程执行。`search`（遍历 3 家 × 全部项目 × 全部会话）、`agent_activity`（`parse_all`）、`project_docs`（还要先跑 `ensure_known_project`，而它会调三家的 `list_projects`，对 Codex 就是问题 7 的全量 peek）都是重 IO。
- 影响：搜索和切 agent 期间 UI 冻结，无法取消。`project_docs` 被宣传为"毫秒级、可自动加载"，但它的白名单校验本身可能比扫描更慢。
- 建议：给这几个命令加 `#[tauri::command(async)]`，或同样丢进 `spawn_blocking`。另外把 `ensure_known_project` 的项目列表做进程内缓存，别每次重建。

**9. `parse-cache.json` 明文存储全部 prompt 原文，无上限、无淘汰、无清理入口**

- 位置：`src-tauri/src/index.rs:36-45`、`143-157`、`160-169`
- 问题：缓存的 `Entry` 里存的是完整 `ParsedSession`，含 `first_prompt`（Claude 侧是**未剥离的原始 prompt 全文**）和全部文件路径，以明文 JSON 落在 `%LOCALAPPDATA%\AgentWorkbench\parse-cache.json`。`entries` / `repo` 两个 map 只增不减：会话文件被删除后条目仍然保留，没有任何 LRU 或容量上限。`clear()` 定义了但**全项目无调用点**，是死代码。
- 影响：(a) 隐私——应用把 agent 工作区里的敏感内容复制到了一个用户不知情的新位置，只读承诺保护的是"不写 agent 目录"，没覆盖"不外扩散数据"；(b) 磁盘占用随时间单调增长，且用户没有任何 UI 手段清理。
- 建议：落盘时剔除 `first_prompt`/`body` 这类正文字段（只缓存统计量，正文按需重读），或至少加容量上限 + 启动时清理"源文件已不存在"的条目，并把 `clear()` 接到一个设置项上。README 里也该写明缓存位置和内容。

### 低

**10. Claude 的 `first_prompt` 没做注入剥离，与 WorkBuddy 不一致** — `workbuddy.rs:285` 存的是 `strip_injected` 后的文本（注释明说"否则搜索会被 8KB 注入块淹没"），而 `claude.rs:352` 存原文。结果：搜索 Claude 会话会命中 `<system-reminder>` 里的注入内容，摘要显示成 `OS Version: win32` 之类。建议在 `claude.rs` 同样应用 `strip_injected`。

**11. `read_text` 只接受严格 UTF-8** — `repo.rs:167` 的 `String::from_utf8(buf).ok()?` 对 GBK / UTF-16 源文件返回 `None`，被当作"不该扫的文件"缓存成空结果，**静默跳过且不再重试**。中文项目里 GBK 源码并不罕见。建议至少用 `from_utf8_lossy`，或在报告里体现"N 个文件因编码跳过"。

**12. `ensure_known_project` 的白名单比对偏宽，且信任链有循环** — `commands.rs:86-92` 用 `to_lowercase()` 做全路径相等比对：在大小写敏感的文件系统上会放行大小写不同的另一个真实目录。更本质的是，白名单的可信项 `p.path` 来自 `peek_cwd`，即**被解析文件自身的内容**——控制了会话 JSONL 就能把任意目录送进白名单，然后让 TODO 扫描去遍历它并把内容回显。与问题 2 同源。建议用 `Path` 语义比对（或 `canonicalize` 后比对），并在 README 的威胁模型里写明"会话文件内容被视为可信输入"。

**13. `assert_app_owned` 有副作用，并存在 TOCTOU 窗口** — `paths.rs:79` 内部调 `app_data_dir()`，而后者会 `create_dir_all`（`paths.rs:32`）。一个断言函数不该改文件系统。另外断言与实际 `fs::write` 之间存在竞态窗口（可替换目录组件为符号链接）。本地场景风险极低，但把创建目录移到初始化阶段是零成本的改进。

**14. 去抖循环没有最大等待上限** — `watcher.rs:76` 的 `while let Ok(ev) = rx.recv_timeout(DEBOUNCE)`：只要事件间隔持续小于 1200ms，循环就无限延长，通知永远发不出去。agent 高频写入时（一次工具调用连写多行）正好落在这个区间，表现为"agent 越忙，UI 越不刷新"。建议加一个绝对上限（例如累计 5 秒强制发一次）。

**15. Mutex 中毒后缓存静默失效** — `index.rs:92/104/123/135` 全部写作 `if let Ok(s) = store().lock()`。任一持锁期间的 panic 之后，所有缓存读写都静默跳过，性能悄悄退化到全量解析且无任何日志。建议改用 `lock().unwrap_or_else(|e| e.into_inner())`。

**16. 计划面板折叠状态按数组下标存储** — `PlansView.svelte:9,11-15` 用 `Set<index>` 记展开态，而 `{#each}` 的 key 是 `p.sessionId + p.source + i`（含下标，等于没做内容键控）。文件监听触发 `selectProject` 重新加载后，展开的会是"位置相同的另一份计划"。建议改用内容键。

**17. `is_todo_doc` 的后缀剥离会重复生效** — `repo.rs:79` 的 `trim_end_matches(".md")` 会剥掉**连续多个**后缀，`plan.md.md` → `plan` 被判为计划文档，`a.txt.md` → `a`。用 `strip_suffix` 更准确。

---

## 没有发现问题的地方

- **前端无 HTML 注入面**：全项目零 `{@html}` / `innerHTML` / `eval`（已 grep 确认）。`Markdown.svelte` 把 markdown 解析成结构化 block 后走 Svelte 模板渲染，所有外部文本都经过自动转义；CSP 也限制了 `default-src 'self'`。这个取舍（自写渲染器换 +10KB 体积和零注入面）是对的。
- **目录遍历不会被符号链接带出项目**：`repo.rs` 用 `DirEntry::file_type()` 判断类型，它不跟随符号链接，链接既不满足 `is_dir()` 也不满足 `is_file()`，会被直接跳过——顺带也杜绝了环路。`scan_project` 用显式栈而非递归，无爆栈风险。三道闸门（忽略目录 / 文件大小上限 / 遍历数与命中数硬上限）齐全。
- **`.gitignore` 的隐私防护到位**：`.workbuddy/` `.claude/` `.codex/` `dist/` 均已排除，`git ls-tree` 确认仓库里没有任何会话数据或构建产物入库。
- **`aggregate` 里的路径归属判断**：`is_absolute` 覆盖了盘符 / POSIX / UNC 三形态，`starts_with_dir` 做了分隔符边界检查（`proj-backup` 不会被误判为 `proj` 的子目录），并有对应单测。
- **改动次数取 `max(deltas, max_version)` 而非相加**（`claude.rs:266-268`）：避开了全量快照重复计数导致的放大，注释把理由写清楚了。

## 建议的修复顺序

问题 1（running 永久错误）和问题 3（静默截断）都是"结果错了但看不出来"的类型，应当最先修；问题 5 是唯一能让应用直接退出的路径，改动量最小；问题 2 和 6 属于把已有的安全声明落到实处，适合一起做。问题 4 是 PLAN.md 上已排期的那条，修的时候记得把 Claude 的 `TodoWrite` 分支一并覆盖。
