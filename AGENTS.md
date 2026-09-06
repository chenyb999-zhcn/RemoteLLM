# AGENTS.md — RemoteLLM 开发与运维要点

## 环境（本机 Windows + PowerShell 5.1）

- 每个 bash 命令先补 PATH：`$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH`
- **`npm run tauri build` 也依赖 cargo 在 PATH**（内部调 `cargo metadata`）：不补 PATH 会报 `failed to run 'cargo metadata' ... program not found`。构建前务必先补 PATH。
- npm 命令用 `npm.cmd`（非 `npm`）
- cargo/npm 必须在正确目录：`cargo` 在 `src-tauri\`，`npm run tauri build` 在仓库根
- 重建/重跑前杀进程：`Get-Process -Name remotellm,RemoteLLM -ErrorAction SilentlyContinue | Stop-Process -Force`
- 类型检查：`npx.cmd vue-tsc --noEmit`（仓库根）

## 编码：PowerShell 文本 → Linux 服务器（重要，易踩坑）

- PowerShell 5.1 的 `Set-Content`/`Out-File` 会写入 **BOM + CRLF**；`Invoke-RestMethod` 发送字符串默认按 **Latin-1**（会把中文变乱码）。
- 直接给远端 shell 传含 `\r` / 多字节字符 / 引号嵌套的命令，很容易被转义破坏（heredoc 终止符带 `\r` 永不匹配、单引号被 PowerShell 剥掉等）。
- **规则**：
  - 本地生成文本文件后用 **scp** 传到服务器再执行（不要 `ssh "echo 长文本"`）。
  - 需要内嵌传输时用 **base64**：本地 `[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($content))` → 远端 `echo <b64> | base64 -d > file`。
  - 写 Rust 源文件/JSON/README 时，用写文件工具或 `[IO.File]::WriteAllText(path, text, (New-Object Text.UTF8Encoding($false)))`（无 BOM）。
  - 从 Rust 源码提取嵌入脚本（如 LIST_MODELS_PY）时用标记定位 `r#"` … `"#;`，勿按行号切片（行号/CRLF 会漂移）。

## 构建与产物

- exe 文件名大小写：**RemoteLLM**（productName）。改 `src-tauri/tauri.conf.json` 的 `productName`，安装包输出形如 `RemoteLLM_0.1.0_x64-setup.exe`。
- 版本号三处同步：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`；改后跑一次 `cargo check`（或 build）刷新 `Cargo.lock`。
- 发行（GitHub API，本机未装 gh）：
  - token：`("protocol=https`nhost=github.com`n" | git credential fill 2>$null | Where-Object { $_ -match '^password=' }).Substring(9)`
  - 建 release/PATCH body：**必须 UTF-8 字节**：`$bytes = [Text.Encoding]::UTF8.GetBytes($json)` + `-Body $bytes` + ContentType 带 `charset=utf-8`（否则中文变乱码）。
  - 上传资产：`Invoke-RestMethod -Method Post -Uri "$upload_url?name=..." -ContentType "<mime>" -InFile <file>`

## 测试服务器

- z420：192.168.31.20（`~/RemoteLLM` 已在跑 docker，GPU 相关先例）
- vbox (VirtualBox VM)：192.168.31.43，vboxuser / 123456（SSH 与 sudo 同），**无 NVIDIA GPU**
- sshpass（密码登录调试）：`C:\msys64\usr\bin\sshpass.exe -p <pass> ssh -o StrictHostKeyChecking=no <user>@<host> "<cmd>"`
- E2E 驱动脚本放 `C:\msys64\tmp\opencode\`（.cjs，WebView 调试口 PORT，产物引用 `target\release\remotellm.exe`）

## 日志（RemoteLLM.log）

- 位置：**exe 同目录** `RemoteLLM.log`（不可写时降级到系统日志目录）。跨天轮转为 `RemoteLLM.log.YYYY-MM-DD`，保留 31 天，总量上限 500MB（超限从最旧删）。
- 实现：`src-tauri/src/applog.rs`（`info/warn/error`、`cmd_summary` 单行、`cmd_block` 完整命令块）。所有 SSH 命令经 `ssh.rs run/run_stream` 咽喉自动记录；业务动作在 install/docker/initcheck/gpumgmt/models 各有一条 `[task]` 语义行。
- **脱敏**：`mask_secrets` 把 `printf '%s\n' '密码' | sudo` 与 `HF_TOKEN=…` 替换为 `***`；改远程脚本的 sudo 包装格式时，保持该模式或同步更新脱敏/单测。

## 远程脚本设计禁忌（sudo 包装）

- `command` 是 shell 内建，**不能**被包装成 `sudo command …`（sudo 找不到该可执行文件）。
- 管道/重定向（`curl | gpg -o /usr/share/…`、`> /etc/…`）被 `sudo` 前缀包装后只有第一段提权，其余以普通用户执行 → 权限被拒被 `|| true` 吞掉。
- 需 root 的管道/重定向：先 heredoc 写 `$HOME/.rl_*.sh`（用户态，不包装），再整行 `sudo bash …`；`\r` 会让 heredoc 终止符失配。
