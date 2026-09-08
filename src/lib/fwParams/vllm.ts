import type { ParamDef, FwTab } from "../fwParams";

export const vllmTabs: FwTab[] = [
  { key: "basic", label: "基本" },
  { key: "gpu", label: "并行与显存" },
  { key: "sched", label: "调度与吞吐" },
  { key: "load", label: "量化与加载" },
  { key: "server", label: "前端与 API" },
];

export const vllmParams: ParamDef[] = [
  // —— 基本 ——
  { key: "bin", label: "命令", type: "text", default: "vllm", nativeOnly: true, tab: "basic", desc: "vllm 可执行文件（1Cat 预编译 wheel 只装 vllm 入口；裸命令名自动回退到 3.12 venv 二进制）" },
  { key: "servedModelName", label: "服务模型名", type: "text", tab: "basic", flag: "--served-model-name", placeholder: "默认取目录名", desc: "API 对外暴露的模型名" },
  { key: "dtype", label: "数据类型", type: "select", options: ["auto", "bfloat16", "float16", "half"], default: "auto", tab: "basic", flag: "--dtype", desc: "模型权重数据类型" },
  { key: "maxModelLen", label: "最大上下文", type: "number", min: 1, tab: "basic", flag: "--max-model-len", placeholder: "默认自动", desc: "最大上下文长度（提示词 + 补全）" },
  { key: "seed", label: "随机种子", type: "number", min: -1, tab: "basic", flag: "--seed", placeholder: "默认 -1（随机）", desc: "复现性随机种子" },
  { key: "revision", label: "模型版本", type: "text", tab: "basic", flag: "--revision", desc: "Hugging Face 模型分支/commit" },
  { key: "hfToken", label: "HF Token", type: "text", tab: "basic", flag: "--hf-token", desc: "Hugging Face 访问令牌（私有仓库用；未设置时读 HF_TOKEN 环境变量）" },
  { key: "trustRemoteCode", label: "trust-remote-code", type: "switch", default: false, tab: "basic", flag: "--trust-remote-code", desc: "允许执行模型自定义代码（部分模型需要）" },

  // —— 并行与显存 ——
  { key: "tp", label: "张量并行 TP", type: "number", default: 1, min: 1, tab: "gpu", flag: "--tensor-parallel-size", desc: "张量并行（模型切分到几张卡）" },
  { key: "pp", label: "流水并行 PP", type: "number", min: 1, tab: "gpu", flag: "--pipeline-parallel-size", placeholder: "默认 1", desc: "流水线并行" },
  { key: "dp", label: "数据并行 DP", type: "number", min: 1, tab: "gpu", flag: "--data-parallel-size", placeholder: "默认 1", desc: "数据并行（模型副本数）" },
  { key: "gpuMemUtil", label: "显存利用率", type: "number", default: 0.9, step: 0.05, min: 0, tab: "gpu", flag: "--gpu-memory-utilization", desc: "GPU 显存使用比例（0–1）" },
  { key: "kvCacheDtype", label: "KV 缓存类型", type: "select", options: ["auto", "fp8", "fp8_e4m3", "fp8_e5m2", "bfloat16", "float16"], tab: "gpu", flag: "--kv-cache-dtype", placeholder: "默认 auto", desc: "KV 缓存数据类型；fp8 省显存（1Cat 在 V100 支持 fp8_e5m2/fp8_e4m3 路径）" },
  { key: "enablePrefixCaching", label: "前缀缓存", type: "switch", default: true, tab: "gpu", flag: "--no-enable-prefix-caching", desc: "自动前缀缓存（默认启用；关闭时输出 --no-enable-prefix-caching）" },
  { key: "blockSize", label: "KV 块大小", type: "number", min: 1, tab: "gpu", flag: "--block-size", placeholder: "默认 16", desc: "KV 缓存每块 token 数" },

  // —— 调度与吞吐 ——
  { key: "maxNumSeqs", label: "最大并发请求", type: "number", min: 1, tab: "sched", flag: "--max-num-seqs", placeholder: "默认自动", desc: "最大同时处理请求数" },
  { key: "maxNumBatchedTokens", label: "批处理最大 token", type: "number", min: 1, tab: "sched", flag: "--max-num-batched-tokens", placeholder: "默认自动", desc: "每个调度步最大批处理 token 数" },
  { key: "enableChunkedPrefill", label: "分块预填充", type: "switch", default: true, tab: "sched", flag: "--no-enable-chunked-prefill", desc: "chunked prefill（默认启用；关闭时输出 --no-enable-chunked-prefill）" },
  { key: "streamInterval", label: "流式间隔", type: "number", min: 1, tab: "sched", flag: "--stream-interval", placeholder: "默认 1", desc: "流式输出每次更新累积的新 token 数" },
  { key: "asyncScheduling", label: "异步调度", type: "switch", tab: "sched", flag: "--async-scheduling", desc: "启用异步调度（实验性，降低 CPU-GPU 同步开销）" },
  { key: "enforceEager", label: "enforce-eager", type: "switch", default: false, tab: "sched", flag: "--enforce-eager", desc: "禁用 CUDA Graph 改用 eager 执行（调试用，正常保持关闭）" },

  // —— 量化与加载 ——
  { key: "quantization", label: "量化", type: "select", options: ["fp8", "gptq", "awq", "bitsandbytes"], tab: "load", flag: "--quantization", placeholder: "默认 auto", desc: "量化方式（需要显式声明的量化 checkpoint）" },
  { key: "loadFormat", label: "加载格式", type: "select", options: ["auto", "safetensors", "pt", "sharded_state", "gguf", "bitsandbytes", "dummy"], tab: "load", flag: "--load-format", placeholder: "默认 auto", desc: "模型权重加载格式" },
  { key: "hfOverrides", label: "HF 配置覆盖", type: "text", tab: "load", flag: "--hf-overrides", placeholder: 'JSON，如 {"max_position_embeddings":32768}', desc: "覆盖模型配置字段（JSON 字符串）" },

  // —— 前端与 API ——
  { key: "host", label: "监听地址", type: "text", tab: "server", flag: "--host", placeholder: "默认 0.0.0.0", desc: "监听 IP 地址" },
  { key: "apiKey", label: "API 密钥", type: "text", tab: "server", flag: "--api-key", desc: "API 认证密钥（可多个）" },
  { key: "chatTemplate", label: "聊天模板", type: "text", tab: "server", flag: "--chat-template", desc: "自定义聊天模板（Jinja）" },
  { key: "chatTemplateKwargs", label: "模板附加参数", type: "text", tab: "server", flag: "--default-chat-template-kwargs", placeholder: 'JSON，如 {"enable_thinking":true}', desc: "聊天模板默认参数（JSON 字符串，1Cat 示例用于开启思考）" },
  { key: "reasoningParser", label: "推理解析器", type: "text", tab: "server", flag: "--reasoning-parser", placeholder: "如 qwen3", desc: "推理/思考内容解析器" },
  { key: "toolCallParser", label: "工具调用解析器", type: "text", tab: "server", flag: "--tool-call-parser", placeholder: "如 qwen3_coder", desc: "函数调用（tool call）解析器" },
  { key: "enableAutoToolChoice", label: "自动工具选择", type: "switch", tab: "server", flag: "--enable-auto-tool-choice", desc: "启用自动工具选择（需配合 tool-call-parser）" },
  { key: "allowedOrigins", label: "CORS 来源", type: "text", tab: "server", flag: "--allowed-origins", placeholder: "逗号分隔，默认 *", desc: "CORS 允许的 Origin 列表" },
  { key: "uvicornLogLevel", label: "访问日志级别", type: "select", options: ["info", "warning", "error", "critical", "debug"], tab: "server", flag: "--uvicorn-log-level", desc: "uvicorn 访问日志级别" },
  { key: "disableLogStats", label: "禁用统计日志", type: "switch", tab: "server", flag: "--disable-log-stats", desc: "禁用周期性统计日志" },
  { key: "enableLogRequests", label: "记录请求", type: "switch", tab: "server", flag: "--enable-log-requests", desc: "记录所有请求详情（调试用）" },
];

