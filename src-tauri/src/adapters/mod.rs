//! Agent 适配层。每家 agent 一个实现，把各自的 JSONL 归一到 model.rs 的结构。
//!
//! 分工约定：
//!   - `list_projects` 只做目录枚举和 stat，不解析文件内容，保证首屏秒开；
//!   - `list_sessions` 才真正逐行解析该项目下的会话，按需付费。
pub mod claude;
pub mod codex;
pub mod workbuddy;

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use crate::model::{
    AgentKind, DayActivity, FileOutcome, ParsedSession, PlanEntry, PlanRecord, ProjectOutcome,
    ProjectSummary, SessionSummary,
};

pub trait AgentAdapter {
    fn kind(&self) -> AgentKind;
    fn list_projects(&self) -> Vec<ProjectSummary>;

    /// 解析该项目下所有会话，含逐文件明细。
    /// 各家 adapter 只需实现这一个，下面几个视图都从它派生。
    fn parse_project(&self, project_id: &str) -> Vec<ParsedSession>;

    /// 解析该 agent 下所有项目。热力图和全局搜索要用。
    /// 首次调用会全量解析，之后走缓存。
    fn parse_all(&self) -> Vec<ParsedSession> {
        self.list_projects().iter().flat_map(|p| self.parse_project(&p.id)).collect()
    }

    /// 该项目下的全部计划 / 待办，按时间倒序。
    fn project_plans(&self, project_id: &str) -> Vec<PlanRecord> {
        let kind = self.kind();
        let mut out: Vec<PlanRecord> = self
            .parse_project(project_id)
            .into_iter()
            .flat_map(|ps| {
                let (id, title) = (ps.summary.id.clone(), ps.summary.title.clone());
                ps.plans.into_iter().map(move |entry| PlanRecord {
                    agent: kind,
                    session_id: id.clone(),
                    session_title: title.clone(),
                    entry,
                })
            })
            .collect();
        out.sort_by(|a, b| b.entry.at.cmp(&a.entry.at));
        out
    }

