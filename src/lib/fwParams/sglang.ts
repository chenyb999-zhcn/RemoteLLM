import type { ParamDef, FwTab } from "../fwParams";

export const sglangTabs: FwTab[] = [
  { key: "basic", label: "基本" },
  { key: "sched", label: "内存与调度" },
  { key: "par", label: "并行" },
  { key: "server", label: "服务与 API" },
  { key: "spec", label: "投机解码" },
];

export const sglangParams: ParamDef[] = [
  // —— 基本 ——
  { key: "dtype", label: "数据类型", type: "select", options: ["auto", "bfloat16", "float16", "half"], default: "auto", tab: "basic", flag: "--dtype", desc: "模型权重数据类型" },
  { key: "contextLength", label: "上下文长度", type: "number", min: 1, tab: "basic", flag: "--context-length", placeholder: "默认取模型自带", desc: "最大上下文长度" },
  { key: "quantization", label: "量化", type: "select", options: ["awq", "fp8", "mxfp8", "gptq", "marlin", "gptq_marlin", "awq_marlin", "bitsandbytes", "gguf", "modelopt", "modelopt_fp8"], tab: "basic", flag: "--quantization", placeholder: "默认 auto", desc: "量化方式" },
  { key: "loadFormat", label: "加载格式", type: "select", options: ["auto", "safetensors", "gguf", "pt", "dummy", "sharded_state", "bitsandbytes", "npcache"], tab: "basic", flag: "--load-format", placeholder: "默认 auto", desc: "模型权重加载格式" },
  { key: "kvCacheDtype", label: "KV 缓存类型", type: "select", options: ["auto", "fp8_e5m2", "fp8_e4m3", "mxfp8", "bfloat16", "float16", "nvfp4"], tab: "basic", flag: "--kv-cache-dtype", placeholder: "默认 auto", desc: "KV 缓存数据类型" },
  { key: "revision", label: "模型版本", type: "text", tab: "basic", flag: "--revision", desc: "模型分支/commit" },
  { key: "trustRemoteCode", label: "trust-remote-code", type: "switch", default: false, tab: "basic", flag: "--trust-remote-code", desc: "允许执行模型自定义代码" },

  // —— 内存与调度 ——
  { key: "memFractionStatic", label: "静态显存比例", type: "number", default: 0.85, step: 0.05, min: 0, tab: "sched", flag: "--mem-fraction-static", desc: "为权重 + KV 缓存预分配的显存比例" },
  { key: "maxNumSeqs", label: "最大并发请求", type: "number", min: 1, tab: "sched", flag: "--max-running-requests", placeholder: "默认自动", desc: "最大同时运行请求数" },
  { key: "maxTotalTokens", label: "最大总 token", type: "number", min: 1, tab: "sched", flag: "--max-total-tokens", placeholder: "默认自动", desc: "token 池最大容量" },
  { key: "chunkedPrefillSize", label: "分块预填充", type: "number", min: 1, tab: "sched", flag: "--chunked-prefill-size", placeholder: "默认 8192", desc: "预填充分块大小（token）" },
  { key: "maxPrefillTokens", label: "最大预填充 token", type: "number", min: 1, tab: "sched", flag: "--max-prefill-tokens", placeholder: "默认自动", desc: "每批最大预填充 token 总量" },
  { key: "schedulePolicy", label: "调度策略", type: "select", options: ["lpm", "fcfs", "dfs-weight", "random", "lof"], tab: "sched", flag: "--schedule-policy", placeholder: "默认 lpm", desc: "请求调度策略；lpm = 最长前缀匹配（对前缀缓存友好）" },
  { key: "scheduleConservativeness", label: "调度保守度", type: "number", step: 0.1, min: 0, tab: "sched", flag: "--schedule-conservativeness", placeholder: "默认 1.0", desc: "调度保守度；大于 1 更保守（少收新请求，降低抢占）" },
  { key: "pageSize", label: "页大小", type: "number", min: 1, tab: "sched", flag: "--page-size", placeholder: "默认 1", desc: "分页 KV 缓存每页 token 数" },
  { key: "disableRadixCache", label: "禁用 radix 缓存", type: "switch", tab: "sched", flag: "--disable-radix-cache", desc: "禁用 radix 树前缀缓存（默认启用）" },

  // —— 并行 ——
  { key: "tp", label: "张量并行 TP", type: "number", default: 1, min: 1, tab: "par", flag: "--tp", desc: "张量并行（模型切分到几张卡）" },
  { key: "pp", label: "流水并行 PP", type: "number", min: 1, tab: "par", flag: "--pp", placeholder: "默认 1", desc: "流水线并行" },
  { key: "dp", label: "数据并行 DP", type: "number", min: 1, tab: "par", flag: "--dp", placeholder: "默认 1", desc: "数据并行（模型副本数）" },
  { key: "enableDpAttention", label: "DP attention", type: "switch", tab: "par", flag: "--enable-dp-attention", desc: "启用数据并行注意力（MoE 模型）" },
  { key: "device", label: "设备", type: "text", tab: "par", flag: "--device", placeholder: "默认 cuda", desc: "使用设备（cuda/cpu/…）" },
  { key: "baseGpuId", label: "起始 GPU 号", type: "number", min: 0, tab: "par", flag: "--base-gpu-id", placeholder: "默认 0", desc: "使用的第一个 GPU 索引" },
  { key: "gpuIdStep", label: "GPU 号步长", type: "number", min: 1, tab: "par", flag: "--gpu-id-step", placeholder: "默认 1", desc: "相邻 GPU 索引步长" },
  { key: "randomSeed", label: "随机种子", type: "number", min: -1, tab: "par", flag: "--random-seed", desc: "随机种子" },

  // —— 服务与 API ——
  { key: "host", label: "监听地址", type: "text", default: "0.0.0.0", tab: "server", flag: "--host", desc: "监听 IP（官方默认 127.0.0.1，此处默认 0.0.0.0 便于远程访问）" },
  { key: "servedModelName", label: "服务模型名", type: "text", tab: "server", flag: "--served-model-name", desc: "API 对外暴露的模型名" },
  { key: "apiKey", label: "API 密钥", type: "text", tab: "server", flag: "--api-key", desc: "API 认证密钥" },
  { key: "chatTemplate", label: "聊天模板", type: "text", tab: "server", flag: "--chat-template", desc: "自定义聊天模板" },
  { key: "chatTemplateKwargs", label: "模板附加参数", type: "text", tab: "server", flag: "--default-chat-template-kwargs", placeholder: "JSON", desc: "聊天模板默认参数（JSON 字符串）" },
  { key: "enableMetrics", label: "Prometheus 指标", type: "switch", tab: "server", flag: "--enable-metrics", desc: "启用 Prometheus 兼容 /metrics 端点（默认禁用）" },
  { key: "logLevel", label: "日志级别", type: "select", options: ["info", "warning", "error", "debug"], tab: "server", flag: "--log-level", desc: "日志级别" },
  { key: "logRequests", label: "记录请求", type: "switch", tab: "server", flag: "--log-requests", desc: "记录所有请求的元数据、输入、输出" },
  { key: "streamInterval", label: "流式间隔", type: "number", min: 1, tab: "server", flag: "--stream-interval", placeholder: "默认 1", desc: "流式输出每次更新累积的新 token 数" },
  { key: "skipServerWarmup", label: "跳过预热", type: "switch", tab: "server", flag: "--skip-server-warmup", desc: "跳过启动时的预热运行（启动更快）" },

  // —— 投机解码 ——
  { key: "speculativeAlgorithm", label: "投机算法", type: "text", tab: "spec", flag: "--speculative-algorithm", placeholder: "如 EAGLE / EAGLE3 / MTP / NGRAM", desc: "投机解码算法" },
  { key: "speculativeDraftModel", label: "Draft 模型", type: "text", tab: "spec", flag: "--speculative-draft-model-path", placeholder: "HF 仓库或本地路径", desc: "draft 模型路径（EAGLE/MTP/STANDALONE 等需要）" },
  { key: "speculativeNumDraftTokens", label: "草稿 token 数", type: "number", min: 1, tab: "spec", flag: "--speculative-num-draft-tokens", desc: "每轮草稿生成的 token 数" },
  { key: "speculativeNumSteps", label: "草稿步数", type: "number", min: 0, tab: "spec", flag: "--speculative-num-steps", desc: "多步草稿的步数" },
  { key: "speculativeEagleTopk", label: "EAGLE Top-K", type: "number", min: 1, tab: "spec", flag: "--speculative-eagle-topk", desc: "EAGLE 分支 Top-K" },
  { key: "speculativeAttentionMode", label: "草稿注意力模式", type: "select", options: ["prefill", "decode"], tab: "spec", flag: "--speculative-attention-mode", desc: "draft 模型注意力计算模式" },
];
