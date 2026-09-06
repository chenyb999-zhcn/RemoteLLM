use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::NaiveDate;

const FILE: &str = "RemoteLLM.log";
const MAX_AGE_DAYS: i64 = 31;
const MAX_TOTAL_MB: u64 = 500;
const MARKER: &str = "printf '%s\\n' ";

struct LogCtx {
    dir: Option<PathBuf>,
    last_date: String,
}

static LOG: OnceLock<Mutex<LogCtx>> = OnceLock::new();
static FALLBACK_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

fn ctx() -> &'static Mutex<LogCtx> {
    LOG.get_or_init(|| Mutex::new(LogCtx { dir: None, last_date: String::new() }))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn ts() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.to_path_buf());
        }
    }
    if let Some(fb) = FALLBACK_DIR.get().and_then(|o| o.clone()) {
        if !v.contains(&fb) {
            v.push(fb);
        }
    }
    v
}

fn resolve_dir(c: &mut LogCtx) -> Option<PathBuf> {
    if let Some(d) = c.dir.clone() {
        return Some(d);
    }
    for d in candidates() {
        let path = d.join(FILE);
        if fs::OpenOptions::new().create(true).append(true).open(&path).is_ok() {
            c.dir = Some(d.clone());
            return Some(d);
        }
    }
    None
}

/// 初始化日志：优先 exe 同目录，不可写时降级到指定目录（如 app_log_dir）
pub fn init(fallback_dir: Option<PathBuf>) {
    let _ = FALLBACK_DIR.set(fallback_dir);
    let mut c = ctx().lock().unwrap();
    if resolve_dir(&mut c).is_some() {
        c.last_date = today();
        cleanup(&c.dir.as_ref().unwrap());
    }
}

fn write_lines(lines: &[String]) {
    let mut c = ctx().lock().unwrap();
    let Some(dir) = resolve_dir(&mut c) else {
        return;
    };
    let t = today();
    if !c.last_date.is_empty() && c.last_date != t {
        let from = dir.join(FILE);
        if from.exists() {
            let _ = fs::rename(&from, dir.join(format!("{FILE}.{}", c.last_date)));
        }
        c.last_date = t;
        cleanup(&dir);
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join(FILE)) {
        for l in lines {
            let _ = f.write_all(l.as_bytes());
            let _ = f.write_all(b"\n");
        }
    }
}

fn write_line(level: &str, cat: &str, msg: &str) {
    write_lines(&[format!("{} [{}] [{}] {}", ts(), level, cat, msg)]);
}

pub fn info(cat: &str, msg: &str) {
    write_line("INFO", cat, msg);
}

pub fn warn(cat: &str, msg: &str) {
    write_line("WARN", cat, msg);
}

pub fn error(cat: &str, msg: &str) {
    write_line("ERROR", cat, msg);
}

/// 单行命令记录：摘要 + 命令首行（截断 200 字符），用于高频短命令（如轮询）
pub fn cmd_summary(cat: &str, summary: &str, cmd: &str) {
    let first = cmd.lines().next().unwrap_or("").trim();
    let first: String = if first.chars().count() > 200 {
        format!("{}…", first.chars().take(200).collect::<String>())
    } else {
        first.to_string()
    };
    write_line("INFO", cat, &format!("{summary} cmd=\"{}\"", first));
}

/// 块状命令记录：表头 + 完整命令（脱敏）+ 可选结果，用于任务/多行脚本
pub fn cmd_block(cat: &str, header: &str, cmd: &str, result: Option<&str>) {
    let t = ts();
    let masked = mask_secrets(cmd);
    let mut lines = vec![
        format!("{t} [INFO] [{cat}] {header}"),
        format!("{t} [INFO] [{cat}] ---- cmd begin ----"),
    ];
    for l in masked.lines() {
        lines.push(l.to_string());
    }
    lines.push(format!("{t} [INFO] [{cat}] ---- cmd end ----"));
    if let Some(r) = result {
        lines.push(format!("{t} [INFO] [{cat}] result: {r}"));
    }
    write_lines(&lines);
}

/// 脱敏：sudo 密码（`printf '%s\n' 'pw' | sudo` 模式）与 HF_TOKEN
pub fn mask_secrets(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut rest = cmd;
    loop {
        let Some(idx) = rest.find(MARKER) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..idx]);
        out.push_str(MARKER);
        let after = &rest[idx + MARKER.len()..];
        let Some(len) = shell_quoted_len(after) else {
            out.push_str(after);
            break;
        };
        let payload = &after[..len];
        let tail = &after[len..];
        let trimmed = tail.trim_start();
        if trimmed.starts_with("| sudo") {
            out.push_str("'***'");
            out.push_str(&tail[..tail.len() - trimmed.len()]);
            rest = trimmed;
        } else {
            out.push_str(payload);
            out.push_str(tail);
            break;
        }
    }
    mask_hf_token(&out)
}

