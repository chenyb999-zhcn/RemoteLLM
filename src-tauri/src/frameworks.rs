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

fn pnumf_opt(p: &serde_json::Value, key: &str) -> Option<f64> {
    p.get(key).and_then(|v| v.as_f64())
}

fn pbool(p: &serde_json::Value, key: &str) -> bool {
    p.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// 数字格式化：整数不带小数点（99 → "99"，0.85 → "0.85"）
fn fnum(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// 简单字符串标志：参数非空才输出
fn pflag_s(p: &serde_json::Value, c: &mut Vec<String>, key: &str, flag: &str) {
    if let Some(v) = pstr_opt(p, key) {
        c.push(format!("{flag} {v}"));
    }
}

/// 需加引号的字符串标志（JSON/路径等含特殊字符的值）
fn pflag_sq(p: &serde_json::Value, c: &mut Vec<String>, key: &str, flag: &str) {
    if let Some(v) = pstr_opt(p, key) {
        c.push(format!("{flag} {}", shq(&v)));
    }
}

/// 简单数字标志：参数非空才输出
fn pflag_n(p: &serde_json::Value, c: &mut Vec<String>, key: &str, flag: &str) {
    if let Some(v) = pnumf_opt(p, key) {
        c.push(format!("{flag} {}", fnum(v)));
    }
}

/// 开关标志（默认关闭）：为 true 才输出
fn pflag_on(p: &serde_json::Value, c: &mut Vec<String>, key: &str, flag: &str) {
    if pbool(p, key) {
        c.push(flag.to_string());
    }
}

/// 开关标志（默认开启）：显式关闭（false）才输出对应的否定标志
fn pflag_off(p: &serde_json::Value, c: &mut Vec<String>, key: &str, flag: &str) {
    if p.get(key).and_then(|v| v.as_bool()) == Some(false) {
        c.push(flag.to_string());
    }
}

/// 由实例配置组装远端执行命令（前端预览与真正启动共用）
pub fn norm_args(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// llama-cpp 全部可选参数（原生与 Docker 共用），标志对照官方 tools/server/README.md
fn push_llama_common(p: &serde_json::Value, c: &mut Vec<String>) {
    // GPU 与显存
    pflag_s(p, c, "flashAttn", "--flash-attn");
    pflag_s(p, c, "cacheTypeK", "--cache-type-k");
    pflag_s(p, c, "cacheTypeV", "--cache-type-v");
    pflag_off(p, c, "kvOffload", "--no-kv-offload");
    // 加载模式：新参数 loadMode；旧实例用 mlock 布尔（true → mlock）向后兼容
    let load_mode = pstr_opt(p, "loadMode")
        .filter(|s| s != "auto")
        .or_else(|| pbool(p, "mlock").then(|| "mlock".to_string()));
    if let Some(v) = load_mode {
        c.push(format!("--load-mode {v}"));
    }
    pflag_s(p, c, "splitMode", "--split-mode");
    pflag_s(p, c, "tensorSplit", "-ts");
    pflag_n(p, c, "mainGpu", "-mg");
    pflag_s(p, c, "device", "-dev");
    pflag_on(p, c, "cpuMoe", "-cmoe");
    pflag_n(p, c, "nCpuMoe", "-ncmoe");
    pflag_n(p, c, "nCpuFfn", "-ncffn");
    pflag_s(p, c, "numa", "--numa");
    pflag_on(p, c, "noHost", "--no-host");
    pflag_on(p, c, "swaFull", "--swa-full");
    // 采样
    if let Some(v) = pnumf_opt(p, "temperature") {
        c.push(format!("--temperature {}", fnum(v)));
    }
    pflag_n(p, c, "topK", "--top-k");
    pflag_n(p, c, "topP", "--top-p");
    pflag_n(p, c, "minP", "--min-p");
    pflag_n(p, c, "typical", "--typical-p");
    pflag_n(p, c, "topNsigma", "--top-nsigma");
    pflag_n(p, c, "xtcProbability", "--xtc-probability");
    pflag_n(p, c, "xtcThreshold", "--xtc-threshold");
    pflag_n(p, c, "repeatPenalty", "--repeat-penalty");
    pflag_n(p, c, "repeatLastN", "--repeat-last-n");
    pflag_n(p, c, "presencePenalty", "--presence-penalty");
    pflag_n(p, c, "frequencyPenalty", "--frequency-penalty");
    pflag_n(p, c, "dryMultiplier", "--dry-multiplier");
    pflag_n(p, c, "dryBase", "--dry-base");
    pflag_n(p, c, "dryAllowedLength", "--dry-allowed-length");
    pflag_n(p, c, "dryPenaltyLastN", "--dry-penalty-last-n");
    pflag_n(p, c, "dynatempRange", "--dynatemp-range");
    pflag_n(p, c, "dynatempExp", "--dynatemp-exp");
    pflag_s(p, c, "mirostat", "--mirostat");
    pflag_n(p, c, "mirostatLr", "--mirostat-lr");
    pflag_n(p, c, "mirostatEnt", "--mirostat-ent");
    pflag_n(p, c, "seed", "-s");
    pflag_sq(p, c, "samplers", "--samplers");
    pflag_on(p, c, "ignoreEos", "--ignore-eos");
    // 上下文与 RoPE
    pflag_s(p, c, "ropeScaling", "--rope-scaling");
    pflag_n(p, c, "ropeScale", "--rope-scale");
    pflag_n(p, c, "ropeFreqBase", "--rope-freq-base");
    pflag_n(p, c, "ropeFreqScale", "--rope-freq-scale");
    pflag_n(p, c, "yarnOrigCtx", "--yarn-orig-ctx");
    pflag_n(p, c, "yarnExtFactor", "--yarn-ext-factor");
    pflag_n(p, c, "yarnAttnFactor", "--yarn-attn-factor");
    pflag_n(p, c, "yarnBetaSlow", "--yarn-beta-slow");
    pflag_n(p, c, "yarnBetaFast", "--yarn-beta-fast");
    pflag_on(p, c, "contextShift", "--context-shift");
    // 服务与高级
    pflag_s(p, c, "reasoning", "--reasoning");
    pflag_s(p, c, "reasoningFormat", "--reasoning-format");
    pflag_s(p, c, "reasoningEffort", "--reasoning-effort");
    pflag_off(p, c, "jinja", "--no-jinja");
    pflag_s(p, c, "chatTemplate", "--chat-template");
    pflag_s(p, c, "alias", "-a");
    pflag_s(p, c, "apiKey", "--api-key");
    pflag_n(p, c, "timeout", "-to");
    pflag_n(p, c, "threadsHttp", "--threads-http");
    pflag_off(p, c, "cachePrompt", "--no-cache-prompt");
    pflag_n(p, c, "cacheReuse", "--cache-reuse");
    pflag_n(p, c, "slotPromptSimilarity", "-sps");
    pflag_off(p, c, "webui", "--no-ui");
    pflag_sq(p, c, "lora", "--lora");
    pflag_sq(p, c, "overrideKv", "--override-kv");
}

/// llama-cpp 投机解码参数（原生与 Docker 共用）。
/// specType: none | draft-mtp | draft-dflash | draft-dspark；
/// 向后兼容：旧实例用 mtp 布尔开关（true → draft-mtp）。
/// dflash/dspark 需要 sidecar draft 模型（--spec-draft-model），mtp 不需要。
fn push_llama_spec(p: &serde_json::Value, c: &mut Vec<String>) {
    let spec = pstr_opt(p, "specType")
        .or_else(|| pbool(p, "mtp").then(|| "draft-mtp".to_string()))
        .filter(|s| s != "none");
    let Some(spec) = spec else {
        return;
    };
    c.push(format!("--spec-type {spec}"));
    if spec.starts_with("draft-d") {
        if let Some(v) = pstr_opt(p, "specDraftModel") {
            c.push(format!("--spec-draft-model {}", shq(&v)));
        }
    }
    c.push(format!(
        "--spec-draft-n-max {}",
        fnum(pnumf_opt(p, "specDraftNMax").unwrap_or(4.0))
    ));
    if let Some(v) = pnumf_opt(p, "specDraftNMin") {
        c.push(format!("--spec-draft-n-min {}", fnum(v)));
    }
    c.push(format!(
        "--spec-draft-p-min {}",
        fnum(pnumf_opt(p, "specDraftPMin").unwrap_or(1.0))
    ));
    if let Some(v) = pnumf_opt(p, "specDraftPSplit") {
        c.push(format!("--spec-draft-p-split {}", fnum(v)));
    }
    if let Some(v) = pnumf_opt(p, "specDraftNgl") {
        c.push(format!("-ngld {}", fnum(v)));
    }
    if let Some(v) = pstr_opt(p, "specDraftDevice") {
        c.push(format!("-devd {v}"));
    }
}

/// vLLM / 1Cat-vLLM 通用参数（原生与 Docker 共用），
/// 对照 v1.5 实际版本 `vllm serve --help`（ModelConfig/ParallelConfig/CacheConfig/SchedulerConfig/Frontend 配置组）
/// 注意：vllm serve 没有 --temperature/--top-p/--top-k/--repetition-penalty/--max-tokens
/// 这类启动参数（采样是 OpenAI API 每请求参数），传了会 argparse 报错
fn push_vllm_common(p: &serde_json::Value, c: &mut Vec<String>) {
    // 基本
    pflag_n(p, c, "maxModelLen", "--max-model-len");
    pflag_s(p, c, "servedModelName", "--served-model-name");
    pflag_s(p, c, "dtype", "--dtype");
    pflag_n(p, c, "seed", "--seed");
    pflag_s(p, c, "revision", "--revision");
    pflag_s(p, c, "hfToken", "--hf-token");
    pflag_on(p, c, "trustRemoteCode", "--trust-remote-code");
    // 并行与显存
    pflag_n(p, c, "pp", "--pipeline-parallel-size");
    pflag_n(p, c, "dp", "--data-parallel-size");
    pflag_n(p, c, "gpuMemUtil", "--gpu-memory-utilization");
    pflag_s(p, c, "kvCacheDtype", "--kv-cache-dtype");
    pflag_off(p, c, "enablePrefixCaching", "--no-enable-prefix-caching");
    pflag_n(p, c, "blockSize", "--block-size");
    // 调度与吞吐
    pflag_n(p, c, "maxNumSeqs", "--max-num-seqs");
    pflag_n(p, c, "maxNumBatchedTokens", "--max-num-batched-tokens");
    pflag_off(p, c, "enableChunkedPrefill", "--no-enable-chunked-prefill");
    pflag_n(p, c, "streamInterval", "--stream-interval");
    pflag_on(p, c, "asyncScheduling", "--async-scheduling");
    pflag_on(p, c, "enforceEager", "--enforce-eager");
    // 量化与加载
    pflag_s(p, c, "quantization", "--quantization");
    pflag_s(p, c, "loadFormat", "--load-format");
    pflag_sq(p, c, "hfOverrides", "--hf-overrides");
    // 前端与 API
    pflag_s(p, c, "host", "--host");
    pflag_s(p, c, "apiKey", "--api-key");
    pflag_s(p, c, "chatTemplate", "--chat-template");
    pflag_sq(p, c, "chatTemplateKwargs", "--default-chat-template-kwargs");
    pflag_s(p, c, "reasoningParser", "--reasoning-parser");
    pflag_s(p, c, "toolCallParser", "--tool-call-parser");
    pflag_on(p, c, "enableAutoToolChoice", "--enable-auto-tool-choice");
    pflag_s(p, c, "allowedOrigins", "--allowed-origins");
    pflag_s(p, c, "uvicornLogLevel", "--uvicorn-log-level");
    pflag_on(p, c, "disableLogStats", "--disable-log-stats");
    pflag_on(p, c, "enableLogRequests", "--enable-log-requests");
}

/// 1Cat-vLLM 专属参数（SM70/V100 支持），对照其 README 与 1.5 实际版本 --help
fn push_1cat_extra(p: &serde_json::Value, c: &mut Vec<String>) {
    pflag_s(p, c, "attentionBackend", "--attention-backend");
    pflag_s(p, c, "gdnPrefillBackend", "--gdn-prefill-backend");
    pflag_sq(p, c, "speculativeConfig", "--speculative-config");
    pflag_s(p, c, "performanceMode", "--performance-mode");
}

/// FastLLM 全部可选参数（仅原生模式），对照 https://github.com/ztxz16/fastllm README「常用参数」
/// 服务入口 ftllm server <model>（OpenAI 兼容 API）。注意 --startup-progress 是连字符，
/// 其余长参数为下划线形式（README 列出的别名如 --max-context-length 未采用）。
fn push_fastllm_common(p: &serde_json::Value, c: &mut Vec<String>) {
    // 基本
    pflag_s(p, c, "device", "--device");
    pflag_s(p, c, "tp", "--tp");
    pflag_n(p, c, "threads", "-t");
    pflag_n(p, c, "gpuMemRatio", "--gpu_mem_ratio");
    // MoE 混合
    // moe_device 可为设备名或比例组合（含 { } ' : 等字符），加引号
    pflag_sq(p, c, "moeDevice", "--moe_device");
    pflag_n(p, c, "moeDeviceLayers", "--moe_device_layers");
    pflag_s(p, c, "moeDtype", "--moe_dtype");
    pflag_s(p, c, "atype", "--atype");
    pflag_s(p, c, "moeAtype", "--moe_atype");
    pflag_sq(p, c, "moeCudaCache", "--moe_cuda_cache");
    pflag_s(p, c, "ngramDevice", "--ngram_device");
    // 显存与上下文
    pflag_s(p, c, "dtype", "--dtype");
    pflag_s(p, c, "kvCacheDtype", "--kv_cache_dtype");
    pflag_n(p, c, "tokens", "--tokens");
    pflag_n(p, c, "pageSize", "--page_size");
    pflag_n(p, c, "maxBatch", "--max_batch");
    pflag_n(p, c, "maxContextLength", "--max_context_length");
    // rope_scaling 接受 yarn 或 JSON，加引号
    pflag_sq(p, c, "ropeScaling", "--rope_scaling");
    pflag_n(p, c, "chunkedPrefillSize", "--chunked_prefill_size");
    pflag_s(p, c, "prefixCache", "--prefix_cache");
    pflag_n(p, c, "cudaSlab", "--cuda_slab");
    // 解码与投机
    pflag_n(p, c, "mtp", "--mtp");
    pflag_n(p, c, "dspark", "--dspark");
    pflag_sq(p, c, "draft", "--draft");
    pflag_n(p, c, "draftTokens", "--draft_tokens");
    pflag_s(p, c, "enableThinking", "--enable_thinking");
    pflag_s(p, c, "toolCallParser", "--tool_call_parser");
    pflag_sq(p, c, "chatTemplate", "--chat_template");
    // 服务
    pflag_s(p, c, "host", "--host");
    pflag_s(p, c, "modelName", "--model_name");
    pflag_s(p, c, "apiKey", "--api_key");
    pflag_n(p, c, "temperature", "--temperature");
    pflag_n(p, c, "topP", "--top_p");
    pflag_n(p, c, "topK", "--top_k");
    pflag_n(p, c, "repeatPenalty", "--repeat_penalty");
    pflag_on(p, c, "hideInput", "--hide_input");
    pflag_s(p, c, "startupProgress", "--startup-progress");
}

/// SGLang 全部可选参数（原生与 Docker 共用），对照 0.5.19 实际版本 --help
/// 注意：sglang serve 没有 --temperature/--top-p/--top-k/--repetition-penalty/--max-tokens
/// 这类启动参数（采样是 OpenAI API 每请求参数），传了会 argparse 报错
fn push_sglang_common(p: &serde_json::Value, c: &mut Vec<String>) {
    // 基本
    pflag_s(p, c, "dtype", "--dtype");
    pflag_n(p, c, "contextLength", "--context-length");
    pflag_s(p, c, "quantization", "--quantization");
    pflag_s(p, c, "loadFormat", "--load-format");
    pflag_s(p, c, "kvCacheDtype", "--kv-cache-dtype");
    pflag_s(p, c, "revision", "--revision");
    pflag_on(p, c, "trustRemoteCode", "--trust-remote-code");
    // 内存与调度
    pflag_n(p, c, "memFractionStatic", "--mem-fraction-static");
    pflag_n(p, c, "maxNumSeqs", "--max-running-requests");
    pflag_n(p, c, "maxTotalTokens", "--max-total-tokens");
    pflag_n(p, c, "chunkedPrefillSize", "--chunked-prefill-size");
    pflag_n(p, c, "maxPrefillTokens", "--max-prefill-tokens");
    pflag_s(p, c, "schedulePolicy", "--schedule-policy");
    pflag_n(p, c, "scheduleConservativeness", "--schedule-conservativeness");
    pflag_n(p, c, "pageSize", "--page-size");
    pflag_on(p, c, "disableRadixCache", "--disable-radix-cache");
    // 并行
    pflag_n(p, c, "pp", "--pp");
    pflag_n(p, c, "dp", "--dp");
    pflag_on(p, c, "enableDpAttention", "--enable-dp-attention");
    pflag_s(p, c, "device", "--device");
    pflag_n(p, c, "baseGpuId", "--base-gpu-id");
    pflag_n(p, c, "gpuIdStep", "--gpu-id-step");
    pflag_n(p, c, "randomSeed", "--random-seed");
    // 服务与 API
    pflag_s(p, c, "host", "--host");
    pflag_s(p, c, "servedModelName", "--served-model-name");
    pflag_s(p, c, "apiKey", "--api-key");
    pflag_s(p, c, "chatTemplate", "--chat-template");
    pflag_sq(p, c, "chatTemplateKwargs", "--default-chat-template-kwargs");
    pflag_on(p, c, "enableMetrics", "--enable-metrics");
    pflag_s(p, c, "logLevel", "--log-level");
    pflag_on(p, c, "logRequests", "--log-requests");
    pflag_n(p, c, "streamInterval", "--stream-interval");
    pflag_on(p, c, "skipServerWarmup", "--skip-server-warmup");
    // 投机解码
    pflag_s(p, c, "speculativeAlgorithm", "--speculative-algorithm");
    pflag_s(p, c, "speculativeDraftModel", "--speculative-draft-model-path");
    pflag_n(p, c, "speculativeNumDraftTokens", "--speculative-num-draft-tokens");
    pflag_n(p, c, "speculativeNumSteps", "--speculative-num-steps");
    pflag_n(p, c, "speculativeEagleTopk", "--speculative-eagle-topk");
    pflag_s(p, c, "speculativeAttentionMode", "--speculative-attention-mode");
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
            // vLLM/1Cat 均装/需跑在 3.12 venv 里（1Cat 预编译 wheel 只装 `vllm` 入口，
            // 无 1cat-vllm 命令）；裸命令名由启动脚本回退到 venv 的 vllm 二进制
            let bin = pstr(p, "bin", "vllm");
            let mut c = vec![
                format!("{} serve", bin),
                m.clone(),
                format!("--port {}", cfg.port),
                format!("--tensor-parallel-size {}", pnum(p, "tp", 1)),
            ];
            push_vllm_common(p, &mut c);
            if cfg.framework == "1cat-vllm" {
                push_1cat_extra(p, &mut c);
            }
            if let Some(v) = pstr_opt(p, "extraArgs") {
                c.push(norm_args(&v));
            }
            Ok(c.join(" "))
        }
        "sglang" => {
            // sglang 装在 3.12 venv 里（系统 python3 是 3.14，torch.compile 不支持 3.14，
            // sglang import 即崩）。用 venv 的 python 跑，裸命令名由启动脚本回退解析。
            let mut c = vec![
                "sglang_py -m sglang.launch_server".to_string(),
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
            // 基本（可选）
            pflag_n(p, &mut c, "nPredict", "-n");
            pflag_n(p, &mut c, "threadsBatch", "-tb");
            pflag_n(p, &mut c, "batchSize", "-b");
            pflag_n(p, &mut c, "ubatchSize", "-ub");
            pflag_n(p, &mut c, "parallel", "-np");
            pflag_n(p, &mut c, "keep", "--keep");
            if let Some(v) = pstr_opt(p, "host") {
                c.push(format!("--host {}", v));
            }
            push_llama_common(p, &mut c);
            push_llama_spec(p, &mut c);
            if let Some(v) = pstr_opt(p, "extraArgs") {
                c.push(norm_args(&v));
            }
            Ok(c.join(" "))
        }
        "fastllm" => {
            // ftllm 装在 3.12 venv 里（一键安装），裸命令名由启动脚本回退解析
            let bin = pstr(p, "bin", "ftllm");
            let mut c = vec![
                format!("{} server", bin),
                m.clone(),
                format!("--port {}", cfg.port),
            ];
            push_fastllm_common(p, &mut c);
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
    // FastLLM 暂无官方 Docker 镜像（仓库 Dockerfile 为源码构建的老版 webui），不支持 Docker 模式
    if cfg.framework == "fastllm" {
        return Err(AppError::Other("FastLLM 暂无官方 Docker 镜像，不支持 Docker 模式（请使用原生模式）".into()));
    }
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
            if cfg.framework == "1cat-vllm" {
                push_1cat_extra(p, &mut run);
            }
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
            // 容器内需监听 0.0.0.0 才能被 -p 端口映射访问
            run.push(match pstr_opt(p, "host") {
                Some(v) => format!("--host {v}"),
                None => "--host 0.0.0.0".to_string(),
            });
            run.push("--metrics".into());
            pflag_n(p, &mut run, "nPredict", "-n");
            pflag_n(p, &mut run, "threadsBatch", "-tb");
            pflag_n(p, &mut run, "batchSize", "-b");
            pflag_n(p, &mut run, "ubatchSize", "-ub");
            pflag_n(p, &mut run, "parallel", "-np");
            pflag_n(p, &mut run, "keep", "--keep");
            push_llama_common(p, &mut run);
            push_llama_spec(p, &mut run);
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
        "1cat-vllm" => "ghcr.io/chenyb999-zhcn/1cat-vllm:1.5".into(),
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
# vLLM 装在 3.12 venv 里（一键安装）；先查 PATH 上的 vllm，再回退到 venv 的 vllm 二进制
_v=""
command -v vllm >/dev/null 2>&1 && _v=$(vllm --version 2>/dev/null | head -1)
if [ -z "$_v" ] && [ -x "$HOME/RemoteLLM/frameworks/vllm-venv/bin/vllm" ]; then
  _v=$("$HOME/RemoteLLM/frameworks/vllm-venv/bin/vllm" --version 2>/dev/null | head -1)
fi
_p=$(python3 -c "import vllm; print('python-vllm', vllm.__version__)" 2>/dev/null)
[ -n "$_v" ] && echo "$_v"
[ -n "$_p" ] && echo "$_p"
[ -z "$_v" ] && [ -z "$_p" ] && echo NONE
echo "==1CAT=="
_v=""
# 1Cat-vLLM 预编译 wheel 装的是 `vllm` 入口，且跑在 3.12 venv 里；
# 先查 PATH 上的 1cat-vllm/vllm，再回退到 venv 的 vllm 二进制
if command -v 1cat-vllm >/dev/null 2>&1; then _v=$(1cat-vllm --version 2>/dev/null | head -1); fi
if [ -z "$_v" ] && [ -x "$HOME/RemoteLLM/frameworks/1cat-venv/bin/vllm" ]; then
  _v=$("$HOME/RemoteLLM/frameworks/1cat-venv/bin/vllm" --version 2>/dev/null | head -1)
fi
[ -n "$_v" ] && echo "$_v"
[ -z "$_v" ] && echo NONE
echo "==SGLANG=="
# sglang 装在 3.12 venv 里（系统 python3 是 3.14，import 即崩），优先查 venv
_p=$("$HOME/RemoteLLM/frameworks/sglang-venv/bin/python" -c "import sglang; print('sglang', sglang.__version__)" 2>/dev/null)
[ -z "$_p" ] && _p=$(python3 -c "import sglang; print('sglang', sglang.__version__)" 2>/dev/null)
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
echo "==FASTLLM=="
# ftllm 装在 3.12 venv 里（一键安装）；先查 PATH，再回退到 venv 的 ftllm 二进制
_v=""
command -v ftllm >/dev/null 2>&1 && _v=$(ftllm --version 2>/dev/null | head -1)
if [ -z "$_v" ] && [ -x "$HOME/RemoteLLM/frameworks/ftllm-venv/bin/ftllm" ]; then
  _v=$("$HOME/RemoteLLM/frameworks/ftllm-venv/bin/ftllm" --version 2>/dev/null | head -1)
fi
[ -n "$_v" ] && echo "$_v"
[ -z "$_v" ] && echo NONE
exit 0
"#;

const DETECT_FRAMEWORKS: [(&str, &str); 5] = [
    ("vllm", "VLLM"),
    ("1cat-vllm", "1CAT"),
    ("sglang", "SGLANG"),
    ("llama-cpp", "LLAMA"),
    ("fastllm", "FASTLLM"),
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

/// 端口占用探测片段（幂等、无外部依赖）：目标端口已被监听则输出 PORT_BUSY 并退出。
/// 优先 ss（iproute2，绝大多数发行版自带），回退 bash /dev/tcp 连接探测。
fn port_busy_check(port: u16) -> String {
    format!(
        "if command -v ss >/dev/null 2>&1; then \
         ss -lnt 2>/dev/null | awk '{{print $4}}' | grep -Eq \":{port}$\" && {{ echo PORT_BUSY {port}; exit 5; }}; \
         else (exec 3<>dev/tcp/127.0.0.1/{port}) 2>/dev/null && {{ echo PORT_BUSY {port}; exit 5; }}; \
         fi; "
    )
}

fn start_script(cfg: &InstanceConfig, profile: &crate::profile::ServerProfile) -> String {
    let run = profile.run_dir();
    let logs = profile.logs_dir();
    let s = slug(&cfg.name);
    let pidfile = format!("{run}/{s}.pid");
    let log = format!("{logs}/{s}.log");
    let portcheck = port_busy_check(cfg.port);

    if cfg.mode == "docker" {
        let cmd = build_command(cfg).unwrap_or_default();
        return format!(
            "{portcheck}\n\
             docker ps -a --format '{{{{.Names}}}}' | grep -qx {s} && docker rm -f {s} >/dev/null 2>&1\n\
             {cmd}\necho \"DOCKER_STARTED\"",
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
    // vLLM 装在 3.12 venv 里（一键安装；系统 python3 可能是 3.14），
    // 裸命令名不在 PATH 时回退到 venv 的 vllm 二进制
    if cfg.framework == "vllm" {
        let bin = pstr(&cfg.params, "bin", "vllm");
        if !bin.is_empty() && !bin.contains('/') && cmd.starts_with(bin.as_str()) {
            let venv_bin = format!(
                "{}/frameworks/vllm-venv/bin/{}",
                profile.base_dir.trim_end_matches('/'),
                bin
            );
            prelude = format!(
                "vllm_bin=$(command -v {bin} 2>/dev/null)\n\
                 [ -z \"$vllm_bin\" ] && [ -x {venv_bin} ] && vllm_bin={venv_bin}\n\
                 [ -z \"$vllm_bin\" ] && vllm_bin={bin}\n"
            );
            cmd = format!("\"$vllm_bin\"{}", &cmd[bin.len()..]);
        }
    }
    // 1Cat-vLLM 装在 3.12 venv 里（系统 python3 是 3.14，跑不了 1Cat），
    // 裸命令名不在 PATH 时回退到 venv 的 vllm 二进制
    if cfg.framework == "1cat-vllm" {
        let bin = pstr(&cfg.params, "bin", "vllm");
        if !bin.is_empty() && !bin.contains('/') && cmd.starts_with(bin.as_str()) {
            let venv_bin = format!(
                "{}/frameworks/1cat-venv/bin/{}",
                profile.base_dir.trim_end_matches('/'),
                bin
            );
            prelude = format!(
                "cat_bin=$(command -v {bin} 2>/dev/null)\n\
                 [ -z \"$cat_bin\" ] && [ -x {venv_bin} ] && cat_bin={venv_bin}\n\
                 [ -z \"$cat_bin\" ] && cat_bin={bin}\n"
            );
            cmd = format!("\"$cat_bin\"{}", &cmd[bin.len()..]);
        }
    }
    // sglang 装在 3.12 venv 里（系统 python3 是 3.14，torch.compile 不支持 3.14，
    // sglang import 即崩）。命令以 `sglang_py` 开头时回退到 venv 的 python。
    if cfg.framework == "sglang" && cmd.starts_with("sglang_py ") {
        let venv_py = format!(
            "{}/frameworks/sglang-venv/bin/python",
            profile.base_dir.trim_end_matches('/')
        );
        prelude = format!(
            "sglang_py=$(command -v python3 2>/dev/null)\n\
              [ -x {venv_py} ] && sglang_py={venv_py}\n"
        );
        cmd = format!("\"$sglang_py\"{}", &cmd["sglang_py".len()..]);
    }
    // FastLLM（ftllm）装在 3.12 venv 里（一键安装），裸命令名不在 PATH 时回退到 venv 的 ftllm 二进制
    if cfg.framework == "fastllm" {
        let bin = pstr(&cfg.params, "bin", "ftllm");
        if !bin.is_empty() && !bin.contains('/') && cmd.starts_with(bin.as_str()) {
            let venv_bin = format!(
                "{}/frameworks/ftllm-venv/bin/{}",
                profile.base_dir.trim_end_matches('/'),
                bin
            );
            prelude = format!(
                "ftllm_bin=$(command -v {bin} 2>/dev/null)\n\
                  [ -z \"$ftllm_bin\" ] && [ -x {venv_bin} ] && ftllm_bin={venv_bin}\n\
                  [ -z \"$ftllm_bin\" ] && ftllm_bin={bin}\n"
            );
            cmd = format!("\"$ftllm_bin\"{}", &cmd[bin.len()..]);
        }
    }
    format!(
        "mkdir -p {run} {logs}\n\
         if [ -f {pidfile} ] && kill -0 \"$(cat {pidfile})\" 2>/dev/null; then echo ALREADY_RUNNING; exit 3; fi\n\
         {portcheck}\
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
    // 同名实例已在运行：友好提示（非错误，进程本就健康）
    if out.contains("ALREADY_RUNNING") {
        return Err(AppError::Other(format!("实例「{}」已在运行", inst.name)));
    }
    // 端口被占用（可能是其它实例或手动进程）：拦截并提示
    if let Some(rest) = out.split("PORT_BUSY ").nth(1) {
        let port = rest.split_whitespace().next().unwrap_or("");
        return Err(AppError::Other(format!(
            "端口 {port} 已被占用（可能是其它实例或手动启动的进程），请先释放该端口或改用其它端口"
        )));
    }
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
    // 注意：ssh 会话的 $HOME 可能被客户端环境污染，路径中的 $HOME 必须用双引号包裹
    // 强制 shell 展开（单引号会按字面量传给 [ -f ]，导致误判文件不存在）
    let cmd = format!(
        "if [ -f \"{l}\" ]; then tail -n {n} \"{l}\"; else echo '(无日志)'; fi",
        l = log,
        n = lines
    );
    Ok(run_on(&state, &profile.id, &cmd).await?)
}

/// 日志文件总行数（用于判断是否已加载到开头）
#[tauri::command]
pub async fn instance_log_total_lines(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<u32, AppError> {
    let (inst, profile) = get_instance(&app, &state, &id).await?;
    let s = slug(&inst.name);
    let log = format!("{}/{}.log", profile.logs_dir(), s);
    let cmd = format!("wc -l < \"{l}\" 2>/dev/null || echo 0", l = log);
    let out = run_on(&state, &profile.id, &cmd).await?;
    Ok(out.trim().parse().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_detect_all_none() {
        let raw = "\n==VLLM==\nNONE\n==1CAT==\nNONE\n==SGLANG==\nNONE\n==LLAMA==\nNONE\n==FASTLLM==\nNONE\n";
        let r = parse_detect(raw);
        assert_eq!(r.len(), 5);
        for d in &r {
            assert!(!d.installed, "{}", d.framework);
            assert_eq!(d.version, None);
            assert_eq!(d.note, None);
        }
    }

    #[test]
    fn parse_detect_legacy_marker_leak() {
        // 旧脚本空段时会把下一段标记当内容（vLLM 误报 "已安装 ==1CAT=="）
        let raw = "\n==VLLM==\n==1CAT==\n==SGLANG==\nNONE\n==LLAMA==\nNONE\n==FASTLLM==\nNONE\n";
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

    #[test]
    fn build_command_llama_dflash_flags() {
        // DFlash：specType 下拉 + sidecar draft 模型（--spec-draft-model）
        let cfg = test_cfg(serde_json::json!({
            "specType": "draft-dflash",
            "specDraftModel": "/mnt/dflash-sidecar.gguf",
            "specDraftNMax": 6
        }));
        let cmd = build_command(&cfg).unwrap();
        assert!(
            cmd.contains("--spec-type draft-dflash --spec-draft-model '/mnt/dflash-sidecar.gguf' --spec-draft-n-max 6 --spec-draft-p-min 1"),
            "cmd: {cmd}"
        );
    }

    #[test]
    fn build_command_llama_dspark_docker() {
        // DSpark（docker 模式）：同样走 sidecar 机制
        let mut cfg = test_cfg(serde_json::json!({
            "specType": "draft-dspark",
            "specDraftModel": "/mnt/dspark.gguf"
        }));
        cfg.mode = "docker".into();
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.starts_with("docker run -d"), "cmd: {cmd}");
        assert!(
            cmd.contains("--spec-type draft-dspark --spec-draft-model '/mnt/dspark.gguf' --spec-draft-n-max 4 --spec-draft-p-min 1"),
            "cmd: {cmd}"
        );
    }

    #[test]
    fn build_command_llama_spec_none_and_mtp_ignores_draft_model() {
        // specType=none：不加任何投机 flag
        let cfg = test_cfg(serde_json::json!({ "specType": "none" }));
        let cmd = build_command(&cfg).unwrap();
        assert!(!cmd.contains("--spec-type"), "cmd: {cmd}");

        // MTP 选了 draft 模型路径也应忽略（mtp 不需要 sidecar）
        let cfg2 = test_cfg(serde_json::json!({
            "specType": "draft-mtp",
            "specDraftModel": "/mnt/should-ignore.gguf"
        }));
        let cmd2 = build_command(&cfg2).unwrap();
        assert!(!cmd2.contains("--spec-draft-model"), "cmd: {cmd2}");
        assert!(cmd2.contains("--spec-type draft-mtp"), "cmd: {cmd2}");
    }

    fn test_cfg_fw(framework: &str, params: serde_json::Value) -> InstanceConfig {
        let mut c = test_cfg(params);
        c.framework = framework.into();
        c
    }

    #[test]
    fn build_command_fastllm_native() {
        // ftllm server <model> --port <port> + 常用参数；moe_device 比例组合需加引号
        let cfg = test_cfg_fw(
            "fastllm",
            serde_json::json!({
                "device": "cuda",
                "tp": "0,1",
                "gpuMemRatio": 0.85,
                "moeDevice": "{'cuda':1,'numa':8,'disk':1}",
                "kvCacheDtype": "fp8_e4m3",
                "maxContextLength": 131072,
                "ropeScaling": "yarn",
                "prefixCache": "true",
                "mtp": 4,
                "host": "0.0.0.0",
                "modelName": "local-model",
                "extraArgs": "--triton"
            }),
        );
        let cmd = build_command(&cfg).unwrap();
        assert!(
            cmd.starts_with("ftllm server '/mnt/m.gguf' --port 8080"),
            "cmd: {cmd}"
        );
        for flag in [
            "--device cuda",
            "--tp 0,1",
            "--gpu_mem_ratio 0.85",
            "--kv_cache_dtype fp8_e4m3",
            "--max_context_length 131072",
            "--rope_scaling 'yarn'",
            "--prefix_cache true",
            "--mtp 4",
            "--host 0.0.0.0",
            "--model_name local-model",
            "--triton",
        ] {
            assert!(cmd.contains(&flag), "缺少 {flag}；cmd: {cmd}");
        }
        // moe_device 比例组合含单引号，必须整体加 shell 引号（shq 把 ' 转成 '\''）
        let i = cmd.find("--moe_device ").unwrap();
        let seg = &cmd[i..i + 60];
        assert!(seg.starts_with("--moe_device '{"), "cmd: {cmd}");
        for w in ["cuda", "numa", "disk"] {
            assert!(seg.contains(w), "cmd: {cmd}");
        }
    }

    #[test]
    fn build_command_fastllm_docker_rejected() {
        // FastLLM 无官方 Docker 镜像，docker 模式直接报错
        let mut cfg = test_cfg_fw("fastllm", serde_json::json!({}));
        cfg.mode = "docker".into();
        assert!(build_command(&cfg).is_err());
    }

    #[test]
    fn start_script_resolves_bare_ftllm_bin_to_venv() {
        // ftllm 装在 3.12 venv，裸命令名回退到 venv 的 ftllm 二进制
        let mut cfg = test_cfg_fw("fastllm", serde_json::json!({}));
        cfg.mode = "native".into();
        let s = start_script(&cfg, &test_profile());
        assert!(s.contains("ftllm_bin=$(command -v ftllm"), "script: {s}");
        assert!(s.contains("~/RemoteLLM/frameworks/ftllm-venv/bin/ftllm"), "script: {s}");
        assert!(s.contains("nohup \"$ftllm_bin\" server"), "script: {s}");

        // bin 为绝对路径时不注入回退
        let mut cfg2 = test_cfg_fw("fastllm", serde_json::json!({ "bin": "/opt/venv/bin/ftllm" }));
        cfg2.mode = "native".into();
        let s2 = start_script(&cfg2, &test_profile());
        assert!(!s2.contains("ftllm_bin="), "script: {s2}");
        assert!(s2.contains("nohup /opt/venv/bin/ftllm server"), "script: {s2}");
    }

    #[test]
    fn build_command_vllm_flags_no_sampling() {
        // vllm serve 无 --temperature/--top-p/--top-k/--repetition-penalty/--max-tokens
        // 启动参数（采样是 OpenAI API 每请求参数），传了会 argparse 报错
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
            "--limit-concurrency 32",
        ] {
            assert!(cmd.contains(flag), "缺少 {flag}；cmd: {cmd}");
        }
        for bad in ["--temperature", "--top-p", "--top-k", "--repetition-penalty", "--max-tokens"] {
            assert!(!cmd.contains(bad), "vllm 不应含 {bad}；cmd: {cmd}");
        }
    }

    #[test]
    fn build_command_vllm_docker_no_sampling() {
        let mut cfg = test_cfg_fw(
            "vllm",
            serde_json::json!({ "tp": 1, "maxNumSeqs": 32, "temperature": 0.5 }),
        );
        cfg.mode = "docker".into();
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.starts_with("docker run -d"), "cmd: {cmd}");
        assert!(cmd.contains("--tensor-parallel-size 1"), "cmd: {cmd}");
        assert!(cmd.contains("--max-num-seqs 32"), "cmd: {cmd}");
        assert!(!cmd.contains("--temperature"), "cmd: {cmd}");
    }

    #[test]
    fn build_command_sglang_flags_no_sampling() {
        // sglang serve 无采样启动参数；maxNumSeqs 映射 --max-running-requests（非 --max-num-reqs）
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
            cmd.starts_with("sglang_py -m sglang.launch_server --model-path '/mnt/m.gguf' --port 8080 --tp 1"),
            "cmd: {cmd}"
        );
        for flag in [
            "--mem-fraction-static 0.8",
            "--context-length 16384",
            "--host 0.0.0.0",
            "--max-running-requests 128",
            "--chunked-prefill-size 8192",
            "--dtype float16",
            "--quantization awq",
            "--trust-remote-code",
        ] {
            assert!(cmd.contains(flag), "缺少 {flag}；cmd: {cmd}");
        }
        for bad in ["--temperature", "--top-p", "--top-k", "--repetition-penalty", "--max-tokens", "--max-num-reqs"] {
            assert!(!cmd.contains(bad), "sglang 不应含 {bad}；cmd: {cmd}");
        }
    }

    #[test]
    fn build_command_sglang_docker_no_sampling() {
        let mut cfg = test_cfg_fw(
            "sglang",
            serde_json::json!({ "tp": 2, "maxNumSeqs": 64, "topP": 0.9 }),
        );
        cfg.mode = "docker".into();
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.starts_with("docker run -d"), "cmd: {cmd}");
        assert!(cmd.contains("--tp 2"), "cmd: {cmd}");
        assert!(cmd.contains("--max-running-requests 64"), "cmd: {cmd}");
        assert!(!cmd.contains("--top-p"), "cmd: {cmd}");
    }

    #[test]
    fn build_command_llama_optin_params() {
        // 新参数 opt-in：填写才输出
        let cfg = test_cfg(serde_json::json!({
            "nPredict": 128,
            "splitMode": "row",
            "ropeScale": 2.0,
            "lora": "/mnt/lora/adapter.gguf",
            "kvOffload": false,
            "loadMode": "mlock",
            "jinja": false
        }));
        let cmd = build_command(&cfg).unwrap();
        for flag in [
            "-n 128",
            "--split-mode row",
            "--rope-scale 2",
            "--lora '/mnt/lora/adapter.gguf'",
            "--no-kv-offload",
            "--load-mode mlock",
            "--no-jinja",
        ] {
            assert!(cmd.contains(flag), "缺少 {flag}；cmd: {cmd}");
        }

        // 未填写不输出
        let cmd2 = build_command(&test_cfg(serde_json::json!({}))).unwrap();
        for bad in [
            "-n ",
            "--split-mode",
            "--rope-scale",
            "--lora",
            "--no-kv-offload",
            "--load-mode",
            "--no-jinja",
            "--mirostat",
            "--reasoning-format",
        ] {
            assert!(!cmd2.contains(bad), "不应含 {bad}；cmd: {cmd2}");
        }
    }

    #[test]
    fn build_command_llama_mlock_compat() {
        // 旧实例 mlock=true（无 loadMode）→ --load-mode mlock
        let cfg = test_cfg(serde_json::json!({ "mlock": true }));
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.contains("--load-mode mlock"), "cmd: {cmd}");
        assert!(!cmd.contains("--mlock"), "cmd: {cmd}");
    }

    #[test]
    fn build_command_vllm_new_params() {
        let cfg = test_cfg_fw(
            "vllm",
            serde_json::json!({
                "pp": 2,
                "dp": 3,
                "kvCacheDtype": "fp8_e5m2",
                "enablePrefixCaching": false,
                "host": "10.0.0.5",
                "toolCallParser": "qwen3_coder",
                "enableAutoToolChoice": true,
                "streamInterval": 5
            }),
        );
        let cmd = build_command(&cfg).unwrap();
        for flag in [
            "--pipeline-parallel-size 2",
            "--data-parallel-size 3",
            "--kv-cache-dtype fp8_e5m2",
            "--no-enable-prefix-caching",
            "--host 10.0.0.5",
            "--tool-call-parser qwen3_coder",
            "--enable-auto-tool-choice",
            "--stream-interval 5",
        ] {
            assert!(cmd.contains(flag), "缺少 {flag}；cmd: {cmd}");
        }

        // 1Cat 专属参数不应出现在普通 vLLM
        let cfg2 = test_cfg_fw(
            "vllm",
            serde_json::json!({
                "attentionBackend": "FLASH_ATTN_V100",
                "speculativeConfig": "{\"method\":\"dflash\"}"
            }),
        );
        let cmd2 = build_command(&cfg2).unwrap();
        assert!(!cmd2.contains("--attention-backend"), "cmd2: {cmd2}");
        assert!(!cmd2.contains("--speculative-config"), "cmd2: {cmd2}");
    }

    #[test]
    fn build_command_onecat_sm70_params() {
        let cfg = test_cfg_fw(
            "1cat-vllm",
            serde_json::json!({
                "attentionBackend": "FLASH_ATTN_V100",
                "gdnPrefillBackend": "flashqla_sm70",
                "speculativeConfig": "{\"method\":\"dflash\",\"model\":\"incoai/Qwen3.8-27B-DFlash2\"}",
                "kvCacheDtype": "fp8_e5m2"
            }),
        );
        let cmd = build_command(&cfg).unwrap();
        assert!(cmd.contains("--attention-backend FLASH_ATTN_V100"), "cmd: {cmd}");
        assert!(cmd.contains("--gdn-prefill-backend flashqla_sm70"), "cmd: {cmd}");
        assert!(
            cmd.contains(
                "--speculative-config '{\"method\":\"dflash\",\"model\":\"incoai/Qwen3.8-27B-DFlash2\"}'"
            ),
            "cmd: {cmd}"
        );
        assert!(cmd.contains("--kv-cache-dtype fp8_e5m2"), "cmd: {cmd}");
    }

    #[test]
    fn build_command_onecat_full_defaults() {
        // 1Cat 全套默认参数（Qwen3.8-27B-NVFP4 + DFlash2 示例，模型路径除外）
        let cfg = test_cfg_fw(
            "1cat-vllm",
            serde_json::json!({
                "servedModelName": "qwen3.8-27b-dflash2",
                "trustRemoteCode": true,
                "tp": 4,
                "attentionBackend": "FLASH_ATTN_V100",
                "kvCacheDtype": "fp8_e5m2",
                "maxModelLen": 262144,
                "gpuMemUtil": 0.8,
                "enableAutoToolChoice": true,
                "toolCallParser": "qwen3_coder",
                "reasoningParser": "qwen3",
                "chatTemplateKwargs": "{\"enable_thinking\":true}",
                "speculativeConfig": "{\"method\":\"dflash\",\"model\":\"incoai/Qwen3.8-27B-DFlash2\",\"revision\":\"dedf8df68adfb1afeaf7b7480c0a0243108177b4\",\"kv_cache_dtype\":\"auto\"}",
                "host": "0.0.0.0"
            }),
        );
        let cmd = build_command(&cfg).unwrap();
        for flag in [
            "vllm serve '/mnt/m.gguf'",
            "--port 8080",
            "--tensor-parallel-size 4",
            "--served-model-name qwen3.8-27b-dflash2",
            "--trust-remote-code",
            "--attention-backend FLASH_ATTN_V100",
            "--kv-cache-dtype fp8_e5m2",
            "--max-model-len 262144",
            "--gpu-memory-utilization 0.8",
            "--enable-auto-tool-choice",
            "--tool-call-parser qwen3_coder",
            "--reasoning-parser qwen3",
            "--default-chat-template-kwargs '{\"enable_thinking\":true}'",
            "--host 0.0.0.0",
        ] {
            assert!(cmd.contains(flag), "缺少 {flag}；cmd: {cmd}");
        }
        assert!(
            cmd.contains(
                "--speculative-config '{\"method\":\"dflash\",\"model\":\"incoai/Qwen3.8-27B-DFlash2\",\"revision\":\"dedf8df68adfb1afeaf7b7480c0a0243108177b4\",\"kv_cache_dtype\":\"auto\"}'"
            ),
            "cmd: {cmd}"
        );
    }

    #[test]
    fn build_command_sglang_new_params() {
        let cfg = test_cfg_fw(
            "sglang",
            serde_json::json!({
                "maxTotalTokens": 100000,
                "schedulePolicy": "fcfs",
                "disableRadixCache": true,
                "enableMetrics": true,
                "speculativeAlgorithm": "MTP",
                "speculativeNumDraftTokens": 4,
                "randomSeed": 42
            }),
        );
        let cmd = build_command(&cfg).unwrap();
        for flag in [
            "--max-total-tokens 100000",
            "--schedule-policy fcfs",
            "--disable-radix-cache",
            "--enable-metrics",
            "--speculative-algorithm MTP",
            "--speculative-num-draft-tokens 4",
            "--random-seed 42",
        ] {
            assert!(cmd.contains(flag), "缺少 {flag}；cmd: {cmd}");
        }
        // 未填写不输出
        let cmd2 = build_command(&test_cfg_fw("sglang", serde_json::json!({}))).unwrap();
        for bad in [
            "--max-total-tokens",
            "--schedule-policy",
            "--disable-radix-cache",
            "--enable-metrics",
            "--speculative-algorithm",
            "--random-seed",
            "--kv-cache-dtype",
        ] {
            assert!(!cmd2.contains(bad), "不应含 {bad}；cmd: {cmd2}");
        }
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

    #[test]
    fn start_script_resolves_bare_vllm_bin_to_venv() {
        // vLLM 装在 3.12 venv，裸命令名回退到 venv 的 vllm 二进制
        let mut cfg = test_cfg_fw("vllm", serde_json::json!({}));
        cfg.mode = "native".into();
        let s = start_script(&cfg, &test_profile());
        assert!(s.contains("vllm_bin=$(command -v vllm"), "script: {s}");
        assert!(s.contains("~/RemoteLLM/frameworks/vllm-venv/bin/vllm"), "script: {s}");
        assert!(s.contains("nohup \"$vllm_bin\" serve"), "script: {s}");

        // bin 含路径时不注入解析
        let mut cfg2 = test_cfg_fw("vllm", serde_json::json!({ "bin": "/opt/venv/bin/vllm" }));
        cfg2.mode = "native".into();
        let s2 = start_script(&cfg2, &test_profile());
        assert!(!s2.contains("vllm_bin="), "script: {s2}");
        assert!(s2.contains("nohup /opt/venv/bin/vllm serve"), "script: {s2}");
    }

    #[test]
    fn start_script_resolves_bare_onecat_bin_to_venv() {
        // 1Cat-vLLM 装在 3.12 venv，裸命令名回退到 venv 的 vllm 二进制
        let mut cfg = test_cfg_fw("1cat-vllm", serde_json::json!({}));
        cfg.mode = "native".into();
        let s = start_script(&cfg, &test_profile());
        assert!(s.contains("cat_bin=$(command -v vllm"), "script: {s}");
        assert!(s.contains("~/RemoteLLM/frameworks/1cat-venv/bin/vllm"), "script: {s}");
        assert!(s.contains("nohup \"$cat_bin\" serve"), "script: {s}");

        // bin 含路径时不注入解析
        let mut cfg2 = test_cfg_fw("1cat-vllm", serde_json::json!({ "bin": "/opt/venv/bin/vllm" }));
        cfg2.mode = "native".into();
        let s2 = start_script(&cfg2, &test_profile());
        assert!(!s2.contains("cat_bin="), "script: {s2}");
        assert!(s2.contains("nohup /opt/venv/bin/vllm serve"), "script: {s2}");
    }

    #[test]
    fn start_script_resolves_sglang_py_to_venv() {
        // sglang 装在 3.12 venv（系统 python3 是 3.14，import 即崩），
        // 命令以 sglang_py 开头时回退到 venv 的 python
        let mut cfg = test_cfg_fw("sglang", serde_json::json!({}));
        cfg.mode = "native".into();
        let s = start_script(&cfg, &test_profile());
        assert!(s.contains("sglang_py=$(command -v python3"), "script: {s}");
        assert!(s.contains("~/RemoteLLM/frameworks/sglang-venv/bin/python"), "script: {s}");
        assert!(s.contains("nohup \"$sglang_py\" -m sglang.launch_server"), "script: {s}");
    }

    #[test]
    fn start_script_port_busy_check_native() {
        // native 模式：启动前探测端口占用，被占则 PORT_BUSY 退出
        let cfg = test_cfg(serde_json::json!({}));
        let s = start_script(&cfg, &test_profile());
        assert!(s.contains("PORT_BUSY 8080"), "script: {s}");
        assert!(s.contains("ss -lnt"), "script: {s}");
        // 端口探测在 nohup 启动之前
        assert!(
            s.find("PORT_BUSY").unwrap() < s.find("nohup").unwrap(),
            "端口探测应在启动之前: {s}"
        );
    }

    #[test]
    fn start_script_port_busy_check_docker() {
        // docker 模式：同样先探测端口
        let mut cfg = test_cfg(serde_json::json!({}));
        cfg.mode = "docker".into();
        let s = start_script(&cfg, &test_profile());
        assert!(s.contains("PORT_BUSY 8080"), "script: {s}");
        assert!(
            s.find("PORT_BUSY").unwrap() < s.find("docker run").unwrap(),
            "端口探测应在 docker run 之前: {s}"
        );
    }

    #[test]
    fn port_busy_check_uses_custom_port() {
        let mut cfg = test_cfg(serde_json::json!({}));
        cfg.port = 9999;
        let s = start_script(&cfg, &test_profile());
        assert!(s.contains("PORT_BUSY 9999"), "script: {s}");
        assert!(s.contains("dev/tcp/127.0.0.1/9999"), "script: {s}");
    }
}
