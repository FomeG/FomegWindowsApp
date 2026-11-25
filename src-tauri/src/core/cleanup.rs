// Dọn dẹp (Hệ thống → Dọn dẹp): tệp tạm, báo cáo lỗi, dump, cache cập nhật, cache ứng dụng, Thùng rác.
//
// Quét (`Cleaner::scan`, chỉ khi người dùng mở trang / bấm quét):
// - các vị trí cố định (FIXED) + Thùng rác (SHQueryRecycleBinW);
// - tìm thư mục cache trong AppData (Local, LocalLow, Roaming) và các thư mục ẩn ".xxx" của hồ sơ người dùng theo TÊN
//   thư mục (CACHE_NAMES), không đi vào node_modules / .git / LocalCache / chương trình đã cài, rồi gom theo ứng dụng.
//   Không đụng Documents/Desktop/Downloads… – đó là dữ liệu của người dùng, không phải cache.
// - liệt kê bằng du::list_dir (một lần gọi trả cả trăm mục kèm kích thước, không mở từng tệp). Tìm có hạn giờ; đo song song
//   theo thư mục. Sau MEASURE_WAIT_MS trả kết quả ngay, mục chưa đo xong có `pending` và được gửi tiếp qua sự kiện
//   `cleanup_item` khi xong (kho cache hàng triệu tệp của uv/gradle… không bắt giao diện chờ).
//
// Xoá (`Cleaner::apply`): UI chỉ gửi MÃ mục của lần quét gần nhất, không bao giờ gửi đường dẫn. Mỗi gốc được giữ lại,
// chỉ xoá bên trong, VĨNH VIỄN và không hộp thoại:
// - mở từng mục con TƯƠNG ĐỐI với handle thư mục cha (NtOpenFile + FILE_OPEN_REPARSE_POINT): không bao giờ đi theo
//   junction / symlink (liên kết bị bỏ qua, không xoá nội dung đích), không có kẽ hở đổi đường dẫn giữa chừng;
// - tệp mở với share = 0: tệp đang được chương trình khác mở -> mở thất bại -> tự bỏ qua;
// - FILE_OPEN_FOR_BACKUP_INTENT + SeBackup/SeRestore (khi chạy Admin): không vướng ACL; xoá kiểu POSIX và bỏ qua thuộc
//   tính chỉ đọc (FileDispositionInfoEx), không được thì bỏ thuộc tính chỉ đọc rồi xoá kiểu cũ;
// - tuỳ chọn "giữ tệp mới" (< 24 giờ) để không phá bộ cài đang chạy.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{c_void, OsString};
use crate::tr;
use std::os::windows::ffi::OsStringExt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Security::{SE_BACKUP_NAME, SE_RESTORE_NAME};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_LocalAppDataLow, FOLDERID_Profile, FOLDERID_ProgramData, FOLDERID_RoamingAppData,
    SHEmptyRecycleBinW, SHGetKnownFolderPath, SHQueryRecycleBinW, KNOWN_FOLDER_FLAG, SHERB_NOCONFIRMATION,
    SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHQUERYRBINFO,
};

use super::du;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *const c_void, disp: u32, flags: u32, tmpl: *mut c_void) -> *mut c_void;
    fn CloseHandle(h: *mut c_void) -> i32;
    fn GetLastError() -> u32;
    fn GetFileInformationByHandleEx(h: *mut c_void, class: i32, buf: *mut c_void, size: u32) -> i32;
    fn SetFileInformationByHandle(h: *mut c_void, class: i32, buf: *const c_void, size: u32) -> i32;
    fn GetFinalPathNameByHandleW(h: *mut c_void, buf: *mut u16, len: u32, flags: u32) -> u32;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtOpenFile(h: *mut *mut c_void, access: u32, oa: *const ObjectAttributes, iosb: *mut IoStatusBlock, share: u32, options: u32) -> i32;
}

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: *mut c_void,
    object_name: *const UnicodeString,
    attributes: u32,
    security_descriptor: *const c_void,
    security_qos: *const c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status: isize,
    information: usize,
}

const DELETE: u32 = 0x0001_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_LIST_DIRECTORY: u32 = 0x1;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
const SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
const OPEN_EXISTING: u32 = 3;
const FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const OBJ_CASE_INSENSITIVE: u32 = 0x40;
const OPT_DIRECTORY_FILE: u32 = 0x1;
const OPT_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const OPT_NON_DIRECTORY_FILE: u32 = 0x40;
const OPT_OPEN_FOR_BACKUP_INTENT: u32 = 0x4000;
const OPT_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const CLASS_BASIC_INFO: i32 = 0;
const CLASS_DISPOSITION_INFO: i32 = 4;
const CLASS_ATTRIBUTE_TAG_INFO: i32 = 9;
const CLASS_DISPOSITION_INFO_EX: i32 = 21;
const DISPOSITION_DELETE: u32 = 0x1;
const DISPOSITION_POSIX: u32 = 0x2;
const DISPOSITION_IGNORE_READONLY: u32 = 0x10;
const ATTR_READONLY: u32 = 0x1;
const ATTR_DIRECTORY: u32 = 0x10;
const ATTR_NORMAL: u32 = 0x80;
const ATTR_REPARSE: u32 = 0x400;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;

/// Tệp sửa trong khoảng này được giữ lại khi bật "giữ tệp mới".
const RECENT_MS: i64 = 24 * 60 * 60 * 1000;
/// Kết quả quét dùng được trong bao lâu.
const PLAN_LIFETIME: Duration = Duration::from_secs(30 * 60);
/// Nhóm cache nhỏ hơn mức này không hiện (đỡ rối).
const CACHE_MIN: u64 = 5 << 20;
/// Độ sâu tối đa khi tìm thư mục cache (Local\Google\Chrome\User Data\Default\Cache = 5).
const FIND_DEPTH: usize = 6;
/// Xoá: số luồng. Đo 20 000 tệp / 400 thư mục (bench_delete): 1 luồng 5,8 s, 3 luồng 3,2 s, 6 luồng 2,9 s, 12 luồng 3,2 s.
const DELETE_WORKERS: usize = 6;
/// Chu kỳ báo tiến độ dọn lên giao diện.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// Đo kích thước: 6 luồng là điểm nhanh nhất khi đo trang Ổ cứng (nhiều hơn thì tranh nhau trong NTFS/Defender).
const MEASURE_WORKERS: usize = 6;
/// Hạn tìm thư mục cache.
const DISCOVER_MS: u64 = 2500;
/// Chờ đo tối đa chừng này rồi trả kết quả; phần còn lại gửi sau qua sự kiện.
const MEASURE_WAIT_MS: u64 = 800;
const SUMMARY_MAX: usize = 3500;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Base {
    Local,
    LocalLow,
    Roaming,
    Profile,
    ProgramData,
    Windows,
}

struct Fixed {
    id: &'static str,
    group: &'static str,
    name: &'static str,
    note: &'static str,
    safety: &'static str,
    on: bool,
    admin: bool,
    roots: &'static [(Base, &'static str)],
}

