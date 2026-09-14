//! 监听 agent 数据目录，有变动就通知前端刷新。
//!
//! 只订阅文件系统事件，不读也不写被监听的文件——真正的重新解析发生在前端
//! 收到通知、主动重新调命令的时候，届时仍然走 `open_readonly`。
//!
//! agent 写会话文件是高频的（一次工具调用就可能追加多行），所以必须去抖：
//! 攒够 `DEBOUNCE` 这段安静期再发一次通知，否则前端会被刷爆。
use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

use crate::model::AgentKind;
use crate::paths::agent_root;

/// 安静期。攒到这么久没有新事件才发通知。
const DEBOUNCE: Duration = Duration::from_millis(1200);

/// 去抖的绝对上限。只看安静期会有一个要命的退化：agent 高频写入时事件间隔
/// 持续小于 `DEBOUNCE`（一次工具调用就连写多行，正好落在这个区间），循环就
/// 无限延长，通知永远发不出去——表现为「agent 越忙，UI 越不刷新」，恰好和
/// 用户的期待相反。攒够这么久就强制发一次，之后重新开始攒。
const MAX_DEBOUNCE: Duration = Duration::from_secs(5);

/// 判断变动路径属于哪个 agent。
fn owner_of(path: &Path) -> Option<AgentKind> {
    let s = path.to_string_lossy().to_lowercase();
    AgentKind::all().into_iter().find(|k| {
        agent_root(*k)
            .map(|r| s.starts_with(&r.to_string_lossy().to_lowercase()))
            .unwrap_or(false)
    })
}

/// 在后台线程启动监听。失败不致命——退化成手动刷新即可，不该让应用起不来。
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("文件监听启动失败，已退化为手动刷新: {e}");
                return;
            }
        };

        // 每家只监听存放会话的子目录，不去递归整个 agent 根目录——
        // 根目录下还有缓存、日志等高频写入的东西，会产生大量无关事件
        for kind in AgentKind::all() {
            let Some(root) = agent_root(kind) else { continue };
            let targets: &[&str] = match kind {
                AgentKind::Claude | AgentKind::WorkBuddy => &["projects"],
                AgentKind::Codex => &["sessions", "archived_sessions"],
            };
            for t in targets {
                let dir = root.join(t);
                if dir.exists() {
                    let _ = watcher.watch(&dir, RecursiveMode::Recursive);
                }
            }
        }

        // 去抖循环：先阻塞等第一个事件，然后不断用短超时收尾，
        // 直到攒够一段安静期再统一发通知
        loop {
            let Ok(first) = rx.recv() else { return };

            let mut changed: HashSet<AgentKind> = HashSet::new();
            let mut note = |ev: notify::Result<notify::Event>| {
                if let Ok(ev) = ev {
                    for p in &ev.paths {
                        if let Some(k) = owner_of(p) {
                            changed.insert(k);
                        }
                    }
                }
            };
            note(first);
            let deadline = std::time::Instant::now() + MAX_DEBOUNCE;
            loop {
                // 安静期和绝对上限取较小者：谁先到就停止攒事件
                let wait = DEBOUNCE.min(deadline.saturating_duration_since(std::time::Instant::now()));
                if wait.is_zero() {
                    break;
                }
                match rx.recv_timeout(wait) {
                    Ok(ev) => note(ev),
                    Err(_) => break,
                }
            }

            for kind in changed {
                let _ = app.emit("agent-data-changed", kind.id());
            }
        }
    });
}
