use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AuthMethod {
    Password { password: String },
    Key { key_path: String, passphrase: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    pub auth: AuthMethod,
    /// 服务器上的工作根目录（模型、PID、日志都放这里）
    #[serde(default = "default_base_dir")]
    pub base_dir: String,
    /// 独立模型目录（空 = 用全局设置 modelDir，再空 = baseDir/models）
    #[serde(default)]
    pub models_dir: Option<String>,
    /// 1Cat-vLLM git 仓库地址（原生安装用）
    #[serde(default)]
    pub onecat_repo: Option<String>,
    /// 1Cat-vLLM Docker 镜像
    #[serde(default)]
    pub onecat_image: Option<String>,
}

fn default_port() -> u16 {
    22
}

fn default_base_dir() -> String {
    "~/RemoteLLM".into()
}

/// 路径归一化：去掉尾部斜杠；前导 ~/ 换成 $HOME（bash 对引号内的 ~ 不做展开，
/// 直接传 "~/x" 给 cd 会失败，而 $HOME 在 shell 命令字符串中会被展开）
fn shell_path(p: &str) -> String {
    let t = p.trim().trim_end_matches('/');
    if t == "~" {
        "$HOME".into()
    } else if let Some(rest) = t.strip_prefix("~/") {
        format!("$HOME/{rest}")
    } else {
        t.to_string()
    }
}

impl ServerProfile {
    fn root(&self) -> &str {
        self.base_dir.trim_end_matches('/')
    }
    pub fn default_models_dir(&self) -> String {
        shell_path(&format!("{}/models", self.root()))
    }
    pub fn run_dir(&self) -> String {
        shell_path(&format!("{}/run", self.root()))
    }
    pub fn logs_dir(&self) -> String {
        shell_path(&format!("{}/logs", self.root()))
    }
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// 生效的模型目录：档案覆盖 > 全局设置 > baseDir/models
pub fn effective_models_dir(profile: &ServerProfile, settings: &crate::settings::AppSettings) -> String {
    let dir = profile
        .models_dir
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .or_else(|| {
            let g = settings.model_dir.trim();
            if g.is_empty() {
                None
            } else {
                Some(g)
            }
        })
        .map(str::to_string)
        .unwrap_or_else(|| profile.default_models_dir());
    shell_path(&dir)
}

const STORE_KEY: &str = "profiles";

pub fn load_profiles(app: &AppHandle) -> Result<Vec<ServerProfile>, AppError> {
    let store = app
        .store("remotellm.json")
        .map_err(|e| AppError::Other(e.to_string()))?;
    let value = store.get(STORE_KEY).unwrap_or_default();
    let profiles: Vec<ServerProfile> = serde_json::from_value(value).unwrap_or_default();
    Ok(profiles)
}

fn save_store(app: &AppHandle, profiles: &[ServerProfile]) -> Result<(), AppError> {
    let store = app
        .store("remotellm.json")
        .map_err(|e| AppError::Other(e.to_string()))?;
    store.set(
        STORE_KEY,
        serde_json::to_value(profiles).map_err(|e| AppError::Other(e.to_string()))?,
    );
    store.save().map_err(|e| AppError::Other(e.to_string()))
}

#[tauri::command]
pub fn list_profiles(app: AppHandle) -> Result<Vec<ServerProfile>, AppError> {
    load_profiles(&app)
}

#[tauri::command]
pub async fn save_profile(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile: ServerProfile,
) -> Result<Vec<ServerProfile>, AppError> {
    let mut profiles = load_profiles(&app)?;
    let id = profile.id.clone();
    crate::applog::info(
        "app",
        &format!(
            "profile saved name={} host={} user={} auth={}",
            profile.name,
            profile.addr(),
            profile.user,
            match &profile.auth {
                crate::profile::AuthMethod::Password { .. } => "password",
                crate::profile::AuthMethod::Key { .. } => "key",
            }
        ),
    );
    if let Some(p) = profiles.iter_mut().find(|p| p.id == id) {
        *p = profile;
    } else {
        profiles.push(profile);
    }
    save_store(&app, &profiles)?;
    // 配置可能变了，断开旧连接
    let _ = state.conns.lock().await.remove(&id);
    Ok(profiles)
}

#[tauri::command]
pub async fn delete_profile(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<Vec<ServerProfile>, AppError> {
    let mut profiles = load_profiles(&app)?;
    profiles.retain(|p| p.id != id);
    save_store(&app, &profiles)?;
    let _ = state.conns.lock().await.remove(&id);
    Ok(profiles)
}
