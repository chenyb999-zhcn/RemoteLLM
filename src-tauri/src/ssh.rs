use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::ChannelMsg;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppError;
use crate::profile::ServerProfile;

#[derive(Default)]
pub struct ClientHandler {}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    // 当前策略：信任服务器主机密钥（后续可加 known_hosts 校验）
    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

pub struct SshSession {
    pub handle: client::Handle<ClientHandler>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnInfo {
    pub host: String,
    pub user: String,
    pub server_version: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: u32,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StreamChunk {
    pub task_id: String,
    pub kind: String, // "out" | "err"
    pub data: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StreamDone {
    pub task_id: String,
    pub exit_code: u32,
}

impl SshSession {
    pub async fn connect(profile: &ServerProfile) -> Result<Self, AppError> {
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(120)),
            ..Default::default()
        });
        let mut handle = client::connect(config, (profile.host.as_str(), profile.port), ClientHandler {})
            .await
            .map_err(|e| AppError::Ssh(format!("connect {}: {}", profile.addr(), e)))?;

        let auth = match &profile.auth {
            crate::profile::AuthMethod::Password { password } => handle
                .authenticate_password(&profile.user, password)
                .await
                .map_err(|e| AppError::Ssh(e.to_string()))?,
            crate::profile::AuthMethod::Key { key_path, passphrase } => {
                let key = load_secret_key(key_path, passphrase.as_deref())
                    .map_err(|e| AppError::Auth(format!("load key {}: {e}", key_path)))?;
                let hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .map_err(|e| AppError::Ssh(e.to_string()))?
                    .flatten();
                handle
                    .authenticate_publickey(
                        &profile.user,
                        PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await
                    .map_err(|e| AppError::Ssh(e.to_string()))?
            }
        };

        if !auth.success() {
            return Err(AppError::Auth(format!(
                "auth failed for {}@{}",
                profile.user,
                profile.addr()
            )));
        }
        Ok(Self { handle })
    }

    /// 执行短命令并收集全部输出
    pub async fn run(&mut self, command: &str) -> Result<CmdOutput, AppError> {
        let started = Instant::now();
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| AppError::Ssh(e.to_string()))?;
        channel
            .exec(true, command)
            .await
            .map_err(|e| AppError::Ssh(e.to_string()))?;

        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut code = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => stdout.extend(&data),
                ChannelMsg::ExtendedData { data, .. } => stderr.extend(&data),
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                _ => {}
            }
        }
        let out = CmdOutput {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            exit_code: code.unwrap_or(255),
        };
        let dur = started.elapsed().as_millis();
        let summary = format!(
            "run exit={} dur={}ms out={}B err={}B",
            out.exit_code,
            dur,
            out.stdout.len(),
            out.stderr.len()
        );
        let is_script = command.contains('\n') || command.chars().count() > 200;
        if is_script {
            let result = if out.exit_code == 0 {
                "-".to_string()
            } else {
                let first = out.stderr.lines().next().unwrap_or("").trim();
                let first: String = first.chars().take(200).collect();
                format!("stderr: {first}")
            };
            crate::applog::cmd_block("ssh", &summary, command, Some(&result));
        } else {
            crate::applog::cmd_summary("ssh", &summary, command);
        }
        Ok(out)
    }

    /// 后台执行长命令：调用方立即返回（task_id 由调用方生成），
    /// 流期间持有 conns 锁（其他命令等待），结束时发 task://exit
    pub fn spawn_stream(app: AppHandle, profile_id: String, command: String, task_id: String) {
        tokio::spawn(async move {
            let fail = |code: u32, extra: Option<String>| {
                if let Some(msg) = extra {
                    let _ = app.emit(
                        "task://stream",
                        StreamChunk {
                            task_id: task_id.clone(),
                            kind: "err".into(),
                            data: msg,
                        },
                    );
                }
                let _ = app.emit(
                    "task://exit",
                    StreamDone {
                        task_id: task_id.clone(),
                        exit_code: code,
                    },
                );
            };
            let state = app.state::<crate::AppState>();
            let mut conns = state.conns.lock().await;
            let Some(session) = conns.get_mut(&profile_id) else {
                fail(255, Some(format!("\n[未连接到服务器: {profile_id}]\n")));
                return;
            };
            match session.run_stream(&command, &task_id, &app).await {
                Ok(_) => {}
                Err(e) => fail(255, Some(format!("\n[SSH 错误: {e}]\n"))),
            }
        });
    }

    /// 执行长命令，输出经 Tauri 事件实时推送；结束时发 task://exit
    pub async fn run_stream(
        &mut self,
        command: &str,
        task_id: &str,
        app: &AppHandle,
    ) -> Result<u32, AppError> {
        let started = Instant::now();
        crate::applog::cmd_block("ssh", &format!("run_stream task={task_id} begin"), command, None);
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| AppError::Ssh(e.to_string()))?;
        channel
            .exec(true, command)
            .await
            .map_err(|e| AppError::Ssh(e.to_string()))?;

        let mut code = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    let _ = app.emit(
                        "task://stream",
                        StreamChunk {
                            task_id: task_id.to_string(),
                            kind: "out".into(),
                            data: String::from_utf8_lossy(&data).into_owned(),
                        },
                    );
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    let _ = app.emit(
                        "task://stream",
                        StreamChunk {
                            task_id: task_id.to_string(),
                            kind: "err".into(),
                            data: String::from_utf8_lossy(&data).into_owned(),
                        },
                    );
                }
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                _ => {}
            }
        }
        let exit_code = code.unwrap_or(255);
        crate::applog::info(
            "ssh",
            &format!(
                "run_stream task={task_id} exit={exit_code} dur={}ms",
                started.elapsed().as_millis()
            ),
        );
        let _ = app.emit(
            "task://exit",
            StreamDone {
                task_id: task_id.to_string(),
                exit_code,
            },
        );
        Ok(exit_code)
    }
}

