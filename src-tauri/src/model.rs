//! 跨 agent 的统一数据模型。三家 agent 的原始 JSONL schema 各不相同，
//! adapter 负责把它们归一到这里的结构，前端只认这一套。
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    WorkBuddy,
}

impl AgentKind {
    pub fn id(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::WorkBuddy => "workbuddy",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::WorkBuddy => "WorkBuddy",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(AgentKind::Claude),
            "codex" => Some(AgentKind::Codex),
            "workbuddy" => Some(AgentKind::WorkBuddy),
            _ => None,
        }
    }

    pub fn all() -> [AgentKind; 3] {
        [AgentKind::Claude, AgentKind::Codex, AgentKind::WorkBuddy]
    }
}

/// 左侧 agent 栏的一项。需求 6：一切先按 agent 划分。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub kind: AgentKind,
    pub id: String,
    pub display_name: String,
    /// 该 agent 的数据根目录是否存在。不存在时前端置灰，不报错。
    pub installed: bool,
    pub root: String,
    pub project_count: usize,
    pub session_count: usize,
    pub last_active: Option<i64>,
}

/// 一个 agent 下的一个工作目录。同一 cwd 在不同 agent 下会各出现一次，
/// 需求 6 明确允许这种重复，不做跨 agent 合并。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub agent: AgentKind,
    /// agent 数据目录下的原始目录名（或 codex 的 cwd 哈希），用作路由 key
    pub id: String,
    /// 真实工作目录绝对路径
    pub path: String,
    /// 取路径最后一段作显示名
    pub name: String,
    pub session_count: usize,
    pub last_active: Option<i64>,
    pub first_active: Option<i64>,
    pub total_wall_ms: i64,
    pub files_touched: usize,
    pub git_branch: Option<String>,
    /// 心跳判定：最近仍在写入 → 视为进行中
    pub running: bool,
}

/// 会话列表项。只读扫描得到，不含逐轮明细。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub agent: AgentKind,
    pub project_id: String,
    pub project_path: String,
    pub id: String,
    /// 优先用 agent 自己生成的标题，退化到首条用户 prompt 截断
    pub title: String,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub wall_ms: i64,
    pub user_turns: usize,
    pub assistant_turns: usize,
    pub tool_calls: usize,
    pub models: Vec<String>,
    pub git_branch: Option<String>,
    pub files_touched: usize,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub cost_usd: Option<f64>,
    pub running: bool,
    /// 源文件大小，用于前端提示大会话解析耗时
    pub bytes: u64,
}
