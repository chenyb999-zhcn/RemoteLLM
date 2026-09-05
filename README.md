# RemoteLLM

基于 Rust + Tauri v2 的桌面应用，通过 SSH 远程管理 GPU Linux 服务器上的 LLM 推理环境。

## 功能

- **服务器管理**：多服务器档案（密码/公钥认证），SSH 连接与断开，启动自动连接
- **总览**：环境信息卡片（系统 / CPU 型号 / 多分区磁盘进度条，挂载点与容量左右对齐 / Python / CUDA / 驱动 / Docker）；GPU 利用率/显存/温度/功耗实时曲线、GPU 进程列表（pmon）；推理服务 `/metrics` 指标抓取 + 折线图（自动勾选关键指标 + 手动勾选、自动刷新）
- **初始化检查**：24 项体检（基础工具 / GPU 驱动 / Docker 与 GPU 运行时 / 模型工具 / 推理引擎），一键修复（pip / apt sudo / Docker 安装授权 / 跳转拉取），V100 等 sm70 卡型兼容性提示，NVIDIA 驱动等手动项提供复制命令
- **GPU 管理**：每卡概览（型号/序列号/VBIOS/PCIe/ECC/降频原因，兼容新旧驱动的位掩码格式）+ 迷你趋势图；GPU 进程管理（显示属主，仅可结束自己的进程）；Persistence Mode 开关、功耗上限调整（sudo 密码流）；GPU 拓扑展示
- **引擎管理**：vLLM / 1Cat-vLLM / SGLang / llama.cpp 检测；实例参数动态表单 + 命令实时预览；nohup+PID 启停；Docker 模式；运行日志
- **Docker 管理**：状态与 GPU 运行时三信号检测、镜像列表/拉取、GPU 实测（`--gpus all`）；sudo 密码流安装 docker.io + nvidia-container-toolkit、授权；拉取代理自动配置（systemd drop-in + 重启，代理不匹配时自动引导）
- **模型管理**：ModelScope / Hugging Face 搜索，服务器端下载（流式日志）；本地模型扫描用官方 `gguf` / `safetensors` 包解析头部（架构/参数量/上下文/量化，分片组聚合）；**持久化解析缓存**（目录指纹未变化时每次刷新均秒级返回，含重启应用后；活跃日志目录不触发误判，列表大小始终取最新）；模型删除
- **设置**：默认下载来源、模型目录、HF 镜像端点 + Token、轮询间隔、深色主题；**下载代理**（启用后模型下载 / pip / git clone 走代理，Docker 拉镜像自动配置 daemon）
- **一键安装**：pip / git+cmake / docker pull 安装框架与工具链，1Cat-vLLM 仓库地址与镜像可在服务器档案中配置

## 技术栈

| 层 | 技术 |
|---|---|
| 前端 | Vue 3 + TypeScript + Naive UI + Pinia + Chart.js |
| 后端 | Rust + Tauri v2 |
| SSH | russh（密码 / 公钥认证） |
| 存储 | tauri-plugin-store（本机 JSON） |

## 开发

```bash
npm install
npm run tauri dev
```

## 构建

```bash
npm run tauri build -- --bundles nsis
# 产物: src-tauri/target/release/bundle/nsis/remotellm_*_x64-setup.exe
```

## 远程目录约定

所有远端操作统一使用 `~/RemoteLLM/` 根目录：

```
~/RemoteLLM/
├── models/   # 模型权重
├── run/      # 实例 PID 文件
└── logs/     # 实例日志
```

## License

Apache-2.0
