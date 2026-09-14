//! 路径解析 + 只读守卫。
//!
//! 需求 2：本应用绝不写入任何 agent 工作区。保证分三层：
//!   1. 读 agent 数据一律走 `open_readonly`，句柄本身没有写权限；
//!   2. 任何写操作必须先过 `assert_app_owned`，越界直接 panic；
//!   3. tauri.conf.json 不开放 fs / shell 权限，前端拿不到写能力。
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::model::AgentKind;

/// agent 数据根目录。三家在 Windows 和 macOS 上都落在用户主目录下的同名隐藏目录，
/// 所以这里不需要按平台分支。
pub fn agent_root(kind: AgentKind) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let dir = match kind {
        AgentKind::Claude => ".claude",
        AgentKind::Codex => ".codex",
        AgentKind::WorkBuddy => ".workbuddy",
    };
    Some(home.join(dir))
}

/// 本应用自己的数据目录，索引缓存只允许落在这里。
/// Windows: %LOCALAPPDATA%\AgentWorkbench
/// macOS:   ~/Library/Application Support/AgentWorkbench
pub fn app_data_dir() -> io::Result<PathBuf> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "无法定位用户数据目录"))?;
    let dir = base.join("AgentWorkbench");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 以只读方式打开。读取 agent 数据的唯一入口。
pub fn open_readonly(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(false).create(false).open(path)
}

/// 归一化路径，供越界比对使用。
///
/// 不能直接对目标调 `canonicalize`：待写入的文件通常还不存在，此时 canonicalize
/// 会失败并退回原始路径，而根目录是存在的、会被解析成 Windows 的 `\\?\C:\...`
/// 扩展长度形式 —— 两边前缀不一致，`starts_with` 就失效了。
///
/// 做法：从目标往上找到最深的已存在祖先，只对它 canonicalize，再把剩下的尾段接回去。
/// 这样两边必定走同一套前缀规则。
fn normalize(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();

    while !existing.exists() {
        match existing.file_name() {
            Some(n) => tail.push(n.to_os_string()),
            None => break,
        }
        if !existing.pop() {
            break;
        }
    }

    let mut base = existing.canonicalize().unwrap_or(existing);
    for seg in tail.iter().rev() {
        base.push(seg);
    }
    base
}

/// 写入前的越界断言。目标必须在本应用数据目录内，否则视为编程错误直接终止 —
/// 宁可崩溃，也不能写进用户的 agent 工作区。
pub fn assert_app_owned(path: &Path) {
    // `..` 无法靠字符串比对安全处理（尾段里的 `..` 不会被 canonicalize 解析），
    // 直接拒绝，不给绕出目录的机会。
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        panic!("只读守卫拦截：路径含 `..`，拒绝处理\n  目标: {}", path.display());
    }

    let root = match app_data_dir() {
        Ok(r) => r,
        Err(e) => panic!("只读守卫：无法解析应用数据目录: {e}"),
    };

    let canon_root = normalize(&root);
    let canon_target = normalize(path);

    if !canon_target.starts_with(&canon_root) {
        panic!(
            "只读守卫拦截：试图写入应用数据目录之外的路径\n  目标: {}\n  允许: {}",
            canon_target.display(),
            canon_root.display()
        );
    }
}

/// Claude / WorkBuddy 把 cwd 编码成目录名（分隔符换成 `-`）。
///
/// **这个编码是有损的**：路径里本来就带 `-` 的目录（如 `my-notes`）解码后会被
/// 拆成两级。所以真实 cwd 一律优先从会话文件内容里读（见各 adapter 的 `peek_cwd`），
/// 本函数只在文件读不出时兜底。
pub fn decode_project_dir(encoded: &str) -> String {
    // POSIX 绝对路径编码后以 `-` 开头（`/Users/a` → `-Users-a`）
    if let Some(rest) = encoded.strip_prefix('-') {
        return format!("/{}", rest.replace('-', "/"));
    }
    // Windows 盘符：`C--Users-a` → 首段是盘符，`--` 是 `:\` 的编码
    if let Some((drive, rest)) = encoded.split_once("--") {
        if drive.len() == 1 && drive.chars().all(|c| c.is_ascii_alphabetic()) {
            return format!("{}:\\{}", drive.to_uppercase(), rest.replace('-', "\\"));
        }
    }
    encoded.replace('-', "\\")
}

