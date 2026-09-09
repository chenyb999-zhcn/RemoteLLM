<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import {
  NButton,
  NCard,
  NEmpty,
  NInput,
  NModal,
  NResult,
  NSpace,
  NTag,
  useMessage,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useRouter } from "vue-router";
import { useServerStore } from "../stores/server";
import { useSettingsStore } from "../stores/settings";
import { api, onTaskStream } from "../lib/api";
import type { DockerStatus, InitCheckResult, InitItem } from "../lib/types";
import DockerInstaller from "../components/DockerInstaller.vue";
import StreamLog from "../components/StreamLog.vue";

const store = useServerStore();
const { current } = storeToRefs(store);
const settingsStore = useSettingsStore();
const { value: settings } = storeToRefs(settingsStore);
const message = useMessage();
const router = useRouter();

const result = ref<InitCheckResult | null>(null);
const loading = ref(false);

const GROUPS = [
  { key: "sys", label: "基础系统" },
  { key: "gpu", label: "GPU 驱动" },
  { key: "cuda", label: "CUDA 库" },
  { key: "docker", label: "Docker 与 GPU 运行时" },
  { key: "tools", label: "模型工具" },
  { key: "engine", label: "推理引擎" },
];

const FIX_LABEL: Record<string, string> = {
  modelscope: "安装 modelscope",
  huggingface: "安装 hf-cli",
  "parser-libs": "安装解析库",
  "docker-install": "安装 Docker",
  "docker-authorize": "授权",
  "docker-proxy": "配置 daemon 代理",
  "goto-frameworks": "去框架管理添加",
  apt: "安装",
};

function groupItems(key: string): InitItem[] {
  return result.value?.items.filter((i) => i.group === key) ?? [];
}

// docker-install 按钮文案：Docker 已装、仅缺 GPU 运行时 → 更精确的提示
function fixLabel(it: InitItem): string {
  if (it.fix === "docker-install" && it.id === "docker.gpu") {
    return "安装 GPU 容器运行时";
  }
  return FIX_LABEL[it.fix ?? ""] ?? "修复";
}

async function refresh() {
  const pid = current.value?.id;
  if (!pid || loading.value) return;
  loading.value = true;
  try {
    result.value = await api.serverInitCheck(pid);
  } catch (e: any) {
    message.error(`环境检查失败: ${e?.message ?? JSON.stringify(e)}`);
  } finally {
    loading.value = false;
  }
}

function stateTagType(s: string): "success" | "warning" | "error" | "default" {
  if (s === "ok") return "success";
  if (s === "warn") return "warning";
  if (s === "missing") return "error";
  return "default";
}

function stateText(s: string): string {
  if (s === "ok") return "就绪";
  if (s === "warn") return "建议";
  if (s === "missing") return "缺失";
  return "信息";
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success("已复制");
  } catch {
    message.error("复制失败，请手动选择复制");
  }
}

// ---------- 通用日志弹框（pip 安装 / apt 安装共用） ----------
const logShow = ref(false);
const logTitle = ref("安装日志");
const logText = ref("");
const logDone = ref<number | null>(null);
const cancelLog = ref<null | (() => Promise<void>)>(null);

