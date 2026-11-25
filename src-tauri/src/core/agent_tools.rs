// Bộ công cụ CHỈ ĐỌC cho trợ lý AI (vòng lặp tool-use trong ai.rs) và cho MCP server (core/mcp.rs, binary fomeg-windows-app-mcp).
// Theo RustWTM (crates/wtm-ai/src/tools.rs): mỗi tool có tên, mô tả, JSON schema; kết quả là văn bản gọn.
//
// Giới hạn cứng: không tool nào ghi / xoá / tắt / dừng / kết thúc / khởi chạy gì. Muốn thao tác thì model viết khối đề
// xuất (`xoa`, `dong_cong`, `don_dep`, `tat_khoi_dong`, `dich_vu`, `ket_thuc`) và người dùng tự xác nhận trong app.
//
// Hiệu năng: không có vòng lấy mẫu riêng. Tiến trình / CPU / GPU được lấy mẫu khi một tool cần (hai mẫu cách 0,5 s để có
// tốc độ, dùng lại trong 2 s); danh sách khởi động / dịch vụ quét một lần và dùng lại 60 s. Dữ liệu chỉ app có (lịch sử
// 1 giờ, kết quả quét ổ đĩa, kết quả dọn dẹp) đến qua `AppData` khi chạy trong app.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::cleanup::Cleaner;
use super::du::{self, sz};
use super::gpu::{GpuMetrics, GpuSampler};
use super::metrics::{Metrics, MetricsSampler};
use super::procs::{ProcRow, ProcSampler, ProcStatic};
use super::{ports, scan, Item, ScanResult};

/// Trần độ dài một kết quả (ký tự).
pub const MAX_OUT: usize = 6000;
const FRESH: Duration = Duration::from_secs(2);
const WINDOW: Duration = Duration::from_millis(500);
const SCAN_FRESH: Duration = Duration::from_secs(60);

/// Dữ liệu chỉ có khi chạy trong app (MCP không có).
pub trait AppData {
    /// Tóm tắt lịch sử CPU / RAM / đĩa / mạng `minutes` phút gần nhất.
    fn history(&self, minutes: u32) -> Option<String>;
    /// Tóm tắt lần quét ổ đĩa đang có (trang Ổ cứng), `focus` = thư mục cần chi tiết.
    fn disk_summary(&self, focus: Option<&str>) -> Option<String>;
    /// Kết quả quét dọn dẹp gần nhất (trang Dọn dẹp).
    fn cleanup_summary(&self) -> Option<String>;
    /// Thông tin app: phiên bản, quyền Admin, chu kỳ cập nhật…
    fn app_info(&self) -> String;
}

/// Tên tool -> nhãn tiếng Việt cho giao diện ("Đã xem: …").
pub fn label(name: &str) -> &'static str {
    match name {
        "system_overview" => "tổng quan máy",
        "hardware" => "cấu hình phần cứng",
        "metrics_history" => "lịch sử hiệu năng",
        "list_processes" => "danh sách tiến trình",
        "process_report" => "chi tiết tiến trình",
        "process_tree" => "cây tiến trình",
        "startup_items" => "mục khởi động",
        "services" => "dịch vụ",
        "service_details" => "chi tiết dịch vụ",
        "ports" => "cổng mạng",
        "drives" => "ổ đĩa",
        "disk_usage" => "dung lượng thư mục",
        "folder_files" => "tệp trong thư mục",
        "scan_folder" => "quét thư mục",
        "cleanup_report" => "kết quả dọn dẹp",
        "app_info" => "thông tin ứng dụng",
        "installed_apps" => "ứng dụng đã cài",
        "app_folders" => "thư mục của ứng dụng",
        "leftovers" => "phần còn sót sau khi gỡ",
        _ => "dữ liệu máy",
    }
}

pub struct Toolbox<'a> {
    app: Option<&'a dyn AppData>,
    procs: Option<ProcSampler>,
    metrics: Option<MetricsSampler>,
    gpu: Option<GpuSampler>,
    rows: HashMap<u32, ProcRow>,
    gpu_pids: HashMap<u32, f32>,
    last: Option<Metrics>,
    gpus: Vec<GpuMetrics>,
    sampled: Option<Instant>,
    scan: Option<(Instant, ScanResult)>,
    cleaner: Option<Cleaner>,
    /// Gửi dòng lệnh tiến trình (đã lọc bí mật). Tắt = không gửi.
    pub command_lines: bool,
}

impl<'a> Toolbox<'a> {
    pub fn new(app: Option<&'a dyn AppData>) -> Self {
        Self {
            app,
            procs: None,
            metrics: None,
            gpu: None,
            rows: HashMap::new(),
            gpu_pids: HashMap::new(),
            last: None,
            gpus: Vec::new(),
            sampled: None,
            scan: None,
            cleaner: None,
            command_lines: true,
        }
    }

