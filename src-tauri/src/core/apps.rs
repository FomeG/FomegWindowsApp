//! Ứng dụng đã cài (giống Cài đặt → Ứng dụng → Ứng dụng đã cài của Windows).
//!
//! - App thường: khoá `Uninstall` của registry (máy 64-bit, máy 32-bit, người dùng), lọc như Windows: bỏ thành phần hệ
//!   thống (`SystemComponent`), bản cập nhật (`ParentKeyName`, `ReleaseType`) và mục không có lệnh gỡ. ~10–30 ms.
//! - App Store (MSIX): `PackageManager` của người dùng hiện tại, chỉ gói chính (không framework / tài nguyên).
//! - Gỡ cài đặt = chạy TRÌNH GỠ CỦA CHÍNH APP (như Windows) trong một Job Object: mọi tiến trình con (NSIS/Inno chép
//!   trình gỡ ra Temp rồi chạy lại) tự vào job, nên biết chắc khi nào gỡ xong mà không phải đoán. Xong thì đọc lại khoá
//!   registry để biết app đã thật sự bị gỡ hay người dùng bấm Huỷ. App Store: `RemovePackageAsync`.
//! - Dung lượng thật: đo thư mục cài đặt bằng `du::list_dir` (một lệnh mỗi thư mục, không mở từng tệp) trên ≤ 6 luồng,
//!   có hạn thời gian, ghi nhớ 10 phút. `EstimatedSize` trình cài tự khai thường thiếu hoặc sai.
//!
//! Không có gì ở đây tự chạy nền: chỉ khi trang Ứng dụng mở / tool của trợ lý hỏi.
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use crate::tr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
use winreg::RegKey;

use super::{du, util};

pub(crate) const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
/// (mã trong id, hive, cờ view, phạm vi, kiến trúc)
pub(crate) const ROOTS: [(&str, winreg::HKEY, u32, &str, &str); 3] = [
    ("m64", HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, "machine", "64-bit"),
    ("m32", HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY, "machine", "32-bit"),
    ("u", HKEY_CURRENT_USER, 0, "user", ""),
];
const SIZE_TTL: Duration = Duration::from_secs(10 * 60);
const MEASURE_WORKERS: usize = 6;
/// Hạn đo một thư mục cài đặt (quá hạn -> kết quả "≥", không ghi nhớ).
const MEASURE_ITEM: Duration = Duration::from_secs(20);
/// Gỡ cài đặt lâu nhất được theo dõi (trình gỡ để mở cả buổi thì thôi không chờ).
const UNINSTALL_MAX: Duration = Duration::from_secs(2 * 60 * 60);
const SUMMARY_MAX: usize = 5800;

#[derive(Serialize, Clone, Debug, Default)]
pub struct App {
    /// `w|m64|<khoá>`, `w|m32|<khoá>`, `w|u|<khoá>` hoặc `s|<PackageFullName>`
    pub id: String,
    /// "win32" | "store"
    pub kind: &'static str,
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// ngày cài, ms Unix (0 = không rõ)
    pub installed: i64,
    /// dung lượng trình cài khai báo (EstimatedSize), 0 = không có
    pub est_bytes: u64,
    pub location: String,
    /// thư mục cài suy ra từ vị trí trình gỡ / icon (app không khai báo)
    pub location_guessed: bool,
    /// lần dùng cuối (BAM / Prefetch / UserAssist), ms Unix; 0 = không thấy
    pub last_used: i64,
    /// số lần người dùng mở từ Start / Explorer (UserAssist), 0 = không rõ
    pub launches: u32,
    /// tệp lấy icon (exe/ico/png), "" = không có
    pub icon: String,
    /// "machine" | "user"
    pub scope: &'static str,
    pub arch: &'static str,
    pub can_uninstall: bool,
    pub can_modify: bool,
    pub msi: bool,
    /// gói hệ thống của Windows (không gỡ được)
    pub system: bool,
    /// khoá registry đầy đủ / tên gói đầy đủ
    pub key: String,
    /// lệnh gỡ (để xem / sao chép)
    pub command: String,
    pub url: String,
}

#[derive(Serialize)]
pub struct ListOut {
    pub apps: Vec<App>,
    pub took_ms: u64,
    pub store_error: Option<String>,
    /// đọc được BAM / Prefetch (có quyền Admin) -> "chưa thấy dùng" đáng tin
    pub usage_full: bool,
}

// ---------------- danh sách (có cache) ----------------
//
// Đọc đầy đủ tốn ~0,3–0,5 s (gói Store tra tên / logo, liệt kê exe ~230 thư mục cài). Kết quả được dùng lại khi "dấu
// thay đổi" còn nguyên: thời điểm ghi cuối của 3 khoá Uninstall + kho gói Store của người dùng (cài / gỡ app là đổi
// ngay; đọc 4 khoá < 1 ms), và chưa quá LIST_TTL (bắt cả cập nhật phiên bản tại chỗ, không đổi khoá cha).
// Lần dùng cuối tính lại từ danh sách exe đã nhớ mỗi khi usage.rs có bản đọc mới (≤ 60 s) – không đụng ổ đĩa.
// RAM: vài trăm KB.

const LIST_TTL: Duration = Duration::from_secs(120);
const STORE_REPO: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";

struct Cached {
    stamp: Vec<i64>,
    at: Instant,
    apps: Vec<App>,
    /// tên exe trong thư mục cài của từng app (cùng thứ tự `apps`)
    exes: Vec<Vec<String>>,
    store_error: Option<String>,
    /// bản usage đã áp (địa chỉ Arc), khác -> tính lại lần dùng cuối
    usage: usize,
}

static LIST: Mutex<Option<Cached>> = Mutex::new(None);
static GEN: AtomicUsize = AtomicUsize::new(0);

