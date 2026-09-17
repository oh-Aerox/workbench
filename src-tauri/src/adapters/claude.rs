//! Claude Code 适配器。
//!
//! 布局: ~/.claude/projects/<cwd编码>/<sessionId>.jsonl
//! 每行一条事件，关键类型:
//!   user / assistant         对话轮，assistant.message.content[] 里的 tool_use 即工具调用
//!   ai-title                 Claude 自己生成的会话标题，直接拿来当卡片标题
//!   cost-state               总花费、增删行数、各模型 token 用量
//!   file-history-snapshot    file-history-delta  本次会话触达过的文件
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{
    cached_collect, extract_checkboxes, fold_plan_snapshots, local_day, lossy_lines, make_title,
    markdown_title, now_ms, parse_iso_ms, strip_injected, AgentAdapter, RUNNING_WINDOW_MS,
};
use crate::model::{
    AgentKind, FileTouch, ParsedSession, PlanEntry, ProjectSummary, SessionSummary, TodoItem,
};
use crate::paths::{
    agent_root, decode_project_dir, normalize_drive, open_readonly, project_display_name,
    resolve_under,
};

pub struct ClaudeAdapter;

fn projects_dir() -> Option<PathBuf> {
    agent_root(AgentKind::Claude).map(|r| r.join("projects"))
}

/// 文件最后修改时间（epoch 毫秒）。
pub fn mtime_ms(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

/// 从会话文件头部读真实 cwd 和 git 分支。
///
/// 目录名编码是有损的（`my-notes` 会被解码成 `my\notes`），所以宁可多读几十行
/// 也要拿到准确路径。cwd 通常出现在第 5 行以内，这里最多读 60 行就收手。
fn peek_meta(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(file) = open_readonly(path) else {
        return (None, None);
    };
    let reader = BufReader::with_capacity(64 * 1024, file);
    let mut cwd = None;
    let mut branch = None;

    // lossy_lines：一个非 UTF-8 行不该让 cwd 读不到而退回有损的 decode_project_dir
    for line in lossy_lines(reader).take(60) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if cwd.is_none() {
            if let Some(c) = v.get("cwd").and_then(Value::as_str) {
                cwd = Some(c.to_string());
            }
        }
        if branch.is_none() {
            if let Some(b) = v.get("gitBranch").and_then(Value::as_str) {
                if !b.is_empty() && b != "HEAD" {
                    branch = Some(b.to_string());
                }
            }
        }
        if cwd.is_some() && branch.is_some() {
            break;
        }
    }
    (cwd, branch)
}

impl AgentAdapter for ClaudeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    /// 只 stat 不解析：目录名解码成 cwd，会话数=jsonl 个数，活跃时间=最新 mtime。
    /// 77MB 的历史数据全解析要几秒，首屏不值得付这个钱。
    fn list_projects(&self) -> Vec<ProjectSummary> {
        let Some(root) = projects_dir() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        let now = now_ms();
        let mut out = Vec::new();

        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            let Ok(files) = std::fs::read_dir(entry.path()) else {
                continue;
            };

            let mut session_count = 0usize;
            let mut last_active: Option<i64> = None;
            // 最新的那个会话文件，用来 peek 真实 cwd
            let mut newest: Option<(i64, PathBuf)> = None;

            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                session_count += 1;
                if let Some(m) = mtime_ms(&p) {
                    last_active = Some(last_active.map_or(m, |c: i64| c.max(m)));
                    if newest.as_ref().map_or(true, |(bm, _)| m > *bm) {
                        newest = Some((m, p));
                    }
                }
            }
            if session_count == 0 {
                continue;
            }

            let (peeked_cwd, git_branch) = newest
                .as_ref()
                .map(|(_, p)| peek_meta(p))
                .unwrap_or((None, None));
            let path =
                normalize_drive(&peeked_cwd.unwrap_or_else(|| decode_project_dir(&dir_name)));
            out.push(ProjectSummary {
                agent: AgentKind::Claude,
                id: dir_name,
                name: project_display_name(&path),
                path,
                session_count,
                last_active,
                first_active: None,
                total_wall_ms: 0,
                files_touched: 0,
                git_branch,
                running: last_active.map_or(false, |m| now - m < RUNNING_WINDOW_MS),
            });
        }

        out.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        out
    }

    fn parse_project(&self, project_id: &str) -> Vec<ParsedSession> {
        let Some(root) = projects_dir() else {
            return Vec::new();
        };
        let dir = root.join(project_id);
        let Ok(files) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };

        let paths: Vec<_> = files
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .collect();

        cached_collect(paths, |p| parse_session(p, project_id))
    }
}

