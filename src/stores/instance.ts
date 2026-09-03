import { defineStore } from "pinia";
import { api } from "../lib/api";
import type {
  FwDetect,
  InstanceConfig,
  InstanceStatus,
} from "../lib/types";

let refreshBusy = false;

export const useInstanceStore = defineStore("instances", {
  state: () => ({
    instances: [] as InstanceConfig[],
    statuses: {} as Record<string, InstanceStatus>,
    detections: [] as FwDetect[],
    detecting: false,
    starting: {} as Record<string, boolean>,
    stopping: {} as Record<string, boolean>,
    logId: null as string | null,
    logs: "",
    logsLoading: false,
    autoRefreshLogs: true,
  }),
  actions: {
    async load() {
      this.instances = await api.listInstances();
      await this.refreshStatuses();
    },
    async save(cfg: InstanceConfig) {
      this.instances = await api.saveInstance(cfg);
    },
    async remove(id: string) {
      this.instances = await api.deleteInstance(id);
      delete this.statuses[id];
    },
    async detect(profileId: string) {
      this.detecting = true;
      try {
        this.detections = await api.detectFrameworks(profileId);
      } finally {
        this.detecting = false;
      }
    },
    async refreshStatuses() {
      if (refreshBusy) return;
      refreshBusy = true;
      try {
        for (const inst of this.instances) {
          try {
            this.statuses[inst.id] = await api.instanceStatus(inst.id);
          } catch {
            /* 离线实例忽略 */
          }
        }
      } finally {
        refreshBusy = false;
      }
    },
    async start(id: string): Promise<string> {
      this.starting[id] = true;
      try {
        const r = await api.instanceStart(id);
        await this.refreshStatuses();
        return r;
      } finally {
        this.starting[id] = false;
      }
    },
    async stop(id: string): Promise<string> {
      this.stopping[id] = true;
      try {
        const r = await api.instanceStop(id);
        await this.refreshStatuses();
        return r;
      } finally {
        this.stopping[id] = false;
      }
    },
    async openLogs(id: string) {
      this.logId = id;
      await this.refreshLogs();
    },
    async refreshLogs() {
      if (!this.logId) return;
      this.logsLoading = true;
      try {
        this.logs = await api.instanceLogs(this.logId, 300);
      } catch (e: any) {
        this.logs = `加载日志失败: ${e?.message ?? JSON.stringify(e)}`;
      } finally {
        this.logsLoading = false;
      }
    },
    closeLogs() {
      this.logId = null;
      this.logs = "";
    },
  },
});
