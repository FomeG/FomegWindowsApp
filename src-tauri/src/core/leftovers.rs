//! Thư mục liên quan của một ứng dụng và phần còn sót sau khi gỡ.
//!
//! Không có vị trí cố định nào, nên tìm theo DẤU NHẬN DIỆN của app: tên (bỏ phiên bản), tên thư mục cài, tên exe của
//! icon, nhà phát hành. Chỉ xem cấp 1 của các nơi app hay ghi (AppData Roaming/Local/LocalLow, Programs, ProgramData,
//! Program Files, thư mục chấm trong hồ sơ người dùng) và cấp 2 trong thư mục của nhà phát hành – mỗi nơi một lệnh
//! liệt kê (`du::list_dir`), vài ms. Thư mục cũng khớp với một app KHÁC đang cài thì đánh dấu "dùng chung", không tích sẵn.
//!
//! - `related(app)`: cho popup gỡ cài đặt và ngăn chi tiết.
//! - `scan()`: phần còn sót của các app ĐÃ GỠ (app tự nhớ danh sách app thấy lần trước, `apps_seen.json`; app biến mất
//!   -> ghi vào `apps_removed.json`), dữ liệu app Store mồ côi (`AppData\Local\Packages\<family>` không còn gói), và các
//!   thư mục trong WindowsApps không còn thuộc tài khoản này (gói của tài khoản khác: gỡ qua PackageManager; mồ côi /
//!   bản cũ: chỉ báo, Windows tự dọn – không xoá tay thư mục do TrustedInstaller giữ).
//!
//! Chỉ đọc. Xoá thư mục đi qua `du_delete_paths` (Thùng rác, chặn thư mục hệ thống); gỡ gói qua `remove_packages`.
use std::collections::{HashMap, HashSet};
use std::path::Path;
use crate::tr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use winreg::enums::KEY_READ;
use winreg::RegKey;

use super::apps::{self, App};
use super::{du, util};

const MEASURE_WORKERS: usize = 6;
const RELATED_BUDGET: Duration = Duration::from_secs(5);
const SCAN_BUDGET: Duration = Duration::from_secs(15);
const HISTORY_MAX: usize = 300;
/// dữ liệu app Store mồ côi không đụng tới trong chừng này thì tích sẵn
const OLD_MS: i64 = 30 * 86_400_000;

/// Tên quá chung, không dùng làm dấu nhận diện.
const STOP: &[&str] = &[
    "app", "apps", "main", "launcher", "setup", "uninstall", "uninst", "unins000", "update", "updater", "install", "installer",
    "program", "programs", "client", "desktop", "bin", "icon", "logo", "productlogo", "appicon", "windows", "microsoft",
    "google", "data", "user", "users", "common", "shared", "tools", "system", "service", "helper", "runtime", "cache", "temp",
    "local", "roaming", "x64", "x86", "amd64", "win64", "win32", "software", "studio", "edition", "free", "pro", "player",
    "browser", "manager", "driver", "drivers", "package", "packages", "packagecache", "default", "settings", "config",
];
/// Hậu tố công ty bỏ khi lấy tên nhà phát hành.
const CORP: &[&str] = &["inc", "corp", "corporation", "llc", "ltd", "limited", "co", "gmbh", "ag", "sa", "srl", "bv", "plc", "company", "international", "technologies", "technology", "software", "the"];
/// Thư mục không bao giờ đề xuất (của Windows / dùng chung / chính app này).
const NEVER: &[&str] = &[
    "microsoft", "windows", "packages", "temp", "programs", "commonfiles", "windowsapps", "modifiablewindowsapps", "fomegwindowsapp", "quanlykhoidong",
    "comfomegwindowsapp", "comquanlykhoidongapp", "packagecache", "regid199106commicrosoft", "usoshared", "usoprivate", "ssh", "intel", "nvidia",
    "nvidiacorporation", "amd", "realtek", "windowsdefender", "windowsnt", "internetexplorer", "referenceassemblies", "msbuild",
    "dotnet", "uninstallinformation", "installshieldinstallationinformation", "virtualstore", "crashdumps", "d3dscache",
    "connecteddevicesplatform", "comms", "publishers", "desktop", "documents", "downloads", "pictures", "music", "videos",
    "onedrive", "favorites", "links", "contacts", "searches", "savedgames", "appdata",
];

