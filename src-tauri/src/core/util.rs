use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use crate::tr;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_INVALID_DRIVE, ERROR_NOT_READY, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, DELETE, FILE_FLAG_DELETE_ON_CLOSE};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide_buf(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

pub fn pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { p.to_string().unwrap_or_default() }
}

pub fn expand_env(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let w = wide(s);
    unsafe {
        let n = ExpandEnvironmentStringsW(PCWSTR(w.as_ptr()), None);
        if n == 0 {
            return s.to_string();
        }
        let mut buf = vec![0u16; n as usize + 1];
        ExpandEnvironmentStringsW(PCWSTR(w.as_ptr()), Some(&mut buf));
        from_wide_buf(&buf)
    }
}

pub fn windir() -> String {
    std::env::var("windir").unwrap_or_else(|_| r"C:\Windows".into())
}

/// Tách đường dẫn file thực thi từ một dòng lệnh (giống Get-ExePath trong bản cũ).
pub fn exe_path(cmd: &str) -> String {
    let mut c = expand_env(cmd.trim());
    if c.is_empty() {
        return c;
    }
    if let Some(rest) = c.strip_prefix(r"\??\") {
        c = rest.to_string();
    }
    if c.len() > 12 && c[..12].eq_ignore_ascii_case(r"\SystemRoot\") {
        c = format!(r"{}\{}", windir(), &c[12..]);
    }
    if let Some(rest) = c.strip_prefix('"') {
        if let Some(e) = rest.find('"') {
            if e > 0 {
                return rest[..e].to_string();
            }
        }
    }
    let lower = c.to_ascii_lowercase();
    let mut best: Option<usize> = None;
    for ext in [".exe", ".cmd", ".bat", ".com", ".lnk", ".vbs", ".ps1", ".dll"] {
        if let Some(i) = lower.find(ext) {
            let end = i + ext.len();
            if best.is_none_or(|b| end < b) {
                best = Some(end);
            }
        }
    }
    let p = match best {
        Some(end) => c[..end].to_string(),
        None => c.split(' ').next().unwrap_or("").to_string(),
    };
    if !p.is_empty() && !Path::new(&p).is_absolute() {
        let w = windir();
        for cand in [PathBuf::from(&w).join("System32").join(&p), PathBuf::from(&w).join(&p)] {
            if cand.is_file() {
                return cand.to_string_lossy().into_owned();
            }
        }
    }
    p
}

// Cache nhà phát hành: trong RAM + lưu ra đĩa (%LOCALAPPDATA%\fomeg-windows-app\pubcache.json),
// mỗi mục kèm thời gian sửa + kích thước file để tự làm mới khi file đổi.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct PubEntry {
    m: u64,
    s: u64,
    c: String,
}

#[derive(Default)]
struct PubCache {
    map: HashMap<String, PubEntry>,
    dirty: bool,
}

static PUB_CACHE: LazyLock<Mutex<PubCache>> = LazyLock::new(|| {
    let map = data_dir()
        .and_then(|d| std::fs::read(d.join("pubcache.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    Mutex::new(PubCache { map, dirty: false })
});

fn stamp(path: &str) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let m = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some((m, md.len()))
}

pub fn cached_publisher(path: &str) -> Option<String> {
    let (m, s) = stamp(path)?;
    let c = PUB_CACHE.lock().ok()?;
    let e = c.map.get(&path.to_lowercase())?;
    (e.m == m && e.s == s).then(|| e.c.clone())
}

/// Đọc CompanyName từ version resource của file (có cache).
pub fn publisher(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    if let Some(v) = cached_publisher(path) {
        return v;
    }
    let v = company_name(path);
    if let Some((m, s)) = stamp(path) {
        if let Ok(mut c) = PUB_CACHE.lock() {
            c.map.insert(path.to_lowercase(), PubEntry { m, s, c: v.clone() });
            c.dirty = true;
        }
    }
    v
}

pub fn save_publisher_cache() {
    let Ok(mut c) = PUB_CACHE.lock() else { return };
    if !c.dirty {
        return;
    }
    if let (Some(d), Ok(json)) = (data_dir(), serde_json::to_vec(&c.map)) {
        let _ = std::fs::create_dir_all(&d);
        let tmp = d.join("pubcache.json.tmp");
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, d.join("pubcache.json")).is_ok() {
            c.dirty = false;
        }
    }
}

fn company_name(path: &str) -> String {
    let w = wide(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(w.as_ptr()), None);
        if size == 0 {
            return String::new();
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(PCWSTR(w.as_ptr()), None, size, buf.as_mut_ptr() as *mut c_void).is_err() {
            return String::new();
        }
        let block = buf.as_ptr() as *const c_void;
        let mut langs: Vec<(u16, u16)> = Vec::new();
        let mut p: *mut c_void = std::ptr::null_mut();
        let mut len: u32 = 0;
        let tq = wide(r"\VarFileInfo\Translation");
        if VerQueryValueW(block, PCWSTR(tq.as_ptr()), &mut p, &mut len).as_bool() && !p.is_null() && len >= 4 {
            let pairs = std::slice::from_raw_parts(p as *const u16, (len / 2) as usize);
            for ch in pairs.as_chunks::<2>().0 {
                langs.push((ch[0], ch[1]));
            }
        }
        langs.extend([(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0000, 0x04B0)]);
        for (l, cp) in langs {
            let q = wide(&format!(r"\StringFileInfo\{l:04x}{cp:04x}\CompanyName"));
            let mut p: *mut c_void = std::ptr::null_mut();
            let mut len: u32 = 0;
            if VerQueryValueW(block, PCWSTR(q.as_ptr()), &mut p, &mut len).as_bool() && !p.is_null() && len > 0 {
                let s = std::slice::from_raw_parts(p as *const u16, len as usize);
                let v = from_wide_buf(s).trim().to_string();
                if !v.is_empty() {
                    return v;
                }
            }
        }
        String::new()
    }
}

pub fn is_ms_publisher(p: &str) -> bool {
    p.to_ascii_lowercase().contains("microsoft")
}

pub fn data_dir() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(|d| Path::new(&d).join("fomeg-windows-app"))
}

