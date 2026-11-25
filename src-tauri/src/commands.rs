// Các command Tauri. Việc nặng / có thể chờ đều chạy trong spawn_blocking để giao diện không bị treo.
use std::collections::HashMap;
use crate::tr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::core::{actions, ai, apps, cleanup, gpu, icons, metrics, proc_actions, registry, scan, util, Item, ScanResult};
use crate::state::{AppState, MetricsHistory};

type St<'a> = State<'a, Arc<AppState>>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Dọn dẹp

#[tauri::command]
pub async fn cleanup_scan(app: AppHandle, state: St<'_>) -> Result<cleanup::ScanOut, String> {
    let state = state.inner().clone();
    // mục đo xong sau khi đã trả kết quả -> sự kiện `cleanup_item`
    blocking(move || state.cleanup.scan(move |scan, item| { let _ = app.emit("cleanup_item", serde_json::json!({ "scan": scan, "item": item })); })).await?
}

/// Dọn các mục (theo mã) của lần quét `id`. UI không bao giờ gửi đường dẫn.
/// Trong lúc dọn phát `cleanup_progress` ~4 lần/giây (bỏ qua khi cửa sổ ẩn / thu nhỏ; bước cuối luôn gửi).
#[tauri::command]
pub async fn cleanup_apply(app: AppHandle, state: St<'_>, id: u64, items: Vec<String>, keep_recent: bool) -> Result<cleanup::ApplyOut, String> {
    let state = state.inner().clone();
    blocking(move || {
        let st = state.clone();
        state.cleanup.apply(id, &items, keep_recent, move |p| {
            if p.stage != "delete" || st.visible.load(Ordering::Relaxed) {
                let _ = app.emit("cleanup_progress", p);
            }
        })
    })
    .await?
}

// ---------------------------------------------------------------------------
// Ứng dụng đã cài (core/apps.rs)

#[tauri::command]
pub async fn apps_list(force: Option<bool>) -> Result<apps::ListOut, String> {
    blocking(move || {
        if force == Some(true) {
            crate::core::leftovers::invalidate();
            apps::clear_sizes();
        }
        let r = apps::list_with(force == Some(true));
        // app có lần trước mà nay mất = đã gỡ (kể cả gỡ ngoài app) -> nhớ để tìm phần còn sót
        crate::core::leftovers::note_seen(&r.apps);
        r
    })
    .await
}

/// Thư mục cài đặt + các thư mục liên quan của app (đã đo, ≤ ~5 s).
#[tauri::command]
pub async fn apps_related(id: String) -> Result<Vec<crate::core::leftovers::Found>, String> {
    blocking(move || crate::core::leftovers::related(&id)).await?
}

/// Phần còn sót của các app đã gỡ + dữ liệu app Store mồ côi + WindowsApps (≤ ~15 s).
#[tauri::command]
pub async fn leftovers_scan() -> Result<crate::core::leftovers::ScanOut, String> {
    blocking(crate::core::leftovers::scan).await
}

/// Gỡ gói chỉ còn ở tài khoản khác (cho mọi tài khoản). Trả [{ name, ok, error }].
#[tauri::command]
pub async fn leftovers_remove_packages(names: Vec<String>) -> Result<Vec<serde_json::Value>, String> {
    blocking(move || {
        crate::core::leftovers::remove_packages(&names)
            .into_iter()
            .map(|(n, r)| serde_json::json!({ "name": n, "ok": r.is_ok(), "error": r.err() }))
            .collect()
    })
    .await
}

/// Không nhắc phần còn sót của app đã gỡ này nữa.
#[tauri::command]
pub async fn leftovers_forget(name: String) -> Result<(), String> {
    blocking(move || crate::core::leftovers::forget(&name)).await
}

/// Lần đo dung lượng đang chạy (gọi đo mới / rời trang = dừng lần cũ).
static APPS_MEASURE: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

fn stop_apps_measure(next: Option<Arc<AtomicBool>>) {
    if let Ok(mut g) = APPS_MEASURE.lock() {
        if let Some(old) = std::mem::replace(&mut *g, next) {
            old.store(true, Ordering::Relaxed);
        }
    }
}

/// Đo thư mục cài đặt; mỗi thư mục xong -> sự kiện `app_size { gen, path, bytes, files, done }`.
#[tauri::command]
pub async fn apps_measure(app: AppHandle, gen: u64, paths: Vec<String>) -> Result<(), String> {
    let stop = Arc::new(AtomicBool::new(false));
    stop_apps_measure(Some(stop.clone()));
    blocking(move || {
        apps::measure(&paths, std::time::Duration::from_secs(180), &stop, |path, bytes, files, done| {
            let _ = app.emit("app_size", serde_json::json!({ "gen": gen, "path": path, "bytes": bytes, "files": files, "done": done }));
        })
    })
    .await
}

#[tauri::command]
pub fn apps_measure_stop() {
    stop_apps_measure(None);
}

/// Chạy trình gỡ của app (hoặc gỡ gói Store). Trả tên app khi đã bắt đầu; xong -> sự kiện `app_uninstall`.
#[tauri::command]
pub async fn apps_uninstall(app: AppHandle, state: St<'_>, id: String) -> Result<String, String> {
    let st = state.inner().clone();
    blocking(move || {
        let before = apps::find(&id);
        apps::uninstall(&id, move |r| {
            st.wake();
            if let (true, Some(a)) = (r.removed, before.as_ref()) {
                crate::core::leftovers::note_removed(a);
            }
            let _ = app.emit("app_uninstall", &r);
        })
    })
    .await?
}

