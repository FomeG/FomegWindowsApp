// Luồng đo duy nhất: mỗi nhịp lấy mẫu tiến trình (1 lời gọi NtQuerySystemInformation), chỉ số hệ thống
// (CPU/RAM/đĩa/mạng), GPU (PDH), rồi phát sự kiện cho giao diện. Khi cửa sổ bị ẩn/thu nhỏ hoặc người dùng
// tạm dừng, chỉ đo thưa (5 s) để giữ lịch sử, không phát sự kiện → gần như không tốn CPU.
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::core::gpu::GpuSampler;
use crate::core::metrics::{Metrics, MetricsSampler};
use crate::core::procs::{ProcSampler, ProcStatic};
use crate::state::{AppState, HistPoint, Matcher, PortProc, HISTORY_MAX, HISTORY_MS, PORT_WATCH_MS};

#[derive(Serialize, Clone)]
struct TopProc {
    name: String,
    mb: u32,
    count: u32,
}

#[derive(Serialize, Clone)]
struct RamTick {
    t: i64,
    used: u64,
    total: u64,
    items: Vec<(String, u32)>,
    top: Vec<TopProc>,
    reset: bool,
}

#[derive(Serialize, Clone)]
struct MetricsEvent<'a> {
    #[serde(flatten)]
    m: &'a Metrics,
    gpus: &'a [crate::core::gpu::GpuMetrics],
}

#[derive(Serialize, Clone)]
struct ProcsEvent<'a> {
    t: i64,
    reset: bool,
    added: Vec<ProcStatic>,
    removed: &'a [u32],
    /// [pid, cpu %, bộ nhớ (byte), đĩa (B/s), gpu %, luồng, handle]
    rows: Vec<(u32, f32, u64, u64, f32, u32, u32)>,
    windows: Option<&'a HashMap<u32, String>>,
}

fn round1(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}

fn window_hidden(app: &AppHandle) -> bool {
    match app.get_webview_window("main") {
        Some(w) => w.is_minimized().unwrap_or(false) || !w.is_visible().unwrap_or(true),
        None => false,
    }
}

fn gb(b: u64) -> f64 {
    b as f64 / 1_073_741_824.0
}