    /// Mô tả tool (name, description, inputSchema) – dạng MCP; ai.rs đổi sang dạng của từng nhà cung cấp.
    pub fn tools(&self) -> Vec<Value> {
        let none = || json!({ "type": "object", "properties": {} });
        let pid = || json!({ "type": "object", "properties": { "pid": { "type": "integer" } }, "required": ["pid"] });
        vec![
            tool("system_overview", "Current CPU, memory, GPU, disk and network load, uptime, battery, and the busiest processes (by CPU and by memory, grouped by app). Start here for 'why is my PC slow'.", none()),
            tool("hardware", "Hardware and OS: CPU model and cores, RAM size and modules, GPUs, physical disks (SSD/HDD), motherboard, Windows version, boot time.", none()),
            tool("metrics_history", "CPU / memory / disk / network over the last minutes (min, average, max, peaks). Only while the app is open.", json!({ "type": "object", "properties": { "minutes": { "type": "integer", "minimum": 1, "maximum": 60 } } })),
            tool("list_processes", "Processes sorted by cpu, memory, disk or gpu, optionally filtered by part of the name. group=true sums processes of the same app (e.g. all chrome.exe). Each line has the PID and parent PID (PIDs or image names go in a `ket_thuc` proposal).", json!({
                "type": "object",
                "properties": {
                    "sort": { "type": "string", "enum": ["cpu", "memory", "disk", "gpu"] },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "name": { "type": "string", "description": "Part of the image name, e.g. chrome" },
                    "group": { "type": "boolean" }
                }
            })),
            tool("process_report", "Everything known about one process: path, publisher, description, command line, user, start time, usage, parent chain, children, Windows services it hosts, ports it listens on, whether it starts with Windows. Use it to explain what a process is and whether it is safe.", pid()),
            tool("process_tree", "Ancestors and ALL descendants of one process with CPU/memory of each and the TOTAL of the whole branch (e.g. an app plus its WebView2 / browser helper processes).", pid()),
            tool("startup_items", "Things that start with Windows (Registry Run, Startup folder, Scheduled Tasks, services, Store apps): enabled or not, publisher, command, and when Windows last saw the program run (BAM / Prefetch / UserAssist) – a startup item whose program has not run for months is a good candidate to disable. Filter by source, state or name. Names are what a `tat_khoi_dong` proposal uses.", json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "enum": ["all", "registry", "folder", "task", "service", "store"] },
                    "enabled_only": { "type": "boolean" },
                    "name": { "type": "string" },
                    "include_microsoft": { "type": "boolean" }
                }
            })),
            tool("services", "Windows services with state, start type, account and PID, optionally filtered by part of the name, by state, or third-party only. The service name in parentheses is what a `dich_vu` proposal uses.", json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "running_only": { "type": "boolean" },
                    "third_party_only": { "type": "boolean" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 300 }
                }
            })),
            tool("service_details", "One service by its service name or display name: description, path, account, start type, state, PID, memory, whether the app treats it as critical.", json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] })),
            tool("ports", "Listening TCP ports (and optionally UDP) with the process holding each and who can reach it (this PC only / one network / every network). Optionally one port.", json!({
                "type": "object",
                "properties": { "port": { "type": "integer", "minimum": 1, "maximum": 65535 }, "udp": { "type": "boolean" } }
            })),
            tool("drives", "Every drive with type, file system, size and free space.", none()),
            tool("disk_usage", "Largest folders and likely junk/cache folders from the Disk page's EXISTING scan (whatever drive the user scanned), optionally details of one folder. If the folder is not in that scan, call scan_folder instead of asking the user to scan.", json!({ "type": "object", "properties": { "path": { "type": "string" } } })),
            tool("scan_folder", "Measure ONE folder right now (any path, e.g. C:\\Users\\A\\.cache or a cache from cleanup_report): total size, file count, and the biggest subfolders as a tree `depth` levels deep with sizes, file counts and last-modified month. Read-only (names, sizes, dates; never file contents), does not follow junctions, stops after ~12 s and then reports minimum sizes. Use it to see what is inside a folder before suggesting to clean or delete it; never ask the user to scan first. Then folder_files for the files of one subfolder.", json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Full path of the folder" },
                    "depth": { "type": "integer", "minimum": 1, "maximum": 3, "description": "Subfolder levels to show (default 2)" },
                    "limit": { "type": "integer", "minimum": 3, "maximum": 30, "description": "Subfolders per level (default 12)" }
                },
                "required": ["path"]
            })),
            tool("folder_files", "The largest files directly inside one folder (name, size, modified date). Metadata only, never file contents.", json!({ "type": "object", "properties": { "path": { "type": "string" }, "limit": { "type": "integer", "minimum": 1, "maximum": 50 } }, "required": ["path"] })),
            tool("cleanup_report", "What the Cleanup page can clean: temp files, error reports, dumps, update caches, shader caches, Recycle Bin and app caches, with sizes, safety and ids usable in a `don_dep` proposal. Scans if needed (a few seconds).", none()),
            tool("installed_apps", "Installed programs like Windows Settings > Apps > Installed apps (desktop apps from the Uninstall registry + Store/MSIX apps): name, publisher, version, install date, LAST USED (when Windows last saw any of its programs run, plus how many times the user opened it from Start/Explorer), size, scope, install folder. sort=last_used lists the least recently used first – good for 'what can I uninstall'. Sizes marked \"đo\" are measured from the install folder, \"≈\" is what the installer declared (often missing data in AppData). measure=true measures install folders now (up to ~10 s). Uninstalling is done by the user on the Apps page, never by you.", json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Part of the app or publisher name" },
                    "sort": { "type": "string", "enum": ["size", "name", "date", "last_used"] },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200 },
                    "include_system": { "type": "boolean" },
                    "measure": { "type": "boolean" }
                }
            })),
            tool("app_folders", "Folders that belong to ONE installed app (install folder + data folders in AppData, ProgramData, Program Files, profile dot-folders found by the app's name / exe / publisher), measured, with folders also used by another installed app marked shared. Use it before advising to uninstall, or to say what stays after uninstalling.", json!({ "type": "object", "properties": { "name": { "type": "string", "description": "Part of the app name as listed by installed_apps" } }, "required": ["name"] })),
            tool("leftovers", "What uninstalled apps left behind: folders of apps that were removed (the app remembers apps it saw before), data of Store apps that are no longer installed, and WindowsApps folders no longer used by this account (packages of other accounts, old versions, orphans – those are only reported). Measured sizes. Folders can be proposed for deletion in an `xoa` block. Takes up to ~15 s.", none()),
            tool("app_info", "This app: version, running as administrator or not, update interval.", none()),
        ]
    }

    /// Chạy một tool. Lỗi = thông báo cho model (tham số sai, không tìm thấy…).
    pub fn run(&mut self, name: &str, args: &Value) -> Result<String, String> {
        let out = match name {
            "system_overview" => self.overview(),
            "hardware" => Ok(hardware()),
            "metrics_history" => {
                let m = arg_u64(args, "minutes").unwrap_or(10).clamp(1, 60) as u32;
                self.app.and_then(|a| a.history(m)).ok_or_else(|| "Lịch sử hiệu năng chỉ có khi ứng dụng đang mở.".to_owned())
            }
            "list_processes" => self.list_processes(args),
            "process_report" => self.process_report(arg_pid(args)?),
            "process_tree" => self.process_tree(arg_pid(args)?),
            "startup_items" => self.startup_items(args),
            "services" => self.services(args),
            "service_details" => self.service_details(arg_str(args, "name").ok_or("Thiếu `name`.")?),
            "ports" => self.ports(args),
            "drives" => Ok(drives()),
            "disk_usage" => self.disk_usage(arg_str(args, "path")),
            "scan_folder" => scan_folder(
                arg_str(args, "path").ok_or("Thiếu `path`.")?,
                arg_u64(args, "depth").unwrap_or(2).clamp(1, 3) as usize,
                arg_u64(args, "limit").unwrap_or(12).clamp(3, 30) as usize,
            ),
            "folder_files" => folder_files(arg_str(args, "path").ok_or("Thiếu `path`.")?, arg_u64(args, "limit").unwrap_or(20).clamp(1, 50) as usize),
            "cleanup_report" => self.cleanup_report(),
            "installed_apps" => Ok(crate::core::apps::ai_report(
                arg_str(args, "name"),
                arg_str(args, "sort").unwrap_or("size"),
                arg_u64(args, "limit").unwrap_or(40).clamp(1, 200) as usize,
                args.get("include_system").and_then(Value::as_bool).unwrap_or(false),
                args.get("measure").and_then(Value::as_bool).unwrap_or(false),
            )),
            "app_folders" => {
                let n = arg_str(args, "name").ok_or("Thiếu `name`.")?.to_lowercase();
                let list = crate::core::apps::list().apps;
                let a = list.iter().find(|a| a.name.to_lowercase() == n).or_else(|| list.iter().find(|a| a.name.to_lowercase().contains(&n))).ok_or_else(|| format!("Không có ứng dụng nào tên chứa \"{n}\"."))?;
                let r = crate::core::leftovers::related(&a.id)?;
                let mut s = format!("{} ({}) – {} thư mục:\n", a.name, a.publisher, r.len());
                for f in r {
                    let edited = if f.newest > 0 { format!(" · tệp mới nhất sửa {}", crate::core::usage::describe(f.newest, 0)) } else { String::new() };
                    let _ = writeln!(s, "- {} · {}{} · {} · {}{}{edited}", f.path, if f.partial { "≥" } else { "" }, sz(f.bytes), f.area, f.reason, f.shared.map(|o| format!(" · DÙNG CHUNG với {o}")).unwrap_or_default());
                }
                Ok(s)
            }
            "leftovers" => Ok(crate::core::leftovers::ai_report(&crate::core::leftovers::scan())),
            "app_info" => Ok(self.app.map(|a| a.app_info()).unwrap_or_else(|| format!("Vietnamese's Ultimate W App {} (MCP, app chưa mở) · Admin: {}", env!("CARGO_PKG_VERSION"), yes(crate::is_elevated())))),
            _ => Err(format!("Không có tool `{name}`.")),
        }?;
        Ok(cap(out))
    }

    // ---------------- lấy mẫu ----------------

    /// Mẫu tiến trình + máy còn mới (≤ 2 s); không thì lấy hai mẫu cách 0,5 s để có CPU %.
    fn fresh(&mut self) {
        if self.sampled.is_some_and(|t| t.elapsed() <= FRESH) {
            return;
        }
        let first = self.procs.is_none();
        let procs = self.procs.get_or_insert_with(ProcSampler::new);
        let metrics = self.metrics.get_or_insert_with(MetricsSampler::new);
        let gpu = self.gpu.get_or_insert_with(GpuSampler::new);
        if first || self.sampled.is_none_or(|t| t.elapsed() > FRESH * 5) {
            procs.sample();
            metrics.sample();
            gpu.sample();
            std::thread::sleep(WINDOW);
        }
        let s = procs.sample();
        self.rows = s.rows.into_iter().map(|r| (r.pid, r)).collect();
        self.last = Some(metrics.sample());
        let (g, per_pid) = gpu.sample();
        self.gpus = g;
        self.gpu_pids = per_pid;
        self.sampled = Some(Instant::now());
    }

    fn statics(&self) -> Vec<ProcStatic> {
        self.procs.as_ref().map(ProcSampler::all_static).unwrap_or_default()
    }

    fn info(&self, pid: u32) -> Option<&ProcStatic> {
        self.procs.as_ref()?.get_static(pid)
    }

    fn scan(&mut self) -> &ScanResult {
        if self.scan.as_ref().is_none_or(|(t, _)| t.elapsed() > SCAN_FRESH) {
            self.scan = Some((Instant::now(), scan::run()));
        }
        &self.scan.as_ref().expect("vừa quét").1
    }

    // ---------------- tools ----------------

    fn overview(&mut self) -> Result<String, String> {
        self.fresh();
        let m = self.last.as_ref().ok_or("Không đo được.")?;
        let mut s = String::new();
        let _ = writeln!(s, "CPU {:.0}% ({} luồng logic, {} MHz) · RAM {} / {} ({:.0}%), commit {} / {} · {} tiến trình, {} luồng, {} handle · bật máy {}",
            m.cpu.total, m.cpu.cores.len(), m.cpu.mhz, sz(m.mem.used), sz(m.mem.total), pct(m.mem.used, m.mem.total), sz(m.mem.committed), sz(m.mem.commit_limit),
            m.counts.processes, m.counts.threads, m.counts.handles, dur(m.uptime_s));
        for g in &self.gpus {
            let _ = writeln!(s, "GPU {}: {:.0}% · VRAM {} / {}", g.name, g.util, sz(g.ded_used), sz(g.ded_total));
        }
        for d in &m.disks {
            let _ = writeln!(s, "Đĩa {} {}: bận {:.0}% · đọc {}/s · ghi {}/s", d.id, d.name, d.active, sz(d.read as u64), sz(d.write as u64));
        }
        for n in m.nets.iter().filter(|n| n.rx + n.tx > 0.0 || n.link_bps > 0) {
            let _ = writeln!(s, "Mạng {} ({}): nhận {}/s · gửi {}/s", n.name, n.kind, sz(n.rx as u64), sz(n.tx as u64));
        }
        if let Some(b) = &m.battery {
            let _ = writeln!(s, "Pin {}%{}", b.percent, if b.charging { " · đang sạc" } else if b.plugged { " · cắm sạc" } else { "" });
        }
        let groups = self.groups();
        let mut by_cpu: Vec<&Group> = groups.values().collect();
        by_cpu.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        s.push_str("Tốn CPU nhất (theo ứng dụng):\n");
        for g in by_cpu.iter().take(8).filter(|g| g.cpu >= 0.1) {
            let _ = writeln!(s, "  {}", g.line());
        }
        let mut by_mem = by_cpu;
        by_mem.sort_by_key(|g| std::cmp::Reverse(g.mem));
        s.push_str("Tốn RAM nhất (theo ứng dụng):\n");
        for g in by_mem.iter().take(8) {
            let _ = writeln!(s, "  {}", g.line());
        }
        Ok(s)
    }

    fn groups(&self) -> HashMap<String, Group> {
        let mut out: HashMap<String, Group> = HashMap::new();
        for p in self.statics() {
            let Some(r) = self.rows.get(&p.pid) else { continue };
            let key = p.name.to_lowercase();
            let g = out.entry(key).or_insert_with(|| Group { name: p.name.clone(), desc: p.desc.clone(), ..Default::default() });
            g.count += 1;
            g.cpu += r.cpu;
            g.mem += r.mem;
            g.disk += r.disk;
            g.gpu += self.gpu_pids.get(&p.pid).copied().unwrap_or(0.0);
            g.pids.push(p.pid);
        }
        out
    }

    fn list_processes(&mut self, args: &Value) -> Result<String, String> {
        self.fresh();
        let sort = arg_str(args, "sort").unwrap_or("cpu");
        let limit = arg_u64(args, "limit").unwrap_or(25).clamp(1, 100) as usize;
        let needle = arg_str(args, "name").map(str::to_lowercase);
        let mut s = String::new();
        if args.get("group").and_then(Value::as_bool).unwrap_or(false) {
            let mut gs: Vec<Group> = self.groups().into_values().filter(|g| needle.as_ref().is_none_or(|n| g.name.to_lowercase().contains(n))).collect();
            match sort {
                "memory" => gs.sort_by_key(|g| std::cmp::Reverse(g.mem)),
                "disk" => gs.sort_by_key(|g| std::cmp::Reverse(g.disk)),
                "gpu" => gs.sort_by(|a, b| b.gpu.total_cmp(&a.gpu)),
                _ => gs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu)),
            }
            let _ = writeln!(s, "{} ứng dụng (gộp theo tên tệp), sắp theo {sort}:", gs.len());
            for g in gs.iter().take(limit) {
                let _ = writeln!(s, "{}", g.line());
            }
            return Ok(s);
        }
        let mut ps: Vec<(ProcStatic, ProcRow)> = self
            .statics()
            .into_iter()
            .filter(|p| needle.as_ref().is_none_or(|n| p.name.to_lowercase().contains(n)))
            .filter_map(|p| self.rows.get(&p.pid).map(|r| (p, *r)))
            .collect();
        let gpu = &self.gpu_pids;
        match sort {
            "memory" => ps.sort_by_key(|(_, r)| std::cmp::Reverse(r.mem)),
            "disk" => ps.sort_by_key(|(_, r)| std::cmp::Reverse(r.disk)),
            "gpu" => ps.sort_by(|a, b| gpu.get(&b.0.pid).unwrap_or(&0.0).total_cmp(gpu.get(&a.0.pid).unwrap_or(&0.0))),
            _ => ps.sort_by(|a, b| b.1.cpu.total_cmp(&a.1.cpu)),
        }
        let _ = writeln!(s, "{} tiến trình{}, sắp theo {sort}. Cột: tên PID (cha) · CPU · RAM · đĩa/s · GPU · loại", ps.len(), if needle.is_some() { " khớp" } else { "" });
        for (p, r) in ps.iter().take(limit) {
            let _ = writeln!(s, "{} {} (cha {}) · {:.1}% · {} · {}/s · {:.0}% · {}{}", p.name, p.pid, p.ppid, r.cpu, sz(r.mem), sz(r.disk), gpu.get(&p.pid).unwrap_or(&0.0), kind_vi(&p.kind), if p.elevated { " · Admin" } else { "" });
        }
        if ps.len() > limit {
            let _ = writeln!(s, "… và {} tiến trình nữa", ps.len() - limit);
        }
        Ok(s)
    }

    fn process_report(&mut self, pid: u32) -> Result<String, String> {
        self.fresh();
        let p = self.info(pid).cloned().ok_or_else(|| format!("Không có tiến trình PID {pid}."))?;
        let r = self.rows.get(&pid).copied();
        let mut s = String::new();
        let _ = writeln!(s, "{} (PID {pid})", p.name);
        if !p.desc.is_empty() {
            let _ = writeln!(s, "Mô tả: {}", p.desc);
        }
        let _ = writeln!(s, "Đường dẫn: {}", or_dash(&p.exe));
        if let Some((_, n)) = crate::core::usage::get().exe(&p.exe).filter(|x| x.1 > 0) {
            let _ = writeln!(s, "Người dùng đã mở chương trình này {n} lần (Start / Explorer)");
        }
        let _ = writeln!(s, "Nhà phát hành: {}", or_dash(&p.publisher));
        let _ = writeln!(s, "Loại: {} · người dùng {} · phiên {}{}", kind_vi(&p.kind), or_dash(&p.user), p.session, if p.elevated { " · chạy quyền Admin" } else { "" });
        if p.start_ms > 0 {
            let _ = writeln!(s, "Bắt đầu: {}", time_text(p.start_ms));
        }
        if let Some(r) = r {
            let _ = writeln!(s, "Đang dùng: CPU {:.1}% · RAM {} · đĩa {}/s · GPU {:.0}% · {} luồng · {} handle", r.cpu, sz(r.mem), sz(r.disk), self.gpu_pids.get(&pid).unwrap_or(&0.0), r.threads, r.handles);
        }
        if let Some(t) = self.procs.as_ref().and_then(|ps| ps.window_titles().get(&pid)) {
            let _ = writeln!(s, "Cửa sổ: {t}");
        }
        if self.command_lines && !p.cmd.is_empty() {
            let _ = writeln!(s, "Dòng lệnh: {}", redact(&p.cmd));
        }
        // cha
        let mut chain = Vec::new();
        let mut cur = p.ppid;
        let mut seen = HashSet::from([pid]);
        while cur != 0 && seen.insert(cur) && chain.len() < 8 {
            match self.info(cur) {
                Some(x) => {
                    chain.push(format!("{} {}", x.name, x.pid));
                    cur = x.ppid;
                }
                None => {
                    chain.push(format!("PID {cur} (đã thoát)"));
                    break;
                }
            }
        }
        if !chain.is_empty() {
            let _ = writeln!(s, "Được khởi chạy bởi: {}", chain.join(" ← "));
        }
        let kids = self.procs.as_ref().map(|ps| ps.children_of(pid)).unwrap_or_default();
        if !kids.is_empty() {
            let names: Vec<String> = kids.iter().take(10).filter_map(|k| self.info(*k).map(|x| format!("{} {}", x.name, x.pid))).collect();
            let _ = writeln!(s, "Tiến trình con ({}): {}{}", kids.len(), names.join(", "), if kids.len() > 10 { ", …" } else { "" });
        }
        let exe = p.exe.to_lowercase();
        let scan = self.scan();
        let svcs: Vec<String> = scan.services.iter().filter(|x| x.pid == pid).map(|x| format!("{} ({})", x.name, x.key1)).collect();
        if !svcs.is_empty() {
            let _ = writeln!(s, "Chạy dịch vụ: {}", svcs.join(", "));
        }
        let starts: Vec<String> = scan.startup.iter().filter(|x| !exe.is_empty() && x.file_path.to_lowercase() == exe).map(|x| format!("{} [{}{}]", x.name, x.source, if x.on { "" } else { ", đang tắt" })).collect();
        if !starts.is_empty() {
            let _ = writeln!(s, "Khởi động cùng Windows qua: {}", starts.join("; "));
        }
        if let Ok(uses) = ports::list(true) {
            let mine: Vec<String> = uses.iter().filter(|u| u.pid == pid).map(|u| format!("{} {} ({})", u.proto, u.port, reach_vi(u.reach))).collect();
            if !mine.is_empty() {
                let _ = writeln!(s, "Nghe cổng: {}", mine.join(", "));
            }
        }
        Ok(s)
    }

    fn process_tree(&mut self, pid: u32) -> Result<String, String> {
        self.fresh();
        let p = self.info(pid).cloned().ok_or_else(|| format!("Không có tiến trình PID {pid}."))?;
        let mut s = String::new();
        let mut chain = Vec::new();
        let mut cur = p.ppid;
        let mut seen = HashSet::from([pid]);
        while cur != 0 && seen.insert(cur) && chain.len() < 8 {
            let Some(x) = self.info(cur) else { break };
            chain.push(format!("{} {}", x.name, x.pid));
            cur = x.ppid;
        }
        if !chain.is_empty() {
            chain.reverse();
            let _ = writeln!(s, "Tổ tiên: {}", chain.join(" → "));
        }
        let (mut cpu, mut mem, mut n) = (0f32, 0u64, 0usize);
        let mut lines = Vec::new();
        let mut stack = vec![(pid, 0usize)];
        let mut seen = HashSet::new();
        while let Some((id, depth)) = stack.pop() {
            if !seen.insert(id) || depth > 12 {
                continue;
            }
            let Some(x) = self.info(id) else { continue };
            let r = self.rows.get(&id).copied();
            let (c, m) = r.map(|r| (r.cpu, r.mem)).unwrap_or_default();
            cpu += c;
            mem += m;
            n += 1;
            if lines.len() < 80 {
                lines.push(format!("{}{} {} · {:.1}% · {}", "  ".repeat(depth), x.name, id, c, sz(m)));
            }
            let mut kids = self.procs.as_ref().map(|ps| ps.children_of(id)).unwrap_or_default();
            kids.sort_unstable_by(|a, b| b.cmp(a));
            stack.extend(kids.into_iter().map(|k| (k, depth + 1)));
        }
        let _ = writeln!(s, "Nhánh {} {}: {n} tiến trình · TỔNG CPU {:.1}% · TỔNG RAM {}", p.name, pid, cpu, sz(mem));
        for l in &lines {
            let _ = writeln!(s, "{l}");
        }
        if n > lines.len() {
            let _ = writeln!(s, "… và {} tiến trình nữa", n - lines.len());
        }
        Ok(s)
    }

    fn startup_items(&mut self, args: &Value) -> Result<String, String> {
        let source = arg_str(args, "source").unwrap_or("all").to_owned();
        let enabled_only = args.get("enabled_only").and_then(Value::as_bool).unwrap_or(false);
        let ms = args.get("include_microsoft").and_then(Value::as_bool).unwrap_or(false);
        let needle = arg_str(args, "name").map(str::to_lowercase);
        let cmd = self.command_lines;
        let scan = self.scan();
        let list: Vec<&Item> = scan
            .startup
            .iter()
            .filter(|x| source_matches(&source, &x.kind))
            .filter(|x| !enabled_only || x.on)
            .filter(|x| ms || !x.is_ms)
            .filter(|x| needle.as_ref().is_none_or(|n| x.name.to_lowercase().contains(n) || x.command.to_lowercase().contains(n)))
            .collect();
        let on = list.iter().filter(|x| x.on).count();
        let u = crate::core::usage::get();
        let mut s = format!("{} mục khởi động ({on} đang bật){}. Cột: tên · bật/tắt · nguồn · nhà phát hành · lệnh · chạy lần cuối (Windows ghi nhận)\n", list.len(), if ms { "" } else { ", không tính mục của Microsoft" });
        for x in list.iter().take(120) {
            let _ = writeln!(s, "{} · {} · {} · {} · {} · {}", x.name, if x.on { "bật" } else { "tắt" }, x.source, or_dash(&x.publisher), if cmd { redact(&x.command) } else { x.file_path.clone() }, last_run(&u, &x.file_path));
        }
        Ok(s)
    }

    fn services(&mut self, args: &Value) -> Result<String, String> {
        let needle = arg_str(args, "name").map(str::to_lowercase);
        let running = args.get("running_only").and_then(Value::as_bool).unwrap_or(false);
        let third = args.get("third_party_only").and_then(Value::as_bool).unwrap_or(false);
        let limit = arg_u64(args, "limit").unwrap_or(80).clamp(1, 300) as usize;
        let scan = self.scan();
        let list: Vec<&Item> = scan
            .services
            .iter()
            .filter(|x| !running || x.state == "running")
            .filter(|x| !third || !x.is_ms)
            .filter(|x| needle.as_ref().is_none_or(|n| x.name.to_lowercase().contains(n) || x.key1.to_lowercase().contains(n)))
            .collect();
        let u = crate::core::usage::get();
        let mut s = format!("{} dịch vụ. Cột: tên hiển thị (tên dịch vụ) · trạng thái · kiểu khởi động · PID · tài khoản · nhà phát hành · chạy lần cuối (chỉ dịch vụ có exe riêng)\n", list.len());
        for x in list.iter().take(limit) {
            let _ = writeln!(s, "{} ({}) · {} · {} · {} · {} · {}{} · {}", x.name, x.key1, x.state, x.start_type, x.pid, or_dash(&x.account), or_dash(&x.publisher), if x.critical { " · QUAN TRỌNG" } else { "" }, last_run(&u, &x.file_path));
        }
        if list.len() > limit {
            let _ = writeln!(s, "… và {} dịch vụ nữa (lọc theo tên để xem)", list.len() - limit);
        }
        Ok(s)
    }

    fn service_details(&mut self, name: &str) -> Result<String, String> {
        let n = name.to_lowercase();
        let x = {
            let scan = self.scan();
            scan.services
                .iter()
                .find(|x| x.key1.to_lowercase() == n || x.name.to_lowercase() == n)
                .or_else(|| scan.services.iter().find(|x| x.name.to_lowercase().contains(&n) || x.key1.to_lowercase().contains(&n)))
                .cloned()
                .ok_or_else(|| format!("Không tìm thấy dịch vụ `{name}`."))?
        };
        let mut s = String::new();
        let _ = writeln!(s, "{} ({})", x.name, x.key1);
        if !x.description.is_empty() {
            let _ = writeln!(s, "Mô tả: {}", x.description);
        }
        let _ = writeln!(s, "Trạng thái: {} · kiểu khởi động: {} · tài khoản: {}", x.state, x.start_type, or_dash(&x.account));
        let _ = writeln!(s, "Nhà phát hành: {}{}", or_dash(&x.publisher), if x.is_ms { " (Microsoft)" } else { "" });
        let _ = writeln!(s, "Chạy lần cuối: {}", last_run(&crate::core::usage::get(), &x.file_path));
        let _ = writeln!(s, "Lệnh: {}", if self.command_lines { redact(&x.command) } else { x.file_path.clone() });
        if x.critical {
            s.push_str("Ứng dụng coi đây là dịch vụ QUAN TRỌNG: không cho đặt Thủ công / Tắt / Dừng.\n");
        }
        if x.pid != 0 {
            self.fresh();
            if let Some(r) = self.rows.get(&x.pid) {
                let _ = writeln!(s, "Tiến trình PID {}: CPU {:.1}% · RAM {}{}", x.pid, r.cpu, sz(r.mem), if x.shared > 1 { format!(" (dùng chung với {} dịch vụ khác)", x.shared - 1) } else { String::new() });
            }
        }
        Ok(s)
    }

    fn ports(&mut self, args: &Value) -> Result<String, String> {
        let udp = args.get("udp").and_then(Value::as_bool).unwrap_or(false);
        let only = arg_u64(args, "port").map(|p| p as u16);
        let mut uses = ports::list(udp)?;
        uses.retain(|u| only.is_none_or(|p| u.port == p));
        self.fresh();
        for u in &mut uses {
            if let Some(p) = self.procs.as_ref().and_then(|ps| ps.get_static(u.pid)) {
                u.name.clone_from(&p.name);
                u.exe.clone_from(&p.exe);
            }
            if let Some(r) = self.rows.get(&u.pid) {
                u.cpu = r.cpu;
                u.mem = r.mem;
            }
        }
        if uses.is_empty() {
            return Ok(match only {
                Some(p) => format!("Không chương trình nào đang nghe cổng {p}."),
                None => "Không có cổng nào đang nghe.".into(),
            });
        }
        Ok(ports::ai_summary(&uses))
    }

    fn disk_usage(&mut self, path: Option<&str>) -> Result<String, String> {
        match self.app.and_then(|a| a.disk_summary(path)) {
            Some(s) => Ok(s),
            None => Ok(match path {
                Some(p) => format!("Chưa có lần quét ổ đĩa nào chứa {p}. Người dùng có thể quét ở trang Ổ cứng. Có thể dùng folder_files để xem tệp lớn trong một thư mục."),
                None => "Chưa có lần quét ổ đĩa (trang Ổ cứng). Có thể dùng drives và folder_files.".into(),
            }),
        }
    }

    fn cleanup_report(&mut self) -> Result<String, String> {
        if let Some(s) = self.app.and_then(AppData::cleanup_summary) {
            return Ok(s);
        }
        let c = self.cleaner.get_or_insert_with(Cleaner::default);
        if c.ai_summary().is_none() {
            c.scan(|_, _| {})?;
            // đợi các mục lớn đo xong tối đa ~10 s
            for _ in 0..100 {
                if c.ai_summary().is_some_and(|s| !s.contains("đang đo")) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        c.ai_summary().ok_or_else(|| "Không quét được.".into())
    }
}

#[derive(Default)]
struct Group {
    name: String,
    desc: String,
    count: usize,
    cpu: f32,
    mem: u64,
    disk: u64,
    gpu: f32,
    pids: Vec<u32>,
}

impl Group {
    fn line(&self) -> String {
        let pids: Vec<String> = self.pids.iter().take(4).map(u32::to_string).collect();
        format!(
            "{}{} ×{} · CPU {:.1}% · RAM {} · đĩa {}/s · GPU {:.0}% · PID {}{}",
            self.name,
            if self.desc.is_empty() { String::new() } else { format!(" ({})", self.desc) },
            self.count,
            self.cpu,
            sz(self.mem),
            sz(self.disk),
            self.gpu,
            pids.join(","),
            if self.pids.len() > 4 { ",…" } else { "" }
        )
    }
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": schema })
}

/// "chạy lần cuối …" của một exe (svchost / exe chung thì không nói được gì).
fn last_run(u: &crate::core::usage::Usage, exe: &str) -> String {
    if exe.is_empty() || exe.to_lowercase().ends_with("svchost.exe") {
        return "–".into();
    }
    match u.exe(exe) {
        Some((t, n)) => format!("chạy lần cuối {}", crate::core::usage::describe(t, n)),
        None if u.full => "chưa thấy chạy gần đây".into(),
        None => "không rõ (cần quyền Admin)".into(),
    }
}

fn arg_str<'v>(args: &'v Value, key: &str) -> Option<&'v str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    let v = args.get(key)?;
    v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn arg_pid(args: &Value) -> Result<u32, String> {
    arg_u64(args, "pid").and_then(|p| u32::try_from(p).ok()).ok_or_else(|| "Thiếu `pid` (số).".into())
}

