//! 扫描项目工作目录里的 TODO 标记和待办文档。
//!
//! **这是本应用唯一读取 agent 数据目录之外文件的地方**。仍然是只读——用
//! `paths::open_readonly`，且路径必须先通过 `commands` 里的白名单校验（只允许
//! 扫描出现在某个 agent 项目列表里的目录），不接受前端传任意路径。
//!
//! 性能是这里的主要风险：项目源码树可能有几十万文件。三道闸门：
//!   1. 忽略已知的依赖和构建产物目录（node_modules / target / dist …）
//!   2. 跳过超大文件和二进制文件
//!   3. 遍历数、命中数都有硬上限，到顶就停
//! 再配合 `index` 里按 (大小, mtime) 的逐文件缓存，重复扫描只读变化过的文件。
use std::path::{Path, PathBuf};

use crate::adapters::extract_checkboxes;
use crate::model::{RepoTodo, RepoTodoDoc, RepoTodoReport};
use crate::paths::open_readonly;

/// 依赖目录和构建产物，扫了纯属浪费。
/// 注意没有排除 `bin`：有些项目把脚本放在那里，而它通常不大。
const IGNORE_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "obj",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".venv",
    "venv",
    "env",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    "vendor",
    "Pods",
    ".gradle",
    ".idea",
    ".vscode",
    "coverage",
    ".cache",
    ".terraform",
];

/// 超过这个大小的文件不看：TODO 极少出现在大文件里，
/// 大文件多半是生成物、压缩产物或数据集。
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// 遍历文件数上限，防止误扫超大仓库时卡死。
const MAX_FILES: usize = 60_000;
/// 命中数上限，防止前端被几万条标记压垮。
const MAX_TODOS: usize = 3_000;
/// 待办文档的查找深度（相对项目根）。
const DOC_MAX_DEPTH: usize = 2;

/// 认这几种标记。大写敏感——小写的 "todo" 在正常英文里太常见，会带来大量噪声。
const MARKERS: &[&str] = &["TODO", "FIXME", "XXX", "HACK"];

fn is_ignored_dir(name: &str) -> bool {
    IGNORE_DIRS.iter().any(|d| d.eq_ignore_ascii_case(name))
}

/// 文件名像不像待办文档。
fn is_todo_doc(name: &str) -> bool {
    let lower = name.to_lowercase();
    if !lower.ends_with(".md") && !lower.ends_with(".txt") {
        return false;
    }
    let stem = lower.trim_end_matches(".md").trim_end_matches(".txt");
    matches!(stem, "todo" | "todos" | "roadmap" | "plan" | "plans" | "backlog")
        || stem.contains("待办")
        || stem.contains("计划")
}

/// 读文本文件；二进制和超大文件返回 None。
fn read_text(path: &Path, size: u64) -> Option<String> {
    if size > MAX_FILE_BYTES {
        return None;
    }
    let mut file = open_readonly(path).ok()?;
    let mut buf = Vec::with_capacity(size as usize);
    std::io::Read::read_to_end(&mut file, &mut buf).ok()?;
    // 含 NUL 字节就当二进制，别拿去做行扫描
    if buf.iter().take(8192).any(|b| *b == 0) {
        return None;
    }
    String::from_utf8(buf).ok()
}

/// 从一个文件的内容里抽出 TODO 标记。
pub fn scan_text(rel: &str, content: &str) -> Vec<RepoTodo> {
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        // 一行里可能有多个标记，只认第一个，避免同一行重复上报
        let Some((marker, at)) = MARKERS
            .iter()
            .filter_map(|m| line.find(*m).map(|p| (*m, p)))
            .min_by_key(|(_, p)| *p)
        else {
            continue;
        };

        // 要求是独立单词：前后不能紧邻字母数字，
        // 否则 "TODOS_TABLE"、"XXXX" 之类会被误判
        let before_ok = line[..at].chars().next_back().map_or(true, |c| !c.is_alphanumeric());
        let rest = &line[at + marker.len()..];
        let after_ok = rest.chars().next().map_or(true, |c| !c.is_alphanumeric() && c != '_');
        if !before_ok || !after_ok {
            continue;
        }

        let text = rest.trim_start_matches([':', '：', '(', ')', '-', ' ', '\t']).trim();
        // 截断过长行：压缩后的代码可能一行几千字
        let text: String = text.chars().take(160).collect();

        out.push(RepoTodo {
            file: rel.to_string(),
            line: i + 1,
            marker: marker.to_string(),
            text,
        });
    }
    out
}