#[derive(Serialize, Clone, Debug, Default)]
pub struct Found {
    /// thư mục trên đĩa
    pub path: String,
    /// tên gói đầy đủ (kind = "package")
    pub package: Option<String>,
    /// "folder" | "package" (gỡ qua Windows) | "info" (chỉ báo, không thao tác được)
    pub kind: &'static str,
    /// nơi tìm thấy: "Thư mục cài đặt", "AppData\Roaming"…
    pub area: String,
    /// app sở hữu (với phần còn sót: app đã gỡ / gói)
    pub owner: String,
    pub reason: String,
    /// chắc là của app -> tích sẵn
    pub sure: bool,
    /// tên app khác đang cài cũng khớp thư mục này
    pub shared: Option<String>,
    pub bytes: u64,
    pub files: u64,
    pub newest: i64,
    /// đo chưa hết (quá hạn), số liệu là tối thiểu
    pub partial: bool,
}

#[derive(Serialize)]
pub struct ScanOut {
    pub items: Vec<Found>,
    pub took_ms: u64,
    /// số app đã gỡ đang được nhớ
    pub removed_known: usize,
    pub windowsapps_error: Option<String>,
}

// ---------------- dấu nhận diện ----------------

/// chữ thường, chỉ chữ + số
pub(crate) fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Ident {
    pub name: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub removed_at: i64,
}

impl Ident {
    fn of(a: &App) -> Self {
        Ident { name: a.name.clone(), publisher: a.publisher.clone(), location: a.location.clone(), icon: a.icon.clone(), removed_at: 0 }
    }
}

struct Keys {
    keys: Vec<String>,
    publisher: Option<String>,
}

fn publisher_key(p: &str) -> Option<String> {
    let w: Vec<String> = words(p).into_iter().filter(|w| !CORP.contains(&w.as_str())).collect();
    let k = w.first().cloned()?;
    // nhà phát hành lớn (google…) vẫn dùng được: có nhiều app nên chỉ xem từng thư mục con, không lấy cả thư mục
    (k.chars().count() >= 4).then_some(k)
}

fn keys(id: &Ident) -> Keys {
    // tên bỏ phần trong ngoặc ("(x64)", "(User)") và các từ có chữ số (phiên bản)
    let base = id.name.split('(').next().unwrap_or(&id.name);
    let all = words(base);
    let no_ver: Vec<&String> = all.iter().filter(|w| !w.chars().any(|c| c.is_ascii_digit())).collect();
    let pubw: HashSet<String> = words(&id.publisher).into_iter().collect();
    let mut k: Vec<String> = vec![
        norm(base),
        no_ver.iter().map(|s| s.as_str()).collect::<String>(),
        no_ver.iter().filter(|w| !pubw.contains(w.as_str())).map(|s| s.as_str()).collect::<String>(),
    ];
    if !id.location.is_empty() {
        if let Some(b) = Path::new(&id.location).file_name() {
            k.push(norm(&b.to_string_lossy()));
        }
    }
    if !id.icon.is_empty() {
        if let Some(b) = Path::new(&id.icon).file_stem() {
            k.push(norm(&b.to_string_lossy()));
        }
    }
    let mut seen = HashSet::new();
    k.retain(|x| x.chars().count() >= 4 && !STOP.contains(&x.as_str()) && !NEVER.contains(&x.as_str()) && seen.insert(x.clone()));
    Keys { keys: k, publisher: publisher_key(&id.publisher) }
}

/// "ldplayer9" khớp "ldplayer" (phần thừa là số / ≤ 2 ký tự); khoá ngắn (< 6) phải trùng hẳn.
fn key_match(f: &str, key: &str) -> bool {
    if f == key {
        return true;
    }
    let rest_ok = |r: &str| r.chars().all(|c| c.is_ascii_digit()) || r.chars().count() <= 2;
    (key.len() >= 6 && f.strip_prefix(key).is_some_and(rest_ok)) || (f.len() >= 6 && key.strip_prefix(f).is_some_and(rest_ok))
}

fn matches(f: &str, k: &Keys) -> bool {
    k.keys.iter().any(|key| key_match(f, key))
}

// ---------------- nơi tìm ----------------

struct Entry {
    area: &'static str,
    path: String,
    norm: String,
}

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_default().trim_end_matches('\\').to_owned()
}

/// Nhãn nơi "hồ sơ người dùng" (khoá yml); các nhãn khác là tên thư mục, không dịch.
const PROFILE_AREA: &str = "leftovers.area.profile";

fn area_label(a: &str) -> String {
    if a == PROFILE_AREA { tr!(PROFILE_AREA) } else { a.to_owned() }
}

