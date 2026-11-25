//! Lần dùng cuối của chương trình – Windows tự ghi, app chỉ đọc (không theo dõi gì thêm):
//! - BAM (`HKLM\SYSTEM\CurrentControlSet\Services\bam\State\UserSettings\<SID>`): đường dẫn exe (dạng
//!   `\Device\HarddiskVolumeN\…`) hoặc tên family của app Store -> lần chạy gần nhất. Cần quyền Admin.
//! - Prefetch (`%windir%\Prefetch\TEN.EXE-HASH.pf`): lần sửa tệp = lần chạy gần nhất theo TÊN exe. Cần quyền Admin.
//! - UserAssist (`HKCU\…\Explorer\UserAssist\{GUID}\Count`, tên mã ROT13): số lần người dùng mở từ Start / Explorer
//!   và lần mở cuối. Không cần Admin.
//!
//! Đọc một lần (~5–20 ms), giữ 60 s. Dùng cho trang Ứng dụng, bộ tool của trợ lý (ứng dụng, khởi động, dịch vụ).
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

use super::{du, util};

const TTL: Duration = Duration::from_secs(60);

#[derive(Default, Debug)]
pub struct Usage {
    /// đường dẫn exe viết thường -> (lần cuối ms, số lần mở; 0 = không rõ)
    paths: HashMap<String, (i64, u32)>,
    /// tên exe viết thường -> lần chạy cuối (Prefetch)
    names: HashMap<String, i64>,
    /// family app Store viết thường -> lần chạy cuối (BAM / UserAssist)
    families: HashMap<String, (i64, u32)>,
    /// đọc được BAM / Prefetch (có quyền Admin)
    pub full: bool,
}

type Cached = Option<(Instant, Arc<Usage>)>;
static CACHE: LazyLock<Mutex<Cached>> = LazyLock::new(|| Mutex::new(None));

/// Bản đọc gần nhất (≤ 60 s).
pub fn get() -> Arc<Usage> {
    if let Ok(g) = CACHE.lock() {
        if let Some((t, u)) = g.as_ref() {
            if t.elapsed() < TTL {
                return u.clone();
            }
        }
    }
    let u = Arc::new(read());
    if let Ok(mut g) = CACHE.lock() {
        *g = Some((Instant::now(), u.clone()));
    }
    u
}

fn ft_ms(ft: u64) -> i64 {
    if ft == 0 { 0 } else { (ft / 10_000) as i64 - 11_644_473_600_000 }
}

fn bump(m: &mut HashMap<String, (i64, u32)>, k: String, t: i64, n: u32) {
    let e = m.entry(k).or_insert((0, 0));
    e.0 = e.0.max(t);
    e.1 += n;
}

/// \Device\HarddiskVolume3 -> C:
fn devices() -> Vec<(String, String)> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::QueryDosDeviceW;
    let mut out = Vec::new();
    for l in b'A'..=b'Z' {
        let drive = format!("{}:", l as char);
        let w = util::wide(&drive);
        let mut buf = [0u16; 512];
        let n = unsafe { QueryDosDeviceW(PCWSTR(w.as_ptr()), Some(&mut buf)) };
        if n > 0 {
            let dev = util::from_wide_buf(&buf);
            if !dev.is_empty() {
                out.push((dev.to_lowercase(), drive));
            }
        }
    }
    out
}

