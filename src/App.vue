<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from "vue";
import {
  NButton,
  NConfigProvider,
  NDialogProvider,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NLayoutSider,
  NMenu,
  NMessageProvider,
  NModal,
  NSelect,
  NSpace,
  NTag,
  darkTheme,
  dateZhCN,
  zhCN,
} from "naive-ui";
import { useRouter, useRoute } from "vue-router";
import { storeToRefs } from "pinia";
import { useServerStore } from "./stores/server";
import { useDashboardStore } from "./stores/dashboard";
import { useSettingsStore } from "./stores/settings";
import { api, onTaskStream, startTaskBus } from "./lib/api";
import DockerInstaller from "./components/DockerInstaller.vue";
import StreamLog from "./components/StreamLog.vue";
import type { DockerStatus } from "./lib/types";

const store = useServerStore();
const { current, currentId, connInfo, connecting } = storeToRefs(store);
const dash = useDashboardStore();
const settings = useSettingsStore();
const router = useRouter();
const route = useRoute();

const theme = computed(() => (settings.value.darkTheme ? darkTheme : null));

void startTaskBus();

onMounted(async () => {
  await store.loadProfiles();
  await settings.load();
  if (settings.value.autoConnect && settings.value.lastProfileId) {
    const p = store.profiles.find((x) => x.id === settings.value.lastProfileId);
    if (p) {
      store
        .connect(p)
        .then(() => router.push(`/s/${p.id}`))
        .catch(() => {
          /* 自动连接失败时留在服务器列表页 */
        });
    }
  }
});

const dockerInstallerRef = ref<InstanceType<typeof DockerInstaller> | null>(null);

watch(
  currentId,
  (id) => {
    if (id) {
      dash.start();
      void onConnected(id);
    } else {
      dash.stop();
    }
  },
  { immediate: true },
);

// ---------- 登录后检查：先 Docker 服务，再下载工具 ----------
let checkToken = 0;
let pendingToolCheckToken = 0;

async function onConnected(id: string) {
  const token = ++checkToken;
  let st: DockerStatus | null = null;
  try {
    st = await api.checkDocker(id);
  } catch {
    return; // 检查失败静默忽略
  }
  if (token !== checkToken || currentId.value !== id) return;
  if (st && (!st.installed || !st.usable)) {
    // 需要用户决策；下载工具检查推迟到 Docker 处理完（组件 emit done）
    pendingToolCheckToken = token;
    if (!st.installed) dockerInstallerRef.value?.askInstall(id, st);
    else dockerInstallerRef.value?.askAuthorize(id, st);
    return;
  }
  checkDownloadTools(id);
}

function onDockerDone() {
  if (
    pendingToolCheckToken !== 0 &&
    pendingToolCheckToken === checkToken &&
    currentId.value
  ) {
    pendingToolCheckToken = 0;
    checkDownloadTools(currentId.value);
  }
}

