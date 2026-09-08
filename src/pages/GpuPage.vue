<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref } from "vue";
import {
  NButton,
  NCard,
  NDataTable,
  NEmpty,
  NInputNumber,
  NModal,
  NProgress,
  NResult,
  NSlider,
  NSpace,
  NSwitch,
  NTag,
  useDialog,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useServerStore } from "../stores/server";
import { useSettingsStore } from "../stores/settings";
import { api, fmtBytes, onTaskStream } from "../lib/api";
import type { GpuProcRow, GpuQueryResult } from "../lib/types";

const store = useServerStore();
const { current } = storeToRefs(store);
const settingsStore = useSettingsStore();
const message = useMessage();
const dialog = useDialog();

const result = ref<GpuQueryResult | null>(null);
const loading = ref(false);

async function refresh() {
  const pid = current.value?.id;
  if (!pid || loading.value) return;
  loading.value = true;
  try {
    result.value = await api.gpuQuery(pid);
  } catch (e: any) {
    message.error(`GPU 信息获取失败: ${e?.message ?? JSON.stringify(e)}`);
  } finally {
    loading.value = false;
  }
}

interface MergedCard {
  index: number;
  name: string;
  serial: string | null;
  driver: string;
  vbios: string | null;
  pcie: string;
  stat: GpuQueryResult["stats"][number] | null;
  persistence: boolean | null;
  computeMode: string | null;
  ecc: boolean | null;
  eccInfo: GpuQueryResult["ecc"][number] | null;
  throttleReasons: string[];
}

const mergedCards = computed<MergedCard[]>(() => {
  const r = result.value;
  if (!r) return [];
  return r.cards.map((c) => {
    const st = r.stats.find((s) => s.index === c.index) ?? null;
    const set = r.settings.find((s) => s.index === c.index);
    const thr = r.throttles.find((t) => t.index === c.index);
    const ecc = r.ecc.find((e) => e.index === c.index) ?? null;
    const pcie = c.pcieGenCurrent != null
      ? `PCIe ${c.pcieGenCurrent}/${c.pcieGenMax ?? "?"} x${c.pcieWidth ?? "?"}`
      : "";
    return {
      index: c.index,
      name: c.name,
      serial: c.serial,
      driver: c.driver,
      vbios: c.vbios,
      pcie,
      stat: st,
      persistence: set?.persistence ?? null,
      computeMode: set?.computeMode ?? null,
      ecc: set?.ecc ?? null,
      eccInfo: ecc,
      throttleReasons: thr?.reasons ?? [],
    };
  });
});

function memPct(st: MergedCard["stat"]): number {
  if (!st?.memTotalMb || st.memUsedMb == null) return 0;
  return Math.min(100, Math.round((st.memUsedMb / st.memTotalMb) * 100));
}

// ---------- 进程结束（仅自己的进程） ----------
async function killProc(p: GpuProcRow) {
  const pid = current.value?.id;
  if (!pid) return;
  dialog.error({
    title: "结束 GPU 进程",
    content: `PID ${p.pid}（${p.name}，占用 ${p.memMb ?? "?"} MiB）\n先 SIGTERM，3 秒后仍存活将 SIGKILL。确认结束？`,
    positiveText: "结束",
    negativeText: "取消",
    style: "color: #e88080",
    onPositiveClick: async () => {
      try {
        const out = await api.gpuKill(pid, p.pid);
        if (out.includes("NOT_OWNER")) {
          message.error(`不是你的进程（属主 ${out.replace("NOT_OWNER", "").trim()}）`);
        } else if (out.includes("PROC_GONE")) {
          message.info("进程已不存在");
        } else {
          message.success(`已结束: ${out.split("\n").join(" / ")}`);
        }
        refresh();
      } catch (e: any) {
        message.error(`结束失败: ${e?.message ?? JSON.stringify(e)}`);
      }
    },
  });
}

const procColumns: DataTableColumns<GpuProcRow> = [
  { title: "GPU", key: "gpu", width: 60 },
  { title: "PID", key: "pid", width: 90 },
  {
    title: "属主",
    key: "user",
    width: 140,
    render: (r) => (r.mine ? `${r.user}（我）` : r.user || "-"),
  },
  { title: "进程", key: "name", ellipsis: { tooltip: true } },
  { title: "运行时长", key: "elapsed", width: 120 },
  {
    title: "显存",
    key: "memMb",
    width: 110,
    render: (r) => (r.memMb != null ? `${r.memMb} MiB` : "-"),
  },
  {
    title: "操作",
    key: "actions",
    width: 90,
    render: (r) =>
      h(
        NButton,
        {
          size: "tiny",
          type: "error",
          ghost: true,
          disabled: !r.mine,
          title: r.mine ? "结束进程" : "只能结束自己的进程",
          onClick: () => killProc(r),
        },
        { default: () => "结束" },
      ),
  },
];

