<script setup lang="ts">
import { computed, h, onMounted } from "vue";
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

const store = useServerStore();
const { current, currentId, connInfo, connecting } = storeToRefs(store);
const router = useRouter();
const route = useRoute();

onMounted(() => store.loadProfiles());

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
    label: "框架管理",
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
];

const selectedKey = computed(() => {
  const p = route.path;
  if (p.includes("/frameworks")) return "frameworks";
  if (p.includes("/models")) return "models";
  return "dashboard";
});

function onMenu(key: string) {
  if (!current.value) return;
  if (key === "frameworks") router.push(`/s/${current.value.id}/frameworks`);
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
  <n-config-provider :theme="darkTheme" :locale="zhCN" :date-locale="dateZhCN">
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