/// Thôi chờ trình gỡ đang chạy (nó vẫn chạy tiếp; app đọc lại registry rồi báo `app_uninstall` như bình thường).
#[tauri::command]
pub fn apps_uninstall_stop() {
    apps::stop_waiting();
}

#[tauri::command]
pub async fn apps_modify(id: String) -> Result<(), String> {
    blocking(move || apps::modify(&id)).await?
}

/// Mở thư mục cài đặt (`folder`) hoặc khoá registry (`registry`) của app. Đọc lại theo id, không nhận đường dẫn từ UI.
#[tauri::command]
pub async fn apps_open(id: String, what: String) -> Result<(), String> {
    blocking(move || {
        let a = apps::find(&id).ok_or_else(|| tr!("apps.err.gone"))?;
        match what.as_str() {
            "folder" if !a.location.is_empty() && shell::exists(&a.location) => shell::open_target(&a.location),
            "folder" => Err(tr!("apps.err.no_folder")),
            "registry" if a.kind == "win32" => {
                registry::set_regedit_last_key(&a.key);
                std::process::Command::new("regedit.exe").arg("/m").spawn().map(|_| ()).map_err(|e| e.to_string())
            }
            _ => Err(tr!("apps.err.unsupported")),
        }
    })
    .await?
}

// ---------------------------------------------------------------------------
// Khởi động & dịch vụ
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn scan(state: St<'_>) -> Result<ScanResult, String> {
    let st = state.inner().clone();
    blocking(move || {
        let r = scan::run();
        st.set_tracked(&r.startup, &r.services);
        let on: Vec<&Item> = r.startup.iter().filter(|i| i.on).collect();
        let third: Vec<&&Item> = on.iter().filter(|i| !i.is_ms).collect();
        let mut sum = format!(
            "Khởi động cùng Windows: {} mục đang bật / {} (bên thứ ba đang bật: {}). Dịch vụ: {} ({} đang chạy).\n",
            on.len(),
            r.startup.len(),
            third.len(),
            r.services.len(),
            r.services.iter().filter(|s| s.state == "running").count()
        );
        let names: Vec<&str> = third.iter().take(25).map(|i| i.name.as_str()).collect();
        if !names.is_empty() {
            sum.push_str(&format!("Mục khởi động bên thứ ba đang bật: {}.\n", names.join(", ")));
        }
        if let Ok(mut s) = st.scan_summary.lock() {
            *s = sum;
        }
        r
    })
    .await
}

#[derive(Serialize)]
pub struct ToggleRes {
    id: String,
    ok: bool,
    item: Option<Item>,
    error: Option<String>,
}

#[tauri::command]
pub async fn toggle_items(state: St<'_>, items: Vec<Item>, on: bool) -> Result<Vec<ToggleRes>, String> {
    let st = state.inner().clone();
    blocking(move || {
        items
            .into_iter()
            .map(|it| {
                let id = it.id.clone();
                match actions::toggle(it, on) {
                    Ok(it) => {
                        if it.kind == "Service" || it.kind == "Svc" {
                            st.update_service(&it.key1, it.pid);
                        }
                        ToggleRes { id, ok: true, item: Some(it), error: None }
                    }
                    Err(e) => ToggleRes { id, ok: false, item: None, error: Some(e) },
                }
            })
            .collect()
    })
    .await
}

#[derive(Serialize)]
pub struct SvcRes {
    name: String,
    ok: bool,
    error: Option<String>,
    service: Option<Item>,
}

/// Chạy thao tác dịch vụ song song (chạy/dừng phải chờ dịch vụ phản hồi).
fn svc_batch(st: &AppState, names: Vec<String>, f: impl Fn(&str) -> Result<Item, String> + Sync) -> Vec<SvcRes> {
    let res: Vec<SvcRes> = std::thread::scope(|s| {
        let hs: Vec<_> = names
            .iter()
            .map(|n| {
                let f = &f;
                s.spawn(move || match f(n) {
                    Ok(it) => SvcRes { name: n.clone(), ok: true, error: None, service: Some(it) },
                    Err(e) => {
                        let fresh = scan::refresh_service(n).ok();
                        SvcRes { name: n.clone(), ok: false, error: Some(e), service: fresh }
                    }
                })
            })
            .collect();
        hs.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    for r in &res {
        if let Some(s) = &r.service {
            st.update_service(&s.key1, s.pid);
        }
    }
    res
}

#[tauri::command]
pub async fn service_set_mode(state: St<'_>, names: Vec<String>, mode: String) -> Result<Vec<SvcRes>, String> {
    let st = state.inner().clone();
    blocking(move || svc_batch(&st, names, |n| actions::service_mode(n, &mode))).await
}

#[tauri::command]
pub async fn service_control(state: St<'_>, names: Vec<String>, action: String) -> Result<Vec<SvcRes>, String> {
    let st = state.inner().clone();
    let start = match action.as_str() {
        "start" => true,
        "stop" => false,
        _ => return Err(tr!("app.err.bad_action", action = action)),
    };
    blocking(move || svc_batch(&st, names, |n| actions::service_control(n, start))).await
}

#[tauri::command]
pub async fn open_location(item: Item) -> Result<(), String> {
    blocking(move || actions::open_location(&item)).await
}

#[derive(Serialize)]
pub struct Settings {
    restart_apps: bool,
    is_admin: bool,
    log_path: String,
    /// thư mục chứa log.txt
    log_dir: String,
    /// người dùng đã chọn thư mục log riêng (không phải mặc định)
    log_dir_custom: bool,
    /// thư mục dữ liệu của app (LOCALAPPDATA, thư mục fomeg-windows-app)
    data_dir: String,
}

#[tauri::command]
pub fn get_settings() -> Settings {
    let s = |p: Option<std::path::PathBuf>| p.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    Settings {
        restart_apps: registry::read_restart_apps(),
        is_admin: crate::is_elevated(),
        log_path: s(util::log_path()),
        log_dir: s(util::log_dir()),
        log_dir_custom: util::log_dir_is_custom(),
        data_dir: s(util::data_dir()),
    }
}

/// Đổi thư mục lưu log (Cài đặt → Nhật ký); `dir` null = về mặc định `%LOCALAPPDATA%\fomeg-windows-app`.
/// Trả lại Settings mới để giao diện cập nhật đường dẫn.
#[tauri::command]
pub async fn set_log_dir(dir: Option<String>) -> Result<Settings, String> {
    blocking(move || {
        util::set_log_dir(dir.as_deref())?;
        util::log_line(&format!("Thư mục log: {}", util::log_dir().map(|p| p.display().to_string()).unwrap_or_default()));
        Ok(get_settings())
    })
    .await?
}

#[tauri::command]
pub fn set_restart_apps(on: bool) -> Result<(), String> {
    registry::set_restart_apps(on)?;
    util::log_line(&format!("Restart apps = {on}"));
    Ok(())
}

/// MCP server chỉ đọc cho coding agent: đường dẫn `fomeg-windows-app-mcp.exe` và có sẵn chưa.
#[derive(Serialize)]
pub struct McpInfo {
    path: String,
    exists: bool,
    /// exe nằm sẵn trong app (không phải build/copy riêng)
    embedded: bool,
}

/// Bản release nhúng sẵn fomeg-windows-app-mcp.exe và tự giải ra %LOCALAPPDATA%\fomeg-windows-app (`embedded`); bản dev tìm cạnh exe.
#[tauri::command]
pub fn mcp_info() -> McpInfo {
    let embedded = crate::MCP_EXE.get().is_some_and(|b| !b.is_empty());
    let path = if embedded {
        crate::core::mcp::installed_path()
    } else {
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("fomeg-windows-app-mcp.exe")))
    }
    .unwrap_or_default();
    McpInfo { exists: path.is_file(), embedded, path: path.to_string_lossy().into_owned() }
}

