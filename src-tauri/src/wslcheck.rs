use serde::Serialize;
use std::time::Duration;

/// 本机 WSL2 一键检测（纯 Windows 本地操作，不走 SSH）
///
/// 采集：`wsl -l -v`（发行版列表）、`wsl --status`（默认版本/发行版，旧版回退
/// `wsl --version`）、Windows 可选特性（WSL + VirtualMachinePlatform）、
/// 每个运行中 v2 发行版的 IP（`wsl -d <name> -- hostname -I`）。

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslDistro {
    pub name: String,
    pub state: String,
    /// 1 | 2，0 = 未知
    pub version: u8,
    pub is_default: bool,
    /// 运行中的 v2 发行版才有 IP
    pub ip: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslFeatures {
    /// Microsoft-Windows-Subsystem-Linux
    pub wsl: bool,
    /// VirtualMachinePlatform（WSL2 必需）
    pub vm_platform: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslCheckResult {
    pub wsl_installed: bool,
    /// `wsl --version` 首行（如 1.0.6.0）
    pub wsl_version: Option<String>,
    /// `wsl --status` 的 Default Version
    pub default_version: Option<u8>,
    pub default_distro: Option<String>,
    pub distros: Vec<WslDistro>,
    pub features: Option<WslFeatures>,
    /// ready | not_installed | no_distro | wsl1_only
    pub verdict: String,
    /// 修复命令（前端原样展示为可复制代码块）
    pub hints: Vec<String>,
}

// ---------- 纯函数解析（可单测） ----------

/// 解析 `wsl --status`：返回 (默认发行版, 默认版本)。
/// 兼容英文/中文标签与半角/全角冒号。
pub fn parse_wsl_status(out: &str) -> (Option<String>, Option<u8>) {
    let mut distro: Option<String> = None;
    let mut version: Option<u8> = None;
    for line in out.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once(':').or_else(|| line.split_once('：')) else {
            continue;
        };
        let k = k.trim().to_lowercase();
        let v = v.trim();
        if k.contains("version") || k.contains("版本") {
            version = v.parse().ok();
        } else if k.contains("distribution") || k.contains("分发") {
            distro = if v.is_empty() { None } else { Some(v.to_string()) };
        }
    }
    (distro, version)
}

/// 解析 `wsl -l -v`：返回 (名称, 状态, 版本, 是否默认) 列表。
/// 末列为版本数字、次末列为状态、其余为名称（允许含空格）。
pub fn parse_wsl_list(out: &str) -> Vec<(String, String, u8, bool)> {
    let mut rows: Vec<(String, String, u8, bool)> = Vec::new();
    for raw in out.lines() {
        let t = raw.trim_start();
        if t.is_empty() {
            continue;
        }
        // 跳过表头
        if t.to_uppercase().starts_with("NAME") {
            continue;
        }
        let is_default = t.starts_with('*');
        let t = t.trim_start_matches('*').trim();
        // 末列 = 版本数字，次末列 = 状态，其余 = 名称（允许含空格）
        let toks: Vec<&str> = t.split_whitespace().collect();
        if toks.len() < 3 {
            continue;
        }
        let ver = toks[toks.len() - 1].parse::<u8>().ok().unwrap_or(0);
        let state = toks[toks.len() - 2].to_string();
        let name = toks[..toks.len() - 2].join(" ");
        if name.is_empty() {
            continue;
        }
        rows.push((name, state, ver, is_default));
    }
    rows
}

/// 解析 `wsl --version` 首行版本（如 "Windows Subsystem for Linux Version: 1.0.6.0"）
pub fn parse_wsl_version(out: &str) -> Option<String> {
    for line in out.lines() {
        let low = line.to_lowercase();
        if low.contains("version") && (low.contains("subsystem") || line.contains('：')) {
            let v = line
                .rsplit_once(':')
                .or_else(|| line.rsplit_once('：'))
                .map(|(_, v)| v.trim().to_string());
            if let Some(v) = v {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// 解析特性探测输出（两行 True/False）
pub fn parse_features(out: &str) -> Option<(bool, bool)> {
    let lines: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.len() < 2 {
        return None;
    }
    Some((
        lines[0].eq_ignore_ascii_case("true"),
        lines[1].eq_ignore_ascii_case("true"),
    ))
}

/// 从 `hostname -I` 输出中取第一个 IPv4
pub fn parse_ip(out: &str) -> Option<String> {
    out.split_whitespace()
        .find(|t| is_ipv4(t))
        .map(|s| s.to_string())
}

fn is_ipv4(t: &str) -> bool {
    let parts: Vec<&str> = t.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 3
                && p.chars().all(|c| c.is_ascii_digit())
                && p.parse::<u16>().map(|n| n <= 255).unwrap_or(false)
        })
}

/// 判定结论 + 修复命令
pub fn decide_verdict(
    installed: bool,
    distros: &[(String, String, u8, bool)],
    default_version: Option<u8>,
) -> (String, Vec<String>) {
    if !installed {
        return (
            "not_installed".into(),
            vec!["wsl --install".into(), "wsl --update".into()],
        );
    }
    if distros.is_empty() {
        return ("no_distro".into(), vec!["wsl --install -d Ubuntu".into()]);
    }
    let has_v2 = distros.iter().any(|d| d.2 == 2) || default_version == Some(2);
    if !has_v2 {
        return (
            "wsl1_only".into(),
            vec!["wsl --update".into(), "wsl --set-default-version 2".into()],
        );
    }
    ("ready".into(), vec![])
}

// ---------- 命令执行 ----------

async fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::process::Command::new(cmd)
            .args(args)
            .output(),
    )
    .await
    .map_err(|_| "timeout".to_string())?
    .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_default();
        Err(if stderr.is_empty() {
            format!("exit {code}")
        } else {
            format!("exit {code}: {stderr}")
        })
    }
}

