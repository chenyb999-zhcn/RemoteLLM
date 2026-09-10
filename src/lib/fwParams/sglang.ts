import type { ParamDef, FwTab } from "../fwParams";

export const sglangTabs: FwTab[] = [
  { key: "basic", label: "基本", labelEn: "Basic" },
  { key: "sched", label: "内存与调度", labelEn: "Memory & Scheduling" },
  { key: "par", label: "并行", labelEn: "Parallelism" },
  { key: "server", label: "服务与 API", labelEn: "Server & API" },
  { key: "spec", label: "投机解码", labelEn: "Speculative Decoding" },
];

export const sglangParams: ParamDef[] = [
  // —— 基本 / Basic ——
  { key: "dtype", label: "数据类型", labelEn: "Data type", type: "select", options: ["auto", "bfloat16", "float16", "half"], default: "auto", tab: "basic", flag: "--dtype", desc: "模型权重数据类型", descEn: "Model weight data type" },
  { key: "contextLength", label: "上下文长度", labelEn: "Context length", type: "number", min: 1, tab: "basic", flag: "--context-length", placeholder: "默认取模型自带", placeholderEn: "Default: model value", desc: "最大上下文长度", descEn: "Max context length" },
  { key: "quantization", label: "量化", labelEn: "Quantization", type: "select", options: ["awq", "fp8", "mxfp8", "gptq", "marlin", "gptq_marlin", "awq_marlin", "bitsandbytes", "gguf", "modelopt", "modelopt_fp8"], tab: "basic", flag: "--quantization", placeholder: "默认 auto", placeholderEn: "Default auto", desc: "量化方式", descEn: "Quantization method" },
  { key: "loadFormat", label: "加载格式", labelEn: "Load format", type: "select", options: ["auto", "safetensors", "gguf", "pt", "dummy", "sharded_state", "bitsandbytes", "npcache"], tab: "basic", flag: "--load-format", placeholder: "默认 auto", placeholderEn: "Default auto", desc: "模型权重加载格式", descEn: "Model weight load format" },
  { key: "kvCacheDtype", label: "KV 缓存类型", labelEn: "KV cache dtype", type: "select", options: ["auto", "fp8_e5m2", "fp8_e4m3", "mxfp8", "bfloat16", "float16", "nvfp4"], tab: "basic", flag: "--kv-cache-dtype", placeholder: "默认 auto", placeholderEn: "Default auto", desc: "KV 缓存数据类型", descEn: "KV cache data type" },
  { key: "revision", label: "模型版本", labelEn: "Revision", type: "text", tab: "basic", flag: "--revision", desc: "模型分支/commit", descEn: "Model branch/commit" },
  { key: "trustRemoteCode", label: "trust-remote-code", type: "switch", default: false, tab: "basic", flag: "--trust-remote-code", desc: "允许执行模型自定义代码", descEn: "Allow executing model custom code" },

  // —— 内存与调度 / Memory & Scheduling ——
  { key: "memFractionStatic", label: "静态显存比例", labelEn: "Static VRAM fraction", type: "number", default: 0.85, step: 0.05, min: 0, tab: "sched", flag: "--mem-fraction-static", desc: "为权重 + KV 缓存预分配的显存比例", descEn: "VRAM fraction preallocated for weights + KV cache" },
  { key: "maxNumSeqs", label: "最大并发请求", labelEn: "Max concurrent reqs", type: "number", min: 1, tab: "sched", flag: "--max-running-requests", placeholder: "默认自动", placeholderEn: "Default: auto", desc: "最大同时运行请求数", descEn: "Max running requests" },
  { key: "maxTotalTokens", label: "最大总 token", labelEn: "Max total tokens", type: "number", min: 1, tab: "sched", flag: "--max-total-tokens", placeholder: "默认自动", placeholderEn: "Default: auto", desc: "token 池最大容量", descEn: "Token pool max capacity" },
  { key: "chunkedPrefillSize", label: "分块预填充", labelEn: "Chunked prefill", type: "number", min: 1, tab: "sched", flag: "--chunked-prefill-size", placeholder: "默认 8192", placeholderEn: "Default 8192", desc: "预填充分块大小（token）", descEn: "Prefill chunk size (tokens)" },
  { key: "maxPrefillTokens", label: "最大预填充 token", labelEn: "Max prefill tokens", type: "number", min: 1, tab: "sched", flag: "--max-prefill-tokens", placeholder: "默认自动", placeholderEn: "Default: auto", desc: "每批最大预填充 token 总量", descEn: "Max total prefill tokens per batch" },
  { key: "schedulePolicy", label: "调度策略", labelEn: "Schedule policy", type: "select", options: ["lpm", "fcfs", "dfs-weight", "random", "lof"], tab: "sched", flag: "--schedule-policy", placeholder: "默认 lpm", placeholderEn: "Default lpm", desc: "请求调度策略；lpm = 最长前缀匹配（对前缀缓存友好）", descEn: "Request scheduling policy; lpm = longest prefix match (prefix-cache friendly)" },
  { key: "scheduleConservativeness", label: "调度保守度", labelEn: "Sched. conservativeness", type: "number", step: 0.1, min: 0, tab: "sched", flag: "--schedule-conservativeness", placeholder: "默认 1.0", placeholderEn: "Default 1.0", desc: "调度保守度；大于 1 更保守（少收新请求，降低抢占）", descEn: "Scheduling conservativeness; >1 is more conservative (fewer new requests, less preemption)" },
  { key: "pageSize", label: "页大小", labelEn: "Page size", type: "number", min: 1, tab: "sched", flag: "--page-size", placeholder: "默认 1", placeholderEn: "Default 1", desc: "分页 KV 缓存每页 token 数", descEn: "Tokens per paged KV cache page" },
  { key: "disableRadixCache", label: "禁用 radix 缓存", labelEn: "Disable radix cache", type: "switch", tab: "sched", flag: "--disable-radix-cache", desc: "禁用 radix 树前缀缓存（默认启用）", descEn: "Disable radix-tree prefix caching (default on)" },

  // —— 并行 / Parallelism ——
  { key: "tp", label: "张量并行 TP", labelEn: "Tensor parallel TP", type: "number", default: 1, min: 1, tab: "par", flag: "--tp", desc: "张量并行（模型切分到几张卡）", descEn: "Tensor parallelism (across how many GPUs)" },
  { key: "pp", label: "流水并行 PP", labelEn: "Pipeline parallel PP", type: "number", min: 1, tab: "par", flag: "--pp", placeholder: "默认 1", placeholderEn: "Default 1", desc: "流水线并行", descEn: "Pipeline parallelism" },
  { key: "dp", label: "数据并行 DP", labelEn: "Data parallel DP", type: "number", min: 1, tab: "par", flag: "--dp", placeholder: "默认 1", placeholderEn: "Default 1", desc: "数据并行（模型副本数）", descEn: "Data parallelism (model replicas)" },
  { key: "enableDpAttention", label: "DP attention", type: "switch", tab: "par", flag: "--enable-dp-attention", desc: "启用数据并行注意力（MoE 模型）", descEn: "Enable data-parallel attention (MoE models)" },
  { key: "device", label: "设备", labelEn: "Device", type: "text", tab: "par", flag: "--device", placeholder: "默认 cuda", placeholderEn: "Default cuda", desc: "使用设备（cuda/cpu/…）", descEn: "Device to use (cuda/cpu/…)" },
  { key: "baseGpuId", label: "起始 GPU 号", labelEn: "Base GPU id", type: "number", min: 0, tab: "par", flag: "--base-gpu-id", placeholder: "默认 0", placeholderEn: "Default 0", desc: "使用的第一个 GPU 索引", descEn: "Index of the first GPU used" },
  { key: "gpuIdStep", label: "GPU 号步长", labelEn: "GPU id step", type: "number", min: 1, tab: "par", flag: "--gpu-id-step", placeholder: "默认 1", placeholderEn: "Default 1", desc: "相邻 GPU 索引步长", descEn: "Step between adjacent GPU indices" },
  { key: "randomSeed", label: "随机种子", labelEn: "Seed", type: "number", min: -1, tab: "par", flag: "--random-seed", desc: "随机种子", descEn: "Random seed" },

  // —— 服务与 API / Server & API ——
  { key: "host", label: "监听地址", labelEn: "Host", type: "text", default: "0.0.0.0", tab: "server", flag: "--host", desc: "监听 IP（官方默认 127.0.0.1，此处默认 0.0.0.0 便于远程访问）", descEn: "Listen IP (official default 127.0.0.1, here 0.0.0.0 for remote access)" },
  { key: "servedModelName", label: "服务模型名", labelEn: "Served model name", type: "text", tab: "server", flag: "--served-model-name", desc: "API 对外暴露的模型名", descEn: "Model name exposed by the API" },
  { key: "apiKey", label: "API 密钥", labelEn: "API key", type: "text", tab: "server", flag: "--api-key", desc: "API 认证密钥", descEn: "API auth key" },
  { key: "chatTemplate", label: "聊天模板", labelEn: "Chat template", type: "text", tab: "server", flag: "--chat-template", desc: "自定义聊天模板", descEn: "Custom chat template" },
  { key: "chatTemplateKwargs", label: "模板附加参数", labelEn: "Template kwargs", type: "text", tab: "server", flag: "--default-chat-template-kwargs", placeholder: "JSON", placeholderEn: "JSON", desc: "聊天模板默认参数（JSON 字符串）", descEn: "Chat template default kwargs (JSON string)" },
  { key: "enableMetrics", label: "Prometheus 指标", labelEn: "Prometheus metrics", type: "switch", tab: "server", flag: "--enable-metrics", desc: "启用 Prometheus 兼容 /metrics 端点（默认禁用）", descEn: "Enable the Prometheus-compatible /metrics endpoint (default off)" },
  { key: "logLevel", label: "日志级别", labelEn: "Log level", type: "select", options: ["info", "warning", "error", "debug"], tab: "server", flag: "--log-level", desc: "日志级别", descEn: "Log level" },
  { key: "logRequests", label: "记录请求", labelEn: "Log requests", type: "switch", tab: "server", flag: "--log-requests", desc: "记录所有请求的元数据、输入、输出", descEn: "Log metadata, inputs and outputs of all requests" },
  { key: "streamInterval", label: "流式间隔", labelEn: "Stream interval", type: "number", min: 1, tab: "server", flag: "--stream-interval", placeholder: "默认 1", placeholderEn: "Default 1", desc: "流式输出每次更新累积的新 token 数", descEn: "New accumulated tokens per streaming update" },
  { key: "skipServerWarmup", label: "跳过预热", labelEn: "Skip warmup", type: "switch", tab: "server", flag: "--skip-server-warmup", desc: "跳过启动时的预热运行（启动更快）", descEn: "Skip the warmup run at startup (faster start)" },

  // —— 投机解码 / Speculative Decoding ——
  { key: "speculativeAlgorithm", label: "投机算法", labelEn: "Spec algorithm", type: "text", tab: "spec", flag: "--speculative-algorithm", placeholder: "如 EAGLE / EAGLE3 / MTP / NGRAM", placeholderEn: "e.g. EAGLE / EAGLE3 / MTP / NGRAM", desc: "投机解码算法", descEn: "Speculative decoding algorithm" },
  { key: "speculativeDraftModel", label: "Draft 模型", labelEn: "Draft model", type: "text", tab: "spec", flag: "--speculative-draft-model-path", placeholder: "HF 仓库或本地路径", placeholderEn: "HF repo or local path", desc: "draft 模型路径（EAGLE/MTP/STANDALONE 等需要）", descEn: "Draft model path (needed by EAGLE/MTP/STANDALONE etc.)" },
  { key: "speculativeNumDraftTokens", label: "草稿 token 数", labelEn: "Draft tokens", type: "number", min: 1, tab: "spec", flag: "--speculative-num-draft-tokens", desc: "每轮草稿生成的 token 数", descEn: "Draft tokens generated per round" },
  { key: "speculativeNumSteps", label: "草稿步数", labelEn: "Draft steps", type: "number", min: 0, tab: "spec", flag: "--speculative-num-steps", desc: "多步草稿的步数", descEn: "Steps for multi-step drafting" },
  { key: "speculativeEagleTopk", label: "EAGLE Top-K", type: "number", min: 1, tab: "spec", flag: "--speculative-eagle-topk", desc: "EAGLE 分支 Top-K", descEn: "EAGLE branching Top-K" },
  { key: "speculativeAttentionMode", label: "草稿注意力模式", labelEn: "Draft attention mode", type: "select", options: ["prefill", "decode"], tab: "spec", flag: "--speculative-attention-mode", desc: "draft 模型注意力计算模式", descEn: "Attention mode for the draft model" },
];