#[tauri::command]
pub fn open_log() -> Result<(), String> {
    let p = util::log_path().ok_or_else(|| tr!("app.err.no_log_dir"))?;
    if !p.exists() {
        util::log_line("Tạo file log.");
    }
    std::process::Command::new("notepad.exe").arg(p).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Serialize)]
pub struct RamHistory {
    total: u64,
    points: Vec<(i64, u64)>,
}

#[tauri::command]
pub fn ram_history(state: St<'_>) -> RamHistory {
    let points = state.history.lock().map(|h| h.iter().map(|p| (p.t, p.mem)).collect()).unwrap_or_default();
    RamHistory { total: state.total_mem.load(Ordering::Relaxed), points }
}

// ---------------------------------------------------------------------------
// Hiệu năng
// ---------------------------------------------------------------------------
#[derive(Serialize, Clone)]
pub struct SystemInfoOut {
    #[serde(flatten)]
    base: metrics::SystemInfo,
    gpus: Vec<gpu::GpuInfo>,
}

static SYSINFO: LazyLock<std::sync::Mutex<Option<SystemInfoOut>>> = LazyLock::new(|| std::sync::Mutex::new(None));

#[tauri::command]
pub async fn system_info() -> Result<SystemInfoOut, String> {
    if let Some(v) = SYSINFO.lock().ok().and_then(|g| g.clone()) {
        return Ok(v);
    }
    let v = blocking(|| SystemInfoOut { base: metrics::system_info(), gpus: gpu::adapter_info() }).await?;
    if let Ok(mut g) = SYSINFO.lock() {
        *g = Some(v.clone());
    }
    Ok(v)
}

#[tauri::command]
pub fn metrics_history(state: St<'_>) -> MetricsHistory {
    state.history_columns()
}

#[tauri::command]
pub fn set_interval(state: St<'_>, ms: u64) -> Result<(), String> {
    if !matches!(ms, 0 | 500 | 1_000 | 2_000 | 5_000) {
        return Err(tr!("app.err.bad_interval"));
    }
    state.interval_ms.store(ms, Ordering::Relaxed);
    state.wake();
    Ok(())
}

/// Ngôn ngữ của thông báo backend và câu trả lời của trợ lý: "vi" (mặc định) hoặc "en".
#[tauri::command]
pub fn set_locale(locale: String) {
    crate::core::i18n::set(&locale);
}

/// Chu kỳ lấy mẫu khi cửa sổ ẩn: 5 s (mặc định), 15 s, 60 s hoặc 0 = không lấy mẫu (lịch sử có khoảng trống).
#[tauri::command]
pub fn set_idle_interval(state: St<'_>, ms: u64) -> Result<(), String> {
    if !matches!(ms, 0 | 5_000 | 15_000 | 60_000) {
        return Err(tr!("app.err.bad_idle_interval"));
    }
    state.idle_ms.store(ms, Ordering::Relaxed);
    state.wake();
    Ok(())
}