/// Vị trí cố định. `on` = chọn sẵn. `admin` = cần chạy app bằng quyền Admin.
const FIXED: &[Fixed] = &[
    Fixed {
        id: "user_temp",
        group: "temp",
        name: "cleanup.fixed.user_temp.name",
        note: "cleanup.fixed.user_temp.note",
        safety: "safe",
        on: true,
        admin: false,
        roots: &[(Base::Local, "Temp")],
    },
    Fixed {
        id: "windows_temp",
        group: "temp",
        name: "cleanup.fixed.windows_temp.name",
        note: "cleanup.fixed.windows_temp.note",
        safety: "safe",
        on: true,
        admin: true,
        roots: &[(Base::Windows, "Temp")],
    },
    Fixed {
        id: "error_reports",
        group: "system",
        name: "cleanup.fixed.error_reports.name",
        note: "cleanup.fixed.error_reports.note",
        safety: "safe",
        on: true,
        admin: false,
        roots: &[
            (Base::Local, "Microsoft\\Windows\\WER"),
            (Base::ProgramData, "Microsoft\\Windows\\WER\\ReportArchive"),
            (Base::ProgramData, "Microsoft\\Windows\\WER\\ReportQueue"),
            (Base::ProgramData, "Microsoft\\Windows\\WER\\Temp"),
        ],
    },
    Fixed {
        id: "crash_dumps",
        group: "system",
        name: "cleanup.fixed.crash_dumps.name",
        note: "cleanup.fixed.crash_dumps.note",
        safety: "safe",
        on: false,
        admin: false,
        roots: &[(Base::Local, "CrashDumps"), (Base::Windows, "Minidump"), (Base::Windows, "LiveKernelReports")],
    },
    Fixed {
        id: "windows_update",
        group: "system",
        name: "cleanup.fixed.windows_update.name",
        note: "cleanup.fixed.windows_update.note",
        safety: "caution",
        on: false,
        admin: true,
        roots: &[(Base::Windows, "SoftwareDistribution\\Download")],
    },
    Fixed {
        id: "delivery_opt",
        group: "system",
        name: "cleanup.fixed.delivery_opt.name",
        note: "cleanup.fixed.delivery_opt.note",
        safety: "safe",
        on: false,
        admin: true,
        roots: &[(Base::Windows, "ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization\\Cache")],
    },
    Fixed {
        id: "shader_cache",
        group: "system",
        name: "cleanup.fixed.shader_cache.name",
        note: "cleanup.fixed.shader_cache.note",
        safety: "safe",
        on: false,
        admin: false,
        roots: &[
            (Base::Local, "D3DSCache"),
            (Base::Local, "NVIDIA\\DXCache"),
            (Base::Local, "NVIDIA\\GLCache"),
            (Base::Local, "AMD\\DxCache"),
            (Base::Local, "AMD\\DxcCache"),
            (Base::Local, "AMD\\GLCache"),
        ],
    },
    Fixed {
        id: "inet_cache",
        group: "system",
        name: "cleanup.fixed.inet_cache.name",
        note: "cleanup.fixed.inet_cache.note",
        safety: "safe",
        on: false,
        admin: false,
        roots: &[(Base::Local, "Microsoft\\Windows\\INetCache")],
    },
];

const RECYCLE_ID: &str = "recycle";

/// Tên thư mục (chữ thường) được coi là cache. Khớp thì lấy cả thư mục, không đi tiếp vào trong.
const CACHE_NAMES: &[&str] = &[
    "cache",
    "cache2",
    "caches",
    "code cache",
    "gpucache",
    "dawncache",
    "dawngraphitecache",
    "dawnwebgpucache",
    "graphitedawncache",
    "grshadercache",
    "shadercache",
    "cacheddata",
    "cachedextensionvsixs",
    "cache_data",
    "htmlcache",
    "npm-cache",
    "go-build",
    "v3-cache",
    ".cache",
];

/// Không đi vào: thư viện dự án, kho git, dữ liệu ứng dụng Store (LocalCache có thể chứa dữ liệu thật, vd. Python Store).
/// Extension / môi trường Python: thư mục tên "cache" trong đó có thể là một phần của extension / thư viện.
/// Thư mục của chính app này: WebView đang dùng.
const SKIP_NAMES: &[&str] = &["node_modules", ".git", "localcache", "windowsapps", "extensions", "site-packages", ".venv", "venv", "envs", "filestore", "com.fomeg.windows-app", "com.quanlykhoidong.app"];

/// Thư mục hãng: tên nhóm lấy thêm thư mục ứng dụng phía sau (Microsoft OneDrive, NVIDIA Corporation GeForceNOW…).
const VENDORS: &[&str] = &["google", "microsoft", "mozilla", "nvidia", "nvidia corporation", "adobe", "riot games", "amd", "intel", "logitech", "unity technologies"];

/// (đoạn đầu đường dẫn, chữ thường) -> (tên hiển thị, loại). Loại: browser | app | dev.
const KNOWN: &[(&[&str], &str, &str)] = &[
    (&["google", "chrome"], "Google Chrome", "browser"),
    (&["microsoft", "edge"], "Microsoft Edge", "browser"),
    (&["bravesoftware"], "Brave", "browser"),
    (&["mozilla", "firefox"], "Firefox", "browser"),
    (&["opera software"], "Opera", "browser"),
    (&["vivaldi"], "Vivaldi", "browser"),
    (&["coccoc"], "Cốc Cốc", "browser"),
    (&["yandex"], "Yandex Browser", "browser"),
    (&["code"], "VS Code", "app"),
    (&["cursor"], "Cursor", "app"),
    (&["microsoft", "teams"], "Microsoft Teams", "app"),
    (&["jetbrains"], "JetBrains", "dev"),
    (&["pip"], "pip", "dev"),
    (&["uv"], "uv", "dev"),
    (&["yarn"], "Yarn", "dev"),
    (&["npm-cache"], "npm", "dev"),
    (&["go-build"], "Go", "dev"),
    (&["nuget"], "NuGet", "dev"),
    (&[".nuget"], "NuGet", "dev"),
    (&[".gradle"], "Gradle", "dev"),
    (&[".cargo"], "Cargo", "dev"),
    (&[".android"], "Android SDK", "dev"),
    (&[".cache"], "~/.cache", "dev"),
];

/// Đoạn đường dẫn không mang tên ứng dụng (bỏ qua khi đặt tên nhóm).
const GENERIC: &[&str] = &["user data", "default", "profiles", "local", "roaming", "app", "data"];

#[derive(Clone, Serialize)]
pub struct ItemOut {
    pub id: String,
    /// temp | system | cache
    pub group: &'static str,
    pub name: String,
    pub note: String,
    pub paths: Vec<String>,
    pub bytes: u64,
    pub files: u64,
    /// phần sửa trong 24 giờ qua (giữ lại khi bật "giữ tệp mới")
    pub recent_bytes: u64,
    pub recent_files: u64,
    /// lần sửa mới nhất, ms Unix (0 = không có tệp)
    pub newest: i64,
    /// safe | caution
    pub safety: &'static str,
    pub default_on: bool,
    /// đang đo (kích thước gửi sau qua sự kiện `cleanup_item`)
    pub pending: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ScanOut {
    pub id: u64,
    pub items: Vec<ItemOut>,
    pub took_ms: f64,
    pub admin: bool,
}

#[derive(Default, Serialize)]
pub struct ItemResult {
    pub id: String,
    pub removed: u64,
    /// tệp/thư mục bỏ qua: đang được dùng, không có quyền, liên kết, hoặc mới (khi giữ tệp mới)
    pub skipped: u64,
    pub kept_recent: u64,
    pub bytes: u64,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ApplyOut {
    pub results: Vec<ItemResult>,
    /// các mục vừa dọn, đã đo lại
    pub items: Vec<ItemOut>,
    pub took_ms: f64,
}

struct Plan {
    id: u64,
    created: Instant,
    items: Vec<ItemOut>,
}

#[derive(Default)]
pub struct Cleaner {
    busy: AtomicBool,
    seq: AtomicU64,
    plan: Arc<Mutex<Option<Plan>>>,
    /// lượt đo đang chạy nền (huỷ khi quét lại / dọn)
    measure: Mutex<Option<Arc<Measure>>>,
}

/// Cờ "đang bận" tự trả lại khi xong (kể cả khi lỗi).
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Cleaner {
    fn enter(&self) -> Result<Busy<'_>, String> {
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(tr!("cleanup.err.busy"));
        }
        Ok(Busy(&self.busy))
    }

