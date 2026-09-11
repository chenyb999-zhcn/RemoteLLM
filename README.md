# RemoteLLM

English | [简体中文](README.zh-CN.md)

A Windows desktop app built with Rust + Tauri v2 that manages LLM inference environments on remote GPU Linux servers over SSH — from environment checks and driver maintenance to inference framework deployment, model downloads, instance start/stop and real-time monitoring, all in one place without typing commands on the server.

![release](https://img.shields.io/github/v/release/chenyb999-zhcn/RemoteLLM)
![license](https://img.shields.io/github/license/chenyb999-zhcn/RemoteLLM)
![platform](https://img.shields.io/badge/platform-Windows-blue)
![tauri](https://img.shields.io/badge/Tauri-v2-orange)

## Features

- **Server management**: multiple server profiles (password / public-key auth), SSH connect & disconnect, auto-connect to the last server on startup
- **Dashboard**: environment info cards; real-time GPU utilization / memory / temperature / power curves and GPU process list (pmon); `/metrics` scraping from inference services with line charts (auto-selected key metrics + manual selection, auto-refresh; derived Token gen rate (tokens/s) metric from consecutive counter deltas)
- **Environment check**: 25-item health check with one-click fixes (pip / apt sudo / Docker install & authorization / jump to image pull); includes a Python 3.12 check with one-click uv install (venv base for the native frameworks); 12 CUDA libraries detected via both dpkg and pip (cuBLAS / cuDNN / NCCL / TensorRT-LLM, etc.); sm70 compatibility hints for cards like the V100
- **GPU management**: per-GPU overview with mini trend charts; GPU process management (shows owner, only your own processes can be killed); Persistence Mode toggle and power limit adjustment (sudo password flow); GPU topology display
- **Framework management**:
  - Docker images: one-click add for the four built-in framework images; "Add Framework Image" supports images from any registry (framework name + image address, validated, pulled automatically and persisted, removable at any time)
   - Native framework detection: install status and version for vLLM / 1Cat-vLLM / SGLang / llama.cpp / FastLLM, with one-click install / upgrade / uninstall (FastLLM is a C++ implementation without a PyTorch dependency, native mode only)
   - Instance creation branches by framework: the five built-in frameworks keep the full tabbed parameter settings (hover for CLI flags and official defaults); custom frameworks use a simplified form where the startup command is written by the user
  - Instance start/stop: native process (nohup + PID) or Docker container (`--gpus all`, model path mounted as-is); live startup command preview
  - Run logs: loads the last 500 lines initially, automatically loads 500 more when scrolled to the top (viewport-anchored, no jumping); drawer width is 2/3 of the window
- **Model management**: ModelScope / Hugging Face search with server-side downloads (streaming logs); local model scanning with official `gguf` / `safetensors` header parsing (architecture / parameter count / context size / quantization, split-shard aggregation); persistent metadata cache (instant refresh while the directory fingerprint is unchanged, even across app restarts); model deletion
- **Settings**: default download source, model directory, HF mirror endpoint + token, polling interval, dark theme; download proxy (when enabled, model downloads / pip / git clone go through the proxy; the Docker daemon proxy is configured automatically)
- **One-click install**: pip (Python frameworks install into an isolated Python 3.12 venv) / git+cmake / docker pull to install frameworks and toolchains; the 1Cat-vLLM repository URL and image are configurable per server profile

## Screenshots

> Note: the UI is available in Chinese and English — switch in Settings.

<img src="screenshoot/ui-tour.gif" width="840" alt="UI tour (Dashboard → Environment Check → GPU Management → Framework Management → Model Management → Settings)"/>

- **Dashboard**: environment info cards (OS / CPU / multi-partition disks / Python / CUDA / driver / Docker); real-time curves for GPU utilization, memory, temperature and power, plus a GPU process list; `/metrics` scraping from inference services with line charts (key metrics auto-selected, manual selection and auto-refresh supported; includes a derived Token gen rate (tokens/s) metric — delta of the `llamacpp:tokens_predicted_total` and `llamacpp:tokens_predicted_seconds_total` counters between consecutive samples).
- **Environment check**: a 25-item health check (essential tools / GPU driver / Docker & GPU runtime / model tooling / inference engines / CUDA libraries) with one-click fixes (pip / apt / Docker install & authorization / Python 3.12 via uv / jump to image pull); sm70 compatibility hints for cards like the V100; copyable commands for manual items such as driver installation.
- **GPU management**: per-GPU overview (model / serial / VBIOS / PCIe / ECC / throttle reasons, compatible with both legacy and modern driver bitmask formats); GPU process management (shows owner, only your own processes can be killed); Persistence Mode toggle and power limit adjustment (sudo password flow); GPU topology display.
- **Framework management**: one-click add / pull of Docker images; "Add Framework Image" supports images from any registry (framework name + image address, validated before automatic pull and persisted); native framework detection for vLLM / 1Cat-vLLM / SGLang / llama.cpp / FastLLM; dynamic instance parameter forms with live command preview; run-log drawer (last 500 lines initially, older lines load on scroll-to-top).
- **Model management**: ModelScope / Hugging Face search with server-side streaming downloads; local model scanning parses headers with the official `gguf` / `safetensors` packages (architecture / parameter count / context size / quantization, split-shard aggregation) and a persistent metadata cache — refreshing an unchanged directory returns in seconds.
- **Settings**: default download source, model directory, HF mirror endpoint + token, polling interval, dark theme; download proxy (when enabled, model downloads / pip / git clone go through the proxy; Docker daemon proxy is configured automatically for image pulls).

## Quick Start

1. **Install**: download `RemoteLLM_*_x64-setup.exe` from [Releases](https://github.com/chenyb999-zhcn/RemoteLLM/releases/latest)
2. **Add a server**: enter host, account and authentication (password or private key), then connect
3. **Health check**: review the 25 items on the Environment Check page and fix missing ones with one click
4. **Prepare a framework**: add / pull images on the Docker images card (custom framework images supported), or one-click install a native framework
5. **Create an instance**: pick a framework, model path and parameters (for custom frameworks, write the startup command directly) → start
6. **Monitor**: watch GPU curves and `/metrics` on the dashboard; view instance output in the log drawer

## Download & Installation

- URL: <https://github.com/chenyb999-zhcn/RemoteLLM/releases/latest> (each release ships both packages)
- Installer: `RemoteLLM_<version>_x64-setup.exe` (NSIS, Windows 10 / 11 x64)
- Portable: `RemoteLLM_<version>_x64-portable.zip` (extract and run — a single `RemoteLLM.exe`, no installation)
- Requires the WebView2 Runtime: bundled with Windows 11; the installer bootstraps it on Windows 10 if missing (portable: install the runtime yourself if absent)

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

- **V100 (sm70) users**: recent official vLLM releases no longer support sm70 — use [1Cat-vLLM](https://github.com/1CatAI/1Cat-vLLM) instead (a vLLM fork with SM70 support), default image `ghcr.io/chenyb999-zhcn/1cat-vllm:1.5`
- **sudo operations**: use the password flow — passwords are used once and never stored; passwordless sudo or root login on the server makes this seamless
- **Proxy**: once the download proxy is enabled in Settings, model downloads / pip / git clone go through it; if the Docker daemon proxy mismatches during an image pull, the app guides you through reconfiguring it (docker restarts, running containers are interrupted)
- **Credential safety**: SSH passwords / key paths and HF tokens are stored only in the local app data directory (JSON) and never uploaded anywhere

## License

[Apache-2.0](LICENSE)