#[tauri::command]
pub async fn wsl_check() -> Result<WslCheckResult, String> {
    // 并行：列表 / 状态 / 版本 / Windows 特性
    let (list_res, status_res, version_res, features_res) = tokio::join!(
        run("wsl.exe", &["-l", "-v"]),
        run("wsl.exe", &["--status"]),
        run("wsl.exe", &["--version"]),
        run(
            "powershell.exe",
            &[
                "-NoProfile",
                "-Command",
                "(Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Windows-Subsystem-Linux).State -eq 'Enabled';(Get-WindowsOptionalFeature -Online -FeatureName VirtualMachinePlatform).State -eq 'Enabled'",
            ],
        ),
    );

    let installed = list_res.is_ok();
    let distros_raw = match &list_res {
        Ok(out) => parse_wsl_list(out),
        Err(_) => Vec::new(),
    };

    // --status 失败（旧版 WSL 不支持）→ 尝试从 --version 输出里找默认版本线索
    let (default_distro, default_version) = match &status_res {
        Ok(out) => parse_wsl_status(out),
        Err(_) => (None, None),
    };
    let wsl_version = match &version_res {
        Ok(out) => parse_wsl_version(out),
        Err(_) => None,
    };

    let features = features_res
        .ok()
        .and_then(|o| parse_features(&o))
        .map(|(wsl, vm)| WslFeatures {
            wsl,
            vm_platform: vm,
        });

    // 运行中的 v2 发行版 → 取 IP（逐个，通常 1-2 个）
    let mut distros: Vec<WslDistro> = Vec::new();
    for (name, state, version, is_default) in &distros_raw {
        let ip = if *version == 2 && state.eq_ignore_ascii_case("running") {
            match run("wsl.exe", &["-d", name, "--", "hostname", "-I"]).await {
                Ok(out) => parse_ip(&out),
                Err(_) => None,
            }
        } else {
            None
        };
        distros.push(WslDistro {
            name: name.clone(),
            state: state.clone(),
            version: *version,
            is_default: *is_default,
            ip,
        });
    }

    let (verdict, hints) = decide_verdict(installed, &distros_raw, default_version);

    Ok(WslCheckResult {
        wsl_installed: installed,
        wsl_version,
        default_version,
        default_distro,
        distros,
        features,
        verdict,
        hints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_english() {
        let out = "Default Distribution: Ubuntu-22.04\nDefault Version: 2\n";
        assert_eq!(
            parse_wsl_status(out),
            (Some("Ubuntu-22.04".into()), Some(2))
        );
    }

    #[test]
    fn status_chinese_fullwidth() {
        let out = "默认分发: Ubuntu\n默认版本：2\n";
        assert_eq!(parse_wsl_status(out), (Some("Ubuntu".into()), Some(2)));
    }

    #[test]
    fn status_empty_lines() {
        assert_eq!(parse_wsl_status(""), (None, None));
        assert_eq!(parse_wsl_status("no data\n"), (None, None));
    }

    #[test]
    fn list_english_multi() {
        let out = "  NAME            STATE           VERSION\n* Ubuntu-22.04    Running         2\n  Win10-WSL       Stopped         1\n  My Distro       Running         2\n";
        let rows = parse_wsl_list(out);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], ("Ubuntu-22.04".into(), "Running".into(), 2, true));
        assert_eq!(rows[1], ("Win10-WSL".into(), "Stopped".into(), 1, false));
        // 名称含空格
        assert_eq!(rows[2], ("My Distro".into(), "Running".into(), 2, false));
    }

    #[test]
    fn list_chinese_states() {
        // 个别系统下状态可能被本地化；解析保持原样透传
        let out = "  NAME            STATE           VERSION\n* Ubuntu        运行中          2\n";
        let rows = parse_wsl_list(out);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "Ubuntu");
        assert_eq!(rows[0].2, 2);
        assert!(rows[0].3);
    }

    #[test]
    fn list_empty() {
        assert!(parse_wsl_list("").is_empty());
        // 仅有表头（未装任何发行版）
        assert!(parse_wsl_list("  NAME            STATE           VERSION\n").is_empty());
    }

    #[test]
    fn version_first_line() {
        let out = "Windows Subsystem for Linux Version: 1.0.6.0\nKernel Version: 5.15.153.1-1\n";
        assert_eq!(parse_wsl_version(out), Some("1.0.6.0".into()));
    }

    #[test]
    fn version_missing() {
        assert_eq!(parse_wsl_version(""), None);
        assert_eq!(parse_wsl_version("no version here\n"), None);
    }

    #[test]
    fn features_parse() {
        assert_eq!(parse_features("True\nTrue\n"), Some((true, true)));
        assert_eq!(parse_features("True\nFalse\n"), Some((true, false)));
        assert_eq!(parse_features("False\nFalse\n"), Some((false, false)));
        assert_eq!(parse_features("True\n"), None);
        assert_eq!(parse_features(""), None);
    }

    #[test]
    fn ip_parse() {
        assert_eq!(
            parse_ip(" 172.28.96.3 172.29.128.3 \n"),
            Some("172.28.96.3".into())
        );
        assert_eq!(parse_ip(""), None);
        assert_eq!(parse_ip("no ip here\n"), None);
        assert_eq!(parse_ip("999.1.1.1\n"), None);
        assert_eq!(parse_ip("1.2.3\n"), None);
    }

    #[test]
    fn verdict_matrix() {
        let none: Vec<(String, String, u8, bool)> = vec![];
        assert_eq!(
            decide_verdict(false, &none, None).0,
            "not_installed"
        );
        assert_eq!(decide_verdict(true, &none, None).0, "no_distro");
        let only_v1 = vec![("U".into(), "Stopped".into(), 1, true)];
        assert_eq!(decide_verdict(true, &only_v1, None).0, "wsl1_only");
        // 默认版本 2 也算就绪
        assert_eq!(decide_verdict(true, &only_v1, Some(2)).0, "ready");
        let v2 = vec![("U".into(), "Running".into(), 2, true)];
        let (v, hints) = decide_verdict(true, &v2, Some(2));
        assert_eq!(v, "ready");
        assert!(hints.is_empty());
        // not_installed 有修复命令
        assert!(!decide_verdict(false, &none, None).1.is_empty());
    }

    #[test]
    fn result_serde_camel() {
        let r = WslCheckResult {
            wsl_installed: true,
            wsl_version: Some("1.0.6.0".into()),
            default_version: Some(2),
            default_distro: Some("Ubuntu".into()),
            distros: vec![WslDistro {
                name: "Ubuntu".into(),
                state: "Running".into(),
                version: 2,
                is_default: true,
                ip: Some("172.28.96.3".into()),
            }],
            features: Some(WslFeatures {
                wsl: true,
                vm_platform: true,
            }),
            verdict: "ready".into(),
            hints: vec![],
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"wslInstalled\":true"));
        assert!(s.contains("\"defaultVersion\":2"));
        assert!(s.contains("\"isDefault\":true"));
        assert!(s.contains("\"vmPlatform\":true"));
    }
}
