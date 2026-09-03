use serde::Serialize;
use tauri::State;

use crate::error::AppError;
use crate::ssh::SshSession;

const ENV_SCRIPT: &str = r#"
echo "==OS=="
grep PRETTY_NAME /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"'; uname -sr
echo "==CPU=="
nproc; grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2
echo "==MEM=="
free -b | awk '/^Mem:/ {print $2, $3}'
echo "==DISK=="
df -B1 / | awk 'NR==2 {print $2, $3}'
echo "==PYTHON=="
python3 --version 2>/dev/null
echo "==CUDA=="
nvcc --version 2>/dev/null | grep -o 'release [0-9.]*' | head -1
ls -d /usr/local/cuda* 2>/dev/null | head -1
echo "==DOCKER=="
docker --version 2>/dev/null | head -1
echo "==GPU=="
nvidia-smi --query-gpu=index,name,driver_version,memory.total,memory.used,temperature.gpu,power.draw,utilization.gpu --format=csv,noheader 2>/dev/null
echo "==END=="
"#;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub index: u32,
    pub name: String,
    pub driver_version: String,
    pub mem_total_mb: Option<u32>,
    pub mem_used_mb: Option<u32>,
    pub temp_c: Option<u32>,
    pub power_w: Option<f32>,
    pub util: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvInfo {
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub cpu_count: Option<u32>,
    pub cpu_model: Option<String>,
    pub mem_total: Option<u64>,
    pub mem_used: Option<u64>,
    pub disk_total: Option<u64>,
    pub disk_used: Option<u64>,
    pub python: Option<String>,
    pub cuda: Option<String>,
    pub cuda_path: Option<String>,
    pub docker: Option<String>,
    pub driver: String,
    pub gpus: Vec<GpuInfo>,
}

fn section<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    let start_marker = format!("=={name}==\n");
    let start = raw.find(&start_marker)? + start_marker.len();
    let rest = &raw[start..];
    let end = rest.find("\n==").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

fn parse_mb(s: &str) -> Option<u32> {
    let s = s.trim();
    let (num, unit) = s.split_once(' ')?;
    let n: f64 = num.parse().ok()?;
    match unit {
        "MiB" => Some(n as u32),
        "GiB" => Some((n * 1024.0) as u32),
        _ => None,
    }
}

fn parse_power(s: &str) -> Option<f32> {
    s.trim().split(' ').next()?.parse().ok()
}

fn parse_gpus(csv: &str) -> Vec<GpuInfo> {
    csv.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(',').map(|s| s.trim()).collect();
            GpuInfo {
                index: f.first().and_then(|s| s.parse().ok()).unwrap_or(0),
                name: f.get(1).unwrap_or(&"").to_string(),
                driver_version: f.get(2).unwrap_or(&"").to_string(),
                mem_total_mb: f.get(3).and_then(|s| parse_mb(s)),
                mem_used_mb: f.get(4).and_then(|s| parse_mb(s)),
                temp_c: f.get(5).and_then(|s| s.parse().ok()),
                power_w: f.get(6).and_then(|s| parse_power(s)),
                util: f.get(7).and_then(|s| s.replace('%', "").parse().ok()),
            }
        })
        .collect()
}

fn parse_two_u64(s: &str) -> (Option<u64>, Option<u64>) {
    let mut it = s.split_whitespace();
    (
        it.next().and_then(|v| v.parse().ok()),
        it.next().and_then(|v| v.parse().ok()),
    )
}

pub async fn env_check(session: &mut SshSession) -> Result<EnvInfo, AppError> {
    let out = session.run(ENV_SCRIPT).await?;
    let raw = out.stdout.as_str();

    let (os, kernel) = section(raw, "OS")
        .and_then(|s| s.split_once('\n'))
        .map(|(a, b)| (Some(a.trim().to_string()), Some(b.trim().to_string())))
        .unwrap_or((None, None));

    let cpu = section(raw, "CPU").unwrap_or("");
    let mut cpu_lines = cpu.lines().filter(|l| !l.trim().is_empty());
    let cpu_count = cpu_lines.next().and_then(|l| l.parse().ok());
    let cpu_model = cpu_lines.next().map(|l| l.trim().to_string());

    let (mem_total, mem_used) = section(raw, "MEM")
        .map(|s| parse_two_u64(s))
        .unwrap_or((None, None));

    let (disk_total, disk_used) = section(raw, "DISK")
        .map(|s| parse_two_u64(s))
        .unwrap_or((None, None));

    let python = section(raw, "PYTHON").map(|s| s.trim().to_string());

    let cuda_sec = section(raw, "CUDA").unwrap_or("");
    let mut cuda_lines = cuda_sec.lines().filter(|l| !l.trim().is_empty());
    let cuda = cuda_lines.next().map(|l| l.trim().to_string());
    let cuda_path = cuda_lines.next().map(|l| l.trim().to_string());

    let docker = section(raw, "DOCKER").map(|s| s.trim().to_string());

    let gpus = section(raw, "GPU").map(parse_gpus).unwrap_or_default();
    let driver = gpus.first().map(|g| g.driver_version.clone()).filter(|d| !d.is_empty());

    Ok(EnvInfo {
        os,
        kernel,
        cpu_count,
        cpu_model,
        mem_total,
        mem_used,
        disk_total,
        disk_used,
        python,
        cuda,
        cuda_path,
        docker,
        driver: driver.unwrap_or_default(),
        gpus,
    })
}

#[tauri::command]
pub async fn env_check_cmd(
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<EnvInfo, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&id) else {
        return Err(AppError::NotConnected(id));
    };
    env_check(session).await
}
