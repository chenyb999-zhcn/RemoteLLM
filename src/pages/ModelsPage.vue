<script setup lang="ts">
import { h, onBeforeUnmount, onMounted, reactive, ref } from "vue";
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
  NTab,
  NTabs,
  NTag,
  useDialog,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useServerStore } from "../stores/server";
import { api, onTaskStream } from "../lib/api";
import type { LocalModel, ModelInfo } from "../lib/types";
import { useClipboard } from "@vueuse/core";

const store = useServerStore();
const { current } = storeToRefs(store);
const message = useMessage();
const dialog = useDialog();
const { copy } = useClipboard();

type Source = "modelscope" | "huggingface";
const activeTab = ref<Source>("modelscope");

const query = ref("");
const limit = ref(20);
const hfToken = ref("");
const searching = ref(false);
const results = ref<ModelInfo[]>([]);
const searched = ref(false);

const localModels = ref<LocalModel[]>([]);
const localLoading = ref(false);

const dlShow = ref(false);
const dlForm = reactive({
  modelId: "",
  source: "modelscope" as Source,
  dest: "",
});

const dlStreamShow = ref(false);
const dlStream = ref("");
const dlDone = ref<number | null>(null);
const cancelTask = ref<null | (() => Promise<void>)>(null);

async function onSearch() {
  if (!query.value.trim()) {
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

function openDownload(m: ModelInfo) {
  dlForm.modelId = m.id;
  dlForm.source = m.source === "huggingface" ? "huggingface" : "modelscope";
  const base = current.value ? current.value.baseDir : "~/RemoteLLM";
  dlForm.dest = `${base}/models/${m.id.split("/").pop()}`;
  dlShow.value = true;
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
      dlForm.source === "huggingface" ? hfToken.value || null : null,
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
}

function doDelete(m: LocalModel) {
  dialog.error({
    title: "删除模型",
    content: `将执行 rm -rf ${current.value?.baseDir ?? ""}/models/${m.name}，确认删除？`,
    positiveText: "删除",
    negativeText: "取消",
    style: "color: #e88080",
    onPositiveClick: async () => {
      const id = current.value?.id;
      if (!id) return;
      try {
        const r = await api.modelDelete(id, m.name);
        message.success(r);
        refreshLocal();
      } catch (e: any) {
        message.error(`删除失败: ${e?.message ?? JSON.stringify(e)}`);
      }
    },
  });
}

function copyPath(m: LocalModel) {
  const p = `${current.value?.baseDir ?? ""}/models/${m.name}`;
  copy(p);
  message.success("路径已复制");
}

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
  { title: "目录", key: "name" },
  { title: "大小", key: "size", width: 110, render: (m) => m.size ?? "-" },
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
          placeholder="输入模型名，如 Qwen/Qwen2.5-7B-Instruct"
          clearable
          style="width: 380px"
          @keyup.enter="onSearch"
        />
        <n-input-number v-model:value="limit" :min="1" :max="50" style="width: 100px" />
        <n-input
          v-if="activeTab === 'huggingface'"
          v-model:value="hfToken"
          placeholder="HF Token（可选，私有模型需要）"
          type="password"
          show-password-on="click"
          style="width: 220px"
        />
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
          <n-tag size="small">{{ current?.baseDir }}/models</n-tag>
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

    <!-- 下载确认 -->
    <n-modal v-model:show="dlShow" preset="card" title="下载模型到服务器" style="width: 560px">
      <n-form label-placement="left" label-width="90">
        <n-form-item label="模型 ID">
          <n-input :value="dlForm.modelId" readonly />
        </n-form-item>
        <n-form-item label="来源">
          <n-tag size="small">{{ dlForm.source === "modelscope" ? "ModelScope" : "Hugging Face" }}</n-tag>
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