fn stamp() -> Vec<i64> {
    let mut v: Vec<i64> = ROOTS.iter().map(|(_, hive, flags, ..)| open_root(*hive, *flags).map(|k| key_time(&k)).unwrap_or(0)).collect();
    v.push(RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(STORE_REPO, KEY_READ).map(|k| key_time(&k)).unwrap_or(0));
    v
}

/// Bỏ cache (vừa gỡ app / người dùng bấm Làm mới).
pub fn invalidate() {
    if let Ok(mut g) = LIST.lock() {
        *g = None;
    }
}

/// Đếm lần đọc lại đầy đủ – leftovers.rs dùng để biết khi nào phải dựng lại dữ liệu phụ thuộc danh sách.
pub fn generation() -> u64 {
    GEN.load(Ordering::Relaxed) as u64
}

/// Toàn bộ danh sách (app thường + Store), dùng cache nếu chưa có gì đổi.
pub fn list() -> ListOut {
    list_with(false)
}

/// `force` = đọc lại hết (nút Làm mới).
pub fn list_with(force: bool) -> ListOut {
    let t0 = Instant::now();
    let st = stamp();
    let u = super::usage::get();
    // giữ khoá trong lúc đọc lại: lời gọi cùng lúc (trang + trợ lý) chờ rồi dùng chung kết quả, không đọc hai lần
    let mut g = LIST.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = !force && g.as_ref().is_some_and(|c| c.stamp == st && c.at.elapsed() < LIST_TTL);
    if !fresh {
        let mut apps = win32_apps();
        let store_error = store_apps(&mut apps).err();
        let exes = exe_lists(&apps);
        GEN.fetch_add(1, Ordering::Relaxed);
        *g = Some(Cached { stamp: st, at: Instant::now(), apps, exes, store_error, usage: 0 });
    }
    let c = g.as_mut().expect("vừa gán");
    let uid = std::sync::Arc::as_ptr(&u) as usize;
    if c.usage != uid {
        apply_usage(&mut c.apps, &c.exes, &u);
        c.usage = uid;
    }
    ListOut { apps: c.apps.clone(), took_ms: t0.elapsed().as_millis() as u64, store_error: c.store_error.clone(), usage_full: u.full }
}

/// Một app trong danh sách đã nhớ (không đọc lại), nếu có.
pub fn cached(id: &str) -> Option<App> {
    LIST.lock().ok()?.as_ref()?.apps.iter().find(|a| a.id == id).cloned()
}

/// Tên exe trong thư mục cài của mỗi app (liệt kê cấp 1–2, chia 6 luồng) – để khớp Prefetch.
fn exe_lists(apps: &[App]) -> Vec<Vec<String>> {
    let mut out = vec![Vec::new(); apps.len()];
    let per = apps.len().div_ceil(MEASURE_WORKERS).max(1);
    std::thread::scope(|sc| {
        for (chunk, slot) in apps.chunks(per).zip(out.chunks_mut(per)) {
            sc.spawn(move || {
                let mut buf = vec![0u64; du::BUF_U64];
                for (a, s) in chunk.iter().zip(slot.iter_mut()) {
                    if a.kind == "win32" && !a.location.is_empty() && std::path::Path::new(&a.location).is_dir() {
                        *s = super::usage::exe_names(&a.location, &mut buf);
                    }
                }
            });
        }
    });
    out
}

/// Lần dùng cuối từ bản usage hiện tại (chỉ so chuỗi trong bộ nhớ, ~1 ms cho vài trăm app).
fn apply_usage(apps: &mut [App], exes: &[Vec<String>], u: &super::usage::Usage) {
    for (a, names) in apps.iter_mut().zip(exes) {
        let hit = if a.kind == "store" {
            super::leftovers::family_of(&a.key).and_then(|f| u.family(&f))
        } else if !a.location.is_empty() {
            u.in_dir(&a.location, names)
        } else if a.icon.to_lowercase().ends_with(".exe") {
            u.exe(&a.icon)
        } else {
            None
        };
        let (t, n) = hit.unwrap_or((0, 0));
        a.last_used = t;
        a.launches = n;
    }
}

// ---------------- app thường (registry) ----------------

fn open_root(hive: winreg::HKEY, flags: u32) -> Option<RegKey> {
    RegKey::predef(hive).open_subkey_with_flags(UNINSTALL, KEY_READ | flags).ok()
}

fn s(k: &RegKey, name: &str) -> String {
    k.get_value::<String, _>(name).map(|v| v.trim().trim_matches('\0').to_owned()).unwrap_or_default()
}
fn dw(k: &RegKey, name: &str) -> Option<u32> {
    k.get_value::<u32, _>(name).ok()
}

fn is_guid(s: &str) -> bool {
    s.len() == 38 && s.starts_with('{') && s.ends_with('}')
}

/// "C:\a\app.exe,0" / "\"C:\a\app.exe\"" -> C:\a\app.exe
fn icon_path(raw: &str) -> String {
    let mut p = raw.trim().to_owned();
    if let Some(i) = p.rfind(',') {
        let tail = p[i + 1..].trim();
        if !tail.is_empty() && tail.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) {
            p.truncate(i);
        }
    }
    let p = util::expand_env(p.trim().trim_matches('"'));
    let l = p.to_lowercase();
    if l.ends_with(".exe") || l.ends_with(".ico") || l.ends_with(".dll") { p } else { String::new() }
}

/// Thư mục chứa tệp, nếu là thư mục riêng của app (không phải Windows, Temp, bộ đệm cài đặt, gốc Program Files…).
fn guess_dir(file: &str) -> Option<String> {
    let dir = std::path::Path::new(file).parent()?.to_string_lossy().trim_end_matches('\\').to_owned();
    let l = format!("{}\\", dir.to_lowercase());
    let bad = ["\\windows\\", "\\temp\\", "\\package cache\\", "\\installer\\", "\\installshield installation information\\", "\\common files\\"];
    if dir.len() <= 3 || bad.iter().any(|b| l.contains(b)) || !measurable(&dir) || !std::path::Path::new(&dir).is_dir() {
        return None;
    }
    Some(dir)
}

