//! 路径解析 + 只读守卫。
//!
//! 需求 2：本应用绝不写入任何 agent 工作区。保证分三层：
//!   1. 读 agent 数据一律走 `open_readonly`，句柄本身没有写权限；
//!   2. 任何写操作必须走本模块的 `write_app_file` / `rename_app_file` /
//!      `remove_app_file`，它们内部先过 `assert_app_owned`，越界直接 panic；
//!   3. Cargo.toml 根本没引入 tauri-plugin-fs / tauri-plugin-shell，
//!      写文件的代码压根没被编译进二进制，前端无论如何拿不到写能力。
//!
//! 第 2 层原先只是「记得调用断言」的约定：`std::fs::write` 在 crate 里任何地方
//! 都能直接调，一次疏忽的提交就能破坏这条全项目最高优先级的不变量，而守卫单测
//! 只验「守卫函数判得对不对」，验不了「是否所有写操作都过了守卫」。现在把写操作
//! 收成本模块的唯一出口，并用 `写操作只能出现在_paths_rs` 这条单测做静态兜底。
//!
//! 读侧还有一条对称的 `resolve_under`：从**会话文件内容里读出来的名字**（如
//! `~/.claude/plans/<slug>.md` 的 slug）拼路径前必须过它，否则 `../../机密`
//! 这种值会把任意文件读进计划面板。
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
///
/// **纯函数，只算路径不碰文件系统**：`assert_app_owned` 会调它，而一个断言
/// 函数不该有副作用（原先这里带 `create_dir_all`，等于「检查一下」就顺手建了目录）。
/// 需要目录真实存在时用 `ensure_app_data_dir`。
pub fn app_data_dir() -> io::Result<PathBuf> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "无法定位用户数据目录"))?;
    Ok(base.join("AgentWorkbench"))
}

/// 同上，但确保目录已建好。只有真要写盘时才调。
pub fn ensure_app_data_dir() -> io::Result<PathBuf> {
    let dir = app_data_dir()?;
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

/// 写文件。**全 crate 唯一允许写盘的出口**，内部先做越界断言。
pub fn write_app_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    assert_app_owned(path);
    ensure_app_data_dir()?;
    std::fs::write(path, bytes)
}

/// 改名。先写临时文件再改名是为了避免留下半个损坏的缓存，两端都要断言。
pub fn rename_app_file(from: &Path, to: &Path) -> io::Result<()> {
    assert_app_owned(from);
    assert_app_owned(to);
    std::fs::rename(from, to)
}