fn read() -> Usage {
    let mut u = Usage::default();
    // BAM: mọi tài khoản (giá trị REG_BINARY, 8 byte đầu là FILETIME)
    let devs = devices();
    for root in [r"SYSTEM\CurrentControlSet\Services\bam\State\UserSettings", r"SYSTEM\CurrentControlSet\Services\bam\UserSettings"] {
        let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(root, KEY_READ) else { continue };
        u.full = true;
        for sid in k.enum_keys().flatten() {
            let Ok(sk) = k.open_subkey_with_flags(&sid, KEY_READ) else { continue };
            for (name, val) in sk.enum_values().flatten() {
                if val.bytes.len() < 8 {
                    continue;
                }
                let t = ft_ms(u64::from_le_bytes(val.bytes[..8].try_into().unwrap_or_default()));
                let l = name.to_lowercase();
                if l.starts_with(r"\device\") {
                    if let Some((dev, drive)) = devs.iter().find(|(d, _)| l.starts_with(&format!("{d}\\"))) {
                        bump(&mut u.paths, format!("{drive}{}", &l[dev.len()..]), t, 0);
                    }
                } else if !l.contains('\\') && l.contains('_') {
                    bump(&mut u.families, l, t, 0);
                }
            }
        }
    }
    // Prefetch
    let pf = format!("{}\\Prefetch", util::windir());
    let mut buf = vec![0u64; du::BUF_U64];
    let mut z = du::long_path(&pf);
    z.push(0);
    if du::list_dir(&z, &mut buf, |e| {
        let n = String::from_utf16_lossy(e.name).to_lowercase();
        if let Some(stem) = n.strip_suffix(".pf") {
            if let Some((exe, _)) = stem.rsplit_once('-') {
                let v = u.names.entry(exe.to_owned()).or_insert(0);
                *v = (*v).max(e.mtime);
            }
        }
    })
    .is_ok()
    {
        u.full = true;
    }
    // UserAssist
    let kf = known_folders();
    if let Ok(ua) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist", KEY_READ) {
        for g in ua.enum_keys().flatten() {
            let Ok(c) = ua.open_subkey_with_flags(format!(r"{g}\Count"), KEY_READ) else { continue };
            for (name, val) in c.enum_values().flatten() {
                if val.bytes.len() < 68 {
                    continue;
                }
                let name = rot13(&name);
                let count = u32::from_le_bytes(val.bytes[4..8].try_into().unwrap_or_default());
                let t = ft_ms(u64::from_le_bytes(val.bytes[60..68].try_into().unwrap_or_default()));
                if count == 0 && t <= 0 {
                    continue;
                }
                let mut l = name.to_lowercase();
                // {KNOWNFOLDERID}\đường\dẫn -> đường dẫn thật
                if l.starts_with('{') {
                    if let Some(end) = l.find('}') {
                        if let Some(base) = kf.get(&l[..=end]) {
                            l = format!("{base}{}", &l[end + 1..]);
                        }
                    }
                }
                if l.len() > 3 && l.as_bytes()[1] == b':' && l.ends_with(".exe") {
                    bump(&mut u.paths, l, t, count);
                } else if let Some((fam, _)) = l.split_once('!') {
                    bump(&mut u.families, fam.to_owned(), t, count); // app Store: "Family!App"
                }
            }
        }
    }
    u
}

fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a' + 13) % 26) + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A' + 13) % 26) + b'A') as char,
            _ => c,
        })
        .collect()
}

