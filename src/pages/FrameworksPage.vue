<script setup lang="ts">
import {
  computed,
  h,
  onBeforeUnmount,
  onMounted,
  reactive,
  ref,
  watch,
} from "vue";
import {
  NButton,
  NCard,
  NCheckbox,
  NDataTable,
  NDrawer,
  NDrawerContent,
  NEmpty,
  NForm,
  NFormItem,
  NGrid,
  NGridItem,
  NInput,
  NInputNumber,
  NModal,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NSpace,
  NSwitch,
  NTag,
  useDialog,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useServerStore } from "../stores/server";
import { useInstanceStore } from "../stores/instance";
import { api, onTaskStream } from "../lib/api";
import type { InstanceConfig } from "../lib/types";

interface ParamDef {
  key: string;
  label: string;
  type: "text" | "number" | "switch" | "select";
  options?: string[];
  default?: string | number | boolean;
  placeholder?: string;
  step?: number;
  nativeOnly?: boolean;
  dockerOnly?: boolean;
}

interface FwMeta {
  label: string;
  desc: string;
  defaultPort: number;
  dockerImage: string;
  params: ParamDef[];
}

const vllmParams: ParamDef[] = [
  { key: "bin", label: "命令", type: "text", default: "vllm", placeholder: "vllm", nativeOnly: true },
  { key: "tp", label: "张量并行 TP", type: "number", default: 1 },
  { key: "maxModelLen", label: "max-model-len", type: "number", placeholder: "留空=自动" },
  { key: "gpuMemUtil", label: "显存利用率", type: "number", default: 0.9, step: 0.05 },
  { key: "dtype", label: "数据类型", type: "select", options: ["auto", "bfloat16", "float16", "half"], default: "auto" },
  { key: "enforceEager", label: "enforce-eager", type: "switch", default: false },
  { key: "servedModelName", label: "served-model-name", type: "text", placeholder: "留空=目录名" },
  { key: "extraArgs", label: "附加参数", type: "text", placeholder: "--limit-concurrency 32" },
];

const FW_META: Record<string, FwMeta> = {
  "vllm": {
    label: "vLLM",
    desc: "高吞吐推理引擎（OpenAI 兼容 API）",
    defaultPort: 8000,
    dockerImage: "vllm/vllm-openai:latest",
    params: vllmParams,
  },
  "1cat-vllm": {
    label: "1Cat-vLLM",
    desc: "vLLM fork（含 sm70/V100 支持）",
    defaultPort: 8000,
    dockerImage: "vllm/vllm-openai:latest",
    params: vllmParams.map((p) =>
      p.key === "bin" ? { ...p, default: "1cat-vllm" } : p,
    ),
  },
  "sglang": {
    label: "SGLang",
    desc: "结构化生成优化的推理框架",
    defaultPort: 30000,
    dockerImage: "lmsysorg/sglang:latest",
    params: [
      { key: "tp", label: "张量并行 TP", type: "number", default: 1 },
      { key: "memFractionStatic", label: "mem-fraction-static", type: "number", default: 0.85, step: 0.05 },
      { key: "contextLength", label: "context-length", type: "number", placeholder: "留空=默认" },
      { key: "host", label: "host", type: "text", default: "0.0.0.0" },
      { key: "extraArgs", label: "附加参数", type: "text" },
    ],
  },
  "llama-cpp": {
    label: "llama.cpp",
    desc: "GGUF 量化模型推理（llama-server）",
    defaultPort: 8080,
    dockerImage: "ggml-org/llama.cpp:server",
    params: [
      { key: "bin", label: "命令", type: "text", default: "llama-server", nativeOnly: true },
      { key: "ngl", label: "GPU 层数 -ngl", type: "number", default: 99 },
      { key: "ctxSize", label: "上下文 -c", type: "number", default: 4096 },
      { key: "threads", label: "线程数", type: "number", default: 0, placeholder: "0=自动" },
      { key: "host", label: "host", type: "text", default: "0.0.0.0" },
      { key: "extraArgs", label: "附加参数", type: "text" },
    ],
  },
};

const serverStore = useServerStore();
const store = useInstanceStore();
const { current } = storeToRefs(serverStore);
const {
  instances,
  statuses,
  detections,
  detecting,
  starting,
  stopping,
  logId,
  logs,
  logsLoading,
  autoRefreshLogs,
} = storeToRefs(store);
const message = useMessage();
const dialog = useDialog();

let statusTimer: number | null = null;
let logTimer: number | null = null;

onMounted(async () => {
  if (current.value) {
    await store.detect(current.value.id);
    await store.load();
  }
  statusTimer = window.setInterval(() => store.refreshStatuses(), 5000);
});

onBeforeUnmount(() => {
  if (statusTimer) window.clearInterval(statusTimer);
  if (logTimer) window.clearInterval(logTimer);
});

// ---------- 新建/编辑 ----------
const showModal = ref(false);
const form = reactive({
  id: null as string | null,
  name: "",
  framework: "vllm" as string,
  mode: "native" as "native" | "docker",
  modelPath: "",
  port: 8000,
  dockerImage: "",
  params: {} as Record<string, any>,
});

const meta = computed(() => FW_META[form.framework] ?? FW_META["vllm"]);

function defaultParams(fw: string): Record<string, any> {
  const out: Record<string, any> = {};
  for (const p of FW_META[fw]?.params ?? []) {
    if (p.default !== undefined) out[p.key] = p.default;
  }
  return out;
}

function onFrameworkChange() {
  form.port = meta.value.defaultPort;
  form.dockerImage = meta.value.dockerImage;
  form.params = defaultParams(form.framework);
}

function openAdd() {
  form.id = null;
  form.name = "";
  form.framework = "vllm";
  form.mode = "native";
  form.modelPath = current.value ? `${current.value.baseDir}/models/` : "";
  onFrameworkChange();
  showModal.value = true;
}

function openEdit(inst: InstanceConfig) {
  form.id = inst.id;
  form.name = inst.name;
  form.framework = inst.framework;
  form.mode = inst.mode;
  form.modelPath = inst.modelPath;
  form.port = inst.port;
  form.dockerImage = inst.dockerImage ?? meta.value.dockerImage;
  form.params = { ...defaultParams(inst.framework), ...(inst.params ?? {}) };
  showModal.value = true;
}

function buildPreviewConfig(): InstanceConfig {
  return {
    id: "preview",
    profileId: current.value?.id ?? "",
    name: form.name || "preview",
    framework: form.framework,
    mode: form.mode,
    modelPath: form.modelPath,
    port: form.port,
    dockerImage: form.dockerImage || null,
    params: form.params,
    createdAt: 0,
  };
}

const preview = ref("");
const previewErr = ref("");
let previewTimer: number | null = null;

watch(
  () => [form.name, form.framework, form.mode, form.modelPath, form.port, form.dockerImage, form.params],
  () => {
    if (previewTimer) window.clearTimeout(previewTimer);
    previewTimer = window.setTimeout(async () => {
      try {
        preview.value = await api.previewCommand(buildPreviewConfig());
        previewErr.value = "";
      } catch (e: any) {
        previewErr.value = String(e?.message ?? e);
        preview.value = "";
      }
    }, 250);
  },
  { deep: true },
);

async function onSubmit() {
  if (!form.name.trim()) {
    message.warning("请输入实例名称");
    return;
  }
  if (!form.modelPath.trim()) {
    message.warning("请输入模型路径");
    return;
  }
  const cfg: InstanceConfig = {
    ...buildPreviewConfig(),
    id: form.id ?? crypto.randomUUID(),
    name: form.name.trim(),
    createdAt: Date.now(),
  };
  await store.save(cfg);
  showModal.value = false;
  message.success("实例已保存");
}

// ---------- 操作 ----------
async function doStart(inst: InstanceConfig) {
  try {
    const r = await store.start(inst.id);
    message.success(r);
  } catch (e: any) {
    message.error(`启动失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

async function doStop(inst: InstanceConfig) {
  try {
    const r = await store.stop(inst.id);
    message.info(r);
  } catch (e: any) {
    message.error(`停止失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

function doDelete(inst: InstanceConfig) {
  dialog.warning({
    title: "删除实例",
    content: `确认删除实例「${inst.name}」？（只删除配置，不停止已运行的进程）`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      await store.remove(inst.id);
      message.success("已删除");
    },
  });
}

async function doOpenLogs(inst: InstanceConfig) {
  await store.openLogs(inst.id);
  if (logTimer) window.clearInterval(logTimer);
  logTimer = window.setInterval(() => {
    if (autoRefreshLogs.value && store.logId) store.refreshLogs();
  }, 5000);
}

const columns = computed<DataTableColumns<InstanceConfig>>(() => [
  { title: "名称", key: "name", width: 150 },
  {
    title: "框架",
    key: "framework",
    width: 120,
    render: (i) => h(NTag, { size: "small" }, { default: () => FW_META[i.framework]?.label ?? i.framework }),
  },
  {
    title: "模式",
    key: "mode",
    width: 80,
    render: (i) => (i.mode === "docker" ? "Docker" : "原生"),
  },
  { title: "模型", key: "modelPath", ellipsis: { tooltip: true } },
  { title: "端口", key: "port", width: 80 },
  {
    title: "状态",
    key: "status",
    width: 170,
    render: (i) => {
      const s = statuses.value[i.id];
      if (!s) return h(NTag, { size: "small" }, { default: () => "未知" });
      if (s.running) {
        const ok = s.healthCode == null || s.healthCode < 500;
        return h(NSpace, { size: 4 }, {
          default: () => [
            h(NTag, { size: "small", type: "success" }, { default: () => "运行中" }),
            h(NTag, { size: "small", type: ok ? "default" : "warning" }, {
              default: () => `PID ${s.pid ?? "?"} · HTTP ${s.healthCode ?? "-"}`,
            }),
          ],
        });
      }
      return h(NTag, { size: "small", type: "error" }, { default: () => "已停止" });
    },
  },
  {
    title: "操作",
    key: "actions",
    width: 260,
    render: (i) => {
      const s = statuses.value[i.id];
      return h(NSpace, { size: 6 }, {
        default: () => [
          h(
            NButton,
            {
              size: "small",
              type: "primary",
              disabled: s?.running,
              loading: starting.value[i.id],
              onClick: () => doStart(i),
            },
            { default: () => "启动" },
          ),
          h(
            NButton,
            {
              size: "small",
              type: "warning",
              disabled: !s?.running,
              loading: stopping.value[i.id],
              onClick: () => doStop(i),
            },
            { default: () => "停止" },
          ),
          h(NButton, { size: "small", onClick: () => openEdit(i) }, { default: () => "参数" }),
          h(NButton, { size: "small", onClick: () => doOpenLogs(i) }, { default: () => "日志" }),
          h(NButton, { size: "small", type: "error", onClick: () => doDelete(i) }, { default: () => "删除" }),
        ],
      });
    },
  },
]);

const detectionCards = computed(() =>
  ["vllm", "1cat-vllm", "sglang", "llama-cpp"].map((fw) => {
    const d = detections.value.find((x) => x.framework === fw);
    return {
      fw,
      label: FW_META[fw].label,
      desc: FW_META[fw].desc,
      installed: d?.installed ?? false,
      version: d?.version ?? null,
      note: d?.note ?? null,
      nativeTool: NATIVE_TOOL[fw],
      dockerTool: DOCKER_TOOL[fw],
    };
  }),
);

// ---------- 安装 ----------
const NATIVE_TOOL: Record<string, string> = {
  vllm: "vllm",
  "1cat-vllm": "1cat-vllm",
  sglang: "sglang",
  "llama-cpp": "llama-cpp",
};
const DOCKER_TOOL: Record<string, string> = {
  vllm: "docker-vllm",
  "1cat-vllm": "docker-vllm",
  sglang: "docker-sglang",
  "llama-cpp": "docker-llama",
};

const installShow = ref(false);
const installScript = ref("");
const installTool = ref("");
const installStreamShow = ref(false);
const installStream = ref("");
const installDone = ref<number | null>(null);
const cancelInstall = ref<null | (() => Promise<void>)>(null);

async function askInstall(tool: string, label: string) {
  const pid = current.value?.id;
  if (!pid) return;
  try {
    installScript.value = await api.installPreview(pid, tool);
    installTool.value = tool;
    installShow.value = true;
    void label;
  } catch (e: any) {
    message.error(`获取安装命令失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

async function onInstallConfirm() {
  const pid = current.value?.id;
  if (!pid) return;
  installShow.value = false;
  try {
    const taskId = await api.installStart(pid, installTool.value);
    installStream.value = "";
    installDone.value = null;
    installStreamShow.value = true;
    cancelInstall.value = await onTaskStream(
      taskId,
      (c) => {
        installStream.value += c.data;
      },
      (d) => {
        installDone.value = d.exitCode;
      },
    );
  } catch (e: any) {
    message.error(`启动安装失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

function closeInstall() {
  installStreamShow.value = false;
  cancelInstall.value?.();
  cancelInstall.value = null;
  if (installDone.value === 0 && current.value) store.detect(current.value.id);
}

onBeforeUnmount(() => {
  cancelInstall.value?.();
});

const visibleParams = computed(() =>
  meta.value.params.filter((p) => {
    if (p.nativeOnly && form.mode === "docker") return false;
    if (p.dockerOnly && form.mode === "native") return false;
    return true;
  }),
);
</script>

<template>
  <div>
    <!-- 框架检测 -->
    <n-card size="small" title="框架检测" style="margin-bottom: 16px">
      <template #header-extra>
        <n-button size="small" :loading="detecting" @click="current && store.detect(current.id)">
          重新检测
        </n-button>
      </template>
      <n-grid :x-gap="16" :y-gap="16" cols="1 s:2 m:4" responsive="screen">
        <n-grid-item v-for="c in detectionCards" :key="c.fw">
          <n-card size="small" :bordered="true">
            <div class="fw-name">
              {{ c.label }}
              <n-tag v-if="c.installed" type="success" size="small">已安装</n-tag>
              <n-tag v-else type="default" size="small">未安装</n-tag>
            </div>
            <div class="fw-desc">{{ c.desc }}</div>
            <div v-if="c.version" class="fw-version">{{ c.version }}</div>
            <n-space size="small" style="margin-top: 8px">
              <n-button v-if="c.nativeTool" size="tiny" @click="askInstall(c.nativeTool, c.label)">
                一键安装
              </n-button>
              <n-button size="tiny" @click="askInstall(c.dockerTool, c.label)">
                拉取镜像
              </n-button>
            </n-space>
          </n-card>
        </n-grid-item>
      </n-grid>
    </n-card>

    <!-- 实例列表 -->
    <n-card size="small" title="推理实例">
      <template #header-extra>
        <n-button type="primary" size="small" @click="openAdd">新建实例</n-button>
      </template>
      <n-data-table :columns="columns" :data="instances" :bordered="false" size="small" />
      <n-empty v-if="!instances.length" description="还没有实例，点击「新建实例」配置模型与启动参数" style="padding: 24px 0" />
    </n-card>

    <!-- 参数表单 -->
    <n-modal v-model:show="showModal" preset="card" :title="form.id ? '编辑实例' : '新建实例'" style="width: 720px">
      <n-form label-placement="left" label-width="110">
        <n-form-item label="实例名称">
          <n-input v-model:value="form.name" placeholder="如 qwen7b-chat" />
        </n-form-item>
        <n-form-item label="框架">
          <n-select
            :value="form.framework"
            :options="Object.entries(FW_META).map(([k, v]) => ({ label: v.label, value: k }))"
            @update:value="(v: string) => ((form.framework = v), onFrameworkChange())"
          />
        </n-form-item>
        <n-form-item label="运行模式">
          <n-radio-group v-model:value="form.mode">
            <n-radio-button value="native">原生进程（nohup + PID）</n-radio-button>
            <n-radio-button value="docker">Docker 容器</n-radio-button>
          </n-radio-group>
        </n-form-item>
        <n-form-item :label="form.framework === 'llama-cpp' ? '模型文件' : '模型目录'">
          <n-input
            v-model:value="form.modelPath"
            :placeholder="current ? `${current.baseDir}/models/...` : ''"
          />
        </n-form-item>
        <n-form-item label="端口">
          <n-input-number v-model:value="form.port" :min="1" :max="65535" style="width: 160px" />
        </n-form-item>
        <n-form-item v-if="form.mode === 'docker'" label="镜像">
          <n-input v-model:value="form.dockerImage" />
        </n-form-item>

        <n-form-item v-for="p in visibleParams" :key="p.key" :label="p.label">
          <n-input
            v-if="p.type === 'text'"
            v-model:value="form.params[p.key]"
            :placeholder="p.placeholder"
          />
          <n-input-number
            v-else-if="p.type === 'number'"
            v-model:value="form.params[p.key]"
            :placeholder="p.placeholder"
            :step="p.step ?? 1"
            :min="0"
          />
          <n-switch v-else-if="p.type === 'switch'" v-model:value="form.params[p.key]" />
          <n-select
            v-else
            v-model:value="form.params[p.key]"
            :options="(p.options ?? []).map((o) => ({ label: o, value: o }))"
          />
        </n-form-item>

        <n-form-item label="启动命令预览">
          <n-input
            :value="previewErr || preview"
            type="textarea"
            readonly
            :autosize="{ minRows: 2, maxRows: 6 }"
            class="preview"
          />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showModal = false">取消</n-button>
          <n-button type="primary" @click="onSubmit">保存</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 安装确认 -->
    <n-modal v-model:show="installShow" preset="card" title="一键安装" style="width: 620px">
      <p style="margin-top: 0; color: #999; font-size: 13px">
        将在服务器执行以下命令：
      </p>
      <pre class="dllog" style="height: 140px">{{ installScript }}</pre>
      <template #footer>
        <n-space justify="end">
          <n-button @click="installShow = false">取消</n-button>
          <n-button type="primary" @click="onInstallConfirm">开始安装</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 安装日志 -->
    <n-modal
      :show="installStreamShow"
      preset="card"
      title="安装日志"
      style="width: 720px"
      :mask-closable="false"
      @close="closeInstall"
    >
      <pre class="dllog">{{ installStream || "(等待输出...)" }}</pre>
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="installDone != null" :type="installDone === 0 ? 'success' : 'error'">
          退出码 {{ installDone }}
        </n-tag>
        <n-button v-if="installDone != null" type="primary" @click="closeInstall">
          {{ installDone === 0 ? "完成" : "关闭" }}
        </n-button>
      </n-space>
    </n-modal>

    <!-- 日志抽屉 -->
    <n-drawer
      :show="!!logId"
      :width="680"
      @update:show="(v: boolean) => v || store.closeLogs()"
    >
      <n-drawer-content closable>
        <template #header>
          <n-space align="center" justify="space-between" style="width: 100%">
            <span>实例日志</span>
            <n-space align="center">
              <n-checkbox v-model:checked="autoRefreshLogs">自动刷新 (5s)</n-checkbox>
              <n-button size="small" :loading="logsLoading" @click="store.refreshLogs()">
                刷新
              </n-button>
            </n-space>
          </n-space>
        </template>
        <pre class="logbox">{{ logs || "(空)" }}</pre>
      </n-drawer-content>
    </n-drawer>
  </div>
</template>

<style scoped>
.fw-name {
  font-size: 14px;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}
.fw-desc {
  font-size: 12px;
  color: #999;
  min-height: 30px;
}
.fw-version {
  font-size: 12px;
  color: #63e2b7;
  margin-top: 4px;
  word-break: break-all;
}
.preview :deep(textarea) {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  white-space: pre;
  overflow-x: auto;
}
.dllog {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  background: rgba(0, 0, 0, 0.3);
  padding: 12px;
  border-radius: 6px;
  overflow: auto;
}
.logbox {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  background: rgba(0, 0, 0, 0.3);
  padding: 12px;
  border-radius: 6px;
  max-height: calc(100vh - 220px);
  overflow: auto;
}
</style>
