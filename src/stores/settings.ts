import { defineStore } from "pinia";
import { api } from "../lib/api";
import type { AppSettings } from "../lib/types";

const DEFAULTS: AppSettings = {
  defaultModelSource: "modelscope",
  modelDir: "",
  hfEndpoint: "",
  hfToken: "",
  pollIntervalMs: 3000,
  lastProfileId: "",
  autoConnect: false,
  darkTheme: true,
  language: "zh",
  proxyEnabled: false,
  proxyUrl: "",
  pipIndex: "tuna",
  uvPythonMirror: "",
  debMirror: "tuna",
  customFrameworks: [],
};

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    value: { ...DEFAULTS } as AppSettings,
    loaded: false,
  }),
  actions: {
    async load() {
      if (this.loaded) return;
      try {
        this.value = { ...DEFAULTS, ...(await api.getSettings()) };
      } catch {
        this.value = { ...DEFAULTS };
      }
      this.loaded = true;
    },
    async save(next?: Partial<AppSettings>) {
      if (next) Object.assign(this.value, next);
      this.value = await api.saveSettings(this.value);
    },
  },
});
