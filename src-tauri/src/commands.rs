//! 暴露给前端的命令。全部只读，没有任何一个命令能写 agent 目录
//! （唯一的写操作是 `clear_cache` 删本应用自己的缓存文件）。
//!
//! **同步命令一律标 `#[tauri::command(async)]`**：Tauri v2 里不带这个标记的
//! 同步命令在主线程上执行，而 `search`（遍历三家 × 全部项目 × 全部会话）、
//! `agent_activity`（全量解析）、`project_docs`（要先跑白名单校验，而它会调
//! 三家的 `list_projects`）都是重 IO，占着 UI 线程会让搜索和切 agent 期间
//! 整个界面冻结且无法取消。加上标记后它们跑在 async runtime 的线程池里。
use crate::adapters::{adapter_for, same_path};
use crate::model::{
    AgentKind, AgentSummary, DayActivity, PlanEntry, PlanRecord, ProjectOutcome, ProjectSummary,
    RepoTodoReport, SearchHit, SessionSummary,
};
use crate::paths::agent_root;
use crate::repo;

/// 左侧 agent 栏。未安装的 agent 也返回（installed=false），前端置灰而不是消失，
/// 这样用户知道工作台支持它、只是本机没装。
#[tauri::command(async)]
pub fn list_agents() -> Vec<AgentSummary> {
    let _operation = crate::index::operation();
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
#[tauri::command(async)]
pub fn list_projects(agent: String) -> Result<Vec<ProjectSummary>, String> {
    let _operation = crate::index::operation();
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).list_projects())
}

/// 某个项目下的会话列表。这一步才真正解析 JSONL。
#[tauri::command(async)]
pub fn list_sessions(agent: String, project_id: String) -> Result<Vec<SessionSummary>, String> {
    let _operation = crate::index::operation();
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).list_sessions(&project_id))
}

/// 项目成果盘点：累计改了哪些文件、哪几场会话改动最大。
#[tauri::command(async)]
pub fn project_outcome(agent: String, project_id: String) -> Result<ProjectOutcome, String> {
    let _operation = crate::index::operation();
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).project_outcome(&project_id))
}

/// 该项目下提取到的计划 / 待办清单，按时间倒序。
#[tauri::command(async)]
pub fn project_plans(agent: String, project_id: String) -> Result<Vec<PlanRecord>, String> {
    let _operation = crate::index::operation();
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
        let _operation = crate::index::operation();
        ensure_known_project(&project_path)?;
        Ok(repo::scan_project(std::path::Path::new(&project_path)))
    })
    .await
    .map_err(|e| format!("扫描任务失败: {e}"))?
}