/// (nhãn, thư mục, chỉ thư mục chấm)
fn roots() -> Vec<(&'static str, String, bool)> {
    let (local, prof) = (env("LOCALAPPDATA"), env("USERPROFILE"));
    vec![
        ("AppData\\Roaming", env("APPDATA"), false),
        ("AppData\\Local", local.clone(), false),
        ("AppData\\LocalLow", format!("{prof}\\AppData\\LocalLow"), false),
        ("AppData\\Local\\Programs", format!("{local}\\Programs"), false),
        ("ProgramData", env("ProgramData"), false),
        ("Program Files", env("ProgramFiles"), false),
        ("Program Files (x86)", env("ProgramFiles(x86)"), false),
        (PROFILE_AREA, prof, true),
    ]
}

fn nul(p: &str) -> Vec<u16> {
    let mut w = du::long_path(p);
    w.push(0);
    w
}

fn subdirs(dir: &str, buf: &mut [u64], dots_only: bool, mut f: impl FnMut(String, String)) {
    const DIR: u32 = 0x10;
    const REPARSE: u32 = 0x400;
    let _ = du::list_dir(&nul(dir), buf, |e| {
        if e.attrs & DIR == 0 || e.attrs & REPARSE != 0 {
            return;
        }
        let name = String::from_utf16_lossy(e.name);
        if dots_only && !name.starts_with('.') {
            return;
        }
        f(format!("{dir}\\{name}"), name);
    });
}

fn entries() -> Vec<Entry> {
    let mut out = Vec::new();
    let mut buf = vec![0u64; du::BUF_U64];
    let mut seen = HashSet::new();
    for (area, dir, dots) in roots() {
        if dir.is_empty() || !seen.insert(dir.to_lowercase()) {
            continue;
        }
        subdirs(&dir, &mut buf, dots, |path, name| {
            let n = norm(&name);
            if !n.is_empty() && !NEVER.contains(&n.as_str()) {
                out.push(Entry { area, path, norm: n });
            }
        });
    }
    out
}

fn inside(p: &str, dir: &str) -> bool {
    let (p, d) = (p.to_lowercase(), dir.trim_end_matches('\\').to_lowercase());
    p == d || p.starts_with(&format!("{d}\\"))
}

/// Mọi app đang cài (kể cả mục ẩn của registry: thành phần hệ thống, bản cập nhật) -> dấu nhận diện, để biết thư mục
/// nào còn chủ.
fn installed_idents(list: &[App]) -> Vec<Ident> {
    let mut out: Vec<Ident> = list.iter().map(Ident::of).collect();
    for (_, hive, flags, ..) in apps::ROOTS {
        let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(apps::UNINSTALL, KEY_READ | flags) else { continue };
        for key in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey_with_flags(&key, KEY_READ) else { continue };
            let name: String = k.get_value("DisplayName").unwrap_or_default();
            if name.trim().is_empty() || list.iter().any(|a| a.name == name) {
                continue;
            }
            out.push(Ident {
                name,
                publisher: k.get_value("Publisher").unwrap_or_default(),
                location: k.get_value::<String, _>("InstallLocation").map(|s| util::expand_env(s.trim_matches('"'))).unwrap_or_default(),
                icon: String::new(),
                removed_at: 0,
            });
        }
    }
    out
}

struct Owners {
    ids: Vec<(Ident, Keys)>,
    /// nhà phát hành -> số app đang cài của họ
    pubs: HashMap<String, usize>,
}

impl Owners {
    fn new(ids: Vec<Ident>) -> Self {
        let ids: Vec<(Ident, Keys)> = ids.into_iter().map(|i| {
            let k = keys(&i);
            (i, k)
        }).collect();
        let mut pubs = HashMap::new();
        for (_, k) in &ids {
            if let Some(p) = &k.publisher {
                *pubs.entry(p.clone()).or_insert(0) += 1;
            }
        }
        Owners { ids, pubs }
    }

    /// App đang cài (khác `except`) sở hữu thư mục này: khớp tên, hoặc thư mục nằm trong / chứa thư mục cài của nó.
    fn owner(&self, path: &str, f: &str, except: Option<&str>) -> Option<&str> {
        self.ids
            .iter()
            .filter(|(i, _)| except != Some(i.name.as_str()))
            .find(|(i, k)| matches(f, k) || (!i.location.is_empty() && i.location.len() > 3 && (inside(path, &i.location) || inside(&i.location, path))))
            .map(|(i, _)| i.name.as_str())
    }
}

