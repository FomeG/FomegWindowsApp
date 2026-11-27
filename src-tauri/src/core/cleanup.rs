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
use std::sync::{mpsc, Arc, Mutex, OnceLock, RwLock};
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
    fn GetLongPathNameW(short: *const u16, long: *mut u16, len: u32) -> u32;
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
    // Service Worker của Chromium / Electron (Discord, Teams, trình duyệt…): bản lưu offline của trang web, tự tải lại
    "cachestorage",
    "scriptcache",
];

/// Thư mục log của ứng dụng: chỉ nhận khi MỌI tệp ở cấp đầu đúng là tệp log (`is_log_file`), để không bao giờ dọn nhầm
/// một thư mục dữ liệu tình cờ tên "log".
const LOG_NAMES: &[&str] = &["logs", "log"];
/// Báo cáo lỗi / bản ghi khi ứng dụng bị treo (Crashpad của Chromium / Electron, Crash Reports của Firefox…).
const CRASH_NAMES: &[&str] = &["crashpad", "crash reports", "crashreports", "crashes", "crashdumps", "crash dumps"];

/// Tệp log: tên có ".log" (vd. main.log.1) hoặc đuôi nhật ký / bản nén nhật ký thường gặp.
fn is_log_file(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains(".log") || [".txt", ".etl", ".gz", ".zip", ".old", ".bak", ".jsonl", ".dmp", ".trace"].iter().any(|e| n.ends_with(e))
}

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
    (&["microsoft", "onedrive"], "OneDrive", "app"),
    (&["discord"], "Discord", "app"),
    (&["slack"], "Slack", "app"),
    (&["spotify"], "Spotify", "app"),
    (&["telegram desktop"], "Telegram", "app"),
    (&["zalo"], "Zalo", "app"),
    (&["zalopc"], "Zalo", "app"),
    (&["steam"], "Steam", "app"),
    (&["obs-studio"], "OBS Studio", "app"),
    (&["notion"], "Notion", "app"),
    (&["figma"], "Figma", "app"),
    (&["postman"], "Postman", "app"),
    (&["claude"], "Claude", "app"),
    (&["anthropicclaude"], "Claude", "app"),
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
    (&["node-gyp"], "node-gyp", "dev"),
    (&["ms-playwright"], "Playwright", "dev"),
    (&["electron"], "Electron", "dev"),
    (&["electron-builder"], "electron-builder", "dev"),
    (&["numba"], "Numba", "dev"),
    (&["pub"], "Dart pub", "dev"),
    (&["pnpm"], "pnpm", "dev"),
    (&["pnpm-cache"], "pnpm", "dev"),
    (&["deno"], "Deno", "dev"),
    (&["composer"], "Composer", "dev"),
    (&["pypoetry"], "Poetry", "dev"),
    (&["huggingface"], "Hugging Face", "dev"),
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
    /// nhóm cache của ứng dụng: gồm những gì (cache | log | crash), theo thứ tự đó
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<&'static str>,
    /// tệp lấy icon của ứng dụng đã cài (exe / ico), "" = không có -> giao diện dùng icon thư mục
    #[serde(skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// từng thư mục gốc, CÙNG THỨ TỰ với `paths`: loại + số liệu riêng (dọn từng phần: `cleanup_apply` nhận chỉ số)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartOut>,
}

#[derive(Clone, Copy, Default, Serialize)]
pub struct PartOut {
    /// cache | log | crash ("" với mục cố định)
    pub kind: &'static str,
    pub bytes: u64,
    pub files: u64,
    pub recent_bytes: u64,
    pub recent_files: u64,
}

/// Nhóm của thư mục tìm được: ứng dụng nhận ra được | công cụ lập trình / thư mục ẩn | thư mục không rõ của app nào.
pub(crate) fn is_found_group(g: &str) -> bool {
    matches!(g, "cache" | "dev" | "other")
}

#[derive(Serialize)]
pub struct ScanOut {
    pub id: u64,
    pub items: Vec<ItemOut>,
    pub took_ms: f64,
    pub admin: bool,
    pub system_drive: Option<String>,
}

