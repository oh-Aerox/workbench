//! 暴露给前端的命令。全部只读，没有任何一个命令能写 agent 目录。
use crate::adapters::adapter_for;
use crate::model::{
    AgentKind, AgentSummary, DayActivity, PlanEntry, PlanRecord, ProjectOutcome, ProjectSummary,
    RepoTodoReport, SearchHit, SessionSummary,
};
use crate::paths::agent_root;
use crate::repo;

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

/// 扫描项目工作目录里的 TODO 标记和待办文档。
///
/// **这是唯一会读 agent 数据目录之外文件的命令**，所以加了白名单：目标必须
/// 确实出现在某个 agent 的项目列表里。前端传来的路径不可信——没有这道校验，
/// 任何能调到这个命令的地方都能让应用去遍历机器上的任意目录。
/// 大项目首次扫描可能要几十秒（本机 eshop 7162 个文件耗时 85 秒，绝大部分
/// 时间花在逐个读文件上），所以必须丢到阻塞线程池，不能占着 UI 线程；
/// 前端那边也做成按钮触发而非切项目就自动扫。
#[tauri::command]
pub async fn project_todos(project_path: String) -> Result<RepoTodoReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure_known_project(&project_path)?;
        Ok(repo::scan_project(std::path::Path::new(&project_path)))
    })
    .await
    .map_err(|e| format!("扫描任务失败: {e}"))?
}

/// 白名单校验：目标必须确实是某个 agent 记录过的项目目录。
fn ensure_known_project(project_path: &str) -> Result<(), String> {
    let target = project_path.to_lowercase();
    let known = AgentKind::all().into_iter().any(|kind| {
        agent_root(kind).map(|p| p.exists()) == Some(true)
            && adapter_for(kind)
                .list_projects()
                .iter()
                .any(|p| p.path.to_lowercase() == target)
    });
    if known {
        Ok(())
    } else {
        Err(format!("拒绝扫描：{project_path} 不在任何 agent 的项目列表里"))
    }
}

/// 项目里的计划文档（PLAN.md / TODO.md / ROADMAP.md 之类）。
///
/// 与 `project_todos` 分开：找文档只走两层目录、毫秒级，所以可以在切项目时
/// 自动加载；扫 TODO 标记要遍历整棵源码树、几十秒，必须按钮触发。
#[tauri::command]
pub fn project_docs(project_path: String) -> Result<Vec<PlanEntry>, String> {
    ensure_known_project(&project_path)?;
    Ok(repo::scan_docs(std::path::Path::new(&project_path)))
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