fn cap(mut s: String) -> String {
    if s.len() > MAX_OUT {
        let mut cut = MAX_OUT;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n… (cắt bớt – dùng bộ lọc để xem phần còn lại)");
    }
    s
}

fn pct(a: u64, b: u64) -> f64 {
    if b == 0 { 0.0 } else { a as f64 * 100.0 / b as f64 }
}

fn yes(b: bool) -> &'static str {
    if b { "có" } else { "không" }
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "–" } else { s }
}

fn dur(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60);
    if d > 0 { format!("{d} ngày {h} giờ") } else if h > 0 { format!("{h} giờ {m} phút") } else { format!("{m} phút") }
}

fn time_text(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_else(|| "?".into())
}

fn kind_vi(kind: &str) -> &'static str {
    match kind {
        "windows" => "tiến trình Windows",
        "app" => "ứng dụng có cửa sổ",
        _ => "tiến trình nền",
    }
}

fn reach_vi(r: &str) -> &'static str {
    match r {
        "all" => "mọi mạng",
        "network" => "một mạng",
        _ => "chỉ máy này",
    }
}

fn source_matches(source: &str, kind: &str) -> bool {
    match source {
        "registry" => matches!(kind, "Run" | "RegMove"),
        "folder" => kind == "Folder",
        "task" => kind == "Task",
        "service" => kind == "Service",
        "store" => kind == "Packaged",
        _ => true,
    }
}