fn known_folders() -> HashMap<String, String> {
    let e = |k: &str| std::env::var(k).unwrap_or_default().to_lowercase();
    let win = util::windir().to_lowercase();
    [
        ("{6d809377-6af0-444b-8957-a3773f02200e}", e("ProgramW6432")),
        ("{905e63b6-c1bf-494e-b29c-65b732d3d21a}", e("ProgramFiles")),
        ("{7c5a40ef-a0fb-4bfc-874a-c0f2e0b9fa8e}", e("ProgramFiles(x86)")),
        ("{1ac14e77-02e7-4e5d-b744-2eb1ae5198b7}", format!("{win}\\system32")),
        ("{d65231b0-b2f1-4857-a4ce-a8e7c6ea7d27}", format!("{win}\\syswow64")),
        ("{f38bf404-1d43-42f2-9305-67de0b28fc23}", win.clone()),
        ("{f1b32785-6fba-4fcf-9d55-7b8e7f157091}", e("LOCALAPPDATA")),
        ("{3eb685db-65f9-4cf6-a03a-e3ef65729f3d}", e("APPDATA")),
        ("{5e6c858f-0e22-4760-9afe-ea3317b67173}", e("USERPROFILE")),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .map(|(k, v)| (k.to_owned(), v.trim_end_matches('\\').to_owned()))
    .collect()
}

/// Tên exe không đủ riêng để khớp theo Prefetch (nhiều app trùng tên).
const GENERIC_EXE: &[&str] = &["app.exe", "launcher.exe", "update.exe", "updater.exe", "setup.exe", "installer.exe", "helper.exe", "service.exe", "main.exe", "client.exe", "start.exe", "run.exe", "svchost.exe", "rundll32.exe", "dllhost.exe", "conhost.exe", "cmd.exe", "powershell.exe", "python.exe", "pythonw.exe", "java.exe", "javaw.exe", "node.exe", "electron.exe", "msiexec.exe"];

/// Exe không nói lên việc người dùng dùng app (trình gỡ, cập nhật, báo lỗi…).
fn noise(name: &str) -> bool {
    ["unins", "uninst", "update", "crash", "elevation_service", "setup", "install", "notification_helper", "squirrel", "report"].iter().any(|k| name.contains(k))
}

impl Usage {
    /// Một exe theo đường dẫn: BAM / UserAssist theo đường dẫn, rồi Prefetch theo tên (nếu tên đủ riêng).
    pub fn exe(&self, path: &str) -> Option<(i64, u32)> {
        let l = path.trim_matches('"').to_lowercase();
        let mut best = self.paths.get(&l).copied();
        let name = l.rsplit('\\').next().unwrap_or("");
        if !GENERIC_EXE.contains(&name) {
            if let Some(&t) = self.names.get(name) {
                let b = best.get_or_insert((0, 0));
                b.0 = b.0.max(t);
            }
        }
        best.filter(|b| b.0 > 0 || b.1 > 0)
    }

    /// Một thư mục cài đặt: mọi exe đã ghi nhận nằm trong đó + Prefetch của các exe `names` (đã liệt kê trong thư mục).
    pub fn in_dir(&self, dir: &str, names: &[String]) -> Option<(i64, u32)> {
        let d = format!("{}\\", dir.trim_end_matches('\\').to_lowercase());
        if d.len() <= 4 {
            return None;
        }
        let mut best = (0i64, 0u32);
        for (p, &(t, n)) in &self.paths {
            if p.starts_with(&d) && !noise(p.rsplit('\\').next().unwrap_or("")) {
                best.0 = best.0.max(t);
                best.1 += n;
            }
        }
        for n in names {
            let n = n.to_lowercase();
            if !GENERIC_EXE.contains(&n.as_str()) && !noise(&n) {
                if let Some(&t) = self.names.get(&n) {
                    best.0 = best.0.max(t);
                }
            }
        }
        (best.0 > 0 || best.1 > 0).then_some(best)
    }

    /// App Store theo family (Name_PublisherId).
    pub fn family(&self, fam: &str) -> Option<(i64, u32)> {
        self.families.get(&fam.to_lowercase()).copied()
    }
}

/// Tên các exe trong thư mục cài (cấp 1; không có thì cấp 2) – để khớp Prefetch.
pub fn exe_names(dir: &str, buf: &mut [u64]) -> Vec<String> {
    const DIR: u32 = 0x10;
    const REPARSE: u32 = 0x400;
    let list = |p: &str, buf: &mut [u64], out: &mut Vec<String>, subs: &mut Vec<String>| {
        let mut z = du::long_path(p);
        z.push(0);
        let _ = du::list_dir(&z, buf, |e| {
            let n = String::from_utf16_lossy(e.name);
            if e.attrs & DIR != 0 {
                if e.attrs & REPARSE == 0 {
                    subs.push(format!("{p}\\{n}"));
                }
            } else if n.to_lowercase().ends_with(".exe") {
                out.push(n);
            }
        });
    };
    let (mut out, mut subs) = (Vec::new(), Vec::new());
    list(dir.trim_end_matches('\\'), buf, &mut out, &mut subs);
    if out.iter().all(|n| noise(&n.to_lowercase())) {
        let mut ignore = Vec::new();
        for s in subs.iter().take(24) {
            list(s, buf, &mut out, &mut ignore);
        }
    }
    out
}

/// "12/03/2026 (5 ngày trước)" cho trợ lý.
pub fn describe(t: i64, count: u32) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let days = ((now - t) / 86_400_000).max(0);
    let when = chrono::DateTime::from_timestamp_millis(t).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y").to_string()).unwrap_or_else(|| "?".into());
    let ago = match days {
        0 => "hôm nay".to_owned(),
        1 => "hôm qua".to_owned(),
        d if d < 60 => format!("{d} ngày trước"),
        d if d < 730 => format!("{} tháng trước", d / 30),
        d => format!("{} năm trước", d / 365),
    };
    let n = if count > 0 { format!(", mở {count} lần") } else { String::new() };
    if t > 0 { format!("{when} ({ago}{n})") } else { format!("không rõ ngày{n}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rot13_and_describe() {
        assert_eq!(rot13("P:\\Cebtenz Svyrf"), "C:\\Program Files");
        assert!(describe(chrono::Utc::now().timestamp_millis(), 3).contains("hôm nay, mở 3 lần"));
    }

    #[test]
    fn real_read() {
        let t = Instant::now();
        let u = read();
        println!("usage: {} đường dẫn, {} tên (Prefetch), {} app Store, đủ quyền: {}, {} ms", u.paths.len(), u.names.len(), u.families.len(), u.full, t.elapsed().as_millis());
        let mut v: Vec<_> = u.paths.iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1 .0));
        for (p, (t, n)) in v.iter().take(5) {
            println!("  {p} · {}", describe(*t, *n));
        }
    }
}
