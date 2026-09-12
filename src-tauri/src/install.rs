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

/// pip 卸载：只移除指定包本身，不触碰 torch/CUDA 等共享依赖（pip uninstall 默认行为），
/// 因此不会影响其它引擎。遇 PEP 668 自动重试 --break-system-packages
/// pip 框架（vLLM/sglang/FastLLM）公共前导：确保 uv 已装（用户态）→ 确保 Python 3.12
/// 可用 → venv 不存在则创建。幂等可重跑。框架统一装进 {fw_dir}/{venv} 与系统 Python
/// （可能 3.14，torch 生态不支持）隔离，pip 预编译 wheel 安装、不源码编译。
fn uv_venv_setup(fw_dir: &str, venv: &str, mirror: &str) -> String {
    format!(
        "export PATH=\"$HOME/.local/bin:$PATH\"\n\
          {mirror}\
          # 统一装进 3.12 venv（系统 python3 可能是 3.14，torch/torch.compile 不支持 3.14，\n\
         # sglang import 即崩；1Cat 预编译 wheel 仅 cp312），与系统 Python 隔离。\n\
         # 用 uv 确保 3.12 可用并建 venv（uv 装用户目录，无需 sudo）。\n\
         if ! command -v uv >/dev/null 2>&1; then\n\
         echo \"[setup] 安装 uv（用于管理 Python 3.12）...\"\n\
         curl -LsSf https://astral.sh/uv/install.sh | sh 2>&1 || {{ echo \"ERROR: uv 安装失败\"; exit 1; }}\n\
         fi\n\
         uv python install 3.12 2>&1 || {{ echo \"ERROR: uv 安装 Python 3.12 失败\"; exit 1; }}\n\
         if [ ! -x {fw_dir}/{venv}/bin/python ]; then\n\
         uv venv --python 3.12 {fw_dir}/{venv} 2>&1 || {{ echo \"ERROR: 创建 {venv} venv 失败\"; exit 1; }}\n\
         fi\n"
    )
}

