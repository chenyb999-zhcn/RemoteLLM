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
  NSelect,
  NSpace,
  NSwitch,
  useMessage,
} from "naive-ui";
import { useI18n } from "vue-i18n";
import { storeToRefs } from "pinia";
import { useRouter } from "vue-router";
import { useSettingsStore } from "../stores/settings";
import { i18n } from "../i18n";

const settings = useSettingsStore();
const { value } = storeToRefs(settings);
const message = useMessage();
const router = useRouter();
const { t } = useI18n();

const form = reactive({
  defaultModelSource: "modelscope",
  modelDir: "",
  hfEndpoint: "",
  hfToken: "",
  pollSeconds: 3,
  autoConnect: false,
  darkTheme: true,
  language: "zh",
  proxyEnabled: false,
  proxyUrl: "",
  pipIndex: "tuna",
  debMirror: "tuna",
});

const languageOptions = [
  { label: "中文", value: "zh" },
  { label: "English", value: "en" },
];
const pipIndexOptions = [
  { label: t("settings.mirrorTuna"), value: "tuna" },
  { label: t("settings.mirrorAliyun"), value: "aliyun" },
  { label: t("settings.mirrorUstc"), value: "ustc" },
  { label: t("settings.mirrorHuawei"), value: "huawei" },
  { label: t("settings.mirrorTencent"), value: "tencent" },
  { label: t("settings.mirrorPypi"), value: "pypi" },
];
const debMirrorOptions = [
  { label: t("settings.mirrorTuna"), value: "tuna" },
  { label: t("settings.mirrorAliyun"), value: "aliyun" },
  { label: t("settings.mirrorUstc"), value: "ustc" },
  { label: t("settings.mirrorHuawei"), value: "huawei" },
  { label: t("settings.mirrorTencent"), value: "tencent" },
  { label: t("settings.mirrorOfficial"), value: "official" },
];

const saving = ref(false);

function onLanguageChange(lang: string) {
  form.language = lang;
  i18n.global.locale.value = lang as "zh" | "en";
  void settings.save({ language: lang });
}

onMounted(async () => {
  await settings.load();
  form.defaultModelSource = value.value.defaultModelSource || "modelscope";
  form.modelDir = value.value.modelDir;
  form.hfEndpoint = value.value.hfEndpoint;
  form.hfToken = value.value.hfToken;
  form.pollSeconds = Math.min(60, Math.max(1, Math.round(value.value.pollIntervalMs / 1000)));
  form.autoConnect = value.value.autoConnect;
  form.darkTheme = value.value.darkTheme;
  form.language = value.value.language || "zh";
  form.proxyEnabled = value.value.proxyEnabled;
  form.proxyUrl = value.value.proxyUrl;
  form.pipIndex = value.value.pipIndex || "tuna";
  form.debMirror = value.value.debMirror || "tuna";
});

async function onSave() {
  if (form.proxyEnabled) {
    const u = form.proxyUrl.trim();
    if (!u) {
      message.error(t("settings.proxyUrlRequired"));
      return;
    }
    if (!/^(http|https|socks5):\/\/.+/.test(u)) {
      message.error(t("settings.proxyUrlInvalid"));
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
      language: form.language,
      proxyEnabled: form.proxyEnabled,
      proxyUrl: form.proxyUrl.trim(),
      pipIndex: form.pipIndex,
      debMirror: form.debMirror,
    });
    message.success(t("settings.saved"));
  } catch (e: any) {
    message.error(t("common.saveFailed", { msg: e?.message ?? JSON.stringify(e) }));
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="page">
    <n-space vertical :size="16" style="width: 100%">
      <n-card :title="t('settings.general')" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item :label="t('settings.language')">
            <n-select
              :value="form.language"
              :options="languageOptions"
              style="width: 160px"
              @update:value="onLanguageChange"
            />
          </n-form-item>
          <n-form-item :label="t('settings.autoConnect')">
            <n-switch v-model:value="form.autoConnect" />
            <span class="hint">{{ t("settings.autoConnectHint") }}</span>
          </n-form-item>
          <n-form-item :label="t('settings.darkTheme')">
            <n-switch v-model:value="form.darkTheme" />
          </n-form-item>
        </n-form>
      </n-card>

      <n-card :title="t('settings.modelDownload')" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item :label="t('settings.defaultSource')">
            <n-radio-group v-model:value="form.defaultModelSource">
              <n-radio-button value="modelscope">ModelScope</n-radio-button>
              <n-radio-button value="huggingface">Hugging Face</n-radio-button>
            </n-radio-group>
          </n-form-item>
          <n-form-item :label="t('settings.defaultModelDir')">
            <n-input
              v-model:value="form.modelDir"
              :placeholder="t('settings.defaultModelDirPh')"
            />
          </n-form-item>
          <n-form-item :label="t('settings.hfEndpoint')">
            <n-input
              v-model:value="form.hfEndpoint"
              :placeholder="t('settings.hfEndpointPh')"
            />
          </n-form-item>
          <n-form-item label="HF Token">
            <n-input
              v-model:value="form.hfToken"
              type="password"
              show-password-on="click"
              :placeholder="t('settings.hfTokenPh')"
            />
          </n-form-item>
        </n-form>
      </n-card>

      <n-card :title="t('settings.proxyCard')" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item :label="t('settings.enableProxy')">
            <n-switch v-model:value="form.proxyEnabled" />
            <span class="hint">{{ t("settings.proxyHint") }}</span>
          </n-form-item>
          <n-form-item :label="t('settings.proxyUrl')">
            <n-input
              v-model:value="form.proxyUrl"
              :disabled="!form.proxyEnabled"
              :placeholder="t('settings.proxyUrlPh')"
            />
          </n-form-item>
        </n-form>
      </n-card>

      <n-card :title="t('settings.mirrorCard')" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item :label="t('settings.pipIndex')">
            <n-select v-model:value="form.pipIndex" :options="pipIndexOptions" style="width: 220px" />
            <span class="hint">{{ t("settings.pipHint") }}</span>
          </n-form-item>
          <n-form-item :label="t('settings.debMirror')">
            <n-select v-model:value="form.debMirror" :options="debMirrorOptions" style="width: 220px" />
            <span class="hint">{{ t("settings.debHint") }}</span>
          </n-form-item>
        </n-form>
      </n-card>

      <n-card :title="t('settings.dashboard')" size="small">
        <n-form label-placement="left" label-width="110">
          <n-form-item :label="t('settings.pollInterval')">
            <n-input-number v-model:value="form.pollSeconds" :min="1" :max="60" />
            <span class="hint">{{ t("common.seconds") }}</span>
          </n-form-item>
        </n-form>
      </n-card>

      <n-space justify="end">
        <n-button @click="router.back()">{{ t("common.back") }}</n-button>
        <n-button type="primary" :loading="saving" @click="onSave">
          {{ t("settings.saveSettings") }}
        </n-button>
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
