export type AuthMethod =
  | { type: "password"; password: string }
  | { type: "key"; keyPath: string; passphrase?: string | null };

export interface ServerProfile {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
  auth: AuthMethod;
  baseDir: string;
  onecatRepo?: string | null;
  onecatImage?: string | null;
}

export interface GpuInfo {
  index: number;
  name: string;
  driverVersion: string;
  memTotalMb?: number | null;
  memUsedMb?: number | null;
  tempC?: number | null;
  powerW?: number | null;
  util?: number | null;
}

export interface EnvInfo {
  os?: string | null;
  kernel?: string | null;
  cpuCount?: number | null;
  cpuModel?: string | null;
  memTotal?: number | null;
  memUsed?: number | null;
  diskTotal?: number | null;
  diskUsed?: number | null;
  python?: string | null;
  cuda?: string | null;
  cudaPath?: string | null;
  docker?: string | null;
  driver: string;
  gpus: GpuInfo[];
}

export interface ConnInfo {
  host: string;
  user: string;
  serverVersion?: string | null;
}

export interface CmdOutput {
  stdout: string;
  stderr: string;
  exitCode: number;
}

export interface StreamChunk {
  taskId: string;
  kind: "out" | "err";
  data: string;
}

export interface StreamDone {
  taskId: string;
  exitCode: number;
}

export interface GpuSnap {
  index: number;
  name: string;
  util: number | null;
  memTotalMb: number | null;
  memUsedMb: number | null;
  tempC: number | null;
  powerW: number | null;
  powerLimitW: number | null;
}

export interface GpuPoll {
  ts: number;
  gpus: GpuSnap[];
  memTotal: number | null;
  memUsed: number | null;
  diskTotal: number | null;
  diskUsed: number | null;
  uptime: string | null;
}

export interface ProcRow {
  gpu: number;
  pid: number;
  itc: number;
  gmc: number;
  mem: number;
}

export interface MetricSample {
  name: string;
  help: string | null;
  labels: [string, string][];
  value: number;
}

export interface InstanceConfig {
  id: string;
  profileId: string;
  name: string;
  framework: "vllm" | "1cat-vllm" | "sglang" | "llama-cpp" | string;
  mode: "native" | "docker";
  modelPath: string;
  port: number;
  dockerImage?: string | null;
  params: Record<string, unknown>;
  createdAt: number;
}

export interface FwDetect {
  framework: string;
  installed: boolean;
  version: string | null;
  note: string | null;
}

export interface InstanceStatus {
  id: string;
  running: boolean;
  pid: number | null;
  healthCode: number | null;
  modelsJson: string | null;
  detail: string | null;
}

export interface ModelInfo {
  id: string;
  source: string;
  downloads: number | null;
  likes: number | null;
  description: string | null;
}

export interface LocalModel {
  name: string;
  size: string | null;
}
