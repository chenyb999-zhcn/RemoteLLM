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
import { useI18n } from "vue-i18n";
import { useServerStore } from "../stores/server";
import { useSettingsStore } from "../stores/settings";
import { api, fmtBytes, onTaskStream } from "../lib/api";
import { i18n } from "../i18n";
import type { LocalModel, ModelInfo, ParserLibsStatus, RepoFile } from "../lib/types";
import { useClipboard } from "@vueuse/core";
import StreamLog from "../components/StreamLog.vue";

const store = useServerStore();
const { current } = storeToRefs(store);
const settings = useSettingsStore();
const message = useMessage();
const dialog = useDialog();
const { copy } = useClipboard();
const { t } = useI18n();

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

const fileColumns = computed<DataTableColumns<RepoFile>>(() => [
  { type: "selection" },
  { title: t("common.name"), key: "path", ellipsis: { tooltip: true } },
  { title: t("common.size"), key: "size", width: 110, render: (f) => fmtBytes(f.size) },
]);

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
      message.info(t("models.noGguf"));
    }
  } catch (e: any) {
    fileShow.value = false;
    dialog.warning({
      title: t("models.fileListFailed"),
      content: `${e?.message ?? JSON.stringify(e)}\n\n${t("models.fileListFailedHint")}`,
      positiveText: t("models.downloadWhole"),
      negativeText: t("common.cancel"),
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
    message.warning(t("models.needKeyword"));
    return;
  }
  searching.value = true;
  searched.value = false;
  try {
    results.value = await api.searchModels(activeTab.value, query.value, limit.value);
    searched.value = true;
  } catch (e: any) {
    message.error(t("models.searchFailed", { msg: e?.message ?? JSON.stringify(e) }));
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
    message.error(t("models.localFailed", { msg: e?.message ?? JSON.stringify(e) }));
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
    message.info(t("models.dlStarted"));
  } catch (e: any) {
    message.error(t("models.dlStartFailed", { msg: e?.message ?? JSON.stringify(e) }));
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
    message.error(t("init.getCmdFailed", { msg: e?.message ?? JSON.stringify(e) }));
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
    message.error(t("init.startInstallFailed", { msg: e?.message ?? JSON.stringify(e) }));
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
      ? t("models.deleteWhatSplit", { note: m.note ?? "" })
      : m.kind === "hf" || m.kind === "dir"
        ? t("models.deleteWhatDir")
        : t("models.deleteWhatFile");
  dialog.error({
    title: t("models.deleteTitle"),
    content: t("models.deleteContent", { path: m.path, what }),
    positiveText: t("common.delete"),
    negativeText: t("common.cancel"),
    style: "color: #e88080",
    onPositiveClick: async () => {
      const id = current.value?.id;
      if (!id) return;
      try {
        const r = await api.modelDelete(id, m.rel);
        message.success(r);
        refreshLocal();
      } catch (e: any) {
        message.error(t("models.deleteFailed", { msg: e?.message ?? JSON.stringify(e) }));
      }
    },
  });
}

function copyPath(m: LocalModel) {
  copy(m.path);
  message.success(t("models.pathCopied"));
}

function kindLabel(kind: string): string {
  const key = `models.kind_${kind}`;
  return i18n.global.te(key) ? t(key) : kind;
}

const resultColumns = computed<DataTableColumns<ModelInfo>>(() => [
  { title: t("models.colModel"), key: "id", ellipsis: { tooltip: true } },
  {
    title: t("models.colDownloads"),
    key: "downloads",
    width: 100,
    render: (m) =>
      m.downloads != null ? m.downloads.toLocaleString(i18n.global.locale.value) : "-",
  },
  {
    title: t("models.colLikes"),
    key: "likes",
    width: 90,
    render: (m) => (m.likes != null ? m.likes.toLocaleString(i18n.global.locale.value) : "-"),
  },
  {
    title: t("models.colDesc"),
    key: "description",
    ellipsis: { tooltip: true },
    render: (m) => m.description ?? "-",
  },
  {
    title: t("common.actions"),
    key: "actions",
    width: 100,
    render: (m) =>
      h(
        NButton,
        { size: "small", type: "primary", onClick: () => openDownload(m) },
        { default: () => t("models.download") },
      ),
  },
]);

const localColumns = computed<DataTableColumns<LocalModel>>(() => [
  {
    title: t("common.name"),
    key: "name",
    minWidth: 180,
    ellipsis: { tooltip: true },
    render: (m) => (m.kind === "gguf-split" && m.note ? `${m.name}（${m.note}）` : m.name),
  },
  {
    title: t("common.type"),
    key: "kind",
    width: 100,
    render: (m) => h(NTag, { size: "small" }, { default: () => kindLabel(m.kind) }),
  },
  {
    title: t("common.size"),
    key: "sizeBytes",
    width: 100,
    render: (m) => (m.sizeBytes != null ? fmtBytes(m.sizeBytes) : "-"),
  },
  { title: t("models.colArch"), key: "arch", width: 130, ellipsis: { tooltip: true }, render: (m) => m.arch ?? "-" },
  { title: t("models.colQuant"), key: "quant", width: 90, render: (m) => m.quant ?? "-" },
  { title: t("models.colParams"), key: "params", width: 80, render: (m) => m.params ?? "-" },
  {
    title: t("models.colCtx"),
    key: "ctx",
    width: 90,
    render: (m) => (m.ctx != null ? m.ctx.toLocaleString(i18n.global.locale.value) : "-"),
  },
  { title: t("models.colRel"), key: "rel", minWidth: 140, ellipsis: { tooltip: true } },
  {
    title: t("common.actions"),
    key: "actions",
    width: 180,
    render: (m) =>
      h(NSpace, { size: 6 }, {
        default: () => [
          h(NButton, { size: "small", onClick: () => copyPath(m) }, { default: () => t("models.copyPath") }),
          h(NButton, { size: "small", type: "error", onClick: () => doDelete(m) }, { default: () => t("common.delete") }),
        ],
      }),
  },
]);

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
      <n-tab name="modelscope" :tab="t('models.tabMs')" />
      <n-tab name="huggingface" :tab="t('models.tabHf')" />
    </n-tabs>

    <n-card size="small" :title="t('models.searchCard')" style="margin-top: 16px">
      <template #header-extra>
        <n-space size="small">
          <n-button size="tiny" @click="askInstall('modelscope', 'modelscope CLI (pip)')">
            {{ t("init.fixModelscope") }}
          </n-button>
          <n-button size="tiny" @click="askInstall('huggingface', 'huggingface-cli (pip)')">
            {{ t("init.fixHf") }}
          </n-button>
        </n-space>
      </template>
      <n-space align="center">
        <n-input
          v-model:value="query"
          :placeholder="activeTab === 'modelscope' ? t('models.searchPhMs') : t('models.searchPhHf')"
          clearable
          style="width: 380px"
          @keyup.enter="onSearch"
        />
        <n-input-number v-model:value="limit" :min="1" :max="50" style="width: 100px" />
        <n-button type="primary" :loading="searching" @click="onSearch">
          <template #icon><span /></template>
          {{ t("common.search") }}
        </n-button>
      </n-space>

      <n-data-table
        v-if="searched"
        :columns="resultColumns"
        :data="results"
        :bordered="false"
        size="small"
        style="margin-top: 12px"
      />
      <n-empty v-else-if="!searching" :description="t('models.searchEmpty')" style="padding: 24px 0" />
    </n-card>

    <n-card size="small" :title="t('models.localCard')" style="margin-top: 16px">
      <template #header-extra>
        <n-space align="center">
          <n-tag size="small">{{ modelsDirLabel }}</n-tag>
          <n-tag v-if="parserLibs" size="small" :type="parserLibs.gguf && parserLibs.safetensors ? 'success' : 'warning'">
            {{ t("models.parserTag", { state: parserLibs.gguf && parserLibs.safetensors ? t("models.parserInstalled") : t("models.parserMissing") }) }}
          </n-tag>
          <n-button
            v-if="parserLibs && !(parserLibs.gguf && parserLibs.safetensors)"
            size="small"
            @click="askInstall('parser-libs', t('models.parserLibsLabel'))"
          >
            {{ t("init.fixParser") }}
          </n-button>
          <n-button size="small" :loading="localLoading" @click="refreshLocal">
            <template #icon><span /></template>
            {{ t("common.refresh") }}
          </n-button>
        </n-space>
      </template>
      <n-data-table
        :columns="localColumns"
        :data="localModels"
        :bordered="false"
        size="small"
        :loading="localLoading"
      />
      <n-empty v-if="!localModels.length && !localLoading" :description="t('models.localEmpty')" style="padding: 24px 0" />
    </n-card>

    <!-- 选择要下载的文件 -->
    <n-modal v-model:show="fileShow" preset="card" :title="t('models.fileModalTitle')" style="width: 780px">
      <div style="display: flex; gap: 10px; align-items: center; margin-bottom: 10px">
        <n-tag size="small" type="info">{{ fileModel?.id }}</n-tag>
        <n-input
          v-model:value="fileFilter"
          :placeholder="t('models.fileFilterPh')"
          clearable
          size="small"
          style="width: 200px"
        />
        <n-text depth="3" style="font-size: 12px">
          {{ t("models.selectedSummary", { n: checkedFiles.length, size: fmtBytes(checkedSize) }) }}
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
          <n-button @click="fileShow = false">{{ t("common.cancel") }}</n-button>
          <n-button type="primary" :disabled="!checkedFiles.length" @click="onFilesNext">
            {{ t("models.next") }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 下载确认 -->
    <n-modal v-model:show="dlShow" preset="card" :title="t('models.dlModalTitle')" style="width: 560px">
      <n-form label-placement="left" label-width="90">
        <n-form-item :label="t('models.modelId')">
          <n-input :value="dlForm.modelId" readonly />
        </n-form-item>
        <n-form-item :label="t('models.source')">
          <n-tag size="small">{{ dlForm.source === "modelscope" ? "ModelScope" : "Hugging Face" }}</n-tag>
        </n-form-item>
        <n-form-item :label="t('models.dlFiles')">
          <div style="font-size: 13px">
            <template v-if="dlForm.files.length">
              {{ t("models.filesSummary", { n: dlForm.files.length, size: fmtBytes(dlTotalSize) }) }}
              <div v-if="dlForm.files.length <= 5" style="color: #999; font-size: 12px">
                <div v-for="f in dlForm.files" :key="f">{{ f }}</div>
              </div>
            </template>
            <span v-else style="color: #999">{{ t("models.wholeRepo") }}</span>
          </div>
        </n-form-item>
        <n-form-item :label="t('models.dest')">
          <n-input v-model:value="dlForm.dest" :placeholder="t('models.destPh')" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="dlShow = false">{{ t("common.cancel") }}</n-button>
          <n-button type="primary" @click="onDownloadConfirm">{{ t("models.startDownload") }}</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 安装确认 -->
    <n-modal v-model:show="installShow" preset="card" :title="t('app.toolInstallTitle')" style="width: 620px">
      <p style="margin-top: 0; color: #999; font-size: 13px">{{ t("gpu.execHint") }}</p>
      <StreamLog :text="installScript" max-height="120px" :auto-scroll="false" />
      <template #footer>
        <n-space justify="end">
          <n-button @click="installShow = false">{{ t("common.cancel") }}</n-button>
          <n-button type="primary" @click="onInstallConfirm">{{ t("init.startInstall") }}</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 安装日志 -->
    <n-modal
      :show="installStreamShow"
      preset="card"
      :title="t('init.installLog')"
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

    <!-- 下载日志 -->
    <n-modal
      :show="dlStreamShow"
      preset="card"
      :title="t('models.dlLogTitle')"
      style="width: 720px"
      :mask-closable="false"
      @close="closeStream"
    >
      <StreamLog :text="dlStream" />
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="dlDone != null" :type="dlDone === 0 ? 'success' : 'error'">
          {{ t("docker.exitCode", { code: dlDone }) }}
        </n-tag>
        <n-button v-if="dlDone != null" type="primary" @click="closeStream">
          {{ dlDone === 0 ? t("docker.done") : t("common.close") }}
        </n-button>
      </n-space>
    </n-modal>
  </div>
</template>


