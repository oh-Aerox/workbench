//! WorkBuddy 适配器。
//!
//! 布局跟 Claude 同形，schema 不同：
//!   ~/.workbuddy/projects/<cwd编码>/<sessionId>.jsonl
//!   ~/.workbuddy/sessions/<pid>.json     活跃进程心跳
//!
//! 事件类型：
//!   message(role=user)            content[].type = input_text
//!   message(role=assistant)       content[].type = output_text
//!   reasoning                     思考过程，不计入轮数
//!   function_call                 工具调用
//!   function_call_result          工具结果，带 name 字段
//!   file-history-snapshot         触达文件 + cwd
//!
//! 与 Claude 的三处差异：时间戳是 epoch 毫秒（非 ISO 字符串）；模型名在
//! `providerData.model`；目录编码首字母小写（`c--Users-...`）。
//! 另外 WorkBuddy 不写 cost-state，所以没有花费和增删行数。
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{make_title, now_ms, AgentAdapter, RUNNING_WINDOW_MS};
use crate::model::{AgentKind, FileTouch, ParsedSession, ProjectSummary, SessionSummary};
use crate::paths::{
    agent_root, decode_project_dir, normalize_drive, open_readonly, project_display_name,
};

use super::claude::mtime_ms;

pub struct WorkBuddyAdapter;

fn projects_dir() -> Option<PathBuf> {
    agent_root(AgentKind::WorkBuddy).map(|r| r.join("projects"))
}

/// 从会话头部读真实 cwd。理由同 Claude：目录名编码有损。
/// WorkBuddy 的 cwd 挂在 file-history-snapshot 记录上，位置比 Claude 靠后，
/// 所以多读一些行。
fn peek_cwd(path: &Path) -> Option<String> {
    let file = open_readonly(path).ok()?;
    let reader = BufReader::with_capacity(64 * 1024, file);
    for line in reader.lines().map_while(Result::ok).take(80) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(c) = v.get("cwd").and_then(Value::as_str) {
            if !c.is_empty() {
                return Some(c.to_string());
            }
        }
    }
    None
}

impl AgentAdapter for WorkBuddyAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::WorkBuddy
    }

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

            let path = normalize_drive(
                &newest
                    .as_ref()
                    .and_then(|(_, p)| peek_cwd(p))
                    .unwrap_or_else(|| decode_project_dir(&dir_name)),
            );

            out.push(ProjectSummary {
                agent: AgentKind::WorkBuddy,
                id: dir_name,
                name: project_display_name(&path),
                path,
                session_count,
                last_active,
                first_active: None,
                total_wall_ms: 0,
                files_touched: 0,
                git_branch: None,
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

        files
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .filter_map(|p| parse_session(&p, project_id))
            .collect()
    }
}

/// 取 content 数组里第一段文本。user 用 input_text，assistant 用 output_text。
fn first_text(v: &Value, want: &str) -> Option<String> {
    v.get("content")?
        .as_array()?
        .iter()
        .find(|b| b.get("type").and_then(Value::as_str) == Some(want))
        .and_then(|b| b.get("text").and_then(Value::as_str))
        .map(str::to_string)
}

fn parse_session(path: &Path, project_id: &str) -> Option<ParsedSession> {
    let file = open_readonly(path).ok()?;
    let bytes = file.metadata().ok().map(|m| m.len()).unwrap_or(0);
    let reader = BufReader::with_capacity(256 * 1024, file);

    let id = path.file_stem()?.to_string_lossy().to_string();
    let mut first_prompt: Option<String> = None;
    let mut started_at: Option<i64> = None;
    let mut ended_at: Option<i64> = None;
    let mut user_turns = 0usize;
    let mut assistant_turns = 0usize;
    let mut tool_calls = 0usize;
    let mut models: BTreeSet<String> = BTreeSet::new();
    // path -> (版本号, 最后改动时间)。WorkBuddy 只有全量快照、没有 delta 记录，
    // 所以改动次数只能取快照里的 version 最大值。
    let mut files: BTreeMap<String, (usize, Option<i64>)> = BTreeMap::new();
    let mut cwd: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        // WorkBuddy 的时间戳是 epoch 毫秒数字，不是 ISO 字符串
        if let Some(ts) = v.get("timestamp").and_then(Value::as_i64) {
            started_at = Some(started_at.map_or(ts, |c: i64| c.min(ts)));
            ended_at = Some(ended_at.map_or(ts, |c: i64| c.max(ts)));
        }
        if cwd.is_none() {
            if let Some(c) = v.get("cwd").and_then(Value::as_str) {
                cwd = Some(c.to_string());
            }
        }
        if let Some(m) = v.pointer("/providerData/model").and_then(Value::as_str) {
            models.insert(m.to_string());
        }

        match v.get("type").and_then(Value::as_str) {
            Some("message") => match v.get("role").and_then(Value::as_str) {
                Some("user") => {
                    user_turns += 1;
                    if first_prompt.is_none() {
                        first_prompt = first_text(&v, "input_text");
                    }
                }
                Some("assistant") => assistant_turns += 1,
                _ => {}
            },
            Some("function_call") => tool_calls += 1,
            Some("file-history-snapshot") => {
                if let Some(map) =
                    v.pointer("/snapshot/trackedFileBackups").and_then(Value::as_object)
                {
                    // 记录本身带的 epoch 毫秒时间戳，快照内没有 per-file 时间
                    let at = v.get("timestamp").and_then(Value::as_i64);
                    for (k, meta) in map {
                        let ver = meta.get("version").and_then(Value::as_u64).unwrap_or(1) as usize;
                        let e = files.entry(k.clone()).or_insert((0, None));
                        e.0 = e.0.max(ver);
                        if let Some(at) = at {
                            e.1 = Some(e.1.map_or(at, |c: i64| c.max(at)));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let title = first_prompt
        .as_deref()
        .map(make_title)
        .unwrap_or_else(|| id.chars().take(8).collect());

    let last_write = mtime_ms(path).unwrap_or(0);
    let project_path = normalize_drive(&cwd.unwrap_or_else(|| decode_project_dir(project_id)));

    let touches: Vec<FileTouch> = files
        .into_iter()
        .map(|(path, (ver, at))| FileTouch { path, touches: ver.max(1), at })
        .collect();

    let summary = SessionSummary {
        agent: AgentKind::WorkBuddy,
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
        // WorkBuddy 记录里没有 git 分支
        git_branch: None,
        files_touched: touches.len(),
        // WorkBuddy 不写 cost-state，这三项无数据
        lines_added: 0,
        lines_removed: 0,
        cost_usd: None,
        running: now_ms() - last_write < RUNNING_WINDOW_MS,
        bytes,
    };

    Some(ParsedSession { summary, files: touches })
}