    /// Quét. Trả về sau tối đa ~DISCOVER_MS + MEASURE_WAIT_MS; mục chưa đo xong được gửi qua `emit` khi xong.
    pub fn scan(&self, emit: impl Fn(u64, &ItemOut) + Send + 'static) -> Result<ScanOut, String> {
        let _busy = self.enter()?;
        self.stop_measure();
        let t0 = Instant::now();
        let admin = crate::is_elevated();
        let mut items = discover(&Folders::get(), admin);
        for it in items.iter_mut() {
            it.pending = true;
        }
        let (m, rx) = Measure::start(&items);
        let deadline = Instant::now() + Duration::from_millis(MEASURE_WAIT_MS);
        let mut left = items.len();
        while left > 0 {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(i) => {
                    m.fill(i as usize, &mut items[i as usize]);
                    left -= 1;
                }
                Err(_) => break,
            }
        }
        let id = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut p) = self.plan.lock() {
            *p = Some(Plan { id, created: Instant::now(), items: items.clone() });
        }
        if left > 0 {
            if let Ok(mut g) = self.measure.lock() {
                *g = Some(m.clone());
            }
            let plan = self.plan.clone();
            let _ = std::thread::Builder::new().name("clean-wait".into()).stack_size(128 * 1024).spawn(move || {
                for _ in 0..left {
                    let Ok(i) = rx.recv() else { break };
                    if m.cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(mut g) = plan.lock() else { break };
                    let Some(p) = g.as_mut().filter(|p| p.id == id) else { break };
                    let it = &mut p.items[i as usize];
                    m.fill(i as usize, it);
                    let out = it.clone();
                    drop(g);
                    emit(id, &out);
                }
            });
        }
        Ok(ScanOut { id, items, took_ms: t0.elapsed().as_secs_f64() * 1000.0, admin })
    }

    fn stop_measure(&self) {
        if let Some(m) = self.measure.lock().ok().and_then(|mut g| g.take()) {
            m.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Dọn các mục `ids` của lần quét `id`, rồi đo lại CHỈ các mục đó (không quét lại cả máy).
    /// `progress` được gọi ~4 lần/giây trong lúc dọn (từ một luồng riêng, chỉ đọc vài bộ đếm nguyên tử) và một lần khi
    /// chuyển sang đo lại.
    pub fn apply(&self, id: u64, ids: &[String], keep_recent: bool, progress: impl Fn(&CleanProgress) + Sync) -> Result<ApplyOut, String> {
        let _busy = self.enter()?;
        let t0 = Instant::now();
        let mut targets: Vec<ItemOut> = {
            let g = self.plan.lock().map_err(|e| e.to_string())?;
            let Some(plan) = g.as_ref().filter(|p| p.id == id && p.created.elapsed() <= PLAN_LIFETIME) else {
                return Err(tr!("cleanup.err.stale"));
            };
            let want: HashSet<&str> = ids.iter().map(String::as_str).collect();
            plan.items.iter().filter(|it| want.contains(it.id.as_str())).cloned().collect()
        };
        if targets.is_empty() {
            return Err(tr!("cleanup.err.none_selected"));
        }
        self.stop_measure(); // đừng tranh ổ đĩa với việc xoá
        enable_privileges();
        let cutoff = if keep_recent { now_ms() - RECENT_MS } else { i64::MAX };
        let roots: Vec<(usize, String)> = targets
            .iter()
            .enumerate()
            .filter(|(_, t)| t.id != RECYCLE_ID)
            .flat_map(|(i, t)| t.paths.iter().map(move |p| (i, p.clone())))
            .collect();
        let has_recycle = targets.iter().any(|t| t.id == RECYCLE_ID);
        // tổng từ lần quét (Thùng rác không đếm được từng tệp -> không tính vào tổng)
        let files: Vec<u64> = targets.iter().map(|t| if t.id == RECYCLE_ID { 0 } else { t.files }).collect();
        let total_files = files.iter().sum();
        let total_bytes = targets.iter().filter(|t| t.id != RECYCLE_ID).map(|t| t.bytes).sum();
        let tally = Tally { files: AtomicU64::new(0), bytes: AtomicU64::new(0), item: AtomicUsize::new(usize::MAX) };
        let done = AtomicBool::new(false);
        let snapshot = |stage: &'static str| {
            let i = tally.item.load(Ordering::Relaxed);
            CleanProgress {
                stage,
                done_files: tally.files.load(Ordering::Relaxed),
                total_files,
                freed_bytes: tally.bytes.load(Ordering::Relaxed),
                total_bytes,
                item: targets.get(i).or_else(|| (targets.len() == 1).then(|| &targets[0])).map(|t| t.name.clone()).unwrap_or_default(),
                recycle: has_recycle,
                elapsed_ms: t0.elapsed().as_millis() as u64,
            }
        };
        // Thùng rác dọn song song với phần xoá tệp; một luồng nhỏ báo tiến độ tới khi xong
        let (counts, errors, recycle) = std::thread::scope(|s| {
            let rh = has_recycle.then(|| s.spawn(empty_recycle));
            let reporter = s.spawn(|| {
                progress(&snapshot("delete"));
                while !done.load(Ordering::Acquire) {
                    std::thread::park_timeout(PROGRESS_EVERY);
                    if !done.load(Ordering::Acquire) {
                        progress(&snapshot("delete"));
                    }
                }
            });
            let (c, e) = Deleter::run(targets.len(), &roots, cutoff, &tally);
            let r = rh.and_then(|h| h.join().ok());
            done.store(true, Ordering::Release);
            reporter.thread().unpark(); // đừng đợi hết chu kỳ
            (c, e, r)
        });
        progress(&snapshot("measure"));
        let mut recycle = recycle;
        let results: Vec<ItemResult> = targets
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if t.id == RECYCLE_ID {
                    return recycle.take().unwrap_or_default();
                }
                let c = counts[i];
                ItemResult { id: t.id.clone(), removed: c.removed, skipped: c.skipped, kept_recent: c.kept_recent, bytes: c.bytes, error: errors[i].clone() }
            })
            .collect();

        // đo lại các mục vừa dọn (phần còn lại thường rất nhỏ) và cập nhật kết quả quét đang giữ
        let (m, rx) = Measure::start(&targets);
        for _ in 0..targets.len() {
            let Ok(i) = rx.recv() else { break };
            m.fill(i as usize, &mut targets[i as usize]);
        }
        for t in targets.iter_mut().filter(|t| t.id == RECYCLE_ID) {
            let (s, _) = query_recycle();
            t.bytes = s.bytes;
            t.files = s.files;
        }
        if let Ok(mut g) = self.plan.lock() {
            if let Some(p) = g.as_mut().filter(|p| p.id == id) {
                for t in &targets {
                    if let Some(x) = p.items.iter_mut().find(|x| x.id == t.id) {
                        *x = t.clone();
                    }
                }
                p.created = Instant::now();
            }
        }
        Ok(ApplyOut { results, items: targets, took_ms: t0.elapsed().as_secs_f64() * 1000.0 })
    }

    /// Tóm tắt lần quét gần nhất cho trợ lý AI (khối `<cleanup>`), None nếu chưa quét.
    pub fn ai_summary(&self) -> Option<String> {
        let g = self.plan.lock().ok()?;
        let plan = g.as_ref()?;
        Some(summary(&plan.items))
    }
}

// ---------------- vị trí ----------------

#[derive(Clone)]
struct Folders {
    local: Option<String>,
    local_low: Option<String>,
    roaming: Option<String>,
    profile: Option<String>,
    program_data: Option<String>,
    windows: Option<String>,
}

impl Folders {
    fn get() -> Self {
        Folders {
            local: known(&FOLDERID_LocalAppData),
            local_low: known(&FOLDERID_LocalAppDataLow),
            roaming: known(&FOLDERID_RoamingAppData),
            profile: known(&FOLDERID_Profile),
            program_data: known(&FOLDERID_ProgramData),
            windows: windows_dir(),
        }
    }
    fn base(&self, b: Base) -> Option<&str> {
        match b {
            Base::Local => self.local.as_deref(),
            Base::LocalLow => self.local_low.as_deref(),
            Base::Roaming => self.roaming.as_deref(),
            Base::Profile => self.profile.as_deref(),
            Base::ProgramData => self.program_data.as_deref(),
            Base::Windows => self.windows.as_deref(),
        }
    }
    fn join(&self, b: Base, rel: &str) -> Option<String> {
        self.base(b).map(|p| format!("{}\\{rel}", p.trim_end_matches('\\')))
    }
}

