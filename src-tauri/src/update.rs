// Tự cập nhật (tauri-plugin-updater, bộ cài NSIS trên GitHub Releases). Xem docs/auto-update-plan.md.
// Repo và khoá công khai chỉ được CI nhúng lúc build (FOMEG_UPDATE_REPO / FOMEG_UPDATE_PUBKEY), nên bản tự build trên máy
// (`npm run tauri build`, bản dev) không bao giờ tự cập nhật thành bản cài đặt: `enabled = false`.
// Plugin tự kiểm chữ ký minisign của bộ cài trước khi chạy; bản bị tráo / hỏng bị từ chối.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::core::util;
use crate::tr;

const REPO: Option<&str> = option_env!("FOMEG_UPDATE_REPO");
const PUBKEY: Option<&str> = option_env!("FOMEG_UPDATE_PUBKEY");
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// sự kiện tiến trình tải tối đa ~7 lần/giây
const PROGRESS_EVERY: Duration = Duration::from_millis(150);

fn config() -> Option<(&'static str, &'static str)> {
    let repo = REPO.map(str::trim).filter(|s| !s.is_empty() && s.contains('/'))?;
    let key = PUBKEY.map(str::trim).filter(|s| !s.is_empty())?;
    Some((repo, key))
}

/// Plugin luôn được đăng ký (cấu hình `plugins.updater` trong tauri.conf.json là bắt buộc); bản không có khoá thì
/// không bao giờ gọi tới nó.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    let mut b = tauri_plugin_updater::Builder::new();
    if let Some((_, key)) = config() {
        b = b.pubkey(key);
    }
    b.build()
}

/// Bản đã báo cho người dùng; `update_install` cài đúng bản này.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

#[derive(Serialize)]
pub struct Info {
    enabled: bool,
    version: String,
    repo: String,
}

#[derive(Serialize)]
pub struct Available {
    version: String,
    current: String,
    notes: String,
    date_ms: i64,
}

#[derive(Serialize, Clone)]
struct Progress {
    stage: &'static str,
    got: u64,
    total: u64,
}

#[tauri::command]
pub fn update_info(app: AppHandle) -> Info {
    Info {
        enabled: config().is_some(),
        version: app.package_info().version.to_string(),
        repo: config().map(|(r, _)| r.to_owned()).unwrap_or_default(),
    }
}

/// `null` = đang là bản mới nhất. Lỗi mạng / chữ ký / định dạng -> Err (giao diện báo, không treo).
#[tauri::command]
pub async fn update_check(app: AppHandle, pending: State<'_, Pending>) -> Result<Option<Available>, String> {
    let (repo, _) = config().ok_or_else(|| tr!("update.err.disabled"))?;
    let url = format!("https://github.com/{repo}/releases/latest/download/latest.json");
    let endpoint = url.parse().map_err(|e| format!("{e}"))?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .and_then(|b| b.timeout(CHECK_TIMEOUT).build())
        .map_err(|e| tr!("update.err.check", err = e))?;
    let found = updater.check().await.map_err(|e| {
        util::log_line(&format!("Kiểm tra bản mới lỗi: {e}"));
        tr!("update.err.check", err = e)
    })?;
    let out = found.as_ref().map(|u| Available {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone().unwrap_or_default(),
        date_ms: u.date.map_or(0, |d| (d.unix_timestamp_nanos() / 1_000_000) as i64),
    });
    if let Ok(mut p) = pending.0.lock() {
        *p = found;
    }
    Ok(out)
}

/// Tải + kiểm chữ ký + chạy bộ cài (passive, tự mở lại app) rồi thoát. Chỉ trả về khi lỗi; bản đã báo vẫn được giữ
/// để bấm thử lại.
#[tauri::command]
pub async fn update_install(app: AppHandle, pending: State<'_, Pending>) -> Result<(), String> {
    let update = pending
        .0
        .lock()
        .ok()
        .and_then(|p| p.clone())
        .ok_or_else(|| tr!("update.err.none"))?;
    util::log_line(&format!("Cập nhật lên {} …", update.version));
    let mut got: u64 = 0;
    let mut last = Instant::now() - PROGRESS_EVERY;
    let emit = app.clone();
    let res = update
        .download_and_install(
            move |chunk, total| {
                got += chunk as u64;
                if last.elapsed() >= PROGRESS_EVERY {
                    last = Instant::now();
                    let _ = emit.emit("update_progress", Progress { stage: "download", got, total: total.unwrap_or(0) });
                }
            },
            {
                let app = app.clone();
                move || {
                    let _ = app.emit("update_progress", Progress { stage: "install", got: 0, total: 0 });
                }
            },
        )
        .await;
    // tới đây chỉ khi lỗi (cài thành công thì plugin đã chạy bộ cài và thoát app)
    let e = res.err().map_or_else(|| tr!("update.err.install", err = "?"), |e| tr!("update.err.install", err = e));
    util::log_line(&format!("Cập nhật lỗi: {e}"));
    Err(e)
}
