<script setup lang="ts">
import { ref } from "vue";
import { NButton, NInput, NModal, NSpace, NTag, useMessage } from "naive-ui";
import { api, onTaskStream } from "../lib/api";
import type { DockerStatus } from "../lib/types";

const emit = defineEmits<{
  /** 安装/授权退出码 0（调用方应重连以刷新 docker 组权限） */
  (e: "success"): void;
  /** 整个 docker 处理流程结束（取消或日志关闭），可继续后续检查 */
  (e: "done"): void;
}>();

const message = useMessage();

// ---------- 确认弹框 ----------
const confirmShow = ref(false);
const confirmKind = ref<"install" | "authorize" | "proxy">("install");
const confirmScript = ref("");
const confirmStatus = ref<DockerStatus | null>(null);
const confirmProfileId = ref("");

// ---------- sudo 密码弹框 ----------
const passShow = ref(false);
const pass = ref("");
const passErr = ref("");
const passAction = ref<null | (() => void)>(null);

// ---------- 流式日志弹框 ----------
const logShow = ref(false);
const logText = ref("");
const logDone = ref<number | null>(null);
const logTitle = ref("");
const needsReconnect = ref(false);
const cancelLog = ref<null | (() => Promise<void>)>(null);

function sudoModeOf(s: DockerStatus): string {
  if (s.isRoot) return "root";
  if (s.sudoPasswordless) return "sudo";
  return "password";
}

async function runTask(taskId: string, title: string, reconnect: boolean) {
  logText.value = "";
  logDone.value = null;
  logTitle.value = title;
  needsReconnect.value = reconnect;
  logShow.value = true;
  await new Promise<void>((resolve) => {
    onTaskStream(
      taskId,
      (c) => {
        logText.value += c.data;
      },
      (d) => {
        logText.value += `\n[退出码 ${d.exitCode}]\n`;
        logDone.value = d.exitCode;
        if (d.exitCode === 0 && needsReconnect.value) emit("success");
        resolve();
      },
    )
      .then((cancel) => {
        cancelLog.value = cancel;
      })
      .catch(() => resolve());
  });
}

async function start(kind: "install" | "authorize" | "proxy", profileId: string, pwd: string | null) {
  try {
    const taskId =
      kind === "install"
        ? await api.dockerInstallStart(profileId, pwd)
        : kind === "proxy"
          ? await api.dockerProxyStart(profileId, pwd)
          : await api.dockerAuthorizeStart(profileId, pwd);
    await runTask(
      taskId,
      kind === "install"
        ? "Docker 安装日志"
        : kind === "proxy"
          ? "Docker Daemon 代理配置日志"
          : "Docker 授权日志",
      true,
    );
  } catch (e: any) {
    message.error(`启动失败: ${e?.message ?? JSON.stringify(e)}`);
    emit("done");
  }
}

function confirmStart() {
  const kind = confirmKind.value;
  const profileId = confirmProfileId.value;
  const status = confirmStatus.value;
  confirmShow.value = false;
  if (!status || !profileId) return;
  if (sudoModeOf(status) === "password") {
    pass.value = "";
    passErr.value = "";
    passAction.value = () => {
      if (!pass.value.trim()) {
        passErr.value = "请输入 sudo 密码";
        return;
      }
      passShow.value = false;
      void start(kind, profileId, pass.value.trim());
    };
    passShow.value = true;
  } else {
    void start(kind, profileId, null);
  }
}

async function askInstall(profileId: string, status: DockerStatus) {
  confirmKind.value = "install";
  confirmStatus.value = status;
  confirmProfileId.value = profileId;
  try {
    confirmScript.value = await api.dockerInstallPreview(sudoModeOf(status));
  } catch (e: any) {
    confirmScript.value = String(e?.message ?? e);
  }
  confirmShow.value = true;
}

function askAuthorize(profileId: string, status: DockerStatus) {
  confirmKind.value = "authorize";
  confirmStatus.value = status;
  confirmProfileId.value = profileId;
  const mode = sudoModeOf(status);
  const usermod = 'usermod -aG docker "$(whoami)"';
  confirmScript.value =
    mode === "root"
      ? `systemctl start docker\n${usermod}`
      : mode === "sudo"
        ? `sudo systemctl start docker\nsudo ${usermod}`
        : `<需输入 sudo 密码>\nsudo systemctl start docker\nsudo ${usermod}`;
  confirmShow.value = true;
}