/// 从一个 tool_use 块里提取计划 / 待办。
///
/// 两种来源：
///   ExitPlanMode  input.plan 是计划 markdown 全文
///   TodoWrite     input.todos 是 [{content, status, activeForm}] 结构化清单
fn plan_from_tool(block: &Value, at: Option<i64>) -> Option<PlanEntry> {
    let name = block.get("name").and_then(Value::as_str)?;
    let input = block.get("input")?;

    match name {
        "ExitPlanMode" => {
            let plan = input.get("plan").and_then(Value::as_str)?.trim();
            if plan.is_empty() {
                return None;
            }
            Some(PlanEntry {
                at,
                kind: "plan".into(),
                title: markdown_title(plan).unwrap_or_else(|| "实施计划".into()),
                items: extract_checkboxes(plan),
                body: Some(plan.to_string()),
                source: "ExitPlanMode".into(),
                revisions: 1,
            })
        }
        "TodoWrite" => {
            let todos = input.get("todos")?.as_array()?;
            let items: Vec<TodoItem> = todos
                .iter()
                .filter_map(|t| {
                    let text = t
                        .get("content")
                        .or_else(|| t.get("activeForm"))
                        .and_then(Value::as_str)?
                        .trim();
                    (!text.is_empty()).then(|| TodoItem {
                        text: text.to_string(),
                        status: t
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_string(),
                    })
                })
                .collect();
            (!items.is_empty()).then(|| PlanEntry {
                at,
                kind: "todos".into(),
                title: "任务清单".into(),
                body: None,
                items,
                source: "TodoWrite".into(),
                revisions: 1,
            })
        }
        _ => None,
    }
}

/// 读 ~/.claude/plans/<slug>.md。
///
/// 计划文件可能已被删除（Claude 会清理），读不到就跳过，不当错误。
///
/// **slug 来自会话 JSONL 的内容，是不可信输入**：原先直接 `join(format!("{slug}.md"))`，
/// slug 为 `../../../Desktop/机密` 时会把 `~/Desktop/机密.md` 整篇读出来渲染进
/// 计划面板。会话文件是会被复制、同步、从他处拷入的数据，必须过 `resolve_under`
/// 这道读侧守卫（写侧 `assert_app_owned` 的对称物）。
///
/// 另外这里改走 `open_readonly` 而不是 `std::fs::read_to_string`：只读守卫第 1 层
/// 声明「读 agent 数据一律走 open_readonly」，原先这一处是唯一的例外。
fn plan_from_file(slug: &str, at: Option<i64>) -> Option<PlanEntry> {
    let plans_dir = agent_root(AgentKind::Claude)?.join("plans");
    let path = resolve_under(&plans_dir, &format!("{slug}.md"))?;
    let mut file = open_readonly(&path).ok()?;
    let mut body = String::new();
    std::io::Read::read_to_string(&mut file, &mut body).ok()?;
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    Some(PlanEntry {
        at,
        kind: "plan".into(),
        title: markdown_title(body).unwrap_or_else(|| slug.to_string()),
        items: extract_checkboxes(body),
        body: Some(body.to_string()),
        source: format!("plans/{slug}.md"),
        revisions: 1,
    })
}

/// 累积某个文件的改动情况。
#[derive(Default)]
struct TouchAcc {
    /// file-history-delta 记录数：每条对应一次真实改动
    deltas: usize,
    /// 快照里的 version：Claude 自己记的版本号，等于该文件被改过的次数
    max_version: usize,
    last_at: Option<i64>,
}