/// Các thư mục của một app (theo dấu nhận diện) trong `ents`, kèm thư mục của nhà phát hành (cấp 2).
fn find_for(id: &Ident, k: &Keys, ents: &[Entry], owners: &Owners, buf: &mut [u64], out: &mut Vec<Found>) {
    let solo_pub = k.publisher.as_ref().filter(|p| owners.pubs.get(*p).copied().unwrap_or(0) <= 1);
    for e in ents {
        if !id.location.is_empty() && inside(&e.path, &id.location) {
            continue;
        }
        if matches(&e.norm, k) {
            let shared = owners.owner(&e.path, &e.norm, Some(&id.name)).map(str::to_owned);
            out.push(Found {
                path: e.path.clone(),
                kind: "folder",
                area: area_label(e.area),
                owner: id.name.clone(),
                reason: tr!("leftovers.reason.name_match", name = id.name),
                sure: shared.is_none(),
                shared,
                ..Default::default()
            });
        } else if k.publisher.as_deref() == Some(e.norm.as_str()) {
            // thư mục của nhà phát hành: chỉ mình app này -> cả thư mục; nhiều app -> xem từng thư mục con
            if solo_pub.is_some() && owners.owner(&e.path, &e.norm, Some(&id.name)).is_none() {
                out.push(Found {
                    path: e.path.clone(),
                    kind: "folder",
                    area: area_label(e.area),
                    owner: id.name.clone(),
                    reason: tr!("leftovers.reason.publisher_folder", publisher = id.publisher),
                    sure: false,
                    ..Default::default()
                });
            } else {
                subdirs(&e.path, buf, false, |path, name| {
                    let n = norm(&name);
                    if matches(&n, k) && !(!id.location.is_empty() && inside(&path, &id.location)) {
                        let shared = owners.owner(&path, &n, Some(&id.name)).map(str::to_owned);
                        out.push(Found {
                            path,
                            kind: "folder",
                            area: area_label(e.area),
                            owner: id.name.clone(),
                            reason: tr!("leftovers.reason.in_publisher_folder", publisher = id.publisher),
                            sure: shared.is_none(),
                            shared,
                            ..Default::default()
                        });
                    }
                });
            }
        }
    }
}

/// Bỏ mục nằm trong mục khác, bỏ trùng.
fn dedup(v: &mut Vec<Found>) {
    v.sort_by_key(|f| f.path.to_lowercase());
    let mut keep: Vec<Found> = Vec::with_capacity(v.len());
    for f in v.drain(..) {
        if f.kind == "folder" && keep.iter().any(|k| k.kind == "folder" && inside(&f.path, &k.path)) {
            continue;
        }
        keep.push(f);
    }
    *v = keep;
}

/// Đo dung lượng các thư mục (song song, có hạn), bỏ mục rỗng không đáng kể.
fn measure(v: &mut [Found], budget: Duration) {
    let until = Instant::now() + budget;
    let stop = AtomicBool::new(false);
    let next = AtomicUsize::new(0);
    let res: Mutex<Vec<(usize, apps::Walked)>> = Mutex::new(Vec::new());
    let paths: Vec<(usize, String)> = v.iter().enumerate().map(|(i, f)| (i, f.path.clone())).collect();
    std::thread::scope(|sc| {
        for _ in 0..MEASURE_WORKERS.min(paths.len()) {
            sc.spawn(|| {
                let mut buf = vec![0u64; du::BUF_U64];
                while let Some((i, p)) = paths.get(next.fetch_add(1, Ordering::Relaxed)) {
                    // đo rồi (≤ 10 phút, chưa bị xoá / đổi) -> dùng lại
                    let w = apps::cached_walk(p).unwrap_or_else(|| {
                        let w = apps::walk(p, until, &stop, &mut buf);
                        apps::remember_walk(p, &w);
                        w
                    });
                    if let Ok(mut r) = res.lock() {
                        r.push((*i, w));
                    }
                }
            });
        }
    });
    for (i, w) in res.into_inner().unwrap_or_default() {
        let f = &mut v[i];
        f.bytes = w.bytes;
        f.files = w.files;
        f.newest = w.newest;
        f.partial = !w.done;
    }
}

// ---------------- dữ liệu dùng chung (cache) ----------------
//
// Chủ sở hữu (mọi app đang cài + mục ẩn của registry) và danh sách thư mục cấp 1 của các nơi tìm: dựng một lần, dùng lại
// cho mọi app được xem trong 30 s. Danh sách app đọc lại (cài / gỡ, apps::generation đổi) hoặc vừa xoá thư mục
// (`invalidate`) -> dựng lại. Xem chi tiết từng app sau lần đầu chỉ còn so chuỗi trong bộ nhớ.

const CTX_TTL: Duration = Duration::from_secs(30);

