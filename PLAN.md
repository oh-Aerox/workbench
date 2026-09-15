# 后续计划

需求原文见 [require.md](require.md)，实现细节和踩过的坑见 [README.md](README.md)。
本文件只记**还没做完的事**，做完一项就勾掉。

## 需求达成情况

| # | 需求 | 状态 |
|---|---|:--|
| 1 | agent 工作进度可视化工作台 | ✅ |
| 2 | 只读，不写任何 agent 工作区 | ✅ 三层保证 + 4 条守卫单测 |
| 3 | 支持 claude / codex / workbuddy | ✅ 三家适配器均已验证 |
| 4 | 支持 Win10+ 与 Mac M 芯片 | ✅ 两端均已出包并冒烟通过 |
| 5 | 安装后体积尽可能小 | ✅ Win 1.04 MB / Mac DMG 1.5 MB |
| 6 | 按 agent 划分，允许项目重复 | ✅ 不做跨 agent 合并 |

六条需求全部达成。剩余条目都是增强或未验证项，不阻塞验收。

---

## P0 · 阻塞需求验收

- [x] ~~**macOS (Apple Silicon) 出包**~~ —— 已完成
  - `CI=true npm run tauri build`。**`CI=true` 不能省**：否则 `create-dmg` 的
    Finder 美化步骤会卡到 AppleEvent 超时（-1712）而整个出包失败，详见 README「出包」
  - 产物 `Agent Workbench_0.1.0_aarch64.dmg` 1.5 MB，`.app` 3.5 MB，arm64 单架构
  - `tauri.conf.json` 补了 `bundle.macOS.minimumSystemVersion: "11.0"`
    （Tauri 默认 10.13 是 Intel 时代的值，arm64 本身就要 11 起步）
  - 冒烟：从 DMG 装出来启动正常，进程稳定、LaunchServices 已注册为 GUI 应用
- [x] ~~**Mac 上先验证适配器再出包**~~ —— 已完成，`cargo test` 28 绿 + `scan` 三家全通
  - Codex 不需要微调：本机 `~/.codex/sessions/YYYY/MM/DD/` 结构存在
    （23 个 rollout + 4 个归档），`collect_rollouts()` 原样可用
  - 实机解析：Claude 3 项目 / Codex 9 项目 / WorkBuddy 13 项目，路径全部正确
  - **发现并修掉一个真实 bug**：macOS 上 Claude 把 `/Users/a/x` 编码成 `-Users-a-x`
    （保留前导 `/`），**WorkBuddy 却编码成 `Users-a-x`（吃掉前导 `/`）**。
    原 `decode_project_dir` 只认前导 `-`，后者会掉进 Windows 分支被解成 `Users\a\x`。
    已按 `#[cfg(unix)]` 分平台还原并补单测。
    注：这条兜底路径平时走不到（三家都优先 `peek_cwd` 读会话内容里的真实 cwd），
    本机 25 个项目全部走的 peek，所以 GUI 上看不出来——属于潜伏 bug

## P1 · 未验证的代码

- [x] ~~**验证 Codex `update_plan` 提取**~~ —— Mac 实机数据验证通过
  - `cargo run --example scan -- plans` 提取到 17 份计划，勾选态、条目文字、
    伴随说明（`plan_from_call` 的正文）全部正确，字段形状的猜测是对的
- [ ] **验证 Claude `TodoWrite` 提取** —— 仍然无法验证
  - Mac 上照样没有真实数据：`~/.claude/projects/` 全量 grep `TodoWrite`
    只命中 1 个文件，且是本次会话记录里的字面量，不是真实工具调用（`todos` 字段 0 处）
  - 结论：这条要等到实际用一次带任务清单的 Claude 会话才能验，两台机器都没有样本
- [ ] **`update_plan` 快照去重**（Mac 验证时新发现）
  - Codex 每调一次 `update_plan` 就落一条记录，现在把每次快照都当成独立计划列出：
    同一场会话会出现 3–5 份只差勾选进度的「任务计划」，MaxKB / canpay-web 都是如此
  - 更合理的做法是按会话折叠、只留最后一次快照（或展示成进度演进）
  - 不是 bug，是展示口径问题，但计划面板的信噪比明显受影响

## P2 · 数据源扩展（需先决策）

- [x] ~~**源码 `TODO/FIXME` 作为计划面板的补充分区**~~ —— 已实现，见 `repo.rs`
  - **项目计划文档**（本文件这类 `PLAN.md`）自动加载：只走两层目录、毫秒级，
    与 agent 计划合并成一个列表。这才是主角
  - **代码 TODO 标记**手动按钮触发，白名单限制可扫路径，跑在阻塞线程池
  - 实测：eshop 7162 文件首次 85s / 缓存后 1.2s；独立单词规则滤掉 56 处误报
  - **遗留噪声**：eshop 的 296 处里 102 处来自内嵌第三方库
    `inc/class/phpQuery/phpQuery.php`，目录名不在常规忽略列表里，识别不出来。
    可考虑的改进：按「单文件命中数异常高」或「平均行长过长（疑似压缩/生成物）」
    再加一层启发式过滤

## P3 · 功能增强

- [ ] **会话级逐轮时间线**
  - P2 阶段的另一个选项（当时选了「成果盘点」）
  - 点开单场会话看逐轮对话、工具调用分布、该场触达的文件清单
- [ ] **导出报告**（markdown / HTML）
- [ ] **多 agent 横向对比**：同一项目被不同 agent 跑过时的用量对比
- [ ] **代码签名**（仅对外分发时需要）
  - Windows：未签名会弹 SmartScreen，需 OV 证书
  - macOS：当前是 adhoc linker-signed（`codesign -dv` 确认），本机自装没问题，
    但拷给别人会被 Gatekeeper 拦；正式分发需 Apple 开发者账号 + 公证

---

## 已知限制（数据源本身的缺失，不是 bug）

这些不在计划内——除非上游 agent 改变记录方式，否则做不到：

- **拿不到逐文件的增删行数**。Claude 只在 `cost-state` 里记会话级总计，
  不记每个文件改了多少行。所以文件排行只能按改动次数排，行数排行只到会话粒度。
  要做到文件级需读 `file-history` 备份文件做 diff，会大幅拖慢并偏离只读轻量的定位
- **Codex 不做文件历史追踪**，成果盘点的文件清单对它恒为空
- **WorkBuddy 没有花费、增删行数、git 分支、计划/待办**，其工具集里就没有这些
- **目录名编码有损**，真实 cwd 一律从会话内容 peek，兜底解码无法还原
  路径里原有的 `-`

## 维护须知

- 改动任何 adapter 的统计口径后，**必须给 `src-tauri/src/index.rs` 的 `VERSION` +1**，
  否则旧缓存会以新字段含义被读出来
- 提交前检查隐私：agent 会在项目目录里留下自有工作目录（`.workbuddy/` 等），
  内含本机用户名，已在 `.gitignore` 挡掉
- 改完 adapter 先跑 `cargo run --example scan` 系列冒烟检查，比开 GUI 快得多
