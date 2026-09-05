import { defineStore } from "pinia";
import { api } from "../lib/api";
import { useSettingsStore } from "./settings";
import type { ConnInfo, EnvInfo, ServerProfile } from "../lib/types";

export const useServerStore = defineStore("server", {
  state: () => ({
    profiles: [] as ServerProfile[],
    currentId: null as string | null,
    connInfo: null as ConnInfo | null,
    connecting: false,
    env: null as EnvInfo | null,
    envLoading: false,
  }),
  getters: {
    current: (s): ServerProfile | null =>
      s.profiles.find((p) => p.id === s.currentId) ?? null,
  },
  actions: {
    async loadProfiles() {
      this.profiles = await api.listProfiles();
      for (const p of this.profiles) {
        if (await api.isConnected(p.id)) this.currentId = p.id;
      }
    },
    async saveProfile(profile: ServerProfile) {
      this.profiles = await api.saveProfile(profile);
    },
    async removeProfile(id: string) {
      this.profiles = await api.deleteProfile(id);
      if (this.currentId === id) {
        this.currentId = null;
        this.connInfo = null;
        this.env = null;
      }
    },
    async connect(profile: ServerProfile) {
      this.connecting = true;
      try {
        this.connInfo = await api.connect(profile);
        this.currentId = profile.id;
        this.env = null;
        // 记录最后连接的服务器（自动连接用）
        const settings = useSettingsStore();
        await settings.load();
        settings.value.lastProfileId = profile.id;
        await settings.save();
      } finally {
        this.connecting = false;
      }
    },
    async disconnect() {
      if (this.currentId) {
        await api.disconnect(this.currentId);
        this.currentId = null;
        this.connInfo = null;
        this.env = null;
      }
    },
    async refreshEnv() {
      if (!this.currentId) return;
      this.envLoading = true;
      try {
        this.env = await api.envCheck(this.currentId);
      } finally {
        this.envLoading = false;
      }
    },
  },
});
