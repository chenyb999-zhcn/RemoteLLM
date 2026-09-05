use serde::Serialize;
use std::collections::HashMap;
use tauri::{AppHandle, State};

use crate::docker::{parse_mode, probe_sudo_mode, wrap_line, SudoMode};
use crate::error::AppError;
use crate::settings::AppSettings;

/// 单个检查项
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitItem {
    pub id: String,
    /// sys | gpu | docker | tools | engine
    pub group: &'static str,
    pub label: String,
    /// ok | warn | missing | info
    pub state: &'static str,
    pub detail: Option<String>,
    /// 修复动作: modelscope|huggingface|parser-libs|docker-install|docker-authorize|docker-proxy|goto-frameworks|apt
    pub fix: Option<String>,
    /// apt 修复时安装的包名
    pub fix_pkgs: Option<Vec<String>>,
    /// 无自动修复时的手动命令/提示
    pub manual: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitCheckResult {
    pub items: Vec<InitItem>,
    pub ok_count: u32,
    pub warn_count: u32,
    pub missing_count: u32,
}

/// apt 自动安装白名单（初始化检查页只允许装这些）
pub const APT_WHITELIST: &[&str] = &["curl", "git", "build-essential", "cmake", "ca-certificates"];

/// 一次 SSH 拿全部初始化信号（分节输出，section 解析与 docker.rs/envcheck.rs 相同约定）
fn init_script(base_dir: &str) -> String {
    let fw_dir = format!("{}/frameworks", base_dir.trim_end_matches('/'));
    format!(
        r#"
echo "==SYS=="
command -v curl >/dev/null 2>&1 && echo "curl OK" || echo "curl NONE"
command -v git >/dev/null 2>&1 && echo "git OK" || echo "git NONE"
command -v gcc >/dev/null 2>&1 && echo "gcc OK" || echo "gcc NONE"
command -v make >/dev/null 2>&1 && echo "make OK" || echo "make NONE"
command -v cmake >/dev/null 2>&1 && echo "cmake OK" || echo "cmake NONE"
grep PRETTY_NAME /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"'
uname -sr
echo "==GPU=="
nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>/dev/null || echo GPU_NONE
command -v nvcc >/dev/null 2>&1 && nvcc --version 2>/dev/null | grep -o 'release [0-9.]*' | head -1 || true
echo "==DOCKER=="
docker --version 2>/dev/null | head -1
systemctl is-active docker 2>/dev/null
(docker info >/dev/null 2>&1 && echo OK) || echo FAIL
docker info --format '{{{{.Runtimes}}}}' 2>/dev/null | grep -q '"nvidia"' && echo RT_OK || true
command -v nvidia-ctk >/dev/null 2>&1 && echo CTK_OK || true
[ -x /usr/bin/nvidia-container-runtime ] && echo BIN_OK || true
docker info --format '{{{{.HTTPProxy}}}}' 2>/dev/null
echo "==PY=="
python3 --version 2>/dev/null
python3 -m pip --version 2>/dev/null | head -1
{{ command -v modelscope >/dev/null 2>&1 || [ -x "$HOME/.local/bin/modelscope" ]; }} && echo MS_OK || echo MS_NONE
{{ command -v huggingface-cli >/dev/null 2>&1 || [ -x "$HOME/.local/bin/huggingface-cli" ]; }} && echo HF_OK || echo HF_NONE
echo "==PYLIBS=="
python3 -c "import importlib.util as u; print('GGUF_OK' if u.find_spec('gguf') else 'GGUF_NO'); print('ST_OK' if u.find_spec('safetensors') else 'ST_NO')" 2>/dev/null
echo "==VLLM=="
_v=""
command -v vllm >/dev/null 2>&1 && _v=$(vllm --version 2>/dev/null | head -1)
_p=$(python3 -c "import vllm; print('python-vllm', vllm.__version__)" 2>/dev/null)
[ -n "$_v" ] && echo "$_v"
[ -n "$_p" ] && echo "$_p"
[ -z "$_v" ] && [ -z "$_p" ] && echo NONE
echo "==1CAT=="
_v=""
command -v 1cat-vllm >/dev/null 2>&1 && _v=$(1cat-vllm --version 2>/dev/null | head -1)
[ -n "$_v" ] && echo "$_v"
[ -z "$_v" ] && echo NONE
echo "==SGLANG=="
_p=$(python3 -c "import sglang; print('sglang', sglang.__version__)" 2>/dev/null)
[ -n "$_p" ] && echo "$_p"
[ -z "$_p" ] && echo NONE
echo "==LLAMA=="
_v=""
command -v llama-server >/dev/null 2>&1 && _v=$(llama-server --version 2>&1 | head -1)
_f=$(find "{fw_dir}" -maxdepth 4 -name llama-server -type f 2>/dev/null | head -1)
[ -n "$_v" ] && echo "$_v"
[ -n "$_f" ] && echo "$_f"
[ -z "$_v" ] && [ -z "$_f" ] && echo NONE
echo "==IMG=="
docker images --format '{{{{.Repository}}}}:{{{{.Tag}}}}' 2>/dev/null
exit 0
"#,
        fw_dir = fw_dir
    )
}

fn section<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    let start_marker = format!("=={name}==\n");
    let start = raw.find(&start_marker)? + start_marker.len();
    let rest = &raw[start..];
    let end = rest.find("\n==").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// 从脚本输出解析出的服务器初始化信号
#[derive(Debug, Default, Clone)]
pub struct InitSignals {
    pub os: Option<String>,
    pub kernel: Option<String>,
    /// curl/git/gcc/make/cmake
    pub tools: HashMap<String, bool>,
    /// (GPU 型号, 驱动版本)
    pub gpus: Vec<(String, String)>,
    pub nvcc: Option<String>,
    pub docker_version: Option<String>,
    pub daemon_active: bool,
    pub docker_usable: bool,
    pub nvidia_runtime: bool,
    pub nvidia_ctk: bool,
    pub nvidia_bin: bool,
    pub daemon_proxy: Option<String>,
    pub python: Option<String>,
    pub pip: Option<String>,
    pub modelscope: bool,
    pub hf_cli: bool,
    pub gguf: bool,
    pub safetensors: bool,
    /// 4 引擎 native 检测结果（framework id -> installed/version）
    pub fw: Vec<(String, bool, Option<String>)>,
    /// 本地镜像 repo:tag 列表
    pub images: Vec<String>,
}

/// 解析 INIT_SCRIPT 输出（纯函数，可单测）
pub fn parse_signals(raw: &str) -> InitSignals {
    let mut s = InitSignals::default();

    let sys = section(raw, "SYS").unwrap_or("");
    let mut sys_lines = sys.lines().filter(|l| !l.trim().is_empty());
    for l in sys_lines.by_ref() {
        let l = l.trim();
        if let Some((name, st)) = l.split_once(' ') {
            if st == "OK" || st == "NONE" {
                s.tools.insert(name.to_string(), st == "OK");
                continue;
            }
        }
        // 非工具行：os / kernel
        if s.os.is_none() {
            s.os = Some(l.to_string());
        } else if s.kernel.is_none() {
            s.kernel = Some(l.to_string());
        }
    }

    let gpu = section(raw, "GPU").unwrap_or("");
    for l in gpu.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if l == "GPU_NONE" {
            continue;
        }
        if l.starts_with("release ") {
            s.nvcc = Some(l.trim_start_matches("release ").to_string());
            continue;
        }
        let (name, driver) = l.split_once(',').unwrap_or((l, ""));
        s.gpus
            .push((name.trim().to_string(), driver.trim().to_string()));
    }

    let dk = section(raw, "DOCKER").unwrap_or("");
    let mut dk_lines = dk.lines().map(str::trim).filter(|l| !l.is_empty());
    s.docker_version = dk_lines
        .next()
        .map(|l| l.to_string())
        .filter(|l| l.starts_with("Docker"));
    s.daemon_active = dk_lines.next() == Some("active");
    s.docker_usable = dk_lines.next() == Some("OK");
    for l in dk_lines {
        match l {
            "RT_OK" => s.nvidia_runtime = true,
            "CTK_OK" => s.nvidia_ctk = true,
            "BIN_OK" => s.nvidia_bin = true,
            other if !other.is_empty() => {
                if s.daemon_proxy.is_none() && other != "FAIL" && other != "inactive" {
                    s.daemon_proxy = Some(other.to_string());
                }
            }
            _ => {}
        }
    }

    let py = section(raw, "PY").unwrap_or("");
    let mut py_lines = py.lines().map(str::trim).filter(|l| !l.is_empty());
    s.python = py_lines.next().map(|l| l.to_string());
    s.pip = py_lines.next().filter(|l| l.starts_with("pip")).map(|l| l.to_string());
    s.modelscope = py.lines().any(|l| l.trim() == "MS_OK");
    s.hf_cli = py.lines().any(|l| l.trim() == "HF_OK");

    let libs = section(raw, "PYLIBS").unwrap_or("");
    s.gguf = libs.contains("GGUF_OK");
    s.safetensors = libs.contains("ST_OK");

    s.fw = crate::frameworks::parse_detect(raw)
        .into_iter()
        .map(|f| (f.framework, f.installed, f.version))
        .collect();

    s.images = section(raw, "IMG")
        .unwrap_or("")
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| {
            !l.is_empty() && !l.starts_with('<') && l != "exit 0"
        })
        .collect();
    s
}

