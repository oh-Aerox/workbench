pub mod adapters;
pub mod commands;
pub mod index;
pub mod model;
pub mod paths;
pub mod watcher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            watcher::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_agents,
            commands::list_projects,
            commands::list_sessions,
            commands::project_outcome,
            commands::agent_activity,
            commands::search,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
