use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::AppError;
use crate::profile::ServerProfile;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub source: String,
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    pub name: String,
    pub size: Option<String>,
}

fn get_profile(app: &AppHandle, profile_id: &str) -> Result<ServerProfile, AppError> {
    crate::profile::load_profiles(app)?
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| AppError::Other(format!("服务器档案不存在: {profile_id}")))
}

async fn run_on(
    state: &State<'_, crate::AppState>,
    profile_id: &str,
    cmd: &str,
) -> Result<crate::ssh::CmdOutput, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(profile_id) else {
        return Err(AppError::NotConnected(profile_id.to_string()));
    };
    Ok(session.run(cmd).await?)
}

#[tauri::command]
pub async fn search_models(source: String, query: String, limit: u32) -> Result<Vec<ModelInfo>, AppError> {
    let client = reqwest::Client::new();
    let limit = limit.clamp(1, 50);
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    match source.as_str() {
        "modelscope" => {
            let url = format!(
                "https://modelscope.cn/api/v1/models?Query={}&Limit={}",
                urlencoding::encode(q),
                limit
            );
            let v: serde_json::Value = client.get(&url).send().await?.json().await?;
            let models = v
                .get("Data")
                .and_then(|d| d.get("Models"))
                .and_then(|m| m.as_array())
                .cloned()
                .unwrap_or_default();
            Ok(models
                .into_iter()
                .map(|m| ModelInfo {
                    id: m
                        .get("Path")
                        .or_else(|| m.get("Name"))
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    source: "modelscope".into(),
                    downloads: m.get("Downloads").and_then(|x| x.as_u64()),
                    likes: m.get("Likes").and_then(|x| x.as_u64()),
                    description: m
                        .get("Description")
                        .and_then(|x| x.as_str())
                        .map(|s| s.chars().take(200).collect()),
                })
                .filter(|m| !m.id.is_empty())
                .collect())
        }
        "huggingface" => {
            let url = format!(
                "https://huggingface.co/api/models?search={}&limit={}",
                urlencoding::encode(q),
                limit
            );
            let v: serde_json::Value = client
                .get(&url)
                .header("User-Agent", "RemoteLLM/0.1")
                .send()
                .await?
                .json()
                .await?;
            let arr = v.as_array().cloned().unwrap_or_default();
            Ok(arr
                .into_iter()
                .map(|m| ModelInfo {
                    id: m.get("modelId").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                    source: "huggingface".into(),
                    downloads: m.get("downloads").and_then(|x| x.as_u64()),
                    likes: m.get("likes").and_then(|x| x.as_u64()),
                    description: None,
                })
                .filter(|m| !m.id.is_empty())
                .collect())
        }
        other => Err(AppError::Other(format!("未知来源: {other}"))),
    }
}

const LIST_MODELS_SCRIPT: &str = r#"
cd "$1" 2>/dev/null || exit 0
for d in */; do
  [ -d "$d" ] || continue
  size=$(du -sh "$d" 2>/dev/null | cut -f1)
  name=$(basename "$d")
  echo "${name}|${size}"
done
"#;

#[tauri::command]
pub async fn list_local_models(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<Vec<LocalModel>, AppError> {
    let profile = get_profile(&app, &profile_id)?;
    let cmd = format!("bash -c '{}' \"{}\"", LIST_MODELS_SCRIPT, profile.models_dir());
    let out = run_on(&state, &profile_id, &cmd).await?;
    Ok(out
        .stdout
        .lines()
        .filter(|l| l.contains('|'))
        .map(|l| {
            let (name, size) = l.split_once('|').unwrap_or((l, ""));
            LocalModel {
                name: name.trim().to_string(),
                size: if size.trim().is_empty() {
                    None
                } else {
                    Some(size.trim().to_string())
                },
            }
        })
        .filter(|m| !m.name.is_empty())
        .collect())
}

pub fn download_command(source: &str, model_id: &str, dest: &str, token: Option<&str>) -> Result<String, AppError> {
    let dest_q = format!("'{}'", dest.replace('\'', "'\\''"));
    match source {
        "modelscope" => Ok(format!(
            "modelscope download --model '{}' --local_dir {} 2>&1",
            model_id.replace('\'', "'\\''"),
            dest_q
        )),
        "huggingface" => {
            let mut c = format!(
                "huggingface-cli download '{}' --local-dir {}",
                model_id.replace('\'', "'\\''"),
                dest_q
            );
            if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
                c = format!("HF_TOKEN={} {}", t, c);
            }
            Ok(c)
        }
        other => Err(AppError::Other(format!("未知来源: {other}"))),
    }
}

#[tauri::command]
pub async fn model_download_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    source: String,
    model_id: String,
    dest: Option<String>,
    token: Option<String>,
) -> Result<String, AppError> {
    let profile = get_profile(&app, &profile_id)?;
    // 检查 CLI 是否存在
    let check = if source == "modelscope" {
        "command -v modelscope >/dev/null 2>&1 && echo OK || echo MISSING"
    } else {
        "command -v huggingface-cli >/dev/null 2>&1 && echo OK || echo MISSING"
    };
    let out = run_on(&state, &profile_id, check).await?;
    if out.stdout.contains("MISSING") {
        let need = if source == "modelscope" { "modelscope" } else { "huggingface_hub[cli]" };
        return Err(AppError::Other(format!(
            "服务器缺少下载工具，请先安装: pip install {need}"
        )));
    }

    let dest = dest
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| format!("{}/{}", profile.models_dir(), model_id.split('/').last().unwrap_or(model_id.as_str())));

    let task_id = format!("dl-{}", chrono::Utc::now().timestamp_millis());
    let cmd = download_command(&source, &model_id, &dest, token.as_deref())?;
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id.clone()));
    };
    session.run_stream(&cmd, &task_id, &app).await?;
    Ok(task_id)
}

#[tauri::command]
pub async fn model_delete(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    name: String,
) -> Result<String, AppError> {
    if name.contains('/') || name.starts_with('.') || name == ".." {
        return Err(AppError::Other("非法模型名".into()));
    }
    let profile = get_profile(&app, &profile_id)?;
    let dir = format!("{}/{}", profile.models_dir(), name);
    let cmd = format!(
        "if [ -d '{}' ]; then rm -rf '{}' && echo DELETED; else echo NOT_FOUND; fi",
        dir.replace('\'', "'\\''"),
        dir.replace('\'', "'\\''")
    );
    let out = run_on(&state, &profile_id, &cmd).await?;
    Ok(out.stdout.trim().to_string())
}
