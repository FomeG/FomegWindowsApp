// Trạng thái dùng chung giữa luồng đo (monitor) và các command.
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

use crate::core::{ai, Item};

pub const GENERIC_HOSTS: &[&str] = &[
    "cmd.exe", "powershell.exe", "pwsh.exe", "rundll32.exe", "wscript.exe", "cscript.exe", "svchost.exe",
    "conhost.exe", "explorer.exe", "msiexec.exe", "mshta.exe", "dllhost.exe", "taskhostw.exe", "sc.exe",
];
/// Lịch sử giữ 1 giờ (theo thời gian), tối đa 7201 điểm (nhịp nhanh nhất 500 ms).
pub const HISTORY_MAX: usize = 7201;
pub const HISTORY_MS: i64 = 3_600_000;

pub enum Matcher {
    Pid(u32),
    Path(String),
    Prefix(String),
}

pub struct Tracked {
    pub id: String,
    pub m: Matcher,
}

pub fn matcher(it: &Item) -> Option<Matcher> {
    match it.kind.as_str() {
        "Service" | "Svc" => (it.pid > 0).then_some(Matcher::Pid(it.pid)),
        "Packaged" => {
            let p = it.command.split('_').next().unwrap_or("").to_lowercase();
            (!p.is_empty()).then(|| Matcher::Prefix(format!("\\windowsapps\\{p}")))
        }
        _ => {
            if it.file_path.is_empty() {
                return None;
            }
            let fp = it.file_path.replace('/', "\\").to_lowercase();
            let leaf = fp.rsplit('\\').next().unwrap_or("");
            (!GENERIC_HOSTS.contains(&leaf)).then_some(Matcher::Path(fp))
        }
    }
}

/// Một điểm lịch sử hiệu năng.
pub struct HistPoint {
    pub t: i64,
    pub cpu: f32,
    pub mem: u64,
    pub disks: Vec<(String, f32, f64, f64)>,
    pub nets: Vec<(String, f64, f64)>,
    pub gpus: Vec<(String, f32)>,
}

#[derive(Serialize, Default)]
pub struct DiskSeries {
    pub active: Vec<f32>,
    pub read: Vec<f64>,
    pub write: Vec<f64>,
}

#[derive(Serialize, Default)]
pub struct NetSeries {
    pub rx: Vec<f64>,
    pub tx: Vec<f64>,
}

#[derive(Serialize, Default)]
pub struct MetricsHistory {
    pub t: Vec<i64>,
    pub cpu: Vec<f32>,
    pub mem: Vec<u64>,
    pub disks: HashMap<String, DiskSeries>,
    pub nets: HashMap<String, NetSeries>,
    pub gpus: HashMap<String, Vec<f32>>,
}

pub struct AppState {
    pub tracked: Mutex<Vec<Tracked>>,
    /// Gửi lại toàn bộ RAM theo mục ở lần đo tới.
    pub reset_ram: AtomicBool,
    /// Gửi lại toàn bộ danh sách tiến trình ở lần đo tới.
    pub reset_procs: AtomicBool,
    /// Thời điểm tạo (FILETIME) của từng PID trong mẫu gần nhất.
    pub proc_creation_times: Mutex<HashMap<u32, u64>>,
    pub watch_procs: AtomicBool,
    pub visible: AtomicBool,
    /// 0 = tạm dừng cập nhật giao diện.
    pub interval_ms: AtomicU64,
    /// Chu kỳ lấy mẫu khi cửa sổ ẩn / thu nhỏ / tạm dừng (giữ lịch sử 1 giờ); 0 = không lấy mẫu.
    pub idle_ms: AtomicU64,
    pub history: Mutex<VecDeque<HistPoint>>,
    pub total_mem: AtomicU64,
    /// Ảnh chụp ngắn tình trạng máy cho trợ lý AI.
    pub context: Mutex<String>,
    pub scan_summary: Mutex<String>,
    pub wake: Mutex<Option<SyncSender<()>>>,
    pub chats: ai::Chats,
    pub ai_cancel: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub cleanup: crate::core::cleanup::Cleaner,
    /// lần quét dung lượng ổ đĩa hiện tại (trang Ổ cứng); None = đã đóng / chưa quét
    pub du: Mutex<Option<Arc<crate::core::du::Scan>>>,
    pub du_seq: AtomicU64,
    /// Trang Cổng: PID đang giữ cổng (và lúc hỏi gần nhất). Monitor chỉ chép thông tin của các PID này,
    /// và chỉ khi trang vừa hỏi trong PORT_WATCH_MS – đóng trang là hết tốn.
    pub port_pids: Mutex<(Instant, Vec<u32>)>,
    pub port_procs: Mutex<HashMap<u32, PortProc>>,
}