struct Ctx {
    owners: Owners,
    ents: Vec<Entry>,
    gen: u64,
    at: Instant,
}

static CTX: Mutex<Option<Arc<Ctx>>> = Mutex::new(None);

/// Bỏ dữ liệu dùng chung (vừa xoá / gỡ gì đó).
pub fn invalidate() {
    if let Ok(mut g) = CTX.lock() {
        *g = None;
    }
}

fn ctx(list: &[App]) -> Arc<Ctx> {
    let gen = apps::generation();
    let mut g = CTX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = g.as_ref().filter(|c| c.gen == gen && c.at.elapsed() < CTX_TTL) {
        return c.clone();
    }
    let c = Arc::new(Ctx { owners: Owners::new(installed_idents(list)), ents: entries(), gen, at: Instant::now() });
    *g = Some(c.clone());
    c
}

// ---------------- thư mục liên quan của một app ----------------

/// Thư mục cài đặt + các thư mục liên quan của app `id`, đã đo. Thư mục cài đặt luôn đứng đầu.
pub fn related(id: &str) -> Result<Vec<Found>, String> {
    let list = apps::list().apps;
    let app = list.iter().find(|a| a.id == id).cloned().or_else(|| apps::find(id)).ok_or_else(|| tr!("apps.err.gone"))?;
    let cx = ctx(&list);
    let owners = &cx.owners;
    let mut out = Vec::new();
    let store = app.kind == "store";
    if !app.location.is_empty() && Path::new(&app.location).is_dir() {
        out.push(Found {
            path: app.location.clone(),
            kind: if store { "info" } else { "folder" },
            area: tr!("leftovers.area.install"),
            owner: app.name.clone(),
            reason: if store {
                tr!("leftovers.reason.store_install")
            } else if app.location_guessed {
                tr!("leftovers.reason.install_guessed")
            } else {
                tr!("leftovers.reason.install")
            },
            sure: !store && !app.location_guessed && apps::measurable(&app.location),
            ..Default::default()
        });
    }
    if store {
        if let Some(fam) = family_of(&app.key) {
            let p = format!("{}\\Packages\\{fam}", env("LOCALAPPDATA"));
            if Path::new(&p).is_dir() {
                out.push(Found { path: p, kind: "info", area: tr!("leftovers.area.store_data"), owner: app.name.clone(), reason: tr!("leftovers.reason.store_data"), ..Default::default() });
            }
        }
    }
    let ident = Ident::of(&app);
    let k = keys(&ident);
    let mut buf = vec![0u64; du::BUF_U64];
    find_for(&ident, &k, &cx.ents, owners, &mut buf, &mut out);
    let first = out.first().map(|f| f.path.clone());
    dedup(&mut out);
    // giữ thư mục cài đặt ở đầu
    if let Some(p) = first {
        if let Some(i) = out.iter().position(|f| f.path == p) {
            let f = out.remove(i);
            out.insert(0, f);
        }
    }
    measure(&mut out, RELATED_BUDGET);
    Ok(out)
}

/// Name_Version_Arch_Resource_PublisherId -> Name_PublisherId
pub(crate) fn family_of(full: &str) -> Option<String> {
    let p: Vec<&str> = full.split('_').collect();
    (p.len() == 5).then(|| format!("{}_{}", p[0], p[4]))
}

// ---------------- lịch sử app đã gỡ ----------------

fn seen_path() -> Option<std::path::PathBuf> {
    util::data_dir().map(|d| d.join("apps_seen.json"))
}
fn removed_path() -> Option<std::path::PathBuf> {
    util::data_dir().map(|d| d.join("apps_removed.json"))
}
fn load<T: for<'de> Deserialize<'de> + Default>(p: Option<std::path::PathBuf>) -> T {
    p.and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
fn save<T: Serialize>(p: Option<std::path::PathBuf>, v: &T) {
    if let (Some(p), Ok(b)) = (p, serde_json::to_vec(v)) {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, b);
    }
}

static HISTORY: Mutex<()> = Mutex::new(());

/// Gọi sau mỗi lần đọc danh sách: app có lần trước mà nay không còn -> ghi là đã gỡ.
pub fn note_seen(list: &[App]) {
    let _g = HISTORY.lock();
    let old: HashMap<String, Ident> = load(seen_path());
    let now: HashMap<String, Ident> = list.iter().map(|a| (a.id.clone(), Ident::of(a))).collect();
    let names: HashSet<&str> = list.iter().map(|a| a.name.as_str()).collect();
    let gone: Vec<Ident> = old.into_iter().filter(|(id, i)| !now.contains_key(id) && !names.contains(i.name.as_str())).map(|(_, i)| i).collect();
    if !gone.is_empty() {
        add_removed(gone);
    }
    save(seen_path(), &now);
}

