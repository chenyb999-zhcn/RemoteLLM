<script setup lang="ts">
import { h, computed, onMounted, reactive, ref } from "vue";
import {
  NAlert,
  NButton,
  NCard,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NModal,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NSpace,
  NSpin,
  NDataTable,
  NTag,
  useMessage,
  type FormRules,
  type DataTableColumns,
} from "naive-ui";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { storeToRefs } from "pinia";
import { useServerStore } from "../stores/server";
import { api } from "../lib/api";
import type { ServerProfile, WslCheckResult, WslDistro } from "../lib/types";
import SpinButton from "../components/SpinButton.vue";

const store = useServerStore();
const { profiles, currentId, connecting } = storeToRefs(store);
const router = useRouter();
const message = useMessage();
const { t } = useI18n();

const showModal = ref(false);
const editingId = ref<string | null>(null);
const formRef = ref();

const form = reactive({
  name: "",
  host: "",
  port: 22,
  user: "root",
  authType: "password" as "password" | "key",
  password: "",
  keyPath: "",
  passphrase: "",
  baseDir: "~/RemoteLLM",
  modelsDir: "",
  onecatRepo: "",
  onecatImage: "",
  proxyMode: "follow" as "follow" | "off" | "custom",
  proxyUrl: "",
  advanced: false,
});

const rules = computed<FormRules>(() => ({
  name: [{ required: true, message: t("servers.reqName"), trigger: "blur" }],
  host: [{ required: true, message: t("servers.reqHost"), trigger: "blur" }],
  user: [{ required: true, message: t("servers.reqUser"), trigger: "blur" }],
  password: [
    {
      required: true,
      validator: (_r, v: string) =>
        form.authType === "password" && !v ? new Error(t("servers.reqPassword")) : true,
      trigger: "blur",
    },
  ],
  keyPath: [
    {
      required: true,
      validator: (_r, v: string) =>
        form.authType === "key" && !v ? new Error(t("servers.reqKeyPath")) : true,
      trigger: "blur",
    },
  ],
}));

function openAdd() {
  editingId.value = null;
  Object.assign(form, {
    name: "",
    host: "",
    port: 22,
    user: "root",
    authType: "password",
    password: "",
    keyPath: "",
    passphrase: "",
    baseDir: "~/RemoteLLM",
    modelsDir: "",
    onecatRepo: "",
    onecatImage: "",
    proxyMode: "follow",
    proxyUrl: "",
    advanced: false,
  });
  showModal.value = true;
}

function openEdit(p: ServerProfile) {
  editingId.value = p.id;
  Object.assign(form, {
    name: p.name,
    host: p.host,
    port: p.port,
    user: p.user,
    authType: p.auth.type,
    password: p.auth.type === "password" ? p.auth.password : "",
    keyPath: p.auth.type === "key" ? p.auth.keyPath : "",
    passphrase: p.auth.type === "key" ? (p.auth.passphrase ?? "") : "",
    baseDir: p.baseDir,
    modelsDir: p.modelsDir ?? "",
    onecatRepo: p.onecatRepo ?? "",
    onecatImage: p.onecatImage ?? "",
    proxyMode: p.proxy === null || p.proxy === undefined ? "follow" : p.proxy.trim() === "" ? "off" : "custom",
    proxyUrl: p.proxy ?? "",
    advanced: false,
  });
  showModal.value = true;
}

const proxyModeOptions = computed(() => [
  { label: t("servers.proxyFollow"), value: "follow" },
  { label: t("servers.proxyOff"), value: "off" },
  { label: t("servers.proxyCustom"), value: "custom" },
]);

function buildProfile(): ServerProfile {
  const id = editingId.value ?? crypto.randomUUID();
  return {
    id,
    name: form.name,
    host: form.host,
    port: form.port,
    user: form.user,
    auth:
      form.authType === "password"
        ? { type: "password", password: form.password }
        : { type: "key", keyPath: form.keyPath, passphrase: form.passphrase || null },
    baseDir: form.baseDir,
    modelsDir: form.modelsDir.trim() || null,
    onecatRepo: form.onecatRepo || null,
    onecatImage: form.onecatImage || null,
    proxy:
      form.proxyMode === "follow"
        ? null
        : form.proxyMode === "off"
          ? ""
          : form.proxyUrl.trim() || null,
  };
}

