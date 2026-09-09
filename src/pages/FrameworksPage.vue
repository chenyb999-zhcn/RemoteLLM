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
import { useServerStore } from "../stores/server";
import { useInstanceStore } from "../stores/instance";
import { useSettingsStore } from "../stores/settings";
import { api, onTaskStream } from "../lib/api";
import DockerInstaller from "../components/DockerInstaller.vue";
import StreamLog from "../components/StreamLog.vue";
import type { DockerStatus, InstanceConfig, LocalImage } from "../lib/types";

import { FW_META, type ParamDef } from "../lib/fwParams";

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
    form.params = { customCmd: `--model ${form.modelPath || "<模型路径>"} --port ${form.port}` };
  } else {
    form.params = defaultParams(form.framework);
  }
}

// ---------- 本地 GGUF 模型列表（投机解码 draft 模型下拉，懒加载） ----------
const localGgufs = ref<{ label: string; value: string }[]>([]);
let ggufLoadedFor: string | null = null;

async function ensureLocalGgufs() {
  const pid = current.value?.id;
  if (!pid || ggufLoadedFor === pid) return;
  try {
    const models = await api.listLocalModels(pid);
    localGgufs.value = models
      .filter((m) => m.kind === "gguf" || m.kind === "gguf-split")
      .map((m) => ({ label: m.name, value: m.path }));
    ggufLoadedFor = pid;
  } catch {
    // 列表加载失败不阻塞表单，draft 模型下拉留空
  }
}

function openAdd() {
  form.id = null;
  form.name = "";
  form.framework = "vllm";
  form.mode = "native";
  form.modelPath = current.value ? `${current.value.baseDir}/models/` : "";
  onFrameworkChange();
  showModal.value = true;
  ensureLocalGgufs();
}

function openEdit(inst: InstanceConfig) {
  form.id = inst.id;
  form.name = inst.name;
  form.framework = inst.framework;
  form.mode = isCustomFwFor(inst.framework) ? "docker" : inst.mode;
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
  ensureLocalGgufs();
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
    message.warning("请输入实例名称");
    return;
  }
  if (!form.modelPath.trim()) {
    message.warning("请输入模型路径");
    return;
  }
  const isDocker = isCustomFw.value || form.mode === "docker";
  if (isDocker && !form.dockerImage.trim()) {
    message.warning("请输入镜像地址");
    return;
  }
  if (isCustomFw.value && !String(form.params.customCmd ?? "").trim()) {
    message.warning("自定义框架需填写启动命令（镜像后的参数）");
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
const installShow = ref(false);
const installScript = ref("");
const installTool = ref("");
const installAction = ref<"install" | "upgrade" | "uninstall">("install");
const installLabel = ref("");
const installStreamShow = ref(false);
const installStream = ref("");
const installDone = ref<number | null>(null);
const cancelInstall = ref<null | (() => Promise<void>)>(null);

const ACTION_TEXT: Record<string, string> = {
  install: "安装",
  upgrade: "升级",
  uninstall: "卸载",
};

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
    message.error(`获取${ACTION_TEXT[action]}命令失败: ${e?.message ?? JSON.stringify(e)}`);
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
    message.error(`获取卸载命令失败: ${e?.message ?? JSON.stringify(e)}`);
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
    message.error(`启动${ACTION_TEXT[installAction.value]}失败: ${e?.message ?? JSON.stringify(e)}`);
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
  if (!pid) return;
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
  if (s.gpuRuntime) return "GPU 运行时就绪";
  if (s.gpuRuntimeDetail === "ctk") return "GPU 工具链已装，docker 未配置";
  if (s.gpuRuntimeDetail === "bin") return "GPU 运行时二进制存在，未注册";
  return "GPU 运行时未安装";
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
    message.error(`GPU 测试失败: ${e?.message ?? JSON.stringify(e)}`);
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
  if (!t) return "镜像地址不能为空";
  if (t.length > 256) return "镜像地址过长（≤256 字符）";
  if (!/^[A-Za-z0-9._:/@-]+$/.test(t))
    return "镜像地址含非法字符（只允许字母数字 . _ / - : @）";
  return null;
}

