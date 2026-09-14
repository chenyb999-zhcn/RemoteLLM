import type { MetricSample } from "./types";

export interface DerivedRates {
  /** 生成速率（decode，tok/s） */
  decode: number | null;
  /** Prefill 速率（tok/s）；vLLM 为墙钟近似值 */
  prefill: number | null;
}

function byName(samples: MetricSample[] | null, name: string): number | null {
  if (!samples) return null;
  const hit = samples.find((s) => s.name === name);
  return hit && Number.isFinite(hit.value) ? hit.value : null;
}

/**
 * 由相邻两次 /metrics 快照推导 token 速率。
 *
 * - llama.cpp：精确——分母用指标自带的时间计数器（轮询丢拍/间隔抖动不影响）
 *   decode  = Δtokens_predicted_total / Δtokens_predicted_seconds_total
 *   prefill = Δprompt_tokens_total / Δprompt_tokens_seconds
 * - vLLM / 1Cat：decode = 1 / avg(TPOT)（直方图 Δsum/Δcount）；
 *   prefill ≈ Δprompt_tokens_total / Δ墙钟（近似，仅 prefill 发生时才有意义）
 * - SGLang：decode 直接读 gen_throughput gauge（本身即 tok/s）
 *
 * 计数器回退（服务重启）→ 该路返回 null，不产生错误数据点。
 * prev 为 null（首次采样）时无法差分 → 返回 null（SGLang gauge 除外）。
 */
export function deriveRates(
  prev: MetricSample[] | null,
  curr: MetricSample[] | null,
  dtMs?: number
): DerivedRates {
  if (!curr) return { decode: null, prefill: null };
  const names = new Set(curr.map((s) => s.name));

  // ---- SGLang：gauge 直读 ----
  if (names.has("sglang:gen_throughput")) {
    return { decode: byName(curr, "sglang:gen_throughput"), prefill: null };
  }

  // ---- llama.cpp：精确计数器差分 ----
  if (names.has("llamacpp:tokens_predicted_total")) {
    const t1 = byName(curr, "llamacpp:tokens_predicted_total");
    const s1 = byName(curr, "llamacpp:tokens_predicted_seconds_total");
    const t0 = prev ? byName(prev, "llamacpp:tokens_predicted_total") : null;
    const s0 = prev ? byName(prev, "llamacpp:tokens_predicted_seconds_total") : null;
    let decode: number | null = null;
    if (t1 != null && s1 != null && t0 != null && s0 != null && s1 > s0 && t1 >= t0) {
      const dTpot = (s1 - s0) / (t1 - t0);
      if (dTpot > 1e-9) decode = 1 / dTpot;
    }

    const p1 = byName(curr, "llamacpp:prompt_tokens_total");
    const ps1 =
      byName(curr, "llamacpp:prompt_tokens_seconds") ??
      byName(curr, "llamacpp:prompt_seconds_total");
    const p0 = prev ? byName(prev, "llamacpp:prompt_tokens_total") : null;
    const ps0 = prev
      ? byName(prev, "llamacpp:prompt_tokens_seconds") ??
        byName(prev, "llamacpp:prompt_seconds_total")
      : null;
    let prefill: number | null = null;
    if (p1 != null && ps1 != null && p0 != null && ps0 != null && ps1 > ps0 && p1 > p0) {
      prefill = (p1 - p0) / (ps1 - ps0);
    }
    return { decode, prefill };
  }

  // ---- vLLM / 1Cat：TPOT 直方图 + 墙钟近似 ----
  if (names.has("vllm:time_per_output_token_seconds_count")) {
    const sum1 = byName(curr, "vllm:time_per_output_token_seconds_sum");
    const cnt1 = byName(curr, "vllm:time_per_output_token_seconds_count");
    const sum0 = prev ? byName(prev, "vllm:time_per_output_token_seconds_sum") : null;
    const cnt0 = prev ? byName(prev, "vllm:time_per_output_token_seconds_count") : null;
    let decode: number | null = null;
    if (sum1 != null && cnt1 != null && sum0 != null && cnt0 != null && cnt1 > cnt0 && sum1 >= sum0) {
      const avgTpot = (sum1 - sum0) / (cnt1 - cnt0);
      if (avgTpot > 1e-9) decode = 1 / avgTpot;
    }

    let prefill: number | null = null;
    const pt1 = byName(curr, "vllm:prompt_tokens_total");
    const pt0 = prev ? byName(prev, "vllm:prompt_tokens_total") : null;
    if (pt1 != null && pt0 != null && pt1 > pt0 && dtMs && dtMs > 0) {
      prefill = (pt1 - pt0) / (dtMs / 1000);
    }
    return { decode, prefill };
  }

  return { decode: null, prefill: null };
}

/** 紧凑格式化速率：1234 → "1.2k"，45.67 → "45.7" */
export function fmtTps(v: number | null): string {
  if (v == null || !Number.isFinite(v) || v <= 0) return "—";
  if (v >= 1000) return (v / 1000).toFixed(1) + "k";
  return v.toFixed(1);
}