fn known(id: &GUID) -> Option<String> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = OsString::from_wide(p.as_wide()).to_string_lossy().into_owned();
        CoTaskMemFree(Some(p.0.cast()));
        (!s.is_empty()).then_some(s)
    }
}

fn windows_dir() -> Option<String> {
    let mut buf = [0u16; 260];
    let n = unsafe { GetSystemWindowsDirectoryW(Some(&mut buf)) } as usize;
    // GetSystemWindowsDirectory trả "C:\WINDOWS": lấy chữ hoa/thường thật qua đường dẫn cuối của handle
    (n > 0 && n < buf.len()).then(|| {
        let s = String::from_utf16_lossy(&buf[..n]);
        open_dir_path(&s).map(|h| {
            let real = final_path(h.0);
            drop(h);
            real.unwrap_or_else(|| s.clone())
        }).unwrap_or(s)
    })
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ---------------- quét ----------------

#[derive(Default, Clone, Copy)]
struct Size {
    bytes: u64,
    files: u64,
    recent_bytes: u64,
    recent_files: u64,
    newest: i64,
}

impl Size {
    fn add(&mut self, o: &Size) {
        self.bytes += o.bytes;
        self.files += o.files;
        self.recent_bytes += o.recent_bytes;
        self.recent_files += o.recent_files;
        self.newest = self.newest.max(o.newest);
    }
}

fn nul(s: &str) -> Vec<u16> {
    let mut w = du::long_path(s);
    w.push(0);
    w
}

fn is_cache(parents: &[String], name: &str) -> bool {
    CACHE_NAMES.contains(&name) || (name == "packages" && parents.len() == 1 && parents[0].eq_ignore_ascii_case(".nuget"))
}

/// Bước 1: danh sách mục (chưa có kích thước). Tìm cache theo tên thư mục, có hạn giờ `DISCOVER_MS`.
fn discover(f: &Folders, admin: bool) -> Vec<ItemOut> {
    let until = Instant::now() + Duration::from_millis(DISCOVER_MS);
    // vị trí đã có mục riêng (và chương trình đã cài) -> không tìm cache trong đó
    let mut skip: HashSet<String> = FIXED.iter().flat_map(|x| x.roots.iter().filter_map(|&(b, r)| f.join(b, r))).map(|s| s.to_lowercase()).collect();
    for (b, r) in [(Base::Local, "Programs"), (Base::Local, "Microsoft\\WindowsApps"), (Base::Local, "Packages")] {
        if let Some(p) = f.join(b, r) {
            skip.insert(p.to_lowercase());
        }
    }

    let mut tasks: Vec<(String, Vec<String>)> = Vec::new();
    let mut buf = vec![0u64; du::BUF_U64];
    for (b, dots_only) in [(Base::Local, false), (Base::LocalLow, false), (Base::Roaming, false), (Base::Profile, true)] {
        let Some(root) = f.base(b) else { continue };
        let root = root.trim_end_matches('\\').to_owned();
        let _ = du::list_dir(&nul(&root), &mut buf, |e| {
            if e.attrs & ATTR_DIRECTORY == 0 || e.attrs & ATTR_REPARSE != 0 {
                return;
            }
            let name = String::from_utf16_lossy(e.name);
            if dots_only && !name.starts_with('.') {
                return; // Documents, Desktop, dự án… là dữ liệu của người dùng
            }
            let path = format!("{root}\\{name}");
            if !skip.contains(&path.to_lowercase()) && !SKIP_NAMES.contains(&name.to_lowercase().as_str()) {
                tasks.push((path, vec![name]));
            }
        });
    }
    // ứng dụng Store: chỉ tìm trong AC (cache của trình duyệt nhúng), bỏ LocalState / LocalCache (dữ liệu thật)
    if let Some(pk) = f.join(Base::Local, "Packages") {
        let _ = du::list_dir(&nul(&pk), &mut buf, |e| {
            if e.attrs & ATTR_DIRECTORY != 0 && e.attrs & ATTR_REPARSE == 0 {
                let name = String::from_utf16_lossy(e.name);
                tasks.push((format!("{pk}\\{name}\\AC"), vec!["Packages".into(), name, "AC".into()]));
            }
        });
    }

    let queue = Mutex::new(tasks);
    let hits = Mutex::new(Vec::<(Vec<String>, String)>::new());
    std::thread::scope(|s| {
        for _ in 0..MEASURE_WORKERS {
            s.spawn(|| {
                let mut buf = vec![0u64; du::BUF_U64];
                // and_then nhận guard theo giá trị -> khoá được trả ngay, các luồng không chờ nhau
                while let Some((path, comps)) = queue.lock().ok().and_then(|mut q| q.pop()) {
                    let found = find_caches(&path, comps, &skip, until, &mut buf);
                    if let Ok(mut h) = hits.lock() {
                        h.extend(found);
                    }
                }
            });
        }
    });

    let mut items: Vec<ItemOut> = FIXED
        .iter()
        .map(|d| {
            let need_admin = d.admin && !admin;
            let paths = if need_admin { Vec::new() } else { d.roots.iter().filter_map(|&(b, r)| f.join(b, r)).collect() };
            let error = need_admin.then(|| tr!("cleanup.err.need_admin"));
            row(d.id.into(), d.group, tr!(d.name), tr!(d.note), paths, d.safety, d.on, error)
        })
        .collect();
    let (rb, err) = query_recycle();
    let mut recycle = row(RECYCLE_ID.into(), "system", tr!("cleanup.recycle.name"), tr!("cleanup.recycle.note"), Vec::new(), "caution", false, err);
    recycle.bytes = rb.bytes;
    recycle.files = rb.files;
    items.push(recycle);

    let mut groups: HashMap<String, ItemOut> = HashMap::new();
    for (comps, path) in hits.into_inner().unwrap_or_default() {
        let (label, kind) = app_label(&comps);
        let id = format!("cache:{}", label.to_lowercase());
        groups
            .entry(id.clone())
            .or_insert_with(|| {
                let (note, safety) = cache_note(kind, &label);
                row(id, "cache", label, tr!(note), Vec::new(), safety, false, None)
            })
            .paths
            .push(path);
    }
    let mut caches: Vec<ItemOut> = groups.into_values().collect();
    for c in &mut caches {
        c.paths.sort_unstable();
    }
    caches.sort_by_cached_key(|c| c.name.to_lowercase());
    items.extend(caches);
    items
}

#[allow(clippy::too_many_arguments)]
fn row(id: String, group: &'static str, name: String, note: String, paths: Vec<String>, safety: &'static str, on: bool, error: Option<String>) -> ItemOut {
    ItemOut { id, group, name, note, paths, bytes: 0, files: 0, recent_bytes: 0, recent_files: 0, newest: 0, safety, default_on: on, pending: false, error }
}

/// Tìm thư mục cache trong `path` theo tên (sâu tối đa FIND_DEPTH, dừng khi hết giờ).
fn find_caches(path: &str, comps: Vec<String>, skip: &HashSet<String>, until: Instant, buf: &mut [u64]) -> Vec<(Vec<String>, String)> {
    let last = comps.last().map(|s| s.to_lowercase()).unwrap_or_default();
    if is_cache(&comps[..comps.len() - 1], &last) {
        return vec![(comps, path.to_owned())];
    }
    let mut hits = Vec::new();
    let mut stack = vec![(path.to_owned(), comps)];
    while let Some((dir, comps)) = stack.pop() {
        if Instant::now() > until {
            break;
        }
        let _ = du::list_dir(&nul(&dir), buf, |e| {
            if e.attrs & ATTR_DIRECTORY == 0 || e.attrs & ATTR_REPARSE != 0 {
                return;
            }
            let name = String::from_utf16_lossy(e.name);
            let lower = name.to_lowercase();
            let child = format!("{dir}\\{name}");
            if SKIP_NAMES.contains(&lower.as_str()) || skip.contains(&child.to_lowercase()) {
                return;
            }
            let cache = is_cache(&comps, &lower);
            let mut c = comps.clone();
            c.push(name);
            if cache {
                hits.push((c, child));
            } else if c.len() < FIND_DEPTH {
                stack.push((child, c));
            }
        });
    }
    hits
}

/// Bước 2: đo song song theo THƯ MỤC (một kho cache hàng triệu tệp cũng được chia cho mọi luồng).
/// Mỗi mục đo xong -> gửi chỉ số mục qua kênh `done`.
pub struct Measure {
    cancel: AtomicBool,
    /// việc chia XOAY VÒNG giữa các mục: mỗi mục một ngăn xếp (thư mục, là gốc), hàng `turn` = mục còn việc.
    /// Mục nhỏ xong ngay, không phải chờ sau kho cache hàng triệu tệp.
    queue: Mutex<Rr>,
    outstanding: AtomicUsize,
    left: Vec<AtomicUsize>,
    sizes: Vec<Mutex<Size>>,
    errors: Vec<Mutex<Option<String>>>,
    recent_after: i64,
}

/// Hàng việc xoay vòng của `Measure`.
struct Rr {
    turn: VecDeque<u32>,
    stacks: Vec<Vec<(String, bool)>>,
}

impl Rr {
    /// Một thư mục của mục đến lượt; mục còn việc thì xếp lại cuối hàng.
    fn take(&mut self) -> Option<(u32, String, bool)> {
        let i = self.turn.pop_front()?;
        let (dir, root) = self.stacks[i as usize].pop()?;
        if !self.stacks[i as usize].is_empty() {
            self.turn.push_back(i);
        }
        Some((i, dir, root))
    }
    fn give(&mut self, i: u32, dirs: Vec<String>) {
        let st = &mut self.stacks[i as usize];
        if st.is_empty() {
            self.turn.push_back(i);
        }
        st.extend(dirs.into_iter().map(|d| (d, false)));
    }
}

impl Measure {
    fn start(items: &[ItemOut]) -> (Arc<Measure>, mpsc::Receiver<u32>) {
        let mut rr = Rr { turn: VecDeque::new(), stacks: vec![Vec::new(); items.len()] };
        let mut total = 0;
        for (i, it) in items.iter().enumerate() {
            if it.id != RECYCLE_ID && !it.paths.is_empty() {
                rr.stacks[i] = it.paths.iter().map(|p| (p.clone(), true)).collect();
                total += it.paths.len();
                rr.turn.push_back(i as u32);
            }
        }
        let m = Arc::new(Measure {
            cancel: AtomicBool::new(false),
            outstanding: AtomicUsize::new(total),
            left: items.iter().map(|it| AtomicUsize::new(if it.id == RECYCLE_ID { 0 } else { it.paths.len() })).collect(),
            sizes: items.iter().map(|_| Mutex::new(Size::default())).collect(),
            errors: items.iter().map(|_| Mutex::new(None)).collect(),
            queue: Mutex::new(rr),
            recent_after: now_ms() - RECENT_MS,
        });
        let (tx, rx) = mpsc::channel();
        for (i, l) in m.left.iter().enumerate() {
            if l.load(Ordering::Relaxed) == 0 {
                let _ = tx.send(i as u32);
            }
        }
        for _ in 0..MEASURE_WORKERS {
            let (m, tx) = (m.clone(), tx.clone());
            let _ = std::thread::Builder::new().name("clean-measure".into()).stack_size(256 * 1024).spawn(move || m.work(&tx));
        }
        (m, rx)
    }

    fn work(&self, done: &mpsc::Sender<u32>) {
        let mut buf = vec![0u64; du::BUF_U64];
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }
            let job = self.queue.lock().ok().and_then(|mut q| q.take());
            let Some((i, dir, root)) = job else {
                if self.outstanding.load(Ordering::Acquire) == 0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
            };
            let idx = i as usize;
            let mut sz = Size::default();
            let mut subs = Vec::new();
            let recent_after = self.recent_after;
            let r = du::list_dir(&nul(&dir), &mut buf, |e| {
                if e.attrs & ATTR_REPARSE != 0 {
                    return;
                }
                if e.attrs & ATTR_DIRECTORY != 0 {
                    subs.push(format!("{dir}\\{}", String::from_utf16_lossy(e.name)));
                } else {
                    sz.bytes += e.size;
                    sz.files += 1;
                    sz.newest = sz.newest.max(e.mtime);
                    if e.mtime >= recent_after {
                        sz.recent_bytes += e.size;
                        sz.recent_files += 1;
                    }
                }
            });
            if let (true, Err(e)) = (root, r) {
                if e != ERROR_FILE_NOT_FOUND && e != ERROR_PATH_NOT_FOUND {
                    if let Ok(mut g) = self.errors[idx].lock() {
                        *g = Some(tr!("cleanup.err.partial_read"));
                    }
                }
            }
            // thêm việc con TRƯỚC khi trả việc này, để bộ đếm không về 0 sớm
            if !subs.is_empty() {
                self.left[idx].fetch_add(subs.len(), Ordering::AcqRel);
                self.outstanding.fetch_add(subs.len(), Ordering::AcqRel);
                if let Ok(mut q) = self.queue.lock() {
                    q.give(i, subs);
                }
            }
            if let Ok(mut g) = self.sizes[idx].lock() {
                g.add(&sz);
            }
            self.outstanding.fetch_sub(1, Ordering::AcqRel);
            if self.left[idx].fetch_sub(1, Ordering::AcqRel) == 1 {
                let _ = done.send(i);
            }
        }
    }

    /// Chép số liệu đã đo xong vào mục.
    fn fill(&self, i: usize, it: &mut ItemOut) {
        it.pending = false;
        if it.id == RECYCLE_ID {
            return;
        }
        let s = self.sizes[i].lock().map(|g| *g).unwrap_or_default();
        it.bytes = s.bytes;
        it.files = s.files;
        it.recent_bytes = s.recent_bytes;
        it.recent_files = s.recent_files;
        it.newest = s.newest;
        if it.error.is_none() {
            it.error = self.errors[i].lock().ok().and_then(|g| g.clone());
        }
    }
}

