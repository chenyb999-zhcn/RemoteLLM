use std::sync::Arc;
use std::time::Instant;

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

/// 每档案命令并发闸门：
/// - 只读命令（状态探测/列表/日志/指标轮询等）取读锁——互相并发，
///   同一 SSH 连接上开多个独立 channel，互不阻塞；
/// - 变更命令（安装/启停/删除/下载等）取写锁——独占，与全部只读命令互斥。
/// tokio RwLock 是公平的：写锁等待期间新的读锁会排在写锁之后，
/// 因此只读高频轮询不会饿死排队的变更操作。
#[derive(Default)]
pub(crate) struct CmdGate {
    inner: tokio::sync::RwLock<()>,
}

impl CmdGate {
    /// 取读锁：与其它读锁并发
    pub(crate) async fn read_only(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
        self.inner.read().await
    }
    /// 取写锁：独占（等已有读锁结束，并阻止新读锁）
    pub(crate) async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
        self.inner.write().await
    }
}

/// 运行期间持有的闸门句柄（读/写两种守卫类型统一持有，函数结束释放；
/// 字段内容从不读取——守卫的生命周期即锁的持有期）
#[allow(dead_code)]
enum GateGuard<'a> {
    Read(tokio::sync::RwLockReadGuard<'a, ()>),
    Write(tokio::sync::RwLockWriteGuard<'a, ()>),
}

/// 按只读/变更语义取闸门（读锁可并发，写锁独占）
async fn hold_gate(gate: &CmdGate, read_only: bool) -> GateGuard<'_> {
    if read_only {
        GateGuard::Read(gate.read_only().await)
    } else {
        GateGuard::Write(gate.write().await)
    }
}

