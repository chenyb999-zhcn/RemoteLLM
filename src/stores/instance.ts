import { defineStore } from "pinia";
import { api } from "../lib/api";
import { i18n } from "../i18n";
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
    // 已加载的日志行数（每次向上翻页 +500）
    loadedLines: 500,
    // 日志总行数（tail 行数 >= 该值即认为已到开头）
    logTotalLines: 0,
    loadingEarlier: false,
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
      if (this.detecting) return;
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
      this.loadedLines = 500;
      this.logTotalLines = 0;
      await this.refreshLogs();
    },
    async refreshLogs() {
      if (!this.logId || this.logsLoading) return;
      this.logsLoading = true;
      try {
        const [text, total] = await Promise.all([
          api.instanceLogs(this.logId, this.loadedLines),
          api.instanceLogTotalLines(this.logId),
        ]);
        this.logTotalLines = total;
        this.logs = text;
      } catch (e: any) {
        this.logs = i18n.global.t("fw.loadLogFailed", { msg: e?.message ?? JSON.stringify(e) });
      } finally {
        this.logsLoading = false;
      }
    },
    /** 向上加载更早的 500 行（前置插入，返回是否还有更早内容） */
    async loadEarlier(): Promise<boolean> {
      if (!this.logId || this.loadingEarlier) return false;
      this.loadingEarlier = true;
      try {
        const next = this.loadedLines + 500;
        const [text, total] = await Promise.all([
          api.instanceLogs(this.logId, next),
          api.instanceLogTotalLines(this.logId),
        ]);
        this.logTotalLines = total;
        const hasMore = this.loadedLines < total;
        this.loadedLines = next;
        this.logs = text;
        return hasMore;
      } catch (e: any) {
        this.logs = i18n.global.t("fw.loadLogFailed", { msg: e?.message ?? JSON.stringify(e) });
        return false;
      } finally {
        this.loadingEarlier = false;
      }
    },
    closeLogs() {
      this.logId = null;
      this.logs = "";
      this.loadedLines = 500;
      this.logTotalLines = 0;
    },
  },
});
