// Windows release 构建下不弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    agent_workbench_lib::run()
}
