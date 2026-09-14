//! 命令行冒烟检查：不起窗口，直接把三家适配器的解析结果打出来。
//!
//!   cargo run --example scan            只列 agent 和项目
//!   cargo run --example scan -- full    连每个项目的会话一起列
//!
//! 用来验证解析逻辑对本机真实数据是否正确，改完 adapter 先跑这个再开 GUI。
use agent_workbench_lib::adapters::adapter_for;
use agent_workbench_lib::model::AgentKind;
use agent_workbench_lib::paths::agent_root;

fn main() {
    let full = std::env::args().any(|a| a == "full");

    for kind in AgentKind::all() {
        let root = agent_root(kind);
        let installed = root.as_ref().map(|p| p.exists()).unwrap_or(false);
        println!("\n=== {} ===", kind.display_name());
        println!("根目录: {}", root.map(|p| p.display().to_string()).unwrap_or_default());

        if !installed {
            println!("未安装，跳过");
            continue;
        }

        let adapter = adapter_for(kind);
        let t0 = std::time::Instant::now();
        let projects = adapter.list_projects();
        println!("{} 个项目，扫描耗时 {:?}", projects.len(), t0.elapsed());

        for p in &projects {
            println!(
                "  [{}] {} 场会话{}\n      {}",
                p.name,
                p.session_count,
                if p.running { "  <进行中>" } else { "" },
                p.path
            );

            if !full {
                continue;
            }
            let t1 = std::time::Instant::now();
            let sessions = adapter.list_sessions(&p.id);
            println!("      解析 {} 场耗时 {:?}", sessions.len(), t1.elapsed());
            for s in &sessions {
                println!(
                    "        · {}\n          {} 轮 / {} 次工具 / {} 文件 / {:.1} 分钟 / +{} -{}{} [{}]",
                    s.title,
                    s.user_turns,
                    s.tool_calls,
                    s.files_touched,
                    s.wall_ms as f64 / 60_000.0,
                    s.lines_added,
                    s.lines_removed,
                    s.cost_usd.map(|c| format!(" / ${c:.2}")).unwrap_or_default(),
                    s.models.join(", ")
                );
            }
        }
    }
}
