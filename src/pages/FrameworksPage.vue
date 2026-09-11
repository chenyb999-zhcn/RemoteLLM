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
  NSpin,
  NSwitch,
  NTabs,
  NTabPane,
  NTag,
  NTooltip,
  useDialog,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { useServerStore } from "../stores/server";
import { useInstanceStore } from "../stores/instance";
import { useSettingsStore } from "../stores/settings";
import { api, onTaskStream } from "../lib/api";
import { i18n } from "../i18n";
import DockerInstaller from "../components/DockerInstaller.vue";
import StreamLog from "../components/StreamLog.vue";
import SpinButton from "../components/SpinButton.vue";
import type { DockerStatus, InstanceConfig, LocalImage, LocalModel } from "../lib/types";

import { FW_META, type ParamDef, type FwTab } from "../lib/fwParams";

const serverStore = useServerStore();
const settingsStore = useSettingsStore();
const { value: settings } = storeToRefs(settingsStore);
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
  loadedLines,
  logTotalLines,
  loadingEarlier,
} = storeToRefs(store);
const message = useMessage();
const dialog = useDialog();
const { t } = useI18n();

// ---------- 双语取值（fwParams 数据表用 labelEn/descEn/placeholderEn 双字段） ----------
const lang = computed(() => i18n.global.locale.value);
function fwLabel(fw: string): string {
  const m = FW_META[fw];
  return m ? (lang.value === "en" ? (m.labelEn ?? m.label) : m.label) : fw;
}
function fwDesc(fw: string): string {
  const m = FW_META[fw];
  return m ? (lang.value === "en" ? (m.descEn ?? m.desc) : m.desc) : "";
}
function pLabel(p: ParamDef): string {
  return lang.value === "en" ? (p.labelEn ?? p.label) : p.label;
}
function pDesc(p: ParamDef): string {
  return lang.value === "en" ? (p.descEn ?? p.desc ?? "") : (p.desc ?? "");
}
function pPlaceholder(p: ParamDef): string {
  return lang.value === "en"
    ? (p.placeholderEn ?? p.placeholder ?? "")
    : (p.placeholder ?? "");
}
function tabLabel(tab: FwTab): string {
  return lang.value === "en" ? (tab.labelEn ?? tab.label) : tab.label;
}
function actionText(a: string): string {
  const key = `fw.act_${a}`;
  return i18n.global.te(key) ? t(key) : a;
}

let statusTimer: number | null = null;
let logTimer: number | null = null;

const logStreamRef = ref<InstanceType<typeof StreamLog> | null>(null);

async function onLogReachTop() {
  if (store.loadingEarlier || !store.logId) return;
  if (store.loadedLines >= store.logTotalLines) return;
  logStreamRef.value?.anchorBeforePrepend();
  const hasMore = await store.loadEarlier();
  if (!hasMore) {
    // 已无更早内容：回退计数，提示"已显示全部"
    store.loadedLines = Math.max(500, store.logTotalLines);
  }
}

onMounted(async () => {
  if (current.value) {
    await store.detect(current.value.id);
    await store.load();
    void loadDocker();
  }
  statusTimer = window.setInterval(() => store.refreshStatuses(), 5000);
});

watch(
  () => current.value?.id,
  (id) => {
    if (id) void loadDocker();
  },
);

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

const meta = computed(() => FW_META[form.framework] ?? null);
// 自定义框架（非 4 内置）→ 简化表单（无参数选项，启动命令由用户填写）
const isCustomFw = computed(() => !FW_META[form.framework]);

interface FwOption {
  value: string;
  label: string;
  image: string;
  port: number;
}

/** 框架下拉选项：4 内置 + settings 里的自定义框架（value = 框架名） */
const frameworkOptions = computed<FwOption[]>(() => [
  ...Object.entries(FW_META).map(([k, v]) => ({
    value: k,
    label: v.label,
    image: v.dockerImage,
    port: v.defaultPort,
  })),
  ...settings.value.customFrameworks.map((c) => ({
    value: c.label,
    label: c.label,
    image: c.image,
    port: 8000,
  })),
]);

function fwOption(fw: string): FwOption | undefined {
  return frameworkOptions.value.find((o) => o.value === fw);
}

function defaultParams(fw: string): Record<string, any> {
  const out: Record<string, any> = {};
  for (const p of FW_META[fw]?.params ?? []) {
    if (p.default !== undefined) out[p.key] = p.default;
  }
  return out;
}

function onFrameworkChange() {
  const o = fwOption(form.framework);
  form.port = o?.port ?? 8000;
  form.dockerImage = o?.image ?? "";
  if (isCustomFw.value) {
    // 自定义框架：Docker 模式 + 启动命令模板（用户可自行修改）
    form.mode = "docker";
    form.params = {
      customCmd: `--model ${form.modelPath || t("fw.modelPathToken")} --port ${form.port}`,
    };
  } else {
    form.params = defaultParams(form.framework);
  }
  // 仅原生模式的框架（无官方 Docker 镜像）固定为原生
  if (FW_META[form.framework]?.nativeOnly) form.mode = "native";
}

// ---------- 本地模型列表（实例表单模型下拉 / 投机解码 draft 模型下拉，懒加载） ----------
const localModelsRaw = ref<LocalModel[]>([]);
const modelsLoading = ref(false);
let modelsLoadedFor: string | null = null;

