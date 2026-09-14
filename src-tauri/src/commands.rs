//! 暴露给前端的命令。全部只读，没有任何一个命令能写 agent 目录。
use crate::adapters::adapter_for;
use crate::model::{
    AgentKind, AgentSummary, DayActivity, PlanRecord, ProjectOutcome, ProjectSummary, SearchHit,
    SessionSummary,
};
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

/// 该项目下提取到的计划 / 待办清单，按时间倒序。
#[tauri::command]
pub fn project_plans(agent: String, project_id: String) -> Result<Vec<PlanRecord>, String> {
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).project_plans(&project_id))
}

/// 该 agent 的按天活动量，供热力图。首次调用会全量解析，之后走缓存。
#[tauri::command]
pub fn agent_activity(agent: String) -> Result<Vec<DayActivity>, String> {
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).daily_activity())
}

/// 全局搜索：标题、首条 prompt、触达文件路径、项目路径。
///
/// 数据量在几十场会话量级，直接内存扫描就够，不值得为此引入全文索引依赖。
#[tauri::command]
pub fn search(query: String, limit: Option<usize>) -> Vec<SearchHit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let cap = limit.unwrap_or(80);
    let mut hits = Vec::new();

    for kind in AgentKind::all() {
        if agent_root(kind).map(|p| p.exists()) != Some(true) {
            continue;
        }
        let adapter = adapter_for(kind);
        for project in adapter.list_projects() {
            let project_hit = project.path.to_lowercase().contains(&needle);

            for ps in adapter.parse_project(&project.id) {
                let s = &ps.summary;

                // 每场会话只报一次，按 标题 > prompt > 文件 > 项目 的优先级
                let found = if s.title.to_lowercase().contains(&needle) {
                    Some(("title", s.title.clone()))
                } else if let Some(p) = ps
                    .first_prompt
                    .as_ref()
                    .filter(|p| p.to_lowercase().contains(&needle))
                {
                    Some(("prompt", snippet(p, &needle)))
                } else {
                    ps.files
                        .iter()
                        .find(|f| f.path.to_lowercase().contains(&needle))
                        .map(|f| ("file", f.path.clone()))
                        .or_else(|| project_hit.then(|| ("project", project.path.clone())))
                };

                if let Some((field, snippet)) = found {
                    hits.push(SearchHit {
                        agent: kind,
                        project_id: project.id.clone(),
                        project_path: project.path.clone(),
                        project_name: project.name.clone(),
                        session_id: s.id.clone(),
                        title: s.title.clone(),
                        started_at: s.started_at,
                        field: field.to_string(),
                        snippet,
                    });
                }
            }
        }
    }

    hits.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    hits.truncate(cap);
    hits
}

/// 围绕关键词裁一段上下文出来，前后各留约 40 字。
fn snippet(text: &str, needle: &str) -> String {
    let lower = text.to_lowercase();
    let Some(byte_pos) = lower.find(needle) else {
        return text.chars().take(100).collect();
    };
    // 按字符而非字节切，避免把中文截断成乱码
    let char_pos = text[..byte_pos].chars().count();
    let start = char_pos.saturating_sub(40);
    let take = needle.chars().count() + 80;

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(text.chars().skip(start).take(take));
    if char_pos + take < text.chars().count() {
        out.push('…');
    }
    out
}
