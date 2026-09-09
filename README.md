# RemoteLLM

English | [简体中文](README.zh-CN.md)

A Windows desktop app built with Rust + Tauri v2 that manages LLM inference environments on remote GPU Linux servers over SSH — from environment checks and driver maintenance to inference framework deployment, model downloads, instance start/stop and real-time monitoring, all in one place without typing commands on the server.

![release](https://img.shields.io/github/v/release/chenyb999-zhcn/RemoteLLM)
![license](https://img.shields.io/github/license/chenyb999-zhcn/RemoteLLM)
![platform](https://img.shields.io/badge/platform-Windows-blue)
![tauri](https://img.shields.io/badge/Tauri-v2-orange)

## Screenshots

> Note: the UI is currently in Chinese. English localization is on the roadmap.

### Dashboard

Environment info cards (OS / CPU / multi-partition disks / Python / CUDA / driver / Docker); real-time curves for GPU utilization, memory, temperature and power, plus a GPU process list; `/metrics` scraping from inference services with line charts (key metrics auto-selected, manual selection and auto-refresh supported).

<img src="screenshoot/ScreenShot_2026-09-05_174341_871.png" width="840" alt="Dashboard"/>

### Environment Check

A 24-item health check (essential tools / GPU driver / Docker & GPU runtime / model tooling / inference engines / CUDA libraries) with one-click fixes (pip / apt / Docker install & authorization / jump to image pull); sm70 compatibility hints for cards like the V100; copyable commands for manual items such as driver installation.

<img src="screenshoot/ScreenShot_2026-09-05_174411_000.png" width="840" alt="Environment Check"/>

### GPU Management

Per-GPU overview (model / serial / VBIOS / PCIe / ECC / throttle reasons, compatible with both legacy and modern driver bitmask formats); GPU process management (shows owner, only your own processes can be killed); Persistence Mode toggle and power limit adjustment (sudo password flow); GPU topology display.

<img src="screenshoot/ScreenShot_2026-09-05_174436_238.png" width="840" alt="GPU Management"/>

### Framework Management

One-click add / pull of Docker images; "Add Framework Image" supports images from any registry (framework name + image address, validated before automatic pull and persisted); native framework detection for vLLM / 1Cat-vLLM / SGLang / llama.cpp; dynamic instance parameter forms with live command preview; run-log drawer (last 500 lines initially, older lines load on scroll-to-top).

<img src="screenshoot/ScreenShot_2026-09-05_174454_409.png" width="840" alt="Framework Management"/>

### Model Management

ModelScope / Hugging Face search with server-side streaming downloads; local model scanning parses headers with the official `gguf` / `safetensors` packages (architecture / parameter count / context size / quantization, split-shard aggregation) and a persistent metadata cache — refreshing an unchanged directory returns in seconds.

<img src="screenshoot/ScreenShot_2026-09-05_174519_705.png" width="840" alt="Model Management"/>

### Settings

Default download source, model directory, HF mirror endpoint + token, polling interval, dark theme; download proxy (when enabled, model downloads / pip / git clone go through the proxy; Docker daemon proxy is configured automatically for image pulls).

<img src="screenshoot/ScreenShot_2026-09-05_174529_929.png" width="840" alt="Settings"/>

## Features

- **Server management**: multiple server profiles (password / public-key auth), SSH connect & disconnect, auto-connect to the last server on startup
- **Dashboard**: environment info cards; real-time GPU utilization / memory / temperature / power curves and GPU process list (pmon); `/metrics` scraping from inference services with line charts (auto-selected key metrics + manual selection, auto-refresh)
- **Environment check**: 24-item health check with one-click fixes (pip / apt sudo / Docker install & authorization / jump to image pull); 12 CUDA libraries detected via both dpkg and pip (cuBLAS / cuDNN / NCCL / TensorRT-LLM, etc.); sm70 compatibility hints for cards like the V100
- **GPU management**: per-GPU overview with mini trend charts; GPU process management (shows owner, only your own processes can be killed); Persistence Mode toggle and power limit adjustment (sudo password flow); GPU topology display
- **Framework management**:
  - Docker images: one-click add for the four built-in framework images; "Add Framework Image" supports images from any registry (framework name + image address, validated, pulled automatically and persisted, removable at any time)
  - Native framework detection: install status and version for vLLM / 1Cat-vLLM / SGLang / llama.cpp, with one-click install / upgrade / uninstall
  - Instance creation branches by framework: the four built-in frameworks keep the full tabbed parameter settings (hover for CLI flags and official defaults); custom frameworks use a simplified form where the startup command is written by the user
  - Instance start/stop: native process (nohup + PID) or Docker container (`--gpus all`, model path mounted as-is); live startup command preview
  - Run logs: loads the last 500 lines initially, automatically loads 500 more when scrolled to the top (viewport-anchored, no jumping); drawer width is 2/3 of the window
- **Model management**: ModelScope / Hugging Face search with server-side downloads (streaming logs); local model scanning with official `gguf` / `safetensors` header parsing (architecture / parameter count / context size / quantization, split-shard aggregation); persistent metadata cache (instant refresh while the directory fingerprint is unchanged, even across app restarts); model deletion
- **Settings**: default download source, model directory, HF mirror endpoint + token, polling interval, dark theme; download proxy (when enabled, model downloads / pip / git clone go through the proxy; the Docker daemon proxy is configured automatically)
- **One-click install**: pip / git+cmake / docker pull to install frameworks and toolchains; the 1Cat-vLLM repository URL and image are configurable per server profile

## Quick Start

1. **Install**: download `RemoteLLM_*_x64-setup.exe` from [Releases](https://github.com/chenyb999-zhcn/RemoteLLM/releases/latest)
2. **Add a server**: enter host, account and authentication (password or private key), then connect
3. **Health check**: review the 24 items on the Environment Check page and fix missing ones with one click
4. **Prepare a framework**: add / pull images on the Docker images card (custom framework images supported), or one-click install a native framework
5. **Create an instance**: pick a framework, model path and parameters (for custom frameworks, write the startup command directly) → start
6. **Monitor**: watch GPU curves and `/metrics` on the dashboard; view instance output in the log drawer

## Download & Installation

- URL: <https://github.com/chenyb999-zhcn/RemoteLLM/releases/latest>
- Platform: Windows 10 / 11 x64 (NSIS installer `RemoteLLM_<version>_x64-setup.exe`)
- Requires the WebView2 Runtime: bundled with Windows 11; the installer bootstraps it on Windows 10 if missing

## Remote Directory Layout

All remote operations use the `~/RemoteLLM/` root directory by default (configurable per server profile):

```
~/RemoteLLM/
├── models/   # model weights
├── run/      # instance PID files
└── logs/     # instance logs
```

## Tech Stack

| Layer | Technology |
|---|---|
| Frontend | Vue 3 + TypeScript + Naive UI + Pinia + Chart.js |
| Backend | Rust + Tauri v2 |
| SSH | russh (password / public-key auth) |
| Storage | tauri-plugin-store (local JSON) |

## Development & Build

```bash
npm install
npm run tauri dev              # dev server
npx vue-tsc --noEmit           # frontend type check
cargo test                     # Rust tests (run in src-tauri/)
npm run tauri build -- --bundles nsis   # package
# Output: src-tauri/target/release/bundle/nsis/RemoteLLM_*_x64-setup.exe
```

## Notes

- **V100 (sm70) users**: recent official vLLM releases no longer support sm70 — use 1Cat-vLLM instead (a vLLM fork with SM70 support), default image `ghcr.io/chenyb999-zhcn/1cat-vllm:1.5`
- **sudo operations**: use the password flow — passwords are used once and never stored; passwordless sudo or root login on the server makes this seamless
- **Proxy**: once the download proxy is enabled in Settings, model downloads / pip / git clone go through it; if the Docker daemon proxy mismatches during an image pull, the app guides you through reconfiguring it (docker restarts, running containers are interrupted)
- **Credential safety**: SSH passwords / key paths and HF tokens are stored only in the local app data directory (JSON) and never uploaded anywhere

## License

[Apache-2.0](LICENSE)
