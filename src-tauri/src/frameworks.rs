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
pub fn build_command(cfg: &InstanceConfig) -> Result<String, AppError> {
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
            if let Some(v) = pstr_opt(p, "extraArgs") {
                c.push(v);
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
            if let Some(v) = pnumf_opt(p, "memFractionStatic") {
                c.push(format!("--mem-fraction-static {}", v));
            }
            if let Some(v) = pnum_opt(p, "contextLength") {
                c.push(format!("--context-length {}", v));
            }
            if let Some(v) = pstr_opt(p, "host") {
                c.push(format!("--host {}", v));
            }
            if let Some(v) = pstr_opt(p, "extraArgs") {
                c.push(v);
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
            ];
            let threads = pnum(p, "threads", 0);
            if threads > 0 {
                c.push(format!("--threads {}", threads));
            }
            if let Some(v) = pstr_opt(p, "host") {
                c.push(format!("--host {}", v));
            }
            if let Some(v) = pstr_opt(p, "extraArgs") {
                c.push(v);
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
        format!("-p {}:{},", cfg.port, cfg.port),
        format!("-v {}:{}", m, m),
        image,
    ];
    match cfg.framework.as_str() {
        "vllm" | "1cat-vllm" => {
            run.push(format!("--model {}", m));
            run.push(format!("--port {}", cfg.port));
            run.push(format!("--tensor-parallel-size {}", pnum(p, "tp", 1)));
            if let Some(v) = pnum_opt(p, "maxModelLen") {
                run.push(format!("--max-model-len {}", v));
            }
            if let Some(v) = pnumf_opt(p, "gpuMemUtil") {
                run.push(format!("--gpu-memory-utilization {}", v));
            }
            if pbool(p, "enforceEager") {
                run.push("--enforce-eager".into());
            }
            if let Some(v) = pstr_opt(p, "extraArgs") {
                run.push(v);
            }
        }
        "sglang" => {
            run.push(format!("--model-path {}", m));
            run.push(format!("--port {}", cfg.port));
            run.push(format!("--tp {}", pnum(p, "tp", 1)));
            if let Some(v) = pnumf_opt(p, "memFractionStatic") {
                run.push(format!("--mem-fraction-static {}", v));
            }
            if let Some(v) = pstr_opt(p, "extraArgs") {
                run.push(v);
            }
        }
        "llama-cpp" => {
            run.push(format!("--port {}", cfg.port));
            run.push(format!("-m {}", m));
            run.push(format!("-ngl {}", pnum(p, "ngl", 99)));
            run.push(format!("-c {}", pnum(p, "ctxSize", 4096)));
            run.push("--host 0.0.0.0".into());
            if let Some(v) = pstr_opt(p, "extraArgs") {
                run.push(v);
            }
        }
        other => return Err(AppError::Other(format!("未知框架: {other}"))),
    }
    Ok(run.join(" "))
}

fn default_docker_image(fw: &str) -> String {
    match fw {
        "vllm" => "vllm/vllm-openai:latest".into(),
        "1cat-vllm" => "vllm/vllm-openai:latest".into(),
        "sglang" => "lmsysorg/sglang:latest".into(),
        _ => "ggml-org/llama.cpp:server".into(),
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
(command -v vllm >/dev/null 2>&1 && vllm --version 2>/dev/null | head -1) || true
python3 -c "import vllm; print('python-vllm', vllm.__version__)" 2>/dev/null || true
echo "==1CAT=="
(command -v 1cat-vllm >/dev/null 2>&1 && 1cat-vllm --version 2>/dev/null | head -1) || true
echo "==SGLANG=="
python3 -c "import sglang; print('sglang', sglang.__version__)" 2>/dev/null || true
echo "==LLAMA=="
(command -v llama-server >/dev/null 2>&1 && llama-server --version 2>&1 | head -2) || true
find "$HOME/RemoteLLM/frameworks" -maxdepth 4 -name llama-server -type f 2>/dev/null | head -3
"#;

#[tauri::command]
pub async fn detect_frameworks(
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<Vec<FwDetect>, AppError> {
    let out = run_on(&state, &profile_id, DETECT_SCRIPT).await?;
    let mut result = Vec::new();
    for (fw, marker) in [("vllm", "VLLM"), ("1cat-vllm", "1CAT"), ("sglang", "SGLANG"), ("llama-cpp", "LLAMA")] {
        let section = out
            .split(&format!("=={marker}==\n"))
            .nth(1)
            .and_then(|rest| rest.split("\n==").next());
        let lines: Vec<&str> = section
            .unwrap_or("")
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        let installed = !lines.is_empty();
        result.push(FwDetect {
            framework: fw.to_string(),
            installed,
            version: lines.first().map(|s| s.to_string()),
            note: lines.get(1).map(|s| s.to_string()),
        });
    }
    Ok(result)
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

    let cmd = build_command(cfg).unwrap_or_default();
    format!(
        "mkdir -p {run} {logs}\n\
         if [ -f {pidfile} ] && kill -0 \"$(cat {pidfile})\" 2>/dev/null; then echo ALREADY_RUNNING; exit 3; fi\n\
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