async function onSubmit() {
  await formRef.value?.validate();
  const profile = buildProfile();
  await store.saveProfile(profile);
  showModal.value = false;
  message.success(t("servers.saved"));
  if (!editingId.value || editingId.value === profile.id) {
    // 新增时直接连接
    await doConnect(profile);
  }
}

async function doConnect(p: ServerProfile) {
  try {
    await store.connect(p);
    router.push(`/s/${p.id}`);
  } catch (e: any) {
    message.error(t("app.connectFailed", { msg: e?.message ?? JSON.stringify(e) }));
  }
}

async function onDelete(p: ServerProfile) {
  await store.removeProfile(p.id);
  message.success(t("servers.deleted"));
}

// ---------- 本机 WSL2 一键检测 ----------
const wslShow = ref(false);
const wslLoading = ref(false);
const wslResult = ref<WslCheckResult | null>(null);

async function openWslCheck() {
  wslShow.value = true;
  wslLoading.value = true;
  wslResult.value = null;
  try {
    wslResult.value = await api.wslCheck();
  } catch (e: any) {
    message.error(t("common.loadFailed", { msg: e?.message ?? JSON.stringify(e) }));
    wslShow.value = false;
  } finally {
    wslLoading.value = false;
  }
}

function wslVerdictType(v: string): "success" | "warning" | "error" {
  if (v === "ready") return "success";
  if (v === "not_installed") return "error";
  return "warning";
}

function wslStateText(s: string): string {
  if (s === "Running") return t("wsl.stateRunning");
  if (s === "Stopped") return t("wsl.stateStopped");
  return s;
}

function copyCmd(cmd: string) {
  void navigator.clipboard.writeText(cmd).then(() => {
    message.success(t("common.copied"));
  });
}

/** 「添加为服务器」：关闭检测弹窗 → 打开添加表单并预填 */
function addFromWsl(d: WslDistro) {
  wslShow.value = false;
  openAdd();
  form.name = `WSL: ${d.name}`;
  form.host = d.ip ?? "127.0.0.1";
  // WSL 发行版通常禁用 root SSH，留空由用户填发行版登录用户
  form.user = "";
}

const wslColumns = computed<DataTableColumns<WslDistro>>(() => [
  {
    title: t("wsl.colName"),
    key: "name",
    render: (d) =>
      d.isDefault
        ? `${d.name}  ${t("wsl.defaultTag")}`
        : d.name,
  },
  {
    title: t("wsl.colState"),
    key: "state",
    width: 90,
    render: (d) =>
      h(
        NTag,
        {
          size: "small",
          type: d.state === "Running" ? "success" : d.state === "Stopped" ? "default" : "warning",
        },
        { default: () => wslStateText(d.state) },
      ),
  },
  {
    title: t("wsl.colVersion"),
    key: "version",
    width: 70,
    render: (d) =>
      h(
        NTag,
        {
          size: "small",
          type: d.version === 2 ? "success" : d.version === 1 ? "warning" : "error",
        },
        { default: () => String(d.version) },
      ),
  },
  { title: t("wsl.colIp"), key: "ip", width: 140, render: (d) => d.ip ?? "—" },
  {
    title: t("wsl.colActions"),
    key: "actions",
    width: 130,
    render: (d) =>
      h(NButton, { size: "small", type: "primary", quaternary: true, onClick: () => addFromWsl(d) }, {
        default: () => t("wsl.addServer"),
      }),
  },
]);

