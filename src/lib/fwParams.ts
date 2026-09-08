// 四框架实例参数定义入口
// 参数来源：
//   llama.cpp  —— 官方 tools/server/README.md 完整参数表
//   vLLM / 1Cat-vLLM —— v1.5 实际版本 `vllm serve --help`（ModelConfig/ParallelConfig/CacheConfig/SchedulerConfig/Frontend/AttentionConfig 配置组）
//   SGLang     —— 0.5.19 实际版本 `--help` 完整列表
// 约定：无 default 的参数 = 留空不输出该标志（使用引擎自身默认值），placeholder 标注官方默认值。

import { llamaParams, llamaTabs } from "./fwParams/llama";
import { vllmParams, vllmTabs, onecatParams, onecatTabs } from "./fwParams/vllm";
import { sglangParams, sglangTabs } from "./fwParams/sglang";

export interface ParamDef {
  key: string;
  label: string;
  type: "text" | "number" | "switch" | "select";
  options?: string[];
  /** 动态下拉选项来源（如本地 GGUF 模型列表） */
  dynamicOptions?: "localGguf";
  default?: string | number | boolean;
  placeholder?: string;
  step?: number;
  min?: number;
  nativeOnly?: boolean;
  dockerOnly?: boolean;
  /** 对应 CLI 标志（展示用） */
  flag?: string;
  /** 中文说明（tooltip） */
  desc?: string;
  /** 所属 tab */
  tab?: string;
}

export interface FwTab {
  key: string;
  label: string;
}

export interface FwMeta {
  label: string;
  desc: string;
  defaultPort: number;
  dockerImage: string;
  params: ParamDef[];
  tabs?: FwTab[];
}

export const FW_META: Record<string, FwMeta> = {
  vllm: {
    label: "vLLM",
    desc: "高吞吐推理引擎（OpenAI 兼容 API）",
    defaultPort: 8000,
    dockerImage: "vllm/vllm-openai:latest",
    params: vllmParams,
    tabs: vllmTabs,
  },
  "1cat-vllm": {
    label: "1Cat-vLLM",
    desc: "vLLM fork（含 sm70/V100 支持）",
    defaultPort: 8000,
    dockerImage: "ghcr.io/chenyb999-zhcn/1cat-vllm:1.5",
    // 1Cat-vLLM 预编译 wheel 只装 `vllm` 入口（无 sglang/llama-server），bin 默认 vllm
    params: onecatParams,
    tabs: [...vllmTabs, ...onecatTabs],
  },
  sglang: {
    label: "SGLang",
    desc: "面向结构化生成优化的推理框架",
    defaultPort: 30000,
    dockerImage: "lmsysorg/sglang:latest-cu129",
    params: sglangParams,
    tabs: sglangTabs,
  },
  "llama-cpp": {
    label: "llama.cpp",
    desc: "GGUF 量化模型推理（llama-server）",
    defaultPort: 8080,
    dockerImage: "ghcr.io/ggml-org/llama.cpp:server-cuda",
    params: llamaParams,
    tabs: llamaTabs,
  },
};