pub fn spawn(app: AppHandle, st: Arc<AppState>) {
    let (tx, rx) = sync_channel::<()>(1);
    if let Ok(mut w) = st.wake.lock() {
        *w = Some(tx);
    }
    std::thread::Builder::new()
        .name("monitor".into())
        .spawn(move || {
            let mut ps = ProcSampler::new();
            let mut ms = MetricsSampler::new();
            let mut gs = GpuSampler::new();
            let mut last_ram: HashMap<String, u32> = HashMap::new();
            let mut tick: u64 = 0;
            let is_active = |st: &AppState| {
                st.interval_ms.load(Ordering::Relaxed) > 0 && st.visible.load(Ordering::Relaxed) && !window_hidden(&app)
            };
            let mut last_start: Option<Instant> = None;
            loop {
                // Chờ tới hạn. Đánh thức (quét xong, thao tác tiến trình/dịch vụ...) chỉ đo ngay khi giao diện đang hiện;
                // khi ẩn/tạm dừng, nó chỉ khiến tính lại hạn chờ.
                if let Some(ls) = last_start {
                    loop {
                        // ẩn + "không lấy mẫu khi ẩn": chỉ chờ, không đo gì. Hiện lại cửa sổ -> set_visible / set_interval đánh thức ngay;
                        // 15 s chỉ là lưới an toàn (vd. bỏ thu nhỏ mà webview không báo), không đánh thức CPU mỗi giây.
                        if st.idle_ms.load(Ordering::Relaxed) == 0 && !is_active(&st) {
                            if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(Duration::from_secs(15)) {
                                std::thread::sleep(Duration::from_secs(1));
                            }
                            continue;
                        }
                        let period = if is_active(&st) { st.interval_ms.load(Ordering::Relaxed).max(250) } else { st.idle_ms.load(Ordering::Relaxed) };
                        let left = Duration::from_millis(period).saturating_sub(ls.elapsed());
                        if left.is_zero() {
                            break;
                        }
                        match rx.recv_timeout(left) {
                            Ok(()) if is_active(&st) => break,
                            Ok(()) => {}
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => std::thread::sleep(left),
                        }
                    }
                }
                let started = Instant::now();
                last_start = Some(started);

                let s = ps.sample();
                if !s.removed.is_empty() || !s.added.is_empty() {
                    let mut times = st.proc_creation_times.lock().unwrap_or_else(|e| e.into_inner());
                    // PID tái sử dụng xuất hiện ở cả hai danh sách: xóa bản cũ trước.
                    for pid in &s.removed {
                        times.remove(pid);
                    }
                    for p in &s.added {
                        if let Some(created) = ps.creation_time(p.pid) {
                            times.insert(p.pid, created);
                        }
                    }
                }
                let m = ms.sample();
                let (gpus, gpu_pid) = gs.sample();
                let total = m.mem.total;
                st.total_mem.store(total, Ordering::Relaxed);

                // --- RAM theo mục khởi động / dịch vụ ---------------------------------
                let mut by_pid: HashMap<u32, u64> = HashMap::with_capacity(s.rows.len());
                let mut by_path: HashMap<&str, u64> = HashMap::new();
                let mut groups: HashMap<String, (String, u64, u32, f32)> = HashMap::new();
                for r in &s.rows {
                    by_pid.insert(r.pid, r.mem);
                    if let Some(exe) = ps.exe_of(r.pid) {
                        if !exe.is_empty() {
                            *by_path.entry(exe).or_insert(0) += r.mem;
                        }
                    }
                    let name = ps.get_static(r.pid).map(|x| x.name.as_str()).unwrap_or("?");
                    let g = groups.entry(name.to_lowercase()).or_insert_with(|| (name.to_string(), 0, 0, 0.0));
                    g.1 += r.mem;
                    g.2 += 1;
                    g.3 += r.cpu;
                }
                // --- Trang Cổng: chỉ các PID đang giữ cổng, chỉ khi trang vừa hỏi -----
                let want: Option<Vec<u32>> = st
                    .port_pids
                    .lock()
                    .ok()
                    .filter(|g| !g.1.is_empty() && g.0.elapsed().as_millis() < PORT_WATCH_MS)
                    .map(|g| g.1.clone());
                if let Some(pids) = want {
                    let mut out: HashMap<u32, PortProc> = HashMap::with_capacity(pids.len());
                    for r in s.rows.iter().filter(|r| pids.binary_search(&r.pid).is_ok()) {
                        let (name, exe) = ps.get_static(r.pid).map(|x| (x.name.clone(), x.exe.clone())).unwrap_or_default();
                        out.insert(r.pid, PortProc { name, exe, cpu: round1(r.cpu), mem: r.mem });
                    }
                    if let Ok(mut pp) = st.port_procs.lock() {
                        *pp = out;
                    }
                }
                let mut top: Vec<(String, u64, u32, f32)> = groups.into_values().collect();
                top.sort_unstable_by_key(|g| std::cmp::Reverse(g.1));

                let reset = st.reset_ram.swap(false, Ordering::SeqCst);
                if reset {
                    last_ram.clear();
                }
                let mut items = Vec::new();
                if let Ok(tr) = st.tracked.lock() {
                    for x in tr.iter() {
                        let b = match &x.m {
                            Matcher::Pid(pid) => by_pid.get(pid).copied().unwrap_or(0),
                            Matcher::Path(p) => by_path.get(p.as_str()).copied().unwrap_or(0),
                            Matcher::Prefix(pre) => {
                                by_path.iter().filter(|(k, _)| k.contains(pre.as_str())).map(|(_, v)| *v).sum()
                            }
                        };
                        let mb = ((b as f64) / 1_048_576.0).round() as u32;
                        let prev = last_ram.insert(x.id.clone(), mb);
                        if (reset && mb > 0) || (!reset && prev != Some(mb)) {
                            items.push((x.id.clone(), mb));
                        }
                    }
                }

                // --- Lịch sử --------------------------------------------------------
                if let Ok(mut h) = st.history.lock() {
                    h.push_back(HistPoint {
                        t: m.t,
                        cpu: round1(m.cpu.total),
                        mem: m.mem.used,
                        disks: m.disks.iter().map(|d| (d.id.clone(), round1(d.active), d.read, d.write)).collect(),
                        nets: m.nets.iter().map(|n| (n.id.clone(), n.rx, n.tx)).collect(),
                        gpus: gpus.iter().map(|g| (g.id.clone(), round1(g.util))).collect(),
                    });
                    while h.len() > HISTORY_MAX || h.front().is_some_and(|p| m.t - p.t > HISTORY_MS) {
                        h.pop_front();
                    }
                }

                // --- Ngữ cảnh cho trợ lý AI (mỗi ~5 nhịp) ----------------------------
                if tick.is_multiple_of(5) {
                    let mut c = String::with_capacity(1500);
                    c.push_str(&format!(
                        "CPU: {:.0}% ({} luồng logic, {} MHz). RAM: {:.1}/{:.1} GB ({:.0}%), commit {:.1}/{:.1} GB.\n",
                        m.cpu.total,
                        m.cpu.cores.len(),
                        m.cpu.mhz,
                        gb(m.mem.used),
                        gb(total),
                        if total > 0 { m.mem.used as f64 / total as f64 * 100.0 } else { 0.0 },
                        gb(m.mem.committed),
                        gb(m.mem.commit_limit)
                    ));
                    for g in &gpus {
                        c.push_str(&format!("GPU {} ({}): {:.0}%.\n", g.id, g.name, g.util));
                    }
                    for d in &m.disks {
                        c.push_str(&format!(
                            "{}: bận {:.0}%, đọc {:.1} MB/s, ghi {:.1} MB/s.\n",
                            d.name,
                            d.active,
                            d.read / 1_048_576.0,
                            d.write / 1_048_576.0
                        ));
                    }
                    c.push_str(&format!(
                        "Tiến trình: {}, luồng: {}, handle: {}. Thời gian bật máy: {} phút.\n",
                        m.counts.processes,
                        m.counts.threads,
                        m.counts.handles,
                        m.uptime_s / 60
                    ));
                    c.push_str("Top tiến trình theo RAM (tên ×số tiến trình: RAM, CPU):\n");
                    for (name, b, n, cpu) in top.iter().take(10) {
                        c.push_str(&format!("- {name} ×{n}: {} MB, {:.1}% CPU\n", b / 1_048_576, cpu));
                    }
                    let mut by_cpu: Vec<&(String, u64, u32, f32)> = top.iter().filter(|g| g.3 >= 1.0).collect();
                    by_cpu.sort_by(|a, b| b.3.total_cmp(&a.3));
                    if !by_cpu.is_empty() {
                        c.push_str("Top tiến trình theo CPU:\n");
                        for (name, _, n, cpu) in by_cpu.into_iter().take(5) {
                            c.push_str(&format!("- {name} ×{n}: {:.1}% CPU\n", cpu));
                        }
                    }
                    if let Ok(sum) = st.scan_summary.lock() {
                        c.push_str(&sum);
                    }
                    if let Ok(mut ctx) = st.context.lock() {
                        *ctx = c;
                    }
                }

                // --- Phát sự kiện (kiểm tra lại trạng thái hiện tại sau khi đo xong) ---
                if is_active(&st) {
                    let top_view: Vec<TopProc> = top
                        .iter()
                        .take(10)
                        .map(|(name, b, n, _)| TopProc { name: name.clone(), mb: (b / 1_048_576) as u32, count: *n })
                        .collect();
                    let _ = app.emit("ram", RamTick { t: m.t, used: m.mem.used, total, items, top: top_view, reset });
                    let _ = app.emit("metrics", MetricsEvent { m: &m, gpus: &gpus });

                    if st.watch_procs.load(Ordering::Relaxed) {
                        let reset_p = st.reset_procs.swap(false, Ordering::SeqCst);
                        let added = if reset_p { ps.all_static() } else { s.added.clone() };
                        let rows = s
                            .rows
                            .iter()
                            .map(|r| {
                                let g = gpu_pid.get(&r.pid).copied().unwrap_or(0.0);
                                (r.pid, round1(r.cpu), r.mem, r.disk, round1(g), r.threads, r.handles)
                            })
                            .collect();
                        let win = if reset_p { Some(ps.window_titles()) } else { s.windows.as_ref() };
                        let removed: &[u32] = if reset_p { &[] } else { &s.removed };
                        let _ = app.emit("procs", ProcsEvent { t: m.t, reset: reset_p, added, removed, rows, windows: win });
                    }
                } else {
                    // Khi giao diện mở lại phải gửi lại toàn bộ (đã bỏ lỡ added/removed).
                    st.reset_procs.store(true, Ordering::SeqCst);
                    st.reset_ram.store(true, Ordering::SeqCst);
                }

                tick += 1;
            }
        })
        .expect("không tạo được luồng đo");
}
