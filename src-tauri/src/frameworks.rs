use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

/// 框架: "vllm" | "1cat-vllm" | "sglang" | "llama-cpp"
/// 模式: "native" | "docker"

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceConfig {
    pub id: String,
    pub profile_id: String,
    pub name: String,
    pub framework: String,
    pub mode: String,
    pub model_path: String,
    pub port: u16,
    #[serde(default)]
    pub docker_image: Option<String>,
    #[serde(default)]
    pub params: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FwDetect {
    pub framework: String,
    pub installed: bool,
    pub version: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStatus {
    pub id: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub health_code: Option<u16>,
    pub models_json: Option<String>,
    pub detail: Option<String>,
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "inst".to_string()
    } else {
        s.chars().take(40).collect()
    }
}

fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn pstr(p: &serde_json::Value, key: &str, default: &str) -> String {
    p.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(default)
        .to_string()
}

fn pstr_opt(p: &serde_json::Value, key: &str) -> Option<String> {
    p.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn pnum(p: &serde_json::Value, key: &str, default: u64) -> u64 {
    p.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

fn pnum_opt(p: &serde_json::Value, key: &str) -> Option<u64> {
    p.get(key).and_then(|v| v.as_u64())
}

fn pnumf_opt(p: &serde_json::Value, key: &str) -> Option<f64> {
    p.get(key).and_then(|v| v.as_f64())
}

fn pbool(p: &serde_json::Value, key: &str) -> bool {
    p.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// 由实例配置组装远端执行命令（前端预览与真正启动共用）
pub fn norm_args(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// llama-cpp 通用采样/KV 参数（原生与 Docker 共用）
fn push_llama_common(p: &serde_json::Value, c: &mut Vec<String>) {
    if let Some(v) = pstr_opt(p, "cacheTypeK") {
        c.push(format!("--cache-type-k {}", v));
    }
    if let Some(v) = pstr_opt(p, "cacheTypeV") {
        c.push(format!("--cache-type-v {}", v));
    }
    if let Some(v) = pstr_opt(p, "flashAttn") {
        c.push(format!("--flash-attn {}", v));
    }
    if let Some(v) = pstr_opt(p, "reasoning") {
        c.push(format!("--reasoning {}", v));
    }
    if pbool(p, "mlock") {
        c.push("--mlock".into());
    }
    if let Some(v) = pnumf_opt(p, "temperature") {
        c.push(format!("--temperature {}", v));
    }
    if let Some(v) = pnum_opt(p, "topK") {
        c.push(format!("--top-k {}", v));
    }
    if let Some(v) = pnumf_opt(p, "topP") {
        c.push(format!("--top-p {}", v));
    }
    if let Some(v) = pnumf_opt(p, "minP") {
        c.push(format!("--min-p {}", v));
    }
}

/// vLLM 通用参数（原生与 Docker 共用）
fn push_vllm_common(p: &serde_json::Value, c: &mut Vec<String>) {
    if let Some(v) = pnum_opt(p, "maxModelLen") {
        c.push(format!("--max-model-len {}", v));
    }
    if let Some(v) = pnumf_opt(p, "gpuMemUtil") {
        c.push(format!("--gpu-memory-utilization {}", v));
    }
    if let Some(v) = pstr_opt(p, "dtype") {
        c.push(format!("--dtype {}", v));
    }
    if let Some(v) = pstr_opt(p, "servedModelName") {
        c.push(format!("--served-model-name {}", v));
    }
    if pbool(p, "enforceEager") {
        c.push("--enforce-eager".into());
    }
    if let Some(v) = pnumf_opt(p, "maxNumSeqs") {
        c.push(format!("--max-num-seqs {}", v));
    }
    if let Some(v) = pnumf_opt(p, "maxNumBatchedTokens") {
        c.push(format!("--max-num-batched-tokens {}", v));
    }
    if let Some(v) = pstr_opt(p, "quantization") {
        c.push(format!("--quantization {}", v));
    }
    if let Some(v) = pstr_opt(p, "seed") {
        c.push(format!("--seed {}", v));
    }
    if let Some(v) = pnumf_opt(p, "temperature") {
        c.push(format!("--temperature {}", v));
    }
    if let Some(v) = pnumf_opt(p, "topP") {
        c.push(format!("--top-p {}", v));
    }
    if let Some(v) = pnum_opt(p, "topK") {
        c.push(format!("--top-k {}", v));
    }
    if let Some(v) = pnumf_opt(p, "repetitionPenalty") {
        c.push(format!("--repetition-penalty {}", v));
    }
    if let Some(v) = pnum_opt(p, "maxTokens") {
        c.push(format!("--max-tokens {}", v));
    }
    if pbool(p, "trustRemoteCode") {
        c.push("--trust-remote-code".into());
    }
}

/// SGLang 通用参数（原生与 Docker 共用）
fn push_sglang_common(p: &serde_json::Value, c: &mut Vec<String>) {
    if let Some(v) = pnumf_opt(p, "memFractionStatic") {
        c.push(format!("--mem-fraction-static {}", v));
    }
    if let Some(v) = pnum_opt(p, "contextLength") {
        c.push(format!("--context-length {}", v));
    }
    if let Some(v) = pstr_opt(p, "host") {
        c.push(format!("--host {}", v));
    }
    if let Some(v) = pnum_opt(p, "maxNumSeqs") {
        c.push(format!("--max-num-reqs {}", v));
    }
    if let Some(v) = pnum_opt(p, "chunkedPrefillSize") {
        c.push(format!("--chunked-prefill-size {}", v));
    }
    if let Some(v) = pstr_opt(p, "dtype") {
        c.push(format!("--dtype {}", v));
    }
    if let Some(v) = pstr_opt(p, "quantization") {
        c.push(format!("--quantization {}", v));
    }
    if let Some(v) = pnumf_opt(p, "temperature") {
        c.push(format!("--temperature {}", v));
    }
    if let Some(v) = pnumf_opt(p, "topP") {
        c.push(format!("--top-p {}", v));
    }
    if let Some(v) = pnum_opt(p, "topK") {
        c.push(format!("--top-k {}", v));
    }
    if let Some(v) = pnumf_opt(p, "repetitionPenalty") {
        c.push(format!("--repetition-penalty {}", v));
    }
    if let Some(v) = pnum_opt(p, "maxTokens") {
        c.push(format!("--max-tokens {}", v));
    }
    if pbool(p, "trustRemoteCode") {
        c.push("--trust-remote-code".into());
    }
}

fn build_command(cfg: &InstanceConfig) -> Result<String, AppError> {
    let p = &cfg.params;
    let model = cfg.model_path.trim();
    if model.is_empty() {
        return Err(AppError::Other("模型路径不能为空".into()));
    }
    let m = shq(model);

    if cfg.mode == "docker" {
        return docker_command(cfg, m);
    }

    match cfg.framework.as_str() {
        "vllm" | "1cat-vllm" => {
            let bin = pstr(p, "bin", if cfg.framework == "1cat-vllm" { "1cat-vllm" } else { "vllm" });
            let mut c = vec![
                format!("{} serve", bin),
                m.clone(),
                format!("--port {}", cfg.port),
                format!("--tensor-parallel-size {}", pnum(p, "tp", 1)),
            ];
            push_vllm_common(p, &mut c);
            if let Some(v) = pstr_opt(p, "extraArgs") {
                c.push(norm_args(&v));
            }
            Ok(c.join(" "))
        }
        "sglang" => {
            let mut c = vec![
                "python3 -m sglang.launch_server".to_string(),
                format!("--model-path {}", m),
                format!("--port {}", cfg.port),
                format!("--tp {}", pnum(p, "tp", 1)),
            ];
            push_sglang_common(p, &mut c);
            if let Some(v) = pstr_opt(p, "extraArgs") {
                c.push(norm_args(&v));
            }
            Ok(c.join(" "))
        }
        "llama-cpp" => {
            let bin = pstr(p, "bin", "llama-server");
            let mut c = vec![
                bin,
                format!("-m {}", m),
                format!("--port {}", cfg.port),
                format!("-ngl {}", pnum(p, "ngl", 99)),
                format!("-c {}", pnum(p, "ctxSize", 4096)),
                "--metrics".to_string(),
            ];
            let threads = pnum(p, "threads", 0);
            if threads > 0 {
                c.push(format!("--threads {}", threads));
            }
            if let Some(v) = pstr_opt(p, "host") {
                c.push(format!("--host {}", v));
            }
            push_llama_common(p, &mut c);
            if pbool(p, "mtp") {
                c.push("--spec-type draft-mtp".into());
                c.push(format!(
                    "--spec-draft-n-max {}",
                    pnum(p, "specDraftNMax", 4)
                ));
                c.push("--spec-draft-p-min 1".into());
            }
            if let Some(v) = pstr_opt(p, "extraArgs") {
                c.push(norm_args(&v));
            }
            Ok(c.join(" "))
        }
        other => Err(AppError::Other(format!("未知框架: {other}"))),
    }
}

fn docker_command(cfg: &InstanceConfig, m: String) -> Result<String, AppError> {
    let p = &cfg.params;
    let s = slug(&cfg.name);
    let image = cfg
        .docker_image
        .clone()
        .filter(|x| !x.trim().is_empty())
        .unwrap_or_else(|| default_docker_image(&cfg.framework));
    let mut run = vec![
        "docker run -d".to_string(),
        "--gpus all".into(),
        format!("--name {}", s),
        format!("-p {}:{}", cfg.port, cfg.port),
        format!("-v {}:{}", m, m),
        image,
    ];
    // 自定义镜像：启动参数完全由用户定义（模型已按原路径挂载进容器）
    if let Some(custom) = pstr_opt(p, "customCmd") {
        run.push(norm_args(&custom));
        return Ok(run.join(" "));
    }
    match cfg.framework.as_str() {
        "vllm" | "1cat-vllm" => {
            run.push(format!("--model {}", m));
            run.push(format!("--port {}", cfg.port));
            run.push(format!("--tensor-parallel-size {}", pnum(p, "tp", 1)));
            push_vllm_common(p, &mut run);
            if let Some(v) = pstr_opt(p, "extraArgs") {
                run.push(norm_args(&v));
            }
        }
        "sglang" => {
            run.push(format!("--model-path {}", m));
            run.push(format!("--port {}", cfg.port));
            run.push(format!("--tp {}", pnum(p, "tp", 1)));
            push_sglang_common(p, &mut run);
            if let Some(v) = pstr_opt(p, "extraArgs") {
                run.push(norm_args(&v));
            }
        }
        "llama-cpp" => {
            run.push(format!("--port {}", cfg.port));
            run.push(format!("-m {}", m));
            run.push(format!("-ngl {}", pnum(p, "ngl", 99)));
            run.push(format!("-c {}", pnum(p, "ctxSize", 4096)));
            run.push("--host 0.0.0.0".into());
            run.push("--metrics".into());
            push_llama_common(p, &mut run);
            if pbool(p, "mtp") {
                run.push("--spec-type draft-mtp".into());
                run.push(format!(
                    "--spec-draft-n-max {}",
                    pnum(p, "specDraftNMax", 4)
                ));
                run.push("--spec-draft-p-min 1".into());
            }
            if let Some(v) = pstr_opt(p, "extraArgs") {
                run.push(norm_args(&v));
            }
        }
        other => return Err(AppError::Other(format!("未知框架: {other}"))),
    }
    Ok(run.join(" "))
}

fn default_docker_image(fw: &str) -> String {
    match fw {
        "vllm" => "vllm/vllm-openai:latest".into(),
        "1cat-vllm" => "ghcr.io/chenyb999-zhcn/1cat-vllm:1.5-preview".into(),
        "sglang" => "lmsysorg/sglang:latest-cu129".into(),
        _ => "ghcr.io/ggml-org/llama.cpp:server-cuda".into(),
    }
}

fn load_instances(app: &AppHandle) -> Result<Vec<InstanceConfig>, AppError> {
    let store = app
        .store("remotellm.json")
        .map_err(|e| AppError::Other(format!("store: {e}")))?;
    Ok(store
        .get("instances")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default())
}

fn save_instances(app: &AppHandle, list: &[InstanceConfig]) -> Result<(), AppError> {
    let store = app
        .store("remotellm.json")
        .map_err(|e| AppError::Other(format!("store: {e}")))?;
    store.set(
        "instances",
        serde_json::to_value(list).map_err(|e| AppError::Other(e.to_string()))?,
    );
    store
        .save()
        .map_err(|e| AppError::Other(format!("store save: {e}")))
}

async fn get_instance(
    app: &AppHandle,
    state: &State<'_, crate::AppState>,
    id: &str,
) -> Result<(InstanceConfig, crate::profile::ServerProfile), AppError> {
    let inst = load_instances(app)?
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| AppError::Other(format!("实例不存在: {id}")))?;
    let profile = crate::profile::load_profiles(app)?
        .into_iter()
        .find(|p| p.id == inst.profile_id)
        .ok_or_else(|| AppError::Other("服务器档案不存在".into()))?;
    let conns = state.conns.lock().await;
    if !conns.contains_key(&profile.id) {
        return Err(AppError::NotConnected(profile.id.clone()));
    }
    drop(conns);
    Ok((inst, profile))
}

async fn run_on(
    state: &State<'_, crate::AppState>,
    profile_id: &str,
    cmd: &str,
) -> Result<String, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(profile_id) else {
        return Err(AppError::NotConnected(profile_id.to_string()));
    };
    let out = session.run(cmd).await?;
    Ok(out.stdout)
}

#[tauri::command]
pub async fn list_instances(app: AppHandle) -> Result<Vec<InstanceConfig>, AppError> {
    Ok(load_instances(&app)?)
}

#[tauri::command]
pub async fn save_instance(app: AppHandle, cfg: InstanceConfig) -> Result<Vec<InstanceConfig>, AppError> {
    let mut list = load_instances(&app)?;
    if let Some(slot) = list.iter_mut().find(|i| i.id == cfg.id) {
        *slot = cfg.clone();
    } else {
        list.push(cfg);
    }
    save_instances(&app, &list)?;
    Ok(list)
}

#[tauri::command]
pub async fn delete_instance(app: AppHandle, id: String) -> Result<Vec<InstanceConfig>, AppError> {
    let mut list = load_instances(&app)?;
    list.retain(|i| i.id != id);
    save_instances(&app, &list)?;
    Ok(list)
}

const DETECT_SCRIPT: &str = r#"
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
_f=$(find "$HOME/RemoteLLM/frameworks" -maxdepth 4 -name llama-server -type f 2>/dev/null | head -1)
if [ -z "$_v" ] && [ -n "$_f" ]; then _v=$("$_f" --version 2>&1 | head -1); fi
[ -n "$_v" ] && echo "$_v"
if [ -n "$_f" ]; then
  _d=$("$_f" --list-devices 2>/dev/null | grep -oE 'CUDA[0-9]+' | head -1)
  [ -n "$_d" ] && echo "$_f ($_d)" || echo "$_f (CPU only)"
fi
[ -z "$_v" ] && [ -z "$_f" ] && echo NONE
exit 0
"#;

const DETECT_FRAMEWORKS: [(&str, &str); 4] = [
    ("vllm", "VLLM"),
    ("1cat-vllm", "1CAT"),
    ("sglang", "SGLANG"),
    ("llama-cpp", "LLAMA"),
];

/// 解析 DETECT_SCRIPT 输出（每段空时输出 NONE，避免把下一段标记当内容）
pub fn parse_detect(raw: &str) -> Vec<FwDetect> {
    let mut result = Vec::new();
    for (fw, marker) in DETECT_FRAMEWORKS {
        let section = raw
            .split(&format!("=={marker}=="))
            .nth(1)
            .and_then(|rest| rest.split("\n==").next())
            .unwrap_or("");
        let lines: Vec<String> = section
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .filter(|l| !l.eq_ignore_ascii_case("NONE"))
            .filter(|l| !l.starts_with("=="))
            .collect();
        result.push(FwDetect {
            framework: fw.to_string(),
            installed: !lines.is_empty(),
            version: lines.first().cloned(),
            note: lines.get(1).cloned(),
        });
    }
    result
}

#[tauri::command]
pub async fn detect_frameworks(
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<Vec<FwDetect>, AppError> {
    let out = run_on(&state, &profile_id, DETECT_SCRIPT).await?;
    Ok(parse_detect(&out))
}

#[tauri::command]
pub fn preview_command(cfg: InstanceConfig) -> Result<String, AppError> {
    build_command(&cfg)
}

fn start_script(cfg: &InstanceConfig, profile: &crate::profile::ServerProfile) -> String {
    let run = profile.run_dir();
    let logs = profile.logs_dir();
    let s = slug(&cfg.name);
    let pidfile = format!("{run}/{s}.pid");
    let log = format!("{logs}/{s}.log");

    if cfg.mode == "docker" {
        let cmd = build_command(cfg).unwrap_or_default();
        return format!(
            "docker ps -a --format '{{{{.Names}}}}' | grep -qx {s} && docker rm -f {s} >/dev/null 2>&1\n{cmd}\necho \"DOCKER_STARTED\"",
        );
    }

    let mut cmd = build_command(cfg).unwrap_or_default();
    let mut prelude = String::new();
    // llama-cpp 裸命令名不在 PATH 时自动回退到一键安装目录
    if cfg.framework == "llama-cpp" {
        let bin = pstr(&cfg.params, "bin", "llama-server");
        if !bin.is_empty() && !bin.contains('/') && cmd.starts_with(bin.as_str()) {
            let fw_bin = format!(
                "{}/frameworks/llama.cpp/build/bin/{}",
                profile.base_dir.trim_end_matches('/'),
                bin
            );
            prelude = format!(
                "llama_bin=$(command -v {bin} 2>/dev/null)\n\
                 [ -z \"$llama_bin\" ] && [ -x {fw_bin} ] && llama_bin={fw_bin}\n\
                 [ -z \"$llama_bin\" ] && llama_bin={bin}\n"
            );
            cmd = format!("\"$llama_bin\"{}", &cmd[bin.len()..]);
        }
    }
    format!(
        "mkdir -p {run} {logs}\n\
         if [ -f {pidfile} ] && kill -0 \"$(cat {pidfile})\" 2>/dev/null; then echo ALREADY_RUNNING; exit 3; fi\n\
         {prelude}\
         nohup {cmd} > {log} 2>&1 &\n\
         echo $! > {pidfile}\n\
         sleep 1\n\
         if kill -0 \"$(cat {pidfile})\" 2>/dev/null; then echo STARTED pid=$(cat {pidfile}); else echo START_FAILED; tail -n 20 {log}; exit 4; fi"
    )
}

fn stop_script(cfg: &InstanceConfig, profile: &crate::profile::ServerProfile) -> String {
    let run = profile.run_dir();
    let s = slug(&cfg.name);
    let pidfile = format!("{run}/{s}.pid");
    if cfg.mode == "docker" {
        return format!(
            "docker stop {s} >/dev/null 2>&1 && docker rm {s} >/dev/null 2>&1; echo DOCKER_STOPPED"
        );
    }
    format!(
        "if [ -f {pidfile} ] && kill -0 \"$(cat {pidfile})\" 2>/dev/null; then \
         kill \"$(cat {pidfile})\" 2>/dev/null; sleep 2; \
         kill -0 \"$(cat {pidfile})\" 2>/dev/null && kill -9 \"$(cat {pidfile})\" 2>/dev/null; \
         rm -f {pidfile}; echo STOPPED; else echo NOT_RUNNING; fi"
    )
}

fn status_script(cfg: &InstanceConfig, profile: &crate::profile::ServerProfile) -> String {
    let run = profile.run_dir();
    let s = slug(&cfg.name);
    let pidfile = format!("{run}/{s}.pid");
    let head = if cfg.mode == "docker" {
        format!(
            "if docker ps --format '{{{{.Names}}}}' | grep -qx {s}; then echo \"RUN $(docker inspect -f '{{{{.State.Pid}}}}' {s} 2>/dev/null)\"; else echo STOP; fi"
        )
    } else {
        format!(
            "if [ -f {pidfile} ] && kill -0 \"$(cat {pidfile})\" 2>/dev/null; then echo \"RUN $(cat {pidfile})\"; else echo STOP; fi"
        )
    };
    format!(
        "{head}\n\
         code=$(curl -s -o /dev/null -w '%{{http_code}}' --max-time 3 http://localhost:{port}/health 2>/dev/null || echo 000)\n\
         echo \"HEALTH $code\"\n\
         mj=$(curl -s --max-time 3 http://localhost:{port}/v1/models 2>/dev/null | head -c 3000)\n\
         echo \"MODELS $mj\"",
        port = cfg.port
    )
}

#[tauri::command]
pub async fn instance_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<String, AppError> {
    let (inst, profile) = get_instance(&app, &state, &id).await?;
    let script = start_script(&inst, &profile);
    let out = run_on(&state, &profile.id, &script).await?;
    let out = out.trim().to_string();
    if out.contains("START_FAILED") || (out.contains("error") && !out.contains("DOCKER_STARTED"))
    {
        Err(AppError::Other(out))
    } else {
        Ok(out)
    }
}

#[tauri::command]
pub async fn instance_stop(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<String, AppError> {
    let (inst, profile) = get_instance(&app, &state, &id).await?;
    let script = stop_script(&inst, &profile);
    let out = run_on(&state, &profile.id, &script).await?;
    Ok(out.trim().to_string())
}

#[tauri::command]
pub async fn instance_status(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<InstanceStatus, AppError> {
    let (inst, profile) = get_instance(&app, &state, &id).await?;
    let script = status_script(&inst, &profile);
    let out = run_on(&state, &profile.id, &script).await?;
    let mut running = false;
    let mut pid: Option<u32> = None;
    let mut health_code: Option<u16> = None;
    let mut models_json: Option<String> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("RUN ") {
            running = true;
            pid = p.trim().parse().ok();
        } else if line == "STOP" {
            running = false;
        } else if let Some(c) = line.strip_prefix("HEALTH ") {
            health_code = c.trim().parse().ok();
        } else if let Some(m) = line.strip_prefix("MODELS ") {
            models_json = (!m.trim().is_empty()).then(|| m.trim().to_string());
        }
    }
    Ok(InstanceStatus {
        id: inst.id,
        running,
        pid,
        health_code,
        models_json,
        detail: None,
    })
}

#[tauri::command]
pub async fn instance_logs(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    lines: u32,
) -> Result<String, AppError> {
    let (inst, profile) = get_instance(&app, &state, &id).await?;
    let s = slug(&inst.name);
    let log = format!("{}/{}.log", profile.logs_dir(), s);
    let cmd = format!("tail -n {} {} 2>/dev/null || echo '(无日志)'", lines, log);
    Ok(run_on(&state, &profile.id, &cmd).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_detect_all_none() {
        let raw = "\n==VLLM==\nNONE\n==1CAT==\nNONE\n==SGLANG==\nNONE\n==LLAMA==\nNONE\n";
        let r = parse_detect(raw);
        assert_eq!(r.len(), 4);
        for d in &r {
            assert!(!d.installed, "{}", d.framework);
            assert_eq!(d.version, None);
            assert_eq!(d.note, None);
        }
    }

    #[test]
    fn parse_detect_legacy_marker_leak() {
        // 旧脚本空段时会把下一段标记当内容（vLLM 误报 "已安装 ==1CAT=="）
        let raw = "\n==VLLM==\n==1CAT==\n==SGLANG==\nNONE\n==LLAMA==\nNONE\n";
        let r = parse_detect(raw);
        for d in &r {
            assert!(!d.installed, "{}", d.framework);
        }
    }

    #[test]
    fn parse_detect_llama_device_note() {
        let raw = "\n==VLLM==\nNONE\n==1CAT==\nNONE\n==SGLANG==\nNONE\n==LLAMA==\nversion: 0.4.0-dev (build 1, commit 73a43d1)\n/home/chenyb/RemoteLLM/frameworks/llama.cpp/build/bin/llama-server (CUDA0)\n";
        let r = parse_detect(raw);
        let llama = &r[3];
        assert!(llama.installed);
        assert_eq!(
            llama.note.as_deref(),
            Some("/home/chenyb/RemoteLLM/frameworks/llama.cpp/build/bin/llama-server (CUDA0)")
        );
    }

    #[test]
    fn parse_detect_installed() {
        let raw = "\n==VLLM==\nvLLM version 0.9.2\npython-vllm 0.9.2\n==1CAT==\nNONE\n==SGLANG==\nsglang 0.4.5\n==LLAMA==\nllama-server version 1234\n/home/chenyb/RemoteLLM/frameworks/llama-server\n";
        let r = parse_detect(raw);
        let vllm = &r[0];
        assert!(vllm.installed);
        assert_eq!(vllm.version.as_deref(), Some("vLLM version 0.9.2"));
        assert_eq!(vllm.note.as_deref(), Some("python-vllm 0.9.2"));
        let sglang = &r[2];
        assert!(sglang.installed);
        assert_eq!(sglang.version.as_deref(), Some("sglang 0.4.5"));
        assert_eq!(sglang.note, None);
        let llama = &r[3];
        assert!(llama.installed);
        assert_eq!(llama.note.as_deref(), Some("/home/chenyb/RemoteLLM/frameworks/llama-server"));
    }

    fn test_cfg(params: serde_json::Value) -> InstanceConfig {
        InstanceConfig {
            id: "t".into(),
            profile_id: "p".into(),
            name: "t".into(),
            framework: "llama-cpp".into(),
            mode: "native".into(),
            model_path: "/mnt/m.gguf".into(),
            port: 8080,
            docker_image: None,
            params,
            created_at: 0,
        }
    }

    #[test]
    fn build_command_trims_extra_args_newline() {
        // 附加参数末尾带换行会截断 nohup 启动脚本，必须清洗为单空格
        let cfg = test_cfg(serde_json::json!({
            "extraArgs": "--spec-draft-p-min 1\n",
            "host": "0.0.0.0"
        }));
        let cmd = build_command(&cfg).unwrap();
        assert!(!cmd.contains('\n'), "cmd 含换行: {cmd}");
        assert!(cmd.ends_with("--spec-draft-p-min 1"), "cmd: {cmd}");
    }

    #[test]
    fn build_command_llama_mtp_flags() {
        let cfg = test_cfg(serde_json::json!({
            "mtp": true,
            "specDraftNMax": 4,
            "extraArgs": "  --foo   bar  "
        }));
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.contains("--spec-type draft-mtp --spec-draft-n-max 4 --spec-draft-p-min 1"), "cmd: {cmd}");
        // 多余空白压缩为单空格
        assert!(cmd.contains("--foo bar"), "cmd: {cmd}");
    }

    fn test_cfg_fw(framework: &str, params: serde_json::Value) -> InstanceConfig {
        let mut c = test_cfg(params);
        c.framework = framework.into();
        c
    }

    #[test]
    fn build_command_vllm_sampling_flags() {
        let cfg = test_cfg_fw(
            "vllm",
            serde_json::json!({
                "tp": 2,
                "maxModelLen": 8192,
                "gpuMemUtil": 0.85,
                "dtype": "float16",
                "maxNumSeqs": 64,
                "quantization": "fp8",
                "trustRemoteCode": true,
                "temperature": 0.3,
                "topP": 0.9,
                "topK": 20,
                "repetitionPenalty": 1.1,
                "maxTokens": 1024,
                "extraArgs": "--limit-concurrency 32"
            }),
        );
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.starts_with("vllm serve '/mnt/m.gguf' --port 8080 --tensor-parallel-size 2"), "cmd: {cmd}");
        for flag in [
            "--max-model-len 8192",
            "--gpu-memory-utilization 0.85",
            "--dtype float16",
            "--max-num-seqs 64",
            "--quantization fp8",
            "--trust-remote-code",
            "--temperature 0.3",
            "--top-p 0.9",
            "--top-k 20",
            "--repetition-penalty 1.1",
            "--max-tokens 1024",
            "--limit-concurrency 32",
        ] {
            assert!(cmd.contains(flag), "缺少 {flag}；cmd: {cmd}");
        }
    }

    #[test]
    fn build_command_vllm_docker_sampling_flags() {
        let mut cfg = test_cfg_fw(
            "vllm",
            serde_json::json!({ "tp": 1, "maxNumSeqs": 32, "temperature": 0.5 }),
        );
        cfg.mode = "docker".into();
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.starts_with("docker run -d"), "cmd: {cmd}");
        assert!(cmd.contains("--tensor-parallel-size 1"), "cmd: {cmd}");
        assert!(cmd.contains("--max-num-seqs 32"), "cmd: {cmd}");
        assert!(cmd.contains("--temperature 0.5"), "cmd: {cmd}");
    }

    #[test]
    fn build_command_sglang_sampling_flags() {
        let cfg = test_cfg_fw(
            "sglang",
            serde_json::json!({
                "tp": 1,
                "memFractionStatic": 0.8,
                "contextLength": 16384,
                "host": "0.0.0.0",
                "maxNumSeqs": 128,
                "chunkedPrefillSize": 8192,
                "dtype": "float16",
                "quantization": "awq",
                "trustRemoteCode": true,
                "temperature": 0.7,
                "topP": 0.95,
                "topK": 40,
                "repetitionPenalty": 1.05,
                "maxTokens": 2048
            }),
        );
        let cmd = build_command(&cfg).unwrap();
        assert!(
            cmd.starts_with("python3 -m sglang.launch_server --model-path '/mnt/m.gguf' --port 8080 --tp 1"),
            "cmd: {cmd}"
        );
        for flag in [
            "--mem-fraction-static 0.8",
            "--context-length 16384",
            "--host 0.0.0.0",
            "--max-num-reqs 128",
            "--chunked-prefill-size 8192",
            "--dtype float16",
            "--quantization awq",
            "--trust-remote-code",
            "--temperature 0.7",
            "--top-p 0.95",
            "--top-k 40",
            "--repetition-penalty 1.05",
            "--max-tokens 2048",
        ] {
            assert!(cmd.contains(flag), "缺少 {flag}；cmd: {cmd}");
        }
    }

    #[test]
    fn build_command_sglang_docker_sampling_flags() {
        let mut cfg = test_cfg_fw(
            "sglang",
            serde_json::json!({ "tp": 2, "maxNumSeqs": 64, "topP": 0.9 }),
        );
        cfg.mode = "docker".into();
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.starts_with("docker run -d"), "cmd: {cmd}");
        assert!(cmd.contains("--tp 2"), "cmd: {cmd}");
        assert!(cmd.contains("--max-num-reqs 64"), "cmd: {cmd}");
        assert!(cmd.contains("--top-p 0.9"), "cmd: {cmd}");
    }

    fn test_profile() -> crate::profile::ServerProfile {
        serde_json::from_value(serde_json::json!({
            "id": "p", "name": "t", "host": "h", "user": "u",
            "auth": { "type": "password", "password": "x" },
            "baseDir": "~/RemoteLLM"
        }))
        .unwrap()
    }

    #[test]
    fn start_script_resolves_bare_llama_bin() {
        // bin 为裸命令名时：启动脚本注入 PATH 回退解析，nohup 用 "$llama_bin"
        let mut cfg = test_cfg(serde_json::json!({ "bin": "llama-server" }));
        cfg.mode = "native".into();
        let s = start_script(&cfg, &test_profile());
        assert!(s.contains("llama_bin=$(command -v llama-server"), "script: {s}");
        assert!(s.contains("~/RemoteLLM/frameworks/llama.cpp/build/bin/llama-server"), "script: {s}");
        assert!(s.contains("nohup \"$llama_bin\" -m"), "script: {s}");

        // bin 含路径（用户自定义）时不注入解析
        let mut cfg2 = test_cfg(serde_json::json!({ "bin": "/opt/my/llama-server" }));
        cfg2.mode = "native".into();
        let s2 = start_script(&cfg2, &test_profile());
        assert!(!s2.contains("llama_bin="), "script: {s2}");
        assert!(s2.contains("nohup /opt/my/llama-server -m"), "script: {s2}");
    }
}