/// "20240131" -> ms Unix
fn parse_date(s: &str) -> i64 {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    let (y, m, d) = (s[..4].parse().unwrap_or(0), s[4..6].parse().unwrap_or(0), s[6..].parse().unwrap_or(0));
    chrono::NaiveDate::from_ymd_opt(y, m, d)
        .and_then(|d| d.and_hms_opt(12, 0, 0))
        .map(|t| t.and_utc().timestamp_millis())
        .unwrap_or(0)
}

fn key_time(k: &RegKey) -> i64 {
    k.query_info()
        .map(|m| {
            let ft = (u64::from(m.last_write_time.dwHighDateTime) << 32) | u64::from(m.last_write_time.dwLowDateTime);
            (ft / 10_000) as i64 - 11_644_473_600_000
        })
        .unwrap_or(0)
}

/// Lệnh gỡ như Windows chạy: MSI -> `MsiExec.exe /X{GUID}`, còn lại UninstallString.
fn uninstall_command(k: &RegKey, key: &str) -> (String, bool) {
    let raw = s(k, "UninstallString");
    let msi = dw(k, "WindowsInstaller") == Some(1) || raw.to_lowercase().contains("msiexec");
    if msi && is_guid(key) {
        return (format!("MsiExec.exe /X{key}"), true);
    }
    if msi {
        // "MsiExec.exe /I{…}" (mở trình sửa) -> /X (gỡ)
        if let Some(i) = raw.to_lowercase().find("/i{") {
            let mut c = raw.clone();
            c.replace_range(i..i + 2, "/X");
            return (c, true);
        }
    }
    (raw, msi)
}

fn read_win32(code: &str, root: &RegKey, key: &str, scope: &'static str, arch: &'static str) -> Option<App> {
    let k = root.open_subkey_with_flags(key, KEY_READ).ok()?;
    let name = s(&k, "DisplayName");
    if name.is_empty() || dw(&k, "SystemComponent") == Some(1) || !s(&k, "ParentKeyName").is_empty() {
        return None;
    }
    let rt = s(&k, "ReleaseType").to_lowercase();
    if matches!(rt.as_str(), "update" | "hotfix" | "security update" | "update rollup") {
        return None;
    }
    let (command, msi) = uninstall_command(&k, key);
    if command.is_empty() {
        return None;
    }
    let no_modify = dw(&k, "NoModify") == Some(1);
    let modify = !s(&k, "ModifyPath").is_empty() || (msi && is_guid(key));
    let hive = if scope == "user" { "HKEY_CURRENT_USER" } else { "HKEY_LOCAL_MACHINE" };
    let view = if code == "m32" { r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall" } else { UNINSTALL };
    let mut installed = parse_date(&s(&k, "InstallDate"));
    if installed == 0 {
        installed = key_time(&k);
    }
    let mut location = util::expand_env(s(&k, "InstallLocation").trim_matches('"')).trim_end_matches('\\').to_owned();
    if location.len() <= 3 {
        location.clear(); // "C:\" = khai báo sai, đừng đo cả ổ
    }
    let icon = icon_path(&s(&k, "DisplayIcon"));
    // nhiều app (Electron, NSIS) không khai InstallLocation: lấy thư mục chứa trình gỡ, rồi thư mục chứa icon
    let mut guessed = false;
    if location.is_empty() && !msi {
        let un = split_command(&util::expand_env(&command)).0;
        for f in [un.as_str(), icon.as_str()] {
            if let Some(d) = guess_dir(f) {
                location = d;
                guessed = true;
                break;
            }
        }
    }
    Some(App {
        id: format!("w|{code}|{key}"),
        kind: "win32",
        publisher: s(&k, "Publisher"),
        version: s(&k, "DisplayVersion"),
        installed,
        est_bytes: u64::from(dw(&k, "EstimatedSize").unwrap_or(0)) * 1024,
        icon,
        location,
        location_guessed: guessed,
        last_used: 0,
        launches: 0,
        scope,
        arch,
        can_uninstall: dw(&k, "NoRemove") != Some(1),
        can_modify: modify && !no_modify,
        msi,
        system: false,
        key: format!(r"{hive}\{view}\{key}"),
        command,
        url: s(&k, "URLInfoAbout"),
        name,
    })
}

fn win32_apps() -> Vec<App> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (code, hive, flags, scope, arch) in ROOTS {
        let Some(root) = open_root(hive, flags) else { continue };
        for key in root.enum_keys().flatten() {
            if let Some(a) = read_win32(code, &root, &key, scope, arch) {
                // cùng một app ghi ở cả hai view / hai lần: giữ một
                if seen.insert((a.name.to_lowercase(), a.version.clone(), a.scope)) {
                    out.push(a);
                }
            }
        }
    }
    out
}

/// Đọc lại MỘT app theo id (lúc gỡ: không tin dữ liệu UI gửi lên).
fn find_win32(id: &str) -> Option<App> {
    let mut it = id.splitn(3, '|');
    let (Some("w"), Some(code), Some(key)) = (it.next(), it.next(), it.next()) else { return None };
    let (_, hive, flags, scope, arch) = ROOTS.into_iter().find(|r| r.0 == code)?;
    read_win32(code, &open_root(hive, flags)?, key, scope, arch)
}

fn win32_exists(id: &str) -> bool {
    let mut it = id.splitn(3, '|');
    let (Some("w"), Some(code), Some(key)) = (it.next(), it.next(), it.next()) else { return false };
    let Some((_, hive, flags, ..)) = ROOTS.into_iter().find(|r| r.0 == code) else { return false };
    open_root(hive, flags).is_some_and(|r| r.open_subkey_with_flags(key, KEY_READ).is_ok())
}