/// Mở lại app bằng quyền Admin (hộp thoại UAC) rồi đóng bản hiện tại. Người dùng bấm Không -> báo lỗi, app vẫn chạy.
#[tauri::command]
pub async fn relaunch_admin(app: AppHandle) -> Result<(), String> {
    blocking(|| {
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = util::wide(&exe.to_string_lossy());
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(exe.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| tr!("app.err.relaunch_admin", e = util::win_err(&e)))
    })
    .await??;
    util::log_line("Mở lại bằng quyền Admin.");
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn set_visible(state: St<'_>, visible: bool) {
    state.visible.store(visible, Ordering::Relaxed);
    state.wake();
}

// ---------------------------------------------------------------------------
// Tiến trình
// ---------------------------------------------------------------------------
#[tauri::command]
pub fn set_watch(state: St<'_>, procs: bool) {
    // Đặt cờ reset TRƯỚC khi bật theo dõi để lần phát đầu tiên chắc chắn là danh sách đầy đủ.
    if procs {
        state.reset_procs.store(true, Ordering::SeqCst);
    }
    let was = state.watch_procs.swap(procs, Ordering::SeqCst);
    if procs && !was {
        state.wake();
    }
}

#[derive(Serialize)]
pub struct KillRes {
    pid: u32,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
pub struct ProcLookup {
    /// Đích trợ lý viết (PID hoặc tên exe).
    target: String,
    pid: u32,
    name: String,
    exe: String,
    desc: String,
    mem: u64,
    /// Số tiến trình con trực tiếp.
    children: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocked: Option<String>,
}

/// Kiểm tra lại các tiến trình trợ lý đề xuất kết thúc (khối `ket_thuc`): đích là PID hoặc tên exe (mọi tiến trình
/// cùng tên). Một lần lấy mẫu riêng (~vài chục ms), không đụng vòng monitor. Chỉ đọc.
#[tauri::command]
pub async fn proc_lookup(targets: Vec<String>) -> Result<Vec<ProcLookup>, String> {
    blocking(move || {
        let mut sampler = crate::core::procs::ProcSampler::new();
        let rows: HashMap<u32, u64> = sampler.sample().rows.into_iter().map(|r| (r.pid, r.mem)).collect();
        let all = sampler.all_static();
        let mut kids: HashMap<u32, u32> = HashMap::new();
        for p in &all {
            *kids.entry(p.ppid).or_default() += 1;
        }
        let me = std::process::id();
        let mut out = Vec::new();
        for t in targets.iter().take(50) {
            let key = t.trim().trim_start_matches("PID").trim();
            let found: Vec<_> = match key.parse::<u32>() {
                Ok(pid) => all.iter().filter(|p| p.pid == pid).collect(),
                Err(_) => all.iter().filter(|p| p.name.eq_ignore_ascii_case(key)).collect(),
            };
            if found.is_empty() {
                out.push(ProcLookup { target: t.clone(), pid: key.parse().unwrap_or(0), name: String::new(), exe: String::new(), desc: String::new(), mem: 0, children: 0, blocked: Some(tr!("procs.lookup.not_found")) });
            }
            for p in found.into_iter().take(40) {
                let n = p.name.to_ascii_lowercase();
                let blocked = if p.pid == 0 || p.pid == 4 || ["system", "registry", "memory compression", "csrss.exe", "wininit.exe", "winlogon.exe", "smss.exe", "services.exe", "lsass.exe"].contains(&n.as_str()) {
                    Some(tr!("procs.lookup.critical"))
                } else if p.pid == me {
                    Some(tr!("procs.lookup.self"))
                } else if n == "svchost.exe" {
                    Some(tr!("procs.lookup.svchost"))
                } else {
                    None
                };
                out.push(ProcLookup {
                    target: t.clone(),
                    pid: p.pid,
                    name: p.name.clone(),
                    exe: p.exe.clone(),
                    desc: p.desc.clone(),
                    mem: rows.get(&p.pid).copied().unwrap_or(0),
                    children: kids.get(&p.pid).copied().unwrap_or(0),
                    blocked,
                });
            }
        }
        out
    })
    .await
}

#[tauri::command]
pub async fn proc_kill(state: St<'_>, pids: Vec<u32>, tree: bool) -> Result<Vec<KillRes>, String> {
    let st = state.inner().clone();
    let r = blocking(move || {
        let r: Vec<KillRes> = pids
            .into_iter()
            .map(|pid| match proc_actions::kill(pid, tree) {
                Ok(()) => {
                    util::log_line(&format!("[Tiến trình] Kết thúc PID {pid}{}", if tree { " (cả cây)" } else { "" }));
                    KillRes { pid, ok: true, error: None }
                }
                Err(e) => KillRes { pid, ok: false, error: Some(e) },
            })
            .collect();
        st.wake();
        r
    })
    .await?;
    Ok(r)
}

#[derive(Serialize)]
pub struct RestartRes {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tauri::command]
pub async fn proc_restart(state: St<'_>, pid: u32) -> Result<RestartRes, String> {
    let st = state.inner().clone();
    blocking(move || {
        let r = match proc_actions::restart(pid) {
            Ok(n) => {
                util::log_line(&format!("[Tiến trình] Khởi động lại PID {pid} -> {n}"));
                RestartRes { ok: true, pid: Some(n), error: None }
            }
            Err(e) => RestartRes { ok: false, pid: None, error: Some(e) },
        };
        st.wake();
        r
    })
    .await
}

#[tauri::command]
pub async fn proc_open_location(path: String) -> Result<(), String> {
    blocking(move || proc_actions::open_location(&path)).await?
}

#[tauri::command]
pub async fn proc_properties(path: String) -> Result<(), String> {
    blocking(move || proc_actions::properties(&path)).await?
}

#[derive(Serialize)]
pub struct OkRes {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tauri::command]
pub async fn proc_set_priority(pid: u32, level: String) -> Result<OkRes, String> {
    blocking(move || match proc_actions::set_priority(pid, &level) {
        Ok(()) => OkRes { ok: true, error: None },
        Err(e) => OkRes { ok: false, error: Some(e) },
    })
    .await
}

#[tauri::command]
pub async fn get_icons(paths: Vec<String>) -> Result<HashMap<String, Option<String>>, String> {
    blocking(move || paths.into_iter().map(|p| {
        let v = icons::icon_data_url(&p);
        (p, v)
    }).collect())
    .await
}

// ---------------------------------------------------------------------------
// Trợ lý AI
// ---------------------------------------------------------------------------
#[tauri::command(rename_all = "snake_case")]
pub async fn ai_get_config() -> Result<ai::AiConfigView, String> {
    blocking(ai::get_config).await
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)] // mỗi trường cài đặt là một tham số tuỳ chọn của command
pub async fn ai_set_config(
    provider: Option<String>,
    include_context: Option<bool>,
    tools: Option<bool>,
    target: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<ai::AiConfigView, String> {
    let update = ai::AiConfigUpdate {
        provider,
        include_context,
        tools,
        target,
        model,
        effort,
        base_url,
        api_key,
    };
    blocking(move || ai::set_config(update)).await?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ai_list_models(provider: String) -> Result<Vec<String>, String> {
    blocking(move || ai::list_models(&provider)).await?
}

/// `disk`: gửi kèm tóm tắt lần quét ổ cứng hiện tại (`{focus: đường dẫn | null}`), None = không gửi.
#[derive(serde::Deserialize)]
pub struct DiskAsk {
    #[serde(default)]
    focus: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
pub fn ai_send(
    app: AppHandle,
    state: St<'_>,
    chat_id: String,
    text: String,
    disk: Option<DiskAsk>,
    ports: Option<bool>,
    cleanup: Option<bool>,
) -> Result<(), String> {
    let st = state.inner().clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let config;
    {
        let mut m = st.ai_cancel.lock().map_err(|e| e.to_string())?;
        if m.contains_key(&chat_id) {
            return Err(tr!("ai.err.busy"));
        }
        config = ai::load_config()?.prepare_send()?;
        m.insert(chat_id.clone(), cancel.clone());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let context = if config.include_context() {
            st.context.lock().ok().map(|c| c.clone()).filter(|c| !c.is_empty())
        } else {
            None
        };
        // Giữ lại sự kiện kết thúc cho tới khi đã giải phóng lượt, để client gửi tiếp ngay được.
        let terminal: std::sync::Mutex<Option<ai::AiEvent>> = std::sync::Mutex::new(None);
        let emit = |e: ai::AiEvent| match e {
            ai::AiEvent::Done { .. } | ai::AiEvent::Error { .. } => {
                if let Ok(mut t) = terminal.lock() {
                    *t = Some(e);
                }
            }
            other => {
                let _ = app.emit("ai", other);
            }
        };
        // tóm tắt ổ đĩa tính trên luồng nền này (vài ms, chỉ đi các nhánh lớn)
        let disk = disk.and_then(|d| {
            let scan = st.du.lock().ok().and_then(|g| g.clone())?;
            Some(scan.ai_summary(d.focus.as_deref()))
        });
        let mut blocks: Vec<(&str, String)> = Vec::new();
        if let Some(d) = disk {
            blocks.push(("disk_scan", d));
        }
        // danh sách cổng: đọc lại ngay (~2 ms), tên tiến trình từ lần đo gần nhất của monitor
        if ports == Some(true) {
            if let Ok(mut uses) = crate::core::ports::list(true) {
                fill_port_procs(&st, &mut uses);
                blocks.push(("ports", crate::core::ports::ai_summary(&uses)));
            }
        }
        // kết quả quét gần nhất của trang Dọn dẹp (không quét lại)
        if cleanup == Some(true) {
            if let Some(c) = st.cleanup.ai_summary() {
                blocks.push(("cleanup", c));
            }
        }
        // bộ tool chỉ đọc cho lượt này (lấy mẫu khi được gọi, bỏ đi khi xong lượt)
        let view = AppView(st.clone());
        let mut toolbox = crate::core::agent_tools::Toolbox::new(Some(&view));
        let defs = toolbox.tools();
        let mut run = |name: &str, args: &serde_json::Value| toolbox.run(name, args);
        let kit = ai::ToolKit { defs, run: &mut run, label: crate::core::agent_tools::label };
        st.chats.send(config, &chat_id, &text, context, &blocks, Some(kit), &cancel, &emit);
        if let Ok(mut m) = st.ai_cancel.lock() {
            m.remove(&chat_id);
        }
        if let Some(e) = terminal.into_inner().ok().flatten() {
            let _ = app.emit("ai", e);
        }
    });
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub fn ai_cancel(state: St<'_>, chat_id: String) {
    if let Ok(m) = state.ai_cancel.lock() {
        if let Some(c) = m.get(&chat_id) {
            c.store(true, Ordering::SeqCst);
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn ai_reset(state: St<'_>, chat_id: String) {
    ai_cancel(state.clone(), chat_id.clone());
    state.chats.reset(&chat_id);
}

// ---------------------------------------------------------------------------
// Ổ cứng (phân tích dung lượng kiểu TreeSize) – xem core/du.rs, core/shell.rs
// ---------------------------------------------------------------------------
use crate::core::{du, shell};

fn du_current(state: &AppState, scan: u64) -> Result<Arc<du::Scan>, String> {
    let g = state.du.lock().map_err(|_| tr!("errors.lock"))?;
    match g.as_ref() {
        Some(s) if s.id == scan => Ok(s.clone()),
        _ => Err(tr!("du.err.scan_ended")),
    }
}

#[tauri::command]
pub async fn du_drives() -> Result<Vec<du::Drive>, String> {
    blocking(du::drives).await
}

/// Bắt đầu quét (huỷ lần quét trước). Sự kiện `du` (Progress) phát ~4 lần/giây tới khi xong.
#[tauri::command]
pub async fn du_start(app: AppHandle, state: St<'_>, root: String) -> Result<du::Progress, String> {
    let st = state.inner().clone();
    blocking(move || {
        let id = st.du_seq.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(old) = st.du.lock().ok().and_then(|mut g| g.take()) {
            old.cancel();
        }
        let scan = du::Scan::start(id, &root)?;
        if let Ok(mut g) = st.du.lock() {
            *g = Some(scan.clone());
        }
        let first = scan.progress();
        let weak = Arc::downgrade(&scan);
        let _ = std::thread::Builder::new().name("du-progress".into()).spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let Some(s) = weak.upgrade() else { break };
            let p = s.progress();
            let done = p.done;
            let _ = app.emit("du", p);
            if done {
                break;
            }
        });
        Ok(first)
    })
    .await?
}

#[tauri::command(rename_all = "snake_case")]
pub fn du_progress(state: St<'_>) -> Option<du::Progress> {
    state.du.lock().ok().and_then(|g| g.as_ref().map(|s| s.progress()))
}

/// Con của nhiều thư mục cùng lúc (các nút đang mở) – một lần gọi mỗi nhịp làm mới.
#[tauri::command(rename_all = "snake_case")]
pub fn du_children(state: St<'_>, scan: u64, ids: Vec<u32>, sort: String, desc: bool, limit: usize) -> Result<Vec<du::ChildrenOut>, String> {
    let s = du_current(&state, scan)?;
    let limit = limit.clamp(1, 5000);
    Ok(ids.into_iter().filter_map(|id| s.children(id, &sort, desc, limit)).collect())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn du_files(path: String, limit: usize) -> Result<du::FilesOut, String> {
    blocking(move || du::list_files(&path, limit.clamp(1, 20000))).await?
}

#[tauri::command(rename_all = "snake_case")]
pub fn du_cancel(state: St<'_>) {
    if let Some(s) = state.du.lock().ok().and_then(|g| g.clone()) {
        s.cancel();
    }
}

/// Đóng kết quả quét, giải phóng bộ nhớ của cây.
#[tauri::command(rename_all = "snake_case")]
pub fn du_close(state: St<'_>) {
    if let Some(s) = state.du.lock().ok().and_then(|mut g| g.take()) {
        s.cancel();
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn du_forget(state: St<'_>, scan: u64, id: u32) -> Result<(), String> {
    du_current(&state, scan)?.forget(id);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub fn du_forget_file(state: St<'_>, scan: u64, dir: u32, size: u64, alloc: u64) -> Result<(), String> {
    du_current(&state, scan)?.forget_file(dir, size, alloc);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn du_delete(paths: Vec<String>, permanent: bool) -> Result<bool, String> {
    if let Some(p) = paths.iter().find(|p| du::protected(p)) {
        return Err(tr!("du.err.protected_path", p = p));
    }
    blocking(move || shell::delete(&paths, permanent)).await?
}

#[derive(Serialize)]
pub struct DuLookup {
    /// đường dẫn đã chuẩn hoá (hoặc nguyên văn nếu không hợp lệ)
    path: String,
    exists: bool,
    is_dir: bool,
    /// byte; None = thư mục không nằm trong lần quét (không tự quét thêm)
    size: Option<u64>,
    files: Option<u64>,
    /// lý do không cho xoá (đường dẫn hệ thống / không hợp lệ / không tồn tại)
    #[serde(skip_serializing_if = "Option::is_none")]
    blocked: Option<String>,
}

fn block_reason(path: &str) -> Option<String> {
    if du::protected(path) {
        Some(tr!("du.err.protected"))
    } else {
        None
    }
}

/// Thông tin các đường dẫn AI đề xuất xoá (để người dùng xem lại trước khi chọn).
#[tauri::command(rename_all = "snake_case")]
pub async fn du_lookup(state: St<'_>, paths: Vec<String>) -> Result<Vec<DuLookup>, String> {
    let scan = state.du.lock().ok().and_then(|g| g.clone());
    blocking(move || {
        paths
            .into_iter()
            .take(200)
            .map(|raw| {
                let Some(path) = du::normalize_abs(&raw) else {
                    return DuLookup { path: raw, exists: false, is_dir: false, size: None, files: None, blocked: Some(tr!("du.err.not_absolute")) };
                };
                let meta = std::fs::symlink_metadata(&path).ok();
                let exists = meta.is_some();
                let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
                let (size, files) = if is_dir {
                    scan.as_ref().and_then(|s| s.dir_stats(&path)).map_or((None, None), |(sz, f, _)| (Some(sz), Some(f)))
                } else {
                    (meta.as_ref().map(|m| m.len()), None)
                };
                let blocked = if exists { block_reason(&path) } else { Some(tr!("du.err.gone")) };
                DuLookup { path, exists, is_dir, size, files, blocked }
            })
            .collect()
    })
    .await
}

#[derive(Serialize)]
pub struct DuDeleted {
    path: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Xoá nhiều mục người dùng đã chọn (vào Thùng rác hoặc vĩnh viễn), trả kết quả từng mục và bỏ các mục
/// đã xoá khỏi cây quét hiện tại. Một lần SHFileOperation cho cả nhóm; lỗi thì thử lại từng mục để biết mục nào hỏng.
#[tauri::command(rename_all = "snake_case")]
pub async fn du_delete_paths(state: St<'_>, paths: Vec<String>, permanent: bool) -> Result<Vec<DuDeleted>, String> {
    let scan = state.du.lock().ok().and_then(|g| g.clone());
    blocking(move || {
        let mut out = Vec::with_capacity(paths.len());
        // (đường dẫn, thư mục?, kích thước tệp) của các mục được phép xoá
        let mut todo: Vec<(String, bool, u64)> = Vec::new();
        for raw in paths.into_iter().take(200) {
            let Some(path) = du::normalize_abs(&raw) else {
                out.push(DuDeleted { path: raw, ok: false, error: Some(tr!("du.err.not_absolute")) });
                continue;
            };
            if let Some(e) = block_reason(&path) {
                out.push(DuDeleted { path, ok: false, error: Some(e) });
                continue;
            }
            match std::fs::symlink_metadata(&path) {
                Ok(m) => todo.push((path, m.is_dir(), m.len())),
                Err(_) => out.push(DuDeleted { path, ok: false, error: Some(tr!("du.err.gone")) }),
            }
        }
        // thư mục chứa tệp ("C:\a.txt" -> "C:\"), để trừ tệp khỏi số liệu của cây
        let parent_of = |p: &str| p.rfind('\\').map(|i| if i == 2 { p[..3].to_owned() } else { p[..i].to_owned() });
        let all: Vec<String> = todo.iter().map(|t| t.0.clone()).collect();
        let batch = shell::delete(&all, permanent);
        apps::forget_sizes(&all);
        crate::core::leftovers::invalidate();
        for (path, is_dir, len) in todo {
            let mut error = None;
            if shell::exists(&path) && batch.is_err() {
                // thử lại riêng mục này để lấy lỗi cụ thể
                if let Err(e) = shell::delete(std::slice::from_ref(&path), permanent) {
                    error = Some(e);
                }
            }
            let ok = !shell::exists(&path);
            if ok {
                if let Some(s) = &scan {
                    if is_dir {
                        if let Some(id) = s.find(&path) {
                            s.forget(id);
                        }
                    } else if let Some(dir) = parent_of(&path).and_then(|d| s.find(&d)) {
                        s.forget_file(dir, len, len);
                    }
                }
            } else if error.is_none() {
                error = Some(batch.as_ref().err().cloned().unwrap_or_else(|| tr!("du.err.not_deleted")));
            }
            out.push(DuDeleted { path, ok, error });
        }
        out
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub fn du_exists(path: String) -> bool {
    shell::exists(&path)
}

/// Mở trong Explorer: thư mục -> mở thư mục; tệp -> mở thư mục chứa và chọn tệp.
#[tauri::command(rename_all = "snake_case")]
pub async fn du_open(path: String, is_dir: bool) -> Result<(), String> {
    blocking(move || {
        if is_dir {
            let explorer = format!(r"{}\explorer.exe", util::windir());
            std::process::Command::new(explorer).arg(&path).spawn().map(|_| ()).map_err(|e| tr!("du.err.open_explorer", e = e))
        } else {
            proc_actions::open_location(&path)
        }
    })
    .await?
}

fn main_hwnd(window: &tauri::WebviewWindow) -> Result<isize, String> {
    window.hwnd().map(|h| h.0 as isize).map_err(|e| tr!("app.err.no_window", e = e))
}

/// Chạy `f(hwnd)` trên luồng giao diện (menu / hộp thoại của Windows cần luồng sở hữu cửa sổ).
async fn on_ui<T: Send + 'static>(window: tauri::WebviewWindow, f: impl FnOnce(windows::Win32::Foundation::HWND) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let hwnd = main_hwnd(&window)?;
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(f(windows::Win32::Foundation::HWND(hwnd as *mut std::ffi::c_void)));
        })
        .map_err(|e| e.to_string())?;
    blocking(move || rx.recv().map_err(|_| tr!("du.err.menu_cancelled"))).await??
}

/// Menu chuột phải gốc của Windows cho `path` (kèm "Quét thư mục này" nếu là thư mục).
#[tauri::command(rename_all = "snake_case")]
pub async fn du_menu(window: tauri::WebviewWindow, path: String, is_dir: bool, shift: bool) -> Result<shell::MenuPick, String> {
    on_ui(window, move |hwnd| {
        let ids: &[(&str, &str)] = if is_dir { &[("scan", "du.menu.scan"), ("ask_ai", "du.menu.ask_ai"), ("copy_path", "du.menu.copy_path")] } else { &[("copy_path", "du.menu.copy_path")] };
        let labels: Vec<String> = ids.iter().map(|&(_, k)| tr!(k)).collect();
        let items: Vec<(&str, &str)> = ids.iter().zip(&labels).map(|(&(id, _), l)| (id, l.as_str())).collect();
        shell::context_menu(hwnd, &path, &items, shift)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn du_pick_folder(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    on_ui(window, shell::pick_folder).await
}

// ---------------- Cổng (ports) ----------------

/// Cổng TCP đang nghe (và UDP nếu `udp`) kèm tiến trình giữ cổng. Trang Cổng gọi định kỳ khi đang mở.
/// Tên / CPU / RAM lấy từ lần đo gần nhất của monitor (chỉ cho các PID này), không đo lại.
#[tauri::command]
pub async fn ports_list(state: St<'_>, udp: bool) -> Result<crate::core::ports::PortsOut, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let t = std::time::Instant::now();
        let mut uses = crate::core::ports::list(udp)?;
        let mut pids: Vec<u32> = uses.iter().map(|u| u.pid).collect();
        pids.sort_unstable();
        pids.dedup();
        let changed = st.port_pids.lock().map(|mut g| {
            let c = g.1 != pids;
            *g = (std::time::Instant::now(), pids);
            c
        }).unwrap_or(false);
        let pending = fill_port_procs(&st, &mut uses);
        if changed && pending {
            st.wake(); // đo ngay để lần hỏi sau có tên
        }
        Ok(crate::core::ports::PortsOut { uses, took_ms: t.elapsed().as_secs_f64() * 1000.0, pending })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Điền tên / CPU / RAM của tiến trình giữ cổng từ lần đo gần nhất của monitor. Trả true nếu còn PID chưa có.
fn fill_port_procs(st: &AppState, uses: &mut [crate::core::ports::PortUse]) -> bool {
    let mut pending = false;
    if let Ok(pp) = st.port_procs.lock() {
        for u in uses.iter_mut() {
            match pp.get(&u.pid) {
                Some(p) => {
                    u.name.clone_from(&p.name);
                    u.exe.clone_from(&p.exe);
                    u.cpu = p.cpu;
                    u.mem = p.mem;
                }
                None => pending = true,
            }
        }
    }
    pending
}

/// Mở http://localhost:<port> trong trình duyệt mặc định.
#[tauri::command]
pub async fn open_port(port: u16) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::core::ports::open_local(port)).await.map_err(|e| e.to_string())?
}

/// Mở công cụ quản trị của Windows. Chỉ nhận tên trong danh sách cho phép.
#[tauri::command]
pub async fn open_tool(name: String) -> Result<(), String> {
    let target = match name.as_str() {
        "services" => "services.msc",
        "tasks" => "taskschd.msc",
        "startup_folder" => "shell:startup",
        _ => return Err(tr!("app.err.bad_tool")),
    };
    tauri::async_runtime::spawn_blocking(move || crate::core::shell::open_target(target)).await.map_err(|e| e.to_string())?
}

/// Dữ liệu chỉ app có, cho bộ tool của trợ lý (lịch sử hiệu năng, kết quả quét ổ đĩa / dọn dẹp).
pub(crate) struct AppView(pub(crate) Arc<AppState>);

impl crate::core::agent_tools::AppData for AppView {
    fn history(&self, minutes: u32) -> Option<String> {
        use std::fmt::Write;
        let h = self.0.history.lock().ok()?;
        let newest = h.back()?.t;
        let from = newest - i64::from(minutes) * 60_000;
        let pts: Vec<_> = h.iter().filter(|p| p.t >= from).collect();
        if pts.is_empty() {
            return None;
        }
        let n = pts.len() as f64;
        let stat = |v: &mut dyn Iterator<Item = f64>| {
            let (mut lo, mut hi, mut sum, mut k) = (f64::MAX, f64::MIN, 0.0, 0.0);
            for x in v {
                lo = lo.min(x);
                hi = hi.max(x);
                sum += x;
                k += 1.0;
            }
            if k == 0.0 { (0.0, 0.0, 0.0) } else { (lo, sum / k, hi) }
        };
        let total = self.0.total_mem.load(Ordering::Relaxed).max(1) as f64;
        let (c0, c1, c2) = stat(&mut pts.iter().map(|p| f64::from(p.cpu)));
        let (m0, m1, m2) = stat(&mut pts.iter().map(|p| p.mem as f64 * 100.0 / total));
        let mut s = format!("{} phút gần nhất ({} mẫu). CPU min/tb/max {c0:.0}/{c1:.0}/{c2:.0}% · RAM {m0:.0}/{m1:.0}/{m2:.0}%\n", minutes, n as u64);
        if let Some(peak) = pts.iter().max_by(|a, b| a.cpu.total_cmp(&b.cpu)) {
            let ago = (newest - peak.t) / 1000;
            let _ = writeln!(s, "Đỉnh CPU {:.0}% cách đây {} giây", peak.cpu, ago);
        }
        let mut disks: HashMap<&str, (f32, f64)> = HashMap::new();
        let mut nets: HashMap<&str, (f64, f64)> = HashMap::new();
        let mut gpus: HashMap<&str, f32> = HashMap::new();
        for p in &pts {
            for (id, active, r, w) in &p.disks {
                let e = disks.entry(id.as_str()).or_default();
                e.0 = e.0.max(*active);
                e.1 = e.1.max(r + w);
            }
            for (id, rx, tx) in &p.nets {
                let e = nets.entry(id.as_str()).or_default();
                e.0 = e.0.max(*rx);
                e.1 = e.1.max(*tx);
            }
            for (id, u) in &p.gpus {
                let e = gpus.entry(id.as_str()).or_default();
                *e = e.max(*u);
            }
        }
        for (id, (a, io)) in disks {
            let _ = writeln!(s, "Đĩa {id}: bận tối đa {a:.0}% · đọc+ghi tối đa {}/s", crate::core::du::sz(io as u64));
        }
        for (id, (rx, tx)) in nets.into_iter().filter(|(_, (rx, tx))| rx + tx > 0.0) {
            let _ = writeln!(s, "Mạng {id}: nhận tối đa {}/s · gửi tối đa {}/s", crate::core::du::sz(rx as u64), crate::core::du::sz(tx as u64));
        }
        for (id, u) in gpus {
            let _ = writeln!(s, "GPU {id}: tối đa {u:.0}%");
        }
        Some(s)
    }

    fn disk_summary(&self, focus: Option<&str>) -> Option<String> {
        let scan = self.0.du.lock().ok().and_then(|g| g.clone())?;
        Some(scan.ai_summary(focus))
    }

    fn cleanup_summary(&self) -> Option<String> {
        self.0.cleanup.ai_summary()
    }

    fn app_info(&self) -> String {
        let ms = self.0.interval_ms.load(Ordering::Relaxed);
        format!(
            "Vietnamese's Ultimate W App {} · chạy quyền Admin: {} · cập nhật giao diện: {}",
            env!("CARGO_PKG_VERSION"),
            if crate::is_elevated() { "có" } else { "không" },
            if ms == 0 { "tạm dừng".to_owned() } else { format!("mỗi {ms} ms") }
        )
    }
}