impl TouchAcc {
    fn bump_at(&mut self, at: Option<i64>) {
        if let Some(at) = at {
            self.last_at = Some(self.last_at.map_or(at, |c: i64| c.max(at)));
        }
    }

    /// 两个来源取大者。
    ///
    /// 不能简单相加：`file-history-snapshot` 里的 `trackedFileBackups` 是**全量**
    /// 映射、每次快照重复出现，累加会把改动次数放大好几倍；而 delta 记录又可能
    /// 因为会话中途开始追踪而少于真实次数。取 max 两边都不亏。
    fn touches(&self) -> usize {
        self.deltas.max(self.max_version).max(1)
    }
}

fn parse_session(path: &Path, project_id: &str) -> Option<ParsedSession> {
    let file = open_readonly(path).ok()?;
    let bytes = file.metadata().ok().map(|m| m.len()).unwrap_or(0);
    let reader = BufReader::with_capacity(256 * 1024, file);

    let id = path.file_stem()?.to_string_lossy().to_string();
    let mut ai_title: Option<String> = None;
    let mut first_prompt: Option<String> = None;
    let mut started_at: Option<i64> = None;
    let mut ended_at: Option<i64> = None;
    let mut user_turns = 0usize;
    let mut assistant_turns = 0usize;
    let mut tool_calls = 0usize;
    let mut models: BTreeSet<String> = BTreeSet::new();
    let mut files: BTreeMap<String, TouchAcc> = BTreeMap::new();
    let mut git_branch: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut cost_usd: Option<f64> = None;
    let mut lines_added = 0i64;
    let mut lines_removed = 0i64;
    let mut daily: BTreeMap<String, u32> = BTreeMap::new();
    let mut plans: Vec<PlanEntry> = Vec::new();
    let mut slug: Option<String> = None;

    // 会话可能跨天（本机最长一场跨了 4 天多），活动量必须按事件时间逐条归到
    // 当天，不能用起止时间推算
    fn bump(daily: &mut BTreeMap<String, u32>, ts: Option<i64>, n: u32) {
        if let Some(day) = ts.and_then(local_day) {
            *daily.entry(day).or_insert(0) += n;
        }
    }

    // lossy_lines 而非 lines()：坏字节不能截断后面的内容，理由见其文档
    for line in lossy_lines(reader) {
        if line.trim().is_empty() {
            continue;
        }
        // 单行解析失败就跳过：文件尾部可能是半行（agent 正在写入）
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        let ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_iso_ms);
        if let Some(ts) = ts {
            started_at = Some(started_at.map_or(ts, |c: i64| c.min(ts)));
            ended_at = Some(ended_at.map_or(ts, |c: i64| c.max(ts)));
        }
        if git_branch.is_none() {
            if let Some(b) = v.get("gitBranch").and_then(Value::as_str) {
                if !b.is_empty() {
                    git_branch = Some(b.to_string());
                }
            }
        }
        if cwd.is_none() {
            if let Some(c) = v.get("cwd").and_then(Value::as_str) {
                cwd = Some(c.to_string());
            }
        }
        // 会话关联的计划文件名（~/.claude/plans/<slug>.md）
        if slug.is_none() {
            if let Some(s) = v.get("slug").and_then(Value::as_str) {
                if !s.is_empty() {
                    slug = Some(s.to_string());
                }
            }
        }

        match v.get("type").and_then(Value::as_str) {
            Some("ai-title") => {
                if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
                    ai_title = Some(t.to_string());
                }
            }
            Some("user") => {
                // 工具结果也是 type=user，靠 origin.kind 区分真人输入
                let is_human =
                    v.pointer("/origin/kind").and_then(Value::as_str) == Some("human");
                if is_human {
                    user_turns += 1;
                    bump(&mut daily, ts, 1);
                    if first_prompt.is_none() {
                        if let Some(text) = v.pointer("/message/content").and_then(Value::as_str) {
                            first_prompt = Some(text.to_string());
                        }
                    }
                }
            }
            Some("assistant") => {
                assistant_turns += 1;
                if let Some(m) = v.pointer("/message/model").and_then(Value::as_str) {
                    models.insert(m.to_string());
                }
                bump(&mut daily, ts, 1);
                if let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) {
                    let n = blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                        .count();
                    tool_calls += n;
                    bump(&mut daily, ts, n as u32);

                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) != Some("tool_use") {
                            continue;
                        }
                        if let Some(p) = plan_from_tool(b, ts) {
                            plans.push(p);
                        }
                    }
                }
            }
            Some("file-history-delta") => {
                if let Some(p) = v.get("trackingPath").and_then(Value::as_str) {
                    let e = files.entry(p.to_string()).or_default();
                    e.deltas += 1;
                    e.bump_at(
                        v.get("timestamp").and_then(Value::as_str).and_then(parse_iso_ms),
                    );
                }
            }
            Some("file-history-snapshot") => {
                if let Some(map) =
                    v.pointer("/snapshot/trackedFileBackups").and_then(Value::as_object)
                {
                    for (k, meta) in map {
                        let e = files.entry(k.clone()).or_default();
                        let ver = meta.get("version").and_then(Value::as_u64).unwrap_or(1) as usize;
                        e.max_version = e.max_version.max(ver);
                        e.bump_at(
                            meta.get("backupTime").and_then(Value::as_str).and_then(parse_iso_ms),
                        );
                    }
                }
            }
            Some("cost-state") => {
                cost_usd = v.get("totalCostUSD").and_then(Value::as_f64).or(cost_usd);
                lines_added =
                    v.get("totalLinesAdded").and_then(Value::as_i64).unwrap_or(lines_added);
                lines_removed =
                    v.get("totalLinesRemoved").and_then(Value::as_i64).unwrap_or(lines_removed);
            }
            _ => {}
        }
    }

    let title = ai_title
        .or_else(|| first_prompt.as_deref().map(make_title))
        .unwrap_or_else(|| id.chars().take(8).collect());

    let project_path = normalize_drive(&cwd.unwrap_or_else(|| decode_project_dir(project_id)));

    let touches: Vec<FileTouch> = files
        .into_iter()
        .map(|(path, acc)| FileTouch { path, touches: acc.touches(), at: acc.last_at })
        .collect();

    // 计划文件是会话之外的独立产物：会话里没有 ExitPlanMode 记录时它仍可能存在，
    // 但有记录时两者内容通常完全相同（ExitPlanMode 就是把这个文件提交上去的）。
    // 按正文去重 —— 不能按 slug 比对 source，ExitPlanMode 的 source 里没有 slug。
    if let Some(slug) = slug {
        if let Some(p) = plan_from_file(&slug, ended_at) {
            let dup = plans
                .iter()
                .any(|e| e.body.as_deref().map(str::trim) == p.body.as_deref().map(str::trim));
            if !dup {
                plans.push(p);
            }
        }
    }

    let summary = SessionSummary {
        agent: AgentKind::Claude,
        project_id: project_id.to_string(),
        project_path,
        id,
        title,
        started_at,
        ended_at,
        wall_ms: match (started_at, ended_at) {
            (Some(s), Some(e)) => (e - s).max(0),
            _ => 0,
        },
        user_turns,
        assistant_turns,
        tool_calls,
        models: models.into_iter().collect(),
        git_branch,
        files_touched: touches.len(),
        lines_added,
        lines_removed,
        cost_usd,
        // 占位：running 是 now - mtime 的函数，进缓存就会被永久固化成解析当时的
        // 值。真正的取值在 `cached_collect` 里按当前 mtime 现算。
        running: false,
        bytes,
    };

    // 同一份 TodoWrite 清单每刷新一次就落一条记录，折叠成最后一条 + 演进次数
    let plans = fold_plan_snapshots(plans);

    // 存剥掉注入上下文后的真人输入，与 WorkBuddy 保持一致：否则搜索会命中
    // `<system-reminder>` 里的注入内容，摘要显示成 `OS Version: win32` 之类
    let searchable = first_prompt.as_deref().map(strip_injected);

    Some(ParsedSession { summary, files: touches, daily, first_prompt: searchable, plans })
}