// ---------------- app Store (MSIX) ----------------

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Logo khai trong manifest (vd. Assets\StoreLogo.png) thường chỉ có bản theo tỉ lệ (StoreLogo.scale-100.png).
fn logo_file(uri: &str) -> String {
    let Some(rest) = uri.strip_prefix("file:///") else { return String::new() };
    let p = percent_decode(rest).replace('/', "\\");
    if std::path::Path::new(&p).is_file() {
        return p;
    }
    let path = std::path::Path::new(&p);
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) else { return String::new() };
    let stem = stem.to_lowercase();
    let mut best: Option<(usize, String)> = None;
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if !name.starts_with(&format!("{stem}.")) || !name.ends_with(".png") {
            continue;
        }
        let rank = ["scale-100", "scale-125", "scale-150", "targetsize-48", "scale-200", "targetsize-32"]
            .iter()
            .position(|k| name.contains(k))
            .unwrap_or(9);
        if best.as_ref().is_none_or(|b| rank < b.0) {
            best = Some((rank, e.path().to_string_lossy().into_owned()));
        }
    }
    best.map(|b| b.1).unwrap_or_default()
}

fn store_app(p: &windows::ApplicationModel::Package) -> Option<App> {
    use windows::ApplicationModel::PackageSignatureKind;
    if p.IsFramework().unwrap_or(true) || p.IsResourcePackage().unwrap_or(true) || p.IsStub().unwrap_or(false) {
        return None;
    }
    let name = p.DisplayName().map(|h| h.to_string()).unwrap_or_default();
    if name.trim().is_empty() || name.starts_with("ms-resource:") {
        return None;
    }
    let id = p.Id().ok()?;
    let full = id.FullName().map(|h| h.to_string()).unwrap_or_default();
    if full.is_empty() {
        return None;
    }
    let v = id.Version().unwrap_or_default();
    let system = p.SignatureKind().map(|k| k == PackageSignatureKind::System).unwrap_or(false);
    let installed = p.InstalledDate().map(|d| d.UniversalTime / 10_000 - 11_644_473_600_000).unwrap_or(0);
    let logo = p.Logo().ok().and_then(|u| u.AbsoluteUri().ok()).map(|h| logo_file(&h.to_string())).unwrap_or_default();
    Some(App {
        id: format!("s|{full}"),
        kind: "store",
        name: name.trim().to_owned(),
        publisher: p.PublisherDisplayName().map(|h| h.to_string()).unwrap_or_default(),
        version: format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision),
        installed: installed.max(0),
        est_bytes: 0,
        location: p.InstalledPath().map(|h| h.to_string()).unwrap_or_default(),
        location_guessed: false,
        last_used: 0,
        launches: 0,
        icon: logo,
        scope: "user",
        arch: "",
        can_uninstall: !system,
        can_modify: false,
        msi: false,
        system,
        command: format!("Remove-AppxPackage {full}"),
        key: full,
        url: String::new(),
    })
}

fn store_package(full: &str) -> Option<windows::ApplicationModel::Package> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;
    PackageManager::new().and_then(|pm| pm.FindPackageByUserSecurityIdPackageFullName(&HSTRING::new(), &HSTRING::from(full))).ok()
}

fn store_apps(out: &mut Vec<App>) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::{PackageManager, PackageTypes};
    let pm = PackageManager::new().map_err(|e| util::win_err(&e))?;
    use windows::ApplicationModel::PackageSignatureKind;
    let pkgs: Vec<_> = pm
        .FindPackagesByUserSecurityIdWithPackageTypes(&HSTRING::new(), PackageTypes::Main)
        .map_err(|e| util::win_err(&e))?
        .into_iter()
        // gói hệ thống của Windows: không gỡ được, Cài đặt của Windows cũng không liệt kê
        .filter(|p| p.SignatureKind().is_ok_and(|k| k != PackageSignatureKind::System))
        .collect();
    // tên hiển thị / logo phải tra tài nguyên từng gói (~10 ms mỗi gói): chia cho vài luồng
    let per = pkgs.len().div_ceil(MEASURE_WORKERS).max(1);
    std::thread::scope(|sc| {
        let hs: Vec<_> = pkgs.chunks(per).map(|c| sc.spawn(move || c.iter().filter_map(store_app).collect::<Vec<_>>())).collect();
        for h in hs {
            out.extend(h.join().unwrap_or_default());
        }
    });
    Ok(())
}

/// Đọc lại MỘT app theo id (app thường hoặc Store).
pub fn find(id: &str) -> Option<App> {
    match id.strip_prefix("s|") {
        Some(full) => store_app(&store_package(full)?),
        None => find_win32(id),
    }
}

fn store_exists(full: &str) -> bool {
    store_package(full).is_some()
}

// ---------------- dung lượng thật ----------------

/// thư mục (viết thường) -> (bytes, số tệp, lúc đo)
type SizeCache = HashMap<String, (u64, u64, i64, Instant)>;
static SIZES: LazyLock<Mutex<SizeCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Kết quả đo còn mới của thư mục (bytes, số tệp).
pub fn cached_size(path: &str) -> Option<(u64, u64)> {
    let m = SIZES.lock().ok()?;
    m.get(&path.to_lowercase()).filter(|v| v.3.elapsed() < SIZE_TTL).map(|v| (v.0, v.1))
}

/// Kết quả đo còn mới, đủ cả tệp mới nhất (dùng chung cho thư mục liên quan / phần còn sót).
pub(crate) fn cached_walk(path: &str) -> Option<Walked> {
    let m = SIZES.lock().ok()?;
    m.get(&path.to_lowercase()).filter(|v| v.3.elapsed() < SIZE_TTL).map(|v| Walked { bytes: v.0, files: v.1, newest: v.2, done: true })
}

/// Ghi nhớ một lần đo XONG (đo dở thì không nhớ).
pub(crate) fn remember_walk(path: &str, w: &Walked) {
    if w.done {
        if let Ok(mut m) = SIZES.lock() {
            m.insert(path.to_lowercase(), (w.bytes, w.files, w.newest, Instant::now()));
        }
    }
}

