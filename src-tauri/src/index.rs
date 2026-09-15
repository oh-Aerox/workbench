//! 解析结果的增量缓存。
//!
//! 全量解析本机数据要 1 秒出头，每次点项目都重来一遍太浪费。缓存以
//! `(绝对路径, 文件大小, mtime)` 为键：三者全都没变就直接复用，任何一项变了
//! 就重新解析。agent 写会话文件必然改变大小和 mtime，所以不会漏更新。
//!
//! **没有用 SQLite**：rusqlite(bundled) 会给二进制加约 1.5–2 MB，而本机全量
//! 数据也就 1 秒级、几十场会话。为需求 5（体积尽可能小）这笔不划算，
//! JSON 缓存同样是增量，且零新依赖。
//!
//! 这是全应用唯一写磁盘的地方，落盘前必过 `paths::assert_app_owned`。
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::model::{ParsedSession, RepoTodo};
use crate::paths::{app_data_dir, assert_app_owned};

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

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    /// 缓存格式版本。解析逻辑一改，旧缓存的字段含义就可能不同，
    /// 版本对不上直接全部丢弃重建 —— 比留着可疑数据安全。
    version: u32,
    entries: HashMap<String, Entry>,
    /// 项目源码文件 -> TODO 标记
    #[serde(default)]
    repo: HashMap<String, RepoEntry>,
}

/// 当前缓存格式版本。改动任何 adapter 的统计口径都要 +1。
/// v2: ParsedSession 增加 plans 字段（计划 / 待办提取）
/// v3: 增加 repo 映射（项目源码 TODO 扫描）
const VERSION: u32 = 3;

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

fn cache_file() -> Option<std::path::PathBuf> {
    app_data_dir().ok().map(|d| d.join("parse-cache.json"))
}

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| {
        let loaded = cache_file()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Store>(&b).ok())
            .filter(|s| s.version == VERSION)
            .unwrap_or_default();
        Mutex::new(Store { version: VERSION, entries: loaded.entries, repo: loaded.repo })
    })
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
        if let Ok(s) = store().lock() {
            if let Some(e) = s.entries.get(&key) {
                if e.size == size && e.mtime == mtime {
                    return Some(e.parsed.clone());
                }
            }
        }
    }

    let parsed = parse()?;

    if let Some((size, mtime)) = fp {
        if let Ok(mut s) = store().lock() {
            s.entries.insert(key, Entry { size, mtime, parsed: parsed.clone() });
        }
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
        if let Ok(s) = store().lock() {
            if let Some(e) = s.repo.get(&key) {
                if e.size == size && e.mtime == mtime {
                    return e.todos.clone();
                }
            }
        }
    }

    let todos = scan().unwrap_or_default();

    if let Some((size, mtime)) = fp {
        if let Ok(mut s) = store().lock() {
            s.repo.insert(key, RepoEntry { size, mtime, todos: todos.clone() });
        }
    }
    todos
}

/// 落盘。解析完一批后调一次，不要每条都写。
pub fn flush() {
    let Some(path) = cache_file() else { return };
    let Ok(s) = store().lock() else { return };
    let Ok(bytes) = serde_json::to_vec(&*s) else { return };

    // 只读守卫：确认目标确实在本应用数据目录内
    assert_app_owned(&path);

    // 先写临时文件再改名，避免写一半崩溃留下半个损坏的缓存
    let tmp = path.with_extension("json.tmp");
    assert_app_owned(&tmp);
    if std::fs::write(&tmp, &bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// 丢弃全部缓存。
pub fn clear() {
    if let Ok(mut s) = store().lock() {
        s.entries.clear();
        s.repo.clear();
    }
    if let Some(path) = cache_file() {
        assert_app_owned(&path);
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 缓存文件落在应用数据目录内() {
        // 这条断言若失败说明缓存要写到 agent 工作区去了
        let p = cache_file().expect("应能解析缓存路径");
        assert_app_owned(&p);
    }

    #[test]
    fn 指纹随文件变化而变化() {
        let dir = app_data_dir().expect("应用数据目录应可创建");
        let f = dir.join("fingerprint-test.txt");
        assert_app_owned(&f);

        std::fs::write(&f, b"one").unwrap();
        let a = fingerprint(&f).expect("应能取到指纹");
        std::fs::write(&f, b"one-longer").unwrap();
        let b = fingerprint(&f).expect("应能取到指纹");

        assert_ne!(a.0, b.0, "大小变了，指纹就该变");
        let _ = std::fs::remove_file(&f);
    }
}
