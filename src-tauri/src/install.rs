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
/// 自动重试 --break-system-packages；prefix 为代理导出前缀，空 = 未启用代理；
/// idx 为 --index-url 参数（由设置页镜像源决定）
fn pip_install(pkg: &str, prefix: &str, idx: &str) -> String {
    format!(
        "{prefix}{boot}python3 -m pip install -U {idx} '{pkg}' 2>&1 \
          || python3 -m pip install -U {idx} '{pkg}' --break-system-packages 2>&1",
        boot = pip_bootstrap()
    )
}

/// sglang 专用 pip 安装：sglang 依赖 outlines==0.1.11 → outlines_core==0.1.26，
/// 该版本在清华源只有 sdist（无 cp314 wheel），需源码编译。其 PyO3 0.22.6 最高支持
/// Python 3.13，在 Python 3.14 上必须设 PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 才能
/// 用稳定 ABI 编译通过；同时确保 cargo 在 PATH（rustup 装在 ~/.cargo/bin，未必在 PATH）。
fn pip_install_sglang(prefix: &str, idx: &str) -> String {
    format!(
        "{prefix}export PATH=\"$HOME/.cargo/bin:$PATH\"\n\
         export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1\n\
         {boot}python3 -m pip install -U {idx} 'sglang' 2>&1 \
           || python3 -m pip install -U {idx} 'sglang' --break-system-packages 2>&1",
        boot = pip_bootstrap()
    )
}

