use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::error::AppError;
use crate::profile::ServerProfile;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub source: String,
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    /// 服务器上的绝对路径
    pub path: String,
    /// 相对模型目录的路径（删除/复制用）
    pub rel: String,
    pub name: String,
    /// "gguf" | "gguf-split" | "safetensors" | "hf" | "dir"
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub quant: Option<String>,
    #[serde(default)]
    pub params: Option<String>,
    #[serde(default)]
    pub ctx: Option<u64>,
    #[serde(default)]
    pub note: Option<String>,
}

fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 分片文件名前缀：model-00001-of-00003.gguf -> model
fn split_prefix(fname: &str) -> Option<String> {
    let pos = fname.rfind("-of-")?;
    let head = &fname[..pos];
    let tail = &fname[pos + 4..];
    if !tail.to_ascii_lowercase().ends_with(".gguf") {
        return None;
    }
    let mid = &tail[..tail.len() - 5];
    if mid.is_empty() || !mid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let nd = head.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if nd == 0 {
        return None;
    }
    let cut = head.len() - nd;
    // 分片约定 prefix-NNNNN-of-：数字前的单个分隔短横线属于格式，不属于前缀
    let prefix = if cut > 0 && head.as_bytes()[cut - 1] == b'-' {
        &head[..cut - 1]
    } else {
        &head[..cut]
    };
    if !is_safe_name(prefix) {
        return None;
    }
    Some(prefix.to_string())
}

fn is_safe_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoFile {
    pub path: String,
    pub size: u64,
}

/// HF API 基地址：优先用设置里的镜像端点，否则官方
fn hf_base(app: &AppHandle) -> Result<String, AppError> {
    let settings = crate::settings::load_settings(app)?;
    let base = settings.hf_endpoint.trim().trim_end_matches('/');
    Ok(if base.is_empty() {
        "https://huggingface.co".into()
    } else {
        base.to_string()
    })
}

/// HuggingFace 服务端关键词搜索（真实搜索，支持镜像端点）
async fn hf_search(
    client: &reqwest::Client,
    base: &str,
    q: &str,
    limit: u32,
) -> Result<Vec<ModelInfo>, AppError> {
    let url = format!(
        "{}/api/models?search={}&limit={}",
        base,
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
            id: m
                .get("modelId")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            source: "huggingface".into(),
            downloads: m.get("downloads").and_then(|x| x.as_u64()),
            likes: m.get("likes").and_then(|x| x.as_u64()),
            description: None,
        })
        .filter(|m| !m.id.is_empty())
        .collect())
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
pub async fn search_models(
    app: AppHandle,
    source: String,
    query: String,
    limit: u32,
) -> Result<Vec<ModelInfo>, AppError> {
    let client = reqwest::Client::new();
    let limit = limit.clamp(1, 50);
    let q = query.trim();
    match source.as_str() {
        "modelscope" => {
            if q.contains('/') {
                // 精确模型 ID（org/name）：查详情
                let url = format!("https://www.modelscope.cn/api/v1/models/{q}");
                let v: serde_json::Value = client.get(&url).send().await?.json().await?;
                match v.get("Data").and_then(ms_model_info) {
                    Some(info) => Ok(vec![info]),
                    None => Err(AppError::Other(format!("ModelScope 未找到模型: {q}"))),
                }
            } else if q.is_empty() {
                let all = ms_hotlist(&app, &client).await?;
                Ok(all.into_iter().take(limit as usize).collect())
            } else {
                // 关键词：在 Top 10000 全量列表缓存中本地匹配（每天后台刷新）；
                // 缓存未就绪时回退热门列表过滤
                let kw = q.to_lowercase();
                let mut used_full = false;
                let mut hit: Vec<ModelInfo> = Vec::new();
                if let Some(cache) = read_ms_full_cache(&app) {
                    if !cache.models.is_empty() {
                        used_full = true;
                        if cache.date != today_str() {
                            let h = app.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = refresh_ms_fulllist_if_stale(&h).await;
                            });
                        }
                        hit = cache
                            .models
                            .iter()
                            .filter(|m| ms_match(m, &kw))
                            .cloned()
                            .take(limit as usize)
                            .collect();
                    }
                }
                if !used_full {
                    let all = ms_hotlist(&app, &client).await?;
                    hit = all
                        .iter()
                        .filter(|m| ms_match(m, &kw))
                        .cloned()
                        .take(limit as usize)
                        .collect();
                }
                if hit.is_empty() {
                    Err(AppError::Other(format!(
                        "未找到包含 \"{q}\" 的模型。若全量列表（Top 10000）尚在抓取中请稍后再试，或输入完整模型 ID（如 Qwen/Qwen3-8B）"
                    )))
                } else {
                    Ok(hit)
                }
            }
        }
        "huggingface" => {
            let base = hf_base(&app)?;
            hf_search(&client, &base, q, limit).await
        }
        other => Err(AppError::Other(format!("未知来源: {other}"))),
    }
}