const localModels = computed(() =>
  localModelsRaw.value.map((m) => ({ label: m.name, value: m.path })),
);
const localGgufs = computed(() =>
  localModelsRaw.value
    .filter((m) => m.kind === "gguf" || m.kind === "gguf-split")
    .map((m) => ({ label: m.name, value: m.path })),
);

async function ensureLocalModels() {
  const pid = current.value?.id;
  if (!pid || modelsLoadedFor === pid) return;
  modelsLoading.value = true;
  try {
    localModelsRaw.value = await api.listLocalModels(pid);
    modelsLoadedFor = pid;
  } catch {
    // 列表加载失败不阻塞表单，模型下拉留空（可手输路径）
  } finally {
    modelsLoading.value = false;
  }
}

function openAdd() {
  form.id = null;
  form.name = "";
  form.framework = "vllm";
  form.mode = "native";
  form.modelPath = "";
  onFrameworkChange();
  showModal.value = true;
  ensureLocalModels();
}

function openEdit(inst: InstanceConfig) {
  form.id = inst.id;
  form.name = inst.name;
  form.framework = inst.framework;
  form.mode = isCustomFwFor(inst.framework)
    ? "docker"
    : FW_META[inst.framework]?.nativeOnly
      ? "native"
      : inst.mode;
  form.modelPath = inst.modelPath;
  form.port = inst.port;
  form.dockerImage = inst.dockerImage ?? fwOption(inst.framework)?.image ?? "";
  form.params = isCustomFwFor(inst.framework)
    ? { customCmd: typeof inst.params?.customCmd === "string" ? inst.params.customCmd : "" }
    : { ...defaultParams(inst.framework), ...(inst.params ?? {}) };
  // 向后兼容：旧实例用 mtp 布尔开关，映射到 specType
  if (form.params.mtp === true && !form.params.specType) {
    form.params.specType = "draft-mtp";
  }
  showModal.value = true;
  ensureLocalModels();
}

function isCustomFwFor(fw: string): boolean {
  return !FW_META[fw];
}

