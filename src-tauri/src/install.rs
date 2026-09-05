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

/// pip 安装：先常规安装，遇 PEP 668（externally-managed）自动重试 --break-system-packages；
/// prefix 为代理导出前缀（pip 遵守 http_proxy/https_proxy 环境变量），空 = 未启用代理
fn pip_install(pkg: &str, prefix: &str) -> String {
    format!(
        "{prefix}python3 -m pip install -U '{pkg}' 2>&1 \
          || python3 -m pip install -U '{pkg}' --break-system-packages 2>&1"
    )
}

/// 根据工具名组装安装脚本
pub fn build_install_script(
    profile: &ServerProfile,
    settings: &crate::settings::AppSettings,
    tool: &str,
) -> Result<String, AppError> {
    let fw_dir = format!("{}/frameworks", profile.base_dir.trim_end_matches('/'));
    let onecat_repo = profile
        .onecat_repo
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_onecat_repo);
    let onecat_repo_q = onecat_repo.replace('\'', "'\\''");
    let proxy = crate::settings::proxy_env_prefix(settings);

    Ok(match tool {
        "modelscope" => pip_install("modelscope", &proxy),
        "huggingface" => pip_install("huggingface_hub[cli]", &proxy),
        "vllm" => pip_install("vllm", &proxy),
        "sglang" => pip_install("sglang", &proxy),
        "1cat-vllm" => format!(
            "{proxy}mkdir -p {fw_dir}\ncd {fw_dir}\n\
              if [ -d 1Cat-vLLM ]; then cd 1Cat-vLLM && git pull; else git clone {onecat_repo_q} 1Cat-vLLM; fi\n\
              python3 -m pip install -e . 2>&1 || python3 -m pip install -e . --break-system-packages 2>&1"
        ),
        "llama-cpp" => format!(
            "{proxy}mkdir -p {fw_dir}\ncd {fw_dir}\n\
              if [ ! -d llama.cpp ]; then git clone --depth 1 https://github.com/ggml-org/llama.cpp; fi\n\
              cd llama.cpp && cmake -B build >/dev/null && cmake --build build -j --target llama-server 2>&1"
        ),
        "parser-libs" => format!(
            "{proxy}python3 -m pip install -U gguf safetensors 2>&1 \
              || python3 -m pip install -U gguf safetensors --break-system-packages 2>&1"
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
        "docker-sglang" => "docker pull 'lmsysorg/sglang:latest-cu129' 2>&1".into(),
        "docker-llama" => "docker pull 'ghcr.io/ggml-org/llama.cpp:server-cuda' 2>&1".into(),
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
    let settings = crate::settings::load_settings(&app)?;
    build_install_script(&profile, &settings, &tool)
}

#[tauri::command]
pub async fn install_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    tool: String,
) -> Result<String, AppError> {
    let profile = get_profile(&app, &profile_id)?;
    let settings = crate::settings::load_settings(&app)?;
    let script = build_install_script(&profile, &settings, &tool)?;
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    let task_id = format!("inst-{}", chrono::Utc::now().timestamp_millis());
    crate::ssh::SshSession::spawn_stream(app, profile_id, script, task_id.clone());
    Ok(task_id)
}