/// Tên nhóm từ các đoạn đường dẫn (gồm cả thư mục cache ở cuối) + loại (browser | app | dev).
fn app_label(comps: &[String]) -> (String, &'static str) {
    let lower: Vec<String> = comps.iter().map(|s| s.to_lowercase()).collect();
    for &(prefix, label, kind) in KNOWN {
        if lower.len() >= prefix.len() && prefix.iter().zip(&lower).all(|(a, b)| a == b) {
            // JetBrains: thêm tên IDE (IntelliJIdea2024.1…)
            if prefix == ["jetbrains"] && lower.len() > 2 {
                return (format!("JetBrains {}", comps[1]), kind);
            }
            return (label.to_owned(), kind);
        }
    }
    // ứng dụng Store: Packages\<Tên>_<mã>\AC\…
    if lower.first().is_some_and(|s| s == "packages") && comps.len() > 1 {
        let name = comps[1].split('_').next().unwrap_or(&comps[1]);
        return (name.to_owned(), "app");
    }
    // tên ứng dụng = thư mục đầu tiên; thư mục hãng (Microsoft, NVIDIA…) thì thêm thư mục sau đó
    let parents = &comps[..comps.len().saturating_sub(1)];
    let mut parts = parents.iter().filter(|s| {
        let l = s.to_lowercase();
        !GENERIC.contains(&l.as_str()) && !CACHE_NAMES.contains(&l.as_str())
    });
    let label = match parts.next() {
        Some(a) if VENDORS.contains(&a.to_lowercase().as_str()) => match parts.next() {
            Some(b) => format!("{a} {b}"),
            None => a.clone(),
        },
        Some(a) => a.clone(),
        None => comps.last().cloned().unwrap_or_default(),
    };
    let kind = if label.starts_with('.') { "dev" } else { "app" };
    (label, kind)
}

