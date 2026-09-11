// 五框架实例参数定义入口
// 参数来源：
//   llama.cpp  —— 官方 tools/server/README.md 完整参数表
//   vLLM / 1Cat-vLLM —— v1.5 实际版本 `vllm serve --help`（ModelConfig/ParallelConfig/CacheConfig/SchedulerConfig/Frontend/AttentionConfig 配置组）
//   SGLang     —— 0.5.19 实际版本 `--help` 完整列表
//   FastLLM    —— https://github.com/ztxz16/fastllm README「常用参数」（ftllm server）
// 约定：无 default 的参数 = 留空不输出该标志（使用引擎自身默认值），placeholder 标注官方默认值。

import { llamaParams, llamaTabs } from "./fwParams/llama";
import { vllmParams, vllmTabs, onecatParams, onecatTabs } from "./fwParams/vllm";
import { sglangParams, sglangTabs } from "./fwParams/sglang";
import { fastllmParams, fastllmTabs } from "./fwParams/fastllm";

export interface ParamDef {
  key: string;
  label: string;
  /** 英文 label（缺省回退中文） */
  labelEn?: string;
  type: "text" | "number" | "switch" | "select";
  options?: string[];
  /** 动态下拉选项来源（如本地 GGUF 模型列表） */
  dynamicOptions?: "localGguf";
  default?: string | number | boolean;
  placeholder?: string;
  /** 英文 placeholder（缺省回退中文） */
  placeholderEn?: string;
  step?: number;
  min?: number;
  nativeOnly?: boolean;
  dockerOnly?: boolean;
  /** 对应 CLI 标志（展示用） */
  flag?: string;
  /** 中文说明（tooltip） */
  desc?: string;
  /** 英文说明（tooltip，缺省回退中文） */
  descEn?: string;
  /** 所属 tab */
  tab?: string;
}

export interface FwTab {
  key: string;
  label: string;
  /** 英文 tab 名（缺省回退中文） */
  labelEn?: string;
}

export interface FwMeta {
  label: string;
  /** 英文显示名（缺省回退中文） */
  labelEn?: string;
  desc: string;
  /** 英文描述（缺省回退中文） */
  descEn?: string;
  defaultPort: number;
  dockerImage: string;
  /** 仅原生模式（无官方 Docker 镜像），表单隐藏运行方式选择 */
  nativeOnly?: boolean;
  params: ParamDef[];
  tabs?: FwTab[];
}

export const FW_META: Record<string, FwMeta> = {
  vllm: {
    label: "vLLM",
    labelEn: "vLLM",
    desc: "高吞吐推理引擎（OpenAI 兼容 API）",
    descEn: "High-throughput inference engine (OpenAI-compatible API)",
    defaultPort: 8000,
    dockerImage: "vllm/vllm-openai:latest",
    params: vllmParams,
    tabs: vllmTabs,
  },
  "1cat-vllm": {
    label: "1Cat-vLLM",
    labelEn: "1Cat-vLLM",
    desc: "vLLM fork（含 sm70/V100 支持）",
    descEn: "vLLM fork (with sm70/V100 support)",
    defaultPort: 8000,
    dockerImage: "ghcr.io/chenyb999-zhcn/1cat-vllm:1.5",
    // 1Cat-vLLM 预编译 wheel 只装 `vllm` 入口（无 sglang/llama-server），bin 默认 vllm
    params: onecatParams,
    tabs: [...vllmTabs, ...onecatTabs],
  },
  sglang: {
    label: "SGLang",
    labelEn: "SGLang",
    desc: "面向结构化生成优化的推理框架",
    descEn: "Inference framework optimized for structured generation",
    defaultPort: 30000,
    dockerImage: "lmsysorg/sglang:latest-cu129",
    params: sglangParams,
    tabs: sglangTabs,
  },
  "llama-cpp": {
    label: "llama.cpp",
    labelEn: "llama.cpp",
    desc: "GGUF 量化模型推理（llama-server）",
    descEn: "GGUF quantized model inference (llama-server)",
    defaultPort: 8080,
    dockerImage: "ghcr.io/ggml-org/llama.cpp:server-cuda",
    params: llamaParams,
    tabs: llamaTabs,
  },
  fastllm: {
    label: "FastLLM",
    labelEn: "FastLLM",
    desc: "高性能推理引擎（C++ 无 PyTorch 依赖，稠密/MoE 混合推理，ftllm server）",
    descEn: "High-performance inference engine (torch-free C++, dense/MoE hybrid, ftllm server)",
    defaultPort: 8080,
    dockerImage: "",
    // FastLLM 暂无官方 Docker 镜像（仓库 Dockerfile 为源码构建的老版 webui），仅支持原生 pip 安装
    nativeOnly: true,
    params: fastllmParams,
    tabs: fastllmTabs,
  },
};
