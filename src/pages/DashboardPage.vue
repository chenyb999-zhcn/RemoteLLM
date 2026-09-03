<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref } from "vue";
import {
  NButton,
  NCard,
  NDataTable,
  NEmpty,
  NGrid,
  NGridItem,
  NInputNumber,
  NResult,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useServerStore } from "../stores/server";
import { api, fmtBytes } from "../lib/api";
import type { GpuPoll, MetricSample, ProcRow } from "../lib/types";
import LineChart from "../components/LineChart.vue";
import type { Series } from "../components/LineChart.vue";

const store = useServerStore();
const { current, env, envLoading } = storeToRefs(store);
const message = useMessage();

const POLL_MS = 3000;
const MAX_POINTS = 120;

const snaps = ref<GpuPoll[]>([]);
const procs = ref<ProcRow[]>([]);
const pollError = ref<string | null>(null);
let timer: number | null = null;

const metricsPort = ref(8000);
const metrics = ref<MetricSample[]>([]);
const metricsLoading = ref(false);
const metricsErr = ref<string | null>(null);

let polling = false;
async function poll() {
  if (polling) return;
  polling = true;
  const id = current.value?.id;
  if (!id) {
    polling = false;
    return;
  }
  try {
    const snap = await api.gpuPoll(id);
    snaps.value.push(snap);
    if (snaps.value.length > MAX_POINTS) snaps.value.shift();
    procs.value = await api.gpuProcPoll(id);
    pollError.value = null;
  } catch (e: any) {
    pollError.value = e?.message ?? JSON.stringify(e);
  } finally {
    polling = false;
  }
}

onMounted(async () => {
  if (!env.value) {
    try {
      await store.refreshEnv();
    } catch (e: any) {
      message.error(`环境检查失败: ${e?.message ?? JSON.stringify(e)}`);
    }
  }
  await poll();
  timer = window.setInterval(poll, POLL_MS);
});

onBeforeUnmount(() => {
  if (timer) window.clearInterval(timer);
});

