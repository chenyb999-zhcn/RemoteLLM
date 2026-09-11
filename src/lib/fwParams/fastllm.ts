import type { ParamDef, FwTab } from "../fwParams";

// FastLLM 参数来源：https://github.com/ztxz16/fastllm README「常用参数」
// 服务入口：ftllm server <model>（OpenAI 兼容 API，默认 0.0.0.0:8080）
// 约定：无 default 的参数 = 留空不输出该标志（使用引擎自身默认值），placeholder 标注官方默认值。

export const fastllmTabs: FwTab[] = [
  { key: "basic", label: "基本", labelEn: "Basic" },
  { key: "moe", label: "MoE 混合", labelEn: "MoE Hybrid" },
  { key: "vram", label: "显存与上下文", labelEn: "VRAM & Context" },
  { key: "spec", label: "解码与投机", labelEn: "Decoding & Speculative" },
  { key: "server", label: "服务", labelEn: "Server" },
];

export const fastllmParams: ParamDef[] = [
  // —— 基本 / Basic ——
  { key: "bin", label: "命令", labelEn: "Command", type: "text", default: "ftllm", nativeOnly: true, tab: "basic", desc: "ftllm 可执行文件路径；裸命令名会自动回退到一键安装目录（3.12 venv）", descEn: "Path to the ftllm executable; a bare command name falls back to the one-click install dir (3.12 venv)" },
  { key: "device", label: "主计算设备", labelEn: "Device", type: "text", tab: "basic", flag: "--device", placeholder: "默认 auto（cuda/cpu/numa）", placeholderEn: "Default auto (cuda/cpu/numa)", desc: "主计算设备，常用值 cuda、cpu、numa", descEn: "Main compute device; common values: cuda, cpu, numa" },
  { key: "tp", label: "张量并行", labelEn: "Tensor parallel", type: "text", tab: "basic", flag: "--tp", placeholder: "默认 auto（如 0,1 / 2 / auto）", placeholderEn: "Default auto (e.g. 0,1 / 2 / auto)", desc: "CUDA 张量并行设备：0,1 = 指定卡；2 = 前 2 张可见卡；auto = 自动", descEn: "CUDA tensor-parallel devices: 0,1 = explicit; 2 = first 2 visible GPUs; auto = automatic" },
  { key: "threads", label: "CPU 线程", labelEn: "CPU threads", type: "number", min: -1, tab: "basic", flag: "-t", placeholder: "默认 -1（自动）", placeholderEn: "Default -1 (auto)", desc: "CPU/NUMA 线程数；-1 = 自动选择", descEn: "CPU/NUMA threads; -1 = automatic" },
  { key: "gpuMemRatio", label: "显存比例", labelEn: "GPU mem ratio", type: "number", default: 0.9, step: 0.05, min: 0, tab: "basic", flag: "--gpu_mem_ratio", desc: "可用于模型与缓存的 GPU 显存比例（官方默认 0.9）", descEn: "Fraction of GPU VRAM for model & cache (official default 0.9)" },

  // —— MoE 混合 / MoE Hybrid ——
  { key: "moeDevice", label: "MoE 专家设备", labelEn: "MoE device", type: "text", tab: "moe", flag: "--moe_device", placeholder: "cpu/cuda/numa/disk 或 {'cuda':1,'numa':8,'disk':1}", placeholderEn: "cpu/cuda/numa/disk or {'cuda':1,'numa':8,'disk':1}", desc: "MoE 专家层设备，可单设备或按比例组合（磁盘依赖 SSD 随机读性能）", descEn: "Device for MoE expert layers; single device or ratio mix (disk depends on SSD random-read)" },
  { key: "moeDeviceLayers", label: "MoE 层数", labelEn: "MoE layers", type: "number", min: -1, tab: "moe", flag: "--moe_device_layers", placeholder: "默认 -1（全部）", placeholderEn: "Default -1 (all)", desc: "仅最后 N 个 MoE 层使用 moe_device；-1 = 全部", descEn: "Only the last N MoE layers use moe_device; -1 = all" },
  { key: "moeDtype", label: "MoE 权重类型", labelEn: "MoE dtype", type: "text", tab: "moe", flag: "--moe_dtype", placeholder: "默认 auto", placeholderEn: "Default auto", desc: "单独设置 MoE 权重类型", descEn: "Separate weight dtype for MoE layers" },
  { key: "atype", label: "激活类型", labelEn: "Activation type", type: "text", tab: "moe", flag: "--atype", placeholder: "默认 auto", placeholderEn: "Default auto", desc: "普通层激活类型", descEn: "Activation type for dense layers" },
  { key: "moeAtype", label: "MoE 激活类型", labelEn: "MoE activation type", type: "text", tab: "moe", flag: "--moe_atype", placeholder: "默认 auto", placeholderEn: "Default auto", desc: "MoE 层激活类型", descEn: "Activation type for MoE layers" },
  { key: "moeCudaCache", label: "GPU 专家缓存", labelEn: "GPU expert cache", type: "text", tab: "moe", flag: "--moe_cuda_cache", placeholder: "如 3g；0 = 关闭（默认）", placeholderEn: "e.g. 3g; 0 = off (default)", desc: "GPU 专家权重缓存预算（如 3g = 3 GiB）；配合 CUDA 主计算 + CPU/NUMA 专家使用", descEn: "GPU expert weight cache budget (e.g. 3g = 3 GiB); use with CUDA compute + CPU/NUMA experts" },
  { key: "ngramDevice", label: "PLE/N-gram 存储", labelEn: "PLE/n-gram device", type: "select", options: ["cpu", "disk"], tab: "moe", flag: "--ngram_device", placeholder: "默认 cpu", placeholderEn: "Default cpu", desc: "Qwen4 PLE 表放在 cpu 或 disk（磁盘模式省内存、增随机 I/O，建议高速 SSD）", descEn: "Store Qwen4 PLE tables in cpu or disk (disk saves RAM, adds random I/O; fast SSD recommended)" },

  // —— 显存与上下文 / VRAM & Context ——
  { key: "dtype", label: "权重类型", labelEn: "Weight dtype", type: "text", tab: "vram", flag: "--dtype", placeholder: "默认 auto（已量化模型勿覆盖）", placeholderEn: "Default auto (do not override quantized models)", desc: "加载 HF 权重时的权重类型；默认 auto，已量化模型通常不应覆盖", descEn: "Weight dtype when loading HF weights; default auto, usually should not override quantized models" },
  { key: "kvCacheDtype", label: "KV 缓存类型", labelEn: "KV cache dtype", type: "select", options: ["auto", "float16", "bfloat16", "fp8_e4m3", "fp4"], tab: "vram", flag: "--kv_cache_dtype", placeholder: "默认 auto", placeholderEn: "Default auto", desc: "KV Cache 类型，需模型与后端支持", descEn: "KV cache dtype; must be supported by model and backend" },
  { key: "tokens", label: "KV 池 token 数", labelEn: "KV pool tokens", type: "number", tab: "vram", flag: "--tokens", placeholder: "默认自动预算", placeholderEn: "Default auto budget", desc: "所有会话共享的 KV 池容量（Paged KV Cache）", descEn: "Shared KV pool capacity across sessions (Paged KV Cache)" },
  { key: "pageSize", label: "KV 页大小", labelEn: "KV page size", type: "number", tab: "vram", flag: "--page_size", placeholder: "后端决定（多卡默认 16）", placeholderEn: "Backend decides (default 16 for multi-GPU)", desc: "Paged KV Cache 每页 token 数", descEn: "Tokens per Paged KV Cache page" },
  { key: "maxBatch", label: "最大批次数", labelEn: "Max batch", type: "number", tab: "vram", flag: "--max_batch", placeholder: "默认自动", placeholderEn: "Default auto", desc: "每轮最多同时推理的请求数", descEn: "Max concurrent inferences per round" },
  { key: "maxContextLength", label: "最大上下文", labelEn: "Max context length", type: "number", tab: "vram", flag: "--max_context_length", desc: "单会话输入与输出合计上限；扩大模型声明窗口时还需有效的 rope_scaling", descEn: "Per-session input+output cap; extending beyond the declared window also needs a valid rope_scaling" },
  { key: "ropeScaling", label: "RoPE 扩展", labelEn: "RoPE scaling", type: "text", tab: "vram", flag: "--rope_scaling", placeholder: "yarn 或 JSON（默认沿用模型配置）", placeholderEn: "yarn or JSON (default: model config)", desc: "RoPE 上下文扩展，接受 yarn 或 JSON（需模型布局已适配）", descEn: "RoPE context extension; accepts yarn or JSON (needs an adapted model layout)" },
  { key: "chunkedPrefillSize", label: "分块 Prefill", labelEn: "Chunked prefill", type: "number", tab: "vram", flag: "--chunked_prefill_size", placeholder: "默认关闭/模型决定（如 8192）", placeholderEn: "Default off/model (e.g. 8192)", desc: "分块 Prefill 的切片大小", descEn: "Chunk size for chunked prefill" },
  { key: "prefixCache", label: "前缀缓存", labelEn: "Prefix cache", type: "select", options: ["true", "false"], tab: "vram", flag: "--prefix_cache", placeholder: "默认模型/环境决定", placeholderEn: "Default: model/environment", desc: "是否开启前缀缓存", descEn: "Enable prefix caching" },
  { key: "cudaSlab", label: "CUDA 权重 slab", labelEn: "CUDA weight slab", type: "number", tab: "vram", flag: "--cuda_slab", placeholder: "默认 0（关闭，单位 MB）", placeholderEn: "Default 0 (off, in MB)", desc: "CUDA 权重 slab 大小（MB）；0 = 关闭", descEn: "CUDA weight slab size (MB); 0 = off" },

  // —— 解码与投机 / Decoding & Speculative ——
  { key: "mtp", label: "MTP 投机", labelEn: "MTP speculative", type: "number", min: 0, tab: "spec", flag: "--mtp", placeholder: "默认 0（关闭），最大 8", placeholderEn: "Default 0 (off), max 8", desc: "支持 MTP 的模型每轮生成的 draft token 数；0 = 关闭", descEn: "Draft tokens per round for MTP-capable models; 0 = off" },
  { key: "dspark", label: "DSpark 投机", labelEn: "DSpark speculative", type: "number", min: 1, tab: "spec", flag: "--dspark", placeholder: "每轮 draft token 数", placeholderEn: "Draft tokens per round", desc: "启用模型内置 DSpark 并设置每轮 draft token 数（需模型支持）", descEn: "Enable built-in DSpark with N draft tokens per round (model must support it)" },
  { key: "draft", label: "Draft 检查点", labelEn: "Draft checkpoint", type: "text", tab: "spec", flag: "--draft", placeholder: "外部 MTP/DSpark/DFlash2 checkpoint 路径", placeholderEn: "Path to an external MTP/DSpark/DFlash2 checkpoint", desc: "外部投机解码 checkpoint（自动识别 MTP/DFlash2/DSpark）；MTP 可直接指向 mtp.safetensors", descEn: "External speculative checkpoint (auto-detects MTP/DFlash2/DSpark); MTP can point straight at mtp.safetensors" },
  { key: "draftTokens", label: "Draft token 数", labelEn: "Draft tokens", type: "number", min: 1, tab: "spec", flag: "--draft_tokens", placeholder: "默认读取 draft 配置", placeholderEn: "Default: read from draft config", desc: "每轮最多使用的 draft token 数（DFlash2 不含 anchor token）", descEn: "Max draft tokens per round (DFlash2 excludes the anchor token)" },
  { key: "enableThinking", label: "思考模板", labelEn: "Thinking template", type: "select", options: ["true", "false"], tab: "spec", flag: "--enable_thinking", placeholder: "默认沿用模型（需模型支持）", placeholderEn: "Default: model (requires model support)", desc: "控制模型的思考模板开关", descEn: "Toggle the model's thinking template" },
  { key: "toolCallParser", label: "工具调用解析器", labelEn: "Tool call parser", type: "text", tab: "spec", flag: "--tool_call_parser", placeholder: "默认 auto", placeholderEn: "Default auto", desc: "工具调用解析器", descEn: "Tool-call parser" },
  { key: "chatTemplate", label: "聊天模板", labelEn: "Chat template", type: "text", tab: "spec", flag: "--chat_template", desc: "自定义 Jinja chat template 文件", descEn: "Custom Jinja chat template file" },

  // —— 服务 / Server ——
  { key: "host", label: "监听地址", labelEn: "Host", type: "text", default: "0.0.0.0", tab: "server", flag: "--host", desc: "监听 IP；官方默认 0.0.0.0", descEn: "Listen IP; official default 0.0.0.0" },
  { key: "modelName", label: "API 模型名", labelEn: "Model name", type: "text", tab: "server", flag: "--model_name", placeholder: "默认自动", placeholderEn: "Default auto", desc: "API 中校验和返回的部署名称", descEn: "Deployment name validated/returned by the API" },
  { key: "apiKey", label: "API 密钥", labelEn: "API key", type: "text", tab: "server", flag: "--api_key", desc: "非空时开启 Bearer API Key 校验", descEn: "Enables Bearer API key auth when non-empty" },
  { key: "temperature", label: "温度", labelEn: "Temperature", type: "number", step: 0.05, min: 0, tab: "server", flag: "--temperature", placeholder: "默认取模型默认", placeholderEn: "Default: model default", desc: "覆盖服务端默认采样温度", descEn: "Override the server-side default temperature" },
  { key: "topP", label: "Top-P", type: "number", step: 0.01, min: 0, tab: "server", flag: "--top_p", placeholder: "默认取模型默认", placeholderEn: "Default: model default", desc: "覆盖服务端默认 Top-P", descEn: "Override the server-side default top-p" },
  { key: "topK", label: "Top-K", type: "number", min: 0, tab: "server", flag: "--top_k", placeholder: "默认取模型默认", placeholderEn: "Default: model default", desc: "覆盖服务端默认 Top-K", descEn: "Override the server-side default top-k" },
  { key: "repeatPenalty", label: "重复惩罚", labelEn: "Repeat penalty", type: "number", step: 0.01, min: 0, tab: "server", flag: "--repeat_penalty", placeholder: "默认取模型默认", placeholderEn: "Default: model default", desc: "覆盖重复惩罚参数（也支持 --repetition_penalty）", descEn: "Override repetition penalty (also accepts --repetition_penalty)" },
  { key: "hideInput", label: "隐藏请求内容", labelEn: "Hide input", type: "switch", tab: "server", flag: "--hide_input", desc: "不在服务日志中显示请求内容", descEn: "Do not print request content in server logs" },
  { key: "startupProgress", label: "启动进度事件", labelEn: "Startup progress", type: "select", options: ["off", "ndjson"], tab: "server", flag: "--startup-progress", placeholder: "默认 off", placeholderEn: "Default off", desc: "设为 ndjson 时向 stderr 输出模型加载与就绪事件", descEn: "When set to ndjson, emit load/ready events to stderr" },
  { key: "extraArgs", label: "附加参数", labelEn: "Extra args", type: "text", tab: "server", placeholder: "追加原始参数，如 --triton", placeholderEn: "Append raw args, e.g. --triton", desc: "追加到命令末尾的原始命令行参数（用于以上未列出的参数）", descEn: "Raw CLI args appended at the end (for params not listed above)" },
];
