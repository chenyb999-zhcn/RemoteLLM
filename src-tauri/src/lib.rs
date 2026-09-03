mod envcheck;
mod error;
mod frameworks;
mod install;
mod metrics;
mod metrics_parse;
mod models;
mod profile;
mod ssh;

use std::collections::HashMap;

pub struct AppState {
    pub conns: tokio::sync::Mutex<HashMap<String, ssh::SshSession>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(AppState {
            conns: tokio::sync::Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            profile::list_profiles,
            profile::save_profile,
            profile::delete_profile,
            ssh::ssh_connect,
            ssh::ssh_disconnect,
            ssh::ssh_is_connected,
            ssh::ssh_run,
            ssh::ssh_run_stream,
            envcheck::env_check_cmd,
            metrics::gpu_poll,
            metrics::gpu_proc_poll,
            metrics::metrics_poll,
            frameworks::list_instances,
            frameworks::save_instance,
            frameworks::delete_instance,
            frameworks::detect_frameworks,
            frameworks::preview_command,
            frameworks::instance_start,
            frameworks::instance_stop,
            frameworks::instance_status,
            frameworks::instance_logs,
            models::search_models,
            models::list_local_models,
            models::model_download_start,
            models::model_delete,
            install::install_preview,
            install::install_start
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
