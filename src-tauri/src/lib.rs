mod applog;
mod docker;
mod envcheck;
mod error;
mod frameworks;
mod gpumgmt;
mod initcheck;
mod install;
mod metrics;
mod metrics_parse;
mod models;
mod profile;
mod settings;
mod ssh;

use std::collections::HashMap;

use tauri::Manager;

pub struct AppState {
    pub conns: tokio::sync::Mutex<HashMap<String, ssh::SshSession>>,
    /// 本地模型解析结果缓存（profile_id -> 指纹+列表），轻扫指纹一致时避免全量头部解析
    pub model_cache: std::sync::Mutex<HashMap<String, models::ModelCacheEntry>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(AppState {
            conns: tokio::sync::Mutex::new(HashMap::new()),
            model_cache: std::sync::Mutex::new(HashMap::new()),
        })
        .setup(|app| {
            let log_dir = app.path().app_log_dir().ok();
            applog::init(log_dir);
            applog::info("app", &format!("started v{}", env!("CARGO_PKG_VERSION")));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = models::refresh_ms_hotlist_if_stale(&handle).await;
                let _ = models::refresh_ms_fulllist_if_stale(&handle).await;
            });
            Ok(())
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
            frameworks::instance_log_total_lines,
            models::search_models,
            models::list_repo_files,
            models::check_download_tools,
            models::check_parser_libs,
            models::list_local_models,
            models::model_download_start,
            models::model_delete,
            settings::get_settings,
            settings::save_settings,
            install::install_preview,
            install::install_start,
            docker::check_docker,
            docker::list_docker_images,
            docker::docker_install_preview,
            docker::docker_install_start,
            docker::docker_authorize_start,
            docker::docker_proxy_preview,
            docker::docker_proxy_start,
            docker::docker_pull_start,
            docker::docker_gpu_test,
            docker::sudo_mode_check,
            initcheck::server_init_check,
            initcheck::apt_install_preview,
            initcheck::apt_install_start,
            gpumgmt::gpu_query,
            gpumgmt::gpu_kill,
            gpumgmt::gpu_set_preview,
            gpumgmt::gpu_set_start
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
