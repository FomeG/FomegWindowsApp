// Biến môi trường của Windows (Hệ thống → Biến môi trường), như hộp thoại "Environment Variables" của Windows:
//
// - Đọc thẳng registry (~1 ms): người dùng = HKCU\Environment, hệ thống = HKLM\…\Session Manager\Environment.
//   Giữ nguyên kiểu REG_SZ / REG_EXPAND_SZ và giá trị CHƯA mở rộng (%USERPROFILE%\…); bản mở rộng chỉ để hiển thị.
// - Biến dạng danh sách (Path, PSModulePath, PATHEXT…) được tách từng mục: mục là đường dẫn thì kiểm tra còn tồn tại
//   không (GetFileAttributesW, một lần gọi; đường dẫn mạng \\máy\… không đụng tới để không treo), trùng trong cùng biến,
//   hay đã có ở biến cùng tên của hệ thống (Path hiệu lực = hệ thống rồi người dùng, nên mục đó ở người dùng là thừa).
// - Ghi: kiểm tra tên / độ dài, không cho xoá biến Windows cần (Path, ComSpec, windir…) và không cho để trống Path;
//   giá trị cũ vào lịch sử (`env_history.json`, 100 lần gần nhất) để hoàn tác; rồi báo Windows (WM_SETTINGCHANGE
//   "Environment") để Explorer và các chương trình mở SAU đó thấy giá trị mới (chương trình đang chạy thì không).
// - Giá trị của biến có tên như KEY / TOKEN / PASSWORD bị che khi đưa cho trợ lý AI (`ai_report`).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use winreg::enums::{RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};
use winreg::{RegKey, RegValue};

use crate::tr;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileAttributesW(name: *const u16) -> u32;
    fn SetThreadErrorMode(mode: u32, old: *mut u32) -> i32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn SendMessageTimeoutW(hwnd: isize, msg: u32, wparam: usize, lparam: isize, flags: u32, timeout: u32, result: *mut usize) -> isize;
}

const USER_KEY: &str = "Environment";
const SYSTEM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";
/// giới hạn của Windows cho một biến (ký tự UTF-16, không tính NUL)
pub const MAX_VALUE: usize = 32_766;
const MAX_NAME: usize = 255;
const HISTORY_MAX: usize = 100;