fn item(
    id: &str,
    group: &'static str,
    label: &str,
    state: &'static str,
    detail: Option<String>,
) -> InitItem {
    InitItem {
        id: id.into(),
        group,
        label: label.into(),
        state,
        detail,
        fix: None,
        fix_pkgs: None,
        manual: None,
    }
}

fn tool_ok(s: &InitSignals, name: &str) -> bool {
    s.tools.get(name).copied().unwrap_or(false)
}

/// 由信号生成检查项（纯函数，可单测）。
/// default_onecat_image：档案未配置时的 1Cat 默认镜像
pub fn build_items(
    s: &InitSignals,
    settings: &AppSettings,
    default_onecat_image: &str,
) -> Vec<InitItem> {
    let mut v = Vec::new();

    // ---------- 基础系统 ----------
    v.push(item(
        "sys.os",
        "sys",
        "操作系统",
        "info",
        Some(
            [
                s.os.clone(),
                s.kernel.clone(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" "),
        ),
    ));
    v.push(item("sys.ssh", "sys", "SSH 连接", "ok", Some("已连接".into())));
    for t in ["curl", "git"] {
        let ok = tool_ok(s, t);
        v.push(InitItem {
            fix: (!ok).then(|| "apt".into()),
            fix_pkgs: (!ok).then(|| vec![t.to_string()]),
            ..item(
                &format!("sys.{t}"),
                "sys",
                if t == "curl" { "curl" } else { "git" },
                if ok { "ok" } else { "missing" },
                if ok {
                    None
                } else {
                    Some("未安装".into())
                },
            )
        });
    }
    let be = tool_ok(s, "gcc") && tool_ok(s, "make");
    v.push(InitItem {
        fix: (!be).then(|| "apt".into()),
        fix_pkgs: (!be).then(|| vec!["build-essential".into()]),
        ..item(
            "sys.build",
            "sys",
            "build-essential (gcc/make)",
            if be { "ok" } else { "warn" },
            if be {
                None
            } else {
                Some("缺失；仅 llama.cpp 原生编译需要，docker 模式可不装".into())
            },
        )
    });
    let cmake = tool_ok(s, "cmake");
    v.push(InitItem {
        fix: (!cmake).then(|| "apt".into()),
        fix_pkgs: (!cmake).then(|| vec!["cmake".into()]),
        ..item(
            "sys.cmake",
            "sys",
            "cmake",
            if cmake { "ok" } else { "warn" },
            if cmake {
                None
            } else {
                Some("缺失；仅 llama.cpp 原生编译需要，docker 模式可不装".into())
            },
        )
    });

    // ---------- GPU 驱动 ----------
    if s.gpus.is_empty() {
        v.push(InitItem {
            manual: Some("sudo ubuntu-drivers install && sudo reboot".into()),
            ..item(
                "gpu.driver",
                "gpu",
                "NVIDIA 驱动",
                "missing",
                Some("nvidia-smi 不可用，驱动未安装或未加载".into()),
            )
        });
    } else {
        let driver = s
            .gpus
            .first()
            .map(|(_, d)| d.clone())
            .filter(|d| !d.is_empty())
            .unwrap_or_default();
        v.push(item(
            "gpu.driver",
            "gpu",
            "NVIDIA 驱动",
            "ok",
            Some(if driver.is_empty() {
                format!("已安装（{} 块 GPU）", s.gpus.len())
            } else {
                format!("v{driver}，{} 块 GPU", s.gpus.len())
            }),
        ));
        let names: Vec<String> = s
            .gpus
            .iter()
            .map(|(n, _)| n.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        v.push(item(
            "gpu.list",
            "gpu",
            "GPU 型号",
            "info",
            Some(names.join(" + ")),
        ));
        // V100 (sm70) 专项提示
        if names.iter().any(|n| n.to_uppercase().contains("V100")) {
            v.push(item(
                "gpu.sm70",
                "gpu",
                "Volta (sm70) 兼容性",
                "warn",
                Some("V100 不支持 SGLang 新版；vLLM 需 --dtype float16（无原生 bf16）；llama.cpp 完全支持".into()),
            ));
        }
    }
    match &s.nvcc {
        Some(ver) => v.push(item(
            "gpu.nvcc",
            "gpu",
            "CUDA Toolkit",
            "ok",
            Some(format!("{ver}（仅原生模式需要，docker 镜像自带）")),
        )),
        None => v.push(item(
            "gpu.nvcc",
            "gpu",
            "CUDA Toolkit",
            "warn",
            Some("未安装；仅原生模式需要（vLLM/sglang pip 安装、llama.cpp 编译），docker 模式不需要".into()),
        )),
    }

    // ---------- Docker 与 GPU 运行时 ----------
    match &s.docker_version {
        Some(ver) => v.push(item("docker.installed", "docker", "Docker", "ok", Some(ver.clone()))),
        None => v.push(InitItem {
            fix: Some("docker-install".into()),
            ..item(
                "docker.installed",
                "docker",
                "Docker",
                "missing",
                Some("未安装（docker.io + nvidia-container-toolkit）".into()),
            )
        }),
    }
    if !s.daemon_active {
        v.push(InitItem {
            fix: Some("docker-authorize".into()),
            ..item(
                "docker.daemon",
                "docker",
                "Docker 服务",
                "warn",
                Some("daemon 未运行".into()),
            )
        });
    } else {
        v.push(item("docker.daemon", "docker", "Docker 服务", "ok", None));
    }
    if !s.docker_usable {
        v.push(InitItem {
            fix: Some("docker-authorize".into()),
            ..item(
                "docker.usable",
                "docker",
                "当前用户可用 docker",
                "warn",
                Some("不在 docker 组（需要授权或重新登录）".into()),
            )
        });
    } else {
        v.push(item("docker.usable", "docker", "当前用户可用 docker", "ok", None));
    }
    if s.nvidia_runtime {
        v.push(item(
            "docker.gpu",
            "docker",
            "GPU 运行时",
            "ok",
            Some("nvidia runtime 已注册".into()),
        ));
    } else {
        let detail = if s.nvidia_ctk {
            "nvidia-container-toolkit 已装，但 docker 未注册 runtime（需 nvidia-ctk runtime configure + 重启）"
        } else if s.nvidia_bin {
            "运行时二进制存在，未注册 docker runtime"
        } else {
            "nvidia-container-toolkit 未安装"
        };
        v.push(InitItem {
            fix: Some("docker-install".into()),
            ..item(
                "docker.gpu",
                "docker",
                "GPU 运行时",
                if s.nvidia_ctk || s.nvidia_bin { "warn" } else { "missing" },
                Some(detail.into()),
            )
        });
    }
    // daemon 代理（与全局设置比对）
    let want = crate::settings::effective_proxy(settings);
    let cur = s.daemon_proxy.as_deref().filter(|p| !p.is_empty());
    match want {
        Some(w) => {
            if cur == Some(w) {
                v.push(item(
                    "docker.proxy",
                    "docker",
                    "daemon 拉取代理",
                    "ok",
                    Some(w.to_string()),
                ));
            } else {
                v.push(InitItem {
                    fix: Some("docker-proxy".into()),
                    ..item(
                        "docker.proxy",
                        "docker",
                        "daemon 拉取代理",
                        "warn",
                        Some(format!(
                            "全局已启用代理 {}，daemon 当前：{}（拉镜像走直连/旧代理）",
                            w,
                            cur.unwrap_or("未配置")
                        )),
                    )
                });
            }
        }
        None => v.push(item(
            "docker.proxy",
            "docker",
            "daemon 拉取代理",
            "info",
            Some(cur.map(|c| format!("已配置 {c}（全局代理未启用，不干预）")).unwrap_or_else(|| "未配置（全局代理未启用）".into())),
        )),
    }

    // ---------- 模型工具 ----------
    match (&s.python, &s.pip) {
        (Some(p), Some(_)) => v.push(item("tools.python", "tools", "Python3 + pip", "ok", Some(p.clone()))),
        (Some(p), None) => v.push(InitItem {
            manual: Some("sudo apt install python3-pip".into()),
            ..item(
                "tools.python",
                "tools",
                "Python3 + pip",
                "warn",
                Some(format!("{p}，pip 缺失")),
            )
        }),
        _ => v.push(InitItem {
            manual: Some("sudo apt install python3 python3-pip".into()),
            ..item(
                "tools.python",
                "tools",
                "Python3 + pip",
                "missing",
                Some("python3 不可用".into()),
            )
        }),
    }
    v.push(InitItem {
        fix: (!s.modelscope).then(|| "modelscope".into()),
        ..item(
            "tools.modelscope",
            "tools",
            "modelscope CLI",
            if s.modelscope { "ok" } else { "missing" },
            if s.modelscope {
                None
            } else {
                Some("模型下载用（魔搭）".into())
            },
        )
    });
    v.push(InitItem {
        fix: (!s.hf_cli).then(|| "huggingface".into()),
        ..item(
            "tools.hfcli",
            "tools",
            "huggingface-cli",
            if s.hf_cli { "ok" } else { "missing" },
            if s.hf_cli {
                None
            } else {
                Some("模型下载用（HF）".into())
            },
        )
    });
    if s.gguf && s.safetensors {
        v.push(item("tools.parser", "tools", "解析库 gguf+safetensors", "ok", None));
    } else {
        v.push(InitItem {
            fix: Some("parser-libs".into()),
            ..item(
                "tools.parser",
                "tools",
                "解析库 gguf+safetensors",
                "missing",
                Some(format!(
                    "gguf: {} / safetensors: {}（缺失时模型架构/参数/上下文不可见）",
                    if s.gguf { "已装" } else { "未装" },
                    if s.safetensors { "已装" } else { "未装" }
                )),
            )
        });
    }

    // ---------- 推理引擎 ----------
    let fw_installed: Vec<String> = s
        .fw
        .iter()
        .filter(|(_, ok, _)| *ok)
        .map(|(id, _, ver)| match ver {
            Some(v) => format!("{id} ({v})"),
            None => id.clone(),
        })
        .collect();
    if fw_installed.is_empty() {
        v.push(item(
            "engine.native",
            "engine",
            "原生引擎",
            "info",
            Some("未安装（可选；docker 模式不需要）".into()),
        ));
    } else {
        v.push(item(
            "engine.native",
            "engine",
            "原生引擎",
            "ok",
            Some(fw_installed.join("、")),
        ));
    }
    // 镜像（按仓库名前缀匹配，tag 任意）
    let imgs = [
        ("engine.img.vllm", "vLLM 镜像", "vllm/vllm-openai"),
        ("engine.img.sglang", "SGLang 镜像", "lmsysorg/sglang"),
        ("engine.img.llama", "llama.cpp 镜像", "ggml-org/llama.cpp"),
        ("engine.img.onecat", "1Cat-vLLM 镜像", default_onecat_image),
    ];
    for (id, label, repo) in imgs {
        let hit = s
            .images
            .iter()
            .any(|i| i.rsplit_once(':').map(|(r, _)| r) == Some(repo));
        v.push(InitItem {
            fix: (!hit).then(|| "goto-frameworks".into()),
            ..item(
                id,
                "engine",
                label,
                if hit { "ok" } else { "missing" },
                if hit {
                    None
                } else {
                    Some("未拉取（引擎管理页可拉取）".into())
                },
            )
        });
    }

    v
}

fn summarize(items: Vec<InitItem>) -> InitCheckResult {
    let ok_count = items.iter().filter(|i| i.state == "ok").count() as u32;
    let warn_count = items.iter().filter(|i| i.state == "warn").count() as u32;
    let missing_count = items.iter().filter(|i| i.state == "missing").count() as u32;
    InitCheckResult {
        items,
        ok_count,
        warn_count,
        missing_count,
    }
}

/// 服务器初始化检查：一次 SSH 收集全部信号并生成检查项
#[tauri::command]
pub async fn server_init_check(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<InitCheckResult, AppError> {
    let profile = crate::profile::load_profiles(&app)?
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| AppError::Other(format!("服务器档案不存在: {profile_id}")))?;
    let settings = crate::settings::load_settings(&app)?;
    let default_img = profile
        .onecat_image
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "vllm/vllm-openai".into());

    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id));
    };
    let out = session.run(&init_script(&profile.base_dir)).await?;
    let signals = parse_signals(&out.stdout);
    Ok(summarize(build_items(&signals, &settings, &default_img)))
}