/// pip 卸载：只移除指定包本身，不触碰 torch/CUDA 等共享依赖（pip uninstall 默认行为），
/// 因此不会影响其它引擎。遇 PEP 668 自动重试 --break-system-packages
fn pip_uninstall(pkg: &str, prefix: &str) -> String {
    format!(
        "{prefix}echo \"[uninstall] 移除 {pkg}（保留 torch 等共享依赖，不影响其它引擎）\"\n\
         python3 -m pip uninstall -y '{pkg}' 2>&1 \
           || python3 -m pip uninstall -y '{pkg}' --break-system-packages 2>&1",
        prefix = prefix
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
    let idx = crate::settings::pip_index_arg(&settings.pip_index);
    // apt 镜像源切换前缀（仅 apt 系脚本需要；自带提权，幂等）
    let deb = crate::settings::deb_mirror_prefix(&settings.deb_mirror, sudo_pass);
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
        "modelscope" => pip_install("modelscope", &proxy, &idx),
        "huggingface" => pip_install("huggingface_hub[cli]", &proxy, &idx),
        "vllm" => pip_install("vllm", &proxy, &idx),
        "sglang" => pip_install_sglang(&proxy, &idx),
        "1cat-vllm" => format!(
            "{proxy}mkdir -p {fw_dir}\ncd {fw_dir}\n\
              if [ -d 1Cat-vLLM ]; then cd 1Cat-vLLM && git pull; else git clone {onecat_repo_q} 1Cat-vLLM; fi\n\
              {boot}python3 -m pip install {idx} -e . 2>&1 || python3 -m pip install {idx} -e . --break-system-packages 2>&1",
            boot = pip_bootstrap()
        ),
        "llama-cpp" => format!(
            "{proxy}{deb}mkdir -p {fw_dir}\ncd {fw_dir}\n\
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
              cd llama.cpp\n\
              if command -v nvcc >/dev/null 2>&1; then echo \"[build] 检测到 CUDA 工具链（nvcc），构建 GPU 版\"; CUDA_FLAG=\"-DGGML_CUDA=on\"; else echo \"[build] 未检测到 CUDA 工具链（nvcc），构建 CPU 版（不可用 GPU 卸载）\"; CUDA_FLAG=\"\"; fi\n\
              if [ -f build/CMakeCache.txt ]; then\n\
              if [ -n \"$CUDA_FLAG\" ] && ! grep -q \"GGML_CUDA:BOOL=ON\" build/CMakeCache.txt 2>/dev/null; then echo \"[build] 清理旧构建目录（缓存非 GPU 版）\"; rm -rf build; fi\n\
              if [ -z \"$CUDA_FLAG\" ] && grep -q \"GGML_CUDA:BOOL=ON\" build/CMakeCache.txt 2>/dev/null; then echo \"[build] 清理旧构建目录（缓存为 GPU 版）\"; rm -rf build; fi\n\
              fi\n\
              cmake -B build -DCMAKE_BUILD_TYPE=Release $CUDA_FLAG && cmake --build build -j\"$(j=$(awk '/^MemTotal/ {{j=int($2/1024/2500); if(j<1)j=1; print j}}' /proc/meminfo); n=$(nproc 2>/dev/null || echo 4); [ \"$j\" -gt \"$n\" ] && j=$n; echo $j)\" --target llama-server 2>&1"
        ),
        "parser-libs" => format!(
            "{proxy}{boot}python3 -m pip install -U {idx} gguf safetensors 2>&1 \
              || python3 -m pip install -U {idx} gguf safetensors --break-system-packages 2>&1",
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
        // ---------- 卸载：只移除引擎包本身，保留 torch/CUDA 等共享依赖，不影响其它引擎 ----------
        "uninstall-vllm" => pip_uninstall("vllm", &proxy),
        "uninstall-sglang" => pip_uninstall("sglang", &proxy),
        "uninstall-1cat-vllm" => format!(
            "{proxy}{boot}python3 -m pip uninstall -y '1cat-vllm' 2>&1 \
              || python3 -m pip uninstall -y '1cat-vllm' --break-system-packages 2>&1\n\
              rm -rf {fw_dir}/1Cat-vLLM",
            boot = pip_bootstrap()
        ),
        "uninstall-llama-cpp" => format!(
            "echo \"[uninstall] 删除 llama.cpp 源码与构建目录（不影响其它引擎）\"\n\
             rm -rf {fw_dir}/llama.cpp"
        ),
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
        // 编译并发：内存自适应（每 job 约 2.5G，CUDA 编译更吃内存）且不超过核数
        assert!(s.contains(
            "cmake -B build -DCMAKE_BUILD_TYPE=Release $CUDA_FLAG && cmake --build build -j\"$(j=$(awk '/^MemTotal/ {j=int($2/1024/2500); if(j<1)j=1; print j}' /proc/meminfo); n=$(nproc 2>/dev/null || echo 4); [ \"$j\" -gt \"$n\" ] && j=$n; echo $j)\" --target llama-server"
        ));
        // CUDA：有 nvcc 时传 -DGGML_CUDA=on，否则 CPU 版提示；缓存与本次要求不一致时清 build
        assert!(s.contains("command -v nvcc >/dev/null 2>&1"));
        assert!(s.contains("CUDA_FLAG=\"-DGGML_CUDA=on\""));
        assert!(s.contains("未检测到 CUDA 工具链（nvcc），构建 CPU 版"));
        assert!(s.contains("GGML_CUDA:BOOL=ON\" build/CMakeCache.txt"));
        assert!(s.contains("rm -rf build"));
    }

    #[test]
    fn uninstall_scripts_keep_shared_deps() {
        let d = crate::settings::AppSettings::default();
        // pip 引擎：只卸载自身包，绝不卸载 torch 等共享依赖
        for (tool, pkg) in [
            ("uninstall-vllm", "vllm"),
            ("uninstall-sglang", "sglang"),
            ("uninstall-1cat-vllm", "1cat-vllm"),
        ] {
            let s = build_install_script(&profile(), &d, tool, None).unwrap();
            assert!(s.contains(&format!("pip uninstall -y '{pkg}'")), "tool={tool}");
            assert!(!s.contains("uninstall -y 'torch'"), "tool={tool} 误删 torch");
            assert!(!s.contains("uninstall -y 'transformers'"), "tool={tool} 误删 transformers");
        }
        // 1cat-vllm 额外清理 editable 源码目录
        let s1 = build_install_script(&profile(), &d, "uninstall-1cat-vllm", None).unwrap();
        assert!(s1.contains("rm -rf"), "1cat 未清理源码目录");
        assert!(s1.contains("1Cat-vLLM"), "1cat 目录名错误");
        // llama-cpp：删整个源码+构建目录
        let s2 = build_install_script(&profile(), &d, "uninstall-llama-cpp", None).unwrap();
        assert!(s2.contains("rm -rf"), "llama 未删目录");
        assert!(s2.contains("llama.cpp"), "llama 目录名错误");
        assert!(!s2.contains("pip uninstall"), "llama 卸载不应走 pip");
    }

    #[test]
    fn pip_tools_have_bootstrap() {
        let d = crate::settings::AppSettings::default();
        for tool in ["vllm", "sglang", "modelscope", "huggingface", "1cat-vllm", "parser-libs"] {
            let s = build_install_script(&profile(), &d, tool, None).unwrap();
            assert!(s.contains("python3 -m pip --version"), "tool={tool}");
            assert!(s.contains("bootstrap.pypa.io/get-pip.py"), "tool={tool}");
            assert!(!s.contains("apt-get"), "tool={tool}");
            // pip 安装默认走清华镜像源
            assert!(s.contains("pypi.tuna.tsinghua.edu.cn/simple"), "tool={tool} 未用清华源");
        }
        // llama-cpp 纯 cmake 编译，不需要 pip 引导
        let s = build_install_script(&profile(), &d, "llama-cpp", None).unwrap();
        assert!(!s.contains("get-pip.py"));
    }

    #[test]
    fn onecat_editable_install_index_before_e() {
        // 回归：editable 安装必须是 `pip install {index} -e .`，
        // 若写成 `pip install -e {index} .`，pip 会把 --index-url 当成 -e 的目标而报错
        let d = crate::settings::AppSettings::default();
        let s = build_install_script(&profile(), &d, "1cat-vllm", None).unwrap();
        assert!(s.contains("pip install --index-url"), "1cat 未带 index-url: {s}");
        assert!(s.contains("simple/ -e ."), "1cat index 应在 -e 之前: {s}");
        assert!(!s.contains("-e --index-url"), "1cat 出现 -e --index-url 错误顺序: {s}");
    }

    #[test]
    fn sglang_install_sets_pyo3_abi3_and_cargo_path() {
        // sglang 依赖的 outlines_core 0.1.26 在 Python 3.14 上需 ABI3 前向兼容 + cargo 在 PATH
        let d = crate::settings::AppSettings::default();
        let s = build_install_script(&profile(), &d, "sglang", None).unwrap();
        assert!(s.contains("PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1"), "sglang 未设 ABI3 前向兼容");
        assert!(s.contains(".cargo/bin"), "sglang 未把 cargo 加入 PATH");
        // 其它引擎不需要这两个环境变量
        let v = build_install_script(&profile(), &d, "vllm", None).unwrap();
        assert!(!v.contains("PYO3_USE_ABI3_FORWARD_COMPATIBILITY"), "vllm 不应设 ABI3");
    }
}
