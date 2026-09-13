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
    /// 档案级代理覆盖：None = 跟随全局设置；Some("") = 强制不走代理；Some(url) = 用指定代理
    /// （不同服务器网络环境不同：如内网机需代理、WSL 直连外网反而不能走内网代理）
    #[serde(default)]
    pub proxy: Option<String>,
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

/// 档案级生效代理：profile.proxy 覆盖 > 全局设置（proxy_enabled + proxy_url）
/// None = 不走代理
pub fn effective_proxy(profile: &ServerProfile, settings: &crate::settings::AppSettings) -> Option<String> {
    match &profile.proxy {
        Some(p) => {
            let t = p.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        None => crate::settings::effective_proxy(settings).map(str::to_string),
    }
}

/// 服务器侧命令的代理导出前缀（档案级：profile.proxy 覆盖 > 全局）；无代理时返回空串
pub fn proxy_env_prefix(profile: &ServerProfile, settings: &crate::settings::AppSettings) -> String {
    match effective_proxy(profile, settings) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppSettings;

    fn prof(proxy: Option<String>) -> ServerProfile {
        ServerProfile {
            id: "p1".into(),
            name: "t".into(),
            host: "127.0.0.1".into(),
            port: 22,
            user: "u".into(),
            auth: AuthMethod::Password {
                password: "p".into(),
            },
            base_dir: "~/RemoteLLM".into(),
            models_dir: None,
            onecat_repo: None,
            onecat_image: None,
            proxy,
        }
    }

    fn global(on: bool, url: &str) -> AppSettings {
        let mut s = AppSettings::default();
        s.proxy_enabled = on;
        s.proxy_url = url.into();
        s
    }

    #[test]
    fn profile_proxy_none_inherits_global() {
        assert_eq!(
            effective_proxy(&prof(None), &global(true, "http://g:7890")),
            Some("http://g:7890".into())
        );
        assert_eq!(effective_proxy(&prof(None), &global(false, "http://g:7890")), None);
    }

    #[test]
    fn profile_proxy_empty_forces_off() {
        // 档案显式空串 = 强制不走代理，即使全局启用（wsl2 场景：全局代理在内网，WSL 不可达）
        assert_eq!(
            effective_proxy(&prof(Some(String::new())), &global(true, "http://g:7890")),
            None
        );
        assert_eq!(
            effective_proxy(&prof(Some("   ".into())), &global(true, "http://g:7890")),
            None
        );
    }

    #[test]
    fn profile_proxy_custom_wins() {
        assert_eq!(
            effective_proxy(&prof(Some("http://c:1080".into())), &global(true, "http://g:7890")),
            Some("http://c:1080".into())
        );
        // 全局未启用时档案自定义仍生效
        assert_eq!(
            effective_proxy(&prof(Some("http://c:1080".into())), &global(false, "")),
            Some("http://c:1080".into())
        );
    }

    #[test]
    fn profile_proxy_env_prefix() {
        // 全局启用 + 档案未覆盖 → 导出全套代理变量
        let s = proxy_env_prefix(&prof(None), &global(true, "http://192.168.31.10:7890"));
        for v in [
            "http_proxy='http://192.168.31.10:7890'",
            "https_proxy='http://192.168.31.10:7890'",
            "HTTP_PROXY='http://192.168.31.10:7890'",
            "HTTPS_PROXY='http://192.168.31.10:7890'",
            "no_proxy='localhost,127.0.0.1'",
            "NO_PROXY='localhost,127.0.0.1'",
        ] {
            assert!(s.contains(v), "missing {v} in {s}");
        }
        assert!(s.ends_with("; "));
        // 单引号转义
        let s2 = proxy_env_prefix(&prof(None), &global(true, "http://a'b:1"));
        assert!(s2.contains(r#"'http://a'\''b:1'"#));
        // 档案强制关 / 全局未启用 → 空前缀
        assert!(proxy_env_prefix(&prof(Some(String::new())), &global(true, "http://g:7890")).is_empty());
        assert!(proxy_env_prefix(&prof(None), &global(false, "")).is_empty());
        // 档案自定义生效
        let s3 = proxy_env_prefix(&prof(Some("http://c:1080".into())), &global(false, ""));
        assert!(s3.contains("http_proxy='http://c:1080'"));
    }

    #[test]
    fn profile_proxy_field_optional_in_json() {
        // 旧档案 JSON 无 proxy 字段 → 反序列化为 None（向后兼容）
        let json = r#"{"id":"a","name":"n","host":"h","port":22,"user":"u","auth":{"type":"password","password":"p"}}"#;
        let p: ServerProfile = serde_json::from_str(json).unwrap();
        assert_eq!(p.proxy, None);
        let mut p2 = p.clone();
        p2.proxy = Some("http://x:1".into());
        let v = serde_json::to_value(&p2).unwrap();
        assert_eq!(v["proxy"], "http://x:1");
    }
}