/// Bỏ mọi số đo đã nhớ (nút Làm mới).
pub fn clear_sizes() {
    if let Ok(mut m) = SIZES.lock() {
        m.clear();
    }
}

/// Thư mục vừa bị xoá / đổi: bỏ số đo của nó và mọi thư mục cha / con đã nhớ.
pub fn forget_sizes(paths: &[String]) {
    if let Ok(mut m) = SIZES.lock() {
        for p in paths {
            let l = p.trim_end_matches('\\').to_lowercase();
            m.retain(|k, _| !(k == &l || k.starts_with(&format!("{l}\\")) || l.starts_with(&format!("{k}\\"))));
        }
    }
}

/// Đo một thư mục (không theo junction/symlink). Trả (bytes, tệp, đo hết chưa).
/// Kết quả đo một thư mục.
pub(crate) struct Walked {
    pub bytes: u64,
    pub files: u64,
    /// lần sửa mới nhất của một tệp bên trong, ms Unix (0 = thư mục rỗng)
    pub newest: i64,
    /// đo hết (false = hết hạn / bị dừng, số liệu là tối thiểu)
    pub done: bool,
}

pub(crate) fn walk(root: &str, until: Instant, stop: &AtomicBool, buf: &mut [u64]) -> Walked {
    const DIR: u32 = 0x10;
    const REPARSE: u32 = 0x400;
    let (mut bytes, mut files, mut newest) = (0u64, 0u64, 0i64);
    let mut stack = vec![du::long_path(root)];
    let mut n = 0u32;
    while let Some(dir) = stack.pop() {
        n += 1;
        if n.is_multiple_of(64) && (Instant::now() > until || stop.load(Ordering::Relaxed)) {
            return Walked { bytes, files, newest, done: false };
        }
        let mut z = dir.clone();
        z.push(0);
        let _ = du::list_dir(&z, buf, |e| {
            if e.attrs & DIR != 0 {
                if e.attrs & REPARSE == 0 {
                    let mut c = Vec::with_capacity(dir.len() + 1 + e.name.len());
                    c.extend_from_slice(&dir);
                    c.push(b'\\' as u16);
                    c.extend_from_slice(e.name);
                    stack.push(c);
                }
            } else {
                bytes += e.size;
                newest = newest.max(e.mtime);
                files += 1;
            }
        });
    }
    Walked { bytes, files, newest, done: true }
}

/// Thư mục đo được: có thật, không phải gốc ổ / thư mục hệ thống / thư mục chung của nhiều app.
pub(crate) fn measurable(p: &str) -> bool {
    let l = p.trim_end_matches('\\').to_lowercase();
    if l.len() <= 3 || du::protected(p) {
        return false;
    }
    let pf = |k: &str| std::env::var(k).unwrap_or_default().trim_end_matches('\\').to_lowercase();
    let shared = [pf("ProgramFiles"), pf("ProgramFiles(x86)"), pf("ProgramData"), pf("LOCALAPPDATA"), pf("APPDATA"), pf("USERPROFILE"), format!("{}\\programs", pf("LOCALAPPDATA"))];
    !shared.iter().any(|s| !s.is_empty() && l == *s)
}

/// Đo các thư mục (bỏ trùng), lớn trước; `emit(path, bytes, files, done)` sau mỗi thư mục. Dừng khi `stop` hoặc hết `budget`.
pub fn measure(paths: &[String], budget: Duration, stop: &AtomicBool, emit: impl Fn(&str, u64, u64, bool) + Sync) {
    let mut seen = HashSet::new();
    let todo: Vec<&String> = paths.iter().filter(|p| !p.is_empty() && measurable(p) && seen.insert(p.to_lowercase())).collect();
    let until = Instant::now() + budget;
    let next = AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..MEASURE_WORKERS.min(todo.len()) {
            sc.spawn(|| {
                let mut buf = vec![0u64; du::BUF_U64];
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(p) = todo.get(i) else { break };
                    if stop.load(Ordering::Relaxed) || Instant::now() > until {
                        break;
                    }
                    if let Some((b, f)) = cached_size(p) {
                        emit(p, b, f, true);
                        continue;
                    }
                    let w = walk(p, until.min(Instant::now() + MEASURE_ITEM), stop, &mut buf);
                    remember_walk(p, &w);
                    let Walked { bytes: b, files: f, done, .. } = w;
                    if !stop.load(Ordering::Relaxed) {
                        emit(p, b, f, done);
                    }
                }
            });
        }
    });
}

// ---------------- gỡ cài đặt ----------------

static RUNNING: Mutex<Option<String>> = Mutex::new(None);

#[derive(Serialize, Clone, Debug)]
pub struct UninstallOut {
    pub id: String,
    pub name: String,
    /// app đã biến mất khỏi danh sách (gỡ xong thật)
    pub removed: bool,
    pub error: Option<String>,
    pub took_ms: u64,
}

/// App đang được gỡ (id), nếu có.
pub fn running() -> Option<String> {
    RUNNING.lock().ok().and_then(|g| g.clone())
}