fn add_removed(gone: Vec<Ident>) {
    let mut h: Vec<Ident> = load(removed_path());
    let t = chrono::Utc::now().timestamp_millis();
    for mut i in gone {
        h.retain(|x| x.name != i.name);
        i.removed_at = t;
        h.push(i);
    }
    let n = h.len();
    if n > HISTORY_MAX {
        h.drain(..n - HISTORY_MAX);
    }
    save(removed_path(), &h);
}

/// App vừa được gỡ qua trang Ứng dụng.
pub fn note_removed(app: &App) {
    let _g = HISTORY.lock();
    add_removed(vec![Ident::of(app)]);
}

/// Quên một app đã gỡ (người dùng bấm "Bỏ qua").
pub fn forget(name: &str) {
    let _g = HISTORY.lock();
    let mut h: Vec<Ident> = load(removed_path());
    h.retain(|x| x.name != name);
    save(removed_path(), &h);
}

// ---------------- phần còn sót ----------------

/// Gói Store của người dùng hiện tại: (tên đầy đủ, family) – mọi loại gói.
fn my_packages() -> Result<(HashSet<String>, HashSet<String>), String> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::{PackageManager, PackageTypes};
    let pm = PackageManager::new().map_err(|e| util::win_err(&e))?;
    let (mut full, mut fam) = (HashSet::new(), HashSet::new());
    for p in pm.FindPackagesByUserSecurityIdWithPackageTypes(&HSTRING::new(), PackageTypes(0x3f)).map_err(|e| util::win_err(&e))? {
        if let Ok(id) = p.Id() {
            if let Ok(f) = id.FullName() {
                full.insert(f.to_string().to_lowercase());
            }
            if let Ok(f) = id.FamilyName() {
                fam.insert(f.to_string().to_lowercase());
            }
        }
    }
    Ok((full, fam))
}

/// Gói của mọi người dùng (cần quyền Admin) và family của gói "cài sẵn cho tài khoản mới".
fn all_packages() -> Result<(HashSet<String>, HashSet<String>), String> {
    use windows::Management::Deployment::{PackageManager, PackageTypes};
    let pm = PackageManager::new().map_err(|e| util::win_err(&e))?;
    let mut full = HashSet::new();
    for p in pm.FindPackagesWithPackageTypes(PackageTypes(0x3f)).map_err(|e| util::win_err(&e))? {
        if let Ok(f) = p.Id().and_then(|i| i.FullName()) {
            full.insert(f.to_string().to_lowercase());
        }
    }
    let mut prov = HashSet::new();
    if let Ok(v) = pm.FindProvisionedPackages() {
        for p in v {
            if let Ok(f) = p.Id().and_then(|i| i.FamilyName()) {
                prov.insert(f.to_string().to_lowercase());
            }
        }
    }
    Ok((full, prov))
}