fn hardware() -> String {
    let i = super::metrics::system_info();
    let mut s = String::new();
    let _ = writeln!(s, "Máy: {} · {}", i.hostname, i.os);
    if !i.smbios.system.is_empty() {
        let _ = writeln!(s, "Model: {}", i.smbios.system);
    }
    if !i.smbios.bios.is_empty() {
        let _ = writeln!(s, "BIOS: {}", i.smbios.bios);
    }
    if i.os_installed_ms > 0 {
        let _ = writeln!(s, "Cài Windows: {}", time_text(i.os_installed_ms));
    }
    let _ = writeln!(s, "CPU: {} · {} nhân / {} luồng · {} MHz", i.cpu_name, i.cores, i.logical, i.base_mhz);
    let _ = writeln!(s, "RAM: {}{}", sz(i.mem_total), if i.smbios.mem_slots > 0 { format!(" · {} / {} khe dùng", i.smbios.mem_modules.len(), i.smbios.mem_slots) } else { String::new() });
    for m in &i.smbios.mem_modules {
        let _ = writeln!(s, "  {} {} {} {} {} MT/s {}", m.slot, sz(m.size), m.kind, m.form, m.speed, m.maker);
    }
    if !i.smbios.board.is_empty() {
        let _ = writeln!(s, "Bo mạch: {}", i.smbios.board);
    }
    for g in super::gpu::adapter_info() {
        let _ = writeln!(s, "GPU: {} · VRAM {}", g.name, sz(g.ded_total));
    }
    for d in &i.disks {
        let _ = writeln!(s, "Ổ vật lý {}: {} · {} · {}", d.id, d.model, sz(d.size), if d.ssd { "SSD" } else { "HDD" });
    }
    if i.boot_ms > 0 {
        let _ = writeln!(s, "Khởi động lúc: {}", time_text(i.boot_ms));
    }
    s
}