/// Bắt đầu gỡ `id`. Trả lỗi ngay nếu không chạy được trình gỡ; xong (gỡ hết / bị huỷ) thì gọi `done` trên luồng riêng.
pub fn uninstall(id: &str, done: impl FnOnce(UninstallOut) + Send + 'static) -> Result<String, String> {
    {
        let mut g = RUNNING.lock().map_err(|_| tr!("errors.lock"))?;
        if let Some(cur) = g.as_ref() {
            return Err(if cur == id { tr!("apps.err.uninstalling_this") } else { tr!("apps.err.uninstalling_other") });
        }
        *g = Some(id.to_owned());
        STOP_WAIT.store(false, Ordering::Relaxed);
    }
    let release = || {
        if let Ok(mut g) = RUNNING.lock() {
            *g = None;
        }
    };
    let t0 = Instant::now();
    if let Some(full) = id.strip_prefix("s|") {
        let full = full.to_owned();
        let name = list_store_name(&full);
        let id = id.to_owned();
        let name2 = name.clone();
        std::thread::spawn(move || {
            let error = remove_package(&full).err();
            let removed = !store_exists(&full);
            if let Ok(mut m) = SIZES.lock() {
                m.clear();
            }
            invalidate();
            if let Ok(mut g) = RUNNING.lock() {
                *g = None;
            }
            util::log_line(&format!("[Ứng dụng] Gỡ gói {full}: {}", if removed { "xong".to_owned() } else { error.clone().unwrap_or_default() }));
            done(UninstallOut { id, name: name2, removed, error: if removed { None } else { error }, took_ms: t0.elapsed().as_millis() as u64 });
        });
        return Ok(name);
    }
    let Some(app) = find_win32(id) else {
        release();
        return Err(tr!("apps.err.gone_maybe_removed"));
    };
    if !app.can_uninstall {
        release();
        return Err(tr!("apps.err.cannot_uninstall"));
    }
    let tracked = match launch(&app.command) {
        Ok(t) => t,
        Err(e) => {
            release();
            return Err(e);
        }
    };
    util::log_line(&format!("[Ứng dụng] Gỡ {}: {}", app.name, app.command));
    let (id, name) = (id.to_owned(), app.name.clone());
    std::thread::spawn(move || {
        tracked.wait(t0 + UNINSTALL_MAX, &app.location);
        // trình gỡ thường xoá khoá registry ở cuối; chờ thêm chút cho chắc
        let mut removed = !win32_exists(&id);
        for _ in 0..10 {
            if removed {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            removed = !win32_exists(&id);
        }
        if let Ok(mut m) = SIZES.lock() {
            m.clear();
        }
        invalidate();
        if let Ok(mut g) = RUNNING.lock() {
            *g = None;
        }
        util::log_line(&format!("[Ứng dụng] Gỡ {} {}", app.name, if removed { "xong" } else { "chưa xong (bị huỷ?)" }));
        done(UninstallOut { id, name: app.name, removed, error: None, took_ms: t0.elapsed().as_millis() as u64 });
    });
    Ok(name)
}

fn list_store_name(full: &str) -> String {
    store_package(full)
        .and_then(|p| p.DisplayName().ok())
        .map(|h| h.to_string())
        .unwrap_or_else(|| full.split('_').next().unwrap_or(full).to_owned())
}

fn remove_package(full: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;
    let pm = PackageManager::new().map_err(|e| util::win_err(&e))?;
    let r = pm.RemovePackageAsync(&HSTRING::from(full)).and_then(|op| op.join()).map_err(|e| util::win_err(&e))?;
    let code = r.ExtendedErrorCode().map(|h| h.0).unwrap_or(0);
    if code < 0 {
        let text = r.ErrorText().map(|h| h.to_string()).unwrap_or_default();
        return Err(if text.trim().is_empty() { tr!("apps.err.win_code", code = format!("{code:08X}")) } else { text.trim().to_owned() });
    }
    Ok(())
}

/// Mở trình sửa (ModifyPath / MsiExec /I) – không theo dõi.
pub fn modify(id: &str) -> Result<(), String> {
    let app = find_win32(id).ok_or_else(|| tr!("apps.err.gone"))?;
    if !app.can_modify {
        return Err(tr!("apps.err.no_modify"));
    }
    let k = id.splitn(3, '|').nth(2).unwrap_or_default();
    let cmd = match app.msi && is_guid(k) {
        true => format!("MsiExec.exe /I{k}"),
        false => {
            let mut it = id.splitn(3, '|');
            let code = it.nth(1).unwrap_or_default();
            let (_, hive, flags, ..) = ROOTS.into_iter().find(|r| r.0 == code).ok_or_else(|| tr!("apps.err.bad_id"))?;
            let key = open_root(hive, flags).and_then(|r| r.open_subkey_with_flags(k, KEY_READ).ok()).ok_or_else(|| tr!("apps.err.read_key"))?;
            s(&key, "ModifyPath")
        }
    };
    launch(&cmd).map(drop)
}

/// "Thôi chờ" trình gỡ hiện tại (người dùng bấm; xong thì đọc lại registry như bình thường).
static STOP_WAIT: AtomicBool = AtomicBool::new(false);

pub fn stop_waiting() {
    STOP_WAIT.store(true, Ordering::Relaxed);
}

/// PID các tiến trình đang trong job (tối đa 128).
fn job_pids(job: windows::Win32::Foundation::HANDLE) -> Vec<u32> {
    use windows::Win32::System::JobObjects::{JobObjectBasicProcessIdList, QueryInformationJobObject};
    // JOBOBJECT_BASIC_PROCESS_ID_LIST: 2 × u32 rồi mảng ULONG_PTR
    let mut buf = vec![0usize; 1 + 128];
    let ok = unsafe { QueryInformationJobObject(Some(job), JobObjectBasicProcessIdList, buf.as_mut_ptr() as *mut _, (buf.len() * std::mem::size_of::<usize>()) as u32, None) }.is_ok();
    if !ok {
        return Vec::new();
    }
    let n = (buf[0] >> 32) as usize; // NumberOfProcessIdsInList (nửa sau của 8 byte đầu)
    buf.iter().skip(1).take(n.min(128)).map(|&p| p as u32).collect()
}

/// Tiến trình còn là trình gỡ: chạy từ Temp / thư mục của app, hoặc tên kiểu unins*, uninst*, Au_*, msiexec.
fn uninstaller_like(pid: u32, location: &str) -> bool {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    let path = unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return true };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return true; // không biết -> coi như còn đang gỡ
        }
        String::from_utf16_lossy(&buf[..len as usize]).to_lowercase()
    };
    let name = path.rsplit('\\').next().unwrap_or("");
    let temp = std::env::var("TEMP").unwrap_or_default().to_lowercase();
    let loc = location.trim_end_matches('\\').to_lowercase();
    (!temp.is_empty() && path.starts_with(&temp))
        || (!loc.is_empty() && path.starts_with(&format!("{loc}\\")))
        || name.contains("unin")
        || name.starts_with("au_")
        || name == "msiexec.exe"
}

