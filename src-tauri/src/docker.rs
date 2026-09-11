use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::AppError;

/// Docker 服务状态（连接后检查用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub daemon_running: bool,
    /// 当前用户能否直接执行 docker（在 docker 组或 root）
    pub usable: bool,
    pub is_root: bool,
    pub sudo_passwordless: bool,
    /// nvidia-container-toolkit 是否就绪（--gpus all 需要）
    pub gpu_runtime: bool,
    /// GPU 信号明细: "rt"(docker 已注册) | "ctk"(装了 nvidia-ctk 未配置) | "bin"(有运行时二进制未注册) | None(未安装)
    pub gpu_runtime_detail: Option<String>,
    /// docker daemon 当前配置的拉取代理（docker info HTTPProxy），None = 未配置
    pub daemon_proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalImage {
    pub name: String,
    pub size: Option<String>,
}

const CHECK_SCRIPT: &str = r#"
echo "==VER=="
docker --version 2>/dev/null | head -1
echo "==SVC=="
systemctl is-active docker 2>/dev/null
echo "==USABLE=="
(docker info >/dev/null 2>&1 && echo OK) || echo FAIL
echo "==ROOT=="
[ "$(id -u)" = "0" ] && echo YES || echo NO
echo "==SUDO=="
sudo -n true 2>/dev/null && echo OK || echo FAIL
echo "==GPU=="
docker info --format '{{.Runtimes}}' 2>/dev/null | grep -q '"nvidia"' && echo RT_OK || true
command -v nvidia-ctk >/dev/null 2>&1 && echo CTK_OK || true
[ -x /usr/bin/nvidia-container-runtime ] && echo BIN_OK || true
echo "==DPROXY=="
docker info --format '{{.HTTPProxy}}' 2>/dev/null
exit 0
"#;

fn section<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("=={name}==\n");
    let start = raw.find(&marker)? + marker.len();
    let rest = &raw[start..];
    let end = rest.find("\n==").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// 解析 ==GPU== 段的多信号：RT_OK(docker 已注册 nvidia runtime) > CTK_OK > BIN_OK
fn parse_gpu(gpu_section: &str) -> (bool, Option<String>) {
    let (mut runtime, mut ctk, mut bin) = (false, false, false);
    for line in gpu_section.lines() {
        match line.trim() {
            "RT_OK" => runtime = true,
            "CTK_OK" => ctk = true,
            "BIN_OK" => bin = true,
            _ => {}
        }
    }
    let detail = if runtime {
        Some("rt".into())
    } else if ctk {
        Some("ctk".into())
    } else if bin {
        Some("bin".into())
    } else {
        None
    };
    (runtime, detail)
}

#[tauri::command]
pub async fn check_docker(
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<DockerStatus, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id));
    };
    let out = session.run(CHECK_SCRIPT).await?;
    let raw = out.stdout.as_str();

    let version = section(raw, "VER")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let svc = section(raw, "SVC").unwrap_or("").trim();
    let daemon_running = svc == "active";
    let usable = section(raw, "USABLE") == Some("OK");
    let is_root = section(raw, "ROOT") == Some("YES");
    let sudo_passwordless = section(raw, "SUDO") == Some("OK");
    let (gpu_runtime, gpu_runtime_detail) = parse_gpu(section(raw, "GPU").unwrap_or(""));
    let daemon_proxy = section(raw, "DPROXY")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(DockerStatus {
        installed: version.is_some() && daemon_running,
        version,
        daemon_running,
        usable,
        is_root,
        sudo_passwordless,
        gpu_runtime,
        gpu_runtime_detail,
        daemon_proxy,
    })
}

#[tauri::command]
pub async fn list_docker_images(
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<Vec<LocalImage>, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id));
    };
    let cmd = "docker images --format '{{.Repository}}:{{.Tag}}|{{.Size}}' 2>/dev/null";
    let out = session.run(cmd).await?;
    if out.exit_code != 0 {
        return Err(AppError::Other(
            out.stderr.trim().to_string().lines().next().unwrap_or("docker images 执行失败").to_string(),
        ));
    }
    let mut list = Vec::new();
    for line in out.stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('<') {
            continue;
        }
        let (name, size) = match line.rsplit_once('|') {
            Some((n, s)) => (n.trim().to_string(), Some(s.trim().to_string())),
            None => (line.to_string(), None),
        };
        if name.is_empty() || name == "<none>" {
            continue;
        }
        list.push(LocalImage { name, size });
    }
    Ok(list)
}