/// Biến dạng danh sách dù giá trị chỉ có một mục.
const LIST_NAMES: &[&str] = &["PATH", "PSMODULEPATH", "PATHEXT", "CLASSPATH", "PYTHONPATH", "INCLUDE", "LIB", "LIBPATH", "NODE_PATH"];
/// Windows và nhiều chương trình cần: không cho xoá (vẫn sửa được).
const PROTECTED: &[&str] = &[
    "PATH", "PATHEXT", "COMSPEC", "WINDIR", "SYSTEMROOT", "OS", "PSMODULEPATH", "TEMP", "TMP", "USERNAME", "DRIVERDATA",
    "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "PROCESSOR_IDENTIFIER", "PROCESSOR_LEVEL", "PROCESSOR_REVISION",
];
/// Tên có các chữ này: giá trị có thể là bí mật -> giao diện che, trợ lý không bao giờ thấy.
const SECRET_HINTS: &[&str] = &["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "PWD", "CREDENTIAL", "AUTH", "PRIVATE", "CONNECTIONSTRING", "CONN_STR", "COOKIE", "SESSION"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Val {
    pub value: String,
    /// REG_EXPAND_SZ: %BIẾN% được mở rộng khi dùng
    pub expand: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub raw: String,
    pub expanded: String,
    /// None = không phải đường dẫn cục bộ (vd. ".EXE" của PATHEXT, đường dẫn mạng) nên không kiểm tra
    pub exists: Option<bool>,
    /// trùng một mục đứng trước trong cùng biến
    pub dup: bool,
    /// đã có trong biến cùng tên của hệ thống (chỉ đánh dấu ở biến người dùng)
    pub other: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Var {
    pub name: String,
    pub scope: &'static str,
    pub value: String,
    pub expand: bool,
    pub expanded: String,
    pub list: bool,
    pub entries: Vec<Entry>,
    pub secret: bool,
    pub protected: bool,
    /// số mục có vấn đề (không tồn tại + trùng + thừa)
    pub issues: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Change {
    pub id: u64,
    /// ms Unix
    pub at: i64,
    pub scope: String,
    pub name: String,
    pub old: Option<Val>,
    pub new: Option<Val>,
    /// lần này là hoàn tác của thay đổi `undo_of`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_of: Option<u64>,
}

#[derive(Serialize)]
pub struct ListOut {
    pub vars: Vec<Var>,
    /// ghi được biến hệ thống (đang chạy Admin)
    pub can_system: bool,
    pub took_ms: u64,
    pub history: Vec<Change>,
}

fn hive(scope: &str) -> Result<(RegKey, &'static str), String> {
    match scope {
        "user" => Ok((RegKey::predef(HKEY_CURRENT_USER), USER_KEY)),
        "system" => Ok((RegKey::predef(HKEY_LOCAL_MACHINE), SYSTEM_KEY)),
        _ => Err(tr!("env.err.scope")),
    }
}

fn reg_err(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied { tr!("errors.need_admin") } else { e.to_string() }
}

fn decode(v: &RegValue) -> Option<Val> {
    let expand = match v.vtype {
        RegType::REG_SZ => false,
        RegType::REG_EXPAND_SZ => true,
        _ => return None,
    };
    let b: &[u8] = &v.bytes;
    let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    Some(Val { value: String::from_utf16_lossy(&units[..end]), expand })
}

/// Các biến của một phạm vi theo thứ tự registry. Không đọc được khoá (hiếm) -> rỗng.
fn read_scope(scope: &str) -> Vec<(String, Val)> {
    let Ok((root, sub)) = hive(scope) else { return Vec::new() };
    let Ok(k) = root.open_subkey_with_flags(sub, KEY_READ) else { return Vec::new() };
    k.enum_values().filter_map(Result::ok).filter(|(n, _)| !n.is_empty()).filter_map(|(n, v)| decode(&v).map(|x| (n, x))).collect()
}

fn read_one(scope: &str, name: &str) -> Option<Val> {
    let (root, sub) = hive(scope).ok()?;
    root.open_subkey_with_flags(sub, KEY_READ).ok()?.get_raw_value(name).ok().and_then(|v| decode(&v))
}

/// %TÊN% -> giá trị trong `map` (tên viết hoa); không có thì giữ nguyên như Windows.
fn expand(s: &str, map: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find('%') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        match after.find('%') {
            Some(b) if b > 0 => {
                let name = &after[..b];
                match map.get(&name.to_uppercase()) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[b + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Bảng để mở rộng: môi trường của app, rồi biến hệ thống, rồi biến người dùng (như Windows dựng cho phiên mới).
fn expand_map(user: &[(String, Val)], system: &[(String, Val)]) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = std::env::vars().map(|(k, v)| (k.to_uppercase(), v)).collect();
    for (n, v) in system.iter().chain(user) {
        let e = if v.expand { expand(&v.value, &map) } else { v.value.clone() };
        map.insert(n.to_uppercase(), e);
    }
    map
}

fn looks_path(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || s.starts_with(r"\\") || (b.len() == 2 && b[1] == b':')
}

fn is_list(name: &str, value: &str) -> bool {
    let up = name.to_uppercase();
    LIST_NAMES.contains(&up.as_str()) || (value.contains(';') && value.split(';').filter(|x| !x.trim().is_empty()).all(|x| looks_path(x.trim()) || x.trim().starts_with('%')))
}

pub fn is_secret(name: &str) -> bool {
    let up = name.to_uppercase();
    SECRET_HINTS.iter().any(|h| up.contains(h))
}

pub fn is_protected(name: &str) -> bool {
    PROTECTED.contains(&name.to_uppercase().as_str())
}

/// Khoá so sánh mục danh sách: bản mở rộng, không phân biệt hoa thường, bỏ "\" cuối.
fn entry_key(expanded: &str) -> String {
    expanded.trim().trim_end_matches(['\\', '/']).to_lowercase()
}

fn exists(path: &str) -> Option<bool> {
    if !looks_path(path) || path.starts_with(r"\\") {
        return None;
    }
    let w: Vec<u16> = path.encode_utf16().chain([0]).collect();
    Some(unsafe { GetFileAttributesW(w.as_ptr()) } != u32::MAX)
}

fn entries(value: &str, map: &HashMap<String, String>, other: Option<&HashSet<String>>) -> Vec<Entry> {
    let mut seen = HashSet::new();
    value
        .split(';')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(|raw| {
            let expanded = expand(raw, map);
            let key = entry_key(&expanded);
            Entry { exists: exists(&expanded), dup: !seen.insert(key.clone()), other: other.is_some_and(|o| o.contains(&key)), raw: raw.to_string(), expanded }
        })
        .collect()
}

fn build(scope: &'static str, name: String, v: Val, map: &HashMap<String, String>, sys_lists: &HashMap<String, HashSet<String>>) -> Var {
    let expanded = if v.expand { expand(&v.value, map) } else { v.value.clone() };
    let list = is_list(&name, &v.value);
    let up = name.to_uppercase();
    // PATHEXT: mục không phải đường dẫn; trùng với hệ thống chỉ tính cho biến danh sách đường dẫn
    let other = if scope == "user" { sys_lists.get(&up) } else { None };
    let entries = if list { entries(&v.value, map, other) } else { Vec::new() };
    let issues = entries.iter().filter(|e| e.exists == Some(false) || e.dup || e.other).count() as u32;
    Var { secret: is_secret(&name), protected: is_protected(&name), scope, value: v.value, expand: v.expand, expanded, list, entries, issues, name }
}

/// Mọi biến người dùng + hệ thống, đã phân tích.
pub fn list() -> ListOut {
    let t0 = Instant::now();
    unsafe {
        SetThreadErrorMode(1, std::ptr::null_mut()); // ổ rời chưa có đĩa: không hiện hộp thoại lỗi
    }
    let (user, system) = (read_scope("user"), read_scope("system"));
    let map = expand_map(&user, &system);
    let sys_lists: HashMap<String, HashSet<String>> = system
        .iter()
        .filter(|(n, v)| is_list(n, &v.value))
        .map(|(n, v)| {
            let e = if v.expand { expand(&v.value, &map) } else { v.value.clone() };
            (n.to_uppercase(), e.split(';').filter(|x| !x.trim().is_empty()).map(|x| entry_key(&expand(x, &map))).collect())
        })
        .collect();
    let mut vars: Vec<Var> = user.into_iter().map(|(n, v)| build("user", n, v, &map, &sys_lists)).collect();
    vars.extend(system.into_iter().map(|(n, v)| build("system", n, v, &map, &sys_lists)));
    ListOut { vars, can_system: crate::is_elevated(), took_ms: t0.elapsed().as_millis() as u64, history: history() }
}

// ---------------- ghi ----------------

fn history_path() -> Option<std::path::PathBuf> {
    // test không bao giờ ghi vào lịch sử thật của người dùng
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("fomeg_env_history_{}.json", std::process::id())));
    }
    super::util::data_dir().map(|d| d.join("env_history.json"))
}

pub fn history() -> Vec<Change> {
    history_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_history(h: &[Change]) {
    let Some(p) = history_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(b) = serde_json::to_vec(h) {
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Báo mọi cửa sổ (Explorer…) rằng biến môi trường đã đổi. Chạy nền: cửa sổ treo có thể làm chậm tới vài giây.
fn broadcast() {
    let _ = std::thread::Builder::new().name("env-broadcast".into()).spawn(|| {
        let w: Vec<u16> = "Environment".encode_utf16().chain([0]).collect();
        let mut r = 0usize;
        // HWND_BROADCAST, WM_SETTINGCHANGE, SMTO_ABORTIFHUNG, 3 s mỗi cửa sổ
        unsafe {
            SendMessageTimeoutW(0xFFFF, 0x001A, 0, w.as_ptr() as isize, 0x0002, 3000, &mut r);
        }
    });
}

fn validate(name: &str, new: Option<&Val>) -> Result<(), String> {
    if name.is_empty() || name.chars().count() > MAX_NAME || name.contains('=') || name.contains('\0') || name.trim() != name {
        return Err(tr!("env.err.name"));
    }
    match new {
        None if is_protected(name) => Err(tr!("env.err.protected", name = name)),
        Some(v) if v.value.contains('\0') || v.value.encode_utf16().count() > MAX_VALUE => Err(tr!("env.err.too_long", max = MAX_VALUE)),
        Some(v) if name.eq_ignore_ascii_case("PATH") && v.value.trim().trim_matches(';').is_empty() => Err(tr!("env.err.empty_path")),
        _ => Ok(()),
    }
}

fn write(scope: &str, name: &str, new: Option<&Val>) -> Result<(), String> {
    let (root, sub) = hive(scope)?;
    let (k, _) = root.create_subkey_with_flags(sub, KEY_SET_VALUE).map_err(reg_err)?;
    match new {
        Some(v) => {
            let mut bytes: Vec<u8> = v.value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            bytes.shrink_to_fit();
            #[allow(clippy::useless_conversion)]
            let bytes = bytes.into();
            k.set_raw_value(name, &RegValue { bytes, vtype: if v.expand { RegType::REG_EXPAND_SZ } else { RegType::REG_SZ } }).map_err(reg_err)
        }
        None => match k.delete_value(name) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(reg_err(e)),
            _ => Ok(()),
        },
    }
}

fn apply(scope: &str, name: &str, new: Option<Val>, undo_of: Option<u64>) -> Result<Option<Change>, String> {
    let _g = WRITE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    hive(scope)?;
    validate(name, new.as_ref())?;
    let old = read_one(scope, name);
    if old == new {
        return Ok(None);
    }
    write(scope, name, new.as_ref())?;
    let mut h = history();
    let c = Change { id: h.iter().map(|c| c.id).max().unwrap_or(0) + 1, at: chrono::Utc::now().timestamp_millis(), scope: scope.into(), name: name.into(), old, new, undo_of };
    h.push(c.clone());
    let extra = h.len().saturating_sub(HISTORY_MAX);
    h.drain(..extra);
    save_history(&h);
    broadcast();
    Ok(Some(c))
}

/// Đặt (Some) hoặc xoá (None) một biến. Kiểu REG_EXPAND_SZ khi giá trị có %…% hoặc biến cũ đã là kiểu đó.
pub fn set(scope: &str, name: &str, value: Option<String>) -> Result<Option<Change>, String> {
    let name = name.trim();
    let new = value.map(|v| {
        let old = read_one(scope, name);
        Val { expand: v.contains('%') || old.is_some_and(|o| o.expand), value: v }
    });
    apply(scope, name, new, None)
}

/// Hoàn tác thay đổi `id`: đặt lại giá trị trước đó (cũng được ghi vào lịch sử nên hoàn tác tiếp được).
pub fn restore(id: u64) -> Result<Option<Change>, String> {
    let c = history().into_iter().find(|c| c.id == id).ok_or_else(|| tr!("env.err.no_change"))?;
    apply(&c.scope, &c.name, c.old, Some(id))
}

// ---------------- cho trợ lý AI ----------------

#[cfg(test)]
fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max).collect::<String>()) }
}

