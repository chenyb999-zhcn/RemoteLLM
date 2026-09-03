# RemoteLLM

基于 Rust + Tauri v2 的桌面应用，通过 SSH 远程管理 GPU Linux 服务器上的 LLM 推理环境。

## 功能

- **服务器管理**：多服务器档案（密码/公钥认证），SSH 连接与断开
- **环境检查**：GPU 型号/驱动/CUDA/磁盘/Python 环境一键检测
- **实时监控**：GPU 利用率/显存/温度/功耗实时曲线、进程列表（nvidia-smi pmon）、推理服务 `/metrics` 指标抓取
- **框架管理**：vLLM / 1Cat-vLLM / SGLang / llama.cpp 检测；实例参数动态表单 + 命令实时预览；nohup+PID 启停；Docker 模式；运行日志
- **模型管理**：ModelScope / Hugging Face 搜索，服务器端下载（流式日志），本地模型列表与删除
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
