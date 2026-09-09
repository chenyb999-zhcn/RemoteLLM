import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppSettings,
  ConnInfo,
  CmdOutput,
  DockerStatus,
  EnvInfo,
  FwDetect,
  GpuPoll,
  GpuQueryResult,
  InitCheckResult,
  InstanceConfig,
  InstanceStatus,
  LocalImage,
  LocalModel,
  MetricSample,
  ModelInfo,
  ParserLibsStatus,
  RepoFile,
  ProcRow,
  ServerProfile,
  StreamChunk,
  StreamDone,
  ToolsStatus,
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
  instanceLogTotalLines: (id: string) => invoke<number>("instance_log_total_lines", { id }),

  searchModels: (source: string, query: string, limit: number) =>
    invoke<ModelInfo[]>("search_models", { source, query, limit }),
  listRepoFiles: (source: string, modelId: string, token: string | null) =>
    invoke<RepoFile[]>("list_repo_files", { source, modelId, token }),
  listLocalModels: (profileId: string) =>
    invoke<LocalModel[]>("list_local_models", { profileId }),
  modelDownloadStart: (
    profileId: string,
    source: string,
    modelId: string,
    dest: string | null,
    token: string | null,
    files: string[] | null,
  ) =>
    invoke<string>("model_download_start", {
      profileId,
      source,
      modelId,
      dest,
      token,
      files,
    }),
  modelDelete: (profileId: string, name: string) =>
    invoke<string>("model_delete", { profileId, name }),
  checkTools: (profileId: string) =>
    invoke<ToolsStatus>("check_download_tools", { profileId }),
  checkParserLibs: (profileId: string) =>
    invoke<ParserLibsStatus>("check_parser_libs", { profileId }),

  installPreview: (profileId: string, tool: string) =>
    invoke<string>("install_preview", { profileId, tool }),
  installStart: (profileId: string, tool: string) =>
    invoke<string>("install_start", { profileId, tool }),

  checkDocker: (profileId: string) => invoke<DockerStatus>("check_docker", { profileId }),
  listDockerImages: (profileId: string) =>
    invoke<LocalImage[]>("list_docker_images", { profileId }),
  dockerInstallPreview: (sudoMode: string) =>
    invoke<string>("docker_install_preview", { sudoMode }),
  dockerInstallStart: (profileId: string, password: string | null) =>
    invoke<string>("docker_install_start", { profileId, password }),
  dockerAuthorizeStart: (profileId: string, password: string | null) =>
    invoke<string>("docker_authorize_start", { profileId, password }),
  dockerProxyPreview: (sudoMode: string, proxyUrl: string | null) =>
    invoke<string>("docker_proxy_preview", { sudoMode, proxyUrl }),
  dockerProxyStart: (profileId: string, password: string | null) =>
    invoke<string>("docker_proxy_start", { profileId, password }),
  dockerPullStart: (profileId: string, image: string) =>
    invoke<string>("docker_pull_start", { profileId, image }),
  dockerGpuTest: (profileId: string) =>
    invoke<string>("docker_gpu_test", { profileId }),

  serverInitCheck: (profileId: string) =>
    invoke<InitCheckResult>("server_init_check", { profileId }),
  sudoModeCheck: (profileId: string) =>
    invoke<string>("sudo_mode_check", { profileId }),
  aptInstallPreview: (sudoMode: string, pkgs: string[]) =>
    invoke<string>("apt_install_preview", { sudoMode, pkgs }),
  aptInstallStart: (profileId: string, pkgs: string[], password: string | null) =>
    invoke<string>("apt_install_start", { profileId, pkgs, password }),

  gpuQuery: (profileId: string) => invoke<GpuQueryResult>("gpu_query", { profileId }),
  gpuKill: (profileId: string, pid: number) =>
    invoke<string>("gpu_kill", { profileId, pid }),
  gpuSetPreview: (sudoMode: string, action: string, gpu: number | null, value: number) =>
    invoke<string>("gpu_set_preview", { sudoMode, action, gpu, value }),
  gpuSetStart: (
    profileId: string,
    action: string,
    gpu: number | null,
    value: number,
    password: string | null,
  ) => invoke<string>("gpu_set_start", { profileId, action, gpu, value, password }),

  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (s: AppSettings) => invoke<AppSettings>("save_settings", { settings: s }),
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

type TaskBuf = { chunks: StreamChunk[]; done: StreamDone | null };
type TaskSub = { onChunk: (c: StreamChunk) => void; onDone: (d: StreamDone) => void };

const taskBuffers = new Map<string, TaskBuf>();
const taskSubs = new Map<string, Set<TaskSub>>();
let busReady = false;
let busPromise: Promise<void> | null = null;

function taskPrune() {
  if (taskBuffers.size <= 100) return;
  const excess = taskBuffers.size - 100;
  let i = 0;
  for (const k of taskBuffers.keys()) {
    if (i++ >= excess) break;
    taskBuffers.delete(k);
  }
}

/** 全局任务总线：App 启动时注册一次 task://stream / task://exit 监听并缓存各任务输出，
 *  避免快任务在调用方注册监听前结束而丢事件 */
export function startTaskBus(): Promise<void> {
  if (busReady) return Promise.resolve();
  if (busPromise) return busPromise;
  busPromise = (async () => {
    await listen<StreamChunk>("task://stream", (e) => {
      let buf = taskBuffers.get(e.payload.taskId);
      if (!buf) {
        buf = { chunks: [], done: null };
        taskBuffers.set(e.payload.taskId, buf);
      }
      buf.chunks.push(e.payload);
      if (buf.chunks.length > 4000) buf.chunks.splice(0, buf.chunks.length - 4000);
      taskSubs.get(e.payload.taskId)?.forEach((s) => s.onChunk(e.payload));
      taskPrune();
    });
    await listen<StreamDone>("task://exit", (e) => {
      let buf = taskBuffers.get(e.payload.taskId);
      if (!buf) {
        buf = { chunks: [], done: null };
        taskBuffers.set(e.payload.taskId, buf);
      }
      buf.done = e.payload;
      taskSubs.get(e.payload.taskId)?.forEach((s) => s.onDone(e.payload));
    });
    busReady = true;
  })();
  return busPromise;
}

/** 订阅某任务的流式输出；返回取消函数。
 *  任务已产生的输出（含已结束）会先回放，保证快任务不丢事件 */
export async function onTaskStream(
  taskId: string,
  onChunk: (c: StreamChunk) => void,
  onDone: (d: StreamDone) => void,
): Promise<() => Promise<void>> {
  await startTaskBus();
  const buf = taskBuffers.get(taskId);
  if (buf) {
    for (const c of buf.chunks) onChunk(c);
    if (buf.done) {
      onDone(buf.done);
      return async () => {};
    }
  }
  const sub: TaskSub = { onChunk, onDone };
  let subs = taskSubs.get(taskId);
  if (!subs) {
    subs = new Set();
    taskSubs.set(taskId, subs);
  }
  subs.add(sub);
  return async () => {
    subs.delete(sub);
    if (subs.size === 0) taskSubs.delete(taskId);
  };
}