async function runTask(taskId: string, title: string) {
  logTitle.value = title;
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

function closeLog() {
  logShow.value = false;
  cancelLog.value?.();
  cancelLog.value = null;
  if (logDone.value === 0) refresh();
}

// ---------- pip 类工具安装（modelscope / huggingface / parser-libs） ----------
const installShow = ref(false);
const installScript = ref("");
const installTool = ref("");

async function askInstall(tool: string) {
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
    await runTask(taskId, "安装日志");
  } catch (e: any) {
    message.error(`启动安装失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

// ---------- apt 类安装（sudo，白名单包） ----------
const aptShow = ref(false);
const aptScript = ref("");
const aptPkgs = ref<string[]>([]);
const aptMode = ref("sudo");
const passShow = ref(false);
const pass = ref("");
const passErr = ref("");

async function askApt(pkgs: string[]) {
  const pid = current.value?.id;
  if (!pid) return;
  try {
    const mode = await api.sudoModeCheck(pid);
    aptMode.value = mode;
    aptScript.value = await api.aptInstallPreview(mode, pkgs);
    aptPkgs.value = pkgs;
    aptShow.value = true;
  } catch (e: any) {
    message.error(`获取安装命令失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

async function onAptConfirm() {
  const pid = current.value?.id;
  if (!pid) return;
  aptShow.value = false;
  if (aptMode.value === "password") {
    pass.value = "";
    passErr.value = "";
    passShow.value = true;
  } else {
    await runApt(null);
  }
}

async function runApt(password: string | null) {
  const pid = current.value?.id;
  if (!pid) return;
  try {
    const taskId = await api.aptInstallStart(pid, aptPkgs.value, password);
    await runTask(taskId, `apt 安装 ${aptPkgs.value.join(" ")}`);
  } catch (e: any) {
    message.error(`启动安装失败: ${e?.message ?? JSON.stringify(e)}`);
  }
}

function confirmPass() {
  if (!pass.value.trim()) {
    passErr.value = "请输入 sudo 密码";
    return;
  }
  passShow.value = false;
  void runApt(pass.value.trim());
}

// ---------- Docker 类（复用 DockerInstaller：安装/授权/代理） ----------
const dockerInstallerRef = ref<InstanceType<typeof DockerInstaller> | null>(null);

async function askDocker(kind: "install" | "authorize" | "proxy") {
  const pid = current.value?.id;
  if (!pid) return;
  let st: DockerStatus;
  try {
    st = await api.checkDocker(pid);
  } catch (e: any) {
    message.error(`无法获取 docker 状态: ${e?.message ?? JSON.stringify(e)}`);
    return;
  }
  if (kind === "install") dockerInstallerRef.value?.askInstall(pid, st);
  else if (kind === "authorize") dockerInstallerRef.value?.askAuthorize(pid, st);
  else {
    const url = settings.value.proxyUrl.trim();
    if (!url) {
      message.warning("全局代理未配置地址，请先在设置页填写");
      return;
    }
    dockerInstallerRef.value?.askProxy(pid, st, url);
  }
}

async function onDockerDone() {
  refresh();
}

// ---------- 分发 ----------
function onFix(item: InitItem) {
  const fix = item.fix;
  if (!fix) return;
  if (fix === "apt") {
    void askApt(item.fixPkgs ?? []);
  } else if (fix === "modelscope" || fix === "huggingface" || fix === "parser-libs") {
    void askInstall(fix);
  } else if (fix === "docker-install") {
    void askDocker("install");
  } else if (fix === "docker-authorize") {
    void askDocker("authorize");
  } else if (fix === "docker-proxy") {
    void askDocker("proxy");
  } else if (fix === "goto-frameworks") {
    const pid = current.value?.id;
    if (pid) router.push(`/s/${pid}/frameworks`);
  }
}

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
      <h2 style="margin: 0">环境检查</h2>
      <n-space size="small">
        <template v-if="result">
          <n-tag type="success" size="small">就绪 {{ result.okCount }}</n-tag>
          <n-tag type="warning" size="small">建议 {{ result.warnCount }}</n-tag>
          <n-tag type="error" size="small">缺失 {{ result.missingCount }}</n-tag>
        </template>
        <n-button size="small" :loading="loading" @click="refresh">重新检测</n-button>
      </n-space>
    </n-space>

    <n-result
      v-if="!current"
      status="404"
      title="未连接服务器"
      description="请先连接服务器"
    />

    <n-space v-else-if="result" vertical :size="16">
      <n-card v-for="g in GROUPS" :key="g.key" size="small" :title="g.label">
        <div v-for="it in groupItems(g.key)" :key="it.id" class="init-row">
          <n-tag :type="stateTagType(it.state)" size="small" class="init-state">
            {{ stateText(it.state) }}
          </n-tag>
          <span class="init-label">{{ it.label }}</span>
          <span class="init-detail" :title="it.detail ?? undefined">
            {{ it.detail ?? "" }}
          </span>
          <n-space size="small" class="init-actions" :wrap="false">
            <n-button
              v-if="it.fix"
              size="tiny"
              type="primary"
              ghost
              @click="onFix(it)"
            >
              {{ fixLabel(it) }}
            </n-button>
            <n-button
              v-if="it.manual"
              size="tiny"
              quaternary
              @click="copyText(it.manual)"
            >
              复制命令
            </n-button>
          </n-space>
        </div>
        <n-empty
          v-if="!groupItems(g.key).length"
          description="无检查项"
          style="padding: 12px 0"
        />
      </n-card>
    </n-space>

    <n-empty
      v-else-if="!loading"
      description="点击「重新检测」开始检查"
      style="padding: 32px 0"
    />

    <!-- pip 安装确认 -->
    <n-modal v-model:show="installShow" preset="card" title="安装工具" style="width: 620px">
      <p style="margin-top: 0; color: #999; font-size: 13px">将在服务器执行：</p>
      <StreamLog :text="installScript" max-height="120px" :auto-scroll="false" />
      <template #footer>
        <n-space justify="end">
          <n-button @click="installShow = false">取消</n-button>
          <n-button type="primary" @click="onInstallConfirm">开始安装</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- apt 安装确认 -->
    <n-modal v-model:show="aptShow" preset="card" title="安装基础工具（apt）" style="width: 620px">
      <p style="margin-top: 0; color: #999; font-size: 13px">
        将以 sudo 在服务器执行（仅限白名单包 curl/git/build-essential/cmake）：
      </p>
      <StreamLog :text="aptScript" max-height="140px" :auto-scroll="false" />
      <template #footer>
        <n-space justify="end">
          <n-button @click="aptShow = false">取消</n-button>
          <n-button type="primary" @click="onAptConfirm">开始安装</n-button>
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

    <!-- 安装日志 -->
    <n-modal
      :show="logShow"
      preset="card"
      :title="logTitle"
      style="width: 720px"
      :mask-closable="false"
      @close="closeLog"
    >
      <StreamLog :text="logText" />
      <n-space justify="end" style="margin-top: 12px">
        <n-tag v-if="logDone != null" :type="logDone === 0 ? 'success' : 'error'">
          退出码 {{ logDone }}
        </n-tag>
        <n-button v-if="logDone != null" type="primary" @click="closeLog">
          {{ logDone === 0 ? "完成" : "关闭" }}
        </n-button>
      </n-space>
    </n-modal>

    <!-- Docker 安装/授权/代理 -->
    <docker-installer ref="dockerInstallerRef" @done="onDockerDone" />
  </div>
</template>

<style scoped>
.init-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 5px 0;
  border-bottom: 1px dashed rgba(128, 128, 128, 0.15);
  font-size: 13px;
}
.init-row:last-child {
  border-bottom: none;
}
.init-state {
  flex: 0 0 44px;
  justify-content: center;
}
.init-label {
  flex: 0 0 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.init-detail {
  flex: 1 1 auto;
  min-width: 0;
  color: #999;
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.init-actions {
  flex: 0 0 auto;
}
.pass-err {
  color: #e88080;
  font-size: 12px;
  margin-top: 6px;
}
</style>
