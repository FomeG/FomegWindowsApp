use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use crate::tr;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
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

pub fn log_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("log.txt"))
}

pub fn log_line(msg: &str) {
    if let Some(d) = data_dir() {
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