#[tauri::command]
pub async fn ssh_connect(
    state: State<'_, crate::AppState>,
    profile: ServerProfile,
) -> Result<ConnInfo, AppError> {
    let started = Instant::now();
    let connect_result = SshSession::connect(&profile).await;
    match &connect_result {
        Ok(_) => crate::applog::info(
            "ssh",
            &format!(
                "connect ok {}@{} dur={}ms",
                profile.user,
                profile.addr(),
                started.elapsed().as_millis()
            ),
        ),
        Err(e) => crate::applog::error(
            "ssh",
            &format!(
                "connect fail {}@{}: {}",
                profile.user,
                profile.addr(),
                e
            ),
        ),
    }
    let mut session = connect_result?;
    let version = session
        .run("cat /etc/issue 2>/dev/null | head -1")
        .await
        .ok()
        .map(|o| o.stdout.trim().to_string())
        .filter(|s| !s.is_empty());
    state
        .conns
        .lock()
        .await
        .insert(profile.id.clone(), session);
    Ok(ConnInfo {
        host: profile.addr(),
        user: profile.user,
        server_version: version,
    })
}

#[tauri::command]
pub async fn ssh_disconnect(state: State<'_, crate::AppState>, id: String) -> Result<(), AppError> {
    crate::applog::info("ssh", &format!("disconnect {id}"));
    state.conns.lock().await.remove(&id);
    Ok(())
}

#[tauri::command]
pub async fn ssh_is_connected(state: State<'_, crate::AppState>, id: String) -> Result<bool, AppError> {
    Ok(state.conns.lock().await.contains_key(&id))
}

#[tauri::command]
pub async fn ssh_run(
    state: State<'_, crate::AppState>,
    id: String,
    command: String,
) -> Result<CmdOutput, AppError> {
    let mut conns = state.conns.lock().await;
    let Some(session) = conns.get_mut(&id) else {
        return Err(AppError::NotConnected(id));
    };
    session.run(&command).await
}

#[tauri::command]
pub async fn ssh_run_stream(app: AppHandle, id: String, command: String, task_id: String) {
    // 立即返回；后台流式执行（未连接时 spawn 会立即发 task://exit）
    SshSession::spawn_stream(app, id, command, task_id);
}
