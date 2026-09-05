import { defineStore } from "pinia";
import { api } from "../lib/api";
import { useServerStore } from "./server";
import { useSettingsStore } from "./settings";
import type { GpuPoll, MetricSample, ProcRow } from "../lib/types";

const MAX_POINTS = 120;
const MAX_SELECTED = 8;

/** 指标样本的稳定 key：name{label=v,...} */
export function metricKey(m: MetricSample): string {
  const labels = m.labels.map(([k, v]) => `${k}="${v}"`).join(",");
  return labels ? `${m.name}{${labels}}` : m.name;
}

/** 自动勾选白名单（命中指标名即选，上限内取前几个） */
const AUTO_SELECT_RE =
  /tokens_seconds|active_requests|kv_cache|gpu_cache_usage|num_requests|requests_running|requests_waiting|cache_config|prompt_tokens_seconds|time_per_output_token|e2e_request_latency/;

function pollMs() {
  const v = useSettingsStore().value.pollIntervalMs;
  return Math.min(Math.max(v || 3000, 1000), 60000);
}

let timer: number | null = null;
let polling = false;

interface MetricsPoint {
  ts: number;
  values: Record<string, number>;
}

const SEL_KEY = "remotellm.metricsSelected";

function loadSelected(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(SEL_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export const useDashboardStore = defineStore("dashboard", {
  state: () => ({
    serverId: null as string | null,
    snaps: [] as GpuPoll[],
    procs: [] as ProcRow[],
    pollError: null as string | null,
    metricsPort: 8000,
    metrics: [] as MetricSample[],
    metricsLoading: false,
    metricsErr: null as string | null,
    /** 勾选上图的指标 key（持久化到 localStorage） */
    metricsSelected: loadSelected(),
    /** 自动刷新指标（跟随轮询间隔连续抓取） */
    metricsAuto: false,
    /** 指标历史（环形 MAX_POINTS 点） */
    metricsHistory: [] as MetricsPoint[],
  }),
  actions: {
    start() {
      const id = useServerStore().currentId;
      if (!id) return;
      if (this.serverId !== id) {
        this.serverId = id;
        this.snaps = [];
        this.procs = [];
        this.pollError = null;
        this.metrics = [];
        this.metricsErr = null;
        this.metricsHistory = [];
        this.initMetricsPort(id);
      }
      if (timer != null) window.clearInterval(timer);
      timer = window.setInterval(() => this.poll(), pollMs());
      this.poll();
    },
    stop() {
      this.serverId = null;
      if (timer != null) {
        window.clearInterval(timer);
        timer = null;
      }
      polling = false;
      this.snaps = [];
      this.procs = [];
      this.pollError = null;
      this.metrics = [];
      this.metricsErr = null;
      this.metricsHistory = [];
    },
    async poll() {
      if (polling) return;
      const id = useServerStore().currentId;
      if (!id) return;
      polling = true;
      try {
        const snap = await api.gpuPoll(id);
        this.snaps.push(snap);
        if (this.snaps.length > MAX_POINTS) this.snaps.shift();
        this.procs = await api.gpuProcPoll(id);
        this.pollError = null;
        if (this.metricsAuto) void this.fetchMetrics();
      } catch (e: any) {
        this.pollError = e?.message ?? JSON.stringify(e);
      } finally {
        polling = false;
      }
    },
    async initMetricsPort(id: string) {
      try {
        const list = await api.listInstances();
        const hit = list.find((i) => i.profileId === id);
        if (hit) this.metricsPort = hit.port;
      } catch {
        /* 忽略 */
      }
    },
    setSelected(keys: string[]) {
      if (keys.length > MAX_SELECTED) {
        return;
      }
      this.metricsSelected.splice(0, this.metricsSelected.length, ...keys);
      localStorage.setItem(SEL_KEY, JSON.stringify(this.metricsSelected));
    },
    async fetchMetrics() {
      const id = useServerStore().currentId;
      if (!id || this.metricsLoading) return;
      this.metricsLoading = true;
      this.metricsErr = null;
      try {
        const samples = await api.metricsPoll(id, this.metricsPort);
        this.metrics = samples;
        const ts = Date.now();
        const values: Record<string, number> = {};
        for (const s of samples) {
          if (Number.isFinite(s.value)) values[metricKey(s)] = s.value;
        }
        this.metricsHistory.push({ ts, values });
        if (this.metricsHistory.length > MAX_POINTS) this.metricsHistory.shift();
        // 首次抓取且无历史勾选：按白名单自动勾选
        if (!this.metricsSelected.length && samples.length) {
          const auto = samples
            .filter((s) => AUTO_SELECT_RE.test(s.name) && Number.isFinite(s.value))
            .map(metricKey)
            .filter((k, i, arr) => arr.indexOf(k) === i)
            .slice(0, MAX_SELECTED);
          for (const k of auto) this.metricsSelected.push(k);
          if (auto.length) localStorage.setItem(SEL_KEY, JSON.stringify(this.metricsSelected));
        }
      } catch (e: any) {
        this.metricsErr = e?.message ?? JSON.stringify(e);
        this.metrics = [];
      } finally {
        this.metricsLoading = false;
      }
    },
  },
});
