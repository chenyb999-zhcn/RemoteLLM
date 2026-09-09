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
    /// sys | gpu | cuda | docker | tools | engine
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

/// apt 自动安装白名单（环境检查页只允许装这些）
pub const APT_WHITELIST: &[&str] = &["curl", "git", "build-essential", "cmake", "ca-certificates"];

/// CUDA 库检查清单：key -> (中文标签, 作用说明, 缺失时状态)
/// 缺失状态：核心推理库（cuBLAS/cuDNN/NCCL）缺为 warn，其余专用库缺为 info（docker 镜像均自带）
const CUDA_LIBS: &[(&str, &str, &str, &str)] = &[
    ("cublas", "cuBLAS", "GPU 矩阵运算（GEMM/GEMV）；NCCL 传数据、cuBLAS 算数据", "warn"),
    ("cudnn", "cuDNN", "深度学习原语加速（Conv/Attention/Norm）；vLLM/PyTorch 推理训练必装", "warn"),
    ("nccl", "NCCL", "多 GPU/多节点集合通信（AllReduce、Broadcast 等）", "warn"),
    ("cufft", "cuFFT", "GPU 快速傅里叶变换（信号处理/频域计算专用）", "info"),
    ("cusolver", "cuSOLVER", "稠密/稀疏线性求解器（LU/QR/Cholesky/SVD）；科学计算/优化", "info"),
    ("cusparse", "cuSPARSE", "稀疏矩阵运算（SpMV/SpMM，CSR/CSC）；GNN/推荐/MoE 关键依赖", "info"),
    ("curand", "cuRAND", "GPU 高质量随机数生成（训练采样/初始化）", "info"),
    ("npp", "NPP", "图像/信号处理原语（滤波/形态学/统计）；CV 预处理管线", "info"),
    ("nvjpeg", "nvJPEG", "GPU 硬件 JPEG 编解码（数据加载瓶颈优化）", "info"),
    ("nvcomp", "nvCOMP", "GPU 通用压缩/解压（数据加载瓶颈优化）", "info"),
    ("cutlass", "CUTLASS", "高性能 GEMM/Attention 模板库（源码级）；FlashAttention/vLLM 自定义 kernel 基础", "info"),
    ("trtllm", "TensorRT-LLM", "LLM 推理优化引擎（量化/KV Cache/调度）；vLLM 的竞品/互补方案", "info"),
];

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
# Rust 工具链（sglang 的 outlines_core 等 Rust 扩展编译需要；rustup 装在 ~/.cargo/bin，未必在 PATH）
{{ command -v cargo >/dev/null 2>&1 || [ -x "$HOME/.cargo/bin/cargo" ]; }} && {{ cargo --version 2>/dev/null || "$HOME/.cargo/bin/cargo" --version 2>/dev/null; }} | grep -oE 'cargo [0-9.]+' | head -1 || true
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
# 1Cat-vLLM 预编译 wheel 装的是 `vllm` 入口，且跑在 3.12 venv 里；
# 先查 PATH 上的 1cat-vllm，再回退到 venv 的 vllm 二进制
if command -v 1cat-vllm >/dev/null 2>&1; then _v=$(1cat-vllm --version 2>/dev/null | head -1); fi
if [ -z "$_v" ] && [ -x "$HOME/RemoteLLM/frameworks/1cat-venv/bin/vllm" ]; then
  _v=$("$HOME/RemoteLLM/frameworks/1cat-venv/bin/vllm" --version 2>/dev/null | head -1)