pub fn scan() -> ScanOut {
    let t0 = Instant::now();
    let list = apps::list().apps;
    note_seen(&list);
    let cx = ctx(&list);
    let (owners, ents) = (&cx.owners, &cx.ents);
    let removed: Vec<Ident> = load(removed_path());
    let mut out = Vec::new();
    let mut buf = vec![0u64; du::BUF_U64];

    // 1. thư mục của app đã gỡ (không còn app nào đang cài nhận)
    for id in &removed {
        let k = keys(id);
        let mut found = Vec::new();
        if !id.location.is_empty() && Path::new(&id.location).is_dir() && apps::measurable(&id.location) && owners.owner(&id.location, &norm(&id.location), None).is_none() {
            found.push(Found { path: id.location.clone(), kind: "folder", area: tr!("leftovers.area.old_install"), owner: id.name.clone(), reason: tr!("leftovers.reason.old_install"), sure: true, ..Default::default() });
        }
        find_for(id, &k, ents, owners, &mut buf, &mut found);
        for mut f in found {
            if f.shared.is_some() {
                continue; // app đang cài khác cũng dùng: không tính là sót
            }
            f.reason = tr!("leftovers.reason.removed_on", reason = f.reason, date = date(id.removed_at));
            out.push(f);
        }
    }

    // 2. dữ liệu app Store mồ côi
    let mut wa_error = None;
    match my_packages() {
        Ok((mine_full, mine_fam)) => {
            let pk = format!("{}\\Packages", env("LOCALAPPDATA"));
            subdirs(&pk, &mut buf, false, |path, name| {
                let l = name.to_lowercase();
                // family: Name_PublisherId (13 ký tự) – bỏ thư mục khác
                if l.rsplit_once('_').is_some_and(|(_, p)| p.len() == 13) && !mine_fam.contains(&l) {
                    let owner = name.split('_').next().unwrap_or(&name).to_owned();
                    out.push(Found { path, kind: "folder", area: tr!("leftovers.area.store_data"), owner, reason: tr!("leftovers.reason.orphan_store_data"), sure: false, ..Default::default() });
                }
            });
            // 3. WindowsApps
            match all_packages() {
                Ok((all, prov)) => {
                    let wa = format!("{}\\WindowsApps", env("ProgramFiles"));
                    subdirs(&wa, &mut buf, false, |path, name| {
                        let l = name.to_lowercase();
                        if matches!(l.as_str(), "deleted" | "deletedalluserpackages") {
                            out.push(Found { path, kind: "info", area: "WindowsApps".into(), owner: "Windows".into(), reason: tr!("leftovers.reason.wa_deleted"), ..Default::default() });
                            return;
                        }
                        let Some(fam) = family_of(&l) else { return };
                        if mine_full.contains(&l) {
                            return;
                        }
                        let owner = name.split('_').next().unwrap_or(&name).to_owned();
                        let (kind, reason) = if all.contains(&l) {
                            ("package", "leftovers.reason.wa_other_users")
                        } else if prov.contains(&fam) {
                            ("info", "leftovers.reason.wa_provisioned")
                        } else if mine_fam.contains(&fam) {
                            ("info", "leftovers.reason.wa_old_version")
                        } else {
                            ("info", "leftovers.reason.wa_orphan")
                        };
                        let package = (kind == "package").then(|| name.clone());
                        out.push(Found { path, package, kind, area: "WindowsApps".into(), owner, reason: tr!(reason), ..Default::default() });
                    });
                }
                Err(e) => wa_error = Some(e),
            }
        }
        Err(e) => wa_error = Some(e),
    }

    dedup(&mut out);
    measure(&mut out, SCAN_BUDGET.saturating_sub(t0.elapsed()).max(Duration::from_secs(3)));
    let now = chrono::Utc::now().timestamp_millis();
    // dữ liệu app Store mồ côi = thư mục trong %LOCALAPPDATA%\Packages (so theo đường dẫn, không theo nhãn đã dịch)
    let store_dir = format!("{}\\Packages\\", env("LOCALAPPDATA")).to_lowercase();
    for f in &mut out {
        if f.kind == "folder" && f.path.to_lowercase().starts_with(&store_dir) {
            f.sure = f.newest > 0 && now - f.newest > OLD_MS;
        }
    }
    // bỏ mục rỗng (trừ gói / thông tin)
    out.retain(|f| f.kind != "folder" || f.bytes > 0 || f.files > 0);
    out.sort_by_key(|f| std::cmp::Reverse(f.bytes));
    ScanOut { items: out, took_ms: t0.elapsed().as_millis() as u64, removed_known: removed.len(), windowsapps_error: wa_error }
}

fn date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|t| t.format("%d/%m/%Y").to_string()).unwrap_or_else(|| "?".into())
}

/// Gỡ gói chỉ còn ở tài khoản khác (cho mọi tài khoản). Chỉ nhận gói KHÔNG cài cho tài khoản này.
pub fn remove_packages(names: &[String]) -> Vec<(String, Result<(), String>)> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::{PackageManager, RemovalOptions};
    let mine = my_packages().map(|m| m.0).unwrap_or_default();
    let all = all_packages().map(|a| a.0).unwrap_or_default();
    names
        .iter()
        .take(100)
        .map(|n| {
            let l = n.to_lowercase();
            let r = if mine.contains(&l) {
                Err(tr!("leftovers.err.package_mine"))
            } else if !all.contains(&l) {
                Err(tr!("leftovers.err.package_gone"))
            } else {
                PackageManager::new()
                    .and_then(|pm| pm.RemovePackageWithOptionsAsync(&HSTRING::from(n.as_str()), RemovalOptions::RemoveForAllUsers))
                    .and_then(|op| op.join())
                    .map_err(|e| util::win_err(&e))
                    .and_then(|r| {
                        let code = r.ExtendedErrorCode().map(|h| h.0).unwrap_or(0);
                        if code < 0 { Err(r.ErrorText().map(|h| h.to_string()).unwrap_or_else(|_| tr!("errors.code", code = format!("{code:08X}")))) } else { Ok(()) }
                    })
            };
            util::log_line(&format!("[Còn sót] Gỡ gói {n} cho mọi tài khoản: {r:?}"));
            (n.clone(), r)
        })
        .collect()
}

