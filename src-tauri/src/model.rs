//! 跨 agent 的统一数据模型。三家 agent 的原始 JSONL schema 各不相同，
//! adapter 负责把它们归一到这里的结构，前端只认这一套。
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// 一场会话里某个文件的改动情况。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTouch {
    pub path: String,
    /// 本场会话里被改了几次
    pub touches: usize,
    /// 最后一次改动时间
    pub at: Option<i64>,
}

/// 解析一场会话的完整产出。`SessionSummary` 是给列表用的压缩视图，
/// `files` 保留逐文件明细供成果盘点聚合，`daily` 供热力图。
///
/// 整个结构会被写进磁盘缓存，所以必须可反序列化。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedSession {
    pub summary: SessionSummary,
    pub files: Vec<FileTouch>,
    /// 本地日期 `YYYY-MM-DD` -> 当天活动事件数（对话轮 + 工具调用）。
    /// 一场会话可能跨多天，所以必须在解析时按天拆开，不能只用起止时间。
    pub daily: BTreeMap<String, u32>,
    /// 首条真人 prompt 原文，供全局搜索。标题是截断过的，搜不全。
    pub first_prompt: Option<String>,
    /// 本场会话产出的计划 / 待办清单
    #[serde(default)]
    pub plans: Vec<PlanEntry>,
}

/// 待办清单里的一条。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub text: String,
    /// pending / in_progress / completed / unknown
    pub status: String,
}

/// 一份计划或待办清单，解析时从会话里提取，随 ParsedSession 一起进缓存。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub at: Option<i64>,
    /// plan（计划全文）或 todos（结构化清单）
    pub kind: String,
    /// 取计划首个标题行；没有就用来源名
    pub title: String,
    /// 计划 markdown 全文，todos 类型为 None
    pub body: Option<String>,
    /// 勾选项或结构化待办条目
    pub items: Vec<TodoItem>,
    /// 来源标记，如 ExitPlanMode / TodoWrite / plans/xxx.md / update_plan
    pub source: String,
    /// 这份清单在本场会话里被刷新过几次。
    ///
    /// `update_plan` / `TodoWrite` 记的是同一份清单的进度快照，每调一次落一条，
    /// 折叠后只留最后一条（见 `adapters::fold_plan_snapshots`）。这个计数保留
    /// 「被折叠掉多少份」的信息，前端在 >1 时显示成「演进 N 次」角标。
    #[serde(default = "one")]
    pub revisions: usize,
}

/// `revisions` 的默认值。老缓存里没有这个字段，反序列化时按「只有一份」算。
fn one() -> usize {
    1
}

/// 带上会话归属的计划，给前端用。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRecord {
    pub agent: AgentKind,
    pub session_id: String,
    pub session_title: String,
    #[serde(flatten)]
    pub entry: PlanEntry,
}

/// 项目源码里的一条 TODO 标记。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoTodo {
    /// 相对项目根的路径
    pub file: String,
    pub line: usize,
    /// TODO / FIXME / XXX / HACK
    pub marker: String,
    pub text: String,
}

/// 一次源码扫描的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoTodoReport {
    pub root: String,
    /// 项目目录是否还在（agent 记录里的项目可能已被删除或移动）
    pub exists: bool,
    pub todos: Vec<RepoTodo>,
    pub files_scanned: usize,
    /// 触到遍历或命中上限，结果不完整
    pub truncated: bool,
    pub elapsed_ms: u64,
}

/// 热力图的一格。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayActivity {
    /// 本地日期 `YYYY-MM-DD`
    pub day: String,
    pub events: u32,
    pub sessions: usize,
}

/// 全局搜索命中。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub agent: AgentKind,
    pub project_id: String,
    pub project_path: String,
    pub project_name: String,
    pub session_id: String,
    pub title: String,
    pub started_at: Option<i64>,
    /// 命中位置：title / prompt / file / project
    pub field: String,
    /// 命中处的上下文片段，已围绕关键词裁剪
    pub snippet: String,
}

/// 成果盘点里的一个文件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOutcome {
    pub path: String,
    /// 相对项目根的路径，列表里显示这个
    pub rel: String,
    /// 被记录改动的次数
    pub touches: usize,
    /// 涉及多少场会话——跨会话反复改的文件是这个项目的热点
    pub sessions: usize,
    pub last_touched: Option<i64>,
    /// 落在项目目录之外（如 ~/.claude/plans 下的计划文件），单独标记
    pub outside: bool,
}

/// 项目级成果盘点：这个项目被改成了什么样。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOutcome {
    pub agent: AgentKind,
    pub project_id: String,
    pub project_path: String,
    pub session_count: usize,
    pub total_wall_ms: i64,
    pub total_tool_calls: usize,
    pub total_lines_added: i64,
    pub total_lines_removed: i64,
    /// 三家里只有 Claude 记花费，其余为 None
    pub total_cost_usd: Option<f64>,
    pub first_active: Option<i64>,
    pub last_active: Option<i64>,
    /// 按改动次数降序
    pub files: Vec<FileOutcome>,
    /// 按增删行数降序，用于「哪几场会话改动最大」
    pub top_sessions: Vec<SessionSummary>,
}

/// 会话列表项。只读扫描得到，不含逐轮明细。
#[derive(Debug, Clone, Serialize, Deserialize)]
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