/** 配置/移除 docker daemon 拉取代理（proxyUrl 非空 = 配置，空 = 移除） */
async function askProxy(profileId: string, status: DockerStatus, proxyUrl: string) {
  confirmKind.value = "proxy";
  confirmStatus.value = status;
  confirmProfileId.value = profileId;
  try {
    confirmScript.value = await api.dockerProxyPreview(
      sudoModeOf(status),
      proxyUrl || null,
    );
  } catch (e: any) {
    confirmScript.value = String(e?.message ?? e);
  }
  confirmShow.value = true;
}

function cancelConfirm() {
  confirmShow.value = false;
  emit("done");
}

function closePass() {
  passShow.value = false;
  emit("done");
}

function closeLog() {
  logShow.value = false;
  cancelLog.value?.();
  cancelLog.value = null;
  emit("done");
}

defineExpose({ askInstall, askAuthorize, askProxy });
</script>

<template>
  <!-- 安装/授权确认 -->
  <n-modal
    v-model:show="confirmShow"
    preset="card"
    :title="
      confirmKind === 'install'
        ? '安装 Docker'
        : confirmKind === 'proxy'
          ? '配置 Docker 拉取代理'
          : '授权当前用户使用 Docker'
    "
    style="width: 660px"
  >
    <p style="margin-top: 0; color: #999; font-size: 13px">
      <template v-if="confirmKind === 'install'">
        将在服务器安装 docker.io 与 nvidia-container-toolkit（GPU 运行时），并把当前用户加入 docker 组。将执行：
      </template>
      <template v-else-if="confirmKind === 'proxy'">
        将写入 docker daemon 的 systemd 代理配置并重启 docker（正在运行的容器会中断）。将执行：
      </template>
      <template v-else>
        当前用户没有 Docker 使用权限，将确保 daemon 运行并把用户加入 docker 组：
      </template>
    </p>
    <pre class="dlog">{{ confirmScript }}</pre>
    <template #footer>
      <n-space justify="end">
        <n-button @click="cancelConfirm">取消</n-button>
        <n-button type="primary" @click="confirmStart">
          {{ confirmKind === "install" ? "开始安装" : confirmKind === "proxy" ? "开始配置" : "开始授权" }}
        </n-button>
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
    @close="closePass"
  >
    <p style="margin-top: 0; color: #999; font-size: 13px">
      当前用户无免密 sudo 权限，请输入该用户的 sudo 密码（密码仅本次使用，不会保存）。
    </p>
    <n-input
      v-model:value="pass"
      type="password"
      show-password-on="click"
      placeholder="sudo 密码"
      :status="passErr ? 'error' : undefined"
      @keyup.enter="passAction?.()"
    />
    <div v-if="passErr" class="pass-err">{{ passErr }}</div>
    <template #footer>
      <n-space justify="end">
        <n-button @click="closePass">取消</n-button>
        <n-button type="primary" @click="passAction?.()">确定</n-button>
      </n-space>
    </template>
  </n-modal>

  <!-- 流式日志 -->
  <n-modal
    :show="logShow"
    preset="card"
    :title="logTitle"
    style="width: 760px"
    :mask-closable="false"
    @close="closeLog"
  >
    <pre class="dlog">{{ logText || "(等待输出...)" }}</pre>
    <n-space justify="end" style="margin-top: 12px">
      <n-tag v-if="logDone != null" :type="logDone === 0 ? 'success' : 'error'">
        退出码 {{ logDone }}
      </n-tag>
      <n-button v-if="logDone != null" type="primary" @click="closeLog">
        {{ logDone === 0 ? "完成" : "关闭" }}
      </n-button>
    </n-space>
  </n-modal>
</template>

<style scoped>
.dlog {
  font-family: Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  background: rgba(0, 0, 0, 0.3);
  padding: 12px;
  border-radius: 6px;
  max-height: 320px;
  overflow: auto;
}
.pass-err {
  color: #e88080;
  font-size: 12px;
  margin-top: 6px;
}
</style>