const columns = computed<DataTableColumns<ServerProfile>>(() => [
  { title: t("servers.colName"), key: "name", width: 140 },
  {
    title: t("servers.colAddr"),
    key: "host",
    render: (p) => `${p.host}:${p.port}`,
  },
  { title: t("servers.colUser"), key: "user", width: 100 },
  {
    title: t("servers.colAuth"),
    key: "auth",
    width: 90,
    render: (p) =>
      h(
        NTag,
        { size: "small", type: p.auth.type === "key" ? "info" : "default" },
        { default: () => (p.auth.type === "key" ? t("servers.authKey") : t("servers.authPass")) },
      ),
  },
  { title: t("servers.colWorkdir"), key: "baseDir", width: 160, ellipsis: { tooltip: true } },
  {
    title: t("servers.colActions"),
    key: "actions",
    width: 240,
    render: (p) =>
      h(NSpace, {}, {
        default: () => [
          h(
            SpinButton,
            {
              size: "small",
              type: p.id === currentId.value ? "primary" : "default",
              disabled: p.id === currentId.value || connecting.value,
              loading: connecting.value && p.id === currentId.value,
              onClick: () => doConnect(p),
            },
            {
              default: () =>
                p.id === currentId.value ? t("servers.btnConnected") : t("servers.btnConnect"),
            },
          ),
          h(NButton, { size: "small", onClick: () => openEdit(p) }, { default: () => t("servers.btnEdit") }),
          h(NButton, { size: "small", type: "error", onClick: () => onDelete(p) }, { default: () => t("servers.btnDelete") }),
        ],
      }),
  },
]);

onMounted(() => store.loadProfiles());
</script>

