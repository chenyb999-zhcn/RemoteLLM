<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import {
  NButton,
  NCard,
  NDataTable,
  NEmpty,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NModal,
  NSpace,
  NSpin,
  NTab,
  NTabs,
  NTag,
  NText,
  useDialog,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useServerStore } from "../stores/server";
import { useSettingsStore } from "../stores/settings";
import { api, fmtBytes, onTaskStream } from "../lib/api";
import type { LocalModel, ModelInfo, ParserLibsStatus, RepoFile } from "../lib/types";
import { useClipboard } from "@vueuse/core";

const store = useServerStore();
const { current } = storeToRefs(store);
const settings = useSettingsStore();
const message = useMessage();
const dialog = useDialog();
const { copy } = useClipboard();

type Source = "modelscope" | "huggingface";
const activeTab = ref<Source>("modelscope");

// 有效模型目录：档案覆盖 > 全局设置 > baseDir/models
const modelsDirLabel = computed(() => {
  const p = current.value;
  if (!p) return "";
  if (p.modelsDir) return p.modelsDir;
  if (settings.value.modelDir) return settings.value.modelDir;
  return `${p.baseDir}/models`;
});

const query = ref("");
const limit = ref(20);
const searching = ref(false);
const results = ref<ModelInfo[]>([]);
const searched = ref(false);

const localModels = ref<LocalModel[]>([]);
const localLoading = ref(false);
const parserLibs = ref<ParserLibsStatus | null>(null);

const dlShow = ref(false);
const dlForm = reactive({
  modelId: "",
  source: "modelscope" as Source,
  dest: "",
  files: [] as string[],
});
const dlFileSizes = ref<RepoFile[]>([]);
const dlTotalSize = computed(() => dlFileSizes.value.reduce((s, f) => s + f.size, 0));

// ---------- 下载前选择文件（精度/分片） ----------
const fileShow = ref(false);
const fileLoading = ref(false);
const fileModel = ref<ModelInfo | null>(null);
const repoFiles = ref<RepoFile[]>([]);
const checkedFiles = ref<string[]>([]);
const fileFilter = ref("");

const filteredRepoFiles = computed(() => {
  const kw = fileFilter.value.trim().toLowerCase();
  if (!kw) return repoFiles.value;
  return repoFiles.value.filter((f) => f.path.toLowerCase().includes(kw));
});

const checkedSize = computed(
  () =>
    repoFiles.value
      .filter((f) => checkedFiles.value.includes(f.path))
      .reduce((s, f) => s + f.size, 0),
);

const fileColumns: DataTableColumns<RepoFile> = [
  { type: "selection" },
  { title: "文件", key: "path", ellipsis: { tooltip: true } },
  { title: "大小", key: "size", width: 110, render: (f) => fmtBytes(f.size) },
];

function fileRowKey(f: RepoFile) {
  return f.path;
}

function onFilesChecked(keys: (string | number)[]) {
  checkedFiles.value = keys.map(String);
}

function openDestModal(m: ModelInfo, files: string[]) {
  dlForm.modelId = m.id;
  dlForm.source = m.source === "huggingface" ? "huggingface" : "modelscope";
  dlForm.files = files;
  dlFileSizes.value = files.length
    ? repoFiles.value.filter((f) => files.includes(f.path))
    : [];
  const base = modelsDirLabel.value || "~/RemoteLLM/models";
  dlForm.dest = `${base}/${m.id.split("/").pop()}`;
  dlShow.value = true;
}

async function openDownload(m: ModelInfo) {
  fileModel.value = m;
  repoFiles.value = [];
  checkedFiles.value = [];
  fileFilter.value = "";
  fileLoading.value = true;
  fileShow.value = true;
  try {
    const token = m.source === "huggingface" ? settings.value.hfToken || null : null;
    repoFiles.value = await api.listRepoFiles(m.source, m.id, token);
    checkedFiles.value = repoFiles.value
      .filter((f) => f.path.toLowerCase().endsWith(".gguf"))
      .map((f) => f.path);
    if (repoFiles.value.length && !checkedFiles.value.length) {
      message.info("该仓库没有 GGUF 文件，请手动勾选需要的文件");
    }
  } catch (e: any) {
    fileShow.value = false;
    dialog.warning({
      title: "获取文件列表失败",
      content: `${e?.message ?? JSON.stringify(e)}\n\n仍可直接下载整个仓库。`,
      positiveText: "下载整个仓库",
      negativeText: "取消",
      onPositiveClick: () => openDestModal(m, []),
    });
  } finally {
    fileLoading.value = false;
  }
}

