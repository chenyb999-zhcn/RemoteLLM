use serde::Serialize;
use tauri::{AppHandle, State};

use crate::docker::{parse_mode, probe_sudo_mode, wrap_line, SudoMode};
use crate::error::AppError;

/// 一次 SSH 采集全部 GPU 管理信号（分节，section 约定与其他模块相同）
const GPU_SCRIPT: &str = r#"
echo "==INFO=="
nvidia-smi --query-gpu=index,name,serial,driver_version,vbios_version,pcie.link.gen.current,pcie.link.gen.max,pcie.link.width.current --format=csv,noheader 2>/dev/null
echo "==STAT=="
nvidia-smi --query-gpu=index,utilization.gpu,memory.total,memory.used,temperature.gpu,power.draw,power.limit,power.min_limit,power.max_limit,power.default_limit,clocks.sm,clocks.max.sm --format=csv,noheader 2>/dev/null
echo "==SET=="
nvidia-smi --query-gpu=index,persistence_mode,compute_mode,ecc.mode.current --format=csv,noheader 2>/dev/null
echo "==THROT=="
nvidia-smi --query-gpu=index,clocks_event_reasons.active --format=csv,noheader 2>/dev/null
echo "==ECC=="
nvidia-smi --query-gpu=index,ecc.errors.corrected.volatile.total,ecc.errors.uncorrected.volatile.total --format=csv,noheader 2>/dev/null
echo "==PMON=="
nvidia-smi pmon -c 1 2>/dev/null
echo "==APPS=="
nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv,noheader 2>/dev/null
echo "==PS=="
_pids=$(nvidia-smi --query-compute-apps=pid --format=csv,noheader 2>/dev/null | tr -d ' ' | grep -v '^$' | tr '\n' ',' | sed 's/,$//')
if [ -n "$_pids" ]; then ps -o user:24,pid:10,etime:16,cmd -p "$_pids" --no-headers 2>/dev/null | cut -c1-200; fi
echo "==TOPO=="
nvidia-smi topo -m 2>/dev/null
exit 0
"#;