/// Báo cáo chỉ đọc cho tool `environment_variables`. Giá trị bí mật luôn bị che.
pub fn ai_report(filter: Option<&str>) -> String {
    let l = list();
    let f = filter.map(str::to_lowercase);
    let mut s = String::from("Biến môi trường (registry). Chương trình đang chạy chỉ thấy giá trị mới sau khi mở lại. \
[trùng] = trùng mục phía trên, [có ở system] = mục Path người dùng đã có trong Path hệ thống (thừa).\n");
    let path: usize = l.vars.iter().filter(|v| v.name.eq_ignore_ascii_case("PATH")).map(|v| v.expanded.chars().count() + 1).sum();
    let _ = writeln!(s, "PATH hiệu lực (hệ thống rồi người dùng): ≈ {path} ký tự (giới hạn {MAX_VALUE}).");
    if !l.can_system {
        s.push_str("Ứng dụng KHÔNG chạy Admin: chỉ sửa được biến người dùng.\n");
    }
    for scope in ["user", "system"] {
        let vars: Vec<&Var> = l.vars.iter().filter(|v| v.scope == scope && f.as_ref().is_none_or(|f| v.name.to_lowercase().contains(f))).collect();
        let _ = writeln!(s, "\n## {} ({} biến)", if scope == "user" { "Người dùng – user" } else { "Hệ thống – system" }, vars.len());
        for v in vars {
            let kind = if v.expand { " [REG_EXPAND_SZ]" } else { "" };
            let lock = if v.protected { " [không xoá được]" } else { "" };
            if v.secret {
                let _ = writeln!(s, "- {} = •••• (bí mật, đã che, {} ký tự){kind}", v.name, v.value.chars().count());
            } else if v.list {
                let bad = v.entries.iter().filter(|e| e.exists == Some(false)).count();
                let dup = v.entries.iter().filter(|e| e.dup || e.other).count();
                let _ = writeln!(s, "- {}{kind}{lock}: {} mục, {} ký tự{}{}", v.name, v.entries.len(), v.value.chars().count(), if bad > 0 { format!(", {bad} KHÔNG TỒN TẠI") } else { String::new() }, if dup > 0 { format!(", {dup} trùng") } else { String::new() });
                for e in &v.entries {
                    let mark = match (e.exists, e.dup, e.other) {
                        (_, true, _) => " [trùng]",
                        (_, _, true) => " [có ở system]",
                        (Some(false), _, _) => " [KHÔNG TỒN TẠI]",
                        _ => "",
                    };
                    let exp = if e.expanded != e.raw { format!(" → {}", e.expanded) } else { String::new() };
                    let _ = writeln!(s, "    · {}{exp}{mark}", e.raw);
                }
            } else {
                let exp = if v.expanded != v.value { format!(" → {}", v.expanded) } else { String::new() };
                let _ = writeln!(s, "- {} = {}{exp}{kind}{lock}", v.name, v.value);
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_like_windows_and_keeps_unknown() {
        let map: HashMap<String, String> = [("USERPROFILE".to_string(), r"C:\Users\A".to_string())].into();
        assert_eq!(expand(r"%UserProfile%\bin;%NOPE%;50%", &map), r"C:\Users\A\bin;%NOPE%;50%");
        assert_eq!(expand("%%x", &map), "%%x");
    }

    #[test]
    fn splits_lists_and_marks_problems() {
        let map: HashMap<String, String> = [("SYSTEMROOT".to_string(), std::env::var("SystemRoot").unwrap())].into();
        let other: HashSet<String> = [entry_key(&std::env::var("SystemRoot").unwrap())].into();
        let e = entries(r"%SystemRoot%;C:\definitely\missing\dir;;c:\definitely\missing\dir\;.EXE", &map, Some(&other));
        assert_eq!(e.len(), 4, "mục trống bị bỏ");
        assert_eq!((e[0].exists, e[0].other), (Some(true), true));
        assert_eq!((e[1].exists, e[1].dup), (Some(false), false));
        assert!(e[2].dup, "trùng khi khác hoa thường và có \\ cuối");
        assert_eq!(e[3].exists, None, "PATHEXT không phải đường dẫn");
        assert!(is_list("Path", r"C:\a"));
        assert!(is_list("MY_DIRS", r"C:\a;D:\b"));
        assert!(!is_list("PROMPT", "$P$G"));
        assert!(is_secret("OPENAI_API_KEY") && is_secret("GITHUB_TOKEN") && !is_secret("JAVA_HOME"));
    }

    #[test]
    fn refuses_bad_writes() {
        assert!(validate("", None).is_err());
        assert!(validate("A=B", Some(&Val { value: "x".into(), expand: false })).is_err());
        assert!(validate("Path", None).is_err(), "không xoá Path");
        assert!(validate("path", Some(&Val { value: " ; ".into(), expand: false })).is_err(), "không để trống Path");
        assert!(validate("X", Some(&Val { value: "a".repeat(MAX_VALUE + 1), expand: false })).is_err());
        assert!(validate("MY_VAR", None).is_ok());
        assert!(hive("other").is_err());
    }

    /// Ghi thật vào HKCU\Environment một biến tạm, đọc lại, hoàn tác, rồi xoá – không đụng biến khác.
    #[test]
    fn writes_user_variable_and_undoes() {
        let name = format!("FOMEG_TEST_{}", std::process::id());
        let c = set("user", &name, Some(r"%USERPROFILE%\x".into())).unwrap().unwrap();
        assert!(c.old.is_none());
        let v = read_one("user", &name).unwrap();
        assert!(v.expand, "có %…% -> REG_EXPAND_SZ");
        assert!(set("user", &name, Some(r"%USERPROFILE%\x".into())).unwrap().is_none(), "không đổi gì -> không ghi");
        let c2 = set("user", &name, Some("plain".into())).unwrap().unwrap();
        restore(c2.id).unwrap();
        assert_eq!(read_one("user", &name).unwrap().value, r"%USERPROFILE%\x");
        set("user", &name, None).unwrap();
        assert!(read_one("user", &name).is_none());
        let l = list();
        assert!(l.vars.iter().any(|v| v.scope == "user" && v.name.eq_ignore_ascii_case("PATH")) || l.vars.iter().any(|v| v.scope == "system"));
        println!("{} biến, {} ms\n{}", l.vars.len(), l.took_ms, short(&ai_report(Some("path")), 1500));
    }
}
