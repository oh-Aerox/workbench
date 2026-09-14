pub mod adapters;
pub mod commands;
pub mod model;
pub mod paths;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_agents,
            commands::list_projects,
            commands::list_sessions,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