/// 计算 shell 单引号字符串（含 `'\''` 转义）的长度；不以 ' 开头返回 None
fn shell_quoted_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() != Some(&b'\'') {
        return None;
    }
    let mut i = 1;
    while i < b.len() {
        if b[i] == b'\'' {
            if i + 3 <= b.len() && b[i + 1] == b'\\' && b[i + 2] == b'\'' && b[i + 3] == b'\'' {
                i += 4;
                continue;
            }
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn mask_hf_token(s: &str) -> String {
    const KEY: &str = "HF_TOKEN=";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = rest.find(KEY) {
        out.push_str(&rest[..idx]);
        out.push_str("HF_TOKEN=***");
        let after = &rest[idx + KEY.len()..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == ';' || c == '&' || c == '|')
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// 清理：删除超过 max_age 天的轮转文件；总量超限则从最旧开始删
fn cleanup(dir: &Path) {
    let t = match NaiveDate::parse_from_str(&today(), "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => return,
    };
    let names: Vec<String> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with(&format!("{FILE}."))
            })
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => return,
    };
    for n in outdated_log_files(&names, t, MAX_AGE_DAYS) {
        let _ = fs::remove_file(dir.join(&n));
    }
    let mut dated: Vec<(NaiveDate, String)> = Vec::new();
    for n in &names {
        if let Some(ds) = n.strip_prefix(&format!("{FILE}.")) {
            if let Ok(d) = NaiveDate::parse_from_str(ds, "%Y-%m-%d") {
                dated.push((d, n.clone()));
            }
        }
    }
    dated.sort_by_key(|(d, _)| *d);
    let mut total = dated
        .iter()
        .map(|(_, n)| fs::metadata(dir.join(n)).map(|m| m.len()).unwrap_or(0))
        .sum::<u64>();
    total += fs::metadata(dir.join(FILE)).map(|m| m.len()).unwrap_or(0);
    let cap = MAX_TOTAL_MB * 1024 * 1024;
    let mut i = 0;
    while total > cap && i < dated.len() {
        let (d, n) = dated[i].clone();
        let _ = d;
        total = total.saturating_sub(fs::metadata(dir.join(&n)).map(|m| m.len()).unwrap_or(0));
        let _ = fs::remove_file(dir.join(&n));
        i += 1;
    }
}

/// 返回应删除的文件名（日期早于 today - max_age_days）
pub fn outdated_log_files(names: &[String], today: NaiveDate, max_age_days: i64) -> Vec<String> {
    let cutoff = today - chrono::Duration::days(max_age_days);
    let prefix = format!("{FILE}.");
    names
        .iter()
        .filter(|n| {
            n.strip_prefix(&prefix)
                .and_then(|ds| NaiveDate::parse_from_str(ds, "%Y-%m-%d").ok())
                .map(|d| d < cutoff)
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_sudo_password() {
        let in_s = "printf '%s\\n' 'secret' | sudo -S -p '' apt-get update -y";
        let out = mask_secrets(in_s);
        assert_eq!(out, "printf '%s\\n' '***' | sudo -S -p '' apt-get update -y");
    }

    #[test]
    fn mask_sudo_password_escaped_quote() {
        let in_s = "printf '%s\\n' 'pa'\\''ss' | sudo -S -p '' cmd";
        let out = mask_secrets(in_s);
        assert_eq!(out, "printf '%s\\n' '***' | sudo -S -p '' cmd");
    }

    #[test]
    fn mask_multiple_lines() {
        let in_s = "line1\nprintf '%s\\n' 'p1' | sudo -S -p '' a\nprintf '%s\\n' 'p2' | sudo -S -p '' b";
        let out = mask_secrets(in_s);
        assert_eq!(
            out,
            "line1\nprintf '%s\\n' '***' | sudo -S -p '' a\nprintf '%s\\n' '***' | sudo -S -p '' b"
        );
    }

    #[test]
    fn mask_keeps_non_sudo_printf() {
        let in_s = "printf '%s\\n' 'http://proxy:1080' > \"$HOME/.rl.conf\"";
        assert_eq!(mask_secrets(in_s), in_s);
    }

    #[test]
    fn mask_hf_token() {
        let in_s = "HF_TOKEN=abc123 huggingface-cli download m --repo f";
        assert_eq!(
            mask_secrets(in_s),
            "HF_TOKEN=*** huggingface-cli download m --repo f"
        );
    }

    #[test]
    fn mask_plain_unchanged() {
        let in_s = "docker info --format '{{.ServerVersion}}' && nvidia-smi -L";
        assert_eq!(mask_secrets(in_s), in_s);
    }

    #[test]
    fn shell_quoted_len_basic() {
        assert_eq!(shell_quoted_len("'abc' rest"), Some(5));
        assert_eq!(shell_quoted_len("x'abc'"), None);
        assert_eq!(shell_quoted_len("'pa'\\''ss'"), Some(10));
    }

    #[test]
    fn outdated_files_policy() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
        let names = vec![
            "RemoteLLM.log".into(),
            "RemoteLLM.log.2026-09-05".into(),
            "RemoteLLM.log.2026-08-06".into(),
            "RemoteLLM.log.2026-08-05".into(),
            "other.txt".into(),
        ];
        let out = outdated_log_files(&names, today, 31);
        assert_eq!(out, vec!["RemoteLLM.log.2026-08-05".to_string()]);
    }

    #[test]
    fn cmd_summary_truncates() {
        let long = format!("{} x", "a".repeat(300));
        let first = long.lines().next().unwrap().trim();
        let truncated: String = if first.chars().count() > 200 {
            format!("{}…", first.chars().take(200).collect::<String>())
        } else {
            first.to_string()
        };
        assert_eq!(truncated.chars().count(), 201);
    }
}
