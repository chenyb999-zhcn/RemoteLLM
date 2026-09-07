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
  modelsDir?: string | null;
  onecatRepo?: string | null;
  onecatImage?: string | null;
}

export interface AppSettings {
  defaultModelSource: string;
  modelDir: string;
  hfEndpoint: string;
  hfToken: string;
  pollIntervalMs: number;
  lastProfileId: string;
  autoConnect: boolean;
  darkTheme: boolean;
  proxyEnabled: boolean;
  proxyUrl: string;
  /** pip 镜像源 id：tuna/aliyun/ustc/huawei/tencent/pypi */
  pipIndex: string;
  /** deb(apt) 镜像源 id：tuna/aliyun/ustc/huawei/tencent/official */
  debMirror: string;
}

/** 单个磁盘分区 */
export interface DiskInfo {
  mount: string;
  fs: string;
  total: number | null;
  used: number | null;
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
  /** 全部磁盘分区 */
  disks: DiskInfo[];
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
  sm: number;
  memBw: number;
  mem: number | null;
  command: string;
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
  /** 服务器上的绝对路径 */
  path: string;
  /** 相对模型目录的路径（删除/复制用） */
  rel: string;
  name: string;
  /** "gguf" | "gguf-split" | "safetensors" | "hf" | "dir" */
  kind: string;
  sizeBytes: number | null;
  arch: string | null;
  quant: string | null;
  params: string | null;
  ctx: number | null;
  note: string | null;
}

export interface RepoFile {
  path: string;
  size: number;
}

export interface ToolsStatus {
  modelscope: boolean;
  huggingface: boolean;
}

export interface ParserLibsStatus {
  gguf: boolean;
  safetensors: boolean;
}

export interface DockerStatus {
  installed: boolean;
  version: string | null;
  daemonRunning: boolean;
  /** 当前用户能否直接执行 docker（在 docker 组或 root） */
  usable: boolean;
  isRoot: boolean;
  sudoPasswordless: boolean;
  /** nvidia-container-toolkit 就绪（--gpus all 需要） */
  gpuRuntime: boolean;
  /** GPU 信号明细: "rt" | "ctk" | "bin" | null */
  gpuRuntimeDetail: string | null;
  /** docker daemon 当前拉取代理，null = 未配置 */
  daemonProxy: string | null;
}

export interface LocalImage {
  name: string;
  size: string | null;
}

/** 初始化检查单项 */
export interface InitItem {
  id: string;
  group: "sys" | "gpu" | "docker" | "tools" | "engine" | string;
  label: string;
  state: "ok" | "warn" | "missing" | "info";
  detail: string | null;
  fix: string | null;
  fixPkgs: string[] | null;
  manual: string | null;
}

export interface InitCheckResult {
  items: InitItem[];
  okCount: number;
  warnCount: number;
  missingCount: number;
}

// ---------- GPU 管理 ----------

export interface GpuCard {
  index: number;
  name: string;
  serial: string | null;
  driver: string;
  vbios: string | null;
  pcieGenCurrent: number | null;
  pcieGenMax: number | null;
  pcieWidth: number | null;
}

export interface GpuStat {
  index: number;
  util: number | null;
  memTotalMb: number | null;
  memUsedMb: number | null;
  tempC: number | null;
  powerW: number | null;
  powerLimitW: number | null;
  powerMinW: number | null;
  powerMaxW: number | null;
  powerDefaultW: number | null;
  smClockMhz: number | null;
  smClockMaxMhz: number | null;
}

export interface GpuSetState {
  index: number;
  persistence: boolean;
  computeMode: string | null;
  ecc: boolean | null;
}

export interface GpuThrottle {
  index: number;
  reasons: string[];
}

export interface GpuEcc {
  index: number;
  corrected: number | null;
  uncorrected: number | null;
}

export interface GpuProcRow {
  gpu: number;
  pid: number;
  user: string;
  name: string;
  elapsed: string;
  memMb: number | null;
  mine: boolean;
}

export interface GpuQueryResult {
  cards: GpuCard[];
  stats: GpuStat[];
  settings: GpuSetState[];
  throttles: GpuThrottle[];
  ecc: GpuEcc[];
  procs: GpuProcRow[];
  topo: string | null;
}