/// 扫描一个项目目录。
pub fn scan_project(root: &Path) -> RepoTodoReport {
    let started = std::time::Instant::now();
    let mut report = RepoTodoReport {
        root: root.display().to_string(),
        exists: root.is_dir(),
        todos: Vec::new(),
        docs: Vec::new(),
        files_scanned: 0,
        truncated: false,
        elapsed_ms: 0,
    };
    if !report.exists {
        return report;
    }

    // 显式栈做广度遍历，不用递归——深层目录递归有爆栈风险
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if report.files_scanned >= MAX_FILES || report.todos.len() >= MAX_TODOS {
            report.truncated = true;
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            let name = entry.file_name().to_string_lossy().to_string();

            if ft.is_dir() {
                // 隐藏目录一律跳过（.git 之外还有各种工具的私有目录），
                // 但项目根本身允许是隐藏目录
                if is_ignored_dir(&name) || name.starts_with('.') {
                    continue;
                }
                stack.push((path, depth + 1));
                continue;
            }
            if !ft.is_file() {
                continue;
            }

            let Ok(meta) = entry.metadata() else { continue };
            let rel = path
                .strip_prefix(root)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| name.clone());

            // 待办文档：整份读进来抽勾选框
            if depth < DOC_MAX_DEPTH && is_todo_doc(&name) {
                if let Some(content) = read_text(&path, meta.len()) {
                    let items = extract_checkboxes(&content);
                    report.docs.push(RepoTodoDoc {
                        file: rel.clone(),
                        total: items.len(),
                        done: items.iter().filter(|t| t.status == "completed").count(),
                        items: items.into_iter().take(200).collect(),
                    });
                }
            }

            report.files_scanned += 1;
            let mut found = crate::index::get_or_scan_file(&path, || {
                read_text(&path, meta.len()).map(|c| scan_text(&rel, &c))
            });
            report.todos.append(&mut found);

            if report.files_scanned >= MAX_FILES || report.todos.len() >= MAX_TODOS {
                report.truncated = true;
                break;
            }
        }
    }

    report.todos.truncate(MAX_TODOS);
    report.todos.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    report.docs.sort_by(|a, b| b.total.cmp(&a.total));
    crate::index::flush();
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 只认独立单词形式的标记() {
        let hits = scan_text(
            "a.rs",
            "let TODOS_TABLE = 1;\nlet x = 2; // TODO: 补上校验\nconst XXXX = 3;\n// FIXME 越界\n",
        );
        assert_eq!(hits.len(), 2, "TODOS_TABLE 和 XXXX 不该命中");
        assert_eq!(hits[0].marker, "TODO");
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].text, "补上校验", "冒号和空格要剥掉");
        assert_eq!(hits[1].marker, "FIXME");
        assert_eq!(hits[1].text, "越界");
    }

    #[test]
    fn 一行多个标记只报一次() {
        let hits = scan_text("a.rs", "// TODO 先做这个 FIXME 再做那个\n");
        assert_eq!(hits.len(), 1, "同一行重复上报会让计数虚高");
        assert_eq!(hits[0].marker, "TODO");
    }

    #[test]
    fn 小写不命中() {
        // 正常英文散文里 todo 太常见，大小写不敏感会淹没真实标记
        assert!(scan_text("a.md", "things to do: todo list\n").is_empty());
    }

    #[test]
    fn 待办文档识别() {
        assert!(is_todo_doc("TODO.md"));
        assert!(is_todo_doc("todos.txt"));
        assert!(is_todo_doc("ROADMAP.md"));
        assert!(is_todo_doc("待办事项.md"));
        assert!(!is_todo_doc("README.md"));
        assert!(!is_todo_doc("todo.rs"), "只认 md 和 txt");
    }

    #[test]
    fn 忽略目录判定不区分大小写() {
        assert!(is_ignored_dir("node_modules"));
        assert!(is_ignored_dir("Node_Modules"));
        assert!(!is_ignored_dir("src"));
        assert!(!is_ignored_dir("bin"), "bin 里常放脚本，不排除");
    }
}