#[derive(Default, Serialize)]
pub struct ItemResult {
    pub id: String,
    pub removed: u64,
    /// tệp/thư mục bỏ qua: đang được dùng, không có quyền hoặc là liên kết
    pub skipped: u64,
    /// dung lượng các tệp mở/xoá thất bại; không tính thư mục và liên kết
    pub skipped_bytes: u64,
    /// phần của skipped_bytes sửa trong 24 giờ qua, cùng mốc với lần đo lại
    pub skipped_recent_bytes: u64,
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
        let folders = Folders::get();
        let system_drive = folders.windows.as_deref().and_then(|path| path.get(..2)).filter(|prefix| prefix.ends_with(':')).map(str::to_ascii_uppercase);
        let mut items = discover(&folders, admin);
        for it in items.iter_mut() {
            it.pending = true;
        }
        let (m, rx) = Measure::start(&items, now_ms() - RECENT_MS);
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
                    if !it.pending {
                        continue; // mục đã được đo lại sau khi dọn
                    }
                    m.fill(i as usize, it);
                    let out = it.clone();
                    drop(g);
                    emit(id, &out);
                }
            });
        }
        Ok(ScanOut { id, items, took_ms: t0.elapsed().as_secs_f64() * 1000.0, admin, system_drive })
    }

    fn stop_measure(&self) {
        if let Some(m) = self.measure.lock().ok().and_then(|mut g| g.take()) {
            m.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Dọn các mục `ids` của lần quét `id`, rồi đo lại CHỈ các mục đó (không quét lại cả máy).
    /// `parts`: mục chỉ dọn một phần -> chỉ số các thư mục gốc (trong `paths` của lần quét) được chọn; mục không có ở đây
    /// thì dọn hết. Chỉ số ngoài phạm vi bị bỏ qua: backend không bao giờ nhận đường dẫn từ giao diện.
    /// `progress` được gọi ~4 lần/giây trong lúc dọn (từ một luồng riêng, chỉ đọc vài bộ đếm nguyên tử) và một lần khi
    /// chuyển sang đo lại.
    pub fn apply(&self, id: u64, ids: &[String], parts: &HashMap<String, Vec<usize>>, keep_recent: bool, progress: impl Fn(&CleanProgress) + Sync) -> Result<ApplyOut, String> {
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
        // Tạm dừng lượt đo nền. Các mục khác tiếp tục đo từ chỗ cũ sau khi dọn.
        let measure = self.measure.lock().ok().and_then(|g| g.clone());
        let _paused = measure.as_ref().and_then(|m| m.io.write().ok());
        enable_privileges();
        let recent_after = now_ms() - RECENT_MS;
        // thư mục gốc được chọn của từng mục (mục chọn một phần: chỉ các chỉ số hợp lệ)
        let chosen: Vec<Vec<usize>> = targets
            .iter()
            .map(|t| match parts.get(&t.id) {
                Some(sel) => {
                    let mut v: Vec<usize> = sel.iter().copied().filter(|&k| k < t.paths.len()).collect();
                    v.sort_unstable();
                    v.dedup();
                    v
                }
                None => (0..t.paths.len()).collect(),
            })
            .collect();
        if targets.iter().zip(&chosen).all(|(t, c)| t.id != RECYCLE_ID && c.is_empty()) {
            return Err(tr!("cleanup.err.none_selected"));
        }
        let roots: Vec<(usize, String)> = targets
            .iter()
            .enumerate()
            .filter(|(_, t)| t.id != RECYCLE_ID)
            .flat_map(|(i, t)| chosen[i].iter().map(move |&k| (i, t.paths[k].clone())))
            .collect();
        let has_recycle = targets.iter().any(|t| t.id == RECYCLE_ID);
        // tổng từ lần quét (Thùng rác không đếm được từng tệp -> không tính vào tổng); mục một phần: chỉ phần đã chọn
        let part_sum = |t: &ItemOut, c: &[usize], f: fn(&PartOut) -> u64, whole: u64| {
            if c.len() == t.paths.len() || t.parts.len() != t.paths.len() { whole } else { c.iter().map(|&k| f(&t.parts[k])).sum() }
        };
        let files: Vec<u64> = targets.iter().zip(&chosen).map(|(t, c)| if t.id == RECYCLE_ID { 0 } else { part_sum(t, c, |p| p.files, t.files) }).collect();
        let total_files = files.iter().sum();
        let total_bytes = targets.iter().zip(&chosen).filter(|(t, _)| t.id != RECYCLE_ID).map(|(t, c)| part_sum(t, c, |p| p.bytes, t.bytes)).sum();
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
            let (c, e) = Deleter::run(targets.len(), &roots, recent_after, keep_recent, &tally);
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
                ItemResult {
                    id: t.id.clone(),
                    removed: c.removed,
                    skipped: c.skipped,
                    skipped_bytes: c.skipped_bytes,
                    skipped_recent_bytes: c.skipped_recent_bytes,
                    kept_recent: c.kept_recent,
                    bytes: c.bytes,
                    error: errors[i].clone(),
                }
            })
            .collect();

        // đo lại các mục vừa dọn (phần còn lại thường rất nhỏ) và cập nhật kết quả quét đang giữ
        let (m, rx) = Measure::start(&targets, recent_after);
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
                for (i, x) in p.items.iter_mut().enumerate() {
                    if let Some(t) = targets.iter().find(|t| t.id == x.id) {
                        if let Some(m) = &measure {
                            m.discard(i as u32);
                        }
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
    let hits = Mutex::new(Vec::<Hit>::new());
    // ứng dụng đã cài (tên thật + icon của nhóm cache): đọc song song với lúc tìm; apps::list dùng bản nhớ khi
    // chưa cài / gỡ gì (~0,5 ms), lần đầu ~0,4 s – vẫn xong trước bước tìm cache
    let installed = std::thread::scope(|s| {
        let apps = s.spawn(|| super::apps::list().apps);
        for _ in 0..MEASURE_WORKERS {
            s.spawn(|| {
                let mut buf = vec![0u64; du::BUF_U64];
                let mut lbuf = vec![0u64; 1024];
                // and_then nhận guard theo giá trị -> khoá được trả ngay, các luồng không chờ nhau
                while let Some((path, comps)) = queue.lock().ok().and_then(|mut q| q.pop()) {
                    let found = find_caches(&path, comps, &skip, until, &mut buf, &mut lbuf);
                    if let Ok(mut h) = hits.lock() {
                        h.extend(found);
                    }
                }
            });
        }
        apps.join().unwrap_or_default()
    });
    let matcher = super::leftovers::AppMatcher::new(&installed);

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
    // nhãn -> ứng dụng đã cài (tên, icon, thư mục cài); mỗi nhãn chỉ so một lần
    let mut seen: HashMap<String, Option<(String, String, String)>> = HashMap::new();
    // nhóm -> nơi tìm exe để lấy icon khi app không khai icon (thư mục gốc của app + thư mục cài)
    let mut look: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    for (comps, path, what) in hits.into_inner().unwrap_or_default() {
        let (label, kind) = app_label(&comps);
        // thư mục dữ liệu đặt theo tên exe ("Voicemod.exe", "ollama app.exe"): bỏ đuôi
        let label = match label.len().checked_sub(4) {
            Some(i) if label.is_char_boundary(i) && label[i..].eq_ignore_ascii_case(".exe") && i > 0 => label[..i].to_owned(),
            _ => label,
        };
        let lower: Vec<String> = comps.iter().map(|s| s.to_lowercase()).collect();
        // công cụ lập trình (npm, pip, ~/.cache…) giữ tên riêng; ứng dụng thì lấy tên + icon của app đã cài
        let app = if kind == "dev" {
            None
        } else {
            seen.entry(label.to_lowercase())
                .or_insert_with(|| {
                    let a = if lower[0] == "packages" && comps.len() > 1 {
                        store_app(&installed, &comps[1])
                    } else {
                        let vendor_app = (VENDORS.contains(&lower[0].as_str()) && comps.len() > 2).then(|| comps[1].as_str());
                        let mut names = vec![label.as_str()];
                        names.extend(vendor_app);
                        names.push(&comps[0]);
                        matcher.find(&names)
                    };
                    a.map(|a| (a.name.clone(), a.icon.clone(), a.location.clone()))
                })
                .clone()
        };
        // nhãn đã biết (KNOWN) giữ nguyên ("Google Chrome"); nhãn lấy từ tên thư mục ("discord") thì dùng tên của app
        let known = KNOWN.iter().any(|&(_, l, _)| l == label);
        let name = match &app {
            Some((n, _, _)) if !known => clean_app_name(n),
            _ => label.clone(),
        };
        let id = format!("cache:{}", name.to_lowercase());
        // nhóm: ứng dụng = nhận ra được (app đã cài, trình duyệt / app quen thuộc); công cụ lập trình / thư mục ẩn (.bun,
        // .claude…); còn lại = thư mục không rõ của app nào (để tab Ứng dụng chỉ có ứng dụng thật)
        let group = if kind == "dev" { "dev" } else if app.is_some() || known { "cache" } else { "other" };
        let g = groups.entry(id.clone()).or_insert_with(|| {
            let (note, safety) = if group == "other" { ("cleanup.cache_note.other", "safe") } else { cache_note(kind, &name) };
            row(id.clone(), group, name.clone(), tr!(note), Vec::new(), safety, false, None)
        });
        if group == "cache" && g.group == "other" {
            g.group = "cache";
            g.note = tr!(cache_note(kind, &name).0);
        }
        if g.icon.is_empty() {
            if let Some((_, icon, _)) = &app {
                g.icon.clone_from(icon);
            }
        }
        if !g.kinds.contains(&what) {
            g.kinds.push(what);
        }
        if kind != "dev" && g.icon.is_empty() {
            let (rel, roots) = look.entry(id).or_default();
            if let Some((r, root)) = app_root(&path, &comps, &lower) {
                if !rel.contains(&r) {
                    rel.push(r);
                }
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
            if let Some((_, _, loc)) = &app {
                if !loc.is_empty() && !roots.contains(loc) {
                    roots.push(loc.clone());
                }
            }
        }
        g.paths.push(path);
        g.parts.push(PartOut { kind: what, ..Default::default() });
    }
    // app không khai icon (Discord, Figma, OneDrive…): tìm exe của nó cạnh thư mục dữ liệu, song song, chỉ 1–2 cấp
    let need: Vec<(String, Vec<String>, Vec<String>)> = look
        .into_iter()
        .filter(|(id, _)| groups.get(id).is_some_and(|g| g.icon.is_empty()))
        .map(|(id, (rel, roots))| {
            let keys: Vec<String> = rel.iter().chain(groups.get(&id).map(|g| &g.name)).filter_map(|r| r.rsplit('\\').next()).map(super::leftovers::norm).filter(|k| k.len() >= 3).collect();
            (id, exe_dirs(f, &rel, roots), keys)
        })
        .collect();
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..MEASURE_WORKERS.min(need.len()) {
            s.spawn(|| {
                let mut buf = vec![0u64; 2048];
                while let Some((id, dirs, keys)) = need.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Some(exe) = find_exe(dirs, keys, &mut buf) {
                        if let Ok(mut v) = found.lock() {
                            v.push((id.clone(), exe));
                        }
                    }
                }
            });
        }
    });
    for (id, exe) in found.into_inner().unwrap_or_default() {
        if let Some(g) = groups.get_mut(&id) {
            g.icon = exe;
        }
    }
    let mut caches: Vec<ItemOut> = groups.into_values().collect();
    const ORDER: [&str; 3] = ["cache", "log", "crash"];
    let rank = |k: &str| ORDER.iter().position(|x| *x == k);
    for c in &mut caches {
        // đường dẫn + phần cùng thứ tự: theo loại (cache, log, báo cáo lỗi) rồi theo đường dẫn
        let mut both: Vec<(String, PartOut)> = std::mem::take(&mut c.paths).into_iter().zip(std::mem::take(&mut c.parts)).collect();
        both.sort_by(|a, b| rank(a.1.kind).cmp(&rank(b.1.kind)).then_with(|| a.0.cmp(&b.0)));
        (c.paths, c.parts) = both.into_iter().unzip();
        c.kinds.sort_by_key(|k| rank(k));
    }
    caches.sort_by_cached_key(|c| c.name.to_lowercase());
    items.extend(caches);
    items
}

