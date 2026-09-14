//! 命令行冒烟检查：不起窗口，直接把三家适配器的解析结果打出来。
//!
//!   cargo run --example scan             只列 agent 和项目
//!   cargo run --example scan -- full     连每个项目的会话一起列
//!   cargo run --example scan -- outcome  列每个项目的成果盘点（文件改动排行）
//!   cargo run --example scan -- heat     按天活动量 + 缓存冷热对比
//!   cargo run --example scan -- find 关键词   全局搜索
//!
//! 用来验证解析逻辑对本机真实数据是否正确，改完 adapter 先跑这个再开 GUI。
use agent_workbench_lib::adapters::adapter_for;
use agent_workbench_lib::model::AgentKind;
use agent_workbench_lib::paths::agent_root;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let full = args.iter().any(|a| a == "full");
    let outcome = args.iter().any(|a| a == "outcome");

    if args.iter().any(|a| a == "heat") {
        return heat();
    }
    if let Some(i) = args.iter().position(|a| a == "find") {
        return find(args.get(i + 1).map(String::as_str).unwrap_or(""));
    }

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

            if outcome {
                let t1 = std::time::Instant::now();
                let o = adapter.project_outcome(&p.id);
                println!(
                    "      成果: {} 场 / {} 次工具 / +{} -{}{} / 聚合耗时 {:?}",
                    o.session_count,
                    o.total_tool_calls,
                    o.total_lines_added,
                    o.total_lines_removed,
                    o.total_cost_usd.map(|c| format!(" / ${c:.2}")).unwrap_or_default(),
                    t1.elapsed()
                );
                println!("      文件改动排行（共 {} 个）:", o.files.len());
                for f in o.files.iter().take(8) {
                    println!(
                        "        {:>3} 次 / {} 场  {}{}",
                        f.touches,
                        f.sessions,
                        f.rel,
                        if f.outside { "   <项目外>" } else { "" }
                    );
                }
                continue;
            }

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

/// 按天活动量，顺带对比缓存冷热两次的耗时。
fn heat() {
    for kind in AgentKind::all() {
        if agent_root(kind).map(|p| p.exists()) != Some(true) {
            continue;
        }
        let adapter = adapter_for(kind);

        let t0 = std::time::Instant::now();
        let days = adapter.daily_activity();
        let cold = t0.elapsed();

        let t1 = std::time::Instant::now();
        let _ = adapter.daily_activity();
        let warm = t1.elapsed();

        let total: u32 = days.iter().map(|d| d.events).sum();
        println!(
            "\n=== {} ===\n{} 个活动日 / {} 次活动\n首次 {:?}  缓存后 {:?}  加速 {:.1}x",
            kind.display_name(),
            days.len(),
            total,
            cold,
            warm,
            cold.as_secs_f64() / warm.as_secs_f64().max(1e-9)
        );
        for d in days.iter().rev().take(8) {
            let bar = "█".repeat(((d.events as f64).sqrt() as usize).clamp(1, 40));
            println!("  {}  {:>5}  {}", d.day, d.events, bar);
        }
    }
}

/// 全局搜索。
fn find(q: &str) {
    if q.is_empty() {
        println!("用法: cargo run --example scan -- find <关键词>");
        return;
    }
    let t0 = std::time::Instant::now();
    let hits = agent_workbench_lib::commands::search(q.to_string(), Some(20));
    println!("「{}」命中 {} 条，耗时 {:?}\n", q, hits.len(), t0.elapsed());
    for h in &hits {
        println!(
            "[{}] {} / {}\n  命中于 {}: {}\n",
            h.agent.display_name(),
            h.project_name,
            h.title,
            h.field,
            h.snippet.replace('\n', " ")
        );
    }
}