/// 统一盘符大小写。Claude 记的是 `C:\`，WorkBuddy 记的是 `c:\`，
/// 同一个目录在两个 agent 下显示不一致会让人以为是两个路径。
pub fn normalize_drive(path: &str) -> String {
    let mut chars = path.chars();
    match (chars.next(), chars.next()) {
        (Some(d), Some(':')) if d.is_ascii_alphabetic() => {
            format!("{}{}", d.to_ascii_uppercase(), &path[1..])
        }
        _ => path.to_string(),
    }
}

/// 取路径最后一段作为项目显示名。
pub fn project_display_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 应用数据目录内的写入被放行() {
        let dir = app_data_dir().expect("应用数据目录应可创建");
        assert_app_owned(&dir.join("index.db"));
        assert_app_owned(&dir.join("sub").join("nested.json"));
    }

    #[test]
    #[should_panic(expected = "只读守卫拦截")]
    fn 写入_claude_目录被拦截() {
        let root = agent_root(AgentKind::Claude).expect("home 应可解析");
        assert_app_owned(&root.join("projects").join("evil.jsonl"));
    }

    #[test]
    #[should_panic(expected = "路径含 `..`")]
    fn 用_双点_跳出应用目录被拦截() {
        let dir = app_data_dir().expect("应用数据目录应可创建");
        // 尾段里的 `..` 不会被 canonicalize 解析，只能靠组件检查挡住
        assert_app_owned(&dir.join("..").join("..").join("evil.jsonl"));
    }

    #[test]
    #[should_panic(expected = "只读守卫拦截")]
    fn 写入任意用户目录被拦截() {
        let home = dirs::home_dir().expect("home 应可解析");
        assert_app_owned(&home.join("Desktop").join("whatever.txt"));
    }

    #[test]
    fn 目录名解码_windows盘符() {
        assert_eq!(
            decode_project_dir("C--Users-alice-Desktop-project-demo"),
            "C:\\Users\\alice\\Desktop\\project\\demo"
        );
        // WorkBuddy 用小写盘符
        assert_eq!(
            decode_project_dir("c--Users-a-code"),
            "C:\\Users\\a\\code"
        );
    }

    #[test]
    fn 目录名解码_posix路径() {
        assert_eq!(decode_project_dir("-Users-a-code-gallery"), "/Users/a/code/gallery");
    }

    #[test]
    fn 目录名解码有损_故需从会话内容取真实cwd() {
        // `my-notes` 被拆成两级，这是编码本身的信息丢失，兜底解码无法还原。
        // adapter 必须优先 peek 会话文件里的 cwd 字段。
        assert_eq!(
            decode_project_dir("C--Users-a-project-my-notes"),
            "C:\\Users\\a\\project\\my\\notes"
        );
    }

    #[test]
    fn 盘符统一大写() {
        // WorkBuddy 记小写盘符，Claude 记大写，同一目录不能显示成两个
        assert_eq!(normalize_drive("c:\\Users\\a\\x"), "C:\\Users\\a\\x");
        assert_eq!(normalize_drive("C:\\Users\\a\\x"), "C:\\Users\\a\\x");
        assert_eq!(normalize_drive("/Users/a/x"), "/Users/a/x");
    }

    #[test]
    fn 项目显示名取末段() {
        assert_eq!(project_display_name("C:\\Users\\a\\Desktop\\gallery"), "gallery");
        assert_eq!(project_display_name("/Users/a/code/gallery/"), "gallery");
    }
}
