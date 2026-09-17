//! 解析结果的增量缓存。
//!
//! 全量解析本机数据要 1 秒出头，每次点项目都重来一遍太浪费。缓存以
//! `(绝对路径, 文件大小, mtime)` 为键：三者全都没变就直接复用，任何一项变了
//! 就重新解析。agent 写会话文件必然改变大小和 mtime，所以不会漏更新。
//!
//! **只缓存「文件内容的函数」**：凡是随时间变化的派生量（如「进行中」标志）
//! 都不能进来 —— 会话结束后文件不再变化，缓存永远命中，当时算出的 true 会被
//! 永久固化。这类字段一律在 `adapters::cached_collect` 里按当前 mtime 现算。
//!
//! **没有用 SQLite**：rusqlite(bundled) 会给二进制加约 1.5–2 MB，而本机全量
//! 数据也就 1 秒级、几十场会话。为需求 5（体积尽可能小）这笔不划算，
//! JSON 缓存同样是增量，且零新依赖。
//!
//! 这是全应用唯一写磁盘的地方，落盘走 `paths::write_app_file`（内部先断言）。
//!
//! **缓存里有用户数据**：`ParsedSession` 含首条 prompt 和触达文件路径，明文落在
//! `%LOCALAPPDATA%\AgentWorkbench\parse-cache.json`。只读承诺管的是「不写 agent
//! 目录」，不等于可以无限外扩散，所以这里有三道约束：源文件已消失的条目在加载和
//! 落盘时都会被剔除；条目数有硬上限，超了按 mtime 淘汰最旧的；`clear()` 接到了
//! `clear_cache` 命令上，用户可以在界面里一键清空。
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock, RwLock, RwLockReadGuard};

use serde::{Deserialize, Serialize};

use crate::model::{ParsedSession, RepoTodo};
use crate::paths::{app_data_dir, remove_app_file, rename_app_file, write_app_file};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    size: u64,
    mtime: i64,
    parsed: ParsedSession,
}

/// 源码文件的扫描结果。TODO 标记通常为空，所以绝大多数条目只占一个空数组。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoEntry {
    size: u64,
    mtime: i64,
    todos: Vec<RepoTodo>,
}

/// rollout 文件 -> 它记的 cwd。
///
/// Codex 的项目归属只能从文件内容反查，而 `list_projects` 和 `parse_project`
/// 各要 peek 一遍全部 rollout：P 个项目 N 个文件就是 `N + P×N` 次文件打开，
/// 随数据量平方级放大。真正耗时的全量解析有缓存，这一步原先没有，缓存暖了之后
/// 它反而成了 Codex 路径的主要开销。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CwdEntry {
    size: u64,
    mtime: i64,
    /// None 表示这个文件里没有 session_meta，别再重复去读确认一遍
    cwd: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    /// 缓存格式版本。解析逻辑一改，旧缓存的字段含义就可能不同，
    /// 版本对不上直接全部丢弃重建 —— 比留着可疑数据安全。
    version: u32,
    entries: HashMap<String, Entry>,
    /// 项目源码文件 -> TODO 标记
    #[serde(default)]
    repo: HashMap<String, RepoEntry>,
    /// Codex rollout -> cwd
    #[serde(default)]
    cwd: HashMap<String, CwdEntry>,
}

/// 当前缓存格式版本。改动任何 adapter 的统计口径都要 +1。
/// v2: ParsedSession 增加 plans 字段（计划 / 待办提取）
/// v3: 增加 repo 映射（项目源码 TODO 扫描）
/// v4: decode_project_dir 在 unix 上改走 POSIX 还原（WorkBuddy 的 mac 目录名没有前导 `-`），
///     缓存里的 ParsedSession.project_path 可能是旧的错误解码结果，必须重建
/// v5: Claude 的 first_prompt 改存剥离注入块后的文本；快照式计划按来源折叠
///     （PlanEntry 增加 revisions）；坏行不再截断解析，旧缓存里的统计可能偏小
const VERSION: u32 = 5;

/// 条目数上限。超了按 mtime 淘汰最旧的，免得缓存随时间单调膨胀。
/// 两个映射分开计：源码文件条目远多于会话条目。
const MAX_SESSION_ENTRIES: usize = 4_000;
const MAX_REPO_ENTRIES: usize = 40_000;

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
// 查询持共享锁直到整条命令完成；清空持独占锁，等待旧查询及其 flush 结束。
static OPERATIONS: RwLock<()> = RwLock::new(());