async function onDockerSuccess() {
  const p = store.current;
  if (!p) return;
  // 组变更需要重新登录 SSH 会话才生效
  checkToken++;
  pendingToolCheckToken = 0;
  try {
    await store.disconnect();
    await store.connect(p);
  } catch (e: any) {
    window.alert(`Docker 处理完成后重连失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

// ---------- 登录后检查下载工具，缺失则询问安装 ----------
const toolInstallShow = ref(false);
const toolInstallLog = ref("");
const toolInstallDone = ref<number | null>(null);
const cancelToolInstall = ref<null | (() => Promise<void>)>(null);
const toolCheckShow = ref(false);
const toolCheckMissing = ref<string[]>([]);
const toolCheckProfile = ref("");

async function checkDownloadTools(id: string) {
  let status;
  try {
    status = await api.checkTools(id);
  } catch {
    return; // 检查失败静默忽略
  }
  const missing = [
    ...(status.modelscope ? [] : ["modelscope"]),
    ...(status.huggingface ? [] : ["huggingface"]),
  ];
  if (missing.length === 0) return;
  toolCheckMissing.value = missing;
  toolCheckProfile.value = id;
  toolCheckShow.value = true;
}

function confirmToolInstall() {
  toolCheckShow.value = false;
  runToolInstalls(toolCheckProfile.value, toolCheckMissing.value);
}

async function runToolInstalls(id: string, tools: string[]) {
  toolInstallLog.value = "";
  toolInstallDone.value = null;
  toolInstallShow.value = true;
  for (const tool of tools) {
    try {
      const script = await api.installPreview(id, tool);
      toolInstallLog.value += `\n$ ${script.split("\n")[0]}\n`;
    } catch {
      toolInstallLog.value += `\n$ install ${tool}\n`;
    }
    try {
      const taskId = await api.installStart(id, tool);
      await new Promise<void>((resolve) => {
        onTaskStream(
          taskId,
          (c) => {
            toolInstallLog.value += c.data;
          },
          (d) => {
            toolInstallLog.value += `\n[退出码 ${d.exitCode}]\n`;
            toolInstallDone.value = d.exitCode;
            resolve();
          },
        )
          .then((cancel) => {
            cancelToolInstall.value = cancel;
          })
          .catch(() => resolve());
      });
    } catch (e: any) {
      toolInstallLog.value += `\n[启动失败: ${e?.message ?? JSON.stringify(e)}]\n`;
    }
  }
}

function closeToolInstall() {
  toolInstallShow.value = false;
  cancelToolInstall.value?.();
  cancelToolInstall.value = null;
}

function icon(path: string) {
  return () =>
    h(
      "svg",
      {
        xmlns: "http://www.w3.org/2000/svg",
        viewBox: "0 0 24 24",
        width: "1em",
        height: "1em",
        fill: "currentColor",
      },
      [h("path", { d: path })],
    );
}

const menuOptions = [
  {
    label: "总览",
    key: "dashboard",
    icon: icon(
      "M3 13h8V3H3v10zm0 8h8v-6H3v6zm10 0h8V11h-8v10zm0-18v6h8V3h-8z",
    ),
  },
  {
    label: "初始化检查",
    key: "init",
    icon: icon(
      "M19 3H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm-8.7 14.3-3.6-3.6 1.4-1.4 2.2 2.2 5.2-5.2 1.4 1.4-6.6 6.6zM9 8c0 1.1-.9 2-2 2s-2-.9-2-2 .9-2 2-2 2 .9 2 2z",
    ),
  },
  {
    label: "GPU 管理",
    key: "gpu",
    icon: icon(
      "M15 9H9v6h6V9zm-2 4h-2v-2h2v2zm8-2V9h-2V7c0-1.1-.9-2-2-2h-2V3h-2v2h-2V3H9v2H7c-1.1 0-2 .9-2 2v2H3v2h2v2H3v2h2v2c0 1.1.9 2 2 2h2v2h2v-2h2v2h2v-2h2c1.1 0 2-.9 2-2v-2h2v-2h-2v-2h2zm-4 6H7V7h10v10z",
    ),
  },
  {
    label: "引擎管理",
    key: "frameworks",
    icon: icon("M4 6h16v2H4zm0 5h16v2H4zm0 5h16v2H4z"),
  },
  {
    label: "模型管理",
    key: "models",
    icon: icon(
      "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 17.93c-1.33 0-2.63-.27-3.81-.78-.15-.07-.28-.18-.35-.32-.13-.26-.06-.57.16-.75 1.32-1.08 2.72-2.03 4.23-2.8.3-.15.67-.15.97 0 1.51.77 2.91 1.72 4.23 2.8.22.18.29.49.16.75-.07.14-.2.25-.35.32-1.18.51-2.48.78-3.81.78z",
    ),
  },
  {
    label: "设置",
    key: "settings",
    icon: icon(
      "M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.488.488 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.88c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z",
    ),
  },
];

const selectedKey = computed(() => {
  const p = route.path;
  if (p === "/settings") return "settings";
  if (p.includes("/gpu")) return "gpu";
  if (p.includes("/init")) return "init";
  if (p.includes("/frameworks")) return "frameworks";
  if (p.includes("/models")) return "models";
  return "dashboard";
});

function onMenu(key: string) {
  if (key === "settings") {
    router.push("/settings");
    return;
  }
  if (!current.value) return;
  if (key === "gpu") router.push(`/s/${current.value.id}/gpu`);
  else if (key === "init") router.push(`/s/${current.value.id}/init`);
  else if (key === "frameworks") router.push(`/s/${current.value.id}/frameworks`);
  else if (key === "models") router.push(`/s/${current.value.id}/models`);
  else router.push(`/s/${current.value.id}`);
}

const selectOptions = computed(() =>
  store.profiles.map((p) => ({ label: p.name, value: p.id })),
);

async function onSwitch(id: string) {
  const p = store.profiles.find((x) => x.id === id);
  if (!p || id === currentId.value) return;
  try {
    await store.connect(p);
    router.push(`/s/${id}`);
  } catch (e: any) {
    window.alert(`连接失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

async function onDisconnect() {
  await store.disconnect();
  router.push("/servers");
}
</script>

<template>
  <n-config-provider :theme="theme" :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <n-dialog-provider>
        <router-view v-if="!current" />
        <n-layout v-else has-sider style="height: 100vh">
          <n-layout-sider bordered :width="200">
            <div class="brand">RemoteLLM</div>
            <n-menu :value="selectedKey" :options="menuOptions" @update:value="onMenu" />
          </n-layout-sider>
          <n-layout>
            <n-layout-header bordered class="header">
              <n-space align="center">
                <n-select
                  :value="currentId"
                  :options="selectOptions"
                  style="width: 220px"
                  @update:value="onSwitch"
                />
                <n-tag v-if="connInfo" type="success" size="small">
                  {{ connInfo.user }}@{{ connInfo.host }}
                </n-tag>
                <n-button
                  quaternary
                  size="small"
                  :loading="connecting"
                  @click="onDisconnect"
                >
                  断开
                </n-button>
              </n-space>
            </n-layout-header>
            <n-layout-content content-style="padding: 16px">
              <router-view />
            </n-layout-content>
          </n-layout>
        </n-layout>

        <!-- 下载工具缺失确认 -->
        <n-modal
          :show="toolCheckShow"
          preset="card"
          title="未安装下载工具"
          style="width: 460px"
          :mask-closable="false"
        >
          <p style="margin: 0">
            服务器上缺少：{{ toolCheckMissing.join("，") }}。是否现在安装？
          </p>
          <template #footer>
            <n-space justify="end">
              <n-button @click="toolCheckShow = false">暂不</n-button>
              <n-button type="primary" @click="confirmToolInstall">安装</n-button>
            </n-space>
          </template>
        </n-modal>

        <!-- 下载工具安装日志 -->
        <n-modal
          :show="toolInstallShow"
          preset="card"
          title="安装下载工具"
          style="width: 720px"
          :mask-closable="false"
          @close="closeToolInstall"
        >
          <StreamLog :text="toolInstallLog" />
          <n-space justify="end" style="margin-top: 12px">
            <n-tag
              v-if="toolInstallDone != null"
              :type="toolInstallDone === 0 ? 'success' : 'error'"
            >
              最后退出码 {{ toolInstallDone }}
            </n-tag>
            <n-button v-if="toolInstallDone != null" type="primary" @click="closeToolInstall">
              关闭
            </n-button>
          </n-space>
        </n-modal>

        <!-- Docker 安装/授权（连接后检查触发） -->
        <docker-installer
          ref="dockerInstallerRef"
          @success="onDockerSuccess"
          @done="onDockerDone"
        />
        </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>

<style scoped>
.brand {
  font-size: 18px;
  font-weight: 700;
  padding: 14px 18px 10px;
  letter-spacing: 0.5px;
}
.header {
  display: flex;
  align-items: center;
  height: 56px;
  padding: 0 16px;
}
</style>