/// Ghi chú (khoá yml) + mức an toàn của một nhóm cache.
fn cache_note(kind: &str, label: &str) -> (&'static str, &'static str) {
    match kind {
        "browser" => ("cleanup.cache_note.browser", "safe"),
        "dev" if label == "~/.cache" => ("cleanup.cache_note.dot_cache", "caution"),
        "dev" => ("cleanup.cache_note.dev", "caution"),
        _ => ("cleanup.cache_note.app", "safe"),
    }
}

// ---------------- Thùng rác ----------------

fn query_recycle() -> (Size, Option<String>) {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    match unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) } {
        Ok(()) => (Size { bytes: info.i64Size.max(0) as u64, files: info.i64NumItems.max(0) as u64, ..Default::default() }, None),
        Err(_) => (Size::default(), Some(tr!("cleanup.err.read_recycle"))),
    }
}

fn empty_recycle() -> ItemResult {
    let mut r = ItemResult { id: RECYCLE_ID.into(), ..Default::default() };
    let (before, _) = query_recycle();
    if before.files == 0 {
        return r;
    }
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let res = unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) };
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
    let (after, _) = query_recycle();
    r.removed = before.files.saturating_sub(after.files);
    r.bytes = before.bytes.saturating_sub(after.bytes);
    r.skipped = after.files;
    if res.is_err() && r.removed == 0 {
        r.error = Some(tr!("cleanup.err.empty_recycle"));
    }
    r
}

// ---------------- xoá ----------------

/// Handle tự đóng.
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn enable_privileges() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        // Chỉ có khi chạy Admin. Không có thì xoá theo quyền thường (tệp không có quyền sẽ được bỏ qua).
        super::proc_actions::enable_privilege(SE_BACKUP_NAME);
        super::proc_actions::enable_privilege(SE_RESTORE_NAME);
    });
}

fn open_dir_path(path: &str) -> Option<Handle> {
    let w = nul(path);
    let h = unsafe {
        CreateFileW(w.as_ptr(), FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE, SHARE_ALL, std::ptr::null(), OPEN_EXISTING, FLAG_BACKUP_SEMANTICS | FLAG_OPEN_REPARSE_POINT, std::ptr::null_mut())
    };
    (h as isize != -1).then_some(Handle(h))
}

fn final_path(h: *mut c_void) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetFinalPathNameByHandleW(h, buf.as_mut_ptr(), buf.len() as u32, 0) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..n]);
    Some(s.strip_prefix(r"\\?\").unwrap_or(&s).to_owned())
}

/// Thuộc tính của handle đã mở (để biết có phải liên kết không).
fn attrs_of(h: *mut c_void) -> Option<u32> {
    let mut tag = [0u32; 2];
    let ok = unsafe { GetFileInformationByHandleEx(h, CLASS_ATTRIBUTE_TAG_INFO, tag.as_mut_ptr().cast(), 8) };
    (ok != 0).then_some(tag[0])
}

/// Mở mục con `name` TƯƠNG ĐỐI với handle thư mục `parent` (không đi theo liên kết).
fn open_child(parent: *mut c_void, name: &[u16], access: u32, share: u32, options: u32) -> Option<Handle> {
    let us = UnicodeString { length: (name.len() * 2) as u16, maximum_length: (name.len() * 2) as u16, buffer: name.as_ptr() };
    let oa = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent,
        object_name: &us,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: std::ptr::null(),
        security_qos: std::ptr::null(),
    };
    let mut iosb = IoStatusBlock { status: 0, information: 0 };
    let mut h: *mut c_void = std::ptr::null_mut();
    let st = unsafe { NtOpenFile(&mut h, access, &oa, &mut iosb, share, options | OPT_OPEN_REPARSE_POINT | OPT_OPEN_FOR_BACKUP_INTENT | OPT_SYNCHRONOUS_IO_NONALERT) };
    (st >= 0 && !h.is_null()).then_some(Handle(h))
}

/// Đánh dấu xoá mục đang mở (xoá hẳn khi đóng handle). POSIX + bỏ qua chỉ đọc; không được thì cách cũ.
fn delete_open(h: *mut c_void) -> bool {
    let flags: u32 = DISPOSITION_DELETE | DISPOSITION_POSIX | DISPOSITION_IGNORE_READONLY;
    if unsafe { SetFileInformationByHandle(h, CLASS_DISPOSITION_INFO_EX, (&raw const flags).cast(), 4) } != 0 {
        return true;
    }
    // Windows cũ / ổ không phải NTFS: bỏ thuộc tính chỉ đọc rồi xoá kiểu cũ
    let mut basic = [0u8; 40]; // FILE_BASIC_INFO: 4 × i64 thời gian + u32 thuộc tính (+ đệm)
    if unsafe { GetFileInformationByHandleEx(h, CLASS_BASIC_INFO, basic.as_mut_ptr().cast(), 40) } != 0 {
        let attrs = u32::from_le_bytes(basic[32..36].try_into().unwrap_or_default());
        if attrs & ATTR_READONLY != 0 {
            let mut set = [0u8; 40]; // thời gian = 0: không đổi
            let a = match attrs & !ATTR_READONLY { 0 => ATTR_NORMAL, x => x };
            set[32..36].copy_from_slice(&a.to_le_bytes());
            unsafe { SetFileInformationByHandle(h, CLASS_BASIC_INFO, set.as_ptr().cast(), 40) };
        }
    }
    let del: u8 = 1;
    unsafe { SetFileInformationByHandle(h, CLASS_DISPOSITION_INFO, (&raw const del).cast(), 1) != 0 }
}

#[derive(Default, Clone, Copy)]
struct Counts {
    removed: u64,
    skipped: u64,
    kept_recent: u64,
    bytes: u64,
}

impl Counts {
    fn add(&mut self, o: &Counts) {
        self.removed += o.removed;
        self.skipped += o.skipped;
        self.kept_recent += o.kept_recent;
        self.bytes += o.bytes;
    }
}

// Handle của kernel dùng được từ mọi luồng (mỗi thư mục chỉ một luồng liệt kê tại một thời điểm).
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

/// Thư mục đang dọn. Giữ handle tới khi mọi thư mục con đã xong, rồi (nếu không phải gốc) xoá nó khi đã rỗng.
/// Chỉ thư mục "đang mở dở" giữ handle; thư mục con chờ trong hàng chỉ là (cha, tên).
struct DirNode {
    h: Handle,
    parent: Option<Arc<DirNode>>,
    /// việc chưa xong: thư mục con chưa dọn xong + 1 (chính nó đang được liệt kê)
    pending: AtomicUsize,
    remove: bool,
    item: usize,
}

enum Work {
    /// liệt kê + xoá tệp trong thư mục đã mở
    List(Arc<DirNode>),
    /// mở thư mục con `name` (tương đối với cha) rồi liệt kê
    Open { parent: Arc<DirNode>, name: Vec<u16>, mtime: i64 },
}

/// Xoá song song theo THƯ MỤC: mọi luồng lấy việc từ một hàng chung, nên một %TEMP% lớn cũng chia đều.
struct Deleter<'t> {
    queue: Mutex<Vec<Work>>,
    outstanding: AtomicUsize,
    counts: Vec<Mutex<Counts>>,
    cutoff: i64,
    tally: &'t Tally,
}

/// Bộ đếm tiến độ chung của mọi luồng xoá (chỉ cộng nguyên tử, một lần mỗi thư mục).
struct Tally {
    /// tệp đã xử lý (xoá + bỏ qua + giữ lại)
    files: AtomicU64,
    bytes: AtomicU64,
    /// mục vừa được xử lý (để hiện "đang dọn: …")
    item: AtomicUsize,
}

