//! Claude Code 适配器。
//!
//! 布局: ~/.claude/projects/<cwd编码>/<sessionId>.jsonl
//! 每行一条事件，关键类型:
//!   user / assistant         对话轮，assistant.message.content[] 里的 tool_use 即工具调用
//!   ai-title                 Claude 自己生成的会话标题，直接拿来当卡片标题
//!   cost-state               总花费、增删行数、各模型 token 用量
//!   file-history-snapshot    file-history-delta  本次会话触达过的文件
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{make_title, now_ms, parse_iso_ms, AgentAdapter, RUNNING_WINDOW_MS};
use crate::model::{AgentKind, ProjectSummary, SessionSummary};
use crate::paths::{
    agent_root, decode_project_dir, normalize_drive, open_readonly, project_display_name,
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

    for line in reader.lines().map_while(Result::ok).take(60) {
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

    fn list_sessions(&self, project_id: &str) -> Vec<SessionSummary> {
        let Some(root) = projects_dir() else {
            return Vec::new();
        };
        let dir = root.join(project_id);
        let Ok(files) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };

        let mut out: Vec<SessionSummary> = files
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .filter_map(|p| parse_session(&p, project_id))
            .collect();

        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        out
    }
}

fn parse_session(path: &Path, project_id: &str) -> Option<SessionSummary> {
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
    let mut files: BTreeSet<String> = BTreeSet::new();
    let mut git_branch: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut cost_usd: Option<f64> = None;
    let mut lines_added = 0i64;
    let mut lines_removed = 0i64;

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        // 单行解析失败就跳过：文件尾部可能是半行（agent 正在写入）
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if let Some(ts) = v.get("timestamp").and_then(Value::as_str).and_then(parse_iso_ms) {
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
                if let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) {
                    tool_calls += blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                        .count();
                }
            }
            Some("file-history-delta") => {
                if let Some(p) = v.get("trackingPath").and_then(Value::as_str) {
                    files.insert(p.to_string());
                }
            }
            Some("file-history-snapshot") => {
                if let Some(map) =
                    v.pointer("/snapshot/trackedFileBackups").and_then(Value::as_object)
                {
                    for k in map.keys() {
                        files.insert(k.clone());
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

    let last_write = mtime_ms(path).unwrap_or(0);
    let project_path = normalize_drive(&cwd.unwrap_or_else(|| decode_project_dir(project_id)));

    Some(SessionSummary {
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
        files_touched: files.len(),
        lines_added,
        lines_removed,
        cost_usd,
        running: now_ms() - last_write < RUNNING_WINDOW_MS,
        bytes,
    })
}