/// GPU 实测：用本地 nvidia 镜像跑一次 `--gpus all` + nvidia-smi -L
#[tauri::command]
pub async fn docker_gpu_test(
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<String, AppError> {
    const SCRIPT: &str = r#"img=$(docker images --format '{{.Repository}}:{{.Tag}}' 2>/dev/null | grep -Ei 'nvidia/cuda|nvcr.io/nvidia' | grep -v '<none>' | head -1)
if [ -z "$img" ]; then
  echo "NO_NVIDIA_IMAGE"
  echo "未找到本地 nvidia 镜像，请先拉取一个（如 nvidia/cuda:12.4.1-base-ubuntu22.04）"
  exit 3
fi
echo "TEST_IMAGE=$img"
timeout 60 docker run --rm --gpus all "$img" nvidia-smi -L 2>&1
echo "EXIT=$?"
"#;
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id));
    };
    let out = session.run(SCRIPT).await?;
    let mut s = out.stdout;
    if !out.stderr.trim().is_empty() {
        s.push('\n');
        s.push_str(out.stderr.trim());
    }
    Ok(s)
}

// ---------- sudo 提权 ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SudoMode {
    Root,
    Sudo,
    SudoPass,
}

pub(crate) fn parse_mode(s: &str) -> Result<SudoMode, AppError> {
    match s {
        "root" => Ok(SudoMode::Root),
        "sudo" => Ok(SudoMode::Sudo),
        "password" => Ok(SudoMode::SudoPass),
        other => Err(AppError::Other(format!("未知提权模式: {other}"))),
    }
}

fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 逐行包装 sudo（安装脚本必须是逐行独立命令）
pub(crate) fn wrap_line(line: &str, mode: SudoMode, password: Option<&str>) -> String {
    match mode {
        SudoMode::Root => line.to_string(),
        SudoMode::Sudo => format!("sudo {line}"),
        SudoMode::SudoPass => {
            let p = password.unwrap_or("");
            format!("printf '%s\\n' {} | sudo -S -p '' {line}", shq(p))
        }
    }
}