/// Tiến độ dọn gửi lên giao diện (sự kiện `cleanup_progress`).
#[derive(Serialize, Clone)]
pub struct CleanProgress {
    /// "delete" = đang xoá, "measure" = xoá xong, đang đo lại
    pub stage: &'static str,
    pub done_files: u64,
    /// tổng số tệp theo lần quét (không gồm Thùng rác)
    pub total_files: u64,
    pub freed_bytes: u64,
    pub total_bytes: u64,
    /// tên mục vừa xử lý
    pub item: String,
    /// có dọn Thùng rác (chạy song song, không có số tệp)
    pub recycle: bool,
    pub elapsed_ms: u64,
}

impl Deleter<'_> {
    /// Dọn bên trong `roots` (mục, đường dẫn). Trả số liệu theo mục + lỗi theo mục.
    fn run(n_items: usize, roots: &[(usize, String)], cutoff: i64, tally: &Tally) -> (Vec<Counts>, Vec<Option<String>>) {
        let mut errors = vec![None; n_items];
        let mut work = Vec::new();
        for (item, path) in roots {
            match open_root(path) {
                Ok(Some(h)) => work.push(Work::List(Arc::new(DirNode { h, parent: None, pending: AtomicUsize::new(1), remove: false, item: *item }))),
                Ok(None) => {}
                Err(e) => errors[*item] = Some(e),
            }
        }
        let d = Deleter {
            outstanding: AtomicUsize::new(work.len()),
            queue: Mutex::new(work),
            counts: (0..n_items).map(|_| Mutex::new(Counts::default())).collect(),
            cutoff,
            tally,
        };
        std::thread::scope(|s| {
            for _ in 0..DELETE_WORKERS {
                s.spawn(|| d.work());
            }
        });
        (d.counts.into_iter().map(|m| m.into_inner().unwrap_or_default()).collect(), errors)
    }

    fn work(&self) {
        let mut buf = vec![0u64; 2048]; // 16 KB / lần gọi
        let mut entries: Vec<(Vec<u16>, u32, u64, i64)> = Vec::new();
        loop {
            let Some(w) = self.queue.lock().ok().and_then(|mut q| q.pop()) else {
                if self.outstanding.load(Ordering::Acquire) == 0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
            };
            match w {
                Work::List(node) => self.list(node, &mut buf, &mut entries),
                Work::Open { parent, name, mtime } => {
                    let child = open_child(parent.h.0, &name, FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE, SHARE_ALL, OPT_DIRECTORY_FILE)
                        .filter(|h| attrs_of(h.0).is_some_and(|a| a & ATTR_REPARSE == 0));
                    match child {
                        Some(h) => {
                            let item = parent.item;
                            let node = Arc::new(DirNode { h, parent: Some(parent), pending: AtomicUsize::new(1), remove: mtime < self.cutoff, item });
                            self.list(node, &mut buf, &mut entries);
                        }
                        None => {
                            self.add(parent.item, &Counts { skipped: 1, ..Default::default() });
                            finish(parent);
                        }
                    }
                }
            }
            self.outstanding.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn add(&self, item: usize, c: &Counts) {
        if let Ok(mut g) = self.counts[item].lock() {
            g.add(c);
        }
        self.tally.files.fetch_add(c.removed + c.skipped + c.kept_recent, Ordering::Relaxed);
        self.tally.bytes.fetch_add(c.bytes, Ordering::Relaxed);
        self.tally.item.store(item, Ordering::Relaxed);
    }

    fn list(&self, node: Arc<DirNode>, buf: &mut [u64], entries: &mut Vec<(Vec<u16>, u32, u64, i64)>) {
        entries.clear();
        let listed = unsafe { du::list_handle(node.h.0, buf, |e| entries.push((e.name.to_vec(), e.attrs, e.size, e.mtime))) };
        let mut c = Counts::default();
        if listed.is_err() && entries.is_empty() {
            c.skipped += 1;
        }
        let mut subs = Vec::new();
        for (name, attrs, size, mtime) in entries.drain(..) {
            if attrs & ATTR_REPARSE != 0 {
                c.skipped += 1; // liên kết: không đụng tới (cả liên kết lẫn nơi nó trỏ tới)
            } else if attrs & ATTR_DIRECTORY != 0 {
                subs.push(Work::Open { parent: node.clone(), name, mtime });
            } else if mtime >= self.cutoff {
                c.kept_recent += 1;
            } else {
                // share = 0: tệp đang được mở ở đâu đó -> mở thất bại -> bỏ qua, không hỏi.
                // (Đã thử mở kèm "xoá khi đóng" để bớt một lần gọi: không nhanh hơn – chi phí nằm ở bộ lọc Defender/NTFS.)
                match open_child(node.h.0, &name, DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE, 0, OPT_NON_DIRECTORY_FILE) {
                    Some(f) if delete_open(f.0) => {
                        c.removed += 1;
                        c.bytes += size;
                    }
                    _ => c.skipped += 1,
                }
            }
        }
        self.add(node.item, &c);
        if !subs.is_empty() {
            node.pending.fetch_add(subs.len(), Ordering::AcqRel);
            self.outstanding.fetch_add(subs.len(), Ordering::AcqRel);
            if let Ok(mut q) = self.queue.lock() {
                q.extend(subs);
            }
        }
        finish(node);
    }
}

/// Một việc của `node` xong. Hết việc -> xoá thư mục (nếu được; còn tệp bên trong thì Windows từ chối – đúng ý),
/// đóng handle, báo lên cha.
fn finish(node: Arc<DirNode>) {
    let mut cur = Some(node);
    while let Some(n) = cur.take() {
        if n.pending.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        if n.remove {
            delete_open(n.h.0);
        }
        cur = n.parent.clone();
    }
}

/// Mở gốc để dọn: Ok(None) = không tồn tại. Gốc phải là thư mục thật và đường dẫn thật đúng như mong đợi
/// (không đi qua liên kết nào).
fn open_root(root: &str) -> Result<Option<Handle>, String> {
    let root = root.trim_end_matches('\\');
    if du::protected(root) || root.len() < 8 {
        return Err(tr!("cleanup.err.protected"));
    }
    let Some(h) = open_dir_path(root) else {
        return match unsafe { GetLastError() } {
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
            _ => Err(tr!("cleanup.err.open_dir")),
        };
    };
    let a = attrs_of(h.0).unwrap_or(0);
    if a & ATTR_DIRECTORY == 0 || a & ATTR_REPARSE != 0 {
        return Err(tr!("cleanup.err.is_link"));
    }
    if !final_path(h.0).is_some_and(|p| p.eq_ignore_ascii_case(root)) {
        return Err(tr!("cleanup.err.via_link"));
    }
    Ok(Some(h))
}

// ---------------- tóm tắt cho AI ----------------

fn summary(items: &[ItemOut]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(SUMMARY_MAX + 64);
    // cache nhỏ không hiện trên giao diện -> cũng không gửi cho AI
    let items: Vec<&ItemOut> = items.iter().filter(|x| x.group != "cache" || x.pending || x.bytes >= CACHE_MIN).collect();
    let total: u64 = items.iter().filter(|x| x.error.is_none()).map(|x| x.bytes).sum();
    let _ = writeln!(
        s,
        "Kết quả quét của trang Dọn dẹp: {} mục, tổng {}. Mỗi dòng: [mã] tên · dung lượng · số tệp · phần mới < 24 giờ · an toàn/cân nhắc · chọn sẵn hay không; dòng dưới là đường dẫn. Xoá là xoá vĩnh viễn nội dung bên trong, tự bỏ qua tệp đang dùng.",
        items.len(),
        du::sz(total)
    );
    let mut sorted = items;
    sorted.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    let mut left = 0;
    for it in sorted {
        let mut line = if it.pending {
            format!("[{}] {} · đang đo, chưa rõ dung lượng", it.id, it.name)
        } else {
            format!("[{}] {} · {} · {} tệp", it.id, it.name, du::sz(it.bytes), it.files)
        };
        if it.recent_bytes > 0 {
            let _ = write!(line, " · mới {}", du::sz(it.recent_bytes));
        }
        line.push_str(if it.safety == "safe" { " · an toàn" } else { " · cân nhắc" });
        if it.default_on {
            line.push_str(" · chọn sẵn");
        }
        if it.newest > 0 {
            let _ = write!(line, " · sửa gần nhất {}", du::ym((it.newest / 1000).clamp(0, i64::from(u32::MAX)) as u32));
        }
        if let Some(e) = &it.error {
            let _ = write!(line, " · lỗi: {e}");
        }
        line.push('\n');
        if !it.paths.is_empty() {
            let more = it.paths.len().saturating_sub(2);
            let _ = write!(line, "  {}", it.paths.iter().take(2).cloned().collect::<Vec<_>>().join("; "));
            if more > 0 {
                let _ = write!(line, " (+{more})");
            }
            line.push('\n');
        }
        if s.len() + line.len() > SUMMARY_MAX {
            left += 1;
            continue;
        }
        s.push_str(&line);
    }
    if left > 0 {
        let _ = writeln!(s, "… và {left} mục nhỏ hơn");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn bench_clean(path: &str) -> (u64, u64) {
        let c = clean_one(path, i64::MAX);
        (c.removed, c.skipped)
    }

    fn clean_one(path: &str, cutoff: i64) -> Counts {
        enable_privileges();
        let tally = Tally { files: AtomicU64::new(0), bytes: AtomicU64::new(0), item: AtomicUsize::new(0) };
        let (c, e) = Deleter::run(1, &[(0, path.to_owned())], cutoff, &tally);
        assert_eq!(tally.files.load(Ordering::Relaxed), c[0].removed + c[0].skipped + c[0].kept_recent, "bộ đếm tiến độ khớp số liệu");
        assert!(e[0].is_none(), "{:?}", e[0]);
        c[0]
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fomeg-clean-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn clean_root_deletes_inside_skips_locked_links_and_keeps_root() {
        let root = tmp("root");
        let outside = tmp("outside");
        fs::write(outside.join("precious.txt"), b"keep me").unwrap();
        fs::write(root.join("a.tmp"), b"1234").unwrap();
        fs::write(root.join("ro.txt"), b"12").unwrap();
        let mut p = fs::metadata(root.join("ro.txt")).unwrap().permissions();
        p.set_readonly(true);
        fs::set_permissions(root.join("ro.txt"), p).unwrap();
        fs::create_dir_all(root.join("sub").join("deep")).unwrap();
        fs::write(root.join("sub").join("deep").join("b.log"), b"123456").unwrap();
        fs::write(root.join("locked.bin"), b"x").unwrap();
        let locked = fs::File::open(root.join("locked.bin")).unwrap();
        // junction trỏ ra ngoài: phải bị bỏ qua, nội dung đích còn nguyên
        let ok = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(root.join("link"))
            .arg(&outside)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let c = clean_one(root.to_str().unwrap(), i64::MAX);
        assert!(root.is_dir(), "giữ lại gốc");
        assert!(!root.join("a.tmp").exists());
        assert!(!root.join("ro.txt").exists(), "xoá cả tệp chỉ đọc");
        assert!(!root.join("sub").exists(), "xoá thư mục con đã rỗng");
        assert!(root.join("locked.bin").exists(), "tệp đang mở được bỏ qua");
        assert!(outside.join("precious.txt").exists(), "không đi theo junction");
        assert_eq!(c.removed, 3);
        assert_eq!(c.bytes, 12);
        assert!(c.skipped > u64::from(ok));
        if ok {
            assert!(root.join("link").exists(), "không xoá chính junction");
        }
        drop(locked);
        let _ = fs::remove_dir(root.join("link"));
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    /// Đo tốc độ xoá: 20 000 tệp trong 400 thư mục. Chạy riêng: cargo test --release --lib bench_delete -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_delete() {
        let root = tmp("bench");
        for d in 0..400 {
            let dir = root.join(format!("d{d}")).join("sub");
            fs::create_dir_all(&dir).unwrap();
            for f in 0..50 {
                fs::write(dir.join(format!("f{f}.tmp")), b"0123456789").unwrap();
            }
        }
        let t = Instant::now();
        let c = bench_clean(root.to_str().unwrap());
        println!("xoá {} tệp/thư mục trong {} ms (bỏ qua {})", c.0, t.elapsed().as_millis(), c.1);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn keep_recent_keeps_new_files() {
        let root = tmp("recent");
        fs::write(root.join("new.tmp"), b"abc").unwrap();
        let c = clean_one(root.to_str().unwrap(), now_ms() - RECENT_MS);
        assert!(root.join("new.tmp").exists());
        assert_eq!((c.removed, c.kept_recent), (0, 1));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn labels_group_by_app() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(app_label(&v(&["Google", "Chrome", "User Data", "Default", "Cache"])).0, "Google Chrome");
        assert_eq!(app_label(&v(&["Code", "CachedData"])).0, "VS Code");
        assert_eq!(app_label(&v(&["Discord", "Cache"])).0, "Discord");
        assert_eq!(app_label(&v(&["Packages", "Microsoft.WindowsTerminal_8wekyb3d8bbwe", "AC", "INetCache"])).0, "Microsoft.WindowsTerminal");
        assert_eq!(app_label(&v(&["JetBrains", "IntelliJIdea2024.1", "caches"])).0, "JetBrains IntelliJIdea2024.1");
        assert_eq!(app_label(&v(&["Some Vendor", "Some App", "Cache"])).0, "Some Vendor");
        assert_eq!(app_label(&v(&["Microsoft", "OneDrive", "cache"])).0, "Microsoft OneDrive");
        assert!(is_cache(&v(&[".nuget"]), "packages"));
        assert!(!is_cache(&v(&["x"]), "packages"));
    }

    /// Quét thật trên máy này (chỉ đọc): in số liệu và thời gian.
    #[test]
    fn real_scan_is_fast_and_reads_only() {
        let t = Instant::now();
        let n = discover(&Folders::get(), crate::is_elevated()).len();
        println!("tìm {n} mục trong {} ms", t.elapsed().as_millis());
        let t = Instant::now();
        let c = Cleaner::default();
        let (tx, rx) = mpsc::channel::<String>();
        let first = c.scan(move |_, it| { let _ = tx.send(it.id.clone()); }).unwrap();
        let ms = t.elapsed().as_millis();
        let pending = first.items.iter().filter(|x| x.pending).count();
        println!("trả kết quả sau {ms} ms: {} mục, {pending} mục đang đo", first.items.len());
        for _ in 0..pending {
            let id = rx.recv_timeout(Duration::from_secs(300)).expect("đo quá lâu");
            println!("  xong {id} sau {} ms", t.elapsed().as_millis());
        }
        let items = c.plan.lock().unwrap().as_ref().unwrap().items.clone();
        assert!(items.iter().all(|x| !x.pending));
        let items: Vec<ItemOut> = items.into_iter().filter(|x| x.group != "cache" || x.bytes >= CACHE_MIN).collect();
        for it in &items {
            println!("{:>8} {:>7} tệp  {:<28} {} ({} đường dẫn){}", du::sz(it.bytes), it.files, it.id, it.name, it.paths.len(), it.error.as_deref().map(|e| format!(" – {e}")).unwrap_or_default());
        }
        let s = summary(&items);
        println!("quét {} mục trong {ms} ms · tóm tắt AI {} ký tự", items.len(), s.len());
        assert!(items.iter().any(|x| x.id == "user_temp"));
        assert!(s.len() <= SUMMARY_MAX + 64);
    }
}

