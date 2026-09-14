<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  NButton,
  NConfigProvider,
  darkTheme,
  dateEnUS,
  dateZhCN,
  enUS,
  zhCN,
} from "naive-ui";
import { useI18n } from "vue-i18n";
import { i18n } from "../i18n";
import { listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSettingsStore } from "../stores/settings";
import { useServerStore } from "../stores/server";
import { api } from "../lib/api";
import { deriveRates, fmtTps, type DerivedRates } from "../lib/metricsDerive";
import type { GpuPoll, MetricSample } from "../lib/types";
import Gauge from "../components/Gauge.vue";

const { t } = useI18n();
const settings = useSettingsStore();
const server = useServerStore();

const theme = computed(() => (settings.value.darkTheme ? darkTheme : null));
watch(
  () => settings.value.language,
  (lang) => {
    if (lang === "en" || lang === "zh") i18n.global.locale.value = lang;
  },
  { immediate: true },
);
const naiveLocale = computed(() => (settings.value.language === "en" ? enUS : zhCN));
const naiveDateLocale = computed(() =>
  settings.value.language === "en" ? dateEnUS : dateZhCN,
);

// ---------- 状态 ----------
const profileId = ref<string | null>(null);
const poll = ref<GpuPoll | null>(null);
const err = ref<string | null>(null);
const metricsPort = ref<number | null>(null);
const rates = ref<DerivedRates>({ decode: null, prefill: null });
let mPrev: MetricSample[] | null = null;
let mCurr: MetricSample[] | null = null;
let lastMetricsTs = 0;
let timer: number | null = null;
let unlistenProfile: (() => void) | null = null;

const profileName = computed(
  () => server.profiles.find((p) => p.id === profileId.value)?.name ?? "—"
);

// ---------- 多卡聚合：util/温度取 max，显存取 Σused/Σtotal ----------
const agg = computed(() => {
  const g = poll.value?.gpus ?? [];
  if (g.length === 0) return null;
  const utils = g.map((x) => x.util).filter((v): v is number => v != null);
  const temps = g.map((x) => x.tempC).filter((v): v is number => v != null);
  let memUsed = 0;
  let memTotal = 0;
  for (const x of g) {
    if (x.memUsedMb != null) memUsed += x.memUsedMb;
    if (x.memTotalMb != null) memTotal += x.memTotalMb;
  }
  return {
    util: utils.length ? Math.max(...utils) : null,
    memPct: memTotal > 0 ? (memUsed / memTotal) * 100 : null,
    temp: temps.length ? Math.max(...temps) : null,
    perGpu: g.map((x) => ({ i: x.index, util: x.util })),
  };
});

// ---------- 轮询 ----------
function pollMs() {
  const v = settings.value.pollIntervalMs;
  return Math.min(Math.max(v || 3000, 1000), 60000);
}

async function resolvePort() {
  try {
    const list = await api.listInstances();
    const hit = list.find((i) => i.profileId === profileId.value);
    metricsPort.value = hit?.port ?? null;
  } catch {
    metricsPort.value = null;
  }
}

async function tick() {
  const id = profileId.value;
  if (!id) return;
  try {
    poll.value = await api.gpuPoll(id);
    err.value = null;
  } catch (e: any) {
    err.value = e?.message ?? String(e);
  }
  if (metricsPort.value) {
    try {
      const now = Date.now();
      const dtMs = lastMetricsTs > 0 ? now - lastMetricsTs : undefined;
      mPrev = mCurr;
      mCurr = await api.metricsPoll(id, metricsPort.value);
      lastMetricsTs = Date.now();
      rates.value = deriveRates(mPrev, mCurr, dtMs);
    } catch {
      // metrics 暂不可用（实例重启中/端口变更）：保留上次数据
    }
  }
}

function startLoop() {
  if (timer != null) window.clearInterval(timer);
  void tick();
  timer = window.setInterval(() => void tick(), pollMs());
}

async function setProfile(id: string | null) {
  if (!id || id === profileId.value) return;
  profileId.value = id;
  poll.value = null;
  err.value = null;
  mPrev = null;
  mCurr = null;
  lastMetricsTs = 0;
  rates.value = { decode: null, prefill: null };
  await resolvePort();
  startLoop();
}