/// Thư mục log người dùng chọn (Cài đặt → Nhật ký), lưu một dòng trong `<data_dir>\log_dir.txt`.
/// Đọc một lần rồi giữ trong bộ nhớ; `set_log_dir` cập nhật cả hai. None = mặc định (data_dir).
static LOG_DIR: std::sync::RwLock<Option<Option<PathBuf>>> = std::sync::RwLock::new(None);

fn custom_log_dir() -> Option<PathBuf> {
    if let Some(v) = LOG_DIR.read().ok().and_then(|g| g.clone()) {
        return v;
    }
    let v = data_dir()
        .and_then(|d| std::fs::read_to_string(d.join("log_dir.txt")).ok())
        .map(|s| PathBuf::from(s.trim()))
        .and_then(|p| resolve_log_dir(&p).ok());
    if let Ok(mut g) = LOG_DIR.write() {
        *g = Some(v.clone());
    }
    v
}

/// App chạy bằng quyền Admin: không ghi log vào thư mục hệ thống / Program Files / gốc ổ, kể cả khi tệp cấu hình bị sửa tay.
fn log_dir_allowed(p: &Path) -> bool {
    let s = p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    if !p.is_absolute() || s.starts_with(r"\\") || super::du::protected(&s) {
        return false;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default().trim_end_matches('\\').to_lowercase();
    !["SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .iter()
        .map(|k| env(k))
        .any(|root| !root.is_empty() && (s == root || s.starts_with(&format!("{root}\\"))))
}

fn log_dir_io_error(e: std::io::Error) -> String {
    tr!("app.err.log_dir_unwritable", err = e.to_string())
}

fn missing_log_path(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::NotFound
        || e.raw_os_error().is_some_and(|code| {
            code as u32 == ERROR_INVALID_DRIVE.0 || code as u32 == ERROR_NOT_READY.0
        })
}

fn checked_log_dir(resolved: &Path) -> Result<PathBuf, String> {
    let resolved = resolved.to_str().ok_or_else(|| tr!("app.err.log_dir_protected"))?;
    let p = Path::new(resolved.strip_prefix(r"\\?\").unwrap_or(resolved));
    if !log_dir_allowed(p) {
        return Err(tr!("app.err.log_dir_protected"));
    }
    Ok(p.to_owned())
}

fn resolve_log_dir(p: &Path) -> Result<PathBuf, String> {
    if !log_dir_allowed(p) {
        return Err(tr!("app.err.log_dir_protected"));
    }
    let path = std::path::absolute(p).map_err(log_dir_io_error)?;
    for ancestor in path.ancestors() {
        match std::fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                let tail = path.strip_prefix(ancestor).expect("đường dẫn phải chứa thư mục cha");
                if !tail.as_os_str().is_empty() {
                    resolved.push(tail);
                }
                return checked_log_dir(&resolved);
            }
            Err(e) if missing_log_path(&e) => {
                // Junction hoặc symlink còn tồn tại dù đích biến mất thì không thể xác minh.
                if !std::fs::symlink_metadata(ancestor).is_err_and(|e| missing_log_path(&e)) {
                    return Err(log_dir_io_error(e));
                }
            }
            Err(e) => return Err(log_dir_io_error(e)),
        }
    }
    // Ổ đĩa có thể chưa được gắn; giữ đường dẫn nếu nó vẫn hợp lệ.
    checked_log_dir(&path)
}

/// Thư mục chứa log.txt: thư mục người dùng chọn, không thì data_dir.
pub fn log_dir() -> Option<PathBuf> {
    custom_log_dir().or_else(data_dir)
}

pub fn log_dir_is_custom() -> bool {
    custom_log_dir().is_some()
}

/// Đổi thư mục log (None = về mặc định). Chuyển log.txt cũ sang chỗ mới nếu chỗ mới chưa có.
pub fn set_log_dir(dir: Option<&str>) -> Result<(), String> {
    let old = log_path();
    let new_dir = match dir.map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => {
            let requested = Path::new(d);
            let before = resolve_log_dir(requested)?;
            std::fs::create_dir_all(&before).map_err(log_dir_io_error)?;
            let after = std::fs::canonicalize(requested).map_err(log_dir_io_error)?;
            let p = checked_log_dir(&after)?;
            if p != before {
                return Err(tr!("app.err.log_dir_protected"));
            }
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let probe = p.join(format!(".fomeg-write-test-{}-{stamp}", std::process::id()));
            // Xoá đúng tệp vừa tạo khi đóng handle, không xoá theo tên đường dẫn.
            std::fs::OpenOptions::new()
                .access_mode(GENERIC_WRITE.0 | DELETE.0)
                .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0)
                .create_new(true)
                .open(probe)
                .map_err(|e| tr!("app.err.log_dir_unwritable", err = e.to_string()))?;
            Some(p)
        }
        None => None,
    };
    let cfg = data_dir().ok_or_else(|| tr!("app.err.no_log_dir"))?;
    std::fs::create_dir_all(&cfg).map_err(|e| e.to_string())?;
    match &new_dir {
        Some(p) => std::fs::write(cfg.join("log_dir.txt"), p.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?,
        None => match std::fs::remove_file(cfg.join("log_dir.txt")) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(tr!("app.err.log_dir_reset", err = e.to_string())),
        },
    }
    if let Ok(mut g) = LOG_DIR.write() {
        *g = Some(new_dir);
    }
    if let (Some(o), Some(n)) = (old, log_path()) {
        if o != n && o.is_file() && !n.exists() && std::fs::copy(&o, &n).is_ok() {
            let _ = std::fs::remove_file(&o);
        }
    }
    Ok(())
}

