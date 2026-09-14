//! Agent 适配层。每家 agent 一个实现，把各自的 JSONL 归一到 model.rs 的结构。
//!
//! 分工约定：
//!   - `list_projects` 只做目录枚举和 stat，不解析文件内容，保证首屏秒开；
//!   - `list_sessions` 才真正逐行解析该项目下的会话，按需付费。
pub mod claude;
pub mod codex;
pub mod workbuddy;

use std::collections::BTreeMap;

use crate::model::{
    AgentKind, FileOutcome, ParsedSession, ProjectOutcome, ProjectSummary, SessionSummary,
};

pub trait AgentAdapter {
    fn kind(&self) -> AgentKind;
    fn list_projects(&self) -> Vec<ProjectSummary>;

    /// 解析该项目下所有会话，含逐文件明细。
    /// 各家 adapter 只需实现这一个，下面两个视图都从它派生。
    fn parse_project(&self, project_id: &str) -> Vec<ParsedSession>;

    /// 按时间倒序的会话列表。
    fn list_sessions(&self, project_id: &str) -> Vec<SessionSummary> {
        let mut out: Vec<SessionSummary> =
            self.parse_project(project_id).into_iter().map(|p| p.summary).collect();
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        out
    }

    /// 项目级成果盘点：累计改了哪些文件、哪几场会话改动最大。
    fn project_outcome(&self, project_id: &str) -> ProjectOutcome {
        aggregate(self.kind(), project_id, self.parse_project(project_id))
    }
}

/// 把若干场会话聚合成项目成果。
fn aggregate(kind: AgentKind, project_id: &str, parsed: Vec<ParsedSession>) -> ProjectOutcome {
    let project_path = parsed
        .first()
        .map(|p| p.summary.project_path.clone())
        .unwrap_or_else(|| project_id.to_string());

    // path -> (改动次数, 涉及的会话 id 集合, 最后改动时间)
    let mut acc: BTreeMap<String, (usize, std::collections::BTreeSet<String>, Option<i64>)> =
        BTreeMap::new();

    for ps in &parsed {
        for t in &ps.files {
            let e = acc.entry(t.path.clone()).or_insert((0, Default::default(), None));
            e.0 += t.touches;
            e.1.insert(ps.summary.id.clone());
            if let Some(at) = t.at {
                e.2 = Some(e.2.map_or(at, |c: i64| c.max(at)));
            }
        }
    }

    let mut files: Vec<FileOutcome> = acc
        .into_iter()
        .map(|(path, (touches, sessions, last_touched))| {
            // Claude / WorkBuddy 的 trackedFileBackups 键对项目内文件用的是
            // **相对路径**，只有项目外的文件（如 ~/.claude/plans 下的计划）才是
            // 绝对路径。所以先判断是不是绝对路径，再决定怎么算归属。
            let (rel, outside) = if is_absolute(&path) {
                (relative_to(&path, &project_path), !starts_with_dir(&path, &project_path))
            } else {
                (path.clone(), false)
            };
            FileOutcome { rel, path, touches, sessions: sessions.len(), last_touched, outside }
        })
        .collect();

    // 改动次数相同的，晚改的排前面
    files.sort_by(|a, b| b.touches.cmp(&a.touches).then(b.last_touched.cmp(&a.last_touched)));

    let mut top_sessions: Vec<SessionSummary> = parsed.into_iter().map(|p| p.summary).collect();
    top_sessions.sort_by_key(|s| std::cmp::Reverse(s.lines_added + s.lines_removed));

    let total_cost: f64 = top_sessions.iter().filter_map(|s| s.cost_usd).sum();
    let has_cost = top_sessions.iter().any(|s| s.cost_usd.is_some());

    ProjectOutcome {
        agent: kind,
        project_id: project_id.to_string(),
        session_count: top_sessions.len(),
        total_wall_ms: top_sessions.iter().map(|s| s.wall_ms).sum(),
        total_tool_calls: top_sessions.iter().map(|s| s.tool_calls).sum(),
        total_lines_added: top_sessions.iter().map(|s| s.lines_added).sum(),
        total_lines_removed: top_sessions.iter().map(|s| s.lines_removed).sum(),
        total_cost_usd: has_cost.then_some(total_cost),
        first_active: top_sessions.iter().filter_map(|s| s.started_at).min(),
        last_active: top_sessions.iter().filter_map(|s| s.ended_at).max(),
        files,
        top_sessions,
        project_path,
    }
}

/// 是不是绝对路径。覆盖 `C:\…`、`/…`、UNC `\\server\…` 三种形态。
fn is_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    match (b.first(), b.get(1)) {
        (Some(c), Some(b':')) if c.is_ascii_alphabetic() => true,
        (Some(b'/'), _) => true,
        (Some(b'\\'), Some(b'\\')) => true,
        _ => false,
    }
}

/// 大小写不敏感的前缀判断。Windows 路径大小写不敏感，而 agent 记录里
/// 同一目录的大小写可能不一致，用 `str::starts_with` 会漏判。
fn starts_with_dir(path: &str, dir: &str) -> bool {
    let (p, d) = (path.to_lowercase(), dir.to_lowercase());
    let d = d.trim_end_matches(['/', '\\']);
    p.len() > d.len()
        && p.starts_with(d)
        && matches!(p.as_bytes().get(d.len()), Some(b'/') | Some(b'\\'))
}