fn ms_model_info(m: &serde_json::Value) -> Option<ModelInfo> {
    let path = m.get("Path")?.as_str()?;
    let name = m.get("Name")?.as_str()?;
    if path.is_empty() || name.is_empty() {
        return None;
    }
    let description = m
        .get("ChineseName")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| m.get("Description").and_then(|x| x.as_str()))
        .map(|s| s.chars().take(200).collect::<String>());
    Some(ModelInfo {
        id: format!("{path}/{name}"),
        source: "modelscope".into(),
        downloads: m.get("Downloads").and_then(|x| x.as_u64()),
        likes: m.get("Stars").and_then(|x| x.as_u64()),
        description,
    })
}

/// 关键词匹配：模型 ID 或中文名（kw 需已转小写）
fn ms_match(m: &ModelInfo, kw: &str) -> bool {
    m.id.to_lowercase().contains(kw)
        || m.description
            .as_deref()
            .map(|d| d.to_lowercase().contains(kw))
            .unwrap_or(false)
}

const MS_HOT_PAGE_SIZE: u32 = 50;

async fn ms_fetch_hot(client: &reqwest::Client) -> Result<Vec<ModelInfo>, AppError> {
    let v: serde_json::Value = client
        .put("https://www.modelscope.cn/api/v1/dolphin/modelsWithCollections")
        .json(&serde_json::json!({ "PageSize": MS_HOT_PAGE_SIZE, "PageNumber": 1 }))
        .send()
        .await?
        .json()
        .await?;
    let arr = v
        .get("Data")
        .and_then(|d| d.get("ModelCollection"))
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(arr
        .iter()
        .filter_map(|c| c.get("Model"))
        .filter_map(ms_model_info)
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MsHotCache {
    date: String,
    models: Vec<ModelInfo>,
}

fn ms_cache_path(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    Ok(app.path().app_config_dir()?.join("modelscope_hotlist.json"))
}

fn read_ms_cache(app: &AppHandle) -> Option<MsHotCache> {
    let path = ms_cache_path(app).ok()?;
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

fn today_str() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 读取热门列表：当天缓存直接返回；跨天则重新抓取并写缓存；网络失败时回退旧缓存
async fn ms_hotlist(app: &AppHandle, client: &reqwest::Client) -> Result<Vec<ModelInfo>, AppError> {
    let today = today_str();
    if let Some(cache) = read_ms_cache(app) {
        if cache.date == today && !cache.models.is_empty() {
            return Ok(cache.models);
        }
    }
    match ms_fetch_hot(client).await {
        Ok(models) => {
            if let Ok(s) = serde_json::to_string_pretty(&MsHotCache { date: today, models: models.clone() }) {
                if let Ok(path) = ms_cache_path(app) {
                    let _ = std::fs::write(path, s);
                }
            }
            Ok(models)
        }
        Err(e) => {
            if let Some(cache) = read_ms_cache(app) {
                if !cache.models.is_empty() {
                    return Ok(cache.models);
                }
            }
            Err(e)
        }
    }
}

/// 应用启动时调用：缓存日期不是今天（或文件不存在）就刷新
pub async fn refresh_ms_hotlist_if_stale(app: &AppHandle) -> Result<(), AppError> {
    let today = today_str();
    if let Some(cache) = read_ms_cache(app) {
        if cache.date == today && !cache.models.is_empty() {
            return Ok(());
        }
    }
    let client = reqwest::Client::new();
    let models = ms_fetch_hot(&client).await?;
    let s = serde_json::to_string_pretty(&MsHotCache { date: today, models })?;
    let path = ms_cache_path(app)?;
    std::fs::write(path, s)?;
    Ok(())
}

const MS_FULL_PAGES: u32 = 100;
const MS_FULL_PAGE_SIZE: u32 = 200;

async fn ms_fetch_page(client: &reqwest::Client, page: u32) -> Vec<ModelInfo> {
    let resp = client
        .put("https://www.modelscope.cn/api/v1/dolphin/modelsWithCollections")
        .json(&serde_json::json!({
            "PageSize": MS_FULL_PAGE_SIZE,
            "PageNumber": page,
        }))
        .send()
        .await
        .ok();
    let v = match resp {
        Some(r) => r.json::<serde_json::Value>().await.ok(),
        None => None,
    };
    let Some(v) = v else {
        return Vec::new();
    };
    let arr = v
        .get("Data")
        .and_then(|d| d.get("ModelCollection"))
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    arr.iter()
        .filter_map(|c| c.get("Model"))
        .filter_map(ms_model_info)
        .collect()
}

/// 抓取 Top 10000 模型（100 页 × 200，5 页并发）；某页失败/为空时返回已抓取部分
async fn ms_fetch_full(client: &reqwest::Client) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = Vec::with_capacity(10_000);
    let mut page = 1u32;
    while page <= MS_FULL_PAGES {
        let end = (page + 4).min(MS_FULL_PAGES);
        let batch: Vec<u32> = (page..=end).collect();
        let futs: Vec<_> = batch.iter().map(|&p| ms_fetch_page(client, p)).collect();
        for r in futures_util::future::join_all(futs).await {
            if r.is_empty() {
                return out;
            }
            out.extend(r);
        }
        page = end + 1;
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MsFullCache {
    date: String,
    models: Vec<ModelInfo>,
}

fn ms_full_cache_path(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    Ok(app.path().app_config_dir()?.join("modelscope_models.json"))
}

fn read_ms_full_cache(app: &AppHandle) -> Option<MsFullCache> {
    let path = ms_full_cache_path(app).ok()?;
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

/// 缓存日期不是今天（或文件不存在）就重新抓取 Top 10000 全量列表
pub async fn refresh_ms_fulllist_if_stale(app: &AppHandle) -> Result<usize, AppError> {
    let today = today_str();
    if let Some(c) = read_ms_full_cache(app) {
        if c.date == today && !c.models.is_empty() {
            return Ok(c.models.len());
        }
    }
    let client = reqwest::Client::new();
    let models = ms_fetch_full(&client).await;
    if models.len() < 2000 {
        return Err(AppError::Other(format!(
            "ModelScope 全量列表抓取失败（仅 {} 条）",
            models.len()
        )));
    }
    let s = serde_json::to_string(&MsFullCache {
        date: today,
        models: models.clone(),
    })?;
    let path = ms_full_cache_path(app)?;
    std::fs::write(path, s)?;
    Ok(models.len())
}

/// 列出模型仓库内的文件（含大小），供下载前选择精度/分片
#[tauri::command]
pub async fn list_repo_files(
    app: AppHandle,
    source: String,
    model_id: String,
    token: Option<String>,
) -> Result<Vec<RepoFile>, AppError> {
    let client = reqwest::Client::new();
    let id = model_id.trim();
    if id.is_empty() {
        return Err(AppError::Other("模型 ID 不能为空".into()));
    }
    let path_id: String = id
        .split('/')
        .map(urlencoding::encode)
        .collect::<Vec<_>>()
        .join("/");
    let files: Vec<RepoFile> = match source.as_str() {
        "huggingface" => {
            let settings = crate::settings::load_settings(&app)?;
            let base = settings.hf_endpoint.trim().trim_end_matches('/');
            let base = if base.is_empty() { "https://huggingface.co" } else { base };
            let url = format!("{base}/api/models/{path_id}?blobs=true");
            let mut req = client.get(&url).header("User-Agent", "RemoteLLM/0.1");
            let tok = token
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    let t = settings.hf_token.trim();
                    (!t.is_empty()).then(|| t.to_string())
                });
            if let Some(t) = tok {
                req = req.header("Authorization", format!("Bearer {t}"));
            }
            let v: serde_json::Value = req.send().await?.json().await?;
            v.get("siblings")
                .and_then(|s| s.as_array())
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|s| {
                    let p = s.get("rfilename")?.as_str()?.to_string();
                    let size = s.get("size").and_then(|x| x.as_u64()).unwrap_or(0);
                    Some(RepoFile { path: p, size })
                })
                .collect()
        }
        "modelscope" => {
            let url = format!(
                "https://www.modelscope.cn/api/v1/models/{path_id}/repo/files?Revision=master&Recursive=true"
            );
            let v: serde_json::Value = client.get(&url).send().await?.json().await?;
            v.get("Data")
                .and_then(|d| d.get("Files"))
                .and_then(|f| f.as_array())
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|f| {
                    let ty = f.get("Type").and_then(|x| x.as_str()).unwrap_or("blob");
                    if ty != "blob" {
                        return None;
                    }
                    let p = f.get("Path")?.as_str()?.to_string();
                    let size = f.get("Size").and_then(|x| x.as_u64()).unwrap_or(0);
                    Some(RepoFile { path: p, size })
                })
                .collect()
        }
        other => return Err(AppError::Other(format!("未知来源: {other}"))),
    };
    let mut files = files;
    files.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(files)
}

/// 递归扫描本地模型（纯标准库 Python，输出 JSONL）：
/// 散放 .gguf / 分片组(-00001-of-) / .safetensors / config.json 目录(HF) / 顶层普通目录
const LIST_MODELS_PY: &str = r#"
import json, math, os, re, struct, sys

try:
    from gguf.gguf_reader import GGUFReader
except Exception:
    GGUFReader = None
try:
    from safetensors import safe_open
except Exception:
    safe_open = None

root = sys.argv[1]
mode = sys.argv[2] if len(sys.argv) > 2 else "full"
MAXD = 4
SPLIT_RE = re.compile(r"^(.*)-(\d+)-of-(\d+)\.gguf$", re.I)

if not os.path.isdir(root):
    sys.stderr.write("MODELS_DIR_MISSING " + root)
    sys.exit(2)

def emit(o):
    print(json.dumps(o, ensure_ascii=False))

def safe_size(p):
    try:
        return os.path.getsize(p)
    except OSError:
        return None

def dir_size(d):
    t = 0
    for dp, _dn, fn in os.walk(d):
        for x in fn:
            s = safe_size(os.path.join(dp, x))
            if s:
                t += s
    return t

def fmt_params(n):
    if not n or n <= 0:
        return None
    x = float(n)
    for div, u in ((1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")):
        if x >= div:
            s = ("%.2f" % (x / div)).rstrip("0").rstrip(".")
            return s + u
    return str(int(n))

def quant_of(name):
    stem = re.sub(r"\.(gguf|safetensors)$", "", name, flags=re.I)
    cands = re.findall(r"IQ\d{1,2}(?:_[A-Z0-9]+)*|Q\d{1,2}(?:_[A-Z0-9]+)*|BF16|FP16|F16|F32", stem, flags=re.I)
    return cands[-1].upper() if cands else None

def beat():
    # 每解析完一个文件就向 stderr 写一个点：保持 SSH 通道有数据，避免 120s 无输出被断开
    sys.stderr.write(".")
    sys.stderr.flush()

def read_gguf(path):
    # 用 ggml-org 官方 gguf 包解析头部（np.memmap 懒加载，不读权重）；未装则返回 None
    if GGUFReader is None:
        return None
    try:
        r = GGUFReader(path)
        def gv(k):
            f = r.get_field(k)
            return f.contents() if f is not None else None
        arch = gv("general.architecture")
        label = gv("general.size_label")
        ctx = gv(arch + ".context_length") if arch else None
        if not isinstance(ctx, int) or isinstance(ctx, bool):
            ctx = None
        params = 0
        for t in r.tensors:
            params += int(t.n_elements)
        return {
            "arch": arch,
            "label": label,
            "params": fmt_params(params),
            "ctx": ctx,
        }
    except Exception:
        return None

def read_st(path):
    # 优先用 HuggingFace 官方 safetensors 包；未装/异常时回退到标准库读 JSON 头
    if safe_open is not None:
        try:
            with safe_open(path, framework="np") as f:
                params = 0
                dtype = None
                for k in f.keys():
                    sl = f.get_slice(k)
                    shp = sl.get_shape()
                    if shp:
                        params += int(math.prod(shp))
                    if dtype is None:
                        dtype = sl.get_dtype()
                return {"params": fmt_params(params), "dtype": dtype}
        except Exception:
            pass
    try:
        f = open(path, "rb")
        n = struct.unpack("<Q", f.read(8))[0]
        if n <= 0 or n > 100000000:
            f.close()
            return None
        h = json.loads(f.read(n))
        f.close()
    except Exception:
        return None
    params = 0
    dtype = None
    for k, v in h.items():
        if k == "__metadata__" or not isinstance(v, dict):
            continue
        p = 1
        for d in v.get("shape") or []:
            try:
                p *= int(d)
            except Exception:
                p = 0
                break
        params += p
        if dtype is None and v.get("dtype"):
            dtype = v.get("dtype")
    return {"params": fmt_params(params), "dtype": dtype}

def parse_one(p):
    beat()
    try:
        if p.lower().endswith(".gguf"):
            return read_gguf(p)
        return read_st(p)
    except Exception:
        return None

# 两阶段：先收集所有待解析文件，再并行解析（头部解析是 CPU 密集，单文件可达数十秒）
_parse_paths = []
for dp, dn, fn in os.walk(root, topdown=True):
    rel = os.path.relpath(dp, root)
    depth = 0 if rel == "." else rel.count(os.sep) + 1
    dn[:] = [d for d in dn if not d.startswith(".")]
    if depth >= MAXD:
        dn[:] = []
    files = [x for x in fn if not x.startswith(".")]
    if "config.json" in files and rel != ".":
        dn[:] = []
        continue
    for x in files:
        if x.lower().endswith((".gguf", ".safetensors")):
            _parse_paths.append(os.path.join(dp, x))

META = {}
if _parse_paths and mode != "light":
    done = False
    try:
        import multiprocessing as _mp
        n = min(8, os.cpu_count() or 4, len(_parse_paths))
        if n > 1:
            ctx = _mp.get_context("fork")
            with ctx.Pool(n) as pool:
                vals = pool.map(parse_one, _parse_paths, chunksize=1)
            META = {p: v for p, v in zip(_parse_paths, vals) if v}
            done = True
    except Exception:
        pass
    if not done:
        for p in _parse_paths:
            v = parse_one(p)
            if v:
                META[p] = v

def entry(dp, fname, kind, info):
    p = os.path.join(dp, fname)
    return {
        "path": p,
        "rel": os.path.relpath(p, root),
        "name": fname,
        "kind": kind,
        "sizeBytes": safe_size(p),
        "arch": info.get("arch"),
        "quant": info.get("quant"),
        "params": info.get("params"),
        "ctx": info.get("ctx"),
        "note": info.get("label"),
    }

results = []
for dp, dn, fn in os.walk(root, topdown=True):
    rel = os.path.relpath(dp, root)
    is_root = rel == "."
    depth = 0 if is_root else rel.count(os.sep) + 1
    dn[:] = [d for d in dn if not d.startswith(".")]
    if depth >= MAXD:
        dn[:] = []
    files = [x for x in fn if not x.startswith(".")]
    if "config.json" in files and not is_root:
        arch = None
        try:
            with open(os.path.join(dp, "config.json"), "r", encoding="utf-8") as cf:
                cfg = json.load(cf)
            al = cfg.get("architectures") or []
            if al and isinstance(al[0], str):
                arch = al[0]
        except Exception:
            pass
        results.append({
            "path": dp,
            "rel": rel,
            "name": os.path.basename(dp) or dp,
            "kind": "hf",
            "sizeBytes": dir_size(dp),
            "arch": arch,
            "quant": None,
            "params": None,
            "ctx": None,
            "note": None,
        })
        dn[:] = []
        continue
    split_groups = {}
    singles = []
    for x in files:
        low = x.lower()
        if low.endswith(".gguf"):
            m = SPLIT_RE.match(x)
            if m:
                split_groups.setdefault((m.group(1), m.group(3)), []).append(x)
            else:
                singles.append(x)
        elif low.endswith(".safetensors"):
            singles.append(x)
    for key in split_groups:
        xs = sorted(split_groups[key])
        p0 = os.path.join(dp, xs[0])
        info = META.get(p0) or {}
        sz = 0
        for x in xs:
            s = safe_size(os.path.join(dp, x))
            if s:
                sz += s
        results.append({
            "path": p0,
            "rel": os.path.relpath(p0, root),
            "name": key[0] or xs[0],
            "kind": "gguf-split",
            "sizeBytes": sz,
            "arch": info.get("arch"),
            "quant": quant_of(xs[0]),
            "params": None,
            "ctx": info.get("ctx"),
            "note": str(len(xs)) + " shards",
        })
    for x in singles:
        if x.lower().endswith(".gguf"):
            info = META.get(os.path.join(dp, x)) or {}
            info["quant"] = quant_of(x)
            results.append(entry(dp, x, "gguf", info))
        else:
            info = META.get(os.path.join(dp, x)) or {}
            results.append({
                "path": os.path.join(dp, x),
                "rel": os.path.relpath(os.path.join(dp, x), root),
                "name": x,
                "kind": "safetensors",
                "sizeBytes": safe_size(os.path.join(dp, x)),
                "arch": None,
                "quant": info.get("dtype"),
                "params": info.get("params"),
                "ctx": None,
                "note": None,
            })
    if not is_root and depth == 1 and not split_groups and not singles:
        sz = dir_size(dp)
        if sz > 0:
            results.append({
                "path": dp,
                "rel": rel,
                "name": os.path.basename(dp) or dp,
                "kind": "dir",
                "sizeBytes": sz,
                "arch": None,
                "quant": None,
                "params": None,
                "ctx": None,
                "note": None,
            })

results.sort(key=lambda r: -(r.get("sizeBytes") or 0))

if mode == "light":
    # 轻量模式：不解析头部（META 为空，arch/params/ctx 为 null），输出
    # 目录结构指纹 + 带最新 sizes/quant/note 的条目；调用方与缓存比对后
    # 用缓存的解析元数据合并，秒级返回。指纹排除普通 dir 条目：
    # 其 size 汇总含活跃写入的日志（如 docker build 输出），会误判变化；
    # 模型文件/分片/hf 目录仍参与指纹
    import hashlib
    fp_src = "\n".join(sorted(
        r["path"] + "|" + r["kind"] + "|" + str(r.get("sizeBytes") or 0)
        for r in results
        if r["kind"] != "dir"
    ))
    sys.stderr.write("FP:" + hashlib.md5(fp_src.encode()).hexdigest()[:16] + "\n")

for r in results:
    emit(r)
"#;

/// 模型目录校验：只允许绝对路径 / $HOME 路径，拒绝 shell 元字符（目录以双引号传入远端 shell）
fn validate_models_dir(p: &str) -> Result<(), AppError> {
    if !p.starts_with('/') && !p.starts_with("$HOME") {
        return Err(AppError::Other("模型目录必须是绝对路径".into()));
    }
    let rest = p
        .strip_prefix("$HOME")
        .map(|s| s.strip_prefix('/').unwrap_or(s))
        .unwrap_or(p);
    if rest
        .chars()
        .any(|c| matches!(c, '"' | '`' | '$' | '\\' | ';' | '|' | '&' | '<' | '>' | '(' | ')' | '*' | '?' | '#' | '\n' | '\r'))
    {
        return Err(AppError::Other("模型目录含非法字符".into()));
    }
    Ok(())
}

/// 本地模型解析缓存（内存 + 持久化 model-cache.json，按服务器档案缓存；
/// 指纹 = 轻扫 path|kind|sizeBytes 的 md5，排除普通 dir 条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCacheEntry {
    pub fp: String,
    pub list: Vec<LocalModel>,
}

const MODEL_CACHE_STORE: &str = "model-cache.json";

fn load_cache_entry(app: &AppHandle, pid: &str) -> Option<ModelCacheEntry> {
    use tauri_plugin_store::StoreExt;
    let store = app.store(MODEL_CACHE_STORE).ok()?;
    let v = store.get(pid)?;
    serde_json::from_value(v).ok()
}

fn save_cache_entry(app: &AppHandle, pid: &str, entry: &ModelCacheEntry) {
    use tauri_plugin_store::StoreExt;
    if let Ok(store) = app.store(MODEL_CACHE_STORE) {
        if let Ok(v) = serde_json::to_value(entry) {
            let _ = store.set(pid, v);
            let _ = store.save();
        }
    }
}

fn parse_models_stdout(stdout: &str) -> Vec<LocalModel> {
    let mut list = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(m) = serde_json::from_str::<LocalModel>(line) {
            if !m.path.is_empty() {
                list.push(m);
            }
        }
    }
    list
}

/// 轻扫条目（sizes/quant/note 最新）+ 缓存元数据（arch/params/ctx，来自头部解析）合并
fn merge_meta(light: Vec<LocalModel>, cached: &[LocalModel]) -> Vec<LocalModel> {
    let by_path: std::collections::HashMap<&str, &LocalModel> =
        cached.iter().map(|c| (c.path.as_str(), c)).collect();
    light
        .into_iter()
        .map(|mut e| {
            if let Some(c) = by_path.get(e.path.as_str()) {
                e.arch = c.arch.clone();
                e.params = c.params.clone();
                e.ctx = c.ctx;
            }
            e
        })
        .collect()
}

#[tauri::command]
pub async fn list_local_models(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<Vec<LocalModel>, AppError> {
    let profile = get_profile(&app, &profile_id)?;
    let settings = crate::settings::load_settings(&app)?;
    let dir = crate::profile::effective_models_dir(&profile, &settings);
    validate_models_dir(&dir)?;

    // 第一步：轻量扫描（~1s）拿指纹 + 最新条目；指纹命中缓存（内存 → 持久化）则
    // 合并缓存的解析元数据直接返回，应用重启后同样秒开
    let light = format!("python3 - \"{}\" light <<'RLPY'\n{}\nRLPY", dir, LIST_MODELS_PY);
    let mut fp: Option<String> = None;
    if let Ok(out) = run_on(&state, &profile_id, &light).await {
        if out.exit_code == 2 {
            let msg = out.stderr.trim();
            let path = msg.strip_prefix("MODELS_DIR_MISSING").unwrap_or(msg).trim();
            return Err(AppError::Other(format!("模型目录不存在: {path}")));
        }
        let light_list = parse_models_stdout(&out.stdout);
        if let Some(f) = out
            .stderr
            .lines()
            .find_map(|l| l.trim().strip_prefix("FP:"))
        {
            fp = Some(f.trim().to_string());
        }
        if !light_list.is_empty() {
            if let Some(f) = &fp {
                let in_mem = {
                    let cache = state.model_cache.lock().unwrap();
                    cache.get(&profile_id).filter(|e| e.fp == *f).cloned()
                };
                let hit = match in_mem {
                    Some(e) => Some(e),
                    None => load_cache_entry(&app, &profile_id).filter(|e| &e.fp == f).map(
                        |e| {
                            state
                                .model_cache
                                .lock()
                                .unwrap()
                                .insert(profile_id.clone(), e.clone());
                            e
                        },
                    ),
                };
                if let Some(entry) = hit {
                    crate::applog::info(
                        "app",
                        &format!(
                            "model_cache hit profile={profile_id} fp={} n={}",
                            f,
                            light_list.len()
                        ),
                    );
                    return Ok(merge_meta(light_list, &entry.list));
                }
            }
        }
    }

    // 第二步：全量扫描（解析 GGUF/safetensors 头部，较慢）并更新缓存（内存 + 持久化）
    let script = format!("python3 - \"{}\" <<'RLPY'\n{}\nRLPY", dir, LIST_MODELS_PY);
    let out = run_on(&state, &profile_id, &script).await?;
    if out.exit_code == 2 {
        let msg = out.stderr.trim();
        let path = msg.strip_prefix("MODELS_DIR_MISSING").unwrap_or(msg).trim();
        return Err(AppError::Other(format!("模型目录不存在: {path}")));
    }
    let list = parse_models_stdout(&out.stdout);
    if list.is_empty() && out.exit_code != 0 {
        let msg = out.stderr.lines().next().unwrap_or("").trim();
        return Err(AppError::Other(if msg.is_empty() {
            "本地模型扫描失败（python3 不可用？）".into()
        } else {
            msg.to_string()
        }));
    }
    if let Some(f) = fp {
        let entry = ModelCacheEntry {
            fp: f,
            list: list.clone(),
        };
        state
            .model_cache
            .lock()
            .unwrap()
            .insert(profile_id.clone(), entry.clone());
        save_cache_entry(&app, &profile_id, &entry);
    }
    crate::applog::info(
        "app",
        &format!("model_cache full_scan profile={profile_id} n={}", list.len()),
    );
    Ok(list)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsStatus {
    pub modelscope: bool,
    pub huggingface: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParserLibsStatus {
    pub gguf: bool,
    pub safetensors: bool,
}

/// 检查服务器上的下载工具是否已安装（与下载时的检查逻辑一致）
#[tauri::command]
pub async fn check_download_tools(
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<ToolsStatus, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&profile_id) else {
        return Err(AppError::NotConnected(profile_id));
    };
    let out = session
        .run(
            "{ command -v modelscope >/dev/null 2>&1 || [ -x \"$HOME/.local/bin/modelscope\" ]; } \
             && echo MS_OK || echo MS_MISSING; \
             { command -v huggingface-cli >/dev/null 2>&1 || [ -x \"$HOME/.local/bin/huggingface-cli\" ]; } \
             && echo HF_OK || echo HF_MISSING",
        )
        .await?;
    let s = out.stdout.as_str();
    Ok(ToolsStatus {
        modelscope: s.contains("MS_OK"),
        huggingface: s.contains("HF_OK"),
    })
}

/// 检查服务器上的模型解析库是否已安装（gguf / safetensors）；缺失时头部字段（架构/参数/上下文）不可用
#[tauri::command]
pub async fn check_parser_libs(
    state: State<'_, crate::AppState>,
    profile_id: String,
) -> Result<ParserLibsStatus, AppError> {
    let out = run_on(
        &state,
        &profile_id,
        "python3 -c \"import importlib.util as u; \
         print('GGUF_OK' if u.find_spec('gguf') else 'GGUF_NO'); \
         print('ST_OK' if u.find_spec('safetensors') else 'ST_NO')\"",
    )
    .await?;
    let s = out.stdout.as_str();
    Ok(ParserLibsStatus {
        gguf: s.contains("GGUF_OK"),
        safetensors: s.contains("ST_OK"),
    })
}

/// files 为要下载的文件模式（空 = 整个仓库）；proxy 为代理导出前缀（空 = 未启用）
pub fn download_command(
    source: &str,
    model_id: &str,
    dest: &str,
    token: Option<&str>,
    hf_endpoint: Option<&str>,
    files: &[String],
    proxy: &str,
) -> Result<String, AppError> {
    let dest_q = format!("'{}'", dest.replace('\'', "'\\''"));
    let id_q = model_id.replace('\'', "'\\''");
    let pats: Vec<String> = files
        .iter()
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    match source {
        "modelscope" => {
            let mut c = format!("modelscope download --model '{id_q}'");
            for p in &pats {
                c.push_str(&format!(" --include '{}'", p.replace('\'', "'\\''")));
            }
            // 非 root 安装的 CLI 在 ~/.local/bin，非交互 PATH 可能没有它
            Ok(format!(
                "export PATH=\"$PATH:$HOME/.local/bin\"; {proxy}{c} --local_dir {dest_q} 2>&1"
            ))
        }
        "huggingface" => {
            let mut c = format!("huggingface-cli download '{id_q}'");
            for p in &pats {
                c.push_str(&format!(" '{}'", p.replace('\'', "'\\''")));
            }
            c.push_str(&format!(" --local-dir {dest_q}"));
            let mut env = String::new();
            if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
                env.push_str(&format!("HF_TOKEN={} ", t));
            }
            if let Some(e) = hf_endpoint.map(str::trim).filter(|e| !e.is_empty()) {
                env.push_str(&format!("HF_ENDPOINT='{}' ", e.replace('\'', "'\\''")));
            }
            Ok(format!(
                "export PATH=\"$PATH:$HOME/.local/bin\"; {proxy}{env}{c} 2>&1"
            ))
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
    files: Option<Vec<String>>,
) -> Result<String, AppError> {
    let profile = get_profile(&app, &profile_id)?;
    let settings = crate::settings::load_settings(&app)?;
    // 检查 CLI 是否存在
    let check = if source == "modelscope" {
        "{ command -v modelscope >/dev/null 2>&1 || [ -x \"$HOME/.local/bin/modelscope\" ]; } \
         && echo OK || echo MISSING"
    } else {
        "{ command -v huggingface-cli >/dev/null 2>&1 || [ -x \"$HOME/.local/bin/huggingface-cli\" ]; } \
         && echo OK || echo MISSING"
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
        .unwrap_or_else(|| {
            format!(
                "{}/{}",
                crate::profile::effective_models_dir(&profile, &settings),
                model_id.split('/').last().unwrap_or(model_id.as_str())
            )
        });

    let task_id = format!("dl-{}", chrono::Utc::now().timestamp_millis());
    let cmd = download_command(
        &source,
        &model_id,
        &dest,
        token.as_deref(),
        Some(settings.hf_endpoint.as_str()),
        files.as_deref().unwrap_or(&[]),
        &crate::settings::proxy_env_prefix(&settings),
    )?;
    if !state.conns.lock().await.contains_key(&profile_id) {
        return Err(AppError::NotConnected(profile_id));
    }
    crate::applog::info(
        "task",
        &format!(
            "model_download profile={profile_id} source={source} model={model_id} dest={dest}"
        ),
    );
    crate::ssh::SshSession::spawn_stream(app, profile_id, cmd, task_id.clone());
    Ok(task_id)
}

/// name 为相对模型目录的路径（可含子目录）；分片组首片会整组删除
#[tauri::command]
pub async fn model_delete(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    profile_id: String,
    name: String,
) -> Result<String, AppError> {
    let rel = name.trim();
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.split('/').any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(AppError::Other("非法模型路径".into()));
    }
    let profile = get_profile(&app, &profile_id)?;
    let settings = crate::settings::load_settings(&app)?;
    let base = crate::profile::effective_models_dir(&profile, &settings);
    let target = format!("{}/{}", base, rel);
    let cmd = match rel.rsplit_once('/') {
        Some((dir_part, fname))
            if is_safe_name(dir_part) && split_prefix(fname).is_some() =>
        {
            let prefix = split_prefix(fname).unwrap();
            let ddir = format!("{}/{}", base, dir_part);
            format!(
                "if ls {d}/{p}*-of-*.gguf >/dev/null 2>&1; then rm -f {d}/{p}*-of-*.gguf && echo DELETED; else echo NOT_FOUND; fi",
                d = shq(&ddir),
                p = shq(&prefix)
            )
        }
        _ => format!(
            "if [ -e {} ]; then rm -rf {} && echo DELETED; else echo NOT_FOUND; fi",
            shq(&target),
            shq(&target)
        ),
    };
    crate::applog::info("task", &format!("model_delete profile={profile_id} rel={rel}"));
    let out = run_on(&state, &profile_id, &cmd).await?;
    Ok(out.stdout.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_prefix_cases() {
        assert_eq!(split_prefix("model-00001-of-00003.gguf").as_deref(), Some("model"));
        assert_eq!(
            split_prefix("Qwen3.8-27B-00001-of-00013.gguf").as_deref(),
            Some("Qwen3.8-27B")
        );
        assert_eq!(
            split_prefix("llama-3-00001-of-00002.gguf").as_deref(),
            Some("llama-3")
        );
        assert_eq!(split_prefix("model.gguf"), None);
        assert_eq!(split_prefix("-of-1.gguf"), None);
        assert_eq!(split_prefix("x-1-of-2.bin"), None);
        assert_eq!(split_prefix("bad'name-1-of-2.gguf"), None);
    }
}