#[allow(clippy::too_many_arguments)]
fn row(id: String, group: &'static str, name: String, note: String, paths: Vec<String>, safety: &'static str, on: bool, error: Option<String>) -> ItemOut {
    ItemOut { id, group, name, note, paths, bytes: 0, files: 0, recent_bytes: 0, recent_files: 0, newest: 0, safety, default_on: on, pending: false, error, kinds: Vec::new(), icon: String::new(), parts: Vec::new() }
}

/// Thư mục chỉ chứa tệp log ở cấp đầu (thư mục con cho phép: log chia theo ngày). Không đọc được -> không nhận.
fn only_logs(path: &str, buf: &mut [u64]) -> bool {
    let mut ok = true;
    du::list_dir(&nul(path), buf, |e| {
        if e.attrs & ATTR_DIRECTORY == 0 && !is_log_file(&String::from_utf16_lossy(e.name)) {
            ok = false;
        }
    })
    .is_ok()
        && ok
}

/// Thư mục `lower` (cha: `parents`) là loại dọn được nào: cache | crash | log (log còn phải qua `only_logs`).
fn kind_of(parents: &[String], lower: &str) -> Option<&'static str> {
    if is_cache(parents, lower) {
        Some("cache")
    } else if CRASH_NAMES.contains(&lower) {
        Some("crash")
    } else if LOG_NAMES.contains(&lower) && !parents.is_empty() {
        Some("log")
    } else {
        None
    }
}