/// 转成相对项目根的路径；不在项目内的保持绝对路径。
fn relative_to(path: &str, dir: &str) -> String {
    if starts_with_dir(path, dir) {
        let d = dir.trim_end_matches(['/', '\\']);
        path[d.len() + 1..].to_string()
    } else {
        path.to_string()
    }
}

pub fn adapter_for(kind: AgentKind) -> Box<dyn AgentAdapter> {
    match kind {
        AgentKind::Claude => Box::new(claude::ClaudeAdapter),
        AgentKind::Codex => Box::new(codex::CodexAdapter),
        AgentKind::WorkBuddy => Box::new(workbuddy::WorkBuddyAdapter),
    }
}

/// 最近多久内有写入就算「进行中」。
pub const RUNNING_WINDOW_MS: i64 = 90_000;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// ISO8601 → epoch 毫秒。解析失败返回 None，不让一条坏记录污染整个会话。
pub fn parse_iso_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

/// 取出 `<tag>…</tag>` 里的内容。
fn extract_tag(raw: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = raw.find(&open)? + open.len();
    let end = raw[start..].find(&close)? + start;
    let inner = raw[start..end].trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

/// 剥掉注入的上下文块，取出用户真正打的那句话。
///
/// 三家 agent 都会把 `<system-reminder>`、`<user_info>`、`<identity_context>` 之类
/// 的内容拼进首条用户消息里，WorkBuddy 的注入块能到 8KB。分三档：
///
/// 1. WorkBuddy 把真人输入显式包在 `<user_query>` 里，直接取出来最准；
/// 2. 否则取最后一个闭合标签之后的文本 —— 注入块通常在前，真人输入在后；
/// 3. 都不成立才退化为「滤掉尖括号开头的行」。
///
/// 注意第 2 档对 WorkBuddy 是失效的：它的 `</user_query>` 就在末尾，后面没有内容，
/// 落到第 3 档会把 `OS Version: win32` 这类注入元数据当成标题。所以第 1 档必须在前。
pub fn strip_injected(raw: &str) -> String {
    if let Some(q) = extract_tag(raw, "user_query") {
        return q;
    }
    if let Some(idx) = raw.rfind("</") {
        if let Some(end) = raw[idx..].find('>') {
            let tail = raw[idx + end + 1..].trim();
            if !tail.is_empty() {
                return tail.to_string();
            }
        }
    }
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('<'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 把长 prompt 压成一行标题。
pub fn make_title(raw: &str) -> String {
    let cleaned = strip_injected(raw);
    let src = if cleaned.trim().is_empty() { raw.trim() } else { cleaned.trim() };
    let flat = src.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = flat.chars().take(60).collect();
    if flat.chars().count() > 60 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 标题剥离注入上下文() {
        let raw = "<system-reminder data-role=\"user-context\">\n<user_info>\nOS: win32\n</user_info>\n</system-reminder>\n帮我看下 docs 目录";
        assert_eq!(make_title(raw), "帮我看下 docs 目录");
    }

    #[test]
    fn 标题优先取_user_query_包裹的真人输入() {
        // WorkBuddy 的真实形状：8KB 注入块在前，真人输入包在末尾的 user_query 里。
        // 这时「取最后一个闭合标签之后」会落空，必须靠 user_query 分支兜住。
        let raw = "<system-reminder>\n<user_info>\nOS Version: win32\nShell: bash\n</user_info>\n</system-reminder>\n<user_query>看下 docs 的进度</user_query>";
        assert_eq!(make_title(raw), "看下 docs 的进度");
    }

    #[test]
    fn 标题压成单行并截断() {
        let raw = "第一行\n\n第二行";
        assert_eq!(make_title(raw), "第一行 第二行");
        let long = "啊".repeat(80);
        let t = make_title(&long);
        assert_eq!(t.chars().count(), 61, "60 字 + 省略号");
        assert!(t.ends_with('…'));
    }

    #[test]
    fn 相对路径_大小写不敏感() {
        // agent 记录里同一目录的盘符/大小写可能不一致，不能用朴素前缀比对
        let dir = "C:\\Users\\a\\proj";
        assert!(starts_with_dir("c:\\users\\a\\proj\\src\\main.rs", dir));
        assert_eq!(relative_to("C:\\Users\\a\\proj\\src\\main.rs", dir), "src\\main.rs");
    }

    #[test]
    fn 项目外的文件保持绝对路径() {
        let dir = "C:\\Users\\a\\proj";
        // 计划文件落在 ~/.claude/plans，不属于项目，要能识别出来
        let plan = "C:\\Users\\a\\.claude\\plans\\x.md";
        assert!(!starts_with_dir(plan, dir));
        assert_eq!(relative_to(plan, dir), plan);
    }

    #[test]
    fn 绝对路径判定() {
        assert!(is_absolute("C:\\Users\\a\\x.rs"));
        assert!(is_absolute("/Users/a/x.rs"));
        assert!(is_absolute("\\\\server\\share\\x.rs"));
        // Claude 对项目内文件记的就是这种相对路径，不能当成绝对路径
        assert!(!is_absolute("src\\main.rs"));
        assert!(!is_absolute("README.md"));
    }

    #[test]
    fn 同名前缀目录不算子路径() {
        // proj-backup 不是 proj 的子目录，少了分隔符检查就会误判
        assert!(!starts_with_dir("C:\\Users\\a\\proj-backup\\x.rs", "C:\\Users\\a\\proj"));
    }

    #[test]
    fn 全是注入块时退化为原文() {
        // 没有真人输入时不能返回空串，否则卡片没标题
        assert!(!make_title("<system-reminder>只有注入</system-reminder>").is_empty());
    }
}