    /// 按天聚合的活动量，供热力图。
    fn daily_activity(&self) -> Vec<DayActivity> {
        let mut days: BTreeMap<String, (u32, usize)> = BTreeMap::new();
        for ps in self.parse_all() {
            for (day, n) in &ps.daily {
                let e = days.entry(day.clone()).or_insert((0, 0));
                e.0 += n;
                e.1 += 1;
            }
        }
        days.into_iter()
            .map(|(day, (events, sessions))| DayActivity { day, events, sessions })
            .collect()
    }

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

/// 大小写不敏感的前缀判断，命中时返回 `path` 里那个分隔符的字节下标。
///
/// **不能靠 `to_lowercase()` 之后比字节长度**：小写化不保长（`İ` 2 字节 →
/// `i̇` 3 字节），拿小写串的长度去切原串，轻则切错位置、重则落在多字节字符
/// 中间直接 panic。这里逐字符做大小写不敏感比较，下标始终取自原串。
fn dir_prefix_len(path: &str, dir: &str) -> Option<usize> {
    let d = dir.trim_end_matches(['/', '\\']);
    if d.is_empty() {
        return None;
    }
    let mut it = path.char_indices();
    for dc in d.chars() {
        let (_, pc) = it.next()?;
        if !same_path_char(pc, dc) {
            return None;
        }
    }
    // 目录名之后必须紧跟分隔符，否则 proj-backup 会被误判成 proj 的子目录
    let (idx, sep) = it.next()?;
    matches!(sep, '/' | '\\').then_some(idx)
}

/// 路径字符的等价判断：大小写不敏感，且两种分隔符视为等价
/// （同一目录在 agent 记录里可能正反斜杠混用）。
fn same_path_char(a: char, b: char) -> bool {
    if matches!(a, '/' | '\\') && matches!(b, '/' | '\\') {
        return true;
    }
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// 白名单只接受真实存在的目录，比较文件系统解析后的路径。
/// 大小写、分隔符、`.` / `..` 和符号链接的语义由文件系统决定，
/// 不对路径做小写化，也不在解析失败时回退到宽松字符串比较。
pub fn same_path(a: &str, b: &str) -> bool {
    let (Ok(a), Ok(b)) = (Path::new(a).canonicalize(), Path::new(b).canonicalize()) else {
        return false;
    };
    a.is_dir() && b.is_dir() && a == b
}

/// Windows 路径大小写不敏感，而 agent 记录里同一目录的大小写可能不一致，
/// 用 `str::starts_with` 会漏判。
fn starts_with_dir(path: &str, dir: &str) -> bool {
    dir_prefix_len(path, dir).is_some()
}

/// 转成相对项目根的路径；不在项目内的保持绝对路径。
fn relative_to(path: &str, dir: &str) -> String {
    match dir_prefix_len(path, dir) {
        // idx 是分隔符在原串里的字节下标，分隔符必是单字节，+1 一定落在字符边界
        Some(idx) => path[idx + 1..].to_string(),
        None => path.to_string(),
    }
}

pub fn adapter_for(kind: AgentKind) -> Box<dyn AgentAdapter> {
    match kind {
        AgentKind::Claude => Box::new(claude::ClaudeAdapter),
        AgentKind::Codex => Box::new(codex::CodexAdapter),
        AgentKind::WorkBuddy => Box::new(workbuddy::WorkBuddyAdapter),
    }
}

/// 逐个走缓存解析，最后统一落盘一次。
/// 各家 adapter 的 `parse_project` 都该经由它，免得各写一遍缓存逻辑。
///
/// **「进行中」在这里现算，不吃缓存**：它是 `now - mtime` 的函数，而缓存的失效
/// 条件恰恰是 mtime 变化——会话结束后文件不再变化，缓存就永远命中，解析当时算出的
/// `running: true` 会被永久固化，还随 parse-cache.json 落盘、重启也不恢复。
/// 结果是任何「用户趁 agent 干活时打开看过」的会话会永远显示进行中。
pub fn cached_collect<F>(paths: Vec<PathBuf>, parse: F) -> Vec<ParsedSession>
where
    F: Fn(&Path) -> Option<ParsedSession>,
{
    let out: Vec<ParsedSession> = paths
        .iter()
        .filter_map(|p| {
            let mut ps = crate::index::get_or_parse(p, || parse(p))?;
            ps.summary.running = is_running(p);
            Some(ps)
        })
        .collect();
    crate::index::flush();
    out
}

/// 最近是否仍在写入。时间派生量，只能在返回给前端之前现算。
pub fn is_running(path: &Path) -> bool {
    claude::mtime_ms(path).is_some_and(|m| now_ms() - m < RUNNING_WINDOW_MS)
}

/// 折叠快照式计划。
///
/// `update_plan` / `TodoWrite` 每调一次就落一条记录，而它们记的是**同一份清单的
/// 不同进度快照**，不是不同的计划。原样列出会让同一场会话冒出 3–5 张只差勾选
/// 进度的卡片（MaxKB / canpay-web 实测如此），计划面板的信噪比直接崩掉。
///
/// 只折叠 `kind == "todos"`：`ExitPlanMode` 那种带正文的计划，同一场会话里出现
/// 两份通常真的是两份不同的计划，不能合并。同来源只留 `at` 最大的一条，并把被它
/// 取代的次数记进 `revisions`，前端显示成「演进 N 次」角标——信息不丢。
///
/// 放在 `parse_session` 收尾处做，不在 `plan_from_call` / `plan_from_tool` 里做：
/// 那一层只看得见单条记录，看不到全局。
pub fn fold_plan_snapshots(plans: Vec<PlanEntry>) -> Vec<PlanEntry> {
    // source -> 已折叠结果在 out 里的下标
    let mut latest: BTreeMap<String, usize> = BTreeMap::new();
    let mut out: Vec<PlanEntry> = Vec::with_capacity(plans.len());

    for p in plans {
        if p.kind != "todos" {
            out.push(p);
            continue;
        }
        match latest.get(&p.source) {
            Some(&i) => {
                let revisions = out[i].revisions + 1;
                // 时间相同（同一毫秒内连调）时按出现顺序取后者
                if p.at >= out[i].at {
                    out[i] = PlanEntry { revisions, ..p };
                } else {
                    out[i].revisions = revisions;
                }
            }
            None => {
                latest.insert(p.source.clone(), out.len());
                out.push(p);
            }
        }
    }
    out
}

/// 逐行读取会话文件，坏字节不中断、IO 错误不死循环。
///
/// 标准库的两种写法都不能用：
///   - `lines().map_while(Result::ok)`：`BufRead::lines()` 遇到无效 UTF-8 会
///     yield `Err`，`map_while` 把它变成 `None` 从而**结束整个迭代**——不是跳过
///     这一行，是丢弃该行之后的全部内容。一次编码损坏（并发写入撕裂、混入非
///     UTF-8 字节）就让整场会话的轮数、工具调用、花费、文件清单全部少算，而且
///     错误结果会连同指纹一起写进缓存固化下来，UI 上完全看不出区别。
///   - `lines().filter_map(Result::ok)`：编码错误确实被跳过了（`read_line` 已经
///     消耗掉那一行的字节），但真正的 IO 错误不消耗任何字节，会原地无限重试。
///
/// 所以自己按字节读到换行，再 `from_utf8_lossy`：坏字节变成替换字符、这一行照常
/// 产出，后面的内容继续解析；只有 `read_until` 返回 Err（真 IO 错误）或 EOF 才停。
/// 文件尾部的半行（agent 正在写入）照常交出去，由调用方的 JSON 解析跳过。
pub struct LossyLines<R> {
    reader: R,
    buf: Vec<u8>,
}

impl<R: BufRead> Iterator for LossyLines<R> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        self.buf.clear();
        match self.reader.read_until(b'\n', &mut self.buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                while matches!(self.buf.last(), Some(b'\n' | b'\r')) {
                    self.buf.pop();
                }
                Some(String::from_utf8_lossy(&self.buf).into_owned())
            }
        }
    }
}