// ---------- 设置修改（persistence / 功耗，sudo） ----------
const setPreviewShow = ref(false);
const setScript = ref("");
const setPending = ref<null | (() => void)>(null);
const passShow = ref(false);
const pass = ref("");
const passErr = ref("");
const logShow = ref(false);
const logText = ref("");
const logDone = ref<number | null>(null);
const cancelLog = ref<null | (() => Promise<void>)>(null);
const powerDrafts = ref<Record<number, number>>({});

async function runSetTask(taskId: string) {
  logText.value = "";
  logDone.value = null;
  logShow.value = true;
  cancelLog.value = await onTaskStream(
    taskId,
    (c) => {
      logText.value += c.data;
    },
    (d) => {
      logText.value += `\n[退出码 ${d.exitCode}]\n`;
      logDone.value = d.exitCode;
    },
  );
}

async function startSet(action: string, gpu: number | null, value: number) {
  const pid = current.value?.id;
  if (!pid) return;
  setActionTitle.value = SET_TITLES[action] ?? "调整 GPU 设置";
  try {
    const mode = await api.sudoModeCheck(pid);
    setScript.value = await api.gpuSetPreview(mode, action, gpu, value);
    const launch = async (password: string | null) => {
      try {
        const taskId = await api.gpuSetStart(pid, action, gpu, value, password);
        await runSetTask(taskId);
      } catch (e: any) {
        message.error(`启动失败: ${e?.message ?? JSON.stringify(e)}`);
      }
    };
    setPending.value = () => {
      if (mode === "password") {
        pass.value = "";
        passErr.value = "";
        setPending.value = () => {
          if (!pass.value.trim()) {
            passErr.value = "请输入 sudo 密码";
            return;
          }
          passShow.value = false;
          void launch(pass.value.trim());
        };
        passShow.value = true;
      } else {
        void launch(null);
      }
    };
    setPreviewShow.value = true;
  } catch (e: any) {
    message.error(`获取命令失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

function confirmSet() {
  setPreviewShow.value = false;
  setPending.value?.();
}

function confirmPass() {
  setPending.value?.();
}

function closeLog() {
  logShow.value = false;
  cancelLog.value?.();
  cancelLog.value = null;
  if (logDone.value === 0) {
    refresh();
  }
}

function togglePersistence(c: MergedCard, on: boolean) {
  void startSet("pm", c.index, on ? 1 : 0);
}

function applyPower(c: MergedCard) {
  const v = powerDrafts.value[c.index];
  if (v == null) {
    message.warning("请先拖动选择功耗上限");
    return;
  }
  void startSet("pl", c.index, v);
}

function powerRange(c: MergedCard): [number, number] {
  const st = c.stat;
  const min = Math.round(st?.powerMinW ?? 50);
  const max = Math.round(st?.powerMaxW ?? 800);
  return [min, max];
}

const SET_TITLES: Record<string, string> = { pm: "切换 Persistence Mode", pl: "调整功耗上限" };
const setActionTitle = ref("调整 GPU 设置");

onMounted(() => {
  void settingsStore.load();
  refresh();
});

onBeforeUnmount(() => {
  cancelLog.value?.();
});
</script>

<template>
  <div>
    <n-space justify="space-between" align="center" style="margin-bottom: 16px">
      <h2 style="margin: 0">GPU 管理</h2>
      <n-space size="small">
        <n-tag v-if="mergedCards.length" size="small">
          驱动 {{ mergedCards[0].driver || "-" }}
        </n-tag>
        <n-button size="small" :loading="loading" @click="refresh">重新检测</n-button>
      </n-space>
    </n-space>

    <n-result
      v-if="!current"
      status="404"
      title="未连接服务器"
      description="请先连接服务器"
    />

    <n-space v-else vertical :size="16">
      <!-- 每卡概览 -->
      <n-grid v-if="mergedCards.length" :x-gap="16" :y-gap="16" cols="1 m:2" responsive="screen">
        <n-grid-item v-for="c in mergedCards" :key="c.index">
          <n-card size="small">
            <template #header>
              GPU {{ c.index }} · {{ c.name }}
              <n-tag
                size="tiny"
                :type="c.persistence ? 'success' : 'default'"
                style="margin-left: 8px"
              >
                persistence {{ c.persistence ? "开" : "关" }}
              </n-tag>
              <n-tag v-if="c.throttleReasons.length" size="tiny" type="warning" style="margin-left: 4px">
                降频中
              </n-tag>
            </template>
            <div class="gpu-meta">
              <span v-if="c.serial">序列号 {{ c.serial }}</span>
              <span v-if="c.vbios">VBIOS {{ c.vbios }}</span>
              <span v-if="c.pcie">{{ c.pcie }}</span>
              <span v-if="c.computeMode">计算模式 {{ c.computeMode }}</span>
              <span v-if="c.ecc != null">ECC {{ c.ecc ? "开" : "关" }}</span>
              <span v-if="c.eccInfo?.corrected != null">
                ECC 易失错误 {{ c.eccInfo.corrected }}/{{ c.eccInfo.uncorrected ?? 0 }}
              </span>
            </div>
            <div class="stat-row">
              <div class="stat-cell">
                <div class="num">{{ c.stat?.util ?? "-" }}<small>%</small></div>
                <div class="cap">利用率</div>
              </div>
              <div class="stat-cell">
                <n-progress
                  type="line"
                  :percentage="memPct(c.stat)"
                  :height="6"
                  :rail-size="4"
                  :show-indicator="false"
                  style="width: 110px"
                />
                <div class="cap" style="margin-top: 3px">
                  {{ fmtBytes((c.stat?.memUsedMb ?? 0) * 1048576) }} /
                  {{ fmtBytes((c.stat?.memTotalMb ?? 0) * 1048576) }}
                </div>
              </div>
              <div class="stat-cell">
                <div class="num">{{ c.stat?.tempC ?? "-" }}<small>°C</small></div>
                <div class="cap">温度</div>
              </div>
              <div class="stat-cell">
                <div class="num">{{ c.stat?.powerW ?? "-" }}<small>W</small></div>
                <div class="cap">功耗 / {{ c.stat?.powerLimitW ?? "-" }}W</div>
              </div>
              <div class="stat-cell">
                <div class="num">{{ c.stat?.smClockMhz ?? "-" }}</div>
                <div class="cap">SM MHz / 最大 {{ c.stat?.smClockMaxMhz ?? "-" }}</div>
              </div>
            </div>
            <div v-if="c.throttleReasons.length" class="throttle">
              <n-tag v-for="r in c.throttleReasons" :key="r" size="small" type="warning">
                {{ r }}
              </n-tag>
            </div>
          </n-card>
        </n-grid-item>
      </n-grid>
      <n-empty
        v-else-if="!loading"
        description="未检测到 GPU（驱动未安装或 nvidia-smi 不可用）"
        style="padding: 24px 0"
      />

      <!-- GPU 进程 -->
      <n-card size="small" title="GPU 进程">
        <n-data-table
          v-if="result?.procs.length"
          :columns="procColumns"
          :data="result.procs"
          :bordered="false"
          size="small"
          :max-height="260"
        />
        <n-empty v-else description="当前没有占用 GPU 的进程" style="padding: 12px 0" />
      </n-card>

      <!-- 运维设置 -->
      <n-card size="small" title="运维设置（需要 sudo）">
        <div v-for="c in mergedCards" :key="c.index" class="set-row">
          <span class="set-label">GPU {{ c.index }}</span>
          <n-space align="center" :size="8">
            <span class="set-cap">Persistence</span>
            <n-switch
              size="small"
              :value="c.persistence ?? false"
              @update:value="(v: boolean) => togglePersistence(c, v)"
            />
          </n-space>
          <n-space align="center" :size="8" class="power-box">
            <span class="set-cap">功耗上限</span>
            <n-slider
              :value="powerDrafts[c.index] ?? Math.round(c.stat?.powerLimitW ?? 0)"
              :min="powerRange(c)[0]"
              :max="powerRange(c)[1]"
              :step="5"
              :format-tooltip="(v: number) => `${v}W`"
              style="width: 180px"
              @update:value="(v: number) => (powerDrafts[c.index] = v)"
            />
            <n-input-number
              :value="powerDrafts[c.index] ?? Math.round(c.stat?.powerLimitW ?? 0)"
              :min="powerRange(c)[0]"
              :max="powerRange(c)[1]"
              size="small"
              style="width: 100px"
              @update:value="(v: number | null) => v != null && (powerDrafts[c.index] = v)"
            />
            <span class="set-cap" v-if="c.stat?.powerDefaultW">
              默认 {{ Math.round(c.stat.powerDefaultW) }}W
            </span>
            <n-button size="tiny" type="primary" ghost @click="applyPower(c)">应用</n-button>
          </n-space>
        </div>
        <div v-if="!mergedCards.length" class="set-cap" style="padding: 8px 0">无可用 GPU</div>
      </n-card>

      <!-- 拓扑 -->
      <n-card v-if="result?.topo" size="small" title="GPU 拓扑（nvidia-smi topo -m）">
        <pre class="topo">{{ result.topo }}</pre>
      </n-card>
    </n-space>

    <!-- 设置命令预览 -->
    <n-modal v-model:show="setPreviewShow" preset="card" :title="setActionTitle" style="width: 620px">
      <p style="margin-top: 0; color: #999; font-size: 13px">将在服务器执行：</p>
      <pre class="glog" style="height: 140px">{{ setScript }}</pre>
      <template #footer>
        <n-space justify="end">
          <n-button @click="setPreviewShow = false">取消</n-button>
          <n-button type="primary" @click="confirmSet">执行</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- sudo 密码 -->
    <n-modal
      v-model:show="passShow"
      preset="card"
      title="需要 sudo 密码"
      style="width: 440px"
      :mask-closable="false"
    >
      <p style="margin-top: 0; color: #999; font-size: 13px">
        当前用户无免密 sudo 权限，请输入该用户的 sudo 密码（仅本次使用，不会保存）。
      </p>
      <n-input
        v-model:value="pass"
        type="password"
        show-password-on="click"
        placeholder="sudo 密码"
        :status="passErr ? 'error' : undefined"
        @keyup.enter="confirmPass"
      />
      <div v-if="passErr" class="pass-err">{{ passErr }}</div>
      <template #footer>
        <n-space justify="end">
          <n-button @click="passShow = false">取消</n-button>
          <n-button type="primary" @click="confirmPass">确定</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 设置日志 -->
    <n-modal
      :show="logShow"
      preset="card"
      :title="setActionTitle + ' 日志'"
      style="width: 720px"
      :mask-closable="false"
      @close="closeLog"
    >
      <pre class="glog">{{ logText || "(等待输出...)" }}</pre>
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="logDone != null" :type="logDone === 0 ? 'success' : 'error'">
          退出码 {{ logDone }}
        </n-tag>
        <n-button v-if="logDone != null" type="primary" @click="closeLog">
          {{ logDone === 0 ? "完成" : "关闭" }}
        </n-button>
      </n-space>
    </n-modal>
  </div>
</template>

<style scoped>
.gpu-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 16px;
  font-size: 12px;
  color: #999;
  margin-bottom: 10px;
}
.stat-row {
  display: flex;
  gap: 20px;
  align-items: center;
  flex-wrap: wrap;
  margin-bottom: 6px;
}
.stat-cell {
  min-width: 70px;
}
.num {
  font-size: 20px;
  font-weight: 600;
  line-height: 1.2;
}
.num small {
  font-size: 12px;
  color: #999;
  margin-left: 2px;
}
.cap {
  font-size: 11px;
  color: #999;
}
.throttle {
  margin: 4px 0;
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.set-row {
  display: flex;
  align-items: center;
  gap: 24px;
  padding: 10px 0;
  border-bottom: 1px dashed rgba(128, 128, 128, 0.15);
  flex-wrap: wrap;
}
.set-row:last-child {
  border-bottom: none;
}
.set-label {
  flex: 0 0 60px;
  font-weight: 600;
}
.set-cap {
  font-size: 12px;
  color: #999;
}
.power-box {
  flex: 1 1 auto;
}
.topo {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre;
  background: rgba(0, 0, 0, 0.3);
  padding: 12px;
  border-radius: 6px;
  max-height: 240px;
  overflow: auto;
}
.glog {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  background: rgba(0, 0, 0, 0.3);
  padding: 12px;
  border-radius: 6px;
  height: 320px;
  overflow: auto;
}
.pass-err {
  color: #e88080;
  font-size: 12px;
  margin-top: 6px;
}
</style>