function onFilesNext() {
  if (!fileModel.value) return;
  const m = fileModel.value;
  fileShow.value = false;
  openDestModal(m, [...checkedFiles.value]);
}

const dlStreamShow = ref(false);
const dlStream = ref("");
const dlDone = ref<number | null>(null);
const cancelTask = ref<null | (() => Promise<void>)>(null);

async function onSearch() {
  if (!query.value.trim() && activeTab.value === "huggingface") {
    message.warning("请输入模型名关键词");
    return;
  }
  searching.value = true;
  searched.value = false;
  try {
    results.value = await api.searchModels(activeTab.value, query.value, limit.value);
    searched.value = true;
  } catch (e: any) {
    message.error(`搜索失败: ${e?.message ?? JSON.stringify(e)}`);
  } finally {
    searching.value = false;
  }
}

async function refreshLocal() {
  const id = current.value?.id;
  if (!id) return;
  localLoading.value = true;
  try {
    localModels.value = await api.listLocalModels(id);
  } catch (e: any) {
    message.error(`获取本地模型失败: ${e?.message ?? JSON.stringify(e)}`);
  } finally {
    localLoading.value = false;
  }
}

async function refreshParserLibs() {
  const id = current.value?.id;
  if (!id) return;
  try {
    parserLibs.value = await api.checkParserLibs(id);
  } catch {
    parserLibs.value = null;
  }
}