pub fn log_path() -> Option<PathBuf> {
    log_dir().map(|d| d.join("log.txt"))
}

pub fn log_line(msg: &str) {
    // Mỗi lần ghi mở theo tên; junction bị đổi sau khi xác minh sẽ không được kiểm tra lại.
    if let Some(d) = log_dir() {
        let _ = std::fs::create_dir_all(&d);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(d.join("log.txt")) {
            use std::io::Write;
            let _ = writeln!(f, "{}  {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), msg);
        }
    }
}

/// Thông báo lỗi Windows dễ hiểu bằng tiếng Việt cho các mã hay gặp.
pub fn win_err(e: &windows::core::Error) -> String {
    let code = (e.code().0 as u32) & 0xFFFF;
    let facility_win32 = (e.code().0 as u32) >> 16 == 0x8007;
    if facility_win32 {
        match code {
            5 => return tr!("errors.need_admin"),
            1051 => return tr!("services.err.has_dependents"),
            1052 => return tr!("services.err.not_accepted"),
            1053 => return tr!("services.err.no_response"),
            1056 => return tr!("services.err.already_running"),
            1058 => return tr!("services.err.disabled"),
            1060 => return tr!("services.err.not_found"),
            1062 => return tr!("services.err.not_running"),
            1068 => return tr!("services.err.dependency_failed"),
            _ => {}
        }
    }
    let m = e.message();
    if m.trim().is_empty() {
        tr!("errors.code", code = format!("{:08X}", e.code().0 as u32))
    } else {
        m.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_dir_refuses_system_folders() {
        let win = std::env::var("SystemRoot").unwrap_or_else(|_| "C:/Windows".into());
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:/Program Files".into());
        for bad in ["C:/", "C:", "relative/logs", "//server/share/logs"] {
            assert!(!log_dir_allowed(Path::new(&bad.replace('/', "\\"))), "{bad}");
        }
        assert!(!log_dir_allowed(&Path::new(&win).join("System32")));
        assert!(!log_dir_allowed(&Path::new(&pf).join("Some App")));
        assert!(log_dir_allowed(&std::env::temp_dir().join("fomeg-logs")));
    }
}