/// Tiến trình trình gỡ (và job chứa mọi tiến trình con của nó).
struct Tracked {
    process: windows::Win32::Foundation::HANDLE,
    job: Option<windows::Win32::Foundation::HANDLE>,
}

impl Tracked {
    /// Chờ tới khi job hết tiến trình (hoặc tiến trình kết thúc nếu không có job), tối đa tới `until` / tới khi người
    /// dùng bấm "Thôi chờ". Trình gỡ chính đã thoát mà job chỉ còn chương trình KHÔNG phải trình gỡ (trình duyệt mở
    /// trang khảo sát, app vừa được mở lại…) thì cũng coi là xong.
    fn wait(&self, until: Instant, location: &str) {
        use windows::Win32::System::JobObjects::{JobObjectBasicAccountingInformation, QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION};
        use windows::Win32::System::Threading::WaitForSingleObject;
        loop {
            if Instant::now() > until || STOP_WAIT.load(Ordering::Relaxed) {
                return;
            }
            match self.job {
                Some(job) => {
                    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                    let ok = unsafe {
                        QueryInformationJobObject(Some(job), JobObjectBasicAccountingInformation, &mut info as *mut _ as *mut _, std::mem::size_of_val(&info) as u32, None)
                    }
                    .is_ok();
                    if !ok || info.ActiveProcesses == 0 {
                        return;
                    }
                    let main_done = unsafe { WaitForSingleObject(self.process, 0) }.0 == 0;
                    if main_done && !job_pids(job).iter().any(|&pid| uninstaller_like(pid, location)) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                None => {
                    if unsafe { WaitForSingleObject(self.process, 1000) }.0 == 0 {
                        return; // WAIT_OBJECT_0
                    }
                }
            }
        }
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        unsafe {
            if !self.process.is_invalid() {
                let _ = CloseHandle(self.process);
            }
            if let Some(j) = self.job {
                let _ = CloseHandle(j);
            }
        }
    }
}

// SAFETY: HANDLE chỉ là số; dùng trên một luồng tại một thời điểm.
unsafe impl Send for Tracked {}

/// "\"C:\a b\x.exe\" /S" -> ("C:\a b\x.exe", "/S"); "C:\a b\x.exe /S" -> theo ".exe".
fn split_command(cmd: &str) -> (String, String) {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        if let Some(e) = rest.find('"') {
            return (rest[..e].to_owned(), rest[e + 1..].trim().to_owned());
        }
    }
    if let Some(i) = c.to_lowercase().find(".exe") {
        return (c[..i + 4].to_owned(), c[i + 4..].trim().to_owned());
    }
    match c.split_once(' ') {
        Some((a, b)) => (a.to_owned(), b.trim().to_owned()),
        None => (c.to_owned(), String::new()),
    }
}

/// Chạy một dòng lệnh như Windows (CreateProcess), trong một job để theo dõi cả tiến trình con.
/// Bản dev không có quyền Admin mà trình gỡ cần quyền -> hỏi UAC (ShellExecute "runas"), chỉ theo dõi tiến trình đó.
fn launch(cmd: &str) -> Result<Tracked, String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, ERROR_ELEVATION_REQUIRED};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_BREAKAWAY_OK,
    };
    use windows::Win32::System::Threading::{CreateProcessW, ResumeThread, CREATE_SUSPENDED, PROCESS_INFORMATION, STARTUPINFOW};
    let cmd = util::expand_env(cmd.trim());
    if cmd.is_empty() {
        return Err(tr!("apps.err.no_uninstall_cmd"));
    }
    let mut line = util::wide(&cmd);
    let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    let created = unsafe { CreateProcessW(PCWSTR::null(), Some(PWSTR(line.as_mut_ptr())), None, None, false, CREATE_SUSPENDED, None, PCWSTR::null(), &si, &mut pi) };
    if let Err(e) = created {
        if e.code() == ERROR_ELEVATION_REQUIRED.to_hresult() {
            return launch_elevated(&cmd);
        }
        return Err(tr!("apps.err.run_uninstaller", e = util::win_err(&e)));
    }
    unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null()).ok();
        let mut in_job = false;
        if let Some(j) = job {
            // cho phép tiến trình con tự xin ra khỏi job (một số trình cài cần), còn lại tự vào job
            let mut lim = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            lim.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            let _ = SetInformationJobObject(j, JobObjectExtendedLimitInformation, &lim as *const _ as *const _, std::mem::size_of_val(&lim) as u32);
            in_job = AssignProcessToJobObject(j, pi.hProcess).is_ok();
        }
        ResumeThread(pi.hThread);
        let _ = CloseHandle(pi.hThread);
        let job = match (job, in_job) {
            (Some(j), true) => Some(j),
            (Some(j), false) => {
                let _ = CloseHandle(j);
                None
            }
            _ => None,
        };
        Ok(Tracked { process: pi.hProcess, job })
    }
}

