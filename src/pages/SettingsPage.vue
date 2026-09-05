<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import {
  NButton,
  NCard,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NRadioButton,
  NRadioGroup,
  NSpace,
  NSwitch,
  useMessage,
} from "naive-ui";
import { storeToRefs } from "pinia";
import { useRouter } from "vue-router";
import { useSettingsStore } from "../stores/settings";

const settings = useSettingsStore();
const { value } = storeToRefs(settings);
const message = useMessage();
const router = useRouter();

const form = reactive({
  defaultModelSource: "modelscope",
  modelDir: "",
  hfEndpoint: "",
  hfToken: "",
  pollSeconds: 3,
  autoConnect: false,
  darkTheme: true,
  proxyEnabled: false,
  proxyUrl: "",
});

const saving = ref(false);

onMounted(async () => {
  await settings.load();
  form.defaultModelSource = value.value.defaultModelSource || "modelscope";
  form.modelDir = value.value.modelDir;
  form.hfEndpoint = value.value.hfEndpoint;
  form.hfToken = value.value.hfToken;
  form.pollSeconds = Math.min(60, Math.max(1, Math.round(value.value.pollIntervalMs / 1000)));
  form.autoConnect = value.value.autoConnect;
  form.darkTheme = value.value.darkTheme;
  form.proxyEnabled = value.value.proxyEnabled;
  form.proxyUrl = value.value.proxyUrl;
});

async function onSave() {
  if (form.proxyEnabled) {
    const u = form.proxyUrl.trim();
    if (!u) {
      message.error("启用代理前请填写代理地址");
      return;
    }
    if (!/^(http|https|socks5):\/\/.+/.test(u)) {
      message.error("代理地址需以 http:// 、https:// 或 socks5:// 开头");
      return;
    }
  }
  saving.value = true;
  try {
    await settings.save({
      defaultModelSource: form.defaultModelSource,
      modelDir: form.modelDir.trim(),
      hfEndpoint: form.hfEndpoint.trim().replace(/\/+$/, ""),
      hfToken: form.hfToken.trim(),
      pollIntervalMs: Math.min(60, Math.max(1, form.pollSeconds)) * 1000,
      autoConnect: form.autoConnect,
      darkTheme: form.darkTheme,
      proxyEnabled: form.proxyEnabled,
      proxyUrl: form.proxyUrl.trim(),
    });
    message.success("设置已保存");
  } catch (e: any) {
    message.error(`保存失败: ${e?.message ?? JSON.stringify(e)}`);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="page">
    <n-space vertical :size="16" style="width: 100%">
      <n-card title="模型下载" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item label="默认来源">
            <n-radio-group v-model:value="form.defaultModelSource">
              <n-radio-button value="modelscope">ModelScope</n-radio-button>
              <n-radio-button value="huggingface">Hugging Face</n-radio-button>
            </n-radio-group>
          </n-form-item>
          <n-form-item label="默认模型目录">
            <n-input
              v-model:value="form.modelDir"
              placeholder="留空 = 各服务器 baseDir/models（可单台服务器覆盖）"
            />
          </n-form-item>
          <n-form-item label="HF 镜像端点">
            <n-input
              v-model:value="form.hfEndpoint"
              placeholder="留空 = 官方 huggingface.co，如 https://hf-mirror.com"
            />
          </n-form-item>
          <n-form-item label="HF Token">
            <n-input
              v-model:value="form.hfToken"
              type="password"
              show-password-on="click"
              placeholder="私有模型需要，可留空"
            />
          </n-form-item>
        </n-form>
      </n-card>

      <n-card title="下载代理（服务器侧生效）" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item label="启用代理">
            <n-switch v-model:value="form.proxyEnabled" />
            <span class="hint">模型下载、pip 安装、git clone 走代理；docker 拉镜像需配置 daemon（拉取时会提示）</span>
          </n-form-item>
          <n-form-item label="代理地址">
            <n-input
              v-model:value="form.proxyUrl"
              :disabled="!form.proxyEnabled"
              placeholder="如 http://192.168.1.10:7890"
            />
          </n-form-item>
        </n-form>
      </n-card>

      <n-card title="仪表盘" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item label="轮询间隔">
            <n-input-number v-model:value="form.pollSeconds" :min="1" :max="60" />
            <span class="hint">秒（1-60）</span>
          </n-form-item>
        </n-form>
      </n-card>

      <n-card title="常规" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item label="自动连接">
            <n-switch v-model:value="form.autoConnect" />
            <span class="hint">启动时自动连接上次使用的服务器</span>
          </n-form-item>
          <n-form-item label="深色主题">
            <n-switch v-model:value="form.darkTheme" />
          </n-form-item>
        </n-form>
      </n-card>

      <n-space justify="end">
        <n-button @click="router.back()">返回</n-button>
        <n-button type="primary" :loading="saving" @click="onSave">保存设置</n-button>
      </n-space>
    </n-space>
  </div>
</template>

<style scoped>
.page {
  max-width: 760px;
  margin: 40px auto;
}
.hint {
  margin-left: 8px;
  color: #999;
  font-size: 12px;
}
</style>