// ---------- apt 白名单安装 ----------

fn validate_pkgs(pkgs: &[String]) -> Result<(), AppError> {
    if pkgs.is_empty() {
        return Err(AppError::Other("安装包列表为空".into()));
    }
    for p in pkgs {
        if !APT_WHITELIST.contains(&p.as_str()) {
            return Err(AppError::Other(format!(
                "不在允许列表内: {p}（仅 {}）",
                APT_WHITELIST.join("/")
            )));
        }
    }
    Ok(())
}

/// apt 安装脚本（逐行 sudo 包装，与 docker 安装同模式）
pub fn build_apt_script(mode: SudoMode, password: Option<&str>, pkgs: &[String]) -> Result<String, AppError> {
    validate_pkgs(pkgs)?;
    let list = pkgs.join(" ");
    let lines = [
        "set -e".to_string(),
        "apt-get update -y".into(),
        format!("DEBIAN_FRONTEND=noninteractive apt-get install -y {list}"),
        "echo APT_INSTALL_DONE".into(),
    ];
    Ok(lines
        .into_iter()
        .map(|l| {
            if l.starts_with("set ") {
                l
            } else {
                wrap_line(&l, mode, password)
            }
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// apt 安装确认弹框预览（密码用 *** 占位）
#[tauri::command]
pub fn apt_install_preview(sudo_mode: String, pkgs: Vec<String>) -> Result<String, AppError> {
    let mode = parse_mode(&sudo_mode)?;
    let password = (mode == SudoMode::SudoPass).then_some("***");
    build_apt_script(mode, password, &pkgs)
}

/// 执行 apt 安装（后台流式任务）
#[tauri::command]
pub async fn apt_install_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    pkgs: Vec<String>,
    password: Option<String>,
) -> Result<String, AppError> {
    validate_pkgs(&pkgs)?;
    let mode = probe_sudo_mode(&state, &profile_id).await?;
    if mode == SudoMode::SudoPass && password.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(AppError::Other("该服务器 sudo 需要密码，请先输入 sudo 密码".into()));
    }
    let script = build_apt_script(mode, password.as_deref(), &pkgs)?;
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    let task_id = format!("apt-{}", chrono::Utc::now().timestamp_millis());
    crate::ssh::SshSession::spawn_stream(app, profile_id, script, task_id.clone());
    Ok(task_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_proxy(enabled: bool, url: &str) -> AppSettings {
        let mut s = AppSettings::default();
        s.proxy_enabled = enabled;
        s.proxy_url = url.into();
        s
    }

    const RAW_Z420: &str = "\
==SYS==
curl OK
git OK
gcc OK
make OK
cmake NONE
Ubuntu 26.04 LTS
Linux 6.14.0 x86_64
==GPU==
Tesla V100-PCIE-32GB, 580.65.06
Tesla V100-PCIE-32GB, 580.65.06
==DOCKER==
Docker version 29.7.2, build abc
active
OK
RT_OK
http://192.168.31.150:10810/
==PY==
Python 3.14.4
pip 25.0 from /usr/lib (python 3.14)
MS_OK
HF_NONE
==PYLIBS==
GGUF_OK
ST_OK
==VLLM==
NONE
==1CAT==
NONE
==SGLANG==
NONE
==LLAMA==
NONE
==IMG==
hello-world:latest
nvidia/cuda:12.8.1-devel-ubuntu22.04
==END==
";

    #[test]
    fn parse_z420_like_signals() {
        let s = parse_signals(RAW_Z420);
        assert!(s.tools["curl"] && s.tools["git"] && s.tools["gcc"]);
        assert!(!s.tools["cmake"]);
        assert_eq!(s.gpus.len(), 2);
        assert_eq!(s.gpus[0].1, "580.65.06");
        assert!(s.daemon_active && s.docker_usable && s.nvidia_runtime);
        assert_eq!(s.daemon_proxy.as_deref(), Some("http://192.168.31.150:10810/"));
        assert!(s.modelscope && !s.hf_cli);
        assert!(s.gguf && s.safetensors);
        assert_eq!(s.images.len(), 2);
        assert!(s.fw.iter().all(|(_, ok, _)| !ok));
    }

    #[test]
    fn build_items_z420_no_proxy_enabled() {
        let s = parse_signals(RAW_Z420);
        let items = build_items(&s, &settings_proxy(false, ""), "vllm/vllm-openai");
        let get = |id: &str| items.iter().find(|i| i.id == id).unwrap();
        assert_eq!(get("gpu.driver").state, "ok");
        assert_eq!(get("gpu.sm70").state, "warn"); // V100 提示
        assert_eq!(get("docker.gpu").state, "ok"); // RT_OK
        assert_eq!(get("docker.proxy").state, "info"); // 全局未启用，不干预
        assert_eq!(get("sys.cmake").state, "warn"); // cmake 缺，llama.cpp 需要
        assert_eq!(get("tools.modelscope").state, "ok");
        assert_eq!(get("tools.hfcli").state, "missing");
        assert_eq!(get("tools.hfcli").fix.as_deref(), Some("huggingface"));
        assert_eq!(get("engine.img.vllm").state, "missing");
        assert_eq!(get("engine.img.vllm").fix.as_deref(), Some("goto-frameworks"));
        // 未启代理时 daemon 已配代理 → info 不给修复按钮
        assert!(get("docker.proxy").fix.is_none());
    }

    #[test]
    fn build_items_proxy_mismatch() {
        let s = parse_signals(RAW_Z420);
        let items = build_items(&s, &settings_proxy(true, "http://192.168.31.88:7890"), "vllm/vllm-openai");
        let p = items.iter().find(|i| i.id == "docker.proxy").unwrap();
        assert_eq!(p.state, "warn");
        assert_eq!(p.fix.as_deref(), Some("docker-proxy"));
        assert!(p.detail.as_deref().unwrap().contains("192.168.31.88:7890"));
    }

    const RAW_FRESH: &str = "\
==SYS==
curl NONE
git NONE
gcc NONE
make NONE
cmake NONE
Ubuntu 24.04 LTS
Linux 6.8.0 x86_64
==GPU==
GPU_NONE
==DOCKER==
==PY==
Python 3.12.3
MS_NONE
HF_NONE
==PYLIBS==
==VLLM==
NONE
==1CAT==
NONE
==SGLANG==
NONE
==LLAMA==
NONE
==IMG==
==END==
";

    #[test]
    fn build_items_fresh_machine() {
        let s = parse_signals(RAW_FRESH);
        assert!(s.gpus.is_empty());
        let items = build_items(&s, &settings_proxy(false, ""), "vllm/vllm-openai");
        let get = |id: &str| items.iter().find(|i| i.id == id).unwrap();
        assert_eq!(get("gpu.driver").state, "missing");
        assert!(get("gpu.driver").manual.as_deref().unwrap().contains("ubuntu-drivers"));
        assert_eq!(get("sys.curl").state, "missing");
        assert_eq!(get("sys.curl").fix.as_deref(), Some("apt"));
        assert_eq!(get("sys.curl").fix_pkgs.as_deref(), Some(&["curl".to_string()][..]));
        assert_eq!(get("docker.installed").state, "missing");
        assert_eq!(get("docker.installed").fix.as_deref(), Some("docker-install"));
        assert_eq!(get("docker.gpu").state, "missing");
        assert_eq!(get("tools.python").state, "warn"); // pip 缺
        assert_eq!(get("tools.modelscope").state, "missing");
        let r = summarize(items);
        assert!(r.missing_count >= 8);
    }

    #[test]
    fn apt_whitelist_enforced() {
        assert!(build_apt_script(SudoMode::Root, None, &["curl".into()]).is_ok());
        assert!(build_apt_script(SudoMode::Root, None, &["curl".into(), "cmake".into()]).is_ok());
        assert!(build_apt_script(SudoMode::Root, None, &["nginx".into()]).is_err());
        assert!(build_apt_script(SudoMode::Root, None, &[]).is_err());
        let s = build_apt_script(SudoMode::SudoPass, Some("pw"), &["curl".into()]).unwrap();
        assert!(s.contains("sudo -S -p '' DEBIAN_FRONTEND=noninteractive apt-get install -y curl"));
        assert!(s.contains("sudo -S -p '' apt-get update -y"));
        assert!(s.ends_with("echo APT_INSTALL_DONE"));
    }
}