pub fn operation() -> RwLockReadGuard<'static, ()> {
    OPERATIONS.read().unwrap_or_else(|e| e.into_inner())
}

fn cache_file() -> Option<std::path::PathBuf> {
    #[cfg(not(test))]
    let name = "parse-cache.json".to_string();
    #[cfg(test)]
    let name = format!("parse-cache-test-{}.json", std::process::id());
    app_data_dir().ok().map(|d| d.join(name))
}

/// 取锁。**不能用 `if let Ok(..)` 忽略中毒**：任一持锁期间的 panic 之后，
/// 所有缓存读写都会静默跳过，性能悄悄退化成每次全量解析且没有任何提示。
/// 缓存是纯派生数据，中毒后里面的内容依然可用，直接取回内部值继续。
fn lock() -> MutexGuard<'static, Store> {
    store().lock().unwrap_or_else(|e| e.into_inner())
}

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| {
        let loaded = cache_file()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Store>(&b).ok())
            .filter(|s| s.version == VERSION)
            .unwrap_or_default();
        let mut s =
            Store { version: VERSION, entries: loaded.entries, repo: loaded.repo, cwd: loaded.cwd };
        // 源文件已被删除的条目留着既没用又在持续外扩散数据，启动时先扫一遍
        prune_missing(&mut s);
        cap(&mut s);
        Mutex::new(s)
    })
}

/// 丢掉源文件已不存在的条目。
///
/// **只在加载时跑一次**：它要对每个条目做一次 `exists()` 系统调用，而 `flush`
/// 是每解析完一个项目就调一次的——放进 flush 会变成 O(项目数 × 条目数) 次 stat，
/// 实测把一次全局搜索从 131ms 拖到 21s。启动时扫一遍足够了：会话文件在应用
/// 运行期间被删除是罕见情况，留到下次启动清也不迟。
fn prune_missing(s: &mut Store) {
    s.entries.retain(|k, _| Path::new(k).exists());
    s.repo.retain(|k, _| Path::new(k).exists());
    s.cwd.retain(|k, _| Path::new(k).exists());
}

/// 把超出上限的部分按 mtime 从旧到新淘汰。只比长度，没有系统调用，
/// 可以在每次 flush 前跑。
fn cap(s: &mut Store) {
    evict(&mut s.entries, MAX_SESSION_ENTRIES, |e| e.mtime);
    evict(&mut s.repo, MAX_REPO_ENTRIES, |e| e.mtime);
    evict(&mut s.cwd, MAX_REPO_ENTRIES, |e| e.mtime);
}

fn evict<V>(map: &mut HashMap<String, V>, cap: usize, mtime: impl Fn(&V) -> i64) {
    if map.len() <= cap {
        return;
    }
    let drop_count = map.len() - cap;
    let mut keys: Vec<(i64, String)> = map.iter().map(|(k, v)| (mtime(v), k.clone())).collect();
    // 最旧的排前面，砍掉超出的那一截
    keys.sort_by_key(|(m, _)| *m);
    for (_, k) in keys.into_iter().take(drop_count) {
        map.remove(&k);
    }
}

/// 文件的身份指纹：大小 + mtime。
fn fingerprint(path: &Path) -> Option<(u64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((meta.len(), mtime))
}

/// 缓存命中就直接返回，否则调 `parse` 重新解析并记下。
pub fn get_or_parse<F>(path: &Path, parse: F) -> Option<ParsedSession>
where
    F: FnOnce() -> Option<ParsedSession>,
{
    let key = path.to_string_lossy().to_string();
    let fp = fingerprint(path);

    if let Some((size, mtime)) = fp {
        if let Some(e) = lock().entries.get(&key) {
            if e.size == size && e.mtime == mtime {
                return Some(e.parsed.clone());
            }
        }
    }

    let parsed = parse()?;

    if let Some((size, mtime)) = fp {
        lock().entries.insert(key, Entry { size, mtime, parsed: parsed.clone() });
    }
    Some(parsed)
}

/// 源码文件的 TODO 扫描缓存。同样按 (大小, mtime) 判断是否可复用。
///
/// `scan` 返回 None 表示这个文件不该被扫（二进制、超大），此时也记成空结果，
/// 免得每次重扫都去读一遍同一个大文件。
pub fn get_or_scan_file<F>(path: &Path, scan: F) -> Vec<RepoTodo>
where
    F: FnOnce() -> Option<Vec<RepoTodo>>,
{
    let key = path.to_string_lossy().to_string();
    let fp = fingerprint(path);

    if let Some((size, mtime)) = fp {
        if let Some(e) = lock().repo.get(&key) {
            if e.size == size && e.mtime == mtime {
                return e.todos.clone();
            }
        }
    }

    let todos = scan().unwrap_or_default();

    if let Some((size, mtime)) = fp {
        lock().repo.insert(key, RepoEntry { size, mtime, todos: todos.clone() });
    }
    todos
}