fn drives() -> String {
    let mut s = String::new();
    for d in du::drives() {
        if d.ready {
            let _ = writeln!(s, "{} {} ({}, {}): trống {} / {} ({:.0}% đã dùng)", d.letter, d.label, d.kind, d.fs, sz(d.free), sz(d.total), 100.0 - pct(d.free, d.total));
        } else {
            let _ = writeln!(s, "{} ({}): chưa sẵn sàng", d.letter, d.kind);
        }
    }
    s
}

fn folder_files(path: &str, limit: usize) -> Result<String, String> {
    let dir = du::normalize_abs(path).ok_or("Cần đường dẫn đầy đủ, vd. C:\\Users\\A\\Downloads.")?;
    let f = du::list_files(&dir, limit)?;
    let mut s = format!("{dir}: {} tệp nằm trực tiếp, lớn nhất:\n", f.total);
    for it in &f.items {
        let _ = writeln!(s, "{} · {} · sửa {}", it.name, sz(it.size), time_text(it.mtime));
    }
    Ok(s)
}

/// Thời gian tối đa cho `scan_folder`; quá hạn thì trả số liệu đã đếm được (ghi rõ là tối thiểu).
const SCAN_BUDGET: Duration = Duration::from_secs(12);

/// Quét MỘT thư mục ngay lúc được gọi (dùng lại bộ quét của trang Ổ cứng: ≤ 6 luồng, một lần gọi mỗi thư mục, không đi
/// theo junction/symlink, không mở tệp), trả cây thư mục con lớn nhất `depth` tầng. Chỉ đọc tên / cỡ / ngày sửa.
/// Cây quét bị bỏ ngay sau khi trả lời (không giữ RAM).
fn scan_folder(path: &str, depth: usize, limit: usize) -> Result<String, String> {
    let dir = du::normalize_abs(path).ok_or("Cần đường dẫn đầy đủ, vd. C:\\Users\\A\\.cache.")?;
    let t0 = Instant::now();
    let scan = du::Scan::start(0, &dir)?;
    while !scan.is_done() && t0.elapsed() < SCAN_BUDGET {
        std::thread::sleep(Duration::from_millis(40));
    }
    let done = scan.is_done();
    if !done {
        scan.cancel();
    }
    let root = scan.children(0, "size", true, limit).ok_or("Không đọc được kết quả quét.")?;
    let p = scan.progress();
    let mut s = format!(
        "{}: {}{} · {} tệp · {} thư mục · quét {:.1} s{}\n",
        root.path,
        if done { "" } else { "≥ " },
        sz(root.node.size),
        fmt_n(root.node.files),
        fmt_n(u64::from(root.node.dirs)),
        t0.elapsed().as_secs_f64(),
        if done { String::new() } else { format!(" · CHƯA XONG sau {} s, số liệu là tối thiểu – quét thư mục con lớn nhất để xem tiếp", SCAN_BUDGET.as_secs()) },
    );
    if p.errors > 0 {
        let _ = writeln!(s, "{} thư mục không có quyền đọc (bỏ qua)", fmt_n(p.errors));
    }
    s.push_str("Cột: thư mục · dung lượng · số tệp · sửa gần nhất. Thụt lề = thư mục con.\n");
    fn walk(scan: &du::Scan, out: &du::ChildrenOut, level: usize, depth: usize, limit: usize, s: &mut String) {
        let pad = "  ".repeat(level);
        if out.node.own_files > 0 && level > 0 {
            let _ = writeln!(s, "{pad}(tệp nằm trực tiếp) · {} · {} tệp", sz(out.node.own_size), fmt_n(u64::from(out.node.own_files)));
        }
        for c in &out.children {
            let flags = format!("{}{}", if c.denied { " · không có quyền" } else { "" }, if c.link { " · liên kết (không theo)" } else { "" });
            let _ = writeln!(s, "{pad}{} · {} · {} tệp · {}{flags}", c.name, sz(c.size), fmt_n(c.files), du::ym((c.mtime / 1000).clamp(0, i64::from(u32::MAX)) as u32));
            if level + 1 < depth && c.kids > 0 && c.size > 0 {
                if let Some(sub) = scan.children(c.id, "size", true, limit.min(8)) {
                    walk(scan, &sub, level + 1, depth, limit, s);
                }
            }
        }
        if out.more > 0 {
            let _ = writeln!(s, "{pad}… và {} thư mục nhỏ hơn", fmt_n(u64::from(out.more)));
        }
    }
    if root.node.own_files > 0 {
        let _ = writeln!(s, "(tệp nằm trực tiếp) · {} · {} tệp", sz(root.node.own_size), fmt_n(u64::from(root.node.own_files)));
    }
    walk(&scan, &root, 0, depth, limit, &mut s);
    Ok(s)
}