pub fn lossy_lines<R: BufRead>(reader: R) -> LossyLines<R> {
    LossyLines { reader, buf: Vec::with_capacity(512) }
}

/// 从计划 markdown 里抽勾选项 `- [ ]` / `- [x]`。
///
/// 注意只认真正的勾选框，不把普通列表项当待办 —— 计划正文里的普通 `-` 列表
/// 大多是选型说明、约束条件之类，混进待办清单会满屏噪声。
pub fn extract_checkboxes(md: &str) -> Vec<crate::model::TodoItem> {
    md.lines()
        .filter_map(|line| {
            let t = line.trim_start();
            let rest = t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))?;
            let rest = rest.trim_start();
            let (mark, text) = if let Some(r) = rest.strip_prefix("[ ]") {
                ("pending", r)
            } else if let Some(r) = rest.strip_prefix("[x]").or_else(|| rest.strip_prefix("[X]")) {
                ("completed", r)
            } else {
                return None;
            };
            let text = text.trim();
            (!text.is_empty()).then(|| crate::model::TodoItem {
                text: text.to_string(),
                status: mark.to_string(),
            })
        })
        .collect()
}

/// 取 markdown 的首个标题行作标题；没有标题就退回首个非空行。
pub fn markdown_title(md: &str) -> Option<String> {
    let heading = md
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim());
    let fallback = || md.lines().map(str::trim).find(|l| !l.is_empty());

    heading.or_else(fallback).filter(|s| !s.is_empty()).map(|s| {
        let mut out: String = s.chars().take(50).collect();
        if s.chars().count() > 50 {
            out.push('…');
        }
        out
    })
}

