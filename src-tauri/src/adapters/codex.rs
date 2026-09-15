//! Codex 适配器。
//!
//! 布局与另外两家不同：不按 cwd 分目录，而是按日期分目录。
//!   ~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<id>.jsonl
//!   ~/.codex/archived_sessions/rollout-<ts>-<id>.jsonl
//!   ~/.codex/session_index.jsonl        {id, thread_name, updated_at}
//!
//! 记录形如 {timestamp, type, payload}，关键类型：
//!   session_meta      payload.cwd 是项目归属的唯一来源
//!   turn_context      turn_id / cwd / workspace_roots
//!   response_item     payload.type = message | reasoning | function_call | function_call_output
//!   event_msg         payload.type = task_started | user_message | agent_message | turn_aborted
//!
//! 因为项目归属只能从文件内容反查，`list_projects` 无法像另外两家那样纯 stat，
//! 必须 peek 每个 rollout 的头部。Codex 会话数量通常不大，暂时可以接受；
//! P3 上 SQLite 索引后这一步会变成增量。
use std::collections::{BTreeMap, BTreeSet};

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{
    cached_collect, local_day, make_title, now_ms, parse_iso_ms, AgentAdapter, RUNNING_WINDOW_MS,
};
use crate::model::{
    AgentKind, ParsedSession, PlanEntry, ProjectSummary, SessionSummary, TodoItem,
};
use crate::paths::{agent_root, normalize_drive, open_readonly, project_display_name};

use super::claude::mtime_ms;

pub struct CodexAdapter;

/// 收集所有 rollout 文件：日期分层目录 + 归档目录。
/// 日期层级深度固定为 YYYY/MM/DD，所以写死三层递归，不引 walkdir 依赖。
fn collect_rollouts() -> Vec<PathBuf> {
    let Some(root) = agent_root(AgentKind::Codex) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    let push_jsonl = |dir: &Path, out: &mut Vec<PathBuf>| {
        if let Ok(files) = std::fs::read_dir(dir) {
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    out.push(p);
                }
            }
        }
    };

    // sessions/YYYY/MM/DD/*.jsonl
    if let Ok(years) = std::fs::read_dir(root.join("sessions")) {
        for y in years.flatten().filter(|e| e.path().is_dir()) {
            if let Ok(months) = std::fs::read_dir(y.path()) {
                for m in months.flatten().filter(|e| e.path().is_dir()) {
                    if let Ok(days) = std::fs::read_dir(m.path()) {
                        for d in days.flatten().filter(|e| e.path().is_dir()) {
                            push_jsonl(&d.path(), &mut out);
                        }
                    }
                }
            }
        }
    }

    // 归档会话是平铺的
    push_jsonl(&root.join("archived_sessions"), &mut out);
    out
}

/// session_meta 固定在文件首行，只读头部即可拿到 cwd。
fn peek_cwd(path: &Path) -> Option<String> {
    let file = open_readonly(path).ok()?;
    let reader = BufReader::with_capacity(16 * 1024, file);
    for line in reader.lines().map_while(Result::ok).take(10) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(c) = v.pointer("/payload/cwd").and_then(Value::as_str) {
                // 在这里就归一化，保证分组 key 一致
                return Some(normalize_drive(c));
            }
        }
    }
    None
}

/// session_index.jsonl 里存了 Codex 自己生成的会话名，拿来当标题。
fn thread_names() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(root) = agent_root(AgentKind::Codex) else {
        return map;
    };
    let Ok(file) = open_readonly(&root.join("session_index.jsonl")) else {
        return map;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let (Some(id), Some(name)) = (
            v.get("id").and_then(Value::as_str),
            v.get("thread_name").and_then(Value::as_str),
        ) {
            if !name.is_empty() {
                map.insert(id.to_string(), name.to_string());
            }
        }
    }
    map
}

impl AgentAdapter for CodexAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn list_projects(&self) -> Vec<ProjectSummary> {
        let now = now_ms();
        // cwd -> (会话数, 最后活跃)
        let mut grouped: BTreeMap<String, (usize, Option<i64>)> = BTreeMap::new();

        for path in collect_rollouts() {
            let Some(cwd) = peek_cwd(&path) else {
                continue;
            };
            let m = mtime_ms(&path);
            let slot = grouped.entry(cwd).or_insert((0, None));
            slot.0 += 1;
            if let Some(m) = m {
                slot.1 = Some(slot.1.map_or(m, |c: i64| c.max(m)));
            }
        }

        let mut out: Vec<ProjectSummary> = grouped
            .into_iter()
            .map(|(cwd, (count, last_active))| ProjectSummary {
                agent: AgentKind::Codex,
                // Codex 没有目录名可用，直接拿 cwd 当 key
                id: cwd.clone(),
                name: project_display_name(&cwd),
                path: cwd,
                session_count: count,
                last_active,
                first_active: None,
                total_wall_ms: 0,
                files_touched: 0,
                git_branch: None,
                running: last_active.map_or(false, |m| now - m < RUNNING_WINDOW_MS),
            })
            .collect();

        out.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        out
    }

    fn parse_project(&self, project_id: &str) -> Vec<ParsedSession> {
        let names = thread_names();
        let paths: Vec<_> = collect_rollouts()
            .into_iter()
            .filter(|p| peek_cwd(p).as_deref() == Some(project_id))
            .collect();

        cached_collect(paths, |p| parse_session(p, project_id, &names))
    }
}

