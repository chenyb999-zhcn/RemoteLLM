use serde::Serialize;
use tauri::State;

use crate::error::AppError;

const GPU_POLL_SCRIPT: &str = r#"
echo "==GPU=="
nvidia-smi --query-gpu=index,name,utilization.gpu,memory.total,memory.used,temperature.gpu,power.draw,power.limit --format=csv,noheader 2>/dev/null
echo "==MEM=="
free -b | awk '/^Mem:/ {print $2, $3}'
echo "==DISK=="
df -B1 / | awk 'NR==2 {print $2, $3}'
echo "==UP=="
uptime -p 2>/dev/null | sed 's/^up //'
"#;

const PMON_SCRIPT: &str = "nvidia-smi pmon -c 1 2>/dev/null";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSnap {
    pub index: u32,
    pub name: String,
    pub util: Option<u32>,
    pub mem_total_mb: Option<u32>,
    pub mem_used_mb: Option<u32>,
    pub temp_c: Option<u32>,
    pub power_w: Option<f32>,
    pub power_limit_w: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuPoll {
    pub ts: i64,
    pub gpus: Vec<GpuSnap>,
    pub mem_total: Option<u64>,
    pub mem_used: Option<u64>,
    pub disk_total: Option<u64>,
    pub disk_used: Option<u64>,
    pub uptime: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcRow {
    pub gpu: u32,
    pub pid: u32,
    pub itc: u32,
    pub gmc: u32,
    pub mem: u32,
}

fn section<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    let start_marker = format!("=={name}==\n");
    let start = raw.find(&start_marker)? + start_marker.len();
    let rest = &raw[start..];
    let end = rest.find("\n==").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

fn parse_mb(s: &str) -> Option<u32> {
    let (num, unit) = s.trim().split_once(' ')?;
    let n: f64 = num.parse().ok()?;
    match unit {
        "MiB" => Some(n as u32),
        "GiB" => Some((n * 1024.0) as u32),
        _ => None,
    }
}

fn parse_two_u64(s: &str) -> (Option<u64>, Option<u64>) {
    let mut it = s.split_whitespace();
    (
        it.next().and_then(|v| v.parse().ok()),
        it.next().and_then(|v| v.parse().ok()),
    )
}

fn parse_power(s: &str) -> Option<f32> {
    let v: f32 = s.trim().parse().ok()?;
    if v.is_finite() && v >= 0.0 {
        Some(v)
    } else {
        None
    }
}

fn parse_gpus(csv: &str) -> Vec<GpuSnap> {
    csv.lines()
        .filter(|l| l.contains(','))
        .map(|l| {
            let f: Vec<&str> = l.split(',').map(|s| s.trim()).collect();
            GpuSnap {
                index: f.first().and_then(|s| s.parse().ok()).unwrap_or(0),
                name: f.get(1).unwrap_or(&"").to_string(),
                util: f.get(2).and_then(|s| s.parse().ok()),
                mem_total_mb: f.get(3).and_then(|s| parse_mb(s)),
                mem_used_mb: f.get(4).and_then(|s| parse_mb(s)),
                temp_c: f.get(5).and_then(|s| s.parse().ok()),
                power_w: f.get(6).and_then(|s| parse_power(s)),
                power_limit_w: f.get(7).and_then(|s| parse_power(s)),
            }
        })
        .collect()
}

fn parse_pmon(raw: &str) -> Vec<ProcRow> {
    raw.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 6 {
                return None;
            }
            Some(ProcRow {
                gpu: f[0].parse().ok()?,
                pid: f[1].parse().ok()?,
                itc: f[2].parse().ok()?,
                gmc: f[3].parse().ok()?,
                mem: f[4].parse().ok()?,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn gpu_poll(state: State<'_, crate::AppState>, id: String) -> Result<GpuPoll, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&id) else {
        return Err(AppError::NotConnected(id));
    };
    let out = session.run(GPU_POLL_SCRIPT).await?;
    let raw = out.stdout.as_str();
    let (mem_total, mem_used) = section(raw, "MEM").map(parse_two_u64).unwrap_or((None, None));
    let (disk_total, disk_used) = section(raw, "DISK").map(parse_two_u64).unwrap_or((None, None));
    let uptime = section(raw, "UP").map(|s| s.trim().to_string());
    Ok(GpuPoll {
        ts: chrono::Utc::now().timestamp_millis(),
        gpus: section(raw, "GPU").map(parse_gpus).unwrap_or_default(),
        mem_total,
        mem_used,
        disk_total,
        disk_used,
        uptime,
    })
}

#[tauri::command]
pub async fn gpu_proc_poll(state: State<'_, crate::AppState>, id: String) -> Result<Vec<ProcRow>, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&id) else {
        return Err(AppError::NotConnected(id));
    };
    let out = session.run(PMON_SCRIPT).await?;
    Ok(parse_pmon(&out.stdout))
}

/// 通过 SSH 在服务器上抓取推理服务的 Prometheus 指标
#[tauri::command]
pub async fn metrics_poll(
    state: State<'_, crate::AppState>,
    id: String,
    port: u16,
) -> Result<Vec<crate::metrics_parse::MetricSample>, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&id) else {
        return Err(AppError::NotConnected(id));
    };
    let cmd = format!("curl -s --max-time 5 http://localhost:{port}/metrics");
    let out = session.run(&cmd).await?;
    Ok(crate::metrics_parse::parse(&out.stdout))
}