/// epoch 毫秒 → 本地日期 `YYYY-MM-DD`。
///
/// 热力图按用户所在时区的「天」划分，不能用 UTC —— 否则晚上的活动会被算到第二天。
pub fn local_day(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
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
    fn 勾选框只认真正的复选框() {
        let md = "\
# 计划
## 选型
- Electron + React
- 离线优先
## 步骤
- [ ] 扫描目录
- [x] 解析 EXIF
* [X] 分组展示
";
        let items = extract_checkboxes(md);
        // 「选型」下的普通列表项是说明不是待办，不能混进来
        assert_eq!(items.len(), 3, "只有 3 个复选框");
        assert_eq!(items[0].text, "扫描目录");
        assert_eq!(items[0].status, "pending");
        assert_eq!(items[1].status, "completed");
        assert_eq!(items[2].status, "completed", "星号列表和大写 X 也要认");
    }

    #[test]
    fn 计划标题取首个标题行() {
        assert_eq!(markdown_title("# 本地相册应用\n\n## Context\n正文"), Some("本地相册应用".into()));
        // 没有标题行时退回首个非空行
        assert_eq!(markdown_title("\n\n先做扫描\n再做分组"), Some("先做扫描".into()));
        assert_eq!(markdown_title("   \n  "), None);
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

    fn todos(source: &str, at: i64, n: usize) -> PlanEntry {
        PlanEntry {
            at: Some(at),
            kind: "todos".into(),
            title: format!("快照 {at}"),
            body: None,
            items: (0..n)
                .map(|i| crate::model::TodoItem {
                    text: format!("步骤 {i}"),
                    status: "pending".into(),
                })
                .collect(),
            source: source.into(),
            revisions: 1,
        }
    }

    #[test]
    fn 快照式计划按来源折叠成最后一份() {
        // update_plan / TodoWrite 每刷新一次就落一条，同一场会话会出现
        // 3–5 张只差勾选进度的卡片
        let folded = fold_plan_snapshots(vec![
            todos("update_plan", 100, 1),
            todos("update_plan", 200, 2),
            todos("update_plan", 300, 3),
        ]);
        assert_eq!(folded.len(), 1, "三份快照应折叠成一份");
        assert_eq!(folded[0].at, Some(300), "留最新的那份");
        assert_eq!(folded[0].items.len(), 3);
        assert_eq!(folded[0].revisions, 3, "被折叠掉的份数要保留下来给前端显示");
    }

    #[test]
    fn 不同来源的计划不互相折叠() {
        let folded = fold_plan_snapshots(vec![
            todos("update_plan", 100, 1),
            todos("TodoWrite", 150, 2),
            todos("update_plan", 200, 3),
        ]);
        assert_eq!(folded.len(), 2, "update_plan 与 TodoWrite 是两套清单");
    }

    #[test]
    fn 带正文的计划不被折叠() {
        // ExitPlanMode 同一场会话里出现两份，通常真的是两份不同的计划
        let plan = |at: i64, body: &str| PlanEntry {
            at: Some(at),
            kind: "plan".into(),
            title: "实施计划".into(),
            body: Some(body.into()),
            items: Vec::new(),
            source: "ExitPlanMode".into(),
            revisions: 1,
        };
        let folded = fold_plan_snapshots(vec![plan(100, "先做 A"), plan(200, "再做 B")]);
        assert_eq!(folded.len(), 2, "带正文的计划不能合并，会丢内容");
    }

    #[test]
    fn 相对路径对非_ascii_目录名不越界() {
        // starts_with_dir 原先用小写串的字节长度去切原串，而 to_lowercase()
        // 不保长（`İ` 2 字节 → 3 字节），这种输入会切错位置甚至 panic
        let dir = "C:\\Users\\İstanbul\\proj";
        let file = "C:\\Users\\İstanbul\\proj\\src\\main.rs";
        assert!(starts_with_dir(file, dir));
        assert_eq!(relative_to(file, dir), "src\\main.rs");
    }

    #[test]
    fn 同一目录的正反斜杠视为等价() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let alias = root.join("src").join("..");
        assert!(same_path(root.to_str().unwrap(), alias.to_str().unwrap()));
        #[cfg(windows)]
        assert!(same_path(root.to_str().unwrap(), &root.to_string_lossy().replace('\\', "/")));
    }

    #[test]
    fn 白名单拒绝不存在的目录和文件() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let missing = root.join("missing-whitelist-project");
        assert!(!same_path(missing.to_str().unwrap(), missing.to_str().unwrap()));
        let file = root.join("Cargo.toml");
        assert!(!same_path(file.to_str().unwrap(), file.to_str().unwrap()));
    }

    #[test]
    fn 白名单遵守实际文件系统的大小写语义() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let lower = root.join("src");
        let upper = root.join("SRC");
        let expected = upper.canonicalize().ok() == lower.canonicalize().ok();
        assert_eq!(same_path(lower.to_str().unwrap(), upper.to_str().unwrap()), expected);
    }

    fn stub_session(running: bool) -> ParsedSession {
        ParsedSession {
            summary: SessionSummary {
                agent: AgentKind::Claude,
                project_id: "p".into(),
                project_path: "p".into(),
                id: "s".into(),
                title: "t".into(),
                started_at: None,
                ended_at: None,
                wall_ms: 0,
                user_turns: 0,
                assistant_turns: 0,
                tool_calls: 0,
                models: Vec::new(),
                git_branch: None,
                files_touched: 0,
                lines_added: 0,
                lines_removed: 0,
                cost_usd: None,
                running,
                bytes: 0,
            },
            files: Vec::new(),
            daily: BTreeMap::new(),
            first_prompt: None,
            plans: Vec::new(),
        }
    }

    #[test]
    fn 进行中标志不吃缓存() {
        // 这是「会话永久显示进行中」那个 bug 的回归测试：running 是 now - mtime
        // 的函数，而缓存的失效条件恰恰是 mtime 变化——会话结束后文件不再变化，
        // 缓存永远命中，解析当时算出的 true 就被永久固化了。
        // cached_collect 必须无条件按当前 mtime 重算，覆盖 parse 给出的值。

        // 刚写出来的文件 = 进行中，即使解析结果说 false
        let dir = crate::paths::ensure_app_data_dir().expect("应用数据目录应可创建");
        let f = dir.join("running-test.jsonl");
        crate::paths::write_app_file(&f, b"{}").unwrap();

        let out = cached_collect(vec![f.clone()], |_| Some(stub_session(false)));
        assert_eq!(out.len(), 1);
        assert!(out[0].summary.running, "文件刚写过，应判为进行中");

        let _ = crate::paths::remove_app_file(&f);

        // 文件已不在（等价于「早已结束、mtime 取不到」）= 不进行中，
        // 即使缓存里那条记的是 true
        let gone = dir.join("running-test-gone.jsonl");
        let out = cached_collect(vec![gone], |_| Some(stub_session(true)));
        assert_eq!(out.len(), 1);
        assert!(!out[0].summary.running, "缓存里的 true 必须被覆盖掉");
    }

    #[test]
    fn 坏字节不会截断后续行() {
        // 第 2 行里混了一个非 UTF-8 字节。旧实现（map_while）会在这里结束迭代，
        // 第 3 行之后的内容全部读不到，统计静默变小。
        let mut raw: Vec<u8> = Vec::new();
        raw.extend_from_slice("第一行\n".as_bytes());
        raw.extend_from_slice(&[0xff, 0xfe, b'x', b'\n']);
        raw.extend_from_slice("第三行\n".as_bytes());
        // 结尾没有换行：agent 正在写入时的半行，也要照常交出去
        raw.extend_from_slice("半行".as_bytes());

        let lines: Vec<String> = lossy_lines(std::io::Cursor::new(raw)).collect();
        assert_eq!(lines.len(), 4, "坏行之后的内容不能丢");
        assert_eq!(lines[0], "第一行");
        assert!(lines[1].ends_with('x'), "坏字节换成替换字符，这一行照常产出");
        assert_eq!(lines[2], "第三行");
        assert_eq!(lines[3], "半行");
    }

    #[test]
    fn 回车换行也要剥掉() {
        let lines: Vec<String> = lossy_lines(std::io::Cursor::new(b"a\r\nb\n".to_vec())).collect();
        assert_eq!(lines, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn 全是注入块时退化为原文() {
        // 没有真人输入时不能返回空串，否则卡片没标题
        assert!(!make_title("<system-reminder>只有注入</system-reminder>").is_empty());
    }
}
