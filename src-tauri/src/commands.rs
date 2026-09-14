//! 暴露给前端的命令。全部只读，没有任何一个命令能写 agent 目录。
use crate::adapters::adapter_for;
use crate::model::{AgentKind, AgentSummary, ProjectOutcome, ProjectSummary, SessionSummary};
use crate::paths::agent_root;

/// 左侧 agent 栏。未安装的 agent 也返回（installed=false），前端置灰而不是消失，
/// 这样用户知道工作台支持它、只是本机没装。
#[tauri::command]
pub fn list_agents() -> Vec<AgentSummary> {
    AgentKind::all()
        .into_iter()
        .map(|kind| {
            let root = agent_root(kind);
            let installed = root.as_ref().map(|p| p.exists()).unwrap_or(false);
            let projects = if installed {
                adapter_for(kind).list_projects()
            } else {
                Vec::new()
            };
            AgentSummary {
                kind,
                id: kind.id().to_string(),
                display_name: kind.display_name().to_string(),
                installed,
                root: root.map(|p| p.display().to_string()).unwrap_or_default(),
                project_count: projects.len(),
                session_count: projects.iter().map(|p| p.session_count).sum(),
                last_active: projects.iter().filter_map(|p| p.last_active).max(),
            }
        })
        .collect()
}

/// 某个 agent 下的项目列表。需求 6：不跨 agent 合并，同一 cwd 重复出现是预期行为。
#[tauri::command]
pub fn list_projects(agent: String) -> Result<Vec<ProjectSummary>, String> {
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).list_projects())
}

/// 某个项目下的会话列表。这一步才真正解析 JSONL。
#[tauri::command]
pub fn list_sessions(agent: String, project_id: String) -> Result<Vec<SessionSummary>, String> {
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).list_sessions(&project_id))
}

/// 项目成果盘点：累计改了哪些文件、哪几场会话改动最大。
#[tauri::command]
pub fn project_outcome(agent: String, project_id: String) -> Result<ProjectOutcome, String> {
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).project_outcome(&project_id))
}