pub const PORT_WATCH_MS: u128 = 6000;

/// Tên + số liệu sống của một tiến trình giữ cổng (lấy từ lần đo của monitor).
#[derive(Clone, Default)]
pub struct PortProc {
    pub name: String,
    pub exe: String,
    pub cpu: f32,
    pub mem: u64,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            tracked: Mutex::new(Vec::new()),
            reset_ram: AtomicBool::new(true),
            reset_procs: AtomicBool::new(true),
            proc_creation_times: Mutex::new(HashMap::new()),
            watch_procs: AtomicBool::new(false),
            visible: AtomicBool::new(true),
            interval_ms: AtomicU64::new(1000),
            idle_ms: AtomicU64::new(5000),
            history: Mutex::new(VecDeque::with_capacity(HISTORY_MAX)),
            total_mem: AtomicU64::new(0),
            context: Mutex::new(String::new()),
            scan_summary: Mutex::new(String::new()),
            wake: Mutex::new(None),
            chats: ai::Chats::default(),
            ai_cancel: Mutex::new(HashMap::new()),
            cleanup: Default::default(),
            du: Mutex::new(None),
            du_seq: AtomicU64::new(0),
            port_pids: Mutex::new((Instant::now(), Vec::new())),
            port_procs: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_tracked(&self, a: &[Item], b: &[Item]) {
        let v: Vec<Tracked> = a
            .iter()
            .chain(b.iter())
            .filter_map(|it| matcher(it).map(|m| Tracked { id: it.id.clone(), m }))
            .collect();
        if let Ok(mut t) = self.tracked.lock() {
            *t = v;
        }
        self.reset_ram.store(true, Ordering::SeqCst);
        self.wake();
    }

    /// Dịch vụ đổi PID sau khi chạy/dừng: cập nhật cả bản tab Khởi động lẫn tab Dịch vụ.
    pub fn update_service(&self, name: &str, pid: u32) {
        let n = name.to_lowercase();
        let ids = [format!("Service:{n}|"), format!("Svc:{n}|")];
        if let Ok(mut t) = self.tracked.lock() {
            t.retain(|x| !ids.contains(&x.id));
            if pid > 0 {
                for id in ids {
                    t.push(Tracked { id, m: Matcher::Pid(pid) });
                }
            }
        }
        if pid == 0 {
            // Dịch vụ đã dừng: gửi lại toàn bộ để giao diện xoá số RAM cũ.
            self.reset_ram.store(true, Ordering::SeqCst);
        }
        self.wake();
    }

    pub fn wake(&self) {
        if let Ok(w) = self.wake.lock() {
            if let Some(tx) = w.as_ref() {
                let _ = tx.try_send(());
            }
        }
    }

    pub fn history_columns(&self) -> MetricsHistory {
        let mut h = MetricsHistory::default();
        let Ok(pts) = self.history.lock() else { return h };
        let n = pts.len();
        for (i, p) in pts.iter().enumerate() {
            h.t.push(p.t);
            h.cpu.push(p.cpu);
            h.mem.push(p.mem);
            for (id, a, r, w) in &p.disks {
                let s = h.disks.entry(id.clone()).or_insert_with(|| DiskSeries {
                    active: vec![0.0; i],
                    read: vec![0.0; i],
                    write: vec![0.0; i],
                });
                pad(&mut s.active, i, 0.0);
                pad(&mut s.read, i, 0.0);
                pad(&mut s.write, i, 0.0);
                s.active.push(*a);
                s.read.push(*r);
                s.write.push(*w);
            }
            for (id, rx, tx) in &p.nets {
                let s = h.nets.entry(id.clone()).or_insert_with(|| NetSeries { rx: vec![0.0; i], tx: vec![0.0; i] });
                pad(&mut s.rx, i, 0.0);
                pad(&mut s.tx, i, 0.0);
                s.rx.push(*rx);
                s.tx.push(*tx);
            }
            for (id, u) in &p.gpus {
                let s = h.gpus.entry(id.clone()).or_insert_with(|| vec![0.0; i]);
                pad(s, i, 0.0);
                s.push(*u);
            }
        }
        // Thiết bị biến mất giữa chừng: đệm cho đủ độ dài.
        for s in h.disks.values_mut() {
            pad(&mut s.active, n, 0.0);
            pad(&mut s.read, n, 0.0);
            pad(&mut s.write, n, 0.0);
        }
        for s in h.nets.values_mut() {
            pad(&mut s.rx, n, 0.0);
            pad(&mut s.tx, n, 0.0);
        }
        for s in h.gpus.values_mut() {
            pad(s, n, 0.0);
        }
        h
    }
}

fn pad<T: Copy>(v: &mut Vec<T>, len: usize, val: T) {
    while v.len() < len {
        v.push(val);
    }
}
