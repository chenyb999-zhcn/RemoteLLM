use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

fn default_source() -> String {
    "modelscope".into()
}

fn default_poll_ms() -> u32 {
    3000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// 默认模型资源站: modelscope | huggingface
    #[serde(default = "default_source")]
    pub default_model_source: String,
    /// 全局默认模型目录（空 = 各服务器 baseDir/models）
    #[serde(default)]
    pub model_dir: String,
    /// HuggingFace 镜像端点（服务器端下载用，如 https://hf-mirror.com，空 = 官方）
    #[serde(default)]
    pub hf_endpoint: String,
    #[serde(default)]
    pub hf_token: String,
    /// 仪表盘轮询间隔（毫秒）
    #[serde(default = "default_poll_ms")]
    pub poll_interval_ms: u32,
    /// 上次连接的服务器（启动自动连接用）
    #[serde(default)]
    pub last_profile_id: String,
    /// 启动时自动连接上次使用的服务器
    #[serde(default)]
    pub auto_connect: bool,
    /// 深色主题
    #[serde(default = "default_dark")]
    pub dark_theme: bool,
    /// 启用下载代理（服务器侧 pip/模型下载/git clone/docker daemon）
    #[serde(default)]
    pub proxy_enabled: bool,
    /// 代理地址，如 http://192.168.1.10:7890
    #[serde(default)]
    pub proxy_url: String,
}

fn default_dark() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_model_source: default_source(),
            model_dir: String::new(),
            hf_endpoint: String::new(),
            hf_token: String::new(),
            poll_interval_ms: default_poll_ms(),
            last_profile_id: String::new(),
            auto_connect: false,
            dark_theme: true,
            proxy_enabled: false,
            proxy_url: String::new(),
        }
    }
}

/// 生效的代理地址（启用且非空才返回）
pub fn effective_proxy(s: &AppSettings) -> Option<&str> {
    let u = s.proxy_url.trim();
    (s.proxy_enabled && !u.is_empty()).then_some(u)
}

/// 服务器侧命令的代理导出前缀：http_proxy/https_proxy 大小写全套 + no_proxy
/// 未启用代理时返回空串
pub fn proxy_env_prefix(s: &AppSettings) -> String {
    match effective_proxy(s) {
        Some(u) => {
            let u = u.replace('\'', "'\\''");
            format!(
                "export http_proxy='{u}' https_proxy='{u}' HTTP_PROXY='{u}' HTTPS_PROXY='{u}' \
                 no_proxy='localhost,127.0.0.1' NO_PROXY='localhost,127.0.0.1'; "
            )
        }
        None => String::new(),
    }
}

const STORE_KEY: &str = "settings";

pub fn load_settings(app: &AppHandle) -> Result<AppSettings, AppError> {
    let store = app
        .store("remotellm.json")
        .map_err(|e| AppError::Other(e.to_string()))?;
    let value = store.get(STORE_KEY).unwrap_or_default();
    if value.is_null() {
        return Ok(AppSettings::default());
    }
    let s: AppSettings = serde_json::from_value(value).unwrap_or_default();
    Ok(s)
}

pub fn persist_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), AppError> {
    let store = app
        .store("remotellm.json")
        .map_err(|e| AppError::Other(e.to_string()))?;
    store.set(
        STORE_KEY,
        serde_json::to_value(settings).map_err(|e| AppError::Other(e.to_string()))?,
    );
    store.save().map_err(|e| AppError::Other(e.to_string()))
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, AppError> {
    load_settings(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<AppSettings, AppError> {
    persist_settings(&app, &settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_prefix_disabled_or_empty() {
        let mut s = AppSettings::default();
        assert_eq!(proxy_env_prefix(&s), "");
        s.proxy_enabled = true;
        assert_eq!(proxy_env_prefix(&s), "");
    }

    #[test]
    fn proxy_prefix_exports_all_variants() {
        let mut s = AppSettings::default();
        s.proxy_enabled = true;
        s.proxy_url = "http://192.168.31.10:7890".into();
        let p = proxy_env_prefix(&s);
        for v in [
            "http_proxy='http://192.168.31.10:7890'",
            "https_proxy='http://192.168.31.10:7890'",
            "HTTP_PROXY='http://192.168.31.10:7890'",
            "HTTPS_PROXY='http://192.168.31.10:7890'",
            "no_proxy='localhost,127.0.0.1'",
            "NO_PROXY='localhost,127.0.0.1'",
        ] {
            assert!(p.contains(v), "missing {v} in {p}");
        }
        assert!(p.ends_with("; "));
    }

    #[test]
    fn proxy_prefix_escapes_quote() {
        let mut s = AppSettings::default();
        s.proxy_enabled = true;
        s.proxy_url = "http://a'b:1".into();
        assert!(proxy_env_prefix(&s).contains(r#"'http://a'\''b:1'"#));
    }
}