fi
[ -n "$_v" ] && echo "$_v"
[ -z "$_v" ] && echo NONE
echo "==SGLANG=="
# sglang 装在 3.12 venv 里（系统 python3 是 3.14，import 即崩），优先查 venv
_p=$("$HOME/RemoteLLM/frameworks/sglang-venv/bin/python" -c "import sglang; print('sglang', sglang.__version__)" 2>/dev/null)
[ -z "$_p" ] && _p=$(python3 -c "import sglang; print('sglang', sglang.__version__)" 2>/dev/null)
[ -n "$_p" ] && echo "$_p"
[ -z "$_p" ] && echo NONE
echo "==LLAMA=="
_v=""
command -v llama-server >/dev/null 2>&1 && _v=$(llama-server --version 2>&1 | head -1)
_f=$(find "{fw_dir}" -maxdepth 4 -name llama-server -type f 2>/dev/null | head -1)
[ -n "$_v" ] && echo "$_v"
[ -n "$_f" ] && echo "$_f"
[ -z "$_v" ] && [ -z "$_f" ] && echo NONE
echo "==CUDALIBS=="
# CUDA 库探测：dpkg 系统包（前缀匹配，覆盖 libcublas-12-8 / libcublas12 两种命名）+ pip 包
# （nvidia-*-cu12 系列 / tensorrt-llm，系统 python3 + 引擎 venv）
_dpkg=$(dpkg -l 2>/dev/null | awk '/^ii/ {{print $2" "$3}}')
_pips=""
for P in python3 "$HOME/RemoteLLM/frameworks/1cat-venv/bin/python" "$HOME/RemoteLLM/frameworks/sglang-venv/bin/python"; do
  if [ "$P" = "python3" ]; then command -v python3 >/dev/null 2>&1 || continue; else [ -x "$P" ] || continue; fi
  _pips="$_pips
$("$P" -m pip list --format=freeze 2>/dev/null)"
done
for pair in "cublas:libcublas:nvidia-cublas" "cudnn:libcudnn:nvidia-cudnn" "nccl:libnccl:nvidia-nccl" "cufft:libcufft:nvidia-cufft" "cusolver:libcusolver:nvidia-cusolver" "cusparse:libcusparse:nvidia-cusparse" "curand:libcurand:nvidia-curand" "npp:libnpp:nvidia-npp" "nvjpeg:libnvjpeg:nvidia-nvjpeg" "nvcomp:libnvcomp:nvidia-nvcomp" "cutlass::nvidia-cutlass" "trtllm::tensorrt[-_]llm"; do
  key="${{pair%%:*}}"; rest="${{pair#*:}}"; deb="${{rest%%:*}}"; pyp="${{rest##*:}}"
  hit=""
  if [ -n "$deb" ]; then
    hit=$(printf '%s\n' "$_dpkg" | grep -E "^$deb" | head -1)
    if [ -n "$hit" ]; then echo "$key DPKG $hit"; continue; fi
  fi
  if [ -n "$pyp" ]; then
    hit=$(printf '%s\n' "$_pips" | grep -iE "^$pyp" | head -1)
    if [ -n "$hit" ]; then echo "$key PIP $hit"; fi
  fi
