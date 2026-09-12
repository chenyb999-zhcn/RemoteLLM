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
    /// UI 语言：zh | en
    #[serde(default = "default_language")]
    pub language: String,
    /// 启用下载代理（服务器侧 pip/模型下载/git clone/docker daemon）
    #[serde(default)]
    pub proxy_enabled: bool,
    /// 代理地址，如 http://192.168.1.10:7890
    #[serde(default)]
    pub proxy_url: String,
    /// pip 镜像源 id（tuna/aliyun/ustc/huawei/tencent/pypi），默认 tuna
    #[serde(default = "default_pip_index")]
    pub pip_index: String,
    /// uv python install 3.12 的下载镜像前缀（替换官方 GitHub 前缀，空 = 官方）
    #[serde(default)]
    pub uv_python_mirror: String,
    /// deb(apt) 镜像源 id（tuna/aliyun/ustc/huawei/tencent/official），默认 tuna
    #[serde(default = "default_deb_mirror")]
    pub deb_mirror: String,
    /// 用户添加的自定义框架镜像（框架管理页「添加框架镜像」）
    #[serde(default)]
    pub custom_frameworks: Vec<CustomFramework>,
}

/// 自定义框架镜像：label 显示名 + image 镜像地址
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomFramework {
    pub label: String,
    pub image: String,
}

fn default_pip_index() -> String {
    "tuna".into()
}

fn default_deb_mirror() -> String {
    "tuna".into()
}

/// pip 镜像 id → --index-url 参数（空/未知回退清华）
pub fn pip_index_arg(id: &str) -> String {
    let url = match id.trim() {
        "aliyun" => "https://mirrors.aliyun.com/pypi/simple/",
        "ustc" => "https://mirrors.ustc.edu.cn/pypi/simple/",
        "huawei" => "https://mirrors.huaweicloud.com/repository/pypi/simple/",
        "tencent" => "https://mirrors.cloud.tencent.com/pypi/simple/",
        "pypi" => "https://pypi.org/simple/",
        // tuna 或未知 → 清华
        _ => "https://pypi.tuna.tsinghua.edu.cn/simple/",
    };
    format!("--index-url {url}")
}

/// deb(apt) 镜像 id → 主源 base URL（空/official/未知回退官方 archive.ubuntu.com）
pub fn deb_mirror_base(id: &str) -> String {
    match id.trim() {
        "tuna" => "https://mirrors.tuna.tsinghua.edu.cn/ubuntu".into(),
        "aliyun" => "https://mirrors.aliyun.com/ubuntu".into(),
        "ustc" => "https://mirrors.ustc.edu.cn/ubuntu".into(),
        "huawei" => "https://mirrors.huaweicloud.com/ubuntu".into(),
        "tencent" => "https://mirrors.cloud.tencent.com/ubuntu".into(),
        // official 或未知 → 官方
        _ => "http://archive.ubuntu.com/ubuntu".into(),
    }
}

/// 生成切换 apt 源的独立 shell 片段（幂等：只剩目标镜像行则跳过），自带提权
/// （root / 免密 sudo / 密码 sudo 三分支），放在 apt-get 命令之前执行。
/// security 源必须与主源指向同一 base（5 个镜像均在 <base>/dists/<suite>-security/，
/// 不存在 <base>/security 子路径）；第 3 条 sed 修复旧版本误改的 {base}/security 行。
/// official/空/未知 → 返回空串（不改源）。pw 为登录密码（密码 sudo 分支用），可空。
pub fn deb_mirror_prefix(id: &str, pw: Option<&str>) -> String {
    let base = deb_mirror_base(id);
    if base.starts_with("http://archive.ubuntu.com") {
        return String::new();
    }
    // 官方/区域主源（archive 前可有 cn./us. 等区域前缀，裸 archive 亦可）→ base；
    // 官方 security → base；修复旧版本误写的 {base}/security 行
    let sed = format!(
        "sed -i 's#http://\\([a-z]*\\.\\)\\?archive\\.ubuntu\\.com/ubuntu#{base}#g; s#http://security\\.ubuntu\\.com/ubuntu#{base}#g; s#{base}/security#{base}#g' /etc/apt/sources.list"
    );
    // 仅当残留官方行或坏行时才执行 sed（幂等）；提权三分支：root 直接 / 免密 sudo / 密码 sudo；
    // 都失败则告警不阻塞（apt 沿用当前源）
    let pw_branch = match pw {
        Some(p) if !p.is_empty() => {
            let p = p.replace('\'', "'\\''");
            format!("elif [ -n \"$(command -v sudo)\" ]; then printf '%s\\n' '{p}' | sudo -S -p '' {sed} || echo '[deb] 切换镜像源失败（sudo 密码不符），沿用当前源';")
        }
        _ => String::new(),
    };
    format!(
        "if grep -qE 'archive\\.ubuntu\\.com|security\\.ubuntu\\.com|{base}/security' /etc/apt/sources.list 2>/dev/null; then \
          if [ \"$(id -u)\" = \"0\" ]; then {sed} || echo '[deb] 切换镜像源失败，沿用当前源'; \
          elif sudo -n true 2>/dev/null; then sudo {sed} || echo '[deb] 切换镜像源失败，沿用当前源'; \
          {pw_branch} \
          fi; \
          fi; "
    )
}

fn default_dark() -> bool {
    true
}

fn default_language() -> String {
    "zh".into()
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
            language: default_language(),
            proxy_enabled: false,
            proxy_url: String::new(),
            pip_index: default_pip_index(),
            uv_python_mirror: String::new(),
            deb_mirror: default_deb_mirror(),
            custom_frameworks: Vec::new(),
        }
    }
}

/// 生效的代理地址（启用且非空才返回）
pub fn effective_proxy(s: &AppSettings) -> Option<&str> {
    let u = s.proxy_url.trim();
    (s.proxy_enabled && !u.is_empty()).then_some(u)
}

/// uv python install 3.12 的镜像前缀（UV_PYTHON_INSTALL_MIRROR，空 = 默认 GitHub）
pub fn uv_python_mirror_prefix(s: &AppSettings) -> String {
    let u = s.uv_python_mirror.trim();
    if u.is_empty() {
        String::new()
    } else {
        let q = u.replace('\'', "'\\''");
        format!("export UV_PYTHON_INSTALL_MIRROR='{q}'\n")
    }
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
    crate::applog::info(
        "app",
        &format!(
            "settings saved proxy={} hf_token={}",
            if settings.proxy_enabled { "on" } else { "off" },
            if settings.hf_token.trim().is_empty() { "empty" } else { "set" }
        ),
    );
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

    #[test]
    fn uv_mirror_prefix_empty_by_default() {
        let s = AppSettings::default();
        assert_eq!(uv_python_mirror_prefix(&s), "");
        let mut s = AppSettings::default();
        s.uv_python_mirror = "   ".into();
        assert_eq!(uv_python_mirror_prefix(&s), "");
    }

    #[test]
    fn uv_mirror_prefix_exports_value() {
        let mut s = AppSettings::default();
        s.uv_python_mirror = "https://m.example/pbs".into();
        assert_eq!(
            uv_python_mirror_prefix(&s),
            "export UV_PYTHON_INSTALL_MIRROR='https://m.example/pbs'\n"
        );
    }

    #[test]
    fn uv_mirror_prefix_escapes_quote() {
        let mut s = AppSettings::default();
        s.uv_python_mirror = "http://a'b/pbs".into();
        assert!(uv_python_mirror_prefix(&s).contains(r#"'http://a'\''b/pbs'"#));
    }

    #[test]
    fn pip_index_arg_resolves_known_and_fallback() {
        assert!(pip_index_arg("tuna").contains("pypi.tuna.tsinghua.edu.cn"));
        assert!(pip_index_arg("aliyun").contains("mirrors.aliyun.com/pypi"));
        assert!(pip_index_arg("ustc").contains("mirrors.ustc.edu.cn/pypi"));
        assert!(pip_index_arg("huawei").contains("mirrors.huaweicloud.com"));
        assert!(pip_index_arg("tencent").contains("mirrors.cloud.tencent.com"));
        assert!(pip_index_arg("pypi").contains("pypi.org/simple"));
        // 空/未知回退清华
        assert!(pip_index_arg("").contains("pypi.tuna.tsinghua.edu.cn"));
        assert!(pip_index_arg("bogus").contains("pypi.tuna.tsinghua.edu.cn"));
    }

    #[test]
    fn deb_mirror_base_resolves_known_and_official() {
        assert!(deb_mirror_base("tuna").contains("mirrors.tuna.tsinghua.edu.cn/ubuntu"));
        assert!(deb_mirror_base("aliyun").contains("mirrors.aliyun.com/ubuntu"));
        assert!(deb_mirror_base("ustc").contains("mirrors.ustc.edu.cn/ubuntu"));
        assert!(deb_mirror_base("huawei").contains("mirrors.huaweicloud.com/ubuntu"));
        assert!(deb_mirror_base("tencent").contains("mirrors.cloud.tencent.com/ubuntu"));
        // official/空/未知 → 官方 archive
        assert!(deb_mirror_base("official").contains("archive.ubuntu.com"));
        assert!(deb_mirror_base("").contains("archive.ubuntu.com"));
        assert!(deb_mirror_base("bogus").contains("archive.ubuntu.com"));
    }

    #[test]
    fn deb_mirror_prefix_official_is_empty() {
        assert_eq!(deb_mirror_prefix("official", None), "");
        assert_eq!(deb_mirror_prefix("", None), "");
        assert_eq!(deb_mirror_prefix("bogus", None), "");
    }

    #[test]
    fn deb_mirror_prefix_tuna_rewrites_sources() {
        let s = deb_mirror_prefix("tuna", None);
        assert!(s.contains("mirrors.tuna.tsinghua.edu.cn/ubuntu"));
        assert!(s.contains("sed -i"));
        assert!(s.contains("archive\\.ubuntu\\.com"));
        // security 源指向与主源同一 base（不得拼 /security，镜像上不存在该子路径会 404）
        assert!(!s.contains("ubuntu/security#g"));
        // 修复旧版本误写的 {base}/security 行
        assert!(s.contains(
            "s#https://mirrors.tuna.tsinghua.edu.cn/ubuntu/security#https://mirrors.tuna.tsinghua.edu.cn/ubuntu#g"
        ));
        // 幂等守卫：仅当残留官方行或坏行时才执行
        assert!(s.contains(
            "grep -qE 'archive\\.ubuntu\\.com|security\\.ubuntu\\.com|https://mirrors.tuna.tsinghua.edu.cn/ubuntu/security'"
        ));
    }

    #[test]
    fn deb_mirror_prefix_password_branch() {
        let s = deb_mirror_prefix("aliyun", Some("p@ss'word"));
        assert!(s.contains("sudo -S -p ''"));
        // 单引号转义
        assert!(s.contains(r#"'p@ss'\''word'"#));
    }
}