async function onRefreshEnv() {
  try {
    await store.refreshEnv();
  } catch (e: any) {
    message.error(`环境检查失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

async function onFetchMetrics() {
  const id = current.value?.id;
  if (!id) return;
  metricsLoading.value = true;
  metricsErr.value = null;
  try {
    metrics.value = await api.metricsPoll(id, metricsPort.value);
  } catch (e: any) {
    metricsErr.value = e?.message ?? JSON.stringify(e);
    metrics.value = [];
  } finally {
    metricsLoading.value = false;
  }
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
    label: "已用内存 (GB)",
    data: snaps.value.map((s) => ({
      x: s.ts,
      y: s.memUsed != null ? s.memUsed / 1073741824 : null,
    })),
  },
]);

function stat(label: string, value: string, warn = false) {
  return { label, value, warn };
}

const procColumns: DataTableColumns<ProcRow> = [
  { title: "GPU", key: "gpu", width: 70 },
  { title: "PID", key: "pid", width: 110 },
  { title: "计算单元 %", key: "itc", width: 110 },
  { title: "显存带宽 %", key: "gmc", width: 110 },
  { title: "显存占用", key: "mem", render: (r) => `${r.mem} MiB` },
];

const metricColumns: DataTableColumns<MetricSample> = [
  {
    title: "指标名",
    key: "name",
    ellipsis: { tooltip: true },
    render: (m) => h("span", { title: m.help ?? "" }, m.name),
  },
  {
    title: "标签",
    key: "labels",
    ellipsis: { tooltip: true },
    render: (m) =>
      m.labels.length
        ? m.labels.map(([k, v]) => `${k}="${v}"`).join(", ")
        : "-",
  },
  {
    title: "值",
    key: "value",
    width: 130,
    render: (m) =>
      Number.isFinite(m.value)
        ? m.value > 100 || m.value < 0.0001
          ? m.value.toExponential(3)
          : String(Number(m.value.toFixed(4)))
        : String(m.value),
  },
];
</script>

<template>
  <div>
    <n-space justify="space-between" align="center" style="margin-bottom: 16px">
      <h2 style="margin: 0">总览</h2>
      <n-space>
        <n-tag v-if="pollError" type="error" size="small">
          轮询失败: {{ pollError }}
        </n-tag>
        <n-button size="small" :loading="envLoading" @click="onRefreshEnv">
          刷新环境
        </n-button>
      </n-space>
    </n-space>

    <!-- 环境信息 -->
    <template v-if="env">
      <n-grid :x-gap="16" :y-gap="16" cols="1 s:2 m:3 l:4" responsive="screen">
        <n-grid-item
          v-for="(it, i) in [
            stat('系统', [env.os, env.kernel].filter(Boolean).join(' ')),
            stat('CPU', env.cpuCount ? `${env.cpuCount} 核` : '-', env.cpuCount == null),
            stat('内存', env.memTotal ? `${fmtBytes(env.memUsed)} / ${fmtBytes(env.memTotal)}` : '-'),
            stat('磁盘 /', env.diskTotal ? `${fmtBytes(env.diskUsed)} / ${fmtBytes(env.diskTotal)}` : '-'),
            stat('Python', env.python ?? '-', env.python == null),
            stat('CUDA', env.cuda ?? '未安装', env.cuda == null),
            stat('GPU 驱动', env.driver || '未安装', !env.driver),
            stat('Docker', env.docker ?? '未安装', env.docker == null),
          ]"
          :key="i"
        >
          <n-card size="small">
            <div class="stat-label">{{ it.label }}</div>
            <n-tag
              :type="it.warn ? 'warning' : 'default'"
              size="small"
              class="stat-value"
              :bordered="false"
            >
              {{ it.value }}
            </n-tag>
          </n-card>
        </n-grid-item>
      </n-grid>
    </template>
    <n-result
      v-else-if="!envLoading"
      status="404"
      title="暂无环境信息"
      description="点击右上角「刷新环境」获取 GPU / CUDA / 驱动信息"
    />

    <!-- 实时监控 -->
    <n-card size="small" style="margin-top: 16px">
      <template #header>实时监控（每 3s 采样）</template>
      <template #header-extra>
        <n-space size="small" align="center">
          <n-tag v-if="latest?.uptime" size="small">运行 {{ latest.uptime }}</n-tag>
          <n-tag v-if="latest" size="small">
            磁盘 {{ fmtBytes(latest.diskUsed) }} / {{ fmtBytes(latest.diskTotal) }}
          </n-tag>
        </n-space>
      </template>

      <template v-if="latest && latest.gpus.length">
        <n-grid :x-gap="16" :y-gap="16" cols="1 m:2" responsive="screen">
          <n-grid-item>
            <div class="chart-title">GPU 利用率</div>
            <line-chart :series="utilSeries" :y-max="100" y-label="%" />
          </n-grid-item>
          <n-grid-item>
            <div class="chart-title">GPU 显存</div>
            <line-chart :series="memSeries" y-label="GB" fill />
          </n-grid-item>
          <n-grid-item>
            <div class="chart-title">GPU 温度</div>
            <line-chart :series="tempSeries" y-label="°C" />
          </n-grid-item>
          <n-grid-item>
            <div class="chart-title">GPU 功耗</div>
            <line-chart :series="powerSeries" y-label="W" />
          </n-grid-item>
        </n-grid>
        <div class="chart-title" style="margin-top: 16px">系统内存</div>
        <line-chart :series="sysMemSeries" y-label="GB" fill />
      </template>
      <n-empty
        v-else
        description="未检测到 GPU（nvidia-smi 不可用）"
        style="padding: 24px 0"
      />

      <!-- 进程 -->
      <div class="chart-title" style="margin-top: 16px">
        GPU 进程（nvidia-smi pmon）
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
        description="当前没有占用 GPU 的进程"
        style="padding: 12px 0"
      />
    </n-card>

    <!-- 推理服务指标 -->
    <n-card size="small" title="推理服务指标 (/metrics)" style="margin-top: 16px">
      <n-space align="center" style="margin-bottom: 12px">
        <span style="color: #999">服务器本地端口：</span>
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
          抓取
        </n-button>
        <span v-if="metricsErr" class="err-text">{{ metricsErr }}</span>
      </n-space>
      <n-data-table
        :columns="metricColumns"
        :data="metrics"
        :bordered="false"
        size="small"
        :max-height="320"
      />
    </n-card>
  </div>
</template>

<style scoped>
.stat-label {
  font-size: 12px;
  color: #999;
  margin-bottom: 6px;
}
.stat-value {
  max-width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.chart-title {
  font-size: 13px;
  color: #bbb;
  margin-bottom: 8px;
}
.err-text {
  color: #e88080;
  font-size: 12px;
}
</style>