/// rollout 的 cwd 缓存。读不到 cwd 也记下来，不再重复打开同一个文件。
pub fn get_or_peek_cwd<F>(path: &Path, peek: F) -> Option<String>
where
    F: FnOnce() -> Option<String>,
{
    let key = path.to_string_lossy().to_string();
    let fp = fingerprint(path);

    if let Some((size, mtime)) = fp {
        if let Some(e) = lock().cwd.get(&key) {
            if e.size == size && e.mtime == mtime {
                return e.cwd.clone();
            }
        }
    }

    let cwd = peek();

    if let Some((size, mtime)) = fp {
        lock().cwd.insert(key, CwdEntry { size, mtime, cwd: cwd.clone() });
    }
    cwd
}

/// 落盘。解析完一批后调一次，不要每条都写。
pub fn flush() {
    let Some(path) = cache_file() else { return };

    // 同一临时文件的写入/改名必须串行，不能在序列化后提前释放锁。
    let mut s = lock();
    if s.entries.is_empty() && s.repo.is_empty() && s.cwd.is_empty() {
        return;
    }
    cap(&mut s);
    let Ok(bytes) = serde_json::to_vec(&*s) else { return };

    // 先写临时文件再改名，避免写一半崩溃留下半个损坏的缓存。
    // 越界断言在 write_app_file / rename_app_file 内部。
    let tmp = path.with_extension("json.tmp");
    if write_app_file(&tmp, &bytes).is_ok() {
        let _ = rename_app_file(&tmp, &path);
    }
}

/// 丢弃全部缓存，内存和磁盘都清。接在 `commands::clear_cache` 上，
/// 用户可以在界面里主动清掉这份含 prompt 原文的派生数据。
pub fn clear() -> std::io::Result<()> {
    let _exclusive = OPERATIONS.write().unwrap_or_else(|e| e.into_inner());
    let mut s = lock();
    if let Some(path) = cache_file() {
        remove_app_file(&path)?;
        remove_app_file(&path.with_extension("json.tmp"))?;
    }
    s.entries.clear();
    s.repo.clear();
    s.cwd.clear();
    Ok(())
}

/// 缓存文件当前占多少字节，供界面展示。文件不存在时为 0。
pub fn disk_bytes() -> u64 {
    cache_file().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{assert_app_owned, ensure_app_data_dir};

    #[test]
    fn 清空等待进行中的查询结束() {
        let query = operation();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            clear().unwrap();
            done_tx.send(()).unwrap();
        });
        started_rx.recv().unwrap();
        let early = done_rx.recv_timeout(std::time::Duration::from_millis(100));
        drop(query);
        worker.join().unwrap();
        assert!(early.is_err(), "查询仍可能落盘时不能完成清空");
        done_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn 缓存文件落在应用数据目录内() {
        // 这条断言若失败说明缓存要写到 agent 工作区去了
        let p = cache_file().expect("应能解析缓存路径");
        assert_app_owned(&p);
    }

    #[test]
    fn 指纹随文件变化而变化() {
        let dir = ensure_app_data_dir().expect("应用数据目录应可创建");
        let f = dir.join("fingerprint-test.txt");

        write_app_file(&f, b"one").unwrap();
        let a = fingerprint(&f).expect("应能取到指纹");
        write_app_file(&f, b"one-longer").unwrap();
        let b = fingerprint(&f).expect("应能取到指纹");

        assert_ne!(a.0, b.0, "大小变了，指纹就该变");
        let _ = remove_app_file(&f);
    }

    #[test]
    fn 超出上限时淘汰最旧的条目() {
        let mut map: HashMap<String, RepoEntry> = HashMap::new();
        for i in 0..10 {
            map.insert(format!("f{i}"), RepoEntry { size: 1, mtime: i, todos: Vec::new() });
        }
        evict(&mut map, 4, |e| e.mtime);
        assert_eq!(map.len(), 4);
        assert!(map.contains_key("f9"), "最新的必须留下");
        assert!(!map.contains_key("f0"), "最旧的必须被淘汰");
    }
}