fn launch_elevated(cmd: &str) -> Result<Tracked, String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (exe, args) = split_command(cmd);
    let (exe, args) = (util::wide(&exe), util::wide(&args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(args.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| tr!("apps.err.run_uninstaller", e = util::win_err(&e)))?;
    Ok(Tracked { process: info.hProcess, job: None })
}

// ---------------- cho trợ lý AI ----------------

fn short_date(ms: i64) -> String {
    if ms <= 0 {
        return "?".into();
    }
    chrono::DateTime::from_timestamp_millis(ms).map(|t| t.format("%d/%m/%Y").to_string()).unwrap_or_else(|| "?".into())
}

/// Danh sách cho model: lọc theo tên, sắp theo dung lượng / tên / ngày cài. Đo thư mục cài đặt khi `measure`.
pub fn ai_report(name: Option<&str>, sort: &str, limit: usize, include_system: bool, measure_now: bool) -> String {
    let all = list();
    let needle = name.map(|n| n.to_lowercase());
    let mut apps: Vec<&App> = all
        .apps
        .iter()
        .filter(|a| include_system || !a.system)
        .filter(|a| needle.as_ref().is_none_or(|n| a.name.to_lowercase().contains(n) || a.publisher.to_lowercase().contains(n)))
        .collect();
    if measure_now {
        let paths: Vec<String> = apps.iter().map(|a| a.location.clone()).collect();
        measure(&paths, Duration::from_secs(10), &AtomicBool::new(false), |_, _, _, _| {});
    }
    let size = |a: &App| cached_size(&a.location).map(|v| v.0).unwrap_or(a.est_bytes);
    match sort {
        "name" => apps.sort_by_key(|a| a.name.to_lowercase()),
        "date" => apps.sort_by_key(|a| std::cmp::Reverse(a.installed)),
        // lâu không dùng trước (không rõ = 0 lên đầu nhưng chỉ khi có dữ liệu đầy đủ mới đáng tin)
        "last_used" => apps.sort_by_key(|a| a.last_used),
        _ => apps.sort_by_key(|a| std::cmp::Reverse(size(a))),
    }
    let total = apps.len();
    let full = super::usage::get().full;
    let mut s = format!(
        "{} ứng dụng{} ({} app thường, {} app Store). Dung lượng: \"đo\" = đo thư mục cài đặt thật, \"≈\" = trình cài tự khai (thường thiếu dữ liệu trong AppData). \"Dùng lần cuối\" = lần Windows thấy một chương trình của app chạy (BAM/Prefetch, cần Admin{}) + số lần mở từ Start/Explorer; chương trình chạy nền tự động cũng tính.\n",
        total,
        needle.as_ref().map(|n| format!(" khớp \"{n}\"")).unwrap_or_default(),
        apps.iter().filter(|a| a.kind == "win32").count(),
        apps.iter().filter(|a| a.kind == "store").count(),
        if full { "" } else { " – đang thiếu" },
    );
    for a in apps.iter().take(limit) {
        let sz = match cached_size(&a.location) {
            Some((b, f)) => format!("{} đo ({} tệp)", du::sz(b), f),
            None if a.est_bytes > 0 => format!("≈{}", du::sz(a.est_bytes)),
            None => "?".into(),
        };
        let used = if a.last_used > 0 || a.launches > 0 {
            format!("dùng lần cuối {}", super::usage::describe(a.last_used, a.launches))
        } else if full {
            "chưa thấy dùng gần đây".to_owned()
        } else {
            "lần dùng: không rõ".to_owned()
        };
        let line = format!(
            "- {} · {} · v{} · cài {} · {} · {} · {}{}{} · {}\n",
            a.name,
            if a.publisher.is_empty() { "?" } else { &a.publisher },
            if a.version.is_empty() { "?" } else { &a.version },
            short_date(a.installed),
            used,
            sz,
            if a.kind == "store" { "Store" } else if a.scope == "user" { "người dùng" } else { "cả máy" },
            if a.arch.is_empty() { String::new() } else { format!(" {}", a.arch) },
            if a.system { " · hệ thống, không gỡ được" } else if !a.can_uninstall { " · không cho gỡ" } else { "" },
            if a.location.is_empty() { "(không khai thư mục)" } else { &a.location },
        );
        if s.len() + line.len() > SUMMARY_MAX {
            let _ = writeln!(s, "… và {} ứng dụng khác (lọc theo `name` để xem).", total - apps.iter().position(|x| std::ptr::eq(*x, *a)).unwrap_or(0));
            break;
        }
        s.push_str(&line);
    }
    if let Some(e) = &all.store_error {
        let _ = writeln!(s, "(Không đọc được app Store: {e})");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_registry_bits() {
        assert_eq!(icon_path(r#""C:\A b\x.exe",0"#), r"C:\A b\x.exe");
        assert_eq!(icon_path(r"C:\A\x.ico"), r"C:\A\x.ico");
        assert_eq!(icon_path(r"C:\A\x.exe,-101"), r"C:\A\x.exe");
        assert_eq!(icon_path("none"), "");
        assert!(parse_date("20240131") > 0);
        assert_eq!(parse_date("2024-1-31"), 0);
        assert_eq!(split_command(r#""C:\a b\u.exe" /S"#), (r"C:\a b\u.exe".to_owned(), "/S".to_owned()));
        assert_eq!(split_command(r"C:\a b\u.exe /S"), (r"C:\a b\u.exe".to_owned(), "/S".to_owned()));
        assert_eq!(percent_decode("C:/Program%20Files/x"), "C:/Program Files/x");
    }

    /// Danh sách thật trên máy này (chỉ đọc) + đo vài thư mục.
    #[test]
    fn real_list() {
        let r = list();
        println!("{} app, {} ms, lỗi Store: {:?}", r.apps.len(), r.took_ms, r.store_error);
        assert!(!r.apps.is_empty());
        for a in r.apps.iter().take(8) {
            println!("{} | {} | {} | {} | {} | icon={}", a.name, a.kind, a.version, a.location, a.command, a.icon);
        }
        let paths: Vec<String> = r.apps.iter().map(|a| a.location.clone()).take(10).collect();
        let t0 = Instant::now();
        let n = AtomicUsize::new(0);
        measure(&paths, Duration::from_secs(20), &AtomicBool::new(false), |_, _, _, _| {
            n.fetch_add(1, Ordering::Relaxed);
        });
        println!("đo {} thư mục: {} ms", n.load(Ordering::Relaxed), t0.elapsed().as_millis());
        let rep = ai_report(None, "size", 10, false, false);
        assert!(rep.len() <= SUMMARY_MAX + 200);
    }
}
