# 后续计划

需求原文见 [require.md](require.md)，实现细节和踩过的坑见 [README.md](README.md)。
本文件只记**还没做完的事**，做完一项就勾掉。

## 需求达成情况

| # | 需求 | 状态 |
|---|---|:--|
| 1 | agent 工作进度可视化工作台 | ✅ |
| 2 | 只读，不写任何 agent 工作区 | ✅ 三层保证 + 4 条守卫单测 |
| 3 | 支持 claude / codex / workbuddy | ✅ 三家适配器均已验证 |
| 4 | 支持 Win10+ 与 Mac M 芯片 | ⚠️ **Win 已验证，Mac 未出包** |
| 5 | 安装后体积尽可能小 | ✅ 安装包 1.04 MB |
| 6 | 按 agent 划分，允许项目重复 | ✅ 不做跨 agent 合并 |

只剩需求 4 的 macOS 一半没完成。

---

## P0 · 阻塞需求验收

- [ ] **macOS (Apple Silicon) 出包**
  - 环境只需 Rust + Xcode Command Line Tools，运行期不需要 Rust
  - `npm install && npm run tauri build`，产物在 `src-tauri/target/release/bundle/dmg/`
  - 两平台无法交叉编译，必须在 Mac 本地跑
- [ ] **Mac 上先验证适配器再出包**
  - 先跑 `cd src-tauri && cargo run --example scan`
  - 重点看 Codex：本机 `~/.codex/sessions/` 是空的、数据全在 `archived_sessions/`。
    Mac 上若目录结构不同，`codex.rs` 的 `collect_rollouts()` 需要微调
  - 路径解码已按平台分支处理（POSIX 绝对路径编码后以 `-` 开头），但没有真机验证过

## P1 · 未验证的代码

- [ ] **验证 Codex `update_plan` 提取**
  - `codex.rs` 的 `plan_from_call()` **从未跑过真实数据**——本机 Codex 会话里
    没出现过这个工具，字段形状是按常见约定写的
  - 验证方法：用 Codex 做一次带计划的任务，然后
    `cargo run --example scan -- plans` 看能否正确提取
  - 解析已做宽容处理（对象和纯字符串都接受，对不上就返回 None），
    不会把整场会话的解析带崩，但正确性未知
- [ ] **验证 Claude `TodoWrite` 提取**
  - 同样未跑过真实数据（本机 `TodoWrite` 使用次数为 0）
  - 用一次带任务清单的会话即可验证

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
  - macOS：未签名会被 Gatekeeper 拦，需 Apple 开发者账号 + 公证

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