/// Cho trợ lý AI: phần còn sót (đã quét).
pub fn ai_report(scan: &ScanOut) -> String {
    use std::fmt::Write;
    let mut s = format!("{} mục còn sót sau khi gỡ (đã nhớ {} app đã gỡ). Thư mục xoá được bằng khối `xoa`; gói WindowsApps người dùng gỡ trên trang Ứng dụng.\n", scan.items.len(), scan.removed_known);
    for f in &scan.items {
        let line = format!(
            "- [{}] {} · {} · {}{} · {} · {}{}\n",
            match f.kind { "package" => "gói", "info" => "chỉ báo", _ => "thư mục" },
            f.path,
            f.owner,
            if f.partial { "≥" } else { "" },
            du::sz(f.bytes),
            f.area,
            f.reason,
            if f.newest > 0 { format!(" · sửa lần cuối {}", date(f.newest)) } else { String::new() },
        );
        if s.len() + line.len() > 5800 {
            s.push_str("…\n");
            break;
        }
        let _ = write!(s, "{line}");
    }
    if let Some(e) = &scan.windowsapps_error {
        let _ = writeln!(s, "(Không xem được WindowsApps: {e})");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_matching() {
        let id = Ident { name: "LDPlayer 14".into(), publisher: "XUANZHI INTERNATIONAL CO., LIMITED".into(), location: r"C:\LDPlayer\LDPlayer9".into(), icon: r"C:\LDPlayer\LDPlayer9\dnplayer.exe".into(), removed_at: 0 };
        let k = keys(&id);
        assert!(matches("ldplayer9", &k) && matches("ldplayer", &k) && matches("dnplayer", &k));
        assert!(!matches("ldmultiplayer", &k) && !matches("player", &k));
        assert_eq!(k.publisher.as_deref(), Some("xuanzhi"));
        let vs = keys(&Ident { name: "Microsoft Visual Studio Code (User)".into(), publisher: "Microsoft Corporation".into(), icon: r"C:\x\Code.exe".into(), ..Default::default() });
        assert!(matches("code", &vs) && matches("visualstudiocode", &vs));
        assert!(!matches("visualstudio", &vs) && !matches("microsoft", &vs));
        assert_eq!(family_of("Microsoft.WindowsCalculator_11.2411.1.0_x64__8wekyb3d8bbwe").as_deref(), Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe"));
    }

    /// Chạy thật trên máy này (chỉ đọc): thư mục liên quan của vài app + phần còn sót.
    #[test]
    #[ignore]
    fn real_related_and_scan() {
        let list = apps::list().apps;
        for a in list.iter().filter(|a| a.kind == "win32").take(6) {
            let t = Instant::now();
            let r = related(&a.id).unwrap();
            println!("== {} ({} ms)", a.name, t.elapsed().as_millis());
            for f in r {
                println!("   {} | {} | {} | sure={} shared={:?}", f.area, f.path, du::sz(f.bytes), f.sure, f.shared);
            }
        }
        let t = Instant::now();
        let s = scan();
        println!("== còn sót: {} mục, {} ms, lỗi WA: {:?}", s.items.len(), t.elapsed().as_millis(), s.windowsapps_error);
        for f in s.items.iter().take(15) {
            println!("   [{}] {} | {} | {} | {}", f.kind, f.area, f.path, du::sz(f.bytes), f.reason);
        }
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    /// Lần đầu (đọc đầy đủ) so với lần sau (dùng cache) – chạy tay: cargo test --release --lib cache_timing -- --ignored --nocapture
    #[test]
    #[ignore]
    fn cache_timing() {
        let t = Instant::now();
        let n = apps::list().apps.len();
        println!("list lần 1: {n} app, {} ms", t.elapsed().as_millis());
        let t = Instant::now();
        let _ = apps::list();
        println!("list lần 2 (cache): {} µs", t.elapsed().as_micros());
        let ids: Vec<String> = apps::list().apps.iter().filter(|a| a.kind == "win32").take(5).map(|a| a.id.clone()).collect();
        for (i, id) in ids.iter().enumerate() {
            let t = Instant::now();
            let r = related(id).unwrap();
            println!("related #{i}: {} mục, {} ms", r.len(), t.elapsed().as_millis());
        }
        let t = Instant::now();
        let _ = related(&ids[0]).unwrap();
        println!("related #0 lần 2 (đo đã nhớ): {} ms", t.elapsed().as_millis());
    }
}
