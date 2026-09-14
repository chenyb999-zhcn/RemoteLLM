import { createApp } from "vue";
import { createPinia } from "pinia";
import MiniGaugePage from "./pages/MiniGaugePage.vue";
import { i18n } from "./i18n";
import { useSettingsStore } from "./stores/settings";
import "./style.css";

/**
 * GPU 迷你仪表盘独立入口（第二 WebviewWindow，不走主窗口路由）。
 * 先加载设置（主题/语言/轮询间隔）再挂载。
 */
async function boot() {
  const pinia = createPinia();
  const settings = useSettingsStore(pinia);
  await settings.load();
  const app = createApp(MiniGaugePage);
  app.use(pinia).use(i18n);
  app.mount("#app");
}

void boot();