/// 删文件。目标本来就不存在不算错误。
pub fn remove_app_file(path: &Path) -> io::Result<()> {
    assert_app_owned(path);
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// 读侧守卫：把一个**从外部数据里读出来的文件名**安全地拼到 `dir` 之下。
///
/// 写侧有 `assert_app_owned`，读侧一直缺一个对称的东西。`~/.claude/plans/<slug>.md`
/// 的 slug 直接取自会话 JSONL 的 `slug` 字段，未经任何校验就拼进路径——slug 为
/// `../../../Desktop/机密` 时会把 `~/Desktop/机密.md` 整篇读出来渲染进计划面板。
/// 会话文件是会被复制、同步、从他处拷入的数据，不该当成可信输入。
///
/// 规则：文件名必须是单个普通路径组件（无分隔符、无盘符、无 `.` / `..`、无控制
/// 字符），且拼出的路径归一化后仍落在 `dir` 内。两道都过了才返回路径。
pub fn resolve_under(dir: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty()
        || name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':') || c.is_control())
    {
        return None;
    }
    // 只接受单个 Normal 组件：`.` / `..` 是 CurDir / ParentDir，会在这里落空
    let mut comps = Path::new(name).components();
    if !matches!(
        (comps.next(), comps.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) {
        return None;
    }

    let joined = dir.join(name);
    normalize(&joined).starts_with(normalize(dir)).then_some(joined)
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
    // 剩下的既不带前导 `-`、也不带盘符。WorkBuddy 在 macOS 上就是这个形状：
    // `/Users/a/x` → `Users-a-x`，开头的 `/` 被直接吃掉了（Claude 则保留成 `-Users-a-x`）。
    // 同一台机器上不会混两个平台产出的目录名，所以按本平台的分隔符还原。
    #[cfg(unix)]
    let out = format!("/{}", encoded.replace('-', "/"));
    #[cfg(not(unix))]
    let out = encoded.replace('-', "\\");
    out
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
        let dir = app_data_dir().expect("应用数据目录路径应可解析");
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
        let dir = app_data_dir().expect("应用数据目录路径应可解析");
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
    fn 读侧守卫拦截路径穿越的_slug() {
        let plans = agent_root(AgentKind::Claude).expect("home 应可解析").join("plans");
        // 正常 slug 放行
        assert!(resolve_under(&plans, "fix-login.md").is_some());
        assert!(resolve_under(&plans, "重构计划.md").is_some(), "非 ASCII 文件名是合法的");

        // 会话 JSONL 里的 slug 不可信：下面这些值原先会被直接拼进路径
        assert!(resolve_under(&plans, "../../../Desktop/机密.md").is_none());
        assert!(resolve_under(&plans, "..\\..\\evil.md").is_none());
        assert!(resolve_under(&plans, "sub/evil.md").is_none());
        assert!(resolve_under(&plans, "C:\\Windows\\win.ini").is_none());
        assert!(resolve_under(&plans, "..").is_none());
        assert!(resolve_under(&plans, ".").is_none());
        assert!(resolve_under(&plans, "").is_none());
        assert!(resolve_under(&plans, "a\nb.md").is_none(), "控制字符一律拒绝");
    }

    #[test]
    fn 写操作只能出现在_paths_rs() {
        // 第 2 层守卫原本靠「记得调用 assert_app_owned」维持，是全项目唯一没有
        // 自动化兜底的不变量：现有守卫单测只验守卫函数判得对不对，验不了是否所有
        // 写操作都过了守卫。这条测试就是那个兜底——任何绕过 paths.rs 直接写盘的
        // 新代码都会让它变红。新增写操作请走 write_app_file / rename_app_file /
        // remove_app_file。
        const WRITE_CALLS: &[&str] = &[
            "fs::write(",
            "fs::rename(",
            "fs::remove_file(",
            "fs::remove_dir",
            "fs::create_dir",
            "fs::copy(",
            "File::create(",
            "OpenOptions",
        ];

        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src];
        let mut offenders: Vec<String> = Vec::new();

        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("src 目录应可读").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                // paths.rs 自己就是那个唯一出口
                if path.file_name().and_then(|n| n.to_str()) == Some("paths.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("源文件应可读");
                for (i, line) in text.lines().enumerate() {
                    // 只看代码，注释里提到函数名不算
                    let code = line.split("//").next().unwrap_or("");
                    if WRITE_CALLS.iter().any(|m| code.contains(m)) {
                        offenders.push(format!("{}:{} {}", path.display(), i + 1, line.trim()));
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "写操作必须走 paths.rs 的封装（它们内部已做越界断言）：
{}",
            offenders.join("
")
        );
    }

    #[test]
    fn 断言不创建目录() {
        // 断言函数不该有副作用。这里断言一个深层不存在的路径，
        // 跑完之后那些目录必须仍然不存在。
        let deep = app_data_dir().expect("应用数据目录路径应可解析").join("断言不该建这个目录");
        assert_app_owned(&deep.join("x.json"));
        assert!(!deep.exists(), "assert_app_owned 不该顺手建目录");
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

    #[cfg(unix)]
    #[test]
    fn 目录名解码_workbuddy在mac上没有前导短横() {
        // 实机确认：macOS 上 Claude 写 `-Users-a-x`，WorkBuddy 写 `Users-a-x`。
        // 少了前导 `-` 时不能再按 Windows 路径还原，否则会解成 `Users\\a\\x`。
        assert_eq!(decode_project_dir("Users-alice-Downloads-test"), "/Users/alice/Downloads/test");
        assert_eq!(decode_project_dir("Users-a-x"), "/Users/a/x");
        // 非 ASCII 目录名（本机 WorkBuddy 里真实存在）不受影响
        assert_eq!(decode_project_dir("Users-a-倒计时"), "/Users/a/倒计时");
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