/// Docker 安装脚本（apt 系；GPU 运行时失败不阻塞 docker 本体）
///
/// 关键点：guard/heredoc 及其内容/临时文件等行不能被 sudo 包装 ——
/// 1) `command` 是 shell 内建，`sudo command -v ...` 会直接失败；
/// 2) 管道/重定向（`curl | gpg -o /usr/share/...`、`> /etc/apt/...`）被包装后
///    只有第一段拿到 sudo，其余以普通用户运行 → 权限拒绝被 || true 静默吞掉。
/// 需要 root 的管道/重定向放到临时脚本里整体 `sudo bash` 执行。
pub fn build_install_script(mode: SudoMode, password: Option<&str>, deb_mirror: &str) -> String {
    // apt 镜像源切换前缀（自带提权，幂等；official/空 → 空串）
    let deb = crate::settings::deb_mirror_prefix(deb_mirror, password);
    // (文本, 是否参与 sudo 包装)
    let mut steps: Vec<(String, bool)> = vec![
        ("set -e".to_string(), false),
        // guard：绝对路径检测（usr-merge 后 apt-get 恒在 /usr/bin），且不包装
        ("{ [ -x /usr/bin/apt-get ] || [ -x /usr/sbin/apt-get ]; } || { echo '未找到 apt-get，无法一键安装 Docker（当前仅支持 Debian/Ubuntu）'; exit 97; }".to_string(), false),
    ];
    if !deb.is_empty() {
        steps.push((deb, false));
    }
    steps.extend([
        ("apt-get update -y".to_string(), true),
        ("DEBIAN_FRONTEND=noninteractive apt-get install -y docker.io".to_string(), true),
        ("systemctl enable docker".to_string(), true),
        ("systemctl restart docker".to_string(), true),
        ("usermod -aG docker \"$(whoami)\" || true".to_string(), true),
        ("cat > \"$HOME/.rl_nvidia_repo.sh\" <<'RL_EOF'".to_string(), false),
        // --batch：无 TTY 的 SSH 会话里 gpg 会尝试开 /dev/tty 而失败（管道断裂，keyring 建不出来）
        ("curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey | gpg --batch --yes --dearmor -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg || true".to_string(), false),
        ("curl -fsSL https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list | sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g' > /etc/apt/sources.list.d/nvidia-container-toolkit.list || true".to_string(), false),
        ("RL_EOF".to_string(), false),
        ("bash \"$HOME/.rl_nvidia_repo.sh\"".to_string(), true),
        ("rm -f \"$HOME/.rl_nvidia_repo.sh\"".to_string(), false),
        ("apt-get update -y || true".to_string(), true),
        ("DEBIAN_FRONTEND=noninteractive apt-get install -y nvidia-container-toolkit || true".to_string(), true),
        ("nvidia-ctk runtime configure --runtime=docker || true".to_string(), true),
        ("systemctl restart docker || true".to_string(), true),
        ("echo DOCKER_INSTALL_DONE".to_string(), true),
    ]);
    steps
        .iter()
        .map(|(l, wrap)| {
            if *wrap {
                wrap_line(l, mode, password)
            } else {
                l.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 授权脚本：启动 daemon + 把当前用户加入 docker 组
pub fn build_authorize_script(mode: SudoMode, password: Option<&str>) -> String {
    const LINES: &[&str] = &[
        "set -e",
        "systemctl start docker || true",
        "usermod -aG docker \"$(whoami)\" || true",
        "echo DOCKER_AUTHORIZE_DONE",
    ];
    LINES
        .iter()
        .map(|l| {
            if l.starts_with("set ") {
                l.to_string()
            } else {
                wrap_line(l, mode, password)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) async fn probe_sudo_mode(state: &State<'_, crate::AppState>, profile_id: &str) -> Result<SudoMode, AppError> {
    let script = "[ \"$(id -u)\" = 0 ] && echo RL_ROOT; sudo -n true 2>/dev/null && echo RL_NOPASS";
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(profile_id) else {
        return Err(AppError::NotConnected(profile_id.to_string()));
    };
    let out = session.run(script).await?;
    if out.stdout.contains("RL_ROOT") {
        Ok(SudoMode::Root)
    } else if out.stdout.contains("RL_NOPASS") {
        Ok(SudoMode::Sudo)
    } else {
        Ok(SudoMode::SudoPass)
    }
}

/// 安装确认弹框预览（密码用 *** 占位）
#[tauri::command]
pub fn docker_install_preview(app: AppHandle, sudo_mode: String) -> Result<String, AppError> {
    let mode = parse_mode(&sudo_mode)?;
    let password = (mode == SudoMode::SudoPass).then_some("***");
    let settings = crate::settings::load_settings(&app)?;
    Ok(build_install_script(mode, password, &settings.deb_mirror))
}

#[tauri::command]
pub async fn docker_install_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    password: Option<String>,
) -> Result<String, AppError> {
    let mode = probe_sudo_mode(&state, &profile_id).await?;
    if mode == SudoMode::SudoPass && password.as_deref().map(str::trim).unwrap_or("").is_empty()
    {
        return Err(AppError::Other("该服务器 sudo 需要密码，请先输入 sudo 密码".into()));
    }
    let settings = crate::settings::load_settings(&app)?;
    let script = build_install_script(mode, password.as_deref(), &settings.deb_mirror);
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    let task_id = format!("dinst-{}", chrono::Utc::now().timestamp_millis());
    crate::applog::info("task", &format!("docker_install profile={profile_id}"));
    crate::ssh::SshSession::spawn_stream(app, profile_id, script, task_id.clone());
    Ok(task_id)
}

#[tauri::command]
pub async fn docker_authorize_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    password: Option<String>,
) -> Result<String, AppError> {
    let mode = probe_sudo_mode(&state, &profile_id).await?;
    if mode == SudoMode::SudoPass && password.as_deref().map(str::trim).unwrap_or("").is_empty()
    {
        return Err(AppError::Other("该服务器 sudo 需要密码，请先输入 sudo 密码".into()));
    }
    let script = build_authorize_script(mode, password.as_deref());
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    let task_id = format!("dauth-{}", chrono::Utc::now().timestamp_millis());
    crate::applog::info("task", &format!("docker_authorize profile={profile_id}"));
    crate::ssh::SshSession::spawn_stream(app, profile_id, script, task_id.clone());
    Ok(task_id)
}

/// Docker daemon 拉取代理配置脚本（systemd drop-in）：
/// proxy_url = Some(..) 写入/更新代理；None 删除代理配置。均需 daemon-reload + 重启 docker。
/// drop-in 内容先写到用户家目录临时文件再 sudo cp，避免 sudo 包装与重定向/引号冲突。
pub fn build_proxy_script(mode: SudoMode, password: Option<&str>, proxy_url: Option<&str>) -> Result<String, AppError> {
    let mut lines: Vec<String> = vec!["set -e".into()];
    match proxy_url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => {
            if !u.starts_with("http://") && !u.starts_with("https://") && !u.starts_with("socks5://") {
                return Err(AppError::Other(
                    "代理地址需以 http:// 、https:// 或 socks5:// 开头".into(),
                ));
            }
            let conf = format!(
                "[Service]\nEnvironment=\"HTTP_PROXY={u}\"\nEnvironment=\"HTTPS_PROXY={u}\"\nEnvironment=\"NO_PROXY=localhost,127.0.0.1\"\n"
            );
            let conf = conf.replace('\\', "\\\\").replace('\'', "'\\''");
            lines.push(format!(
                "printf '%s\\n' '{conf}' > \"$HOME/.rl_docker_proxy.conf\""
            ));
            lines.push("mkdir -p /etc/systemd/system/docker.service.d".into());
            lines.push("cp \"$HOME/.rl_docker_proxy.conf\" /etc/systemd/system/docker.service.d/http-proxy.conf".into());
        }
        None => {
            lines.push("rm -f /etc/systemd/system/docker.service.d/http-proxy.conf".into());
        }
    }
    lines.push("systemctl daemon-reload".into());
    lines.push("systemctl restart docker".into());
    lines.push("sleep 2".into());
    lines.push("docker info --format 'daemon proxy: {{.HTTPProxy}} | https: {{.HTTPSProxy}}'".into());
    lines.push("echo DOCKER_PROXY_DONE".into());
    Ok(lines
        .into_iter()
        .map(|l| {
            if l.starts_with("set ") || l.starts_with("printf ") {
                l
            } else {
                wrap_line(&l, mode, password)
            }
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// daemon 代理配置确认弹框预览（密码用 *** 占位）
#[tauri::command]
pub fn docker_proxy_preview(sudo_mode: String, proxy_url: Option<String>) -> Result<String, AppError> {
    let mode = parse_mode(&sudo_mode)?;
    let password = (mode == SudoMode::SudoPass).then_some("***");
    build_proxy_script(mode, password, proxy_url.as_deref())
}

/// 应用 daemon 代理配置（读全局设置：启用 → 配置，禁用 → 移除）
#[tauri::command]
pub async fn docker_proxy_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    password: Option<String>,
) -> Result<String, AppError> {
    let settings = crate::settings::load_settings(&app)?;
    let proxy_url = crate::settings::effective_proxy(&settings).map(|s| s.to_string());
    let mode = probe_sudo_mode(&state, &profile_id).await?;
    if mode == SudoMode::SudoPass && password.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(AppError::Other("该服务器 sudo 需要密码，请先输入 sudo 密码".into()));
    }
    let script = build_proxy_script(mode, password.as_deref(), proxy_url.as_deref())?;
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    let task_id = format!("dproxy-{}", chrono::Utc::now().timestamp_millis());
    crate::applog::info(
        "task",
        &format!(
            "docker_proxy profile={profile_id} url={}",
            proxy_url.as_deref().unwrap_or("<remove>")
        ),
    );
    crate::ssh::SshSession::spawn_stream(app, profile_id, script, task_id.clone());
    Ok(task_id)
}

/// 只读：探测服务器 sudo 提权方式（环境检查页 apt 安装前调用）
#[tauri::command]
pub async fn sudo_mode_check(
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<String, AppError> {
    let mode = probe_sudo_mode(&state, &profile_id).await?;
    Ok(match mode {
        SudoMode::Root => "root".into(),
        SudoMode::Sudo => "sudo".into(),
        SudoMode::SudoPass => "password".into(),
    })
}

pub fn validate_image(img: &str) -> Result<(), AppError> {
    let t = img.trim();
    if t.is_empty() {
        return Err(AppError::Other("Docker 镜像地址不能为空".into()));
    }
    if t.len() > 256 {
        return Err(AppError::Other("Docker 镜像地址过长".into()));
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-' | ':' | '@'))
    {
        return Err(AppError::Other("镜像地址含非法字符（只允许字母数字 . _ / - : @）".into()));
    }
    Ok(())
}

#[tauri::command]
pub async fn docker_pull_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    image: String,
) -> Result<String, AppError> {
    validate_image(&image)?;
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id.clone()));
    }
    let script = format!("docker pull '{}' 2>&1", shq(&image.trim().to_string()));
    let task_id = format!("dpull-{}", chrono::Utc::now().timestamp_millis());
    crate::applog::info("task", &format!("docker_pull profile={profile_id} image={image}"));
    crate::ssh::SshSession::spawn_stream(app, profile_id, script, task_id.clone());
    Ok(task_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_script_root() {
        let s = build_proxy_script(SudoMode::Root, None, Some("http://p:7890")).unwrap();
        assert!(s.starts_with("set -e"));
        assert!(s.contains("Environment=\"HTTP_PROXY=http://p:7890\""));
        assert!(s.contains("Environment=\"HTTPS_PROXY=http://p:7890\""));
        assert!(s.contains("cp \"$HOME/.rl_docker_proxy.conf\" /etc/systemd/system/docker.service.d/http-proxy.conf"));
        assert!(s.contains("systemctl restart docker"));
        assert!(s.ends_with("echo DOCKER_PROXY_DONE"));
        assert!(!s.contains("sudo"));
    }

    #[test]
    fn proxy_script_sudopass_wrapped() {
        let s = build_proxy_script(SudoMode::SudoPass, Some("pw"), None).unwrap();
        assert!(s.contains("sudo -S -p '' rm -f /etc/systemd/system/docker.service.d/http-proxy.conf"));
        assert!(s.contains("sudo -S -p '' systemctl restart docker"));
        // 无代理配置时不写 drop-in 内容文件
        assert!(!s.contains(".rl_docker_proxy.conf"));
    }

    #[test]
    fn proxy_script_content_printf_not_wrapped() {
        // 写入内容行（用户家目录临时文件）不应被 sudo 包装；后续 cp 被 sudo 包装
        let s = build_proxy_script(SudoMode::SudoPass, Some("pw"), Some("http://p:1")).unwrap();
        assert!(s.contains("printf '%s\\n' '[Service]"));
        assert!(!s.contains("sudo -S -p '' printf"));
        assert!(s.contains("sudo -S -p '' cp \"$HOME/.rl_docker_proxy.conf\""));
    }

    #[test]
    fn proxy_script_rejects_bad_url() {
        assert!(build_proxy_script(SudoMode::Root, None, Some("ftp://x")).is_err());
    }

    #[test]
    fn wrap_root_passthrough() {
        assert_eq!(wrap_line("apt-get update -y", SudoMode::Root, None), "apt-get update -y");
    }

    #[test]
    fn wrap_sudo_prefix() {
        assert_eq!(wrap_line("apt-get update -y", SudoMode::Sudo, None), "sudo apt-get update -y");
    }

    #[test]
    fn wrap_sudopass_plain() {
        let out = wrap_line("apt-get update -y", SudoMode::SudoPass, Some("secret"));
        assert_eq!(out, "printf '%s\\n' 'secret' | sudo -S -p '' apt-get update -y");
    }

    #[test]
    fn wrap_sudopass_quote_escape() {
        let out = wrap_line("cmd", SudoMode::SudoPass, Some("pa'ss"));
        assert_eq!(out, "printf '%s\\n' 'pa'\\''ss' | sudo -S -p '' cmd");
    }

    #[test]
    fn install_script_root_lines() {
        let s = build_install_script(SudoMode::Root, None, "official");
        assert!(s.starts_with("set -e"));
        assert!(s.contains("apt-get install -y docker.io"));
        assert!(s.contains("nvidia-ctk runtime configure --runtime=docker || true"));
        // 无 TTY 会话必须 --batch（否则 gpg 开 /dev/tty 失败，keyring 建不出来）
        assert!(s.contains("gpg --batch --yes --dearmor"));
        assert!(s.ends_with("echo DOCKER_INSTALL_DONE"));
        assert!(!s.contains("sudo"));
    }

    #[test]
    fn install_script_deb_mirror_prefix() {
        // 选清华源 → 脚本含切换 apt 源的 sed 片段（在 apt-get update 之前）
        let s = build_install_script(SudoMode::Root, None, "tuna");
        assert!(s.contains("mirrors.tuna.tsinghua.edu.cn/ubuntu"));
        assert!(s.contains("sed -i"));
        // official → 无切换片段
        let s2 = build_install_script(SudoMode::Root, None, "official");
        assert!(!s2.contains("sed -i 's#http://archive.ubuntu.com"));
    }

    #[test]
    fn install_script_sudopass_every_cmd_wrapped() {
        let s = build_install_script(SudoMode::SudoPass, Some("p"), "official");
        let lines: Vec<&str> = s.lines().collect();
        for l in lines.iter().skip(1) {
            if l.starts_with("echo DOCKER_INSTALL_DONE") {
                continue;
            }
            // guard / heredoc 内容 / rm 行不包装；其余 apt/systemctl/usermod/bash 行必须包装
            if l.starts_with("{ [ -x /usr/bin/apt-get ]")
                || l.starts_with("cat > \"$HOME/.")
                || l.starts_with("curl -fsSL")
                || *l == "RL_EOF"
                || l.starts_with("rm -f \"$HOME/.")
            {
                assert!(
                    !l.starts_with("printf"),
                    "不该被包装的行: {l}"
                );
                continue;
            }
            assert!(
                l.starts_with("printf '%s\\n' 'p' | sudo -S -p '' "),
                "未包装的行: {l}"
            );
        }
    }

    #[test]
    fn install_script_no_sudo_builtin_command() {
        // Bug 回归：任何模式都不得出现 `sudo ... command -v`（command 是内建命令，sudo 会失败）
        for mode in [SudoMode::Root, SudoMode::Sudo, SudoMode::SudoPass] {
            let s = build_install_script(mode, Some("p"), "official");
            assert!(
                !s.contains("sudo") || !s.contains("command -v"),
                "guard 被 sudo 包装: {s}"
            );
            // guard 用绝对路径
            assert!(s.contains("[ -x /usr/bin/apt-get ]") && s.contains("/usr/sbin/apt-get"));
        }
    }

    #[test]
    fn install_script_nvidia_repo_via_sudo_bash() {
        // Bug 回归：nvidia 仓库管道/重定向必须在 root 下执行 —— 临时脚本整体 sudo bash
        for mode in [SudoMode::Root, SudoMode::Sudo, SudoMode::SudoPass] {
            let s = build_install_script(mode, Some("p"), "official");
            assert!(s.contains("cat > \"$HOME/.rl_nvidia_repo.sh\" <<'RL_EOF'"), "{mode:?}");
            assert!(s.contains("curl -fsSL https://nvidia.github.io"));
            assert!(s.contains("RL_EOF"));
            if mode == SudoMode::SudoPass {
                assert!(s.contains("| sudo -S -p '' bash \"$HOME/.rl_nvidia_repo.sh\""));
            } else if mode == SudoMode::Sudo {
                assert!(s.contains("sudo bash \"$HOME/.rl_nvidia_repo.sh\""));
            } else {
                assert!(s.contains("bash \"$HOME/.rl_nvidia_repo.sh\""));
            }
            assert!(s.contains("rm -f \"$HOME/.rl_nvidia_repo.sh\""));
            assert!(s.contains("apt-get install -y nvidia-container-toolkit || true"));
            assert!(s.contains("nvidia-ctk runtime configure --runtime=docker || true"));
        }
    }

    #[test]
    fn parse_sections() {
        let raw = "x\n==VER==\nDocker version 27.1.1\n==SVC==\nactive\n==USABLE==\nOK\n==ROOT==\nNO\n==SUDO==\nOK\n==GPU==\nRT_OK\nCTK_OK\n";
        assert_eq!(section(raw, "VER").unwrap(), "Docker version 27.1.1");
        assert_eq!(section(raw, "SVC").unwrap(), "active");
        assert_eq!(section(raw, "USABLE").unwrap(), "OK");
        assert_eq!(section(raw, "SUDO").unwrap(), "OK");
        assert_eq!(section(raw, "GPU").unwrap(), "RT_OK\nCTK_OK");
    }

    #[test]
    fn parse_gpu_signals() {
        assert_eq!(parse_gpu("RT_OK\nCTK_OK"), (true, Some("rt".into())));
        assert_eq!(parse_gpu("CTK_OK\n"), (false, Some("ctk".into())));
        assert_eq!(parse_gpu("BIN_OK\n"), (false, Some("bin".into())));
        assert_eq!(parse_gpu(""), (false, None));
        assert_eq!(parse_gpu("CTK_OK\nBIN_OK\n"), (false, Some("ctk".into())));
    }

    #[test]
    fn image_validation() {
        assert!(validate_image("vllm/vllm-openai:latest").is_ok());
        assert!(validate_image("ghcr.io/ggml-org/llama.cpp:server-cuda").is_ok());
        assert!(validate_image("reg.local:5000/x@sha256:abc").is_ok());
        assert!(validate_image("").is_err());
        assert!(validate_image("has space").is_err());
        assert!(validate_image("bad;rm -rf").is_err());
        assert!(validate_image("$(evil)").is_err());
    }
}