// ---------- 数据结构 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuCard {
    pub index: u32,
    pub name: String,
    pub serial: Option<String>,
    pub driver: String,
    pub vbios: Option<String>,
    pub pcie_gen_current: Option<u32>,
    pub pcie_gen_max: Option<u32>,
    pub pcie_width: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuStat {
    pub index: u32,
    pub util: Option<u32>,
    pub mem_total_mb: Option<u64>,
    pub mem_used_mb: Option<u64>,
    pub temp_c: Option<u32>,
    pub power_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub power_min_w: Option<f32>,
    pub power_max_w: Option<f32>,
    pub power_default_w: Option<f32>,
    pub sm_clock_mhz: Option<u32>,
    pub sm_clock_max_mhz: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSetState {
    pub index: u32,
    pub persistence: bool,
    pub compute_mode: Option<String>,
    pub ecc: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuThrottle {
    pub index: u32,
    /// 激活的降频原因（Idle/None 已过滤；空 = 未降频）
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuEcc {
    pub index: u32,
    pub corrected: Option<u64>,
    pub uncorrected: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuProcRow {
    pub gpu: u32,
    pub pid: u32,
    pub user: String,
    pub name: String,
    pub elapsed: String,
    pub mem_mb: Option<u32>,
    /// 属主是当前 SSH 用户（只有自己的进程可结束）
    pub mine: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GpuQueryResult {
    pub cards: Vec<GpuCard>,
    pub stats: Vec<GpuStat>,
    pub settings: Vec<GpuSetState>,
    pub throttles: Vec<GpuThrottle>,
    pub ecc: Vec<GpuEcc>,
    pub procs: Vec<GpuProcRow>,
    pub topo: Option<String>,
}

// ---------- 解析 ----------

fn section<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    let start_marker = format!("=={name}==\n");
    let start = raw.find(&start_marker)? + start_marker.len();
    let rest = &raw[start..];
    let end = rest.find("\n==").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

fn csv_lines(s: &str) -> impl Iterator<Item = Vec<String>> + '_ {
    s.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.split(',').map(|f| f.trim().to_string()).collect())
}

/// "[N/A]" / "[Not Supported]" / "" → None
fn opt(v: &str) -> Option<&str> {
    let t = v.trim();
    if t.is_empty() || t == "[N/A]" || t == "[Not Supported]" {
        None
    } else {
        Some(t)
    }
}

fn parse_u32(v: &str) -> Option<u32> {
    opt(v)?.trim_end_matches('%').trim().parse().ok()
}

/// "16x" / "16" → 16
fn parse_width(v: &str) -> Option<u32> {
    let t = opt(v)?.trim_end_matches(['x', 'X']);
    t.parse().ok()
}

fn parse_u64(v: &str) -> Option<u64> {
    opt(v)?.parse().ok()
}

fn parse_mb(v: &str) -> Option<u64> {
    let t = opt(v)?;
    let (num, unit) = t.split_once(' ')?;
    let n: f64 = num.parse().ok()?;
    match unit {
        "MiB" => Some(n as u64),
        "GiB" => Some((n * 1024.0) as u64),
        _ => None,
    }
}

fn parse_w(v: &str) -> Option<f32> {
    let t = opt(v)?;
    let n: f32 = t.split_whitespace().next()?.parse().ok()?;
    (n.is_finite() && n >= 0.0).then_some(n)
}

fn parse_bool(v: &str) -> Option<bool> {
    match opt(v)?.to_ascii_lowercase().as_str() {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

/// clocks_event_reasons 位掩码（新驱动输出 0x.. 格式；旧驱动输出命名分号串）
const THROTTLE_BITS: &[(u64, &str)] = &[
    (1, "GPU Idle"),
    (1 << 1, "Applications Clocks Setting"),
    (1 << 2, "SW Power Cap"),
    (1 << 3, "HW Slowdown"),
    (1 << 4, "Sync Boost"),
    (1 << 5, "SW Thermal Slowdown"),
    (1 << 6, "HW Thermal Slowdown"),
    (1 << 7, "HW Power Brake Slowdown"),
    (1 << 8, "Display Clock Setting"),
];

/// 解析降频原因：命名串（"SW Power Cap; Idle"）或位掩码（"0x0000000000000004"）；
/// Idle/None 一律过滤（空闲不算降频）
fn parse_throttle_reasons(v: &str) -> Vec<String> {
    let t = match opt(v) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let raw: Vec<String> = if t.starts_with("0x") || t.starts_with("0X") {
        let mask = match u64::from_str_radix(t.trim_start_matches("0x").trim_start_matches("0X"), 16) {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        THROTTLE_BITS
            .iter()
            .filter(|(bit, _)| mask & bit != 0)
            .map(|(_, name)| name.to_string())
            .collect()
    } else {
        t.split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty() && opt(s).is_some())
            .map(str::to_string)
            .collect()
    };
    raw.into_iter()
        .filter(|s| {
            let low = s.to_ascii_lowercase();
            !low.eq_ignore_ascii_case("idle") && !low.eq_ignore_ascii_case("gpu idle") && !low.eq_ignore_ascii_case("none")
        })
        .collect()
}

/// 解析 GPU_SCRIPT 输出（纯函数，可单测）。current_user 用于标记进程归属
pub fn parse_gpu_query(raw: &str, current_user: &str) -> GpuQueryResult {
    let mut r = GpuQueryResult::default();

    for f in csv_lines(section(raw, "INFO").unwrap_or("")) {
        if f.len() < 5 {
            continue;
        }
        r.cards.push(GpuCard {
            index: f[0].parse().unwrap_or(0),
            name: f.get(1).cloned().unwrap_or_default(),
            serial: f.get(2).and_then(|v| opt(v).map(str::to_string)),
            driver: f.get(3).cloned().unwrap_or_default(),
            vbios: f.get(4).and_then(|v| opt(v).map(str::to_string)),
            pcie_gen_current: f.get(5).and_then(|v| parse_u32(v)),
            pcie_gen_max: f.get(6).and_then(|v| parse_u32(v)),
            pcie_width: f.get(7).and_then(|v| parse_width(v)),
        });
    }

    for f in csv_lines(section(raw, "STAT").unwrap_or("")) {
        if f.len() < 4 {
            continue;
        }
        r.stats.push(GpuStat {
            index: f[0].parse().unwrap_or(0),
            util: f.get(1).and_then(|v| parse_u32(v)),
            mem_total_mb: f.get(2).and_then(|v| parse_mb(v)),
            mem_used_mb: f.get(3).and_then(|v| parse_mb(v)),
            temp_c: f.get(4).and_then(|v| parse_u32(v)),
            power_w: f.get(5).and_then(|v| parse_w(v)),
            power_limit_w: f.get(6).and_then(|v| parse_w(v)),
            power_min_w: f.get(7).and_then(|v| parse_w(v)),
            power_max_w: f.get(8).and_then(|v| parse_w(v)),
            power_default_w: f.get(9).and_then(|v| parse_w(v)),
            sm_clock_mhz: f.get(10).and_then(|v| parse_u32(v)),
            sm_clock_max_mhz: f.get(11).and_then(|v| parse_u32(v)),
        });
    }

    for f in csv_lines(section(raw, "SET").unwrap_or("")) {
        if f.is_empty() {
            continue;
        }
        r.settings.push(GpuSetState {
            index: f[0].parse().unwrap_or(0),
            persistence: f.get(1).and_then(|v| parse_bool(v)).unwrap_or(false),
            compute_mode: f.get(2).and_then(|v| opt(v).map(str::to_string)),
            ecc: f.get(3).and_then(|v| parse_bool(v)),
        });
    }

    for f in csv_lines(section(raw, "THROT").unwrap_or("")) {
        if f.is_empty() {
            continue;
        }
        let reasons = f
            .get(1)
            .map(|v| parse_throttle_reasons(v))
            .unwrap_or_default();
        r.throttles.push(GpuThrottle {
            index: f[0].parse().unwrap_or(0),
            reasons,
        });
    }

    for f in csv_lines(section(raw, "ECC").unwrap_or("")) {
        if f.is_empty() {
            continue;
        }
        r.ecc.push(GpuEcc {
            index: f[0].parse().unwrap_or(0),
            corrected: f.get(1).and_then(|v| parse_u64(v)),
            uncorrected: f.get(2).and_then(|v| parse_u64(v)),
        });
    }

    // 进程合并：pmon(gpu,pid,command) + apps(pid→mem) + ps(user,etime)
    let mut mem_by_pid: std::collections::HashMap<u32, u32> = Default::default();
    for f in csv_lines(section(raw, "APPS").unwrap_or("")) {
        if f.len() >= 3 {
            if let (Ok(pid), Some(mb)) = (
                f[0].parse::<u32>(),
                f[2].split_whitespace().next().and_then(|v| v.parse().ok()),
            ) {
                mem_by_pid.insert(pid, mb);
            }
        }
    }
    let mut meta_by_pid: std::collections::HashMap<u32, (String, String)> = Default::default();
    if let Some(ps) = section(raw, "PS") {
        for l in ps.lines().map(str::trim).filter(|l| !l.is_empty()) {
            // 列间是填充空格，必须按连续空白切（user pid etime [cmd...]，只取前 3 列）
            let mut ws = l.split_whitespace();
            let user = ws.next().unwrap_or("").to_string();
            let pid = ws.next().and_then(|p| p.parse::<u32>().ok());
            let elapsed = ws.next().unwrap_or("").to_string();
            if let Some(pid) = pid {
                meta_by_pid.insert(pid, (user, elapsed));
            }
        }
    }
    if let Some(pmon) = section(raw, "PMON") {
        for l in pmon.lines() {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = t.split_whitespace().collect();
            if f.len() < 5 {
                continue;
            }
            let (Ok(gpu), Ok(pid)) = (f[0].parse(), f[1].parse()) else {
                continue;
            };
            let (user, elapsed) = meta_by_pid
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| ("".into(), "".into()));
            let mine = user == current_user;
            r.procs.push(GpuProcRow {
                gpu,
                pid,
                user,
                name: f.last().unwrap_or(&"").to_string(),
                elapsed,
                mem_mb: mem_by_pid.get(&pid).copied(),
                mine,
            });
        }
    }

    r.topo = section(raw, "TOPO")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    r
}

// ---------- 命令 ----------

fn get_profile_user(app: &AppHandle, profile_id: &str) -> Result<String, AppError> {
    crate::profile::load_profiles(app)?
        .into_iter()
        .find(|p| p.id == profile_id)
        .map(|p| p.user)
        .ok_or_else(|| AppError::Other(format!("服务器档案不存在: {profile_id}")))
}

/// GPU 管理数据总览
#[tauri::command]
pub async fn gpu_query(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<GpuQueryResult, AppError> {
    let user = get_profile_user(&app, &profile_id)?;
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id));
    };
    let out = session.run(GPU_SCRIPT).await?;
    Ok(parse_gpu_query(&out.stdout, &user))
}

/// 结束 GPU 进程（仅限当前用户自己的进程，先 TERM 后 KILL）
#[tauri::command]
pub async fn gpu_kill(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    pid: u32,
) -> Result<String, AppError> {
    let user = get_profile_user(&app, &profile_id)?;
    let uq = format!("'{}'", user.replace('\'', "'\\''"));
    let script = format!(
        "_u=$(ps -o user= -p {pid} 2>/dev/null | tr -d ' ')\n\
         if [ -z \"$_u\" ]; then echo PROC_GONE; exit 4; fi\n\
         if [ \"$_u\" != {uq} ]; then echo \"NOT_OWNER $_u\"; exit 3; fi\n\
         kill {pid} 2>/dev/null && echo TERM_SENT || {{ echo TERM_FAIL; exit 5; }}\n\
         sleep 3\n\
         if kill -0 {pid} 2>/dev/null; then kill -9 {pid} && echo KILL_FORCED || echo KILL9_FAIL; else echo EXITED; fi"
    );
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id));
    };
    let out = session.run(&script).await?;
    let mut s = out.stdout;
    if !out.stderr.trim().is_empty() {
        s.push('\n');
        s.push_str(out.stderr.trim());
    }
    Ok(s.trim().to_string())
}