/// 白名单校验：目标必须确实是某个 agent 记录过的项目目录。
///
/// `same_path` 比较实际存在目录的 canonicalize 结果，遵循文件系统的
/// 大小写和链接语义；路径不存在或无法解析时拒绝，不做字符串兜底。
///
/// **信任链的边界要说清楚**：白名单里的 `p.path` 来自各 adapter 的 `peek_cwd`，
/// 也就是**被解析的会话文件自身的内容**。控制了会话 JSONL 就能把任意目录送进
/// 白名单，再让 TODO 扫描去遍历它并把内容回显。这道校验挡的是「前端传来的
/// 任意路径」，不是「被篡改的会话文件」——后者属于威胁模型之外（见 README），
/// 与计划文件 slug 同源。
fn ensure_known_project(project_path: &str) -> Result<(), String> {
    let known = AgentKind::all().into_iter().any(|kind| {
        agent_root(kind).map(|p| p.exists()) == Some(true)
            && adapter_for(kind).list_projects().iter().any(|p| same_path(&p.path, project_path))
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
#[tauri::command(async)]
pub fn project_docs(project_path: String) -> Result<Vec<PlanEntry>, String> {
    let _operation = crate::index::operation();
    ensure_known_project(&project_path)?;
    Ok(repo::scan_docs(std::path::Path::new(&project_path)))
}

/// 该 agent 的按天活动量，供热力图。首次调用会全量解析，之后走缓存。
#[tauri::command(async)]
pub fn agent_activity(agent: String) -> Result<Vec<DayActivity>, String> {
    let _operation = crate::index::operation();
    let kind = AgentKind::from_id(&agent).ok_or_else(|| format!("未知 agent: {agent}"))?;
    Ok(adapter_for(kind).daily_activity())
}

/// 全局搜索：标题、首条 prompt、触达文件路径、项目路径。
///
/// 数据量在几十场会话量级，直接内存扫描就够，不值得为此引入全文索引依赖。
#[tauri::command(async)]
pub fn search(query: String, limit: Option<usize>) -> Vec<SearchHit> {
    let _operation = crate::index::operation();
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
///
/// **不能拿小写串的字节偏移去切原串**：`to_lowercase()` 不保长（`İ` U+0130
/// 2 字节，小写化成 `i̇` 3 字节），偏移一旦错位并落在多字节字符中间，
/// `text[..byte_pos]` 直接 panic；而 Cargo.toml 设了 `panic = "abort"`，
/// release 下没有 unwind，**整个应用会崩溃退出**——触发源还是用户输入的搜索词
/// （可复现输入：文本 `"İ中"` 搜 `中`）。
///
/// 做法：一边小写化一边记下「小写串的每个字节属于原串的第几个字符」，
/// 命中后直接查表拿字符下标，全程不对原串做字节切片。
fn snippet(text: &str, needle: &str) -> String {
    let mut lower = String::with_capacity(text.len());
    // lower 的字节下标 -> 它来自原串的第几个字符
    let mut owner: Vec<usize> = Vec::with_capacity(text.len());
    for (ci, ch) in text.chars().enumerate() {
        let before = lower.len();
        for lc in ch.to_lowercase() {
            lower.push(lc);
        }
        owner.resize(lower.len(), ci);
        debug_assert!(lower.len() >= before);
    }

    let Some(byte_pos) = lower.find(needle) else {
        return text.chars().take(100).collect();
    };
    let char_pos = owner.get(byte_pos).copied().unwrap_or(0);
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

/// 清空解析缓存。
///
/// 缓存里存着各会话的首条 prompt 原文和触达文件路径，明文落在
/// `%LOCALAPPDATA%\AgentWorkbench\parse-cache.json`。只读承诺管的是「不写
/// agent 目录」，不等于可以把 agent 工作区里的内容无限期复制到用户不知情的
/// 新位置，所以给用户留一个主动清除的入口（`index::clear` 原先是死代码）。
///
/// 清完下次打开项目会重新全量解析，只是慢一点，不丢任何数据。
#[tauri::command(async)]
pub fn clear_cache() -> Result<u64, String> {
    let before = crate::index::disk_bytes();
    crate::index::clear().map_err(|e| format!("清空缓存失败: {e}"))?;
    Ok(before)
}

/// 缓存当前占用的磁盘字节数，给设置项显示用。
#[tauri::command(async)]
pub fn cache_size() -> u64 {
    crate::index::disk_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 摘要不会因大小写映射不保长而崩溃() {
        // `İ`(U+0130, 2 字节) 小写化成 `i̇`(3 字节)，小写串的偏移落在原串
        // `中` 的内部。旧实现在这里直接 panic，而 release 是 panic=abort，
        // 等于用户随手一搜就把应用搜崩了。
        assert!(snippet("İ中", "中").contains('中'));
        assert!(!snippet("İ中", "中").is_empty());
        // 一串会放大偏移错位的输入
        let text = "İİİİ关键词ẞß";
        assert!(snippet(text, "关键词").contains("关键词"));
    }

    #[test]
    fn 摘要能围绕关键词裁出上下文() {
        let text = format!("{}命中{}", "前".repeat(100), "后".repeat(100));
        let out = snippet(&text, "命中");
        assert!(out.contains("命中"));
        assert!(out.starts_with('…') && out.ends_with('…'), "两端都该有省略号");
        assert!(out.chars().count() < 130, "只留前后各约 40 字");
    }

    #[test]
    fn 搜不到时退回开头一段() {
        let out = snippet("abcdef", "zzz");
        assert_eq!(out, "abcdef");
    }
}