fn fmt_n(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

/// Che bí mật trong dòng lệnh (theo hình dạng, không chắc bắt hết): `--password=…`, `token xyz`, `sk-…`, `user:pass@host`.
pub fn redact(cmd: &str) -> String {
    const SECRET: [&str; 12] = ["password", "passwd", "pwd", "secret", "token", "apikey", "api-key", "api_key", "accesskey", "privatekey", "credential", "auth"];
    const PREFIX: [&str; 8] = ["sk-", "sk_live_", "ghp_", "gho_", "xoxb-", "xoxp-", "AKIA", "eyJ"];
    let mut out = Vec::new();
    let mut mask_next = false;
    for word in cmd.split(' ') {
        if mask_next && !word.is_empty() {
            out.push("[đã ẩn]".to_owned());
            mask_next = false;
            continue;
        }
        let lower = word.to_lowercase();
        let key = lower.trim_start_matches('-').trim_start_matches('/');
        let is_secret_key = SECRET.iter().any(|k| key.split(['=', ':']).next().is_some_and(|name| name.replace(['-', '_', '.'], "").contains(&k.replace(['-', '_'], ""))));
        if is_secret_key {
            if let Some(i) = word.find(['=', ':']) {
                out.push(format!("{}=[đã ẩn]", &word[..i]));
            } else {
                out.push(word.to_owned());
                mask_next = true;
            }
            continue;
        }
        let bare = word.trim_matches(['"', '\'']);
        if PREFIX.iter().any(|p| bare.starts_with(p)) && bare.len() > 16 {
            out.push("[đã ẩn]".to_owned());
            continue;
        }
        // https://user:pass@host
        if let (Some(scheme), Some(at)) = (word.find("://"), word.find('@')) {
            if at > scheme + 3 && word[scheme + 3..at].contains(':') {
                out.push(format!("{}[đã ẩn]{}", &word[..scheme + 3], &word[at..]));
                continue;
            }
        }
        out.push(word.to_owned());
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_masks_secrets_by_shape() {
        assert_eq!(redact("app.exe --password=hunter2 --port 80"), "app.exe --password=[đã ẩn] --port 80");
        assert_eq!(redact("cli --token abc123 run"), "cli --token [đã ẩn] run");
        assert_eq!(redact("git clone https://me:pw@host/x"), "git clone https://[đã ẩn]@host/x");
        assert_eq!(redact("x sk-abcdefghijklmnopqrstu"), "x [đã ẩn]");
        assert_eq!(redact("node server.js"), "node server.js");
    }

    #[test]
    fn every_tool_has_a_schema_and_label() {
        let t = Toolbox::new(None);
        for d in t.tools() {
            let name = d["name"].as_str().unwrap();
            assert!(d["inputSchema"]["type"] == "object", "{name}");
            assert_ne!(label(name), "dữ liệu máy", "{name}");
        }
    }

    #[test]
    /// Quét thử ~/.cache của máy này như trợ lý (in cây thư mục).
    #[ignore] // chạy tay: cargo test --release --lib scan_folder_user_cache -- --ignored --nocapture
    fn scan_folder_user_cache() {
        let home = std::env::var("USERPROFILE").unwrap();
        let out = Toolbox::new(None).run("scan_folder", &json!({ "path": format!("{home}/.cache"), "depth": 2 })).unwrap();
        println!("{out}");
    }

    /// Chạy thật mọi tool trên máy này (chỉ đọc) và in kết quả.
    #[test]
    fn real_tools_run() {
        let mut t = Toolbox::new(None);
        let me = std::process::id();
        for (name, args) in [
            ("system_overview", json!({})),
            ("hardware", json!({})),
            ("list_processes", json!({ "sort": "memory", "limit": 5 })),
            ("list_processes", json!({ "group": true, "limit": 5 })),
            ("process_report", json!({ "pid": me })),
            ("process_tree", json!({ "pid": me })),
            ("startup_items", json!({ "enabled_only": true })),
            ("services", json!({ "running_only": true, "limit": 5 })),
            ("service_details", json!({ "name": "eventlog" })),
            ("ports", json!({})),
            ("drives", json!({})),
            ("disk_usage", json!({})),
            ("scan_folder", json!({ "path": env!("CARGO_MANIFEST_DIR"), "depth": 2 })),
            ("installed_apps", json!({ "limit": 5 })),
            ("app_folders", json!({ "name": "a" })),
            ("app_info", json!({})),
        ] {
            let t0 = Instant::now();
            let out = t.run(name, &args).unwrap_or_else(|e| panic!("{name}: {e}"));
            println!("== {name} ({} ms, {} ký tự)\n{}", t0.elapsed().as_millis(), out.len(), out.lines().take(6).collect::<Vec<_>>().join("\n"));
            assert!(out.len() <= MAX_OUT + 80);
        }
        assert!(t.run("nope", &json!({})).is_err());
        assert!(t.run("process_report", &json!({})).is_err());
    }
}