pub struct SshSession {
    pub handle: client::Handle<ClientHandler>,
    /// 本档案的命令并发闸门（只读并发 / 变更独占）
    gate: CmdGate,
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
            // 不设空闲超时：编译/安装类长命令（pip 拉大 wheel、CUDA 静默编译）
            // 可能长时间无输出，任何有限超时都会误杀连接导致 exit 255
            inactivity_timeout: None,
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
        Ok(Self {
            handle,
            gate: CmdGate::default(),
        })
    }

    /// 执行短命令并收集全部输出。
    /// read_only=true 取读锁（只读命令互相并发）；false 取写锁（独占）。
    pub async fn run(&self, command: &str, read_only: bool) -> Result<CmdOutput, AppError> {
        let started = Instant::now();
        let _gate = hold_gate(&self.gate, read_only).await;
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
            "run exit={} dur={}ms out={}B err={}B ro={}",
            out.exit_code,
            dur,
            out.stdout.len(),
            out.stderr.len(),
            read_only
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
    /// 结束时发 task://exit。read_only 语义与 run() 相同。
    pub fn spawn_stream(
        app: AppHandle,
        profile_id: String,
        command: String,
        task_id: String,
        read_only: bool,
    ) {
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
            let Ok(session) = session_from(&state, &profile_id) else {
                fail(255, Some(format!("\n[未连接到服务器: {profile_id}]\n")));
                return;
            };
            match session.run_stream(&command, &task_id, &app, read_only).await {
                Ok(_) => {}
                Err(e) => fail(255, Some(format!("\n[SSH 错误: {e}]\n"))),
            }
        });
    }

    /// 执行长命令，输出经 Tauri 事件实时推送；结束时发 task://exit
    pub async fn run_stream(
        &self,
        command: &str,
        task_id: &str,
        app: &AppHandle,
        read_only: bool,
    ) -> Result<u32, AppError> {
        let started = Instant::now();
        let _gate = hold_gate(&self.gate, read_only).await;
        crate::applog::cmd_block("ssh", &format!("run_stream task={task_id} begin ro={read_only}"), command, None);
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

/// 取共享会话引用：只短暂持有连接表锁（克隆 Arc 后立即释放），
/// 命令执行期间不占表锁——不同档案、以及同档案的只读命令均可并行。
pub(crate) fn session_from(
    state: &State<'_, crate::AppState>,
    profile_id: &str,
) -> Result<Arc<SshSession>, AppError> {
    let conns = state
        .conns
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    conns
        .get(profile_id)
        .cloned()
        .ok_or_else(|| AppError::NotConnected(profile_id.to_string()))
}

/// 档案是否已连接（短暂持表锁）
pub(crate) fn is_connected(state: &State<'_, crate::AppState>, profile_id: &str) -> bool {
    match state.conns.lock() {
        Ok(conns) => conns.contains_key(profile_id),
        Err(_) => false,
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
    let session = connect_result?;
    // 自动创建工作根目录及三个子目录（models/run/logs，父目录一并创建）；失败不阻断连接
    let mkdir = format!(
        "mkdir -p {} {} {} 2>/dev/null; true",
        profile.default_models_dir(),
        profile.run_dir(),
        profile.logs_dir()
    );
    let _ = session.run(&mkdir, false).await;
    let version = session
        .run("cat /etc/issue 2>/dev/null | head -1", true)
        .await
        .ok()
        .map(|o| o.stdout.trim().to_string())
        .filter(|s| !s.is_empty());
    {
        let mut conns = state
            .conns
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        conns.insert(profile.id.clone(), Arc::new(session));
    }
    Ok(ConnInfo {
        host: profile.addr(),
        user: profile.user,
        server_version: version,
    })
}

#[tauri::command]
pub async fn ssh_disconnect(state: State<'_, crate::AppState>, id: String) -> Result<(), AppError> {
    crate::applog::info("ssh", &format!("disconnect {id}"));
    // 仍在执行的命令持有自己的 Arc 引用，会自然跑完；新命令将报 NotConnected
    if let Ok(mut conns) = state.conns.lock() {
        conns.remove(&id);
    }
    Ok(())
}

#[tauri::command]
pub async fn ssh_is_connected(state: State<'_, crate::AppState>, id: String) -> Result<bool, AppError> {
    let conns = state
        .conns
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    Ok(conns.contains_key(&id))
}

#[tauri::command]
pub async fn ssh_run(
    state: State<'_, crate::AppState>,
    id: String,
    command: String,
    read_only: Option<bool>,
) -> Result<CmdOutput, AppError> {
    let session = session_from(&state, &id)?;
    // 未知来源的命令一律按变更（独占）处理，保守安全
    session.run(&command, read_only.unwrap_or(false)).await
}

#[tauri::command]
pub async fn ssh_run_stream(
    app: AppHandle,
    id: String,
    command: String,
    task_id: String,
    read_only: Option<bool>,
) {
    // 立即返回；后台流式执行（未连接时 spawn 会立即发 task://exit）
    SshSession::spawn_stream(app, id, command, task_id, read_only.unwrap_or(false));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn gate_read_only_runs_concurrently() {
        // 4 个只读命令各 sleep 300ms：并发应 <1s，串行则 >=1.2s
        let gate = Arc::new(CmdGate::default());
        let started = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..4 {
            let g = Arc::clone(&gate);
            handles.push(tokio::spawn(async move {
                let _h = g.read_only().await;
                tokio::time::sleep(Duration::from_millis(300)).await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(1000),
            "read_only 命令未并发：elapsed={elapsed:?}"
        );
    }

    #[tokio::test]
    async fn gate_write_excludes_read_only() {
        // 写锁持有期间，只读命令必须排队等待
        let gate = Arc::new(CmdGate::default());
        let w = gate.write().await;

        let (tx, mut rx) = tokio::sync::oneshot::channel::<bool>();
        let g2 = Arc::clone(&gate);
        let reader = tokio::spawn(async move {
            let _h = g2.read_only().await;
            let _ = tx.send(true);
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        // 写锁未释放：读锁应拿不到
        assert!(
            rx.try_recv().is_err(),
            "写锁持有期间 read_only 不应立即获得读锁"
        );
        drop(w);
        reader.await.unwrap();
    }

    /// 真实 SSH 目标的只读并发 / 写独占验证（需要本地可达的 SSH 服务器）。
    /// 运行：REOTELLM_TEST_HOST=172.17.103.222 REMOTELLM_TEST_USER=chenyb REMOTELLM_TEST_PASS=123456 \
    ///        cargo test -p remotellm ssh_live_concurrency --release -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "需要真实 SSH 目标（REOTELLM_TEST_HOST/PORT/USER/PASS）"]
    async fn ssh_live_concurrency() {
        let host = std::env::var("REOTELLM_TEST_HOST").expect("set REOTELLM_TEST_HOST");
        let port: u16 = std::env::var("REOTELLM_TEST_PORT").unwrap_or("22".into()).parse().unwrap();
        let user = std::env::var("REOTELLM_TEST_USER").expect("set REOTELLM_TEST_USER");
        let pass = std::env::var("REOTELLM_TEST_PASS").expect("set REOTELLM_TEST_PASS");
        let profile = ServerProfile {
            id: "test".into(),
            name: "test".into(),
            host,
            port,
            user,
            auth: crate::profile::AuthMethod::Password { password: pass },
            base_dir: "~/RemoteLLM".into(),
            models_dir: None,
            onecat_repo: None,
            onecat_image: None,
            proxy: None,
        };
        let session = Arc::new(SshSession::connect(&profile).await.expect("connect failed"));

        // 1) 4 个只读命令（sleep 2）并发：总耗时应明显小于 4*2s
        let started = Instant::now();
        let mut handles = Vec::new();
        for i in 0..4 {
            let s = Arc::clone(&session);
            handles.push(tokio::spawn(async move {
                s.run(&format!("sleep 2; echo ok{i}"), true).await
            }));
        }
        let mut all_ok = true;
        for h in handles {
            match h.await {
                Ok(Ok(o)) => {
                    if o.exit_code != 0 || !o.stdout.contains("ok") {
                        all_ok = false;
                    }
                }
                _ => all_ok = false,
            }
        }
        let ro_elapsed = started.elapsed();
        println!("read_only x4 elapsed={ro_elapsed:?}");
        assert!(all_ok, "只读命令有失败");
        assert!(
            ro_elapsed < Duration::from_secs(4),
            "只读命令未并发：elapsed={ro_elapsed:?}（串行应 >=8s）"
        );

        // 2) 2 个变更命令（sleep 1）互斥：总耗时应 >=2s
        let started = Instant::now();
        let mut handles = Vec::new();
        for i in 0..2 {
            let s = Arc::clone(&session);
            handles.push(tokio::spawn(async move {
                s.run(&format!("sleep 1; echo w{i}"), false).await
            }));
        }
        for h in handles {
            h.await.unwrap().unwrap();
        }
        let w_elapsed = started.elapsed();
        println!("write x2 elapsed={w_elapsed:?}");
        assert!(
            w_elapsed >= Duration::from_millis(1900),
            "变更命令未互斥：elapsed={w_elapsed:?}（应 >=2s）"
        );
    }
}
