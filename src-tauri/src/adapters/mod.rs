//! Agent 适配层。每家 agent 一个实现，把各自的 JSONL 归一到 model.rs 的结构。
//!
//! 分工约定：
//!   - `list_projects` 只做目录枚举和 stat，不解析文件内容，保证首屏秒开；
//!   - `list_sessions` 才真正逐行解析该项目下的会话，按需付费。
pub mod claude;
pub mod codex;
pub mod workbuddy;

use crate::model::{AgentKind, ProjectSummary, SessionSummary};

pub trait AgentAdapter {
    fn kind(&self) -> AgentKind;
    fn list_projects(&self) -> Vec<ProjectSummary>;
    fn list_sessions(&self, project_id: &str) -> Vec<SessionSummary>;
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
    fn 全是注入块时退化为原文() {
        // 没有真人输入时不能返回空串，否则卡片没标题
        assert!(!make_title("<system-reminder>只有注入</system-reminder>").is_empty());
    }
}