/// Một thư mục tìm được: (các đoạn đường dẫn, đường dẫn, loại).
type Hit = (Vec<String>, String, &'static str);

/// Tìm thư mục cache / log / báo cáo lỗi trong `path` theo tên (sâu tối đa FIND_DEPTH, dừng khi hết giờ).
/// `lbuf`: bộ đệm nhỏ riêng để xem bên trong thư mục log (bộ đệm chính đang được liệt kê dở).
fn find_caches(path: &str, comps: Vec<String>, skip: &HashSet<String>, until: Instant, buf: &mut [u64], lbuf: &mut [u64]) -> Vec<Hit> {
    let last = comps.last().map(|s| s.to_lowercase()).unwrap_or_default();
    if let Some(k) = kind_of(&comps[..comps.len() - 1], &last) {
        if k != "log" || only_logs(path, lbuf) {
            return vec![(comps, path.to_owned(), k)];
        }
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
            let kind = kind_of(&comps, &lower).filter(|&k| k != "log" || only_logs(&child, lbuf));
            let mut c = comps.clone();
            c.push(name);
            if let Some(k) = kind {
                hits.push((c, child, k));
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
    /// Đo nền giữ khoá đọc trong mỗi thư mục; dọn giữ khoá ghi để tạm dừng việc đo.
    io: RwLock<()>,
    /// việc chia XOAY VÒNG giữa các mục: mỗi mục một ngăn xếp (thư mục, là gốc), hàng `turn` = mục còn việc.
    /// Mục nhỏ xong ngay, không phải chờ sau kho cache hàng triệu tệp.
    queue: Mutex<Rr>,
    outstanding: AtomicUsize,
    left: Vec<AtomicUsize>,
    /// theo mục, rồi theo thư mục gốc (cùng thứ tự với `paths`)
    sizes: Vec<Mutex<Vec<Size>>>,
    errors: Vec<Mutex<Option<String>>>,
    recent_after: i64,
}

/// Hàng việc xoay vòng của `Measure`.
struct Rr {
    turn: VecDeque<u32>,
    /// (thư mục, là gốc, chỉ số thư mục gốc trong `paths`)
    stacks: Vec<Vec<(String, bool, u32)>>,
}

impl Rr {
    /// Một thư mục của mục đến lượt; mục còn việc thì xếp lại cuối hàng.
    fn take(&mut self) -> Option<(u32, String, bool, u32)> {
        let i = self.turn.pop_front()?;
        let (dir, root, part) = self.stacks[i as usize].pop()?;
        if !self.stacks[i as usize].is_empty() {
            self.turn.push_back(i);
        }
        Some((i, dir, root, part))
    }
    fn give(&mut self, i: u32, part: u32, dirs: Vec<String>) {
        let st = &mut self.stacks[i as usize];
        if st.is_empty() {
            self.turn.push_back(i);
        }
        st.extend(dirs.into_iter().map(|d| (d, false, part)));
    }
}

impl Measure {
    fn start(items: &[ItemOut], recent_after: i64) -> (Arc<Measure>, mpsc::Receiver<u32>) {
        let mut rr = Rr { turn: VecDeque::new(), stacks: vec![Vec::new(); items.len()] };
        let mut total = 0;
        for (i, it) in items.iter().enumerate() {
            if it.id != RECYCLE_ID && !it.paths.is_empty() {
                rr.stacks[i] = it.paths.iter().enumerate().map(|(k, p)| (p.clone(), true, k as u32)).collect();
                total += it.paths.len();
                rr.turn.push_back(i as u32);
            }
        }
        let m = Arc::new(Measure {
            cancel: AtomicBool::new(false),
            io: RwLock::new(()),
            outstanding: AtomicUsize::new(total),
            left: items.iter().map(|it| AtomicUsize::new(if it.id == RECYCLE_ID { 0 } else { it.paths.len() })).collect(),
            sizes: items.iter().map(|it| Mutex::new(vec![Size::default(); it.paths.len().max(1)])).collect(),
            errors: items.iter().map(|_| Mutex::new(None)).collect(),
            queue: Mutex::new(rr),
            recent_after,
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

    /// Bỏ phần đo cũ của một mục đã đo lại. Chỉ gọi khi các luồng đo đang tạm dừng.
    fn discard(&self, i: u32) {
        if let Ok(mut q) = self.queue.lock() {
            let n = q.stacks[i as usize].len();
            q.stacks[i as usize].clear();
            q.turn.retain(|&queued| queued != i);
            self.outstanding.fetch_sub(n, Ordering::AcqRel);
        }
    }

    fn work(&self, done: &mpsc::Sender<u32>) {
        let mut buf = vec![0u64; du::BUF_U64];
        loop {
            let Ok(io) = self.io.read() else { return };
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }
            let job = self.queue.lock().ok().and_then(|mut q| q.take());
            let Some((i, dir, root, part)) = job else {
                drop(io);
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
                    q.give(i, part, subs);
                }
            }
            if let Some(slot) = self.sizes[idx].lock().ok().as_deref_mut().and_then(|v| v.get_mut(part as usize)) {
                slot.add(&sz);
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
        let per: Vec<Size> = self.sizes[i].lock().map(|g| g.clone()).unwrap_or_default();
        let mut s = Size::default();
        for x in &per {
            s.add(x);
        }
        // số liệu từng thư mục gốc (mục cố định chưa có `parts`: tạo, loại để trống)
        if it.parts.len() != it.paths.len() {
            it.parts = vec![PartOut::default(); it.paths.len()];
        }
        for (p, x) in it.parts.iter_mut().zip(&per) {
            (p.bytes, p.files, p.recent_bytes, p.recent_files) = (x.bytes, x.files, x.recent_bytes, x.recent_files);
        }
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
    // thư mục ẩn của hồ sơ (.bun, .claude…) và cache của công cụ Node (env-paths: "<tên>-nodejs") = công cụ lập trình
    let kind = if label.starts_with('.') || label.to_lowercase().ends_with("-nodejs") { "dev" } else { "app" };
    (label, kind)
}

/// Tên app đã cài -> tên gọn cho nhóm: bỏ "Uninstall ", phần trong ngoặc, phiên bản và x64 ở cuối
/// ("Postman x64 12.36.0" -> "Postman", "oCam version 550.0" -> "oCam"; "Microsoft 365" giữ nguyên).
fn clean_app_name(n: &str) -> String {
    let n = n.trim();
    let n = n.strip_prefix("Uninstall ").unwrap_or(n);
    let n = n.split(" (").next().unwrap_or(n);
    let mut words: Vec<&str> = n.split_whitespace().collect();
    let versionish = |w: &str| {
        let l = w.to_lowercase();
        let digit = l.chars().any(|c| c.is_ascii_digit());
        (digit && l.contains('.')) || (l.starts_with('v') && l[1..].starts_with(|c: char| c.is_ascii_digit())) || ["x64", "x86", "64-bit", "32-bit", "version", "-"].contains(&l.as_str())
    };
    while words.len() > 1 && words.last().is_some_and(|w| versionish(w)) {
        words.pop();
    }
    let s = words.join(" ");
    if s.is_empty() { n.to_owned() } else { s }
}

/// Ứng dụng Store của thư mục Packages\<Tên>_<mã nhà phát hành>.
fn store_app<'a>(apps: &'a [super::apps::App], family: &str) -> Option<&'a super::apps::App> {
    let pkg = family.split('_').next()?.to_lowercase();
    let prefix = format!("s|{pkg}_");
    apps.iter().find(|a| a.kind == "store" && a.id.to_lowercase().starts_with(&prefix))
}

/// Thư mục gốc của app chứa thư mục tìm được: (đường dẫn tương đối "Discord" / "Microsoft\OneDrive", đường dẫn đầy đủ).
fn app_root(path: &str, comps: &[String], lower: &[String]) -> Option<(String, String)> {
    if lower.first().is_some_and(|s| s == "packages") {
        return None; // app Store có logo riêng
    }
    let known = KNOWN.iter().find(|&&(p, _, _)| lower.len() >= p.len() && p.iter().zip(lower).all(|(a, b)| a == b)).map(|k| k.0.len());
    let n = known.unwrap_or(if VENDORS.contains(&lower[0].as_str()) && comps.len() > 2 { 2 } else { 1 });
    if n == 0 || n >= comps.len() {
        return None;
    }
    // bỏ (comps.len() - n) đoạn cuối của đường dẫn
    let mut root = path;
    for _ in 0..comps.len() - n {
        root = &root[..root.rfind('\\')?];
    }
    Some((comps[..n].join("\\"), root.to_owned()))
}

/// Nơi có thể có exe của app: thư mục gốc tìm được, thư mục cài, và cùng tên trong Local, Roaming, Local\Programs,
/// Program Files (64 / 32-bit).
fn exe_dirs(f: &Folders, rels: &[String], roots: Vec<String>) -> Vec<String> {
    let mut bases: Vec<String> = [f.local.clone(), f.roaming.clone(), f.local.as_ref().map(|l| format!("{l}\\Programs"))].into_iter().flatten().collect();
    bases.extend(["ProgramFiles", "ProgramFiles(x86)"].iter().filter_map(|v| std::env::var(v).ok()));
    let mut out = roots;
    for r in rels {
        // "Microsoft\OneDrive" cũng có thể cài thành "Program Files\Microsoft OneDrive"
        let flat = r.replace('\\', " ");
        for b in &bases {
            out.push(format!("{}\\{r}", b.trim_end_matches('\\')));
            if flat != *r {
                out.push(format!("{}\\{flat}", b.trim_end_matches('\\')));
            }
        }
    }
    let mut seen = HashSet::new();
    out.retain(|d| seen.insert(d.to_lowercase()));
    out
}

/// exe không phải chính ứng dụng (trình cập nhật, gỡ cài đặt, báo lỗi…): không lấy icon từ đó.
const NOT_APP_EXE: &[&str] = &["update", "squirrel", "uninstall", "uninst", "unins000", "crashpad_handler", "elevate", "notification_helper", "createdump", "setup", "updater"];

/// Exe của app trong `dirs` (cấp 1, và cấp 2 trong thư mục kiểu app-1.2.3 / Application / bin / số phiên bản) có tên
/// khớp một trong `keys` (chữ thường, chỉ chữ + số). Tên trùng hẳn được ưu tiên hơn tên bắt đầu bằng khoá.
fn find_exe(dirs: &[String], keys: &[String], buf: &mut [u64]) -> Option<String> {
    fn consider(best: &mut Option<(u8, String)>, keys: &[String], dir: &str, name: &str) {
        let Some(stem) = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")) else { return };
        let s = super::leftovers::norm(stem);
        let sc = if NOT_APP_EXE.iter().any(|x| s.starts_with(&super::leftovers::norm(x))) {
            0
        } else if keys.contains(&s) {
            2
        } else if keys.iter().any(|k| k.len() >= 4 && s.len() >= 4 && (s.starts_with(k.as_str()) || k.starts_with(s.as_str()))) {
            1
        } else {
            0
        };
        if sc > best.as_ref().map_or(0, |b| b.0) {
            *best = Some((sc, format!("{dir}\\{name}")));
        }
    }
    let mut best: Option<(u8, String)> = None;
    for d in dirs {
        let mut subs = Vec::new();
        let _ = du::list_dir(&nul(d), buf, |e| {
            let name = String::from_utf16_lossy(e.name);
            if e.attrs & ATTR_DIRECTORY == 0 {
                consider(&mut best, keys, d, &name);
            } else if e.attrs & ATTR_REPARSE == 0 {
                let l = name.to_lowercase();
                if l.starts_with("app-") || l == "application" || l == "bin" || l == "current" || l.starts_with(|c: char| c.is_ascii_digit()) {
                    subs.push(name);
                }
            }
        });
        // bản mới nhất trước (app-1.0.9 > app-1.0.8)
        subs.sort_unstable_by(|a, b| b.cmp(a));
        for s in subs.iter().take(3) {
            let sd = format!("{d}\\{s}");
            let _ = du::list_dir(&nul(&sd), buf, |e| {
                if e.attrs & ATTR_DIRECTORY == 0 {
                    consider(&mut best, keys, &sd, &String::from_utf16_lossy(e.name));
                }
            });
        }
        if best.as_ref().is_some_and(|b| b.0 == 2) {
            break;
        }
    }
    best.map(|b| b.1)
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

/// Tên đầy đủ của từng thành phần (`C:\Users\NGUYEN~1\…` -> `C:\Users\Nguyen Van A\…`). TEMP / TMP hay chứa tên 8.3;
/// không đổi junction (khác với GetFinalPathNameByHandle) nên vẫn dùng để phát hiện đường dẫn đi qua liên kết.
fn long_path(p: &str) -> String {
    let w: Vec<u16> = p.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetLongPathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return p.to_owned();
    }
    String::from_utf16_lossy(&buf[..n])
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
    skipped_bytes: u64,
    skipped_recent_bytes: u64,
    kept_recent: u64,
    bytes: u64,
}

impl Counts {
    fn add(&mut self, o: &Counts) {
        self.removed += o.removed;
        self.skipped += o.skipped;
        self.skipped_bytes += o.skipped_bytes;
        self.skipped_recent_bytes += o.skipped_recent_bytes;
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
    recent_after: i64,
    keep_recent: bool,
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
    fn run(n_items: usize, roots: &[(usize, String)], recent_after: i64, keep_recent: bool, tally: &Tally) -> (Vec<Counts>, Vec<Option<String>>) {
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
            recent_after,
            keep_recent,
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
                            let node = Arc::new(DirNode { h, parent: Some(parent), pending: AtomicUsize::new(1), remove: !self.keep_recent || mtime < self.recent_after, item });
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
            } else if self.keep_recent && mtime >= self.recent_after {
                c.kept_recent += 1;
            } else {
                // share = 0: tệp đang được mở ở đâu đó -> mở thất bại -> bỏ qua, không hỏi.
                // (Đã thử mở kèm "xoá khi đóng" để bớt một lần gọi: không nhanh hơn – chi phí nằm ở bộ lọc Defender/NTFS.)
                match open_child(node.h.0, &name, DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE, 0, OPT_NON_DIRECTORY_FILE) {
                    Some(f) if delete_open(f.0) => {
                        c.removed += 1;
                        c.bytes += size;
                    }
                    _ => {
                        c.skipped += 1;
                        c.skipped_bytes += size;
                        if mtime >= self.recent_after {
                            c.skipped_recent_bytes += size;
                        }
                    }
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
    // so với tên đầy đủ: gốc lấy từ TEMP có thể là tên 8.3 (RUNNER~1) dù không đi qua liên kết nào
    let want = long_path(root);
    if !final_path(h.0).is_some_and(|p| p.trim_end_matches('\\').eq_ignore_ascii_case(want.trim_end_matches('\\'))) {
        return Err(tr!("cleanup.err.via_link"));
    }
    Ok(Some(h))
}

// ---------------- tóm tắt cho AI ----------------

fn summary(items: &[ItemOut]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(SUMMARY_MAX + 64);
    // cache nhỏ không hiện trên giao diện -> cũng không gửi cho AI
    let items: Vec<&ItemOut> = items.iter().filter(|x| !is_found_group(x.group) || x.pending || x.bytes >= CACHE_MIN).collect();
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
        line.push_str(match it.group {
            "dev" => " · công cụ lập trình / thư mục ẩn",
            "other" => " · không rõ của ứng dụng nào",
            _ => "",
        });
        if !it.kinds.is_empty() {
            let _ = write!(line, " · gồm {}", it.kinds.join("+"));
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
        let c = clean_one(path, false);
        (c.removed, c.skipped)
    }

    fn clean_one(path: &str, keep_recent: bool) -> Counts {
        enable_privileges();
        let tally = Tally { files: AtomicU64::new(0), bytes: AtomicU64::new(0), item: AtomicUsize::new(0) };
        let (c, e) = Deleter::run(1, &[(0, path.to_owned())], now_ms() - RECENT_MS, keep_recent, &tally);
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

        let c = clean_one(root.to_str().unwrap(), false);
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
        let c = clean_one(root.to_str().unwrap(), true);
        assert!(root.join("new.tmp").exists());
        assert_eq!((c.removed, c.kept_recent), (0, 1));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn apply_reports_skipped_file_sizes_and_remeasures_only_selected_items() {
        let root = tmp("apply-bytes");
        let other = root.join("other");
        let selected = root.join("selected");
        fs::create_dir(&other).unwrap();
        fs::create_dir(&selected).unwrap();
        fs::write(other.join("other.tmp"), b"other").unwrap();
        let old = selected.join("old.tmp");
        fs::write(&old, b"ab").unwrap();
        let old_file = fs::OpenOptions::new().write(true).open(&old).unwrap();
        old_file.set_modified(SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60)).unwrap();
        drop(old_file);
        fs::write(selected.join("recent.tmp"), b"abc").unwrap();
        fs::write(selected.join("delete.tmp"), b"12345").unwrap();
        let old_locked = fs::File::open(&old).unwrap();
        let recent_locked = fs::File::open(selected.join("recent.tmp")).unwrap();

        let item = |id: &str, path: &std::path::Path| row(id.into(), "temp", id.into(), String::new(), vec![path.to_str().unwrap().into()], "safe", false, None);
        let mut untouched = item("other", &other);
        untouched.bytes = 999;
        let cleaner = Cleaner::default();
        *cleaner.plan.lock().unwrap() = Some(Plan {
            id: 1,
            created: Instant::now(),
            items: vec![untouched, item("selected", &selected)],
        });
        let out = cleaner.apply(1, &["selected".into()], &HashMap::new(), false, |_| {}).unwrap();
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].id, "selected");
        assert_eq!((out.items[0].bytes, out.items[0].recent_bytes), (5, 3));
        let r = &out.results[0];
        assert_eq!((r.bytes, r.skipped_bytes, r.skipped_recent_bytes), (5, 5, 3));
        assert_eq!(cleaner.plan.lock().unwrap().as_ref().unwrap().items[0].bytes, 999);
        assert!(other.join("other.tmp").is_file());
        drop(recent_locked);
        drop(old_locked);
        fs::remove_dir_all(&root).unwrap();
    }

    /// Dọn MỘT PHẦN: chỉ thư mục gốc đã chọn bị dọn, chỉ số sai bị bỏ qua, mục được đo lại theo từng thư mục.
    #[test]
    fn apply_cleans_only_the_chosen_parts() {
        let root = tmp("apply-parts");
        let (cache, logs) = (root.join("Cache"), root.join("logs"));
        fs::create_dir(&cache).unwrap();
        fs::create_dir(&logs).unwrap();
        fs::write(cache.join("a.bin"), b"1234567").unwrap();
        fs::write(logs.join("main.log"), b"abc").unwrap();
        let mut it = row("cache:app".into(), "cache", "App".into(), String::new(), vec![cache.to_string_lossy().into(), logs.to_string_lossy().into()], "safe", false, None);
        it.parts = vec![PartOut { kind: "cache", ..Default::default() }, PartOut { kind: "log", ..Default::default() }];
        let cleaner = Cleaner::default();
        *cleaner.plan.lock().unwrap() = Some(Plan { id: 1, created: Instant::now(), items: vec![it] });
        let parts: HashMap<String, Vec<usize>> = [("cache:app".to_string(), vec![1, 7])].into();
        let out = cleaner.apply(1, &["cache:app".into()], &parts, false, |_| {}).unwrap();
        assert!(cache.join("a.bin").is_file(), "cache không được chọn -> giữ");
        assert!(!logs.join("main.log").exists(), "log đã chọn -> dọn");
        assert_eq!(out.results[0].bytes, 3);
        let p = &out.items[0].parts;
        assert_eq!((p[0].kind, p[0].bytes, p[1].kind, p[1].bytes), ("cache", 7, "log", 0), "đo lại theo từng thư mục");
        assert_eq!(out.items[0].bytes, 7);
        let none: HashMap<String, Vec<usize>> = [("cache:app".to_string(), vec![9])].into();
        assert!(cleaner.apply(1, &["cache:app".into()], &none, false, |_| {}).is_err(), "không chỉ số hợp lệ -> không dọn gì");
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
        assert_eq!(app_label(&v(&["Microsoft", "OneDrive", "cache"])).0, "OneDrive");
        assert_eq!(app_label(&v(&["Microsoft", "Some Tool", "cache"])).0, "Microsoft Some Tool");
        assert!(is_cache(&v(&[".nuget"]), "packages"));
        assert!(!is_cache(&v(&["x"]), "packages"));
        // log / báo cáo lỗi / cache Service Worker
        assert_eq!(kind_of(&v(&["discord"]), "logs"), Some("log"));
        assert_eq!(kind_of(&[], "logs"), None, "log ở thẳng AppData: không biết của app nào");
        assert_eq!(kind_of(&v(&["BraveSoftware", "Brave-Browser", "User Data"]), "crashpad"), Some("crash"));
        assert_eq!(kind_of(&v(&["discord", "Service Worker"]), "cachestorage"), Some("cache"));
        assert_eq!(kind_of(&v(&["x"]), "settings"), None);
        assert!(is_log_file("main.log") && is_log_file("renderer.log.1") && is_log_file("trace.etl") && is_log_file("old.txt"));
        assert!(!is_log_file("data.db") && !is_log_file("config.json") && !is_log_file("Local State"));
    }

    /// `cargo test --release --lib cleanup::tests::list_app_groups -- --ignored --nocapture`: các nhóm cache của app trên
    /// máy này (tên, có icon không, gồm những gì, bao nhiêu thư mục) – để xem nhận diện có đúng không.
    #[test]
    #[ignore = "in danh sách trên máy thật"]
    fn list_app_groups() {
        let t = Instant::now();
        let items = discover(&Folders::get(), crate::is_elevated());
        let apps: Vec<&ItemOut> = items.iter().filter(|x| is_found_group(x.group)).collect();
        let icons = apps.iter().filter(|x| !x.icon.is_empty()).count();
        println!("{} nhóm, {icons} có icon, {} ms", apps.len(), t.elapsed().as_millis());
        for a in apps {
            println!("  {:<6} {:<34} {:<16} {:>3} thư mục  {}", a.group, a.name, a.kinds.join("+"), a.paths.len(), if a.icon.is_empty() { "" } else { "[icon]" });
        }
    }

    #[test]
    fn app_names_are_cleaned_and_roots_found() {
        assert_eq!(clean_app_name("Postman x64 12.36.0"), "Postman");
        assert_eq!(clean_app_name("oCam version 550.0"), "oCam");
        assert_eq!(clean_app_name("Uninstall Lunar Client"), "Lunar Client");
        assert_eq!(clean_app_name("PowerToys (Preview) x64"), "PowerToys");
        assert_eq!(clean_app_name("Microsoft 365"), "Microsoft 365", "số không có dấu chấm không phải phiên bản");
        assert_eq!(clean_app_name("Unity 2022.3.62f2"), "Unity");
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let low = |c: &[String]| c.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
        let c = v(&["discord", "Cache", "Cache_Data"]);
        assert_eq!(app_root(r"C:\U\AppData\Roaming\discord\Cache\Cache_Data", &c, &low(&c)), Some(("discord".into(), r"C:\U\AppData\Roaming\discord".into())));
        let c = v(&["Microsoft", "OneDrive", "logs"]);
        assert_eq!(app_root(r"C:\U\Local\Microsoft\OneDrive\logs", &c, &low(&c)), Some((r"Microsoft\OneDrive".into(), r"C:\U\Local\Microsoft\OneDrive".into())));
        let c = v(&["Packages", "Microsoft.WindowsStore_8wekyb3d8bbwe", "AC", "INetCache"]);
        assert_eq!(app_root("x", &c, &low(&c)), None, "app Store dùng logo riêng");
    }

    /// exe của app: tên khớp được ưu tiên, bỏ qua Update.exe / trình gỡ, tìm cả trong app-1.2.3.
    #[test]
    fn finds_app_exe_next_to_its_data() {
        let root = std::env::temp_dir().join(format!("fomeg_exe_{}", std::process::id()));
        let app = root.join("Discord");
        fs::create_dir_all(app.join("app-1.0.9")).unwrap();
        for f in ["Update.exe", "unins000.exe"] {
            fs::write(app.join(f), b"x").unwrap();
        }
        fs::write(app.join("app-1.0.9").join("Discord.exe"), b"x").unwrap();
        let mut buf = vec![0u64; 2048];
        let got = find_exe(&[app.to_string_lossy().into_owned()], &["discord".into()], &mut buf).unwrap();
        assert!(got.ends_with(r"app-1.0.9\Discord.exe"), "{got}");
        assert!(find_exe(&[app.to_string_lossy().into_owned()], &["slack".into()], &mut buf).is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    /// Thư mục tên "logs" chỉ được nhận khi mọi tệp ở cấp đầu là tệp log.
    #[test]
    fn log_folders_need_only_log_files() {
        let root = std::env::temp_dir().join(format!("fomeg_logs_{}", std::process::id()));
        let (good, bad) = (root.join("App").join("logs"), root.join("Other").join("log"));
        fs::create_dir_all(good.join("2026-11-27")).unwrap();
        fs::create_dir_all(&bad).unwrap();
        fs::write(good.join("main.log"), b"x").unwrap();
        fs::write(good.join("main.log.1"), b"x").unwrap();
        fs::write(bad.join("main.log"), b"x").unwrap();
        fs::write(bad.join("state.db"), b"x").unwrap();
        let mut buf = vec![0u64; 1024];
        assert!(only_logs(&good.to_string_lossy(), &mut buf));
        assert!(!only_logs(&bad.to_string_lossy(), &mut buf), "có tệp dữ liệu -> không dọn");
        let (mut b1, mut b2) = (vec![0u64; du::BUF_U64], vec![0u64; 1024]);
        let hits = find_caches(&root.to_string_lossy(), vec!["fomeg_logs".into()], &HashSet::new(), Instant::now() + Duration::from_secs(5), &mut b1, &mut b2);
        assert_eq!(hits.len(), 1, "chỉ App\\logs: {hits:?}");
        assert_eq!(hits[0].2, "log");
        fs::remove_dir_all(&root).unwrap();
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
        let items: Vec<ItemOut> = items.into_iter().filter(|x| !is_found_group(x.group) || x.bytes >= CACHE_MIN).collect();
        for it in &items {
            println!("{:>8} {:>7} tệp  {:<28} {} ({} đường dẫn){}", du::sz(it.bytes), it.files, it.id, it.name, it.paths.len(), it.error.as_deref().map(|e| format!(" – {e}")).unwrap_or_default());
        }
        let s = summary(&items);
        println!("quét {} mục trong {ms} ms · tóm tắt AI {} ký tự", items.len(), s.len());
        assert!(items.iter().any(|x| x.id == "user_temp"));
        assert!(s.len() <= SUMMARY_MAX + 64);
    }
}