fn parse_session(
    path: &Path,
    project_id: &str,
    names: &BTreeMap<String, String>,
) -> Option<ParsedSession> {
    let file = open_readonly(path).ok()?;
    let bytes = file.metadata().ok().map(|m| m.len()).unwrap_or(0);
    let reader = BufReader::with_capacity(256 * 1024, file);

    let mut id = String::new();
    let mut first_prompt: Option<String> = None;
    let mut started_at: Option<i64> = None;
    let mut ended_at: Option<i64> = None;
    let mut user_turns = 0usize;
    let mut assistant_turns = 0usize;
    let mut tool_calls = 0usize;
    let mut models: BTreeSet<String> = BTreeSet::new();
    let mut daily: BTreeMap<String, u32> = BTreeMap::new();
    let mut plans: Vec<PlanEntry> = Vec::new();

    fn bump(daily: &mut BTreeMap<String, u32>, ts: Option<i64>) {
        if let Some(day) = ts.and_then(local_day) {
            *daily.entry(day).or_insert(0) += 1;
        }
    }

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        let ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_iso_ms);
        if let Some(ts) = ts {
            started_at = Some(started_at.map_or(ts, |c: i64| c.min(ts)));
            ended_at = Some(ended_at.map_or(ts, |c: i64| c.max(ts)));
        }

        match v.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(s) = v.pointer("/payload/session_id").and_then(Value::as_str) {
                    id = s.to_string();
                }
                // 注意不要把 model_provider（"openai"）塞进 models，
                // 它是厂商名不是模型名，混进去卡片上会显示成 [gpt-5.6-sol, openai]
            }
            Some("turn_context") => {
                if let Some(m) = v.pointer("/payload/model").and_then(Value::as_str) {
                    models.insert(m.to_string());
                }
            }
            Some("response_item") => {
                if v.pointer("/payload/type").and_then(Value::as_str) == Some("function_call") {
                    tool_calls += 1;
                    bump(&mut daily, ts);
                    if let Some(p) = plan_from_call(v.get("payload").unwrap_or(&Value::Null), ts) {
                        plans.push(p);
                    }
                }
            }
            Some("event_msg") => match v.pointer("/payload/type").and_then(Value::as_str) {
                // 真人输入只认 user_message 事件：response_item 里 role=user 的记录
                // 大量是注入的插件说明和权限提示，算进去轮数会虚高
                Some("user_message") => {
                    user_turns += 1;
                    bump(&mut daily, ts);
                    if first_prompt.is_none() {
                        first_prompt = v
                            .pointer("/payload/message")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                    }
                }
                Some("agent_message") => {
                    assistant_turns += 1;
                    bump(&mut daily, ts);
                }
                _ => {}
            },
            _ => {}
        }
    }

    if id.is_empty() {
        id = path.file_stem()?.to_string_lossy().to_string();
    }

    let title = names
        .get(&id)
        .cloned()
        .or_else(|| first_prompt.as_deref().map(make_title))
        .unwrap_or_else(|| id.chars().take(8).collect());

    let last_write = mtime_ms(path).unwrap_or(0);

    let summary = SessionSummary {
        agent: AgentKind::Codex,
        project_id: project_id.to_string(),
        project_path: project_id.to_string(),
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
        // rollout 里不记 git 分支，也不做文件历史追踪
        git_branch: None,
        files_touched: 0,
        lines_added: 0,
        lines_removed: 0,
        cost_usd: None,
        running: now_ms() - last_write < RUNNING_WINDOW_MS,
        bytes,
    };

    // Codex 不做文件历史追踪，成果盘点的文件清单对它永远是空的
    Some(ParsedSession { summary, files: Vec::new(), daily, first_prompt, plans })
}

/// 从 function_call 里提取 Codex 的计划（`update_plan` 工具）。
///
/// ⚠️ 本机的 Codex 会话里没有出现过这个工具，下面的字段形状是按 Codex 的
/// 常见约定写的、**未经真实数据验证**。所以解析上刻意做得宽容：`plan` 数组
/// 里的元素既接受 `{step, status}` 对象，也接受纯字符串；任何一环对不上就
/// 安静返回 None，绝不让没见过的形状把整场会话的解析带崩。
fn plan_from_call(payload: &Value, at: Option<i64>) -> Option<PlanEntry> {
    if payload.get("name").and_then(Value::as_str) != Some("update_plan") {
        return None;
    }
    // arguments 是被转义成字符串的 JSON
    let raw = payload.get("arguments").and_then(Value::as_str)?;
    let args: Value = serde_json::from_str(raw).ok()?;

    let steps = args.get("plan").and_then(Value::as_array)?;
    let items: Vec<TodoItem> = steps
        .iter()
        .filter_map(|s| {
            let text = s
                .as_str()
                .or_else(|| s.get("step").and_then(Value::as_str))
                .or_else(|| s.get("content").and_then(Value::as_str))?
                .trim();
            (!text.is_empty()).then(|| TodoItem {
                text: text.to_string(),
                status: s
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
        title: args
            .get("explanation")
            .and_then(Value::as_str)
            .map(|s| s.chars().take(50).collect())
            .unwrap_or_else(|| "任务计划".into()),
        body: None,
        items,
        source: "update_plan".into(),
    })
}
