import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ConnInfo,
  CmdOutput,
  EnvInfo,
  FwDetect,
  GpuPoll,
  InstanceConfig,
  InstanceStatus,
  LocalModel,
  MetricSample,
  ModelInfo,
  ProcRow,
  ServerProfile,
  StreamChunk,
  StreamDone,
} from "./types";

export const api = {
  listProfiles: () => invoke<ServerProfile[]>("list_profiles"),
  saveProfile: (profile: ServerProfile) =>
    invoke<ServerProfile[]>("save_profile", { profile }),
  deleteProfile: (id: string) =>
    invoke<ServerProfile[]>("delete_profile", { id }),

  connect: (profile: ServerProfile) =>
    invoke<ConnInfo>("ssh_connect", { profile }),
  disconnect: (id: string) => invoke<void>("ssh_disconnect", { id }),
  isConnected: (id: string) => invoke<boolean>("ssh_is_connected", { id }),

  run: (id: string, command: string) =>
    invoke<CmdOutput>("ssh_run", { id, command }),
  runStream: (id: string, command: string, taskId: string) =>
    invoke<number>("ssh_run_stream", { id, command, taskId }),

  envCheck: (id: string) => invoke<EnvInfo>("env_check_cmd", { id }),

  gpuPoll: (id: string) => invoke<GpuPoll>("gpu_poll", { id }),
  gpuProcPoll: (id: string) => invoke<ProcRow[]>("gpu_proc_poll", { id }),
  metricsPoll: (id: string, port: number) =>
    invoke<MetricSample[]>("metrics_poll", { id, port }),

  listInstances: () => invoke<InstanceConfig[]>("list_instances"),
  saveInstance: (cfg: InstanceConfig) =>
    invoke<InstanceConfig[]>("save_instance", { cfg }),
  deleteInstance: (id: string) =>
    invoke<InstanceConfig[]>("delete_instance", { id }),
  detectFrameworks: (profileId: string) =>
    invoke<FwDetect[]>("detect_frameworks", { profileId }),
  previewCommand: (cfg: InstanceConfig) =>
    invoke<string>("preview_command", { cfg }),
  instanceStart: (id: string) => invoke<string>("instance_start", { id }),
  instanceStop: (id: string) => invoke<string>("instance_stop", { id }),
  instanceStatus: (id: string) => invoke<InstanceStatus>("instance_status", { id }),
  instanceLogs: (id: string, lines: number) =>
    invoke<string>("instance_logs", { id, lines }),

  searchModels: (source: string, query: string, limit: number) =>
    invoke<ModelInfo[]>("search_models", { source, query, limit }),
  listLocalModels: (profileId: string) =>
    invoke<LocalModel[]>("list_local_models", { profileId }),
  modelDownloadStart: (
    profileId: string,
    source: string,
    modelId: string,
    dest: string | null,
    token: string | null,
  ) =>
    invoke<string>("model_download_start", {
      profileId,
      source,
      modelId,
      dest,
      token,
    }),
  modelDelete: (profileId: string, name: string) =>
    invoke<string>("model_delete", { profileId, name }),

  installPreview: (profileId: string, tool: string) =>
    invoke<string>("install_preview", { profileId, tool }),
  installStart: (profileId: string, tool: string) =>
    invoke<string>("install_start", { profileId, tool }),
};

export function fmtBytes(bytes?: number | null, digits = 1): string {
  if (bytes == null) return "-";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(digits)} ${units[i]}`;
}

/** 订阅某任务的流式输出；返回取消函数 */
export async function onTaskStream(
  taskId: string,
  onChunk: (c: StreamChunk) => void,
  onDone: (d: StreamDone) => void,
): Promise<() => Promise<void>> {
  const un1: UnlistenFn = await listen<StreamChunk>("task://stream", (e) => {
    if (e.payload.taskId === taskId) onChunk(e.payload);
  });
  const un2: UnlistenFn = await listen<StreamDone>("task://exit", (e) => {
    if (e.payload.taskId === taskId) onDone(e.payload);
  });
  return async () => {
    await un1();
    await un2();
  };
}