async function onDownloadConfirm() {
  const id = current.value?.id;
  if (!id) return;
  dlShow.value = false;
  try {
    const taskId = await api.modelDownloadStart(
      id,
      dlForm.source,
      dlForm.modelId,
      dlForm.dest || null,
      dlForm.source === "huggingface" ? settings.value.hfToken || null : null,
      dlForm.files.length ? dlForm.files : null,
    );
    dlStream.value = "";
    dlDone.value = null;
    dlStreamShow.value = true;
    cancelTask.value = await onTaskStream(
      taskId,
      (c) => {
        dlStream.value += c.data;
      },
      (d) => {
        dlDone.value = d.exitCode;
      },
    );
    message.info("下载任务已启动，日志实时显示在下方");
  } catch (e: any) {
    message.error(`启动下载失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

function closeStream() {
  dlStreamShow.value = false;
  cancelTask.value?.();
  cancelTask.value = null;
  if (dlDone.value === 0) refreshLocal();
}

// ---------- 下载工具安装 ----------
const installShow = ref(false);
const installScript = ref("");
const installTool = ref("");
const installStreamShow = ref(false);
const installStream = ref("");
const installDone = ref<number | null>(null);
const cancelInstall = ref<null | (() => Promise<void>)>(null);

async function askInstall(tool: string, _label: string) {
  const pid = current.value?.id;
  if (!pid) return;
  try {
    installScript.value = await api.installPreview(pid, tool);
    installTool.value = tool;
    installShow.value = true;
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
  if (installDone.value === 0) {
    refreshLocal();
    refreshParserLibs();
  }
}

function doDelete(m: LocalModel) {
  const what =
    m.kind === "gguf-split"
      ? `整组分片文件（${m.note ?? ""}）`
      : m.kind === "hf" || m.kind === "dir"
        ? "整个目录"
        : "该文件";
  dialog.error({
    title: "删除模型",
    content: `将删除 ${m.path}\n（${what}），确认删除？`,
    positiveText: "删除",
    negativeText: "取消",
    style: "color: #e88080",
    onPositiveClick: async () => {
      const id = current.value?.id;
      if (!id) return;
      try {
        const r = await api.modelDelete(id, m.rel);
        message.success(r);
        refreshLocal();
      } catch (e: any) {
        message.error(`删除失败: ${e?.message ?? JSON.stringify(e)}`);
      }
    },
  });
}

function copyPath(m: LocalModel) {
  copy(m.path);
  message.success("路径已复制");
}

const KIND_LABEL: Record<string, string> = {
  gguf: "GGUF",
  "gguf-split": "GGUF 分片",
  safetensors: "safetensors",
  hf: "HF 目录",
  dir: "目录",
};

const resultColumns: DataTableColumns<ModelInfo> = [
  { title: "模型", key: "id", ellipsis: { tooltip: true } },
  {
    title: "下载量",
    key: "downloads",
    width: 100,
    render: (m) => (m.downloads != null ? m.downloads.toLocaleString() : "-"),
  },
  {
    title: "收藏",
    key: "likes",
    width: 90,
    render: (m) => (m.likes != null ? m.likes.toLocaleString() : "-"),
  },
  {
    title: "描述",
    key: "description",
    ellipsis: { tooltip: true },
    render: (m) => m.description ?? "-",
  },
  {
    title: "操作",
    key: "actions",
    width: 100,
    render: (m) =>
      h(
        NButton,
        { size: "small", type: "primary", onClick: () => openDownload(m) },
        { default: () => "下载" },
      ),
  },
];

const localColumns: DataTableColumns<LocalModel> = [
  {
    title: "名称",
    key: "name",
    minWidth: 180,
    ellipsis: { tooltip: true },
    render: (m) => (m.kind === "gguf-split" && m.note ? `${m.name}（${m.note}）` : m.name),
  },
  {
    title: "类型",
    key: "kind",
    width: 100,
    render: (m) => h(NTag, { size: "small" }, { default: () => KIND_LABEL[m.kind] ?? m.kind }),
  },
  {
    title: "大小",
    key: "sizeBytes",
    width: 100,
    render: (m) => (m.sizeBytes != null ? fmtBytes(m.sizeBytes) : "-"),
  },
  { title: "架构", key: "arch", width: 130, ellipsis: { tooltip: true }, render: (m) => m.arch ?? "-" },
  { title: "量化", key: "quant", width: 90, render: (m) => m.quant ?? "-" },
  { title: "参数", key: "params", width: 80, render: (m) => m.params ?? "-" },
  {
    title: "上下文",
    key: "ctx",
    width: 90,
    render: (m) => (m.ctx != null ? m.ctx.toLocaleString() : "-"),
  },
  { title: "相对路径", key: "rel", minWidth: 140, ellipsis: { tooltip: true } },
  {
    title: "操作",
    key: "actions",
    width: 180,
    render: (m) =>
      h(NSpace, { size: 6 }, {
        default: () => [
          h(NButton, { size: "small", onClick: () => copyPath(m) }, { default: () => "复制路径" }),
          h(NButton, { size: "small", type: "error", onClick: () => doDelete(m) }, { default: () => "删除" }),
        ],
      }),
  },
];

onMounted(() => {
  refreshLocal();
  refreshParserLibs();
  settings.load().then(() => {
    activeTab.value = settings.value.defaultModelSource === "huggingface" ? "huggingface" : "modelscope";
  });
});

onBeforeUnmount(() => {
  cancelTask.value?.();
});
</script>

<template>
  <div>
    <n-tabs v-model:value="activeTab" type="line" animated>
      <n-tab name="modelscope" tab="ModelScope 搜索" />
      <n-tab name="huggingface" tab="Hugging Face 搜索" />
    </n-tabs>

    <n-card size="small" title="搜索模型" style="margin-top: 16px">
      <template #header-extra>
        <n-space size="small">
          <n-button size="tiny" @click="askInstall('modelscope', 'modelscope CLI (pip)')">
            安装 modelscope
          </n-button>
          <n-button size="tiny" @click="askInstall('huggingface', 'huggingface-cli (pip)')">
            安装 hf-cli
          </n-button>
        </n-space>
      </template>
      <n-space align="center">
        <n-input
          v-model:value="query"
          :placeholder="activeTab === 'modelscope' ? '完整模型 ID 精确查，如 Qwen/Qwen3-8B；关键词或留空浏览热门' : '输入模型名，如 Qwen/Qwen2.5-7B-Instruct'"
          clearable
          style="width: 380px"
          @keyup.enter="onSearch"
        />
        <n-input-number v-model:value="limit" :min="1" :max="50" style="width: 100px" />
        <n-button type="primary" :loading="searching" @click="onSearch">搜索</n-button>
      </n-space>

      <n-data-table
        v-if="searched"
        :columns="resultColumns"
        :data="results"
        :bordered="false"
        size="small"
        style="margin-top: 12px"
      />
      <n-empty v-else-if="!searching" description="搜索后在这里选择要下载的模型" style="padding: 24px 0" />
    </n-card>

    <n-card size="small" title="本地模型" style="margin-top: 16px">
      <template #header-extra>
        <n-space align="center">
          <n-tag size="small">{{ modelsDirLabel }}</n-tag>
          <n-tag v-if="parserLibs" size="small" :type="parserLibs.gguf && parserLibs.safetensors ? 'success' : 'warning'">
            解析库 {{ parserLibs.gguf && parserLibs.safetensors ? "已装" : "未装" }}
          </n-tag>
          <n-button
            v-if="parserLibs && !(parserLibs.gguf && parserLibs.safetensors)"
            size="small"
            @click="askInstall('parser-libs', '解析库 gguf+safetensors (pip)')"
          >
            安装解析库
          </n-button>
          <n-button size="small" :loading="localLoading" @click="refreshLocal">刷新</n-button>
        </n-space>
      </template>
      <n-data-table
        :columns="localColumns"
        :data="localModels"
        :bordered="false"
        size="small"
        :loading="localLoading"
      />
      <n-empty v-if="!localModels.length && !localLoading" description="服务器 models 目录还没有模型" style="padding: 24px 0" />
    </n-card>

    <!-- 选择要下载的文件 -->
    <n-modal v-model:show="fileShow" preset="card" title="选择要下载的文件" style="width: 780px">
      <div style="display: flex; gap: 10px; align-items: center; margin-bottom: 10px">
        <n-tag size="small" type="info">{{ fileModel?.id }}</n-tag>
        <n-input
          v-model:value="fileFilter"
          placeholder="筛选文件，如 gguf"
          clearable
          size="small"
          style="width: 200px"
        />
        <n-text depth="3" style="font-size: 12px">
          已选 {{ checkedFiles.length }} 个文件，约 {{ fmtBytes(checkedSize) }}
        </n-text>
      </div>
      <n-spin :show="fileLoading" size="small">
        <n-data-table
          v-if="!fileLoading"
          :columns="fileColumns"
          :data="filteredRepoFiles"
          :checked-row-keys="checkedFiles"
          :row-key="fileRowKey"
          :max-height="360"
          size="small"
          :bordered="false"
          @update:checked-row-keys="onFilesChecked"
        />
      </n-spin>
      <template #footer>
        <n-space justify="end">
          <n-button @click="fileShow = false">取消</n-button>
          <n-button type="primary" :disabled="!checkedFiles.length" @click="onFilesNext">
            下一步
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 下载确认 -->
    <n-modal v-model:show="dlShow" preset="card" title="下载模型到服务器" style="width: 560px">
      <n-form label-placement="left" label-width="90">
        <n-form-item label="模型 ID">
          <n-input :value="dlForm.modelId" readonly />
        </n-form-item>
        <n-form-item label="来源">
          <n-tag size="small">{{ dlForm.source === "modelscope" ? "ModelScope" : "Hugging Face" }}</n-tag>
        </n-form-item>
        <n-form-item label="下载文件">
          <div style="font-size: 13px">
            <template v-if="dlForm.files.length">
              共 {{ dlForm.files.length }} 个文件，约 {{ fmtBytes(dlTotalSize) }}
              <div v-if="dlForm.files.length <= 5" style="color: #999; font-size: 12px">
                <div v-for="f in dlForm.files" :key="f">{{ f }}</div>
              </div>
            </template>
            <span v-else style="color: #999">整个仓库（全部文件）</span>
          </div>
        </n-form-item>
        <n-form-item label="保存目录">
          <n-input v-model:value="dlForm.dest" placeholder="留空=默认 models/模型名" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="dlShow = false">取消</n-button>
          <n-button type="primary" @click="onDownloadConfirm">开始下载</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 安装确认 -->
    <n-modal v-model:show="installShow" preset="card" title="安装下载工具" style="width: 620px">
      <p style="margin-top: 0; color: #999; font-size: 13px">将在服务器执行：</p>
      <pre class="dllog" style="height: 120px">{{ installScript }}</pre>
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

    <!-- 下载日志 -->
    <n-modal
      :show="dlStreamShow"
      preset="card"
      title="下载日志"
      style="width: 720px"
      :mask-closable="false"
      @close="closeStream"
    >
      <pre class="dllog">{{ dlStream || "(等待输出...)" }}</pre>
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="dlDone != null" :type="dlDone === 0 ? 'success' : 'error'">
          退出码 {{ dlDone }}
        </n-tag>
        <n-button v-if="dlDone != null" type="primary" @click="closeStream">
          {{ dlDone === 0 ? "完成" : "关闭" }}
        </n-button>
      </n-space>
    </n-modal>
  </div>
</template>

<style scoped>
.dllog {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  background: rgba(0, 0, 0, 0.3);
  padding: 12px;
  border-radius: 6px;
  height: 380px;
  overflow: auto;
}
</style>