/** 添加自定义框架镜像：校验 → 持久化 → 拉取 */
async function doAddCustomFramework() {
  const label = customLabel.value.trim();
  const image = customImage.value.trim();
  if (!label) {
    message.warning("请输入框架名");
    return;
  }
  if (label.length > 30) {
    message.warning("框架名过长（≤30 字符）");
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
    message.warning(`框架名或镜像已存在：${dup.label} → ${dup.image}`);
    return;
  }
  // 先持久化记录，再拉取镜像
  await settingsStore.save({
    customFrameworks: [...settings.value.customFrameworks, { label, image }],
  });
  customLabel.value = "";
  customImage.value = "";
  message.success(`框架「${label}」已添加，开始拉取镜像`);
  await doPull(image);
}

function removeCustomFramework(label: string) {
  dialog.warning({
    title: "移除自定义框架",
    content: `确认移除「${label}」？（只删除框架记录，不删除服务器上已拉取的镜像）`,
    positiveText: "移除",
    negativeText: "取消",
    onPositiveClick: async () => {
      await settingsStore.save({
        customFrameworks: settings.value.customFrameworks.filter((c) => c.label !== label),
      });
      message.success("已移除");
    },
  });
}

function imageState(img: string): string | null {
  const found = localImages.value.find((i) => i.name === img);
  return found ? found.size ?? "已拉取" : null;
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
    title: "框架",
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
              ? [h(NTag, { size: "small", type: "info" }, { default: () => "默认" })]
              : []),
          ],
        },
      ),
  },
  { title: "镜像地址", key: "image", ellipsis: { tooltip: true } },
  {
    title: "本地状态",
    key: "state",
    width: 140,
    render: (r) =>
      r.state
        ? h(NTag, { size: "small", type: "success" }, { default: () => `已拉取 ${r.state}` })
        : h(NTag, { size: "small" }, { default: () => "未拉取" }),
  },
  {
    title: "操作",
    key: "actions",
    width: 130,
    render: (r) =>
      h(NSpace, { size: 6 }, {
        default: () => [
          h(NButton, {
            size: "small",
            type: "primary",
            ghost: true,
            disabled: !!r.state || pulling.value || !canUseDocker.value,
            loading: pulling.value && pullImage.value === r.image,
            onClick: () => doPull(r.image),
          }, { default: () => "添加" }),
          ...(r.custom
            ? [h(NButton, {
                size: "small",
                type: "error",
                ghost: true,
                disabled: pulling.value,
                onClick: () => removeCustomFramework(r.label),
              }, { default: () => "移除" })]
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
          title: "配置 Docker 拉取代理",
          content: `Docker daemon 当前代理：${cur ?? "未配置"}。将配置为 ${want}（写入 systemd 配置并重启 docker，正在运行的容器会中断），完成后自动继续拉取。`,
          positiveText: "配置并拉取",
          negativeText: "取消",
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
    message.error(`拉取失败: ${e?.message ?? JSON.stringify(e)}`);
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
    message.error(`重连失败: ${e?.message ?? JSON.stringify(e)}`);
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
  if (p.placeholder) return p.placeholder;
  if (p.default !== undefined) {
    const d = p.default === true ? "启用" : p.default === false ? "关闭" : p.default;
    return `默认 ${d}`;
  }
  return "留空 = 引擎默认";
}

function paramTooltip(p: ParamDef): string {
  const parts: string[] = [];
  if (p.flag) parts.push(`CLI: ${p.flag}`);
  if (p.desc) parts.push(p.desc);
  if (p.default !== undefined) {
    const d = p.default === true ? "启用" : p.default === false ? "关闭" : p.default;
    parts.push(`默认: ${d}`);
  }
  return parts.join("\n");
}

</script>

<template>
  <div>
    <!-- Docker 镜像 -->
    <n-card size="small" title="Docker 镜像" style="margin-bottom: 16px">
      <template #header-extra>
        <n-space size="small">
          <n-tag v-if="dockerStatus?.installed" type="success" size="small">
            {{ dockerStatus.version ?? "已安装" }}
          </n-tag>
          <n-tag v-else type="error" size="small">未安装</n-tag>
          <n-tag
            v-if="dockerStatus?.installed && !dockerStatus.usable"
            type="warning"
            size="small"
          >
            当前用户无权限
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
            安装 Docker
          </n-button>
          <n-button
            v-else-if="dockerStatus && !dockerStatus.usable"
            size="small"
            type="warning"
            @click="askAuthorizeDocker"
          >
            授权
          </n-button>
          <n-button size="small" :loading="imagesLoading" @click="loadDocker()">
            刷新
          </n-button>
          <n-button
            v-if="dockerStatus?.installed && dockerStatus.usable"
            size="small"
            :loading="gpuTesting"
            @click="doGpuTest"
          >
            测试 GPU
          </n-button>
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
          placeholder="框架名，如 my-vllm"
          style="width: 180px"
          :disabled="!canUseDocker"
        />
        <n-input
          v-model:value="customImage"
          placeholder="框架镜像，如 nvcr.io/nvidia/xxx:tag 或 registry:5000/xxx:1.0"
          style="width: 420px"
          :disabled="!canUseDocker"
        />
        <n-button
          type="primary"
          :loading="pulling && pullImage === customImage"
          :disabled="!customLabel.trim() || !customImage.trim() || pulling || !canUseDocker"
          @click="doAddCustomFramework"
        >
          添加
        </n-button>
      </n-space>
      <div class="fw-desc" style="margin-top: 8px">
        添加自定义框架镜像（框架名 + 镜像地址），校验通过后自动拉取并加入下方列表；自定义框架可在新建实例时选择。
      </div>
    </n-card>

    <!-- 原生框架管理 -->
    <n-card size="small" title="原生框架管理" style="margin-bottom: 16px">
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
              <n-button
                v-if="c.nativeTool && !c.installed"
                size="tiny"
                @click="askInstall(c.nativeTool, c.label, 'install')"
              >
                一键安装
              </n-button>
              <template v-else-if="c.nativeTool && c.installed">
                <n-button size="tiny" @click="askInstall(c.nativeTool, c.label, 'upgrade')">
                  升级
                </n-button>
                <n-button size="tiny" type="error" ghost @click="askUninstall(c.fw, c.label)">
                  卸载
                </n-button>
              </template>
            </n-space>
          </n-card>
        </n-grid-item>
      </n-grid>
    </n-card>

    <!-- 实例列表 -->
    <n-card size="small" title="框架实例">
      <template #header-extra>
        <n-button type="primary" size="small" @click="openAdd">新建实例</n-button>
      </template>
      <n-data-table :columns="columns" :data="instances" :bordered="false" size="small" />
      <n-empty v-if="!instances.length" description="还没有实例，点击「新建实例」配置模型与启动参数" style="padding: 24px 0" />
    </n-card>

    <!-- 实例表单 -->
    <n-modal v-model:show="showModal" preset="card" :title="form.id ? '编辑实例' : '新建实例'" style="width: 720px">
      <n-form label-placement="left" label-width="132">
        <n-form-item label="实例名称">
          <n-input v-model:value="form.name" placeholder="如 qwen7b-chat" />
        </n-form-item>
        <n-form-item label="框架">
          <n-select
            :value="form.framework"
            :options="frameworkOptions.map((o) => ({ label: o.label, value: o.value }))"
            @update:value="(v: string) => ((form.framework = v), onFrameworkChange())"
          />
        </n-form-item>

        <!-- 自定义框架：简化表单（无参数选项），启动命令由用户填写 -->
        <template v-if="isCustomFw">
          <n-form-item label="模型路径">
            <n-input
              v-model:value="form.modelPath"
              :placeholder="current ? `${current.baseDir}/models/...` : ''"
            />
          </n-form-item>
          <n-form-item label="端口">
            <n-input-number v-model:value="form.port" :min="1" :max="65535" style="width: 160px" />
          </n-form-item>
          <n-form-item label="镜像">
            <n-input v-model:value="form.dockerImage" placeholder="框架默认镜像" />
          </n-form-item>
          <n-form-item label="启动命令">
            <n-input
              v-model:value="form.params.customCmd"
              type="textarea"
              :autosize="{ minRows: 3, maxRows: 10 }"
              :placeholder="'镜像后的参数，如 --model /mnt/models/xx --port 8000 --tp 1'"
              class="preview"
            />
            <div class="fw-desc" style="margin-top: 4px">
              填写镜像后的启动参数（Docker 容器运行，模型路径按原样挂载进容器）
            </div>
          </n-form-item>
        </template>

        <!-- 内置框架：保留完整参数设置 -->
        <template v-else>
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
            <n-input
              v-model:value="form.dockerImage"
              :placeholder="builtinImageOf(form.framework)"
            />
            <div v-if="isCustomImage" class="fw-desc" style="margin-top: 4px">
              自定义镜像：下方需自行填写启动参数
            </div>
          </n-form-item>

          <n-form-item v-if="isCustomImage" label="启动参数">
            <n-input
              v-model:value="form.params.customCmd"
              type="textarea"
              :autosize="{ minRows: 2, maxRows: 6 }"
              :placeholder="`如 python3 -m sglang.launch_server --model-path ${form.modelPath || '<模型路径>'} --port ${form.port} --tp 1`"
              class="preview"
            />
          </n-form-item>

          <template v-else>
            <n-tabs v-if="meta?.tabs" type="line" size="small" class="fw-param-tabs">
              <n-tab-pane v-for="t in meta.tabs" :key="t.key" :name="t.key" :tab="t.label">
                <n-form-item v-for="p in tabParams(t.key)" :key="p.key">
                  <template #label>
                    <n-tooltip trigger="hover" placement="left">
                      <template #trigger>
                        <span class="param-label">{{ p.label }}</span>
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
              <n-form-item v-for="p in visibleParams" :key="p.key" :label="p.label">
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

    <!-- 安装/升级/卸载确认 -->
    <n-modal
      v-model:show="installShow"
      preset="card"
      :title="`${ACTION_TEXT[installAction]} ${installLabel}`"
      style="width: 620px"
    >
      <p style="margin-top: 0; color: #999; font-size: 13px">
        将在服务器执行以下命令：
      </p>
      <p v-if="installAction === 'uninstall'" style="color: #d03050; font-size: 13px; margin: 4px 0">
        仅移除该引擎本身，保留 torch 等共享依赖，不影响其它引擎运行。
      </p>
      <StreamLog :text="installScript" max-height="140px" :auto-scroll="false" />
      <template #footer>
        <n-space justify="end">
          <n-button @click="installShow = false">取消</n-button>
          <n-button
            :type="installAction === 'uninstall' ? 'error' : 'primary'"
            @click="onInstallConfirm"
          >
            开始{{ ACTION_TEXT[installAction] }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 安装/升级/卸载日志 -->
    <n-modal
      :show="installStreamShow"
      preset="card"
      :title="`${ACTION_TEXT[installAction]}日志`"
      style="width: 720px"
      :mask-closable="false"
      @close="closeInstall"
    >
      <StreamLog :text="installStream" />
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="installDone != null" :type="installDone === 0 ? 'success' : 'error'">
          退出码 {{ installDone }}
        </n-tag>
        <n-button v-if="installDone != null" type="primary" @click="closeInstall">
          {{ installDone === 0 ? "完成" : "关闭" }}
        </n-button>
      </n-space>
    </n-modal>

    <!-- 镜像拉取日志 -->
    <n-modal
      :show="pullShow"
      preset="card"
      :title="`拉取镜像 ${pullImage}`"
      style="width: 720px"
      :mask-closable="false"
      @close="closePull"
    >
      <StreamLog :text="pullStream" />
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="pullDone != null" :type="pullDone === 0 ? 'success' : 'error'">
          退出码 {{ pullDone }}
        </n-tag>
        <n-button v-if="pullDone != null" type="primary" @click="closePull">
          {{ pullDone === 0 ? "完成" : "关闭" }}
        </n-button>
      </n-space>
    </n-modal>

    <!-- GPU 实测 -->
    <n-modal v-model:show="gpuTestShow" preset="card" title="GPU 实测" style="width: 720px">
      <StreamLog :text="gpuTestOut" placeholder="(无输出)" />
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="gpuTestOk != null" :type="gpuTestOk ? 'success' : 'error'">
          {{ gpuTestOk ? "GPU 可用" : "GPU 不可用" }}
        </n-tag>
        <n-button type="primary" @click="gpuTestShow = false">关闭</n-button>
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
            <span>实例日志</span>
            <n-space align="center">
              <n-checkbox v-model:checked="autoRefreshLogs">自动刷新 (5s)</n-checkbox>
              <n-button size="small" :loading="logsLoading" @click="store.refreshLogs()">
                刷新
              </n-button>
            </n-space>
          </n-space>
        </template>
        <div class="log-pager">
          <template v-if="loadingEarlier">
            <n-spin size="small" /> 正在加载更早的日志...
          </template>
          <template v-else-if="loadedLines >= logTotalLines">
            已显示全部 {{ logTotalLines }} 行
          </template>
          <template v-else>
            已加载最后 {{ loadedLines }} / {{ logTotalLines }} 行，向上滚动加载更早
          </template>
        </div>
        <StreamLog
          ref="logStreamRef"
          :text="logs"
          :placeholder="'(空)'"
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