// ---------- 设置修改（persistence / 功耗上限，sudo） ----------

/// pm: value=0/1；pl: value=瓦特（50-800 sanity，实际范围由 nvidia-smi 兜底）
fn validate_set(action: &str, value: u32) -> Result<(), AppError> {
    match action {
        "pm" => {
            if value > 1 {
                return Err(AppError::Other("persistence 值只能是 0 或 1".into()));
            }
        }
        "pl" => {
            if !(50..=800).contains(&value) {
                return Err(AppError::Other("功耗上限需在 50-800W 之间".into()));
            }
        }
        other => return Err(AppError::Other(format!("未知设置项: {other}"))),
    }
    Ok(())
}

pub fn build_set_script(
    mode: SudoMode,
    password: Option<&str>,
    action: &str,
    gpu: Option<u32>,
    value: u32,
) -> Result<String, AppError> {
    validate_set(action, value)?;
    let i = gpu.map(|g| format!("-i {g} ")).unwrap_or_default();
    let set_line = match action {
        "pm" => format!("nvidia-smi {i}-pm {value}"),
        _ => format!("nvidia-smi {i}-pl {value}"),
    };
    let lines = [
        "set -e".to_string(),
        set_line,
        "sleep 1".into(),
        "nvidia-smi --query-gpu=index,persistence_mode,power.limit --format=csv,noheader".into(),
        "echo GPU_SET_DONE".into(),
    ];
    Ok(lines
        .into_iter()
        .map(|l| {
            if l.starts_with("set ") || l.starts_with("nvidia-smi --query") || l.starts_with("sleep") {
                l
            } else {
                wrap_line(&l, mode, password)
            }
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// 设置修改确认弹框预览（密码用 *** 占位）
#[tauri::command]
pub fn gpu_set_preview(
    sudo_mode: String,
    action: String,
    gpu: Option<u32>,
    value: u32,
) -> Result<String, AppError> {
    let mode = parse_mode(&sudo_mode)?;
    let password = (mode == SudoMode::SudoPass).then_some("***");
    build_set_script(mode, password, &action, gpu, value)
}

/// 执行 GPU 设置修改（后台流式任务）
#[tauri::command]
pub async fn gpu_set_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    action: String,
    gpu: Option<u32>,
    value: u32,
    password: Option<String>,
) -> Result<String, AppError> {
    validate_set(&action, value)?;
    let mode = probe_sudo_mode(&state, &profile_id).await?;
    if mode == SudoMode::SudoPass && password.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(AppError::Other("该服务器 sudo 需要密码，请先输入 sudo 密码".into()));
    }
    let script = build_set_script(mode, password.as_deref(), &action, gpu, value)?;
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    let task_id = format!("gset-{}", chrono::Utc::now().timestamp_millis());
    crate::ssh::SshSession::spawn_stream(app, profile_id, script, task_id.clone());
    Ok(task_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW_V100: &str = "\
==INFO==
0, Tesla V100-PCIE-32GB, 0322318123456, 580.65.06, 1.7.139, 3, 3, 16x
1, Tesla V100-PCIE-32GB, 0322318123457, 580.65.06, 1.7.139, 3, 3, 16x
==STAT==
0, 42 %, 32768 MiB, 30122 MiB, 71, 189.32 W, 250.00 W, 100.00 W, 250.00 W, 250.00 W, 1530, 1590
1, 0 %, 32768 MiB, 3 MiB, 39, 52.71 W, 250.00 W, 100.00 W, 250.00 W, 250.00 W, 135, 1590
==SET==
0, Enabled, Default, Enabled
1, Disabled, Default, Enabled
==THROT==
0, SW Power Cap; Idle
1, Idle
==ECC==
0, 12, 0
1, 0, 0
==PMON==
# gpu    pid  type    sm   mem    enc    dec    command
    0    5522     C     87     0      -      -   llama-server
    1    6201     C     0     0      -      -   python3
==APPS==
5522, /home/chenyb/llama-server, 28991 MiB
6201, /root/a.py, 512 MiB
==PS==
chenyb              5522     1-02:03:04           /home/chenyb/llama-server --port 8000
root                6201     12:34                 python3 a.py
==TOPO==
GPU0    GPU1    CPU Affinity    NUMA Affinity
GPU0     X      PIX      0-13        0
GPU1    PIX      X      0-13        0
==END==
";

    #[test]
    fn parse_v100_like() {
        let r = parse_gpu_query(RAW_V100, "chenyb");
        assert_eq!(r.cards.len(), 2);
        assert_eq!(r.cards[0].name, "Tesla V100-PCIE-32GB");
        assert_eq!(r.cards[0].serial.as_deref(), Some("0322318123456"));
        assert_eq!(r.cards[0].vbios.as_deref(), Some("1.7.139"));
        assert_eq!(r.cards[0].pcie_width, Some(16));

        assert_eq!(r.stats[0].util, Some(42));
        assert_eq!(r.stats[0].mem_total_mb, Some(32768));
        assert_eq!(r.stats[0].mem_used_mb, Some(30122));
        assert!((r.stats[0].power_w.unwrap() - 189.32).abs() < 1e-3);
        assert_eq!(r.stats[0].power_min_w, Some(100.0));
        assert_eq!(r.stats[0].power_max_w, Some(250.0));
        assert_eq!(r.stats[0].power_default_w, Some(250.0));
        assert_eq!(r.stats[0].sm_clock_mhz, Some(1530));

        assert!(r.settings[0].persistence);
        assert!(!r.settings[1].persistence);
        assert_eq!(r.settings[0].ecc, Some(true));

        // 降频原因：0 卡 SW Power Cap，1 卡无
        assert_eq!(r.throttles[0].reasons, vec!["SW Power Cap".to_string()]);
        assert!(r.throttles[1].reasons.is_empty());

        assert_eq!(r.ecc[0].corrected, Some(12));
        assert_eq!(r.ecc[0].uncorrected, Some(0));

        // 进程合并：pmon gpu/pid + apps mem + ps user/etime；mine 标记
        assert_eq!(r.procs.len(), 2);
        let p0 = &r.procs[0];
        assert_eq!(p0.gpu, 0);
        assert_eq!(p0.pid, 5522);
        assert_eq!(p0.user, "chenyb");
        assert!(p0.mine);
        assert_eq!(p0.mem_mb, Some(28991));
        assert_eq!(p0.elapsed, "1-02:03:04");
        let p1 = &r.procs[1];
        assert!(!p1.mine);
        assert_eq!(p1.user, "root");

        assert!(r.topo.as_deref().unwrap().contains("GPU0"));
    }

    #[test]
    fn parse_throttle_both_formats() {
        // 旧驱动命名串
        assert_eq!(
            parse_throttle_reasons("SW Power Cap; Idle"),
            vec!["SW Power Cap".to_string()]
        );
        assert!(parse_throttle_reasons("Idle").is_empty());
        assert!(parse_throttle_reasons("None").is_empty());
        // 新驱动位掩码：0x1 = GPU Idle（过滤）；0x4 = SW Power Cap；0x44 = SW+HW Thermal
        assert!(parse_throttle_reasons("0x0000000000000001").is_empty());
        assert_eq!(
            parse_throttle_reasons("0x0000000000000004"),
            vec!["SW Power Cap".to_string()]
        );
        assert_eq!(parse_throttle_reasons("0x44").len(), 2);
        assert!(parse_throttle_reasons("[N/A]").is_empty());
        assert!(parse_throttle_reasons("0xZZ").is_empty());
    }

    #[test]
    fn parse_na_tolerant() {
        let raw = "\
==INFO==
0, Some GPU, [N/A], 550.00, [N/A], [N/A], [N/A], [N/A]
==STAT==
==SET==
0, [N/A], Default, [N/A]
==THROT==
0, None
==ECC==
0, N/A, N/A
==PMON==
==APPS==
==PS==
==TOPO==
==END==
";
        let r = parse_gpu_query(raw, "u");
        assert_eq!(r.cards.len(), 1);
        assert_eq!(r.cards[0].serial, None);
        assert!(!r.settings[0].persistence);
        assert!(r.throttles[0].reasons.is_empty());
        assert_eq!(r.ecc[0].corrected, None);
        assert!(r.procs.is_empty());
    }

    #[test]
    fn set_script_builds() {
        let s = build_set_script(SudoMode::Root, None, "pm", Some(1), 1).unwrap();
        assert!(s.contains("nvidia-smi -i 1 -pm 1"));
        assert!(s.contains("echo GPU_SET_DONE"));

        let s = build_set_script(SudoMode::SudoPass, Some("pw"), "pl", None, 200).unwrap();
        assert!(s.contains("sudo -S -p '' nvidia-smi -pl 200"));
        // 查询行不包 sudo
        assert!(s.contains("\nnvidia-smi --query-gpu=index"));

        assert!(build_set_script(SudoMode::Root, None, "pm", None, 2).is_err());
        assert!(build_set_script(SudoMode::Root, None, "pl", None, 999).is_err());
        assert!(build_set_script(SudoMode::Root, None, "pl", None, 49).is_err());
        assert!(build_set_script(SudoMode::Root, None, "hack", None, 1).is_err());
    }
}
