use tauri::{AppHandle, State};

use crate::error::AppError;
use crate::profile::ServerProfile;

pub fn default_onecat_repo() -> String {
    "https://github.com/chenyb999-zhcn/1Cat-vLLM.git".into()
}

fn get_profile(app: &AppHandle, profile_id: &str) -> Result<ServerProfile, AppError> {
    crate::profile::load_profiles(app)?
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| AppError::Other(format!("服务器档案不存在: {profile_id}")))
}

/// 根据工具名组装安装脚本
pub fn build_install_script(profile: &ServerProfile, tool: &str) -> Result<String, AppError> {
    let fw_dir = format!("{}/frameworks", profile.base_dir.trim_end_matches('/'));
    let onecat_repo = profile
        .onecat_repo
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_onecat_repo);
    let onecat_repo_q = onecat_repo.replace('\'', "'\\''");

    Ok(match tool {
        "modelscope" => "python3 -m pip install -U modelscope 2>&1".into(),
        "huggingface" => "python3 -m pip install -U 'huggingface_hub[cli]' 2>&1".into(),
        "vllm" => "python3 -m pip install -U vllm 2>&1".into(),
        "sglang" => "python3 -m pip install -U sglang 2>&1".into(),
        "1cat-vllm" => format!(
            "mkdir -p {fw_dir}\ncd {fw_dir}\n\
             if [ -d 1Cat-vLLM ]; then cd 1Cat-vLLM && git pull; else git clone {onecat_repo_q} 1Cat-vLLM; fi\n\
             python3 -m pip install -e . 2>&1"
        ),
        "llama-cpp" => format!(
            "mkdir -p {fw_dir}\ncd {fw_dir}\n\
             if [ ! -d llama.cpp ]; then git clone --depth 1 https://github.com/ggml-org/llama.cpp; fi\n\
             cd llama.cpp && cmake -B build >/dev/null && cmake --build build -j --target llama-server 2>&1"
        ),
        "docker-vllm" => {
            let img = profile
                .onecat_image
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "vllm/vllm-openai:latest".into())
                .replace('\'', "'\\''");
            format!("docker pull '{img}' 2>&1")
        }
        "docker-sglang" => "docker pull 'lmsysorg/sglang:latest' 2>&1".into(),
        "docker-llama" => "docker pull 'ggml-org/llama.cpp:server' 2>&1".into(),
        other => return Err(AppError::Other(format!("未知安装项: {other}"))),
    })
}

#[tauri::command]
pub fn install_preview(
    app: AppHandle,
    profile_id: String,
    tool: String,
) -> Result<String, AppError> {
    let profile = get_profile(&app, &profile_id)?;
    build_install_script(&profile, &tool)
}

#[tauri::command]
pub async fn install_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    tool: String,
) -> Result<String, AppError> {
    let profile = get_profile(&app, &profile_id)?;
    let script = build_install_script(&profile, &tool)?;
    let task_id = format!("inst-{}", chrono::Utc::now().timestamp_millis());
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id.clone()));
    };
    session.run_stream(&script, &task_id, &app).await?;
    Ok(task_id)
}