done
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
    /// Rust 工具链版本（cargo），sglang 等含 Rust 扩展的引擎编译需要
    pub rust: Option<String>,
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
    /// CUDA 库检测结果（lib key -> 详情：文件路径 或 "Python"）
    pub cuda_libs: HashMap<String, String>,
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
        if let Some(v) = l.strip_prefix("cargo ") {
            s.rust = Some(v.to_string());
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

    // CUDA 库：行形如 "<key> DPKG <包名> <版本>" 或 "<key> PIP <包名>==<版本>"
    // 每个 key 脚本只输出一行（dpkg 命中优先于 pip），or_insert 兜底
    for l in section(raw, "CUDALIBS")
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        let mut parts = l.splitn(3, ' ');
        let (key, tag, rest) = (
            parts.next().unwrap_or(""),
            parts.next().unwrap_or(""),
            parts.next().unwrap_or("").trim(),
        );
        if key.is_empty() || rest.is_empty() {
            continue;
        }
        let val = match tag {
            "DPKG" => format!("dpkg {rest}"),
            "PIP" => format!("pip {rest}"),
            _ => continue,
        };
        s.cuda_libs.entry(key.to_string()).or_insert(val);
    }

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
    // Rust 工具链：sglang 的 outlines_core 等 Rust 扩展需编译（无预编译 wheel 时）
    match &s.rust {
        Some(ver) => v.push(item(
            "gpu.rust",
            "gpu",
            "Rust 工具链",
            "ok",
            Some(format!("cargo {ver}（sglang 等含 Rust 扩展的引擎编译需要）")),
        )),
        None => v.push(InitItem {
            manual: Some(
                "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal"
                    .into(),
            ),
            ..item(
                "gpu.rust",
                "gpu",
                "Rust 工具链",
                "warn",
                Some("未安装；sglang 的 outlines_core 等 Rust 扩展需编译（无预编译 wheel 时），vLLM/llama.cpp 不需要".into()),
            )
        }),
    }

    // ---------- CUDA 库 ----------
    for (key, label, desc, missing_state) in CUDA_LIBS {
        match s.cuda_libs.get(*key) {
            Some(found) => v.push(item(
                &format!("cuda.{key}"),
                "cuda",
                label,
                "ok",
                Some(format!("{desc}；{found}")),
            )),
            None => v.push(item(
                &format!("cuda.{key}"),
                "cuda",
                label,
                missing_state,
                Some(format!(
                    "{desc}；未检测到（docker 镜像自带；原生模式可 apt 装 nvidia-cuda-toolkit 系或 pip 装 nvidia-* 包）"
                )),
            )),
        }
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
            manual: Some(
                "装框架时会自动用官网 get-pip.py 安装；或手动: curl -sSL https://bootstrap.pypa.io/get-pip.py | python3"
                    .into(),
            ),
            ..item(
                "tools.python",
                "tools",
                "Python3 + pip",
                "warn",
                Some(format!("{p}，pip 缺失（装框架时自动 get-pip.py）")),
            )
        }),
        _ => v.push(InitItem {
            manual: Some("sudo apt install python3（装框架时会自动用 get-pip.py 补 pip）".into()),
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
                    Some("未拉取（框架管理页可添加）".into())
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

/// 服务器环境检查：一次 SSH 收集全部信号并生成检查项
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
        .unwrap_or_else(|| "ghcr.io/chenyb999-zhcn/1cat-vllm".into());

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
pub fn build_apt_script(
    mode: SudoMode,
    password: Option<&str>,
    pkgs: &[String],
    deb_mirror: &str,
) -> Result<String, AppError> {
    validate_pkgs(pkgs)?;
    let list = pkgs.join(" ");
    let deb = crate::settings::deb_mirror_prefix(deb_mirror, password);
    let mut lines: Vec<String> = vec!["set -e".to_string()];
    if !deb.is_empty() {
        lines.push(deb);
    }
    lines.push("apt-get update -y".into());
    lines.push(format!("DEBIAN_FRONTEND=noninteractive apt-get install -y {list}"));
    lines.push("echo APT_INSTALL_DONE".into());
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
pub fn apt_install_preview(
    app: AppHandle,
    sudo_mode: String,
    pkgs: Vec<String>,
) -> Result<String, AppError> {
    let mode = parse_mode(&sudo_mode)?;
    let password = (mode == SudoMode::SudoPass).then_some("***");
    let settings = crate::settings::load_settings(&app)?;
    build_apt_script(mode, password, &pkgs, &settings.deb_mirror)
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
    let settings = crate::settings::load_settings(&app)?;
    let script = build_apt_script(mode, password.as_deref(), &pkgs, &settings.deb_mirror)?;
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    let task_id = format!("apt-{}", chrono::Utc::now().timestamp_millis());
    crate::applog::info(
        "task",
        &format!("apt_install profile={profile_id} pkgs={}", pkgs.join(",")),
    );
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
cargo 1.88.0
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
==CUDALIBS==
cublas DPKG libcublas12 12.8.4.1
cudnn PIP nvidia-cudnn-cu12==9.10.2.21
nccl DPKG libnccl2 2.26.5-1
trtllm PIP tensorrt-llm==0.17.0
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
        assert_eq!(s.rust.as_deref(), Some("1.88.0"));
        // CUDA 库：dpkg 命中 / pip 命中；未出现者为缺失
        assert_eq!(
            s.cuda_libs.get("cublas").map(String::as_str),
            Some("dpkg libcublas12 12.8.4.1")
        );
        assert_eq!(
            s.cuda_libs.get("cudnn").map(String::as_str),
            Some("pip nvidia-cudnn-cu12==9.10.2.21")
        );
        assert_eq!(s.cuda_libs.get("nccl").map(String::as_str), Some("dpkg libnccl2 2.26.5-1"));
        assert_eq!(s.cuda_libs.get("trtllm").map(String::as_str), Some("pip tensorrt-llm==0.17.0"));
        assert!(s.cuda_libs.get("cufft").is_none());
        assert!(s.cuda_libs.get("nvcomp").is_none());
    }

    #[test]
    fn build_items_cuda_libs() {
        let s = parse_signals(RAW_Z420);
        let items = build_items(&s, &settings_proxy(false, ""), "vllm/vllm-openai");
        let get = |id: &str| items.iter().find(|i| i.id == id).unwrap();
        // dpkg 命中 → ok，detail 带包名+版本
        assert_eq!(get("cuda.cublas").state, "ok");
        assert!(get("cuda.cublas").detail.as_deref().unwrap().contains("dpkg libcublas12 12.8.4.1"));
        assert_eq!(get("cuda.nccl").state, "ok");
        // pip 命中 → ok，detail 标注 pip
        assert_eq!(get("cuda.trtllm").state, "ok");
        assert!(get("cuda.trtllm").detail.as_deref().unwrap().contains("pip tensorrt-llm==0.17.0"));
        // 核心库缺失 → warn（只保留 nccl 命中，其余缺失）
        let raw_nocuda = RAW_Z420.replace(
            "cublas DPKG libcublas12 12.8.4.1\n\
             cudnn PIP nvidia-cudnn-cu12==9.10.2.21\n\
             nccl DPKG libnccl2 2.26.5-1\n\
             trtllm PIP tensorrt-llm==0.17.0\n",
            "nccl DPKG libnccl2 2.26.5-1\n",
        );
        let s2 = parse_signals(&raw_nocuda);
        let items2 = build_items(&s2, &settings_proxy(false, ""), "vllm/vllm-openai");
        let g2 = |id: &str| items2.iter().find(|i| i.id == id).unwrap();
        assert_eq!(g2("cuda.cublas").state, "warn");
        assert_eq!(g2("cuda.cudnn").state, "warn");
        assert_eq!(g2("cuda.nccl").state, "ok");
        // 专用库缺失 → info
        assert_eq!(g2("cuda.cufft").state, "info");
        assert_eq!(g2("cuda.cutlass").state, "info");
    }

    #[test]
    fn build_items_rust_toolchain() {
        let s = parse_signals(RAW_Z420);
        let items = build_items(&s, &settings_proxy(false, ""), "vllm/vllm-openai");
        let get = |id: &str| items.iter().find(|i| i.id == id).unwrap();
        // 有 cargo → ok
        assert_eq!(get("gpu.rust").state, "ok");
        assert!(get("gpu.rust").detail.as_deref().unwrap().contains("1.88.0"));

        // 无 cargo → warn + 手动 rustup 命令
        let raw_norust = RAW_Z420.replace("cargo 1.88.0\n", "");
        let s2 = parse_signals(&raw_norust);
        assert_eq!(s2.rust, None);
        let items2 = build_items(&s2, &settings_proxy(false, ""), "vllm/vllm-openai");
        let r2 = items2.iter().find(|i| i.id == "gpu.rust").unwrap();
        assert_eq!(r2.state, "warn");
        assert!(r2.manual.as_deref().unwrap().contains("rustup.rs"));
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
        assert!(build_apt_script(SudoMode::Root, None, &["curl".into()], "official").is_ok());
        assert!(build_apt_script(SudoMode::Root, None, &["curl".into(), "cmake".into()], "official").is_ok());
        assert!(build_apt_script(SudoMode::Root, None, &["nginx".into()], "official").is_err());
        assert!(build_apt_script(SudoMode::Root, None, &[], "official").is_err());
        let s = build_apt_script(SudoMode::SudoPass, Some("pw"), &["curl".into()], "official").unwrap();
        assert!(s.contains("sudo -S -p '' DEBIAN_FRONTEND=noninteractive apt-get install -y curl"));
        assert!(s.contains("sudo -S -p '' apt-get update -y"));
        assert!(s.ends_with("echo APT_INSTALL_DONE"));
    }

    #[test]
    fn apt_script_deb_mirror_prefix() {
        let s = build_apt_script(SudoMode::Root, None, &["curl".into()], "aliyun").unwrap();
        assert!(s.contains("mirrors.aliyun.com/ubuntu"));
        assert!(s.contains("sed -i"));
        // official → 无切换片段
        let s2 = build_apt_script(SudoMode::Root, None, &["curl".into()], "official").unwrap();
        assert!(!s2.contains("sed -i 's#http://archive.ubuntu.com"));
    }
}
