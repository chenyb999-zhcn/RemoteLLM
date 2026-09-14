use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_store::StoreExt;

/// 迷你仪表盘窗口 label
pub const MINI_LABEL: &str = "gpu-mini";
/// 事件：主窗口档案切换 → 迷你窗跟随切换轮询目标（payload: MiniProfile）
pub const EVT_MINI_PROFILE: &str = "gpu-mini/profile";
/// 事件：迷你窗已销毁 → 主窗口复位按钮状态（payload: 无）
pub const EVT_MINI_CLOSED: &str = "gpu-mini/closed";

/// 迷你窗尺寸（逻辑像素），与前端 Gauge 布局匹配
const MINI_W: f64 = 380.0;
const MINI_H: f64 = 232.0;

const POS_KEY: &str = "miniPos";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniProfile {
    pub profile_id: String,
}

/// 打开（或聚焦）GPU 迷你仪表盘窗口。返回 true = 新建，false = 已存在（仅聚焦 + 转发档案）。
#[tauri::command]
pub fn open_gpu_mini(app: AppHandle, profile_id: String) -> Result<bool, String> {
    // 单实例：已存在则聚焦，并把最新档案转发给迷你窗（切档案联动）
    if let Some(w) = app.get_webview_window(MINI_LABEL) {
        let _ = w.set_focus();
        let _ = w.emit(EVT_MINI_PROFILE, MiniProfile { profile_id });
        return Ok(false);
    }

    // 恢复记忆位置；首次使用放主显示器右下角
    let pos = restore_position(&app).or_else(|| default_position(&app));

    let mut builder = WebviewWindowBuilder::new(&app, MINI_LABEL, WebviewUrl::App("mini.html".into()))
        .title("RemoteLLM GPU Mini")
        .inner_size(MINI_W, MINI_H)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true);
    if let Some((x, y)) = pos {
        builder = builder.position(x, y);
    }
    let win = builder.build().map_err(|e| e.to_string())?;

    // 通知迷你窗当前档案。窗口 webview 可能尚未加载完（事件可能丢失），
    // 迷你页另有兜底：通过共享连接状态（server store）自行发现已连接档案
    let _ = win.emit(EVT_MINI_PROFILE, MiniProfile { profile_id });

    // 位置记忆（位移超过 5px 才落盘，避免拖动过程频繁写文件）+ 销毁通知主窗口
    let handle = app.clone();
    let w = win.clone();
    let last_saved = std::cell::Cell::new(None::<(i32, i32)>);
    win.on_window_event(move |event: &WindowEvent| {
        match event {
            WindowEvent::Moved(position) => {
                let store = handle.store("remotellm.json").ok();
                let Some(store) = store else {
                    return;
                };
                let last = last_saved.get();
                if last
                    .map(|(x, y)| (position.x - x).abs() < 5 && (position.y - y).abs() < 5)
                    .unwrap_or(false)
                {
                    return;
                }
                last_saved.set(Some((position.x, position.y)));
                let sf = w.scale_factor().unwrap_or(1.0);
                store.set(
                    POS_KEY,
                    serde_json::json!({
                        "x": position.x as f64 / sf,
                        "y": position.y as f64 / sf,
                    }),
                );
                let _ = store.save();
            }
            WindowEvent::Destroyed => {
                let _ = handle.emit(EVT_MINI_CLOSED, ());
            }
            _ => {}
        }
    });

    Ok(true)
}

/// 从 store 恢复逻辑坐标位置
fn restore_position(app: &AppHandle) -> Option<(f64, f64)> {
    let store = app.store("remotellm.json").ok()?;
    let v = store.get(POS_KEY)?;
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    Some((x, y))
}

/// 主显示器右下角（留 24/60 边距）
fn default_position(app: &AppHandle) -> Option<(f64, f64)> {
    let m = app.primary_monitor().ok()??;
    let s = m.size();
    let sf = m.scale_factor();
    Some((
        s.width as f64 / sf - MINI_W - 24.0,
        s.height as f64 / sf - MINI_H - 60.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mini_profile_serde_camel() {
        let s = serde_json::to_string(&MiniProfile {
            profile_id: "abc".into(),
        })
        .unwrap();
        assert_eq!(s, r#"{"profileId":"abc"}"#);
    }

    #[test]
    fn consts_stable() {
        // 事件名/label 与前端约定一致，改动需同步前端
        assert_eq!(MINI_LABEL, "gpu-mini");
        assert_eq!(EVT_MINI_PROFILE, "gpu-mini/profile");
        assert_eq!(EVT_MINI_CLOSED, "gpu-mini/closed");
    }
}
