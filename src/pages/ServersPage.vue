<script setup lang="ts">
import { h, onMounted, reactive, ref } from "vue";
import {
  NButton,
  NCard,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NModal,
  NRadioButton,
  NRadioGroup,
  NSpace,
  NDataTable,
  NTag,
  useMessage,
  type FormRules,
  type DataTableColumns,
} from "naive-ui";
import { useRouter } from "vue-router";
import { storeToRefs } from "pinia";
import { useServerStore } from "../stores/server";
import type { ServerProfile } from "../lib/types";

const store = useServerStore();
const { profiles, currentId, connecting } = storeToRefs(store);
const router = useRouter();
const message = useMessage();

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
  advanced: false,
});

const rules: FormRules = {
  name: [{ required: true, message: "请输入名称", trigger: "blur" }],
  host: [{ required: true, message: "请输入 IP 或域名", trigger: "blur" }],
  user: [{ required: true, message: "请输入用户名", trigger: "blur" }],
  password: [
    {
      required: true,
      validator: (_r, v: string) =>
        form.authType === "password" && !v ? new Error("请输入密码") : true,
      trigger: "blur",
    },
  ],
  keyPath: [
    {
      required: true,
      validator: (_r, v: string) =>
        form.authType === "key" && !v ? new Error("请输入私钥文件路径") : true,
      trigger: "blur",
    },
  ],
};

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
    advanced: false,
  });
  showModal.value = true;
}

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
  };
}

async function onSubmit() {
  await formRef.value?.validate();
  const profile = buildProfile();
  await store.saveProfile(profile);
  showModal.value = false;
  message.success("已保存");
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
    message.error(`连接失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

async function onDelete(p: ServerProfile) {
  await store.removeProfile(p.id);
  message.success("已删除");
}

const columns: DataTableColumns<ServerProfile> = [
  { title: "名称", key: "name", width: 140 },
  {
    title: "地址",
    key: "host",
    render: (p) => `${p.host}:${p.port}`,
  },
  { title: "用户", key: "user", width: 100 },
  {
    title: "认证",
    key: "auth",
    width: 90,
    render: (p) =>
      h(
        NTag,
        { size: "small", type: p.auth.type === "key" ? "info" : "default" },
        { default: () => (p.auth.type === "key" ? "密钥" : "密码") },
      ),
  },
  { title: "工作目录", key: "baseDir", width: 160, ellipsis: { tooltip: true } },
  {
    title: "操作",
    key: "actions",
    width: 240,
    render: (p) =>
      h(NSpace, {}, {
        default: () => [
          h(
            NButton,
            {
              size: "small",
              type: p.id === currentId.value ? "primary" : "default",
              disabled: p.id === currentId.value || connecting.value,
              loading: connecting.value && p.id === currentId.value,
              onClick: () => doConnect(p),
            },
            { default: () => (p.id === currentId.value ? "已连接" : "连接") },
          ),
          h(NButton, { size: "small", onClick: () => openEdit(p) }, { default: () => "编辑" }),
          h(NButton, { size: "small", type: "error", onClick: () => onDelete(p) }, { default: () => "删除" }),
        ],
      }),
  },
];

onMounted(() => store.loadProfiles());
</script>

<template>
  <div class="page">
    <n-card title="GPU 服务器" style="width: 100%">
      <template #header-extra>
        <n-space>
          <n-button quaternary @click="router.push('/settings')">设置</n-button>
          <n-button type="primary" @click="openAdd">添加服务器</n-button>
        </n-space>
      </template>
      <n-data-table :columns="columns" :data="profiles" :bordered="false" />
    </n-card>

    <n-modal
      v-model:show="showModal"
      preset="card"
      :title="editingId ? '编辑服务器' : '添加服务器'"
      style="width: 520px"
    >
      <n-form
        ref="formRef"
        :model="form"
        :rules="rules"
        label-placement="left"
        label-width="90"
      >
        <n-form-item label="名称" path="name">
          <n-input v-model:value="form.name" placeholder="如：A100-01" />
        </n-form-item>
        <n-form-item label="主机" path="host">
          <n-input v-model:value="form.host" placeholder="IP 或域名" />
        </n-form-item>
        <n-form-item label="端口" path="port">
          <n-input-number v-model:value="form.port" :min="1" :max="65535" />
        </n-form-item>
        <n-form-item label="用户" path="user">
          <n-input v-model:value="form.user" />
        </n-form-item>
        <n-form-item label="认证方式">
          <n-radio-group v-model:value="form.authType">
            <n-radio-button value="password">密码</n-radio-button>
            <n-radio-button value="key">密钥</n-radio-button>
          </n-radio-group>
        </n-form-item>
        <n-form-item v-if="form.authType === 'password'" label="密码" path="password">
          <n-input v-model:value="form.password" type="password" show-password-on="click" />
        </n-form-item>
        <n-form-item v-else label="私钥路径" path="keyPath">
          <n-input v-model:value="form.keyPath" placeholder="本机私钥文件路径，如 C:\Users\xxx\.ssh\id_ed25519" />
        </n-form-item>
        <n-form-item v-if="form.authType === 'key'" label="私钥口令">
          <n-input v-model:value="form.passphrase" type="password" placeholder="可选" />
        </n-form-item>
        <n-form-item label="工作目录">
          <n-input v-model:value="form.baseDir" placeholder="~/RemoteLLM" />
        </n-form-item>
        <n-form-item label="高级选项">
          <n-button size="small" quaternary type="primary" @click="form.advanced = !form.advanced">
            {{ form.advanced ? "收起 ▲" : "展开 ▼" }}
          </n-button>
        </n-form-item>
        <template v-if="form.advanced">
          <n-form-item label="模型目录">
            <n-input
              v-model:value="form.modelsDir"
              placeholder="留空 = 用全局设置 / baseDir/models"
            />
          </n-form-item>
          <n-form-item label="1Cat 仓库">
            <n-input
              v-model:value="form.onecatRepo"
              placeholder="留空=默认 https://github.com/chenyb999-zhcn/1Cat-vLLM.git"
            />
          </n-form-item>
          <n-form-item label="1Cat 镜像">
            <n-input
              v-model:value="form.onecatImage"
              placeholder="留空=默认 vllm/vllm-openai:latest"
            />
          </n-form-item>
        </template>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showModal = false">取消</n-button>
          <n-button type="primary" @click="onSubmit">保存并连接</n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<style scoped>
.page {
  max-width: 1080px;
  margin: 40px auto;
}
</style>