function buildPreviewConfig(): InstanceConfig {
  return {
    id: "preview",
    profileId: current.value?.id ?? "",
    name: form.name || "preview",
    framework: form.framework,
    mode: isCustomFw.value ? "docker" : form.mode,
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

function builtinImageOf(fw: string): string {
  if (fw === "1cat-vllm") {
    const p = current.value;
    return p?.onecatImage?.trim() || FW_META["1cat-vllm"].dockerImage;
  }
  return FW_META[fw]?.dockerImage ?? "";
}

const isCustomImage = computed(
  () => !isCustomFw.value && form.mode === "docker" && form.dockerImage.trim() !== builtinImageOf(form.framework),
);

watch(
  () => [form.mode, form.framework, form.dockerImage],
  () => {
    if (!isCustomFw.value && !isCustomImage.value) delete form.params.customCmd;
  },
);

async function onSubmit() {
  if (!form.name.trim()) {
    message.warning(t("fw.reqName"));
    return;
  }
  if (!form.modelPath.trim()) {
    message.warning(t("fw.reqModelPath"));
    return;
  }
  const isDocker = isCustomFw.value || form.mode === "docker";
  if (isDocker && !form.dockerImage.trim()) {
    message.warning(t("fw.reqImage"));
    return;
  }
  if (isCustomFw.value && !String(form.params.customCmd ?? "").trim()) {
    message.warning(t("fw.reqCustomCmd"));
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
  message.success(t("fw.saved"));
}

// ---------- 操作 ----------
async function doStart(inst: InstanceConfig) {
  if (starting.value[inst.id]) return;
  try {
    const r = await store.start(inst.id);
    message.success(r);
  } catch (e: any) {
    message.error(t("common.startFailedMsg", { msg: e?.message ?? JSON.stringify(e) }));
  }
}

async function doStop(inst: InstanceConfig) {
  if (stopping.value[inst.id]) return;
  try {
    const r = await store.stop(inst.id);
    message.info(r);
  } catch (e: any) {
    message.error(t("fw.stopFailed", { msg: e?.message ?? JSON.stringify(e) }));
  }
}

function doDelete(inst: InstanceConfig) {
  dialog.warning({
    title: t("fw.deleteTitle"),
    content: t("fw.deleteContent", { name: inst.name }),
    positiveText: t("common.delete"),
    negativeText: t("common.cancel"),
    onPositiveClick: async () => {
      await store.refreshStatus(inst.id);
      if (store.statuses[inst.id]?.running) {
        try {
          await store.stop(inst.id);
        } catch (e: any) {
          message.error(t("fw.stopFailed", { msg: e?.message ?? JSON.stringify(e) }));
          throw e;
        }
      }
      await store.remove(inst.id);
      message.success(t("fw.deleted"));
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
  { title: t("common.name"), key: "name", width: 150 },
  {
    title: t("fw.colFw"),
    key: "framework",
    width: 120,
    render: (i) => h(NTag, { size: "small" }, { default: () => fwLabel(i.framework) }),
  },
  {
    title: t("fw.colMode"),
    key: "mode",
    width: 80,
    render: (i) => (i.mode === "docker" ? "Docker" : t("fw.modeNativeVal")),
  },
  { title: t("fw.colModel"), key: "modelPath", ellipsis: { tooltip: true } },
  { title: t("fw.colPort"), key: "port", width: 80 },
  {
    title: t("fw.colStatus"),
    key: "status",
    width: 170,
    render: (i) => {
      const s = statuses.value[i.id];
      if (!s) return h(NTag, { size: "small" }, { default: () => t("common.unknown") });
      if (s.running) {
        const ok = s.healthCode == null || s.healthCode < 500;
        return h(NSpace, { size: 4 }, {
          default: () => [
            h(NTag, { size: "small", type: "success" }, { default: () => t("common.running") }),
            h(NTag, { size: "small", type: ok ? "default" : "warning" }, {
              default: () => `PID ${s.pid ?? "?"} · HTTP ${s.healthCode ?? "-"}`,
            }),
          ],
        });
      }
      return h(NTag, { size: "small", type: "error" }, { default: () => t("common.stopped") });
    },
  },
  {
    title: t("common.actions"),
    key: "actions",
    width: 420,
    render: (i) => {
      const s = statuses.value[i.id];
      return h(NSpace, { size: 6 }, {
        default: () => [
          h(
            SpinButton,
            {
              size: "small",
              type: "primary",
              disabled: s?.running,
              loading: starting.value[i.id],
              onClick: () => doStart(i),
            },
            { default: () => t("common.start") },
          ),
          h(
            SpinButton,
            {
              size: "small",
              type: "warning",
              disabled: !s?.running,
              loading: stopping.value[i.id],
              onClick: () => doStop(i),
            },
            { default: () => t("common.stop") },
          ),
          h(NButton, { size: "small", onClick: () => openEdit(i) }, { default: () => t("fw.btnParams") }),
          h(NButton, { size: "small", onClick: () => doOpenLogs(i) }, { default: () => t("fw.btnLogs") }),
          h(NButton, { size: "small", type: "error", onClick: () => doDelete(i) }, { default: () => t("common.delete") }),
        ],
      });
    },
  },
]);

const detectionCards = computed(() =>
  ["vllm", "1cat-vllm", "sglang", "llama-cpp", "fastllm"].map((fw) => {
    const d = detections.value.find((x) => x.framework === fw);
    return {
      fw,
      label: fwLabel(fw),
      desc: fwDesc(fw),
      installed: d?.installed ?? false,
      version: d?.version ?? null,
      note: d?.note ?? null,
      nativeTool: NATIVE_TOOL[fw],
    };
  }),
);

// ---------- 安装 ----------
const NATIVE_TOOL: Record<string, string> = {
  vllm: "vllm",
  "1cat-vllm": "1cat-vllm",
  sglang: "sglang",
  "llama-cpp": "llama-cpp",
  fastllm: "fastllm",
};
const installShow = ref(false);
const installScript = ref("");
const installTool = ref("");
const installAction = ref<"install" | "upgrade" | "uninstall">("install");
const installLabel = ref("");
const installStreamShow = ref(false);
const installStream = ref("");
const installDone = ref<number | null>(null);
const cancelInstall = ref<null | (() => Promise<void>)>(null);

async function askInstall(tool: string, label: string, action: "install" | "upgrade") {
  const pid = current.value?.id;
  if (!pid) return;
  try {
    installScript.value = await api.installPreview(pid, tool);
    installTool.value = tool;
    installAction.value = action;
    installLabel.value = label;
    installShow.value = true;
  } catch (e: any) {
    message.error(
      t("fw.getCmdFailed", { action: actionText(action), msg: e?.message ?? JSON.stringify(e) }),
    );
  }
}

async function askUninstall(fw: string, label: string) {
  const pid = current.value?.id;
  if (!pid) return;
  const tool = `uninstall-${fw}`;
  try {
    installScript.value = await api.installPreview(pid, tool);
    installTool.value = tool;
    installAction.value = "uninstall";
    installLabel.value = label;
    installShow.value = true;
  } catch (e: any) {
    message.error(
      t("fw.getCmdFailed", { action: actionText("uninstall"), msg: e?.message ?? JSON.stringify(e) }),
    );
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
    message.error(
      t("fw.startActionFailed", {
        action: actionText(installAction.value),
        msg: e?.message ?? JSON.stringify(e),
      }),
    );
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
  cancelPull.value?.();
});

// ---------- Docker 镜像 ----------
const dockerInstallerRef = ref<InstanceType<typeof DockerInstaller> | null>(null);
const dockerStatus = ref<DockerStatus | null>(null);
const localImages = ref<LocalImage[]>([]);
const imagesLoading = ref(false);
const customLabel = ref("");
const customImage = ref("");
const pulling = ref(false);
const pullImage = ref("");
const pullShow = ref(false);
const pullStream = ref("");
const pullDone = ref<number | null>(null);
const cancelPull = ref<null | (() => Promise<void>)>(null);

async function loadDocker() {
  const pid = current.value?.id;
  if (!pid || imagesLoading.value) return;
  imagesLoading.value = true;
  try {
    const [st, imgs] = await Promise.all([
      api.checkDocker(pid),
      api.listDockerImages(pid).catch(() => [] as LocalImage[]),
    ]);
    dockerStatus.value = st;
    localImages.value = imgs;
  } catch {
    dockerStatus.value = null;
  } finally {
    imagesLoading.value = false;
  }
}

const canUseDocker = computed(() => !!dockerStatus.value?.usable);

const gpuTagText = computed(() => {
  const s = dockerStatus.value;
  if (!s?.installed || !s.usable) return "";
  if (s.gpuRuntime) return t("fw.gpuReady");
  if (s.gpuRuntimeDetail === "ctk") return t("fw.gpuCtk");
  if (s.gpuRuntimeDetail === "bin") return t("fw.gpuBin");
  return t("fw.gpuMissing");
});

const gpuTesting = ref(false);
const gpuTestShow = ref(false);
const gpuTestOut = ref("");
const gpuTestOk = ref<boolean | null>(null);

async function doGpuTest() {
  const pid = current.value?.id;
  if (!pid || gpuTesting.value) return;
  gpuTesting.value = true;
  try {
    const out = await api.dockerGpuTest(pid);
    gpuTestOut.value = out;
    gpuTestOk.value = !out.includes("NO_NVIDIA_IMAGE") && /EXIT=0\b/.test(out);
    gpuTestShow.value = true;
  } catch (e: any) {
    message.error(t("fw.gpuTestFailed", { msg: e?.message ?? JSON.stringify(e) }));
  } finally {
    gpuTesting.value = false;
  }
}

const builtinImages = computed(() => {
  const p = current.value;
  const onecat = p?.onecatImage?.trim() || FW_META["1cat-vllm"].dockerImage;
  return [
    { label: "1Cat-vLLM", image: onecat, isDefault: true },
    { label: "vLLM", image: "vllm/vllm-openai:latest", isDefault: false },
    { label: "SGLang", image: "lmsysorg/sglang:latest-cu129", isDefault: false },
    { label: "llama.cpp", image: "ghcr.io/ggml-org/llama.cpp:server-cuda", isDefault: false },
    ...settings.value.customFrameworks.map((c) => ({
      label: c.label,
      image: c.image,
      isDefault: false,
    })),
  ];
});

/** 镜像地址合法性（与后端 docker.rs validate_image 同规则） */
function validateImageName(img: string): string | null {
  const t = img.trim();
  if (!t) return i18n.global.t("fw.imageEmpty");
  if (t.length > 256) return i18n.global.t("fw.imageLong");
  if (!/^[A-Za-z0-9._:/@-]+$/.test(t))
    return i18n.global.t("fw.imageChars");
  return null;
}

/** 添加自定义框架镜像：校验 → 持久化 → 拉取 */
async function doAddCustomFramework() {
  const label = customLabel.value.trim();
  const image = customImage.value.trim();
  if (!label) {
    message.warning(t("fw.reqFwLabel"));
    return;
  }
  if (label.length > 30) {
    message.warning(t("fw.fwLabelLong"));
    return;
  }
  const imgErr = validateImageName(image);
  if (imgErr) {
    message.warning(imgErr);
    return;
  }
  const dup =
    builtinImages.value.find((b) => b.label === label || b.image === image) ??
    frameworkOptions.value.find((o) => o.label === label);
  if (dup) {
    message.warning(t("fw.fwDup", { dup: `${dup.label} → ${dup.image}` }));
    return;
  }
  // 先持久化记录，再拉取镜像
  await settingsStore.save({
    customFrameworks: [...settings.value.customFrameworks, { label, image }],
  });
  customLabel.value = "";
  customImage.value = "";
  message.success(t("fw.fwAdded", { label }));
  await doPull(image);
}

function removeCustomFramework(label: string) {
  dialog.warning({
    title: t("fw.removeTitle"),
    content: t("fw.removeContent", { label }),
    positiveText: t("common.remove"),
    negativeText: t("common.cancel"),
    onPositiveClick: async () => {
      await settingsStore.save({
        customFrameworks: settings.value.customFrameworks.filter((c) => c.label !== label),
      });
      message.success(t("fw.removed"));
    },
  });
}

function imageState(img: string): string | null {
  const found = localImages.value.find((i) => i.name === img);
  return found ? (found.size ?? t("fw.imagePulled")) : null;
}

interface ImageRow {
  label: string;
  image: string;
  isDefault: boolean;
  custom: boolean;
  state: string | null;
}

const imageRows = computed<ImageRow[]>(() =>
  builtinImages.value.map((b) => ({
    ...b,
    custom: settings.value.customFrameworks.some((c) => c.label === b.label),
    state: imageState(b.image),
  })),
);

const imageColumns = computed<DataTableColumns<ImageRow>>(() => [
  {
    title: t("fw.colFw"),
    key: "label",
    width: 150,
    render: (r) =>
      h(
        NSpace,
        { size: 4 },
        {
          default: () => [
            r.label,
            ...(r.isDefault
              ? [h(NTag, { size: "small", type: "info" }, { default: () => t("fw.tagDefault") })]
              : []),
          ],
        },
      ),
  },
  { title: t("fw.colImage"), key: "image", ellipsis: { tooltip: true } },
  {
    title: t("fw.colState"),
    key: "state",
    width: 140,
    render: (r) =>
      r.state
        ? h(NTag, { size: "small", type: "success" }, { default: () => t("fw.pulled", { size: r.state }) })
        : h(NTag, { size: "small" }, { default: () => t("fw.notPulled") }),
  },
  {
    title: t("common.actions"),
    key: "actions",
    width: 160,
    render: (r) =>
      h(NSpace, { size: 6 }, {
        default: () => [
          h(SpinButton, {
            size: "small",
            type: "primary",
            ghost: true,
            disabled: !!r.state || pulling.value || !canUseDocker.value,
            loading: pulling.value && pullImage.value === r.image,
            onClick: () => doPull(r.image),
          }, { default: () => t("common.add") }),
          ...(r.custom
            ? [h(NButton, {
                size: "small",
                type: "error",
                ghost: true,
                disabled: pulling.value,
                onClick: () => removeCustomFramework(r.label),
              }, { default: () => t("common.remove") })]
            : []),
        ],
      }),
  },
]);

async function doPull(image: string) {
  const pid = current.value?.id;
  if (!pid || !image.trim() || pulling.value) return;
  // daemon 代理守卫：全局代理启用且 daemon 代理不一致 → 先配置 daemon 再继续拉取
  if (settings.value.proxyEnabled) {
    const want = settings.value.proxyUrl.trim();
    if (want) {
      let st = dockerStatus.value;
      try {
        st = await api.checkDocker(pid);
        dockerStatus.value = st;
      } catch {
        /* 沿用已有状态 */
      }
      const cur = st?.daemonProxy ?? null;
      if (st && cur !== want) {
        dialog.warning({
          title: t("docker.proxyTitle"),
          content: t("fw.proxyDialogContent", { cur: cur ?? t("fw.notConfigured"), want }),
          positiveText: t("fw.configureAndPull"),
          negativeText: t("common.cancel"),
          onPositiveClick: () => {
            pendingPull.value = image.trim();
            dockerInstallerRef.value?.askProxy(pid, st, want);
          },
        });
        return;
      }
    }
  }
  await startPull(pid, image.trim());
}

const pendingPull = ref<string | null>(null);

async function startPull(pid: string, image: string) {
  try {
    const taskId = await api.dockerPullStart(pid, image);
    pullImage.value = image;
    pullStream.value = "";
    pullDone.value = null;
    pullShow.value = true;
    pulling.value = true;
    cancelPull.value = await onTaskStream(
      taskId,
      (c) => {
        pullStream.value += c.data;
      },
      (d) => {
        pullDone.value = d.exitCode;
        pulling.value = false;
        if (d.exitCode === 0) void loadDocker();
      },
    );
  } catch (e: any) {
    message.error(t("fw.pullFailed", { msg: e?.message ?? JSON.stringify(e) }));
  }
}

function closePull() {
  pullShow.value = false;
  cancelPull.value?.();
  cancelPull.value = null;
  pulling.value = false;
}

function askInstallDocker() {
  const p = current.value;
  const st = dockerStatus.value;
  if (p && st) dockerInstallerRef.value?.askInstall(p.id, st);
}

function askAuthorizeDocker() {
  const p = current.value;
  const st = dockerStatus.value;
  if (p && st) dockerInstallerRef.value?.askAuthorize(p.id, st);
}

/** Docker 处理流结束（成功后已消费/取消）：清掉挂起的拉取，避免误触发 */
function onDockerFlowDone() {
  pendingPull.value = null;
}

async function onDockerSuccess() {
  const p = current.value;
  // 先同步取走挂起的拉取，避免 done 事件先清理
  const img = pendingPull.value;
  pendingPull.value = null;
  if (!p) return;
  try {
    await serverStore.disconnect();
    await serverStore.connect(p);
  } catch (e: any) {
    message.error(t("fw.reconnectFailed", { msg: e?.message ?? JSON.stringify(e) }));
  }
  // daemon 代理配置成功 → 刷新状态并继续挂起的拉取
  if (img) {
    void loadDocker();
    await startPull(p.id, img);
  }
}

const visibleParams = computed(() => {
  const specType = form.params.specType as string | undefined;
  return (meta.value?.params ?? []).filter((p) => {
    if (p.nativeOnly && form.mode === "docker") return false;
    if (p.dockerOnly && form.mode === "native") return false;
    // 投机解码：draft 模型仅 dflash/dspark 需要；步数仅启用时显示
    if (p.key === "specDraftModel" && !["draft-dflash", "draft-dspark"].includes(specType ?? "")) return false;
    if (p.key === "specDraftNMax" && (!specType || specType === "none")) return false;
    return true;
  });
});

function paramOptions(p: ParamDef): { label: string; value: string }[] {
  if (p.dynamicOptions === "localGguf") return localGgufs.value;
  return (p.options ?? []).map((o) => ({ label: o, value: o }));
}

function tabParams(tabKey: string): ParamDef[] {
  return visibleParams.value.filter((p) => (p.tab ?? "") === tabKey);
}

function paramPlaceholder(p: ParamDef): string {
  if (pPlaceholder(p)) return pPlaceholder(p);
  if (p.default !== undefined) {
    const d = p.default === true ? t("fw.paramOn") : p.default === false ? t("fw.paramOff") : p.default;
    return t("fw.paramDefault", { d });
  }
  return t("fw.paramEmptyDefault");
}

function paramTooltip(p: ParamDef): string {
  const parts: string[] = [];
  if (p.flag) parts.push(`CLI: ${p.flag}`);
  if (pDesc(p)) parts.push(pDesc(p));
  if (p.default !== undefined) {
    const d = p.default === true ? t("fw.paramOn") : p.default === false ? t("fw.paramOff") : p.default;
    parts.push(t("fw.paramDefaultColon", { d }));
  }
  return parts.join("\n");
}

</script>

<template>
  <div>
    <!-- Docker 镜像 -->
    <n-card size="small" :title="t('fw.dockerCard')" style="margin-bottom: 16px">
      <template #header-extra>
        <n-space size="small">
          <n-tag v-if="dockerStatus?.installed" type="success" size="small">
            {{ dockerStatus.version ?? t("fw.installed") }}
          </n-tag>
          <n-tag v-else type="error" size="small">{{ t("fw.notInstalled") }}</n-tag>
          <n-tag
            v-if="dockerStatus?.installed && !dockerStatus.usable"
            type="warning"
            size="small"
          >
            {{ t("fw.noPerm") }}
          </n-tag>
          <n-tag
            v-if="dockerStatus?.installed && dockerStatus.usable"
            :type="dockerStatus.gpuRuntime ? 'success' : 'warning'"
            size="small"
          >
            {{ gpuTagText }}
          </n-tag>
          <n-button
            v-if="dockerStatus && !dockerStatus.installed"
            size="small"
            type="primary"
            @click="askInstallDocker"
          >
            {{ t("docker.installTitle") }}
          </n-button>
          <n-button
            v-else-if="dockerStatus && !dockerStatus.usable"
            size="small"
            type="warning"
            @click="askAuthorizeDocker"
          >
            {{ t("init.fixDockerAuth") }}
          </n-button>
          <spin-button size="small" :loading="imagesLoading" @click="loadDocker()">
            {{ t("common.refresh") }}
          </spin-button>
          <spin-button
            v-if="dockerStatus?.installed && dockerStatus.usable"
            size="small"
            :loading="gpuTesting"
            @click="doGpuTest"
          >
            {{ t("fw.testGpu") }}
          </spin-button>
        </n-space>
      </template>
      <n-data-table
        :columns="imageColumns"
        :data="imageRows"
        :bordered="false"
        size="small"
      />
      <n-space align="center" style="margin-top: 12px">
        <n-input
          v-model:value="customLabel"
          :placeholder="t('fw.customLabelPh')"
          style="width: 180px"
          :disabled="!canUseDocker"
        />
        <n-input
          v-model:value="customImage"
          :placeholder="t('fw.customImagePh')"
          style="width: 420px"
          :disabled="!canUseDocker"
        />
        <spin-button
          type="primary"
          :loading="pulling && pullImage === customImage"
          :disabled="!customLabel.trim() || !customImage.trim() || pulling || !canUseDocker"
          @click="doAddCustomFramework"
        >
          {{ t("common.add") }}
        </spin-button>
      </n-space>
      <div class="fw-desc" style="margin-top: 8px">
        {{ t("fw.customHint") }}
      </div>
    </n-card>

    <!-- 原生框架管理 -->
    <n-card size="small" :title="t('fw.nativeCard')" style="margin-bottom: 16px">
      <template #header-extra>
        <spin-button size="small" :loading="detecting" @click="current && store.detect(current.id)">
          {{ t("gpu.redetect") }}
        </spin-button>
      </template>
      <n-grid :x-gap="16" :y-gap="16" cols="1 s:2 m:4" responsive="screen">
        <n-grid-item v-for="c in detectionCards" :key="c.fw">
          <n-card size="small" :bordered="true">
            <div class="fw-name">
              {{ c.label }}
              <n-tag v-if="c.installed" type="success" size="small">{{ t("fw.installed") }}</n-tag>
              <n-tag v-else type="default" size="small">{{ t("fw.notInstalled") }}</n-tag>
            </div>
            <div class="fw-desc">{{ c.desc }}</div>
            <div v-if="c.version" class="fw-version">{{ c.version }}</div>
            <n-space size="small" style="margin-top: 8px">
              <n-button
                v-if="c.nativeTool && !c.installed"
                size="tiny"
                @click="askInstall(c.nativeTool, c.label, 'install')"
              >
                {{ t("fw.oneClickInstall") }}
              </n-button>
              <template v-else-if="c.nativeTool && c.installed">
                <n-button size="tiny" @click="askInstall(c.nativeTool, c.label, 'upgrade')">
                  {{ t("fw.act_upgrade") }}
                </n-button>
                <n-button size="tiny" type="error" ghost @click="askUninstall(c.fw, c.label)">
                  {{ t("fw.act_uninstall") }}
                </n-button>
              </template>
            </n-space>
          </n-card>
        </n-grid-item>
      </n-grid>
    </n-card>

    <!-- 实例列表 -->
    <n-card size="small" :title="t('fw.instancesCard')">
      <template #header-extra>
        <n-button type="primary" size="small" @click="openAdd">{{ t("fw.newInst") }}</n-button>
      </template>
      <n-data-table :columns="columns" :data="instances" :bordered="false" size="small" />
      <n-empty v-if="!instances.length" :description="t('fw.noInstances')" style="padding: 24px 0" />
    </n-card>

    <!-- 实例表单 -->
    <n-modal v-model:show="showModal" preset="card" :title="form.id ? t('fw.editInst') : t('fw.newInst')" style="width: 720px">
      <n-form label-placement="left" label-width="132">
        <n-form-item :label="t('fw.name')">
          <n-input v-model:value="form.name" :placeholder="t('fw.namePh')" />
        </n-form-item>
        <n-form-item :label="t('fw.colFw')">
          <n-select
            :value="form.framework"
            :options="frameworkOptions.map((o) => ({ label: o.label, value: o.value }))"
            @update:value="(v: string) => ((form.framework = v), onFrameworkChange())"
          />
        </n-form-item>

        <!-- 自定义框架：简化表单（无参数选项），启动命令由用户填写 -->
        <template v-if="isCustomFw">
          <n-form-item :label="t('fw.modelDir')">
            <n-select
              v-model:value="form.modelPath"
              :options="localModels"
              :loading="modelsLoading"
              filterable
              tag
              :placeholder="t('fw.modelPh')"
            />
          </n-form-item>
          <n-form-item :label="t('fw.port')">
            <n-input-number v-model:value="form.port" :min="1" :max="65535" style="width: 160px" />
          </n-form-item>
          <n-form-item :label="t('fw.image')">
            <n-input v-model:value="form.dockerImage" :placeholder="t('fw.imagePh')" />
          </n-form-item>
          <n-form-item :label="t('fw.startCmd')">
            <n-input
              v-model:value="form.params.customCmd"
              type="textarea"
              :autosize="{ minRows: 3, maxRows: 10 }"
              :placeholder="t('fw.startCmdPh')"
              class="preview"
            />
            <div class="fw-desc" style="margin-top: 4px">
              {{ t("fw.customCmdHint") }}
            </div>
          </n-form-item>
        </template>

        <!-- 内置框架：保留完整参数设置 -->
        <template v-else>
          <!-- 仅原生模式的框架（如 FastLLM，无官方 Docker 镜像）不显示运行方式 -->
          <n-form-item v-if="!meta?.nativeOnly" :label="t('fw.runMode')">
            <n-radio-group v-model:value="form.mode">
              <n-radio-button value="native">{{ t("fw.modeNative") }}</n-radio-button>
              <n-radio-button value="docker">{{ t("fw.modeDocker") }}</n-radio-button>
            </n-radio-group>
          </n-form-item>
          <n-form-item :label="t('fw.modelDir')">
            <n-select
              v-model:value="form.modelPath"
              :options="localModels"
              :loading="modelsLoading"
              filterable
              tag
              :placeholder="t('fw.modelPh')"
            />
          </n-form-item>
          <n-form-item :label="t('fw.port')">
            <n-input-number v-model:value="form.port" :min="1" :max="65535" style="width: 160px" />
          </n-form-item>
          <n-form-item v-if="form.mode === 'docker'" :label="t('fw.image')">
            <n-input
              v-model:value="form.dockerImage"
              :placeholder="builtinImageOf(form.framework)"
            />
            <div v-if="isCustomImage" class="fw-desc" style="margin-top: 4px">
              {{ t("fw.customImageHint") }}
            </div>
          </n-form-item>

          <n-form-item v-if="isCustomImage" :label="t('fw.startArgs')">
            <n-input
              v-model:value="form.params.customCmd"
              type="textarea"
              :autosize="{ minRows: 2, maxRows: 6 }"
              :placeholder="t('fw.startArgsPh', { path: form.modelPath || t('fw.modelPathToken'), port: form.port })"
              class="preview"
            />
          </n-form-item>

          <template v-else>
            <n-tabs v-if="meta?.tabs" type="line" size="small" class="fw-param-tabs">
              <n-tab-pane v-for="t in meta.tabs" :key="t.key" :name="t.key" :tab="tabLabel(t)">
                <n-form-item v-for="p in tabParams(t.key)" :key="p.key">
                  <template #label>
                    <n-tooltip trigger="hover" placement="left">
                      <template #trigger>
                        <span class="param-label">{{ pLabel(p) }}</span>
                      </template>
                      <div class="param-tip">{{ paramTooltip(p) }}</div>
                    </n-tooltip>
                  </template>
                  <n-input
                    v-if="p.type === 'text'"
                    v-model:value="form.params[p.key]"
                    :placeholder="paramPlaceholder(p)"
                  />
                  <n-input-number
                    v-else-if="p.type === 'number'"
                    v-model:value="form.params[p.key]"
                    :placeholder="paramPlaceholder(p)"
                    :step="p.step ?? 1"
                    :min="p.min ?? 0"
                  />
                  <n-switch v-else-if="p.type === 'switch'" v-model:value="form.params[p.key]" />
                  <n-select
                    v-else
                    v-model:value="form.params[p.key]"
                    :options="paramOptions(p)"
                    :placeholder="paramPlaceholder(p)"
                    clearable
                  />
                </n-form-item>
              </n-tab-pane>
            </n-tabs>
            <template v-else>
              <n-form-item v-for="p in visibleParams" :key="p.key" :label="pLabel(p)">
                <n-input
                  v-if="p.type === 'text'"
                  v-model:value="form.params[p.key]"
                  :placeholder="paramPlaceholder(p)"
                />
                <n-input-number
                  v-else-if="p.type === 'number'"
                  v-model:value="form.params[p.key]"
                  :placeholder="paramPlaceholder(p)"
                  :step="p.step ?? 1"
                  :min="p.min ?? 0"
                />
                <n-switch v-else-if="p.type === 'switch'" v-model:value="form.params[p.key]" />
                <n-select
                  v-else
                  v-model:value="form.params[p.key]"
                  :options="paramOptions(p)"
                  :placeholder="paramPlaceholder(p)"
                  clearable
                />
              </n-form-item>
            </template>
          </template>
        </template>

        <n-form-item :label="t('fw.preview')">
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
          <n-button @click="showModal = false">{{ t("common.cancel") }}</n-button>
          <n-button type="primary" @click="onSubmit">{{ t("common.save") }}</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 安装/升级/卸载确认 -->
    <n-modal
      v-model:show="installShow"
      preset="card"
      :title="`${actionText(installAction)} ${installLabel}`"
      style="width: 620px"
    >
      <p style="margin-top: 0; color: #999; font-size: 13px">
        {{ t("fw.execCmdHint") }}
      </p>
      <p v-if="installAction === 'uninstall'" style="color: #d03050; font-size: 13px; margin: 4px 0">
        {{ t("fw.uninstallHint") }}
      </p>
      <StreamLog :text="installScript" max-height="140px" :auto-scroll="false" />
      <template #footer>
        <n-space justify="end">
          <n-button @click="installShow = false">{{ t("common.cancel") }}</n-button>
          <n-button
            :type="installAction === 'uninstall' ? 'error' : 'primary'"
            @click="onInstallConfirm"
          >
            {{ t("fw.startAction", { action: actionText(installAction) }) }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 安装/升级/卸载日志 -->
    <n-modal
      :show="installStreamShow"
      preset="card"
      :title="`${actionText(installAction)} ${t('gpu.logSuffix')}`"
      style="width: 720px"
      :mask-closable="false"
      @close="closeInstall"
    >
      <StreamLog :text="installStream" />
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="installDone != null" :type="installDone === 0 ? 'success' : 'error'">
          {{ t("docker.exitCode", { code: installDone }) }}
        </n-tag>
        <n-button v-if="installDone != null" type="primary" @click="closeInstall">
          {{ installDone === 0 ? t("docker.done") : t("common.close") }}
        </n-button>
      </n-space>
    </n-modal>

    <!-- 镜像拉取日志 -->
    <n-modal
      :show="pullShow"
      preset="card"
      :title="t('fw.pullTitle', { image: pullImage })"
      style="width: 720px"
      :mask-closable="false"
      @close="closePull"
    >
      <StreamLog :text="pullStream" />
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="pullDone != null" :type="pullDone === 0 ? 'success' : 'error'">
          {{ t("docker.exitCode", { code: pullDone }) }}
        </n-tag>
        <n-button v-if="pullDone != null" type="primary" @click="closePull">
          {{ pullDone === 0 ? t("docker.done") : t("common.close") }}
        </n-button>
      </n-space>
    </n-modal>

    <!-- GPU 实测 -->
    <n-modal v-model:show="gpuTestShow" preset="card" :title="t('fw.gpuTestTitle')" style="width: 720px">
      <StreamLog :text="gpuTestOut" :placeholder="t('fw.noOutput')" />
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="gpuTestOk != null" :type="gpuTestOk ? 'success' : 'error'">
          {{ gpuTestOk ? t("fw.gpuOk") : t("fw.gpuNotOk") }}
        </n-tag>
        <n-button type="primary" @click="gpuTestShow = false">{{ t("common.close") }}</n-button>
      </n-space>
    </n-modal>

    <!-- Docker 安装/授权 -->
    <docker-installer
      ref="dockerInstallerRef"
      @success="onDockerSuccess"
      @done="onDockerFlowDone"
    />

    <!-- 日志抽屉 -->
    <n-drawer
      :show="!!logId"
      :width="'66.7vw'"
      @update:show="(v: boolean) => v || store.closeLogs()"
    >
      <n-drawer-content closable>
        <template #header>
          <n-space align="center" justify="space-between" style="width: 100%">
            <span>{{ t("fw.logsTitle") }}</span>
            <n-space align="center">
              <n-checkbox v-model:checked="autoRefreshLogs">{{ t("fw.autoRefresh") }}</n-checkbox>
              <spin-button size="small" :loading="logsLoading" @click="store.refreshLogs()">
                {{ t("common.refresh") }}
              </spin-button>
            </n-space>
          </n-space>
        </template>
        <div class="log-pager">
          <template v-if="loadingEarlier">
            <n-spin size="small" /> {{ t("fw.loadingEarlier") }}
          </template>
          <template v-else-if="loadedLines >= logTotalLines">
            {{ t("fw.allShown", { n: logTotalLines }) }}
          </template>
          <template v-else>
            {{ t("fw.loadedLines", { loaded: loadedLines, total: logTotalLines }) }}
          </template>
        </div>
        <StreamLog
          ref="logStreamRef"
          :text="logs"
          :placeholder="t('fw.empty')"
          :max-height="'calc(100vh - 280px)'"
          @reach-top="onLogReachTop"
        />
      </n-drawer-content>
    </n-drawer>
  </div>
</template>

<style scoped>
.log-pager {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: #999;
  margin-bottom: 8px;
}
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
.fw-param-tabs :deep(.n-tabs-nav) {
  margin-bottom: 4px;
}
.fw-param-tabs :deep(.n-tab-pane) {
  padding-top: 10px;
}
.param-label {
  cursor: default;
}
.param-tip {
  max-width: 340px;
  font-size: 12px;
  line-height: 1.7;
  white-space: pre-line;
}
</style>