export const onecatTabs: FwTab[] = [{ key: "sm70", label: "SM70 专属" }];

// 1Cat-vLLM 默认参数（Qwen3.8-27B-NVFP4 + DFlash2 投机解码示例，模型路径除外）：
// vllm serve <模型> --served-model-name qwen3.8-27b-dflash2 --trust-remote-code
//   --tensor-parallel-size 4 --attention-backend FLASH_ATTN_V100 --kv-cache-dtype fp8_e5m2
//   --max-model-len 262144 --gpu-memory-utilization 0.80 --enable-auto-tool-choice
//   --tool-call-parser qwen3_coder --reasoning-parser qwen3
//   --default-chat-template-kwargs '{"enable_thinking":true}'
//   --speculative-config '{"method":"dflash",...}' --host 0.0.0.0
const onecatDefaults: Record<string, string | number | boolean> = {
  servedModelName: "qwen3.8-27b-dflash2",
  trustRemoteCode: true,
  tp: 4,
  kvCacheDtype: "fp8_e5m2",
  maxModelLen: 262144,
  gpuMemUtil: 0.8,
  enableAutoToolChoice: true,
  toolCallParser: "qwen3_coder",
  reasoningParser: "qwen3",
  chatTemplateKwargs: '{"enable_thinking":true}',
  host: "0.0.0.0",
};

export const onecatParams: ParamDef[] = [
  // 共享 vLLM 参数应用 1Cat 默认值（仅覆盖 placeholder 为「默认 …」的提示，示例型 placeholder 保留）
  ...vllmParams.map((p) => {
    const d = onecatDefaults[p.key];
    if (d === undefined) return p;
    const placeholder = p.placeholder?.startsWith("默认") ? undefined : p.placeholder;
    return { ...p, default: d, placeholder };
  }),
  { key: "attentionBackend", label: "注意力后端", type: "text", tab: "sm70", flag: "--attention-backend", default: "FLASH_ATTN_V100", desc: "注意力计算后端；1Cat 为 V100 重建的 FLASH_ATTN_V100（17.9 → 约 60.8 TFLOP/s）" },
  { key: "gdnPrefillBackend", label: "GDN 预填充后端", type: "select", options: ["flashinfer", "triton", "cutedsl", "flashqla_sm70"], tab: "sm70", flag: "--gdn-prefill-backend", desc: "GDN（Gated DeltaNet）预填充路径后端；flashqla_sm70 为 1Cat V100 实现" },
  { key: "speculativeConfig", label: "投机解码配置", type: "text", tab: "sm70", flag: "--speculative-config", default: '{"method":"dflash","model":"incoai/Qwen3.8-27B-DFlash2","revision":"dedf8df68adfb1afeaf7b7480c0a0243108177b4","kv_cache_dtype":"auto"}', desc: "投机解码配置（JSON）：DFlash2/MTP 等；1Cat 自动解析 SM70 draft 几何与快速路径" },
  { key: "performanceMode", label: "性能模式", type: "text", tab: "sm70", flag: "--performance-mode", desc: "性能模式（并发与预填充策略）" },
];