onMounted(async () => {
  // 共享连接状态兜底：发现当前已连接的档案（与主窗口同一逻辑）
  await server.loadProfiles();
  if (server.currentId) await setProfile(server.currentId);
  // 主窗口切档案 → 跟随
  unlistenProfile = await listen<{ profileId: string }>("gpu-mini/profile", (e) =>
    void setProfile(e.payload.profileId)
  );
});

onBeforeUnmount(() => {
  if (timer != null) window.clearInterval(timer);
  unlistenProfile?.();
});

// ---------- 操作 ----------
function backToMain() {
  void WebviewWindow.getByLabel("main").then((w) => w?.setFocus());
}
function close() {
  void getCurrentWindow().close();
}
</script>

<template>
  <n-config-provider :theme="theme" :locale="naiveLocale" :date-locale="naiveDateLocale">
    <div class="mini" :class="{ stale: err != null }">
      <!-- 标题栏（整条可拖拽移动窗口） -->
      <div class="bar" data-tauri-drag-region>
        <span class="dot" data-tauri-drag-region></span>
        <span class="name" data-tauri-drag-region>{{ profileName }}</span>
        <n-button quaternary size="tiny" class="act" :title="t('mini.backToMain')" @click="backToMain">
          ⤢
        </n-button>
        <n-button quaternary size="tiny" class="act" :title="t('mini.close')" @click="close">
          ✕
        </n-button>
      </div>

      <!-- 三个指针表 -->
      <div class="gauges">
        <gauge :label="t('mini.gaugeUtil')" :value="agg?.util ?? null" unit="%" />
        <gauge :label="t('mini.gaugeMem')" :value="agg?.memPct ?? null" unit="%" />
        <gauge :label="t('mini.gaugeTemp')" :value="agg?.temp ?? null" unit="°C" />
      </div>

      <!-- per-GPU 小条（多卡时显示每张卡 util） -->
      <div class="strip">
        <template v-if="agg">
          <span v-for="g in agg.perGpu" :key="g.i" class="chip">
            G{{ g.i }} {{ g.util == null ? "--" : g.util + "%" }}
          </span>
        </template>
        <span v-else class="chip">--</span>
      </div>

      <!-- 推理速率（有运行实例才显示） -->
      <div v-if="metricsPort != null" class="rates">
        <span>⚡ {{ t("mini.decode") }} {{ fmtTps(rates.decode) }} tok/s</span>
        <span class="sep">·</span>
        <span>⬇ {{ t("mini.prefill") }} {{ fmtTps(rates.prefill) }} tok/s</span>
      </div>

      <div v-if="err != null" class="err">{{ t("mini.disconnected") }}</div>
    </div>
  </n-config-provider>
</template>

<style scoped>
.mini {
  height: 100vh;
  display: flex;
  flex-direction: column;
  user-select: none;
  overflow: hidden;
  background: transparent;
}
.mini.stale {
  opacity: 0.55;
}
.bar {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 4px 0 10px;
  flex: 0 0 auto;
  cursor: default;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #22c55e;
  flex: 0 0 auto;
}
.stale .dot {
  background: #f59e0b;
}
.name {
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1 1 auto;
}
.act {
  flex: 0 0 auto;
}
.gauges {
  display: flex;
  justify-content: space-around;
  align-items: flex-start;
  flex: 1 1 auto;
  min-height: 0;
}
.strip {
  display: flex;
  gap: 6px;
  justify-content: center;
  flex: 0 0 auto;
  padding: 2px 8px 0;
}
.chip {
  font-size: 10.5px;
  padding: 1px 7px;
  border-radius: 8px;
  background: rgba(128, 128, 128, 0.15);
  white-space: nowrap;
}
.rates {
  display: flex;
  gap: 6px;
  justify-content: center;
  font-size: 11px;
  padding: 3px 8px;
  flex: 0 0 auto;
  white-space: nowrap;
}
.rates .sep {
  opacity: 0.5;
}
.err {
  text-align: center;
  font-size: 10.5px;
  color: #f59e0b;
  flex: 0 0 auto;
  padding-bottom: 2px;
}
</style>
