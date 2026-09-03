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

impl ServerProfile {
    fn root(&self) -> &str {
        self.base_dir.trim_end_matches('/')
    }
    pub fn models_dir(&self) -> String {
        format!("{}/models", self.root())
    }
    pub fn run_dir(&self) -> String {
        format!("{}/run", self.root())
    }
    pub fn logs_dir(&self) -> String {
        format!("{}/logs", self.root())
    }
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
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
