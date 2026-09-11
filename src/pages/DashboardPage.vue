<script setup lang="ts">
import { computed, h, onMounted } from "vue";
import {
  NButton,
  NCard,
  NDataTable,
  NEmpty,
  NGrid,
  NGridItem,
  NInputNumber,
  NProgress,
  NResult,
  NSpace,
  NSwitch,
  NTag,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { useServerStore } from "../stores/server";
import { useDashboardStore, metricKey, TOKEN_RATE_KEY } from "../stores/dashboard";
import { fmtBytes } from "../lib/api";
import type { GpuPoll, MetricSample, ProcRow } from "../lib/types";
import LineChart from "../components/LineChart.vue";
import type { Series } from "../components/LineChart.vue";

const store = useServerStore();
const { env, envLoading } = storeToRefs(store);
const dash = useDashboardStore();
const { snaps, procs, pollError, metricsPort, metrics, metricsLoading, metricsErr } =
  storeToRefs(dash);
const message = useMessage();
const { t } = useI18n();

const MAX_SELECTED = 8;

onMounted(async () => {
  if (!env.value) {
    try {
      await store.refreshEnv();
    } catch (e: any) {
      message.error(t("dashboard.envCheckFailed", { msg: e?.message ?? JSON.stringify(e) }));
    }
  }
});

async function onRefreshEnv() {
  try {
    await store.refreshEnv();
  } catch (e: any) {
    message.error(t("dashboard.envCheckFailed", { msg: e?.message ?? JSON.stringify(e) }));
  }
}

function onFetchMetrics() {
  return dash.fetchMetrics();
}

function onMetricsSelChange(keys: Array<string | number> | null) {
  const next = (keys ?? []).filter((k): k is string => typeof k === "string");
  if (next.length > MAX_SELECTED) {
    message.warning(t("dashboard.maxSelected", { n: MAX_SELECTED }));
    return;
  }
  dash.setSelected(next);
}

const latest = computed(() => snaps.value[snaps.value.length - 1]);

function buildSeries(
  pick: (s: GpuPoll, gi: number) => number | null,
): Series[] {
  const n = latest.value?.gpus.length ?? 0;
  return Array.from({ length: n }, (_, gi) => ({
    label: `GPU ${gi}`,
    data: snaps.value.map((s) => ({ x: s.ts, y: pick(s, gi) })),
  }));
}

const utilSeries = computed(() =>
  buildSeries((s, gi) => s.gpus[gi]?.util ?? null),
);
const memSeries = computed(() =>
  buildSeries((s, gi) =>
    s.gpus[gi]?.memUsedMb != null ? s.gpus[gi]!.memUsedMb! / 1024 : null,
  ),
);
const tempSeries = computed(() =>
  buildSeries((s, gi) => s.gpus[gi]?.tempC ?? null),
);
const powerSeries = computed(() =>
  buildSeries((s, gi) => s.gpus[gi]?.powerW ?? null),
);
const sysMemSeries = computed((): Series[] => [
  {
    label: t("dashboard.sysMemLabel"),
    data: snaps.value.map((s) => ({
      x: s.ts,
      y: s.memUsed != null ? s.memUsed / 1073741824 : null,
    })),
  },
]);

// ---------- 推理指标折线 ----------
function shortLabel(key: string): string {
  if (key === TOKEN_RATE_KEY) return t("dashboard.tokenRateLabel");
  const i = key.indexOf("{");
  const name = i > 0 ? key.slice(0, i) : key;
  const labels = i > 0 ? key.slice(i + 1, key.length - 1) : "";
  const short = name.replace(/^(llama_|llamacpp:|vllm:)/, "");
  if (!labels) return short;
  const parts = labels.split(",").map((s) => s.split("=")[1] ?? s);
  return `${short} ${parts.join(",")}`.slice(0, 56);
}

const metricCharts = computed(() =>
  dash.metricsSelected.map((key) => ({
    key,
    label: shortLabel(key),
    series: {
      label: shortLabel(key),
      data: dash.metricsHistory.map((p) => ({ x: p.ts, y: p.values[key] ?? null })),
    } as Series,
  })),
);

// ---------- 环境卡片 ----------
interface StatItem {
  label: string;
  value: string;
  warn?: boolean;
  title?: string;
}

const cpuStat = computed<StatItem>(() => {
  const model = env.value?.cpuModel?.trim() || "";
  const n = env.value?.cpuCount;
  const value = model
    ? n
      ? `${model} (${t("dashboard.cores", { n })})`
      : model
    : n
      ? t("dashboard.cores", { n })
      : "-";
  return { label: "CPU", value, warn: n == null, title: model || undefined };
});

const disks = computed(() => env.value?.disks ?? []);
const rootDisk = computed(() => disks.value.find((d) => d.mount === "/"));

function diskPct(total: number | null, used: number | null): number {
  if (!total || used == null) return 0;
  return Math.min(100, Math.round((used / total) * 100));
}

const envStats = computed<StatItem[]>(() => {
  const e = env.value;
  if (!e) return [];
  return [
    { label: t("dashboard.statSys"), value: [e.os, e.kernel].filter(Boolean).join(" ") || "-" },
    cpuStat.value,
    {
      label: t("dashboard.statMem"),
      value: e.memTotal ? `${fmtBytes(e.memUsed)} / ${fmtBytes(e.memTotal)}` : "-",
    },
    {
      label:
        disks.value.length > 1
          ? t("dashboard.diskPartitions", { n: disks.value.length })
          : t("dashboard.diskRoot"),
      value:
        rootDisk.value?.total != null
          ? `${fmtBytes(rootDisk.value.used)} / ${fmtBytes(rootDisk.value.total)}`
          : "-",
    },
    { label: "Python", value: e.python ?? "-", warn: e.python == null },
    { label: "CUDA", value: e.cuda ?? t("dashboard.notInstalled"), warn: e.cuda == null },
    { label: t("dashboard.gpuDriver"), value: e.driver || t("dashboard.notInstalled"), warn: !e.driver },
    { label: "Docker", value: e.docker ?? t("dashboard.notInstalled"), warn: e.docker == null },
  ];
});

const procColumns = computed<DataTableColumns<ProcRow>>(() => [
  { title: "GPU", key: "gpu", width: 70 },
  { title: "PID", key: "pid", width: 100 },
  { title: t("dashboard.procColSm"), key: "sm", width: 110 },
  { title: t("dashboard.procColMemBw"), key: "memBw", width: 110 },
  { title: t("dashboard.procColMem"), key: "mem", width: 110, render: (r) => (r.mem != null ? `${r.mem} MiB` : "-") },
  { title: t("dashboard.procColCmd"), key: "command", ellipsis: { tooltip: true } },
]);

const metricColumns = computed<DataTableColumns<MetricSample>>(() => [
  { type: "selection" },
  {
    title: t("dashboard.metricColName"),
    key: "name",
    ellipsis: { tooltip: true },
    render: (m) =>
      m.name === TOKEN_RATE_KEY
        ? h("span", { title: t("dashboard.tokenRateHelp") }, t("dashboard.tokenRateLabel"))
        : h("span", { title: m.help ?? "" }, m.name),
  },
  {
    title: t("dashboard.metricColLabels"),
    key: "labels",
    ellipsis: { tooltip: true },
    render: (m) =>
      m.labels.length
        ? m.labels.map(([k, v]) => `${k}="${v}"`).join(", ")
        : "-",
  },
  {
    title: t("dashboard.metricColValue"),
    key: "value",
    width: 130,
    render: (m) =>
      Number.isFinite(m.value)
        ? m.value === 0
          ? "0"
          : m.value > 100 || m.value < 0.0001
            ? m.value.toExponential(3)
            : String(Number(m.value.toFixed(4)))
        : String(m.value),
  },
]);
</script>

<template>
  <div>
    <n-space justify="space-between" align="center" style="margin-bottom: 16px">
      <h2 style="margin: 0">{{ t("dashboard.title") }}</h2>
      <n-space>
        <n-tag v-if="pollError" type="error" size="small">
          {{ t("dashboard.pollError", { err: pollError }) }}
        </n-tag>
        <n-button size="small" :loading="envLoading" @click="onRefreshEnv">
          <template #icon><span /></template>
          {{ t("dashboard.refreshEnv") }}
        </n-button>
      </n-space>
    </n-space>

    <!-- 环境信息 -->
    <template v-if="env">
      <n-grid :x-gap="8" :y-gap="8" cols="2 m:4" responsive="screen">
        <n-grid-item v-for="(it, i) in envStats" :key="i">
          <n-card size="small" class="stat-card" :content-style="{ padding: '8px 12px' }">
            <div class="stat-label">{{ it.label }}</div>
            <n-tag
              :type="it.warn ? 'warning' : 'default'"
              size="small"
              class="stat-value"
              :bordered="false"
              :title="it.title || it.value"
            >
              {{ it.value }}
            </n-tag>
          </n-card>
        </n-grid-item>
        <!-- 多分区磁盘明细 -->
        <n-grid-item v-if="disks.length > 1" span="2 m:4">
          <n-card size="small" class="stat-card" :content-style="{ padding: '8px 12px' }">
            <div class="stat-label">{{ t("dashboard.diskDetail") }}</div>
            <div
              v-for="d in disks.slice(0, 6)"
              :key="d.mount"
              class="disk-row"
              :title="`${d.mount} (${d.fs})`"
            >
              <span class="disk-mount">{{ d.mount }}</span>
              <n-progress
                type="line"
                :percentage="diskPct(d.total, d.used)"
                :height="4"
                :rail-size="3"
                :show-indicator="false"
                class="disk-bar"
              />
              <span class="disk-text">
                {{ fmtBytes(d.used) }} / {{ fmtBytes(d.total) }} ({{ diskPct(d.total, d.used) }}%)
              </span>
            </div>
            <div v-if="disks.length > 6" class="disk-more">
              {{ t("dashboard.diskMore", { n: disks.length - 6 }) }}
            </div>
          </n-card>
        </n-grid-item>
      </n-grid>
    </template>
    <n-result
      v-else-if="!envLoading"
      status="404"
      :title="t('dashboard.noEnvTitle')"
      :description="t('dashboard.noEnvDesc')"
    />

    <!-- 实时监控 -->
    <n-card size="small" style="margin-top: 16px">
      <template #header>{{ t("dashboard.realtime") }}</template>
      <template #header-extra>
        <n-space size="small" align="center">
          <n-tag v-if="latest?.uptime" size="small">
            {{ t("dashboard.uptime", { v: latest.uptime }) }}
          </n-tag>
          <n-tag v-if="latest" size="small">
            {{ t("dashboard.disk") }} {{ fmtBytes(latest.diskUsed) }} / {{ fmtBytes(latest.diskTotal) }}
          </n-tag>
        </n-space>
      </template>

      <template v-if="latest && latest.gpus.length">
        <n-grid :x-gap="16" :y-gap="16" cols="1 m:2" responsive="screen">
          <n-grid-item>
            <div class="chart-title">{{ t("dashboard.chartUtil") }}</div>
            <line-chart :series="utilSeries" :y-max="100" y-label="%" />
          </n-grid-item>
          <n-grid-item>
            <div class="chart-title">{{ t("dashboard.chartMem") }}</div>
            <line-chart :series="memSeries" y-label="GB" fill />
          </n-grid-item>
          <n-grid-item>
            <div class="chart-title">{{ t("dashboard.chartTemp") }}</div>
            <line-chart :series="tempSeries" y-label="°C" />
          </n-grid-item>
          <n-grid-item>
            <div class="chart-title">{{ t("dashboard.chartPower") }}</div>
            <line-chart :series="powerSeries" y-label="W" />
          </n-grid-item>
        </n-grid>
        <div class="chart-title" style="margin-top: 16px">{{ t("dashboard.chartSysMem") }}</div>
        <line-chart :series="sysMemSeries" y-label="GB" fill />
      </template>
      <n-empty
        v-else
        :description="t('dashboard.noGpu')"
        style="padding: 24px 0"
      />

      <!-- 进程 -->
      <div class="chart-title" style="margin-top: 16px">
        {{ t("dashboard.gpuProcs") }}
      </div>
      <n-data-table
        v-if="procs.length"
        :columns="procColumns"
        :data="procs"
        :bordered="false"
        size="small"
        :max-height="240"
      />
      <n-empty
        v-else
        :description="t('dashboard.noProcs')"
        style="padding: 12px 0"
      />
    </n-card>

    <!-- 推理服务指标 -->
    <n-card size="small" :title="t('dashboard.metricsCard')" style="margin-top: 16px">
      <n-space align="center" style="margin-bottom: 12px">
        <span style="color: #999">{{ t("dashboard.localPort") }}</span>
        <n-input-number
          v-model:value="metricsPort"
          :min="1"
          :max="65535"
          size="small"
          style="width: 110px"
        />
        <n-button
          size="small"
          type="primary"
          :loading="metricsLoading"
          @click="onFetchMetrics"
        >
          <template #icon><span /></template>
          {{ t("dashboard.fetch") }}
        </n-button>
        <n-switch v-model:value="dash.metricsAuto" size="small" />
        <span style="color: #999; font-size: 12px">{{ t("dashboard.autoHint") }}</span>
        <div v-if="metricsErr" class="err-text" :title="metricsErr">{{ metricsErr }}</div>
      </n-space>

      <!-- 勾选指标的折线图（每个指标一张小图，避免量纲混用） -->
      <template v-if="metricCharts.length && dash.metricsHistory.length > 1">
        <n-grid :x-gap="12" :y-gap="12" cols="1 m:2" responsive="screen" style="margin-bottom: 12px">
          <n-grid-item v-for="c in metricCharts" :key="c.key">
            <div class="chart-title" :title="c.key">{{ c.label }}</div>
            <line-chart :series="[c.series]" :height="110" />
          </n-grid-item>
        </n-grid>
      </template>
      <div
        v-else-if="dash.metricsSelected.length && dash.metricsHistory.length <= 1"
        class="chart-title"
        style="margin-bottom: 12px"
      >
        {{ t("dashboard.selectedHint", { n: dash.metricsSelected.length }) }}
      </div>

      <n-data-table
        :columns="metricColumns"
        :data="metrics"
        :bordered="false"
        size="small"
        :max-height="320"
        :row-key="(m: MetricSample) => metricKey(m)"
        :checked-row-keys="dash.metricsSelected"
        @update:checked-row-keys="onMetricsSelChange"
      />
    </n-card>
  </div>
</template>

<style scoped>
.stat-card {
  min-width: 0;
}
.stat-label {
  font-size: 11px;
  color: #999;
  margin-bottom: 4px;
}
.stat-value {
  max-width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.disk-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 0;
  font-size: 12px;
}
.disk-mount {
  /* 固定列宽：所有行的进度条起点/终点左右对齐 */
  flex: 0 0 110px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #ccc;
}
.disk-bar {
  flex: 1 1 auto;
  min-width: 60px;
}
.disk-text {
  flex: 0 0 210px;
  text-align: right;
  white-space: nowrap;
  color: #999;
  font-size: 11px;
}
.disk-more {
  font-size: 11px;
  color: #777;
  margin-top: 2px;
}
.chart-title {
  font-size: 13px;
  color: #bbb;
  margin-bottom: 8px;
}
.err-text {
  color: #e88080;
  font-size: 12px;
  display: block;
  max-width: 340px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
