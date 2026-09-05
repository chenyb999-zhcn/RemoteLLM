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

const PMON_SCRIPT: &str = r#"
echo "==PMON=="
nvidia-smi pmon -c 1 2>/dev/null
echo "==APPS=="
nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv,noheader 2>/dev/null
"#;

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
    /// 计算单元（SM）利用率 %
    pub sm: u32,
    /// 显存带宽利用率 %
    pub mem_bw: u32,
    /// 显存占用 MiB（来自 --query-compute-apps）
    pub mem: Option<u32>,
    pub command: String,
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
    // 形如 "56.69 W"，取第一个 token
    let v: f32 = s.trim().split_whitespace().next()?.parse().ok()?;
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
                util: f.get(2).and_then(|s| s.trim().trim_end_matches('%').trim().parse().ok()),
                mem_total_mb: f.get(3).and_then(|s| parse_mb(s)),
                mem_used_mb: f.get(4).and_then(|s| parse_mb(s)),
                temp_c: f.get(5).and_then(|s| s.parse().ok()),
                power_w: f.get(6).and_then(|s| parse_power(s)),
                power_limit_w: f.get(7).and_then(|s| parse_power(s)),
            }
        })
        .collect()
}

/// pmon 列: gpu pid type sm mem enc dec [jpg ofa] command
/// sm/mem 均为百分比；进程实际显存占用来自 --query-compute-apps
fn parse_pmon(pmon_raw: &str, apps_raw: &str) -> Vec<ProcRow> {
    let mut mem_by_pid: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for line in apps_raw.lines() {
        let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if f.len() < 3 {
            continue;
        }
        if let (Ok(pid), Some(mb)) = (
            f[0].parse::<u32>(),
            f[2].split_whitespace().next().and_then(|v| v.parse::<u32>().ok()),
        ) {
            mem_by_pid.insert(pid, mb);
        }
    }
    pmon_raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 5 {
                return None;
            }
            let gpu = f[0].parse().ok()?;
            let pid = f[1].parse().ok()?;
            Some(ProcRow {
                gpu,
                pid,
                sm: f[3].parse().unwrap_or(0),
                mem_bw: f[4].parse().unwrap_or(0),
                mem: mem_by_pid.get(&pid).copied(),
                command: f.last().unwrap_or(&"").to_string(),
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
    let raw = out.stdout.as_str();
    let pmon = section(raw, "PMON").unwrap_or_default();
    let apps = section(raw, "APPS").unwrap_or_default();
    Ok(parse_pmon(pmon, apps))
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
    let cmd = format!(
        "curl -s --max-time 5 -w '\\n%{{http_code}}' http://localhost:{port}/metrics"
    );
    let out = session.run(&cmd).await?;
    let stdout = out.stdout.as_str();
    let (body, status) = match stdout.rfind('\n') {
        Some(i) => (&stdout[..i], &stdout[i + 1..]),
        None => (stdout, "000"),
    };
    let code: u16 = status.trim().parse().unwrap_or(0);
    if code != 200 {
        let msg = if code == 0 {
            format!("无法连接 localhost:{port}（该端口没有监听服务或 curl 失败）")
        } else {
            let snippet: String = body.trim().chars().take(160).collect();
            if snippet.is_empty() {
                format!("HTTP {code}")
            } else {
                format!("HTTP {code}: {snippet}")
            }
        };
        return Err(AppError::Other(msg));
    }
    Ok(crate::metrics_parse::parse(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gpu_poll_line() {
        // 真实服务器输出（V100, driver 580）
        let g = parse_gpus("0, Tesla V100-PCIE-32GB, 0 %, 32768 MiB, 30802 MiB, 74, 56.69 W, 250.00 W");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].index, 0);
        assert_eq!(g[0].util, Some(0));
        assert_eq!(g[0].mem_total_mb, Some(32768));
        assert_eq!(g[0].mem_used_mb, Some(30802));
        assert_eq!(g[0].temp_c, Some(74));
        assert!((g[0].power_w.unwrap() - 56.69).abs() < 1e-3);
        assert!((g[0].power_limit_w.unwrap() - 250.0).abs() < 1e-3);
    }

    #[test]
    fn parses_gpu_poll_line_busy() {
        let g = parse_gpus("0, NVIDIA H20, 97 %, 97871 MiB, 90000 MiB, 65, 505.32 W, 700.00 W");
        assert_eq!(g[0].util, Some(97));
        assert_eq!(g[0].mem_total_mb, Some(97871));
    }

    const PMON_RAW: &str = "\
# gpu         pid   type     sm    mem    enc    dec    jpg    ofa    command 
# Idx           #    C/G      %      %      %      %      %      %      %    name 
    0       5522     C      0      0      -      -      -      -    llama-server   
";

    const PMON_RAW_OLD: &str = "\
# gpu   pid   type  sm   mem   enc   dec   command
    1      4242     C     99    99      [N/A]   [N/A]   python3
";

    const APPS_RAW: &str = "5522, /root/llama.cpp/build/bin/llama-server, 30792 MiB\n";

    #[test]
    fn parses_pmon_new_layout() {
        let rows = parse_pmon(PMON_RAW, APPS_RAW);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.gpu, 0);
        assert_eq!(r.pid, 5522);
        assert_eq!(r.sm, 0);
        assert_eq!(r.mem_bw, 0);
        assert_eq!(r.mem, Some(30792));
        assert_eq!(r.command, "llama-server");
    }

    #[test]
    fn parses_pmon_old_layout() {
        let rows = parse_pmon(PMON_RAW_OLD, "");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sm, 99);
        assert_eq!(rows[0].mem_bw, 99);
        assert_eq!(rows[0].mem, None);
        assert_eq!(rows[0].command, "python3");
    }

    #[test]
    fn parses_apps_memory() {
        let rows = parse_pmon("0 7  C 10 20 - - python\n", "7, python, 512 MiB\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mem, Some(512));
        assert_eq!(rows[0].command, "python");
    }
}