<template>
  <div class="page">
    <n-card :title="t('servers.title')" style="width: 100%">
      <template #header-extra>
        <n-space>
          <n-button quaternary @click="router.push('/settings')">
            {{ t("app.menu.settings") }}
          </n-button>
          <n-button quaternary :loading="wslLoading" @click="openWslCheck">
            {{ t("wsl.check") }}
          </n-button>
          <n-button type="primary" @click="openAdd">{{ t("servers.addServer") }}</n-button>
        </n-space>
      </template>
      <n-data-table :columns="columns" :data="profiles" :bordered="false" />
    </n-card>

    <n-modal
      v-model:show="showModal"
      preset="card"
      :title="editingId ? t('servers.editServer') : t('servers.addServer')"
      style="width: 520px"
    >
      <n-form
        ref="formRef"
        :model="form"
        :rules="rules"
        label-placement="left"
        label-width="90"
      >
        <n-form-item :label="t('servers.name')" path="name">
          <n-input v-model:value="form.name" :placeholder="t('servers.namePh')" />
        </n-form-item>
        <n-form-item :label="t('servers.host')" path="host">
          <n-input v-model:value="form.host" :placeholder="t('servers.hostPh')" />
        </n-form-item>
        <n-form-item :label="t('servers.port')" path="port">
          <n-input-number v-model:value="form.port" :min="1" :max="65535" />
        </n-form-item>
        <n-form-item :label="t('servers.user')" path="user">
          <n-input v-model:value="form.user" />
        </n-form-item>
        <n-form-item :label="t('servers.authType')">
          <n-radio-group v-model:value="form.authType">
            <n-radio-button value="password">{{ t("servers.authPass") }}</n-radio-button>
            <n-radio-button value="key">{{ t("servers.authKey") }}</n-radio-button>
          </n-radio-group>
        </n-form-item>
        <n-form-item v-if="form.authType === 'password'" :label="t('servers.authPassword')" path="password">
          <n-input v-model:value="form.password" type="password" show-password-on="click" />
        </n-form-item>
        <n-form-item v-else :label="t('servers.keyPath')" path="keyPath">
          <n-input v-model:value="form.keyPath" :placeholder="t('servers.keyPathPh')" />
        </n-form-item>
        <n-form-item v-if="form.authType === 'key'" :label="t('servers.keyPassphrase')">
          <n-input v-model:value="form.passphrase" type="password" :placeholder="t('servers.passPh')" />
        </n-form-item>
        <n-form-item :label="t('servers.baseDir')">
          <n-input v-model:value="form.baseDir" placeholder="~/RemoteLLM" />
        </n-form-item>
        <n-form-item :label="t('servers.advanced')">
          <n-button size="small" quaternary type="primary" @click="form.advanced = !form.advanced">
            {{ form.advanced ? t("servers.collapse") : t("servers.expand") }}
          </n-button>
        </n-form-item>
        <template v-if="form.advanced">
          <n-form-item :label="t('servers.modelsDir')">
            <n-input
              v-model:value="form.modelsDir"
              :placeholder="t('servers.modelsDirPh')"
            />
          </n-form-item>
          <n-form-item :label="t('servers.onecatRepo')">
            <n-input
              v-model:value="form.onecatRepo"
              :placeholder="t('servers.onecatRepoPh')"
            />
          </n-form-item>
          <n-form-item :label="t('servers.onecatImage')">
            <n-input
              v-model:value="form.onecatImage"
              :placeholder="t('servers.onecatImagePh')"
            />
          </n-form-item>
          <n-form-item :label="t('servers.proxyOverride')">
            <n-select v-model:value="form.proxyMode" :options="proxyModeOptions" />
          </n-form-item>
          <n-form-item v-if="form.proxyMode === 'custom'" :label="t('servers.proxyUrl')">
            <n-input v-model:value="form.proxyUrl" :placeholder="t('servers.proxyUrlPh')" />
          </n-form-item>
        </template>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showModal = false">{{ t("common.cancel") }}</n-button>
          <n-button type="primary" @click="onSubmit">{{ t("servers.saveAndConnect") }}</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- WSL2 一键检测 -->
    <n-modal
      v-model:show="wslShow"
      preset="card"
      :title="t('wsl.title')"
      style="width: 660px"
    >
      <n-spin :show="wslLoading">
        <template v-if="wslResult">
          <n-alert :type="wslVerdictType(wslResult.verdict)" style="margin-bottom: 12px">
            {{ t(`wsl.verdict_${wslResult.verdict}`) }}
            <template v-if="wslResult.wslVersion">
              （WSL {{ wslResult.wslVersion }}）
            </template>
          </n-alert>

          <div v-if="wslResult.features" class="wsl-feats">
            <n-tag
              size="small"
              :type="wslResult.features.wsl ? 'success' : 'error'"
            >
              {{ t("wsl.featWsl") }} {{ wslResult.features.wsl ? "✓" : "✗" }}
            </n-tag>
            <n-tag
              size="small"
              :type="wslResult.features.vmPlatform ? 'success' : 'error'"
            >
              {{ t("wsl.featVmp") }} {{ wslResult.features.vmPlatform ? "✓" : "✗" }}
            </n-tag>
          </div>

          <n-data-table
            :columns="wslColumns"
            :data="wslResult.distros"
            :bordered="false"
            size="small"
            style="margin: 12px 0"
          />

          <template v-if="wslResult.hints.length > 0">
            <div class="wsl-hint-title">{{ t("wsl.fix") }}</div>
            <div v-for="cmd in wslResult.hints" :key="cmd" class="wsl-hint-row">
              <code class="wsl-cmd">{{ cmd }}</code>
              <n-button size="tiny" quaternary @click="copyCmd(cmd)">
                {{ t("common.copyCmd") }}
              </n-button>
            </div>
          </template>

          <div class="wsl-note">{{ t("wsl.note") }}</div>
        </template>
      </n-spin>
    </n-modal>
  </div>
</template>

<style scoped>
.page {
  max-width: 1080px;
  margin: 40px auto;
}
.wsl-feats {
  display: flex;
  gap: 8px;
}
.wsl-hint-title {
  font-size: 12px;
  opacity: 0.7;
  margin: 10px 0 4px;
}
.wsl-hint-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}
.wsl-cmd {
  flex: 1;
  font-size: 12px;
  padding: 3px 8px;
  border-radius: 4px;
  background: rgba(128, 128, 128, 0.15);
  user-select: all;
}
.wsl-note {
  margin-top: 10px;
  font-size: 12px;
  opacity: 0.65;
  line-height: 1.5;
}
</style>