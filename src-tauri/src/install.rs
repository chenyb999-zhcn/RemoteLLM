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

/// pip 自愈：缺 pip 时用官网 get-pip.py 安装（python urllib 下载，不依赖 curl；
/// 系统目录不可写时自动回退 --user）。代理由脚本前缀的 http_proxy/https_proxy 环境变量生效。
fn pip_bootstrap() -> &'static str {
    r#"if ! python3 -m pip --version >/dev/null 2>&1; then
echo "[pip] 缺少 pip，使用官网 get-pip.py 安装 ..."
python3 -c "import urllib.request; urllib.request.urlretrieve('https://bootstrap.pypa.io/get-pip.py', '$HOME/.rl_get_pip.py')" \
  && { python3 "$HOME/.rl_get_pip.py" || python3 "$HOME/.rl_get_pip.py" --user; } \
  || { echo "ERROR: get-pip.py 安装失败"; exit 1; }
fi
"#
}

/// pip 安装：先引导 pip（缺失时 get-pip.py），再常规安装，遇 PEP 668（externally-managed）
/// 自动重试 --break-system-packages；prefix 为代理导出前缀，空 = 未启用代理
fn pip_install(pkg: &str, prefix: &str) -> String {
    format!(
        "{prefix}{boot}python3 -m pip install -U '{pkg}' 2>&1 \
          || python3 -m pip install -U '{pkg}' --break-system-packages 2>&1",
        boot = pip_bootstrap()
    )
}

/// 密码登录的档案可提取登录密码，作为 llama.cpp 工具链自愈的 sudo 密码
/// （若 sudo 密码与登录密码不同，安装会失败并提示走「初始化检查」页）
fn sudo_password(profile: &ServerProfile) -> Option<&str> {
    match &profile.auth {
        crate::profile::AuthMethod::Password { password } if !password.is_empty() => Some(password),
        _ => None,
    }
}

/// 根据工具名组装安装脚本；sudo_pass 用于 llama.cpp 工具链自愈的密码 sudo 分支
/// （preview 传占位符避免泄漏真实密码）
pub fn build_install_script(
    profile: &ServerProfile,
    settings: &crate::settings::AppSettings,
    tool: &str,
    sudo_pass: Option<&str>,
) -> Result<String, AppError> {
    let fw_dir = format!("{}/frameworks", profile.base_dir.trim_end_matches('/'));
    let onecat_repo = profile
        .onecat_repo
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_onecat_repo);
    let onecat_repo_q = onecat_repo.replace('\'', "'\\''");
    let proxy = crate::settings::proxy_env_prefix(settings);
    // llama.cpp 工具链缺失时的密码 sudo 分支：root / 免密 sudo 之外的第三条路
    let llama_else = match sudo_pass {
        Some(pw) => format!(
            "{{ {u} && {i}; }} || {{ echo \"ERROR: 工具链安装失败（sudo 密码与登录密码不同或网络问题）。请先在「初始化检查」页安装 build-essential + cmake 后重试\"; exit 1; }}",
            u = crate::docker::wrap_line(
                "apt-get update -y",
                crate::docker::SudoMode::SudoPass,
                Some(pw)
            ),
            i = crate::docker::wrap_line(
                "DEBIAN_FRONTEND=noninteractive apt-get install -y build-essential cmake",
                crate::docker::SudoMode::SudoPass,
                Some(pw)
            ),
        ),
        None => "echo \"ERROR: 缺少 C++ 工具链（g++/make/cmake）。请先在「初始化检查」页安装 build-essential + cmake 后重试（或改用密码方式连接可自动安装）\"; exit 1".into(),
    };

    Ok(match tool {
        "modelscope" => pip_install("modelscope", &proxy),
        "huggingface" => pip_install("huggingface_hub[cli]", &proxy),
        "vllm" => pip_install("vllm", &proxy),
        "sglang" => pip_install("sglang", &proxy),
        "1cat-vllm" => format!(
            "{proxy}mkdir -p {fw_dir}\ncd {fw_dir}\n\
              if [ -d 1Cat-vLLM ]; then cd 1Cat-vLLM && git pull; else git clone {onecat_repo_q} 1Cat-vLLM; fi\n\
              {boot}python3 -m pip install -e . 2>&1 || python3 -m pip install -e . --break-system-packages 2>&1",
            boot = pip_bootstrap()
        ),
        "llama-cpp" => format!(
            "{proxy}mkdir -p {fw_dir}\ncd {fw_dir}\n\
              if ! {{ command -v g++ >/dev/null 2>&1 && command -v make >/dev/null 2>&1 && command -v cmake >/dev/null 2>&1; }}; then\n\
              echo \"[toolchain] 缺少 g++/make/cmake，尝试自动安装 build-essential + cmake ...\"\n\
              if [ \"$(id -u)\" = \"0\" ]; then\n\
              {{ apt-get update -y && DEBIAN_FRONTEND=noninteractive apt-get install -y build-essential cmake; }} || {{ echo \"ERROR: 工具链安装失败（apt-get）\"; exit 1; }}\n\
              elif sudo -n true 2>/dev/null; then\n\
              {{ sudo apt-get update -y && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y build-essential cmake; }} || {{ echo \"ERROR: 工具链安装失败（sudo apt-get）\"; exit 1; }}\n\
              else\n\
              {llama_else}\n\
              fi\n\
              command -v g++ >/dev/null 2>&1 && command -v make >/dev/null 2>&1 && command -v cmake >/dev/null 2>&1 || {{ echo \"ERROR: 工具链仍不完整（g++/make/cmake），请在「初始化检查」页安装 build-essential + cmake\"; exit 1; }}\n\
              fi\n\
              if [ ! -d llama.cpp ]; then git clone --depth 1 https://github.com/ggml-org/llama.cpp; fi\n\
              cd llama.cpp && cmake -B build && cmake --build build -j\"$(awk '/^MemTotal/ {{j=int($2/1024/1500); if(j<1)j=1; print j}}' /proc/meminfo)\" --target llama-server 2>&1"
        ),
        "parser-libs" => format!(
            "{proxy}{boot}python3 -m pip install -U gguf safetensors 2>&1 \
              || python3 -m pip install -U gguf safetensors --break-system-packages 2>&1",
            boot = pip_bootstrap()
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
    // 预览用占位符代替真实 sudo 密码
    build_install_script(&profile, &settings, &tool, Some("******"))
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
    let script = build_install_script(&profile, &settings, &tool, sudo_password(&profile))?;
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    let task_id = format!("inst-{}", chrono::Utc::now().timestamp_millis());
    crate::applog::info("task", &format!("install_start tool={tool} profile={profile_id}"));
    crate::ssh::SshSession::spawn_stream(app, profile_id, script, task_id.clone());
    Ok(task_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> ServerProfile {
        ServerProfile {
            id: "p1".into(),
            name: "t".into(),
            host: "1.2.3.4".into(),
            port: 22,
            user: "u".into(),
            auth: crate::profile::AuthMethod::Password { password: "x".into() },
            base_dir: "~/RemoteLLM".into(),
            models_dir: None,
            onecat_repo: None,
            onecat_image: None,
        }
    }

    #[test]
    fn llama_cpp_toolchain_guard() {
        let d = crate::settings::AppSettings::default();
        // 密码登录档案：else 分支用 printf|sudo -S 密码管道自动装工具链
        let s = build_install_script(&profile(), &d, "llama-cpp", Some("x")).unwrap();
        assert!(s.contains("command -v g++ >/dev/null 2>&1 && command -v make >/dev/null 2>&1 && command -v cmake >/dev/null 2>&1"));
        assert!(s.contains("apt-get install -y build-essential cmake"));
        assert!(s.contains("sudo DEBIAN_FRONTEND=noninteractive apt-get install -y build-essential cmake"));
        assert!(s.contains("printf '%s\\n' 'x' | sudo -S -p '' apt-get update -y"));
        assert!(s.contains("printf '%s\\n' 'x' | sudo -S -p '' DEBIAN_FRONTEND=noninteractive apt-get install -y build-essential cmake"));
        // 密钥登录档案：给出「初始化检查」指引
        let mut k = profile();
        k.auth = crate::profile::AuthMethod::Key { key_path: "~/.ssh/id".into(), passphrase: None };
        let s2 = build_install_script(&k, &d, "llama-cpp", None).unwrap();
        assert!(s2.contains("请在「初始化检查」页安装 build-essential + cmake"));
        assert!(!s2.contains("sudo -S"));
        // 白名单外不得出现其他 apt 包
        assert!(!s.contains("apt-get install -y g++"));
        // 配置输出不再吞掉；编译并发按内存自适应（每 job 约 1.5G）
        assert!(!s.contains("cmake -B build >/dev/null"));
        assert!(s.contains("cmake -B build && cmake --build build -j\"$(awk '/^MemTotal/ {j=int($2/1024/1500); if(j<1)j=1; print j}' /proc/meminfo)\" --target llama-server"));
    }

    #[test]
    fn pip_tools_have_bootstrap() {
        let d = crate::settings::AppSettings::default();
        for tool in ["vllm", "sglang", "modelscope", "huggingface", "1cat-vllm", "parser-libs"] {
            let s = build_install_script(&profile(), &d, tool, None).unwrap();
            assert!(s.contains("python3 -m pip --version"), "tool={tool}");
            assert!(s.contains("bootstrap.pypa.io/get-pip.py"), "tool={tool}");
            assert!(!s.contains("apt-get"), "tool={tool}");
        }
        // llama-cpp 纯 cmake 编译，不需要 pip 引导
        let s = build_install_script(&profile(), &d, "llama-cpp", None).unwrap();
        assert!(!s.contains("get-pip.py"));
    }
}