/// 密码登录的档案可提取登录密码，作为 llama.cpp 工具链自愈的 sudo 密码
/// （若 sudo 密码与登录密码不同，安装会失败并提示走「环境检查」页）
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
    // uv python install 3.12 的镜像导出前缀（UV_PYTHON_INSTALL_MIRROR，空 = 官方 GitHub）
    let mirror = crate::settings::uv_python_mirror_prefix(settings);
    // apt 镜像源切换前缀（仅 apt 系脚本需要；自带提权，幂等）
    let deb = crate::settings::deb_mirror_prefix(&settings.deb_mirror, sudo_pass);
    // llama.cpp 工具链缺失时的密码 sudo 分支：root / 免密 sudo 之外的第三条路
    let llama_else = match sudo_pass {
        Some(pw) => format!(
            "{{ {u} && {i}; }} || {{ echo \"ERROR: 工具链安装失败（sudo 密码与登录密码不同或网络问题）。请先在「环境检查」页安装 build-essential + cmake 后重试\"; exit 1; }}",
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
        None => "echo \"ERROR: 缺少 C++ 工具链（g++/make/cmake）。请先在「环境检查」页安装 build-essential + cmake 后重试（或改用密码方式连接可自动安装）\"; exit 1".into(),
    };

    Ok(match tool {
        "modelscope" => pip_install("modelscope", &proxy, &idx),
        "huggingface" => pip_install("huggingface_hub[cli]", &proxy, &idx),
        "python312" => format!(
            "{proxy}export PATH=\"$HOME/.local/bin:$PATH\"\n\
              {mirror}\
              # 检查并安装 Python 3.12（vLLM/sglang/FastLLM/1Cat venv 的底座）。\n\
              # uv 装进用户目录（~/.local/share/uv），不碰系统 Python。\n\
              if ! command -v uv >/dev/null 2>&1; then\n\
              echo \"[setup] 安装 uv（用于管理 Python 3.12）...\"\n\
              curl -LsSf https://astral.sh/uv/install.sh | sh 2>&1 || {{ echo \"ERROR: uv 安装失败\"; exit 1; }}\n\
              fi\n\
              uv python install 3.12 2>&1 || {{ echo \"ERROR: uv 安装 Python 3.12 失败\"; exit 1; }}\n\
              echo \"[ok] Python 3.12: $(uv python find 3.12 2>/dev/null | head -1)\"\n\
              echo PY312_DONE",
        ),
        "vllm" => format!(
            "{proxy}mkdir -p {fw_dir}\n\
              {setup}\
              {fw_dir}/vllm-venv/bin/python -m pip install -U {idx} 'vllm' 2>&1 \
              || {fw_dir}/vllm-venv/bin/python -m pip install -U {idx} 'vllm' --break-system-packages 2>&1",
            setup = uv_venv_setup(&fw_dir, "vllm-venv", &mirror),
        ),
        "sglang" => format!(
            "{proxy}mkdir -p {fw_dir}\n\
              {setup}\
              {fw_dir}/sglang-venv/bin/python -m pip install -U {idx} 'sglang' 2>&1 \
              || {fw_dir}/sglang-venv/bin/python -m pip install -U {idx} 'sglang' --break-system-packages 2>&1",
            setup = uv_venv_setup(&fw_dir, "sglang-venv", &mirror),
        ),
        "1cat-vllm" => format!(
            "{proxy}mkdir -p {fw_dir}\n\
              export PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:$PATH\"\n\
              {mirror}\
              # 1Cat-vLLM 未发布到 PyPI，只能源码安装；预编译 wheel 仅 cp312、源码 flash-attn\n\
              # 拒绝 3.13/3.14，统一装进 3.12 venv（启动/检测均回退 1cat-venv）。用 uv 确保 3.12 可用并建 venv。\n\
              if ! command -v uv >/dev/null 2>&1; then\n\
              echo \"[setup] 安装 uv（用于管理 Python 3.12）...\"\n\
              curl -LsSf https://astral.sh/uv/install.sh | sh 2>&1 || {{ echo \"ERROR: uv 安装失败\"; exit 1; }}\n\
              fi\n\
              uv python install 3.12 2>&1 || {{ echo \"ERROR: uv 安装 Python 3.12 失败\"; exit 1; }}\n\
              if [ ! -x {fw_dir}/1cat-venv/bin/python ]; then\n\
              uv venv --python 3.12 {fw_dir}/1cat-venv 2>&1 || {{ echo \"ERROR: 创建 1cat venv 失败\"; exit 1; }}\n\
              fi\n\
              cd {fw_dir}\n\
              if [ -d 1Cat-vLLM ]; then cd 1Cat-vLLM && git pull; else git clone {onecat_repo_q} 1Cat-vLLM; fi\n\
              {fw_dir}/1cat-venv/bin/python -m pip install {idx} -e . 2>&1 || {fw_dir}/1cat-venv/bin/python -m pip install {idx} -e . --break-system-packages 2>&1",
        ),
        "fastllm" => format!(
            "{proxy}mkdir -p {fw_dir}\n\
              {setup}\
              {fw_dir}/ftllm-venv/bin/python -m pip install -U {idx} 'ftllm' 2>&1 \
              || {fw_dir}/ftllm-venv/bin/python -m pip install -U {idx} 'ftllm' --break-system-packages 2>&1",
            setup = uv_venv_setup(&fw_dir, "ftllm-venv", &mirror),
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
              command -v g++ >/dev/null 2>&1 && command -v make >/dev/null 2>&1 && command -v cmake >/dev/null 2>&1 || {{ echo \"ERROR: 工具链仍不完整（g++/make/cmake），请在「环境检查」页安装 build-essential + cmake\"; exit 1; }}\n\
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
                .unwrap_or_else(|| "ghcr.io/chenyb999-zhcn/1cat-vllm:1.5".into())
                .replace('\'', "'\\''");
            format!("docker pull '{img}' 2>&1")
        }
        "docker-sglang" => "docker pull 'lmsysorg/sglang:latest-cu129' 2>&1".into(),
        "docker-llama" => "docker pull 'ghcr.io/ggml-org/llama.cpp:server-cuda' 2>&1".into(),
        // ---------- 卸载：只移除引擎自身（venv 或 pip 包），保留其它引擎，不影响共享依赖 ----------
        "uninstall-vllm" => format!(
            "echo \"[uninstall] 删除 vllm 专用 venv（不影响其它引擎）\"\n\
              rm -rf {fw_dir}/vllm-venv\n\
              {proxy}{boot}echo \"[uninstall] 清理系统 python 中的 vllm（兼容旧版安装，不存在时忽略）\"\n\
              python3 -m pip uninstall -y 'vllm' 2>&1 \
              || python3 -m pip uninstall -y 'vllm' --break-system-packages 2>&1",
            boot = pip_bootstrap()
        ),
        "uninstall-sglang" => format!(
            "echo \"[uninstall] 删除 sglang 专用 venv（不影响其它引擎）\"\n\
              rm -rf {fw_dir}/sglang-venv\n\
              {proxy}{boot}echo \"[uninstall] 清理系统 python 中的 sglang（兼容旧版安装，不存在时忽略）\"\n\
              python3 -m pip uninstall -y 'sglang' 2>&1 \
              || python3 -m pip uninstall -y 'sglang' --break-system-packages 2>&1",
            boot = pip_bootstrap()
        ),
        "uninstall-1cat-vllm" => format!(
            "echo \"[uninstall] 删除 1cat 专用 venv（不影响其它引擎）\"\n\
              rm -rf {fw_dir}/1cat-venv\n\
              {proxy}{boot}echo \"[uninstall] 清理系统 python 中的 1cat-vllm（兼容旧版安装，不存在时忽略）\"\n\
              python3 -m pip uninstall -y '1cat-vllm' 2>&1 \
              || python3 -m pip uninstall -y '1cat-vllm' --break-system-packages 2>&1\n\
              rm -rf {fw_dir}/1Cat-vLLM",
            boot = pip_bootstrap()
        ),
        "uninstall-llama-cpp" => format!(
            "echo \"[uninstall] 删除 llama.cpp 源码与构建目录（不影响其它引擎）\"\n\
             rm -rf {fw_dir}/llama.cpp"
        ),
        "uninstall-fastllm" => format!(
            "echo \"[uninstall] 删除 ftllm 专用 venv（不影响其它引擎）\"\n\
             rm -rf {fw_dir}/ftllm-venv"
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
        // 密钥登录档案：给出「环境检查」指引
        let mut k = profile();
        k.auth = crate::profile::AuthMethod::Key { key_path: "~/.ssh/id".into(), passphrase: None };
        let s2 = build_install_script(&k, &d, "llama-cpp", None).unwrap();
        assert!(s2.contains("请在「环境检查」页安装 build-essential + cmake"));
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
        // 1cat-vllm 额外清理专用 venv 与 editable 源码目录
        let s1 = build_install_script(&profile(), &d, "uninstall-1cat-vllm", None).unwrap();
        assert!(s1.contains("rm -rf"), "1cat 未清理源码目录");
        assert!(s1.contains("1Cat-vLLM"), "1cat 目录名错误");
        assert!(s1.contains("1cat-venv"), "1cat 未删专用 venv");
        // sglang 卸载必须删 venv（旧版只卸系统包会留 venv，卸不干净）
        let s3 = build_install_script(&profile(), &d, "uninstall-sglang", None).unwrap();
        assert!(s3.contains("sglang-venv"), "sglang 未删 venv");
        assert!(s3.contains("rm -rf"), "sglang 卸载未用 rm -rf");
        // vllm 卸载同样先删 venv，再 best-effort 清旧版系统 pip 包
        let s0 = build_install_script(&profile(), &d, "uninstall-vllm", None).unwrap();
        assert!(s0.contains("vllm-venv"), "vllm 未删 venv");
        assert!(s0.contains("rm -rf"), "vllm 卸载未用 rm -rf");
        // llama-cpp：删整个源码+构建目录
        let s2 = build_install_script(&profile(), &d, "uninstall-llama-cpp", None).unwrap();
        assert!(s2.contains("rm -rf"), "llama 未删目录");
        assert!(s2.contains("llama.cpp"), "llama 目录名错误");
        assert!(!s2.contains("pip uninstall"), "llama 卸载不应走 pip");
    }

    #[test]
    fn pip_tools_have_bootstrap() {
        let d = crate::settings::AppSettings::default();
        // vllm / sglang / 1cat-vllm / fastllm 走 uv venv（自带 pip），不在此列
        for tool in ["modelscope", "huggingface", "parser-libs"] {
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
    fn onecat_install_uses_python312_venv() {
        // 1Cat-vLLM 预编译 wheel 仅 cp312，源码 flash-attn 也拒绝 3.13/3.14，
        // 统一装进 3.12 venv（启动/检测均回退 1cat-venv），不再依赖系统 python3 版本
        let d = crate::settings::AppSettings::default();
        let s = build_install_script(&profile(), &d, "1cat-vllm", None).unwrap();
        assert!(s.contains("uv python install 3.12"), "1cat 未装 Python 3.12: {s}");
        assert!(s.contains("uv venv --python 3.12"), "1cat 未建 3.12 venv: {s}");
        assert!(s.contains("1cat-venv/bin/python -m pip install"), "1cat 未装进 venv: {s}");
        assert!(!s.contains("version_info"), "1cat 不应再探测系统 Python 版本: {s}");
        // venv 必须在 git clone 之前建好（先备好 3.12 环境再拉代码编译）
        let venv = s.find("uv venv --python 3.12").unwrap();
        let clone = s.find("git clone").unwrap();
        assert!(venv < clone, "1cat venv 应在 clone 之前");
    }

    #[test]
    fn sglang_install_uses_python312_venv() {
        // sglang 需跑在 Python 3.12（系统 python3 可能是 3.14，torch.compile 不支持 3.14，
        // sglang import 即崩）。安装脚本用 uv 建 3.12 venv 再装 sglang。
        let d = crate::settings::AppSettings::default();
        let s = build_install_script(&profile(), &d, "sglang", None).unwrap();
        assert!(s.contains("uv python install 3.12"), "sglang 未装 Python 3.12: {s}");
        assert!(s.contains("uv venv --python 3.12"), "sglang 未建 3.12 venv: {s}");
        assert!(s.contains("sglang-venv/bin/python -m pip install"), "sglang 未装进 venv: {s}");
        // 3.12 venv 走预编译 wheel，不再需要编译回退（PYO3 稳定 ABI / cargo PATH）
        assert!(!s.contains("PYO3_USE_ABI3_FORWARD_COMPATIBILITY"), "sglang 不应再有编译回退: {s}");
        assert!(!s.contains(".cargo/bin"), "sglang 不应再依赖 cargo PATH: {s}");
        // 其它引擎不需要 venv / ABI3
        let v = build_install_script(&profile(), &d, "vllm", None).unwrap();
        assert!(!v.contains("sglang-venv"), "vllm 不应建 sglang venv");
        assert!(!v.contains("PYO3_USE_ABI3_FORWARD_COMPATIBILITY"), "vllm 不应设 ABI3");
    }

    #[test]
    fn vllm_install_uses_python312_venv() {
        // vLLM 与系统 Python（可能 3.14）隔离，统一装进 3.12 venv（pip 预编译 wheel）
        let d = crate::settings::AppSettings::default();
        let s = build_install_script(&profile(), &d, "vllm", None).unwrap();
        assert!(s.contains("uv python install 3.12"), "vllm 未装 Python 3.12: {s}");
        assert!(s.contains("uv venv --python 3.12"), "vllm 未建 3.12 venv: {s}");
        assert!(s.contains("vllm-venv/bin/python -m pip install"), "vllm 未装进 venv: {s}");
        assert!(s.contains("'vllm'"), "vllm 包名错误: {s}");
        // 不再走系统 pip（无 get-pip 引导、无 --break-system-packages 系统回退）
        assert!(!s.contains("get-pip.py"), "vllm 不应走系统 pip 引导: {s}");
        assert!(!s.contains("python3 -m pip install"), "vllm 不应装到系统 python: {s}");
        // 卸载：先删 venv，再 best-effort 清旧版系统包
        let u = build_install_script(&profile(), &d, "uninstall-vllm", None).unwrap();
        assert!(u.contains("rm -rf"), "vllm 卸载未删目录");
        assert!(u.contains("vllm-venv"), "vllm 卸载目录名错误");
    }

    #[test]
    fn python312_tool_installs_uv_python() {
        // 环境检查页「Python 3.12」一键安装：uv 装用户目录 + uv python install 3.12，
        // 无需 sudo、不碰系统 Python
        let d = crate::settings::AppSettings::default();
        let s = build_install_script(&profile(), &d, "python312", None).unwrap();
        assert!(s.contains("astral.sh/uv/install.sh"), "python312 未装 uv: {s}");
        assert!(s.contains("uv python install 3.12"), "python312 未装 3.12: {s}");
        assert!(s.contains("uv python find 3.12"), "python312 未回显安装结果: {s}");
        assert!(s.contains("PY312_DONE"), "python312 缺完成标记: {s}");
        assert!(!s.contains("apt-get"), "python312 不应走 apt: {s}");
        assert!(!s.contains("sudo"), "python312 不应需要 sudo: {s}");
    }

    #[test]
    fn uv_python_mirror_export_in_scripts() {
        // 配置 uv Python 镜像后，所有走 uv python install 3.12 的脚本都须导出
        // UV_PYTHON_INSTALL_MIRROR，且导出必须在安装命令之前
        let mut d = crate::settings::AppSettings::default();
        d.uv_python_mirror = "https://m.example/pbs".into();
        for tool in ["python312", "vllm", "sglang", "fastllm", "1cat-vllm"] {
            let s = build_install_script(&profile(), &d, tool, None).unwrap();
            assert!(
                s.contains("export UV_PYTHON_INSTALL_MIRROR='https://m.example/pbs'"),
                "tool={tool} 未导出 uv 镜像: {s}"
            );
            let m = s.find("export UV_PYTHON_INSTALL_MIRROR=").unwrap();
            let inst = s.find("uv python install 3.12").unwrap();
            assert!(m < inst, "tool={tool} 镜像导出应在 uv python install 之前");
        }
        // 未配置时不导出（走官方 GitHub）
        let d2 = crate::settings::AppSettings::default();
        for tool in ["python312", "vllm", "sglang", "fastllm", "1cat-vllm"] {
            let s = build_install_script(&profile(), &d2, tool, None).unwrap();
            assert!(!s.contains("UV_PYTHON_INSTALL_MIRROR"), "tool={tool} 未配置却导出镜像");
        }
    }

    #[test]
    fn fastllm_install_uses_python312_venv() {
        // FastLLM（pip 包名 ftllm）与系统 Python 隔离，装进 3.12 venv
        let d = crate::settings::AppSettings::default();
        let s = build_install_script(&profile(), &d, "fastllm", None).unwrap();
        assert!(s.contains("uv python install 3.12"), "fastllm 未装 Python 3.12: {s}");
        assert!(s.contains("uv venv --python 3.12"), "fastllm 未建 3.12 venv: {s}");
        assert!(s.contains("ftllm-venv/bin/python -m pip install"), "fastllm 未装进 venv: {s}");
        assert!(s.contains("'ftllm'"), "fastllm 包名错误: {s}");
        // 卸载：删整个 venv（ftllm 及其依赖都在 venv 内）
        let u = build_install_script(&profile(), &d, "uninstall-fastllm", None).unwrap();
        assert!(u.contains("rm -rf"), "fastllm 卸载未删目录");
        assert!(u.contains("ftllm-venv"), "fastllm 卸载目录名错误");
        assert!(!u.contains("uninstall -y 'torch'"), "fastllm 卸载误删 torch");
    }
}
