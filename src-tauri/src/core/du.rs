// Phân tích dung lượng ổ đĩa / thư mục (kiểu TreeSize), tối ưu cho tốc độ và RAM:
//
// - Liệt kê mỗi thư mục bằng GetFileInformationByHandleEx(FileFullDirectoryInfo) với bộ đệm 64 KB:
//   một lần gọi trả về hàng trăm mục kèm kích thước, dung lượng trên đĩa, thuộc tính, thời gian
//   -> không mở từng tệp, không gọi GetFileAttributes/metadata riêng.
// - Nhiều luồng (≤ 6 – điểm nhanh nhất khi đo) lấy việc từ một ngăn xếp chung (LIFO
//   -> hàng đợi nhỏ). Mỗi thư mục chỉ khoá cây MỘT lần để ghi kết quả và cộng dồn lên các thư mục cha,
//   nên giao diện đọc được cây đang lớn dần bất cứ lúc nào (hiện ngay trong lúc quét).
// - Chỉ lưu THƯ MỤC trong RAM (nút 56 byte, xem phần "cây"); tệp của một thư mục chỉ được liệt kê khi người
//   dùng mở mục "[N tệp]" (list_files), nên quét cả ổ hàng trăm nghìn tệp vẫn nhẹ.
// - Không đi theo junction / symlink (không đếm trùng, không lặp vô hạn) – giống TreeSize.
// - Đường dẫn dạng \\?\ để đọc được thư mục sâu > 260 ký tự.

use std::ffi::c_void;
use crate::tr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *const c_void, disp: u32, flags: u32, tmpl: *mut c_void) -> *mut c_void;
    fn GetFileInformationByHandleEx(h: *mut c_void, class: i32, buf: *mut c_void, size: u32) -> i32;
    fn CloseHandle(h: *mut c_void) -> i32;
    fn GetLastError() -> u32;
    fn SetThreadErrorMode(mode: u32, old: *mut u32) -> i32;
    fn GetLogicalDrives() -> u32;
    fn QueryDosDeviceW(name: *const u16, target: *mut u16, max: u32) -> u32;
    fn GetDriveTypeW(root: *const u16) -> u32;
    fn GetVolumeInformationW(root: *const u16, name: *mut u16, name_len: u32, serial: *mut u32, max_comp: *mut u32, flags: *mut u32, fs: *mut u16, fs_len: u32) -> i32;
    fn GetDiskFreeSpaceExW(root: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
}

const FILE_LIST_DIRECTORY: u32 = 0x1;
const SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FULL_DIRECTORY_INFO: i32 = 14;
const FILE_STANDARD_INFO: i32 = 1;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const ERROR_NO_MORE_FILES: u32 = 18;
const ATTR_DIRECTORY: u32 = 0x10;
const ATTR_REPARSE: u32 = 0x400;
const SEM_FAILCRITICALERRORS: u32 = 0x1;
const NONE: u32 = u32::MAX;
pub(crate) const BUF_U64: usize = 64 * 1024 / 8;
const MAX_WORKERS: usize = 6;

/// Mã lỗi Win32 -> thông báo tiếng Việt (dùng chung util::win_err).
fn err_text(e: u32) -> String {
    super::util::win_err(&windows::core::Error::from(windows::core::HRESULT::from_win32(e)))
}

/// FILETIME (100 ns từ 1601) -> ms Unix.
fn ft_ms(ft: i64) -> i64 {
    if ft <= 0 { 0 } else { ft / 10_000 - 11_644_473_600_000 }
}

/// Đường dẫn người dùng -> dạng \\?\ (UTF-16, không có NUL cuối).
pub(crate) fn long_path(p: &str) -> Vec<u16> {
    let p = p.replace('/', "\\");
    let s = if p.starts_with(r"\\?\") {
        p
    } else if let Some(unc) = p.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{unc}")
    } else {
        format!(r"\\?\{p}")
    };
    s.encode_utf16().collect()
}

fn join_w(base: &[u16], name: &[u16]) -> Vec<u16> {
    let mut v = Vec::with_capacity(base.len() + 1 + name.len() + 1);
    v.extend_from_slice(base);
    if v.last() != Some(&(b'\\' as u16)) {
        v.push(b'\\' as u16);
    }
    v.extend_from_slice(name);
    v
}

pub(crate) struct Entry<'a> {
    pub name: &'a [u16],
    pub size: u64,
    pub alloc: u64,
    pub attrs: u32,
    /// lần sửa cuối, ms Unix
    pub mtime: i64,
}

/// Liệt kê một thư mục (đường dẫn kết thúc bằng NUL). Err(mã lỗi Win32) nếu không mở được.
pub(crate) fn list_dir(path_nul: &[u16], buf: &mut [u64], f: impl FnMut(&Entry)) -> Result<(), u32> {
    unsafe {
        let h = CreateFileW(path_nul.as_ptr(), FILE_LIST_DIRECTORY, SHARE_ALL, std::ptr::null(), OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, std::ptr::null_mut());
        if h as isize == -1 {
            return Err(GetLastError());
        }
        let result = list_handle(h, buf, f);
        CloseHandle(h);
        result
    }
}

/// Liệt kê thư mục đã mở sẵn (handle đồng bộ, có quyền FILE_LIST_DIRECTORY). Không đóng handle.
///
/// # Safety
/// `h` phải là handle thư mục hợp lệ, mở ở chế độ I/O đồng bộ.
pub(crate) unsafe fn list_handle(h: *mut c_void, buf: &mut [u64], mut f: impl FnMut(&Entry)) -> Result<(), u32> {
    unsafe {
        let bytes = buf.len() * 8;
        let base = buf.as_mut_ptr() as *mut u8;
        let mut result = Ok(());
        loop {
            if GetFileInformationByHandleEx(h, FILE_FULL_DIRECTORY_INFO, base as *mut c_void, bytes as u32) == 0 {
                let e = GetLastError();
                if e != ERROR_NO_MORE_FILES {
                    result = Err(e);
                }
                break;
            }
            let mut off = 0usize;
            loop {
                let p = base.add(off);
                let rd_u32 = |o: usize| (p.add(o) as *const u32).read_unaligned();
                let rd_i64 = |o: usize| (p.add(o) as *const i64).read_unaligned();
                let next = rd_u32(0) as usize;
                let name_len = rd_u32(60) as usize / 2;
                if off + 68 + name_len * 2 > bytes {
                    break; // dữ liệu hỏng: dừng an toàn
                }
                let name = std::slice::from_raw_parts(p.add(68) as *const u16, name_len);
                let dot = name == [b'.' as u16] || name == [b'.' as u16, b'.' as u16];
                if !dot {
                    f(&Entry {
                        name,
                        size: rd_i64(40).max(0) as u64,
                        alloc: rd_i64(48).max(0) as u64,
                        attrs: rd_u32(56),
                        mtime: ft_ms(rd_i64(24)),
                    });
                }
                if next == 0 {
                    break;
                }
                off += next;
                if off >= bytes {
                    break;
                }
            }
        }
        result
    }
}

// ---------------- cây ----------------
//
// Bố cục gọn cho cây hàng trăm nghìn thư mục (đo ổ C 595k thư mục: 129 MB với Vec<Dir>+Box<str>+Vec con
// -> ~40 MB với bố cục này):
// - nút 56 byte, con nối bằng first/next (không Vec con, không cấp phát riêng mỗi nút);
// - tên nằm chung trong các khối 256 KB (không malloc mỗi tên);
// - nút cấp theo khối cố định 16k -> không bao giờ realloc/chép cả cây trong lúc giữ khoá (không khựng);
// - số liệu "tệp trực tiếp" không lưu: = tổng của nút − tổng các con (luôn đúng vì mọi lần cộng đều lan lên cha).

const CHUNK: usize = 16 * 1024;
const NAME_BITS: u32 = 18;
const NAME_CHUNK: usize = 1 << NAME_BITS;
const F_DENIED: u8 = 1;
const F_LINK: u8 = 2;
const F_REMOVED: u8 = 4;
/// các tệp TRỰC TIẾP của thư mục đã được liệt kê và cộng vào cây (thư mục con có thể còn đang quét)
const F_LISTED: u8 = 8;

#[derive(Clone, Copy)]
struct Dir {
    /// tổng đệ quy
    size: u64,
    alloc: u64,
    parent: u32,
    first: u32,
    next: u32,
    files: u32,
    dirs: u32,
    /// 1 (chưa liệt kê chính nó) + số thư mục con chưa xong; 0 = số liệu đã cuối cùng
    left: u32,
    /// giây Unix
    mtime: u32,
    name_off: u32,
    name_len: u16,
    flags: u8,
}

#[derive(Default)]
struct Tree {
    chunks: Vec<Vec<Dir>>,
    names: Vec<Vec<u8>>,
    len: u32,
}

struct Own {
    size: u64,
    alloc: u64,
    files: u32,
    kids: u32,
}

impl Tree {
    fn get(&self, id: u32) -> &Dir {
        &self.chunks[id as usize / CHUNK][id as usize % CHUNK]
    }
    fn get_mut(&mut self, id: u32) -> &mut Dir {
        &mut self.chunks[id as usize / CHUNK][id as usize % CHUNK]
    }
    /// Nút còn nằm trong cây (chính nó và mọi tổ tiên chưa bị gỡ). Nhánh đã gỡ không còn trả số liệu cũ ra giao diện.
    fn valid(&self, id: u32) -> bool {
        id < self.len && !self.detached(id)
    }
    fn push_name(&mut self, s: &str) -> (u32, u16) {
        let b = &s.as_bytes()[..s.len().min(u16::MAX as usize)];
        if self.names.last().is_none_or(|c| c.len() + b.len() > NAME_CHUNK) {
            self.names.push(Vec::with_capacity(NAME_CHUNK.max(b.len())));
        }
        let ci = self.names.len() - 1;
        let c = &mut self.names[ci];
        let pos = c.len();
        c.extend_from_slice(b);
        (((ci as u32) << NAME_BITS) | pos as u32, b.len() as u16)
    }
    fn name(&self, id: u32) -> &str {
        let d = self.get(id);
        let ci = (d.name_off >> NAME_BITS) as usize;
        let pos = (d.name_off as usize) & (NAME_CHUNK - 1);
        std::str::from_utf8(&self.names[ci][pos..pos + d.name_len as usize]).unwrap_or("?")
    }
    fn add(&mut self, parent: u32, name: &str, mtime: u32, link: bool) -> u32 {
        let (name_off, name_len) = self.push_name(name);
        if self.chunks.last().is_none_or(|c| c.len() == CHUNK) {
            self.chunks.push(Vec::with_capacity(CHUNK));
        }
        let id = self.len;
        self.len += 1;
        let next = if parent == NONE { NONE } else { self.get(parent).first };
        self.chunks.last_mut().unwrap().push(Dir {
            size: 0,
            alloc: 0,
            parent,
            first: NONE,
            next,
            files: 0,
            dirs: 0,
            left: u32::from(!link),
            mtime,
            name_off,
            name_len,
            flags: if link { F_LINK } else { 0 },
        });
        if parent != NONE {
            self.get_mut(parent).first = id;
        }
        id
    }
    fn kids(&self, id: u32) -> impl Iterator<Item = u32> + '_ {
        let mut cur = self.get(id).first;
        std::iter::from_fn(move || {
            (cur != NONE).then(|| {
                let c = cur;
                cur = self.get(c).next;
                c
            })
        })
    }
    fn own(&self, id: u32) -> Own {
        let d = self.get(id);
        let (mut s, mut a, mut f, mut k) = (0u64, 0u64, 0u32, 0u32);
        for c in self.kids(id) {
            let x = self.get(c);
            s += x.size;
            a += x.alloc;
            f += x.files;
            k += 1;
        }
        Own { size: d.size.saturating_sub(s), alloc: d.alloc.saturating_sub(a), files: d.files.saturating_sub(f), kids: k }
    }
    /// Có tổ tiên (hoặc chính nó) đã bị xoá khỏi cây không.
    fn detached(&self, mut id: u32) -> bool {
        while id != NONE {
            let d = self.get(id);
            if d.flags & F_REMOVED != 0 {
                return true;
            }
            id = d.parent;
        }
        false
    }
    /// Cộng (hoặc trừ, khi `sub`) vào thư mục và mọi thư mục cha.
    fn bubble(&mut self, mut id: u32, size: u64, alloc: u64, files: u32, dirs: u32, sub: bool) {
        while id != NONE {
            let d = self.get_mut(id);
            if sub {
                d.size = d.size.saturating_sub(size);
                d.alloc = d.alloc.saturating_sub(alloc);
                d.files = d.files.saturating_sub(files);
                d.dirs = d.dirs.saturating_sub(dirs);
            } else {
                d.size += size;
                d.alloc += alloc;
                d.files = d.files.saturating_add(files);
                d.dirs = d.dirs.saturating_add(dirs);
            }
            id = d.parent;
        }
    }
    /// Cộng một độ chênh CÓ DẤU vào thư mục và mọi thư mục cha.
    fn bubble_delta(&mut self, mut id: u32, size: i64, alloc: i64, files: i64, dirs: i64) {
        let cl = |v: u32, d: i64| (i64::from(v) + d).clamp(0, i64::from(u32::MAX)) as u32;
        while id != NONE {
            let d = self.get_mut(id);
            d.size = d.size.saturating_add_signed(size);
            d.alloc = d.alloc.saturating_add_signed(alloc);
            d.files = cl(d.files, files);
            d.dirs = cl(d.dirs, dirs);
            id = d.parent;
        }
    }
    /// Một phần việc của `id` xong: giảm `left`, lan lên cha khi về 0.
    fn finish(&mut self, mut id: u32) {
        while id != NONE {
            let d = self.get_mut(id);
            d.left = d.left.saturating_sub(1);
            if d.left > 0 {
                return;
            }
            id = d.parent;
        }
    }
    fn path(&self, mut id: u32) -> String {
        let mut parts = Vec::new();
        while id != NONE {
            parts.push(self.name(id));
            id = self.get(id).parent;
        }
        let mut s = String::new();
        for p in parts.iter().rev() {
            if !s.is_empty() && !s.ends_with('\\') {
                s.push('\\');
            }
            s.push_str(p);
        }
        s
    }
    /// Tên của `id` và các thư mục cha khớp `comps` (tính từ cuối đường dẫn lên, không phân biệt hoa thường ASCII).
    fn ends_with(&self, mut id: u32, comps: &[&str]) -> bool {
        for c in comps {
            if id == NONE || id == 0 || !self.name(id).eq_ignore_ascii_case(c) {
                return false;
            }
            id = self.get(id).parent;
        }
        true
    }
    /// Đường dẫn -> id (gốc của cây là `root`).
    fn find(&self, root: &str, path: &str) -> Option<u32> {
        let norm = |p: &str| p.trim().trim_matches('"').replace('/', "\\").trim_end_matches('\\').to_lowercase();
        let (root_n, path_n) = (norm(root), norm(path));
        let rest = path_n.strip_prefix(&root_n)?;
        if !rest.is_empty() && !rest.starts_with('\\') {
            return None; // "C:\Users2" không nằm trong "C:\Users"
        }
        let mut id = 0u32;
        for part in rest.split('\\').filter(|x| !x.is_empty()) {
            id = self.kids(id).find(|&c| self.get(c).flags & F_REMOVED == 0 && self.name(c).to_lowercase() == part)?;
        }
        Some(id)
    }
    fn unlink(&mut self, id: u32) {
        let parent = self.get(id).parent;
        let next = self.get(id).next;
        if parent == NONE {
            return;
        }
        if self.get(parent).first == id {
            self.get_mut(parent).first = next;
            return;
        }
        let mut cur = self.get(parent).first;
        while cur != NONE {
            let nx = self.get(cur).next;
            if nx == id {
                self.get_mut(cur).next = next;
                return;
            }
            cur = nx;
        }
    }
}

/// Ghép con của nút `m` (cây chính `t`) với con của nút `sn` (cây phụ `st`) theo tên:
/// (cặp còn tồn tại, nút phụ mới chưa có ở cây chính, nút chính đã biến mất).
fn match_kids(t: &Tree, st: &Tree, m: u32, sn: u32) -> (Vec<(u32, u32)>, Vec<u32>, Vec<u32>) {
    let mut by_name: std::collections::HashMap<&str, u32> = t.kids(m).map(|c| (t.name(c), c)).collect();
    let (mut matched, mut fresh) = (Vec::new(), Vec::new());
    for sc in st.kids(sn) {
        match by_name.remove(st.name(sc)) {
            Some(mc) => matched.push((mc, sc)),
            None => fresh.push(sc),
        }
    }
    (matched, fresh, by_name.into_values().collect())
}

/// Nhánh `id` của `st` có thư mục không đọc được không.
fn has_denied(st: &Tree, id: u32) -> bool {
    let mut stack = vec![id];
    while let Some(c) = stack.pop() {
        if st.get(c).flags & F_DENIED != 0 {
            return true;
        }
        stack.extend(st.kids(c));
    }
    false
}

fn secs(ms: i64) -> u32 {
    (ms / 1000).clamp(0, i64::from(u32::MAX)) as u32
}

struct Job {
    id: u32,
    path: Vec<u16>,
}

struct Queue {
    jobs: Vec<Job>,
    busy: usize,
}

pub struct Scan {
    pub id: u64,
    pub root: String,
    tree: Mutex<Tree>,
    queue: Mutex<Queue>,
    wake: Condvar,
    cancel: AtomicBool,
    done: AtomicBool,
    started: Instant,
    elapsed_ms: AtomicU64,
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    errors: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Serialize, Clone)]
pub struct Progress {
    pub scan: u64,
    pub root: String,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    pub done: bool,
    pub cancelled: bool,
}

#[derive(Serialize)]
pub struct NodeOut {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub alloc: u64,
    pub files: u64,
    pub dirs: u32,
    pub own_files: u32,
    pub own_size: u64,
    pub own_alloc: u64,
    pub mtime: i64,
    /// số liệu đã cuối cùng (thư mục và mọi thư mục con đã quét xong)
    pub done: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub denied: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub link: bool,
    /// số thư mục con trực tiếp
    pub kids: u32,
}

#[derive(Serialize)]
pub struct ChildrenOut {
    pub node: NodeOut,
    pub path: String,
    pub children: Vec<NodeOut>,
    /// số thư mục con không trả về (ngoài `limit`)
    pub more: u32,
}

#[derive(Serialize)]
pub struct FileOut {
    pub name: String,
    pub size: u64,
    pub alloc: u64,
    pub mtime: i64,
}

#[derive(Serialize)]
pub struct FilesOut {
    pub total: u32,
    pub items: Vec<FileOut>,
}

/// Kết quả `Scan::sync_dir` (đưa số liệu một nhánh về đúng với đĩa).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncResult {
    /// không còn trên đĩa (hoặc đã không còn trong cây): nhánh được gỡ, số liệu cha đã trừ
    Gone,
    /// còn trên đĩa: nhánh đã đo lại, phần chênh đã áp lên các thư mục cha
    Updated,
    /// lần quét còn đang chạy ở nhánh này: không làm gì (lần quét sẽ đọc trạng thái hiện tại)
    Busy,
    /// không đo được (quá lớn so với thời gian cho phép / không đọc được): số liệu có thể cũ
    Stale,
}

/// Đo lại một nhánh tối đa bấy lâu; quá hạn thì giữ số cũ và báo `Stale` (không bao giờ quét lại cả ổ).
pub const SYNC_BUDGET: Duration = Duration::from_secs(8);

impl Scan {
    /// Bắt đầu quét `root` ở nền; trả về ngay.
    pub fn start(id: u64, root: &str) -> Result<Arc<Scan>, String> {
        let mut root = root.trim().trim_matches('"').replace('/', "\\");
        if root.len() == 2 && root.ends_with(':') {
            root.push('\\');
        }
        if root.len() > 3 && root.ends_with('\\') {
            root.pop();
        }
        let wroot = long_path(&root);
        let mut probe = wroot.clone();
        probe.push(0);
        let mut buf = vec![0u64; 512];
        // thử mở trước để báo lỗi rõ ràng (không tồn tại / không có quyền)
        if let Err(e) = list_dir(&probe, &mut buf, |_| {}) {
            if e != ERROR_NO_MORE_FILES {
                return Err(err_text(e));
            }
        }
        let mut tree = Tree::default();
        tree.add(NONE, &root, 0, false);
        let scan = Arc::new(Scan {
            id,
            root,
            tree: Mutex::new(tree),
            queue: Mutex::new(Queue { jobs: vec![Job { id: 0, path: wroot }], busy: 0 }),
            wake: Condvar::new(),
            cancel: AtomicBool::new(false),
            done: AtomicBool::new(false),
            started: Instant::now(),
            elapsed_ms: AtomicU64::new(0),
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        });
        let s = scan.clone();
        std::thread::Builder::new()
            .name("du-scan".into())
            .spawn(move || s.run())
            .map_err(|e| tr!("du.err.thread", e = e))?;
        Ok(scan)
    }

    fn run(self: Arc<Self>) {
        // Đo trên SSD (817k tệp / 130k thư mục): 1 luồng 18 s, 4 luồng 5,5 s, 6 luồng 4,8 s, 8+ luồng chậm lại
        // (nghẽn trong NTFS / bộ lọc Defender). Phần lớn thời gian nằm trong kernel nên không giành CPU của giao diện.
        let n = std::thread::available_parallelism().map_or(2, |n| n.get() / 2).clamp(2, MAX_WORKERS);
        std::thread::scope(|sc| {
            for _ in 0..n {
                let me = &*self;
                let _ = std::thread::Builder::new().name("du-worker".into()).stack_size(256 * 1024).spawn_scoped(sc, move || me.work());
            }
        });
        self.elapsed_ms.store(self.started.elapsed().as_millis() as u64, Ordering::Relaxed);
        self.done.store(true, Ordering::Release);
    }

    fn take(&self) -> Option<Job> {
        let mut q = lock(&self.queue);
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                q.jobs.clear();
                self.wake.notify_all();
                return None;
            }
            if let Some(j) = q.jobs.pop() {
                q.busy += 1;
                return Some(j);
            }
            if q.busy == 0 {
                self.wake.notify_all();
                return None;
            }
            q = self.wake.wait(q).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn work(&self) {
        unsafe {
            SetThreadErrorMode(SEM_FAILCRITICALERRORS, std::ptr::null_mut());
        }
        let mut buf = vec![0u64; BUF_U64];
        // thư mục con của thư mục đang liệt kê: tên nằm liền trong một bộ đệm dùng lại (không cấp phát mỗi tên)
        let mut sub_names: Vec<u16> = Vec::new();
        let mut subs: Vec<(usize, usize, i64, bool)> = Vec::new();
        let mut name = String::new();
        while let Some(job) = self.take() {
            let mut path = job.path;
            path.push(0);
            subs.clear();
            sub_names.clear();
            let (mut nf, mut sz, mut al) = (0u32, 0u64, 0u64);
            let res = list_dir(&path, &mut buf, |e| {
                if e.attrs & ATTR_DIRECTORY != 0 {
                    subs.push((sub_names.len(), e.name.len(), e.mtime, e.attrs & ATTR_REPARSE != 0));
                    sub_names.extend_from_slice(e.name);
                } else {
                    nf += 1;
                    sz += e.size;
                    al += e.alloc;
                }
            });
            path.pop();
            if res.is_err() {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
            self.files.fetch_add(u64::from(nf), Ordering::Relaxed);
            self.bytes.fetch_add(sz, Ordering::Relaxed);
            self.dirs.fetch_add(1, Ordering::Relaxed);

            let mut jobs = Vec::new();
            {
                let mut t = lock(&self.tree);
                // thư mục đã bị xoá khỏi cây trong lúc quét: bỏ kết quả
                if !t.detached(job.id) {
                    let id = job.id;
                    if res.is_err() {
                        t.get_mut(id).flags |= F_DENIED;
                    }
                    t.bubble(id, sz, al, nf, subs.len() as u32, false);
                    t.get_mut(id).flags |= F_LISTED;
                    for &(start, len, mtime, link) in &subs {
                        let w = &sub_names[start..start + len];
                        name.clear();
                        name.extend(char::decode_utf16(w.iter().copied()).map(|r| r.unwrap_or('\u{FFFD}')));
                        let cid = t.add(id, &name, secs(mtime), link);
                        if !link {
                            t.get_mut(id).left += 1;
                            jobs.push(Job { id: cid, path: join_w(&path, w) });
                        }
                    }
                    t.finish(id);
                }
            }
            let mut q = lock(&self.queue);
            let more = jobs.len();
            q.jobs.append(&mut jobs);
            q.busy -= 1;
            if q.busy == 0 && q.jobs.is_empty() {
                self.wake.notify_all(); // hết việc: cho mọi luồng thoát
            } else {
                // chỉ đánh thức đủ số luồng cho việc mới (tránh đánh thức cả đàn)
                for _ in 0..more.min(MAX_WORKERS) {
                    self.wake.notify_one();
                }
            }
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _q = lock(&self.queue);
        self.wake.notify_all();
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    pub fn progress(&self) -> Progress {
        let done = self.is_done();
        Progress {
            scan: self.id,
            root: self.root.clone(),
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            elapsed_ms: if done { self.elapsed_ms.load(Ordering::Relaxed) } else { self.started.elapsed().as_millis() as u64 },
            done,
            cancelled: self.cancel.load(Ordering::Relaxed),
        }
    }

    fn out(t: &Tree, id: u32) -> NodeOut {
        let d = t.get(id);
        let own = t.own(id);
        NodeOut {
            id,
            name: t.name(id).to_owned(),
            size: d.size,
            alloc: d.alloc,
            files: u64::from(d.files),
            dirs: d.dirs,
            own_files: own.files,
            own_size: own.size,
            own_alloc: own.alloc,
            mtime: i64::from(d.mtime) * 1000,
            done: d.left == 0,
            denied: d.flags & F_DENIED != 0,
            link: d.flags & F_LINK != 0,
            kids: own.kids,
        }
    }

    /// Thư mục `id` + tối đa `limit` thư mục con, sắp theo `sort` (size|alloc|files|dirs|name|mtime).
    /// Chỉ chọn top-k (select_nth) nên thư mục có hàng chục nghìn thư mục con vẫn rẻ.
    pub fn children(&self, id: u32, sort: &str, desc: bool, limit: usize) -> Option<ChildrenOut> {
        let t = lock(&self.tree);
        if !t.valid(id) {
            return None;
        }
        let total;
        let ids: Vec<u32> = if sort == "name" {
            // khoá tên viết thường tính MỘT lần mỗi con (không cấp phát trong lúc so sánh)
            let mut v: Vec<(String, u32)> = t.kids(id).map(|c| (t.name(c).to_lowercase(), c)).collect();
            total = v.len();
            v.sort_unstable_by(|a, b| if desc { b.0.cmp(&a.0) } else { a.0.cmp(&b.0) });
            v.truncate(limit.max(1));
            v.into_iter().map(|x| x.1).collect()
        } else {
            let mut v: Vec<u32> = t.kids(id).collect();
            total = v.len();
            let val = |c: u32| {
                let x = t.get(c);
                match sort {
                    "alloc" => x.alloc,
                    "files" => u64::from(x.files),
                    "dirs" => u64::from(x.dirs),
                    "mtime" => u64::from(x.mtime),
                    _ => x.size,
                }
            };
            let key = |a: &u32, b: &u32| {
                let o = val(*a).cmp(&val(*b));
                (if desc { o.reverse() } else { o }).then_with(|| a.cmp(b))
            };
            if total > limit && limit > 0 {
                v.select_nth_unstable_by(limit - 1, key);
                v.truncate(limit);
            }
            v.sort_unstable_by(key);
            v
        };
        Some(ChildrenOut {
            node: Self::out(&t, id),
            path: t.path(id),
            more: (total - ids.len()) as u32,
            children: ids.iter().map(|&c| Self::out(&t, c)).collect(),
        })
    }

    pub fn path_of(&self, id: u32) -> Option<String> {
        let t = lock(&self.tree);
        t.valid(id).then(|| t.path(id))
    }

    /// Chỉnh các bộ đếm của `progress()` (tệp / thư mục / byte đã quét) theo thay đổi của cây, để dòng trạng thái
    /// của trang không giữ số cũ sau khi xoá.
    fn count(&self, size: i64, files: i64, dirs: i64) {
        let bump = |a: &AtomicU64, d: i64| {
            let _ = a.try_update(Ordering::Relaxed, Ordering::Relaxed, |x| Some(x.saturating_add_signed(d)));
        };
        bump(&self.bytes, size);
        bump(&self.files, files);
        bump(&self.dirs, dirs);
    }

    /// Bỏ thư mục `id` khỏi cây (sau khi đã xoá trên đĩa), trừ số liệu khỏi các thư mục cha.
    ///
    /// Nút bị gỡ (và cả nhánh con của nó) chỉ được đánh dấu `F_REMOVED`, KHÔNG tái sử dụng chỗ: luồng quét có thể còn
    /// giữ id của nhánh đó trong hàng việc. Bộ nhớ đó (56 byte/nút + tên) được trả khi đóng / quét lại; lượng giữ
    /// lại chỉ bằng những gì người dùng đã xoá hoặc đo lại trong lần quét này.
    pub fn forget(&self, id: u32) {
        let mut t = lock(&self.tree);
        if id == 0 || id >= t.len || t.detached(id) {
            return;
        }
        let d = *t.get(id);
        t.unlink(id);
        t.get_mut(id).flags |= F_REMOVED;
        t.bubble(d.parent, d.size, d.alloc, d.files, d.dirs + 1, true);
        if d.left > 0 {
            // phần chưa quét của nhánh này sẽ không bao giờ báo xong -> cho cha tiến lên
            t.finish(d.parent);
        }
        drop(t);
        self.count(-(d.size as i64), -i64::from(d.files), -(i64::from(d.dirs) + 1));
    }

    /// Bớt một tệp đã xoá khỏi số liệu của thư mục `id` (tệp trực tiếp = tổng − các con, nên chỉ cần trừ tổng).
    /// `size` / `alloc` phải là số đã được tính lúc quét (xem `file_sizes`), nếu không "dung lượng trên đĩa" lệch.
    pub fn forget_file(&self, id: u32, size: u64, alloc: u64) {
        let mut t = lock(&self.tree);
        if id >= t.len || t.detached(id) {
            return;
        }
        t.bubble(id, size, alloc, 1, 0, true);
        drop(t);
        self.count(-(size as i64), -1, 0);
    }

    /// Đọc lại CHỈ các tệp trực tiếp của thư mục `id` (một lần `list_dir`, không đi vào thư mục con) và áp phần chênh
    /// của (tổng, dung lượng trên đĩa, số tệp) lên `id` và mọi thư mục cha. Dùng sau khi xoá tệp: số đọc lại đúng cả khi
    /// kích thước tệp đã đổi từ lúc quét, hoặc có tệp mới.
    ///
    /// `Busy` = thư mục này chưa được lần quét đang chạy liệt kê tệp -> không làm gì (lần quét sẽ đếm đúng trạng thái hiện tại).
    pub fn resync_files(&self, id: u32) -> SyncResult {
        let path = {
            let t = lock(&self.tree);
            if !t.valid(id) {
                return SyncResult::Gone;
            }
            let d = t.get(id);
            if d.flags & F_LISTED == 0 {
                return SyncResult::Busy;
            }
            if d.flags & (F_LINK | F_DENIED) != 0 {
                return SyncResult::Stale;
            }
            t.path(id)
        };
        let mut p = long_path(&path);
        p.push(0);
        let mut buf = vec![0u64; BUF_U64];
        let (mut nf, mut sz, mut al) = (0u32, 0u64, 0u64);
        let res = list_dir(&p, &mut buf, |e| {
            if e.attrs & ATTR_DIRECTORY == 0 {
                nf += 1;
                sz += e.size;
                al += e.alloc;
            }
        });
        if res.is_err() {
            return SyncResult::Stale;
        }
        let mut t = lock(&self.tree);
        if !t.valid(id) {
            return SyncResult::Gone;
        }
        let own = t.own(id);
        let (ds, da, df) = (sz as i64 - own.size as i64, al as i64 - own.alloc as i64, i64::from(nf) - i64::from(own.files));
        t.bubble_delta(id, ds, da, df, 0);
        drop(t);
        self.count(ds, df, 0);
        SyncResult::Updated
    }

    /// Đo lại CHỈ nhánh `id` trên đĩa (không quét lại cả ổ) và đưa số liệu của nhánh đó, rồi của mọi thư mục cha, về đúng
    /// với đĩa. Dùng khi một lần xoá chỉ thành công một phần (tệp đang dùng…).
    ///
    /// Cách làm: quét thư mục đó bằng một `Scan` phụ tạm (≤ 6 luồng, cùng mã quét), đợi tối đa `budget` (tính từ đầu hàm,
    /// gồm cả lần liệt kê gốc), rồi dưới khoá cây ĐỒNG BỘ TẠI CHỖ: ghép con theo tên, cập nhật số của nút đã có, chỉ đánh
    /// dấu `F_REMOVED` các con đã biến mất và chỉ cấp nút mới cho thư mục thật sự mới. Nên sau nhiều lần xoá một phần
    /// bộ nhớ của cây không tăng (cây phụ bị bỏ ngay), và id của các nút còn lại giữ nguyên (giao diện không mất mở/đóng).
    pub fn sync_dir(&self, id: u32, budget: Duration) -> SyncResult {
        let t0 = Instant::now();
        let (path, d) = {
            let t = lock(&self.tree);
            if id >= t.len || t.detached(id) {
                return SyncResult::Gone;
            }
            (t.path(id), *t.get(id))
        };
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // đã biến mất hẳn (nút gốc của lần quét thì không gỡ được: giao diện đóng lần quét)
                if id == 0 {
                    return SyncResult::Stale;
                }
                self.forget(id);
                return SyncResult::Gone;
            }
            Err(_) => return SyncResult::Stale,
        }
        if d.flags & F_LINK != 0 {
            return SyncResult::Updated; // không đi theo liên kết: không có gì để đo
        }
        if d.left > 0 {
            return SyncResult::Busy; // lần quét còn đang chạy ở nhánh này, nó sẽ đọc trạng thái mới nhất
        }
        let Ok(sub) = Scan::start(0, &path) else { return SyncResult::Stale };
        while !sub.is_done() {
            if t0.elapsed() > budget || self.cancel.load(Ordering::Relaxed) {
                sub.cancel();
                return SyncResult::Stale; // quá lớn: giữ số cũ, giao diện báo "có thể cũ"
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        let st = lock(&sub.tree);
        let mut t = lock(&self.tree);
        if t.detached(id) {
            return SyncResult::Gone;
        }
        let old = *t.get(id);
        if old.left > 0 {
            return SyncResult::Busy;
        }
        let new = *st.get(0);
        // Cây phụ chỉ được thay số cũ khi đọc ĐƯỢC mọi nơi mà cây cũ đọc được: thư mục không đọc được (lỗi / thiếu quyền) mà
        // cây cũ chưa đánh dấu `denied` nghĩa là số mới thiếu -> giữ số cũ, báo Stale (không ghi đè bằng số thấp hơn).
        let mut check = vec![(id, 0u32)];
        while let Some((m, sn)) = check.pop() {
            let (matched, fresh, _) = match_kids(&t, &st, m, sn);
            if fresh.iter().any(|&c| has_denied(&st, c)) || matched.iter().any(|&(mc, sc)| st.get(sc).flags & F_DENIED != 0 && t.get(mc).flags & F_DENIED == 0) {
                return SyncResult::Stale;
            }
            check.extend(matched);
        }
        let mut stack = vec![(id, 0u32)];
        while let Some((m, sn)) = stack.pop() {
            let sd = *st.get(sn);
            {
                let n = t.get_mut(m);
                (n.size, n.alloc, n.files, n.dirs, n.left) = (sd.size, sd.alloc, sd.files, sd.dirs, 0);
                const KEEP: u8 = F_DENIED | F_LISTED | F_LINK;
                n.flags = (n.flags & !KEEP) | (sd.flags & KEEP);
                if m != id {
                    n.mtime = sd.mtime;
                }
            }
            // ghép con theo tên: còn -> cập nhật tiếp; mất -> gỡ; mới -> cấp nút
            let (matched, fresh, vanished) = match_kids(&t, &st, m, sn);
            if !vanished.is_empty() {
                for &c in &vanished {
                    t.get_mut(c).flags |= F_REMOVED;
                }
                // dựng lại danh sách con chỉ gồm các con còn lại
                let mut first = NONE;
                for &(mc, _) in &matched {
                    t.get_mut(mc).next = first;
                    first = mc;
                }
                t.get_mut(m).first = first;
            }
            for sc in fresh {
                let c = *st.get(sc);
                let nid = t.add(m, st.name(sc), c.mtime, c.flags & F_LINK != 0);
                stack.push((nid, sc));
            }
            stack.extend(matched);
        }
        // phần chênh của cả nhánh lên các thư mục cha (số của chính `id` đã đặt tuyệt đối ở trên)
        let (ds, da) = (new.size as i64 - old.size as i64, new.alloc as i64 - old.alloc as i64);
        let (df, dd) = (i64::from(new.files) - i64::from(old.files), i64::from(new.dirs) - i64::from(old.dirs));
        t.bubble_delta(old.parent, ds, da, df, dd);
        drop(t);
        drop(st);
        self.count(ds, df, dd);
        SyncResult::Updated
    }

    /// Id thư mục có đường dẫn `path` trong cây (không phân biệt hoa thường), None nếu nằm ngoài lần quét.
    pub fn find(&self, path: &str) -> Option<u32> {
        let t = lock(&self.tree);
        t.find(&self.root, path)
    }

    /// Số liệu (tổng kích thước, số tệp) của thư mục `path` nếu nằm trong cây.
    pub fn dir_stats(&self, path: &str) -> Option<(u64, u64, bool)> {
        let t = lock(&self.tree);
        let id = t.find(&self.root, path)?;
        let d = t.get(id);
        Some((d.size, u64::from(d.files), d.left == 0))
    }

    /// Tóm tắt gọn (≤ ~4000 ký tự) cho trợ lý AI: tổng quan, thư mục lớn nhất, thư mục nghi là rác/cache
    /// và (nếu có) thư mục người dùng đang hỏi. Chỉ đi các nhánh ≥ 100 MB nên cả ổ C chỉ mất vài ms.
    pub fn ai_summary(&self, focus: Option<&str>) -> String {
        use std::fmt::Write;
        let p = self.progress();
        let mut s = String::with_capacity(4096);
        let (free, total) = free_space(&self.root);
        let mut focus_files: Option<(String, u32)> = None;
        {
            let t = lock(&self.tree);
            let root = *t.get(0);
            let state = if !p.done { "đang quét, số liệu chưa đủ" } else if p.cancelled { "đã dừng giữa chừng, số liệu chưa đủ" } else { "đã quét xong" };
            let _ = write!(s, "Gốc: {} · {state} · {}, {} tệp, {} thư mục", self.root, sz(root.size), num(u64::from(root.files)), num(u64::from(root.dirs)));
            if total > 0 {
                let _ = write!(s, " · ổ còn trống {}/{}", sz(free), sz(total));
            }
            if p.errors > 0 {
                let _ = write!(s, " · {} thư mục không đọc được", num(p.errors));
            }
            s.push('\n');

            // thư mục lớn nhất: mở dần nút lớn nhất (giống đi xuống cây TreeSize)
            let min = (root.size / 100).max(1);
            let mut heap: std::collections::BinaryHeap<(u64, u32)> = t.kids(0).map(|c| (t.get(c).size, c)).filter(|x| x.0 >= min).collect();
            s.push_str("Thư mục lớn nhất:\n");
            for _ in 0..15 {
                let Some((size, id)) = heap.pop() else { break };
                let d = t.get(id);
                let _ = writeln!(s, "{} {} ({} tệp)", sz(size), t.path(id), num(u64::from(d.files)));
                heap.extend(t.kids(id).map(|c| (t.get(c).size, c)).filter(|x| x.0 >= min));
            }

            // nghi là rác / cache theo tên đường dẫn
            let mut junk: Vec<(u64, u32, &str)> = Vec::new();
            let mut stack = vec![0u32];
            while let Some(id) = stack.pop() {
                for c in t.kids(id) {
                    let d = t.get(c);
                    if d.size < JUNK_MIN || d.flags & F_LINK != 0 {
                        continue;
                    }
                    match JUNK.iter().find(|(comps, _)| t.ends_with(c, comps)) {
                        Some((_, "")) => {} // vùng loại trừ: không gợi ý gì bên trong
                        Some((_, label)) => junk.push((d.size, c, label)),
                        None => stack.push(c),
                    }
                }
            }
            junk.sort_unstable_by_key(|j| std::cmp::Reverse(j.0));
            if !junk.is_empty() {
                let sum: u64 = junk.iter().map(|j| j.0).sum();
                let _ = writeln!(s, "Nghi là rác/cache (nhận diện theo tên, tổng {}):", sz(sum));
                for &(size, id, label) in junk.iter().take(15) {
                    let d = t.get(id);
                    let _ = writeln!(s, "{} {} ({} tệp, sửa {}) [{label}]", sz(size), t.path(id), num(u64::from(d.files)), ym(d.mtime));
                }
            }

            if let Some(f) = focus {
                match t.find(&self.root, f) {
                    Some(id) => {
                        let d = *t.get(id);
                        let own = t.own(id);
                        let path = t.path(id);
                        let _ = writeln!(s, "Thư mục đang hỏi: {path} · {}, {} tệp, {} thư mục, sửa {}{}", sz(d.size), num(u64::from(d.files)), num(u64::from(d.dirs)), ym(d.mtime), if d.left == 0 { "" } else { " (đang quét)" });
                        let mut kids: Vec<u32> = t.kids(id).collect();
                        let n = kids.len();
                        let key = |a: &u32, b: &u32| t.get(*b).size.cmp(&t.get(*a).size);
                        if n > 12 {
                            kids.select_nth_unstable_by(11, key);
                            kids.truncate(12);
                        }
                        kids.sort_unstable_by(key);
                        for c in kids {
                            let x = t.get(c);
                            let _ = writeln!(s, "  {} {} ({} tệp, sửa {})", sz(x.size), t.path(c), num(u64::from(x.files)), ym(x.mtime));
                        }
                        if n > 12 {
                            let _ = writeln!(s, "  … và {} thư mục nhỏ hơn", n - 12);
                        }
                        if own.files > 0 {
                            let _ = writeln!(s, "  Tệp nằm trực tiếp: {} tệp, {}", num(u64::from(own.files)), sz(own.size));
                            focus_files = Some((path, own.files));
                        }
                    }
                    None => {
                        let _ = writeln!(s, "Thư mục đang hỏi: {f} (không nằm trong lần quét này)");
                    }
                }
            }
        }
        // đọc tệp lớn nhất của thư mục đang hỏi NGOÀI khoá cây
        if let Some((path, _)) = focus_files {
            if let Ok(f) = list_files(&path, 8) {
                let base = if path.ends_with('\\') { path.clone() } else { format!("{path}\\") };
                for it in f.items.iter().filter(|x| x.size > 0) {
                    let _ = writeln!(s, "  tệp {} {base}{} (sửa {})", sz(it.size), it.name, ym(secs(it.mtime)));
                }
            }
        }
        if s.len() > SUMMARY_MAX {
            let mut cut = SUMMARY_MAX;
            while !s.is_char_boundary(cut) {
                cut -= 1;
            }
            s.truncate(cut);
            s.push_str("\n…");
        }
        s
    }

    /// Bộ nhớ đã cấp cho cây (byte) – để đo.
    pub fn mem_estimate(&self) -> usize {
        let t = lock(&self.tree);
        t.chunks.iter().map(|c| c.capacity() * std::mem::size_of::<Dir>()).sum::<usize>() + t.names.iter().map(Vec::capacity).sum::<usize>()
    }
}

// ---------------- tóm tắt cho trợ lý AI ----------------

const JUNK_MIN: u64 = 100 << 20;
const SUMMARY_MAX: usize = 4000;

/// Thư mục thường là rác / cache tải lại được: (tên từ cuối đường dẫn lên, nhãn). Quy tắc đầu tiên khớp được dùng,
/// không đi tiếp vào bên trong thư mục đã khớp. Chỉ là gợi ý cho AI – người dùng luôn tự chọn trước khi xoá.
/// Nhãn rỗng = vùng loại trừ (chương trình đã cài, gói npm toàn cục, kho thành phần Windows): xoá ở đó làm hỏng app.
const JUNK: &[(&[&str], &str)] = &[
    (&["program files"], ""),
    (&["program files (x86)"], ""),
    (&["programs", "local", "appdata"], ""),
    (&["winsxs", "windows"], ""),
    (&["system32", "windows"], ""),
    (&["node_modules", "npm", "roaming", "appdata"], ""),
    (&["resources"], ""),
    (&["temp", "local", "appdata"], "tệp tạm người dùng"),
    (&["temp", "windows"], "tệp tạm Windows"),
    (&["$recycle.bin"], "Thùng rác"),
    (&["windows.old"], "bản Windows cũ"),
    (&["download", "softwaredistribution", "windows"], "bản cập nhật Windows đã tải"),
    (&["livekernelreports", "windows"], "báo cáo lỗi kernel"),
    (&["wer", "windows", "microsoft", "programdata"], "báo cáo lỗi Windows"),
    (&["crashdumps", "local", "appdata"], "crash dump"),
    (&["npm-cache", "local", "appdata"], "cache npm"),
    (&["cache", "pip", "local", "appdata"], "cache pip"),
    (&["cache", "uv", "local", "appdata"], "cache uv"),
    (&["cache", "yarn", "local", "appdata"], "cache Yarn"),
    (&["store", "pnpm", "local", "appdata"], "kho pnpm"),
    (&["registry", ".cargo"], "cache Cargo"),
    (&["packages", ".nuget"], "cache NuGet"),
    (&["caches", ".gradle"], "cache Gradle"),
    (&["repository", ".m2"], "cache Maven"),
    (&["inetcache", "windows", "microsoft", "local", "appdata"], "cache Internet"),
    (&["d3dscache", "local", "appdata"], "cache shader"),
    (&["dxcache", "nvidia", "local", "appdata"], "cache shader NVIDIA"),
    (&["glcache", "nvidia", "local", "appdata"], "cache shader NVIDIA"),
    (&["package cache", "programdata"], "bộ cài đã lưu – cần cân nhắc, gỡ cài đặt có thể cần"),
    (&["node_modules"], "thư viện Node, cài lại được"),
    (&[".cache"], "cache"),
    (&["cache"], "cache ứng dụng"),
    (&["code cache"], "cache ứng dụng"),
    (&["gpucache"], "cache ứng dụng"),
    (&["cache_data"], "cache ứng dụng"),
    (&["shadercache"], "cache shader"),
];

/// Kích thước gọn cho AI: 9.1G / 850M / 12K.
pub(crate) fn sz(b: u64) -> String {
    const K: f64 = 1024.0;
    let f = b as f64;
    if f >= K * K * K * 10.0 {
        format!("{:.0}G", f / (K * K * K))
    } else if f >= K * K * K {
        format!("{:.1}G", f / (K * K * K))
    } else if f >= K * K {
        format!("{:.0}M", f / (K * K))
    } else {
        format!("{:.0}K", f / K)
    }
}

fn num(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1e6)
    } else if n >= 10_000 {
        format!("{}k", n / 1000)
    } else {
        n.to_string()
    }
}

/// Giây Unix -> "YYYY-MM" (giờ UTC, đủ để AI biết thư mục cũ hay mới).
pub(crate) fn ym(secs: u32) -> String {
    if secs == 0 {
        return "?".into();
    }
    // thuật toán civil-from-days (Howard Hinnant)
    let z = i64::from(secs / 86400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y}-{m:02}")
}

/// (trống, tổng) của ổ chứa `path`; (0, 0) nếu không đọc được.
pub(crate) fn free_space(path: &str) -> (u64, u64) {
    let mut w: Vec<u16> = path.encode_utf16().collect();
    if w.last() != Some(&(b'\\' as u16)) {
        w.push(b'\\' as u16);
    }
    w.push(0);
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) } != 0 {
        (free, total)
    } else {
        (0, 0)
    }
}

/// Đường dẫn tuyệt đối đã chuẩn hoá (\ thay /, bỏ ngoặc kép và \ cuối), None nếu không phải đường dẫn tuyệt đối
/// (không bao giờ xoá theo đường dẫn tương đối – sẽ tính từ thư mục hiện hành của app).
pub fn normalize_abs(p: &str) -> Option<String> {
    let mut s = p.trim().trim_matches('"').trim().replace('/', "\\");
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    let b = s.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    let unc = s.starts_with(r"\\") && s.len() > 2;
    (drive || unc).then_some(s)
}

/// Kích thước của một mục lấy TRƯỚC khi xoá, để trừ đúng khỏi cây quét.
#[derive(Clone, Copy, Debug)]
pub struct EntryInfo {
    pub is_dir: bool,
    /// EndOfFile (cùng nghĩa với cột "Kích thước" của cây)
    pub size: u64,
    /// AllocationSize (cùng nghĩa với "Dung lượng trên đĩa"; làm tròn theo cụm, tệp nén / thưa thì khác `size`)
    pub alloc: u64,
}

/// Thông tin mục `path` (không đi theo liên kết). None nếu không có.
pub fn entry_info(path: &str) -> Option<EntryInfo> {
    let mut w = long_path(path);
    w.push(0);
    unsafe {
        // quyền 0: chỉ đọc thuộc tính, không tranh chấp với tiến trình đang mở tệp; OPEN_REPARSE_POINT: không vào liên kết
        let h = CreateFileW(w.as_ptr(), 0, SHARE_ALL, std::ptr::null(), OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, std::ptr::null_mut());
        if h as isize != -1 {
            let mut buf = [0u64; 3]; // FILE_STANDARD_INFO: AllocationSize, EndOfFile, Links(4) DeletePending(1) Directory(1)
            let ok = GetFileInformationByHandleEx(h, FILE_STANDARD_INFO, buf.as_mut_ptr() as *mut c_void, 24) != 0;
            CloseHandle(h);
            if ok {
                let is_dir = (buf[2] >> 40) & 0xff != 0;
                return Some(EntryInfo { is_dir, size: (buf[1] as i64).max(0) as u64, alloc: (buf[0] as i64).max(0) as u64 });
            }
        }
    }
    let m = std::fs::symlink_metadata(path).ok()?;
    Some(EntryInfo { is_dir: m.is_dir(), size: m.len(), alloc: m.len() })
}

/// Thư mục chứa `p` ("C:\a.txt" -> "C:\", "C:\a\b" -> "C:\a").
pub fn parent_path(p: &str) -> Option<String> {
    p.rfind('\\').map(|i| if i == 2 { p[..3].to_owned() } else { p[..i].to_owned() })
}

impl Scan {
    /// Cập nhật cây sau khi đã THỬ xoá `path` (`before` lấy trước khi xoá). Chỉ đụng các nút bị ảnh hưởng:
    /// - thư mục đã mất: `forget` (trừ tổng đang ghi trong cây khỏi mọi thư mục cha);
    /// - thư mục còn lại một phần (tệp đang dùng…): `sync_dir` đo lại riêng nhánh đó, cập nhật tại chỗ;
    /// - tệp đã mất: `resync_files` đọc lại tệp trực tiếp của thư mục cha (một lần liệt kê);
    /// - tệp chưa xoá được: không đổi.
    ///
    /// None = đường dẫn nằm ngoài lần quét này, hoặc không có gì để cập nhật (tệp còn nguyên, gốc lần quét).
    /// `Stale` / `Busy` = số liệu có thể chưa chính xác (xem `SyncResult`).
    pub fn after_delete(&self, path: &str, before: &EntryInfo) -> Option<SyncResult> {
        self.sync_path_as(path, before.is_dir, SYNC_BUDGET)
    }

    /// Như `after_delete` nhưng khi không có `EntryInfo` lấy trước (vd. sau lệnh xoá / cắt của menu Windows, hoặc sau khi
    /// dọn dẹp): loại mục lấy từ đĩa, nếu đã mất thì từ cây (có nút thư mục = là thư mục). `budget` giới hạn lần đo lại nhánh.
    pub fn sync_path(&self, path: &str, budget: Duration) -> Option<SyncResult> {
        let is_dir = match entry_info(path) {
            Some(i) => i.is_dir,
            None => self.find(path).is_some(),
        };
        self.sync_path_as(path, is_dir, budget)
    }

    fn sync_path_as(&self, path: &str, is_dir: bool, budget: Duration) -> Option<SyncResult> {
        let exists = std::fs::symlink_metadata(path).is_ok();
        if is_dir {
            let id = self.find(path)?;
            if id == 0 {
                return None; // gốc lần quét: người dùng quét lại / đóng
            }
            return Some(if exists { self.sync_dir(id, budget) } else { self.forget(id); SyncResult::Gone });
        }
        if exists {
            return None;
        }
        // tệp: đọc lại tệp trực tiếp của thư mục cha (không trừ theo kích thước lúc này: tệp có thể đã đổi cỡ từ lúc quét);
        // cha chưa được lần quét đang chạy liệt kê -> `Busy` (lần quét sẽ đếm đúng, nhưng báo "có thể chưa chính xác")
        let dir = self.find(&parent_path(path)?)?;
        Some(self.resync_files(dir))
    }
}

/// Tệp nằm trực tiếp trong `dir` (lớn nhất trước, tối đa `limit`).
pub fn list_files(dir: &str, limit: usize) -> Result<FilesOut, String> {
    let mut p = long_path(dir);
    p.push(0);
    let mut buf = vec![0u64; BUF_U64];
    let mut items = Vec::new();
    list_dir(&p, &mut buf, |e| {
        if e.attrs & ATTR_DIRECTORY == 0 {
            items.push(FileOut { name: String::from_utf16_lossy(e.name), size: e.size, alloc: e.alloc, mtime: e.mtime });
        }
    })
    .map_err(err_text)?;
    let total = items.len() as u32;
    let key = |a: &FileOut, b: &FileOut| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name));
    if items.len() > limit && limit > 0 {
        items.select_nth_unstable_by(limit - 1, key);
        items.truncate(limit);
    }
    items.sort_unstable_by(key);
    Ok(FilesOut { total, items })
}

// ---------------- ổ đĩa ----------------

#[derive(Serialize)]
pub struct Drive {
    /// "C:\"
    pub root: String,
    pub letter: String,
    pub label: String,
    pub fs: String,
    /// fixed | removable | network | cdrom | ram
    pub kind: &'static str,
    pub total: u64,
    pub free: u64,
    /// ổ có sẵn (có đĩa trong ổ, đọc được)
    pub ready: bool,
}

/// Danh sách ổ. Mỗi ổ được hỏi trên một luồng riêng với hạn 1,5 s: ổ mạng mất kết nối / ổ quang
/// đang quay có thể treo hàng chục giây, nhưng không bao giờ làm chậm danh sách.
pub fn drives() -> Vec<Drive> {
    use std::sync::mpsc;
    use std::time::Duration;
    let mask = unsafe { GetLogicalDrives() };
    let (tx, rx) = mpsc::channel();
    let mut pending: Vec<Drive> = Vec::new();
    let mut got_network = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = char::from(b'A' + i as u8);
        let root = format!("{letter}:\\");
        pending.push(Drive { root: root.clone(), letter: format!("{letter}:"), label: String::new(), fs: String::new(), kind: "fixed", total: 0, free: 0, ready: false });
        // ổ mạng: nhận ra qua bảng tên thiết bị cục bộ (không chạm mạng) rồi bỏ qua, vì chỉ cần
        // hỏi loại ổ của một ổ mạng mất kết nối cũng có thể treo hàng chục giây.
        if is_network_letter(letter) {
            pending.last_mut().unwrap().kind = "network";
            got_network.push(i);
            continue;
        }
        let tx = tx.clone();
        let _ = std::thread::Builder::new().name("du-drive".into()).stack_size(64 * 1024).spawn(move || {
            let _ = tx.send((i, query_drive(&root)));
        });
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut got = std::collections::HashMap::new();
    let asked = pending.len() - got_network.len();
    while got.len() < asked {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok((i, d)) => { got.insert(i, d); }
            Err(_) => break,
        }
    }
    pending
        .into_iter()
        .filter_map(|d| {
            let i = u32::from(d.letter.as_bytes()[0] - b'A');
            match got.remove(&i) {
                Some(Some(q)) => Some(q),
                Some(None) => None, // không phải ổ đĩa (loại không rõ)
                None => Some(Drive { kind: "network", ..d }), // quá hạn: hiện là chưa sẵn sàng
            }
        })
        .collect()
}

fn is_network_letter(letter: char) -> bool {
    let name: Vec<u16> = format!("{letter}:").encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 512];
    let n = unsafe { QueryDosDeviceW(name.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 {
        return false;
    }
    let t = super::util::from_wide_buf(&buf).to_ascii_lowercase();
    t.contains("lanmanredirector") || t.contains(r"\device\mup") || t.contains("webdavredirector") || t.contains("rdpdr")
}

fn query_drive(root: &str) -> Option<Drive> {
    unsafe {
        SetThreadErrorMode(SEM_FAILCRITICALERRORS, std::ptr::null_mut());
        let w: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let kind = match GetDriveTypeW(w.as_ptr()) {
            2 => "removable",
            3 => "fixed",
            4 => "network",
            5 => "cdrom",
            6 => "ram",
            _ => return None,
        };
        let letter = root.trim_end_matches('\\').to_owned();
        let mut d = Drive { root: root.to_owned(), letter, label: String::new(), fs: String::new(), kind, total: 0, free: 0, ready: false };
        let mut name = [0u16; 261];
        let mut fs = [0u16; 32];
        if GetVolumeInformationW(w.as_ptr(), name.as_mut_ptr(), name.len() as u32, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), fs.len() as u32) != 0 {
            d.label = super::util::from_wide_buf(&name);
            d.fs = super::util::from_wide_buf(&fs);
            let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
            if GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) != 0 {
                d.total = total;
                d.free = free;
                d.ready = true;
            }
        }
        Some(d)
    }
}

/// Đường dẫn không được phép xoá từ app (gốc ổ, thư mục hệ thống, hồ sơ người dùng…).
pub fn protected(path: &str) -> bool {
    let p = path.trim_end_matches('\\').to_lowercase();
    if p.len() <= 2 {
        return true; // "c:" = gốc ổ
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default().trim_end_matches('\\').to_lowercase();
    let sys = env("SystemRoot");
    let drive = sys.get(..2).unwrap_or("c:").to_owned();
    let mut list = vec![
        sys.clone(),
        env("ProgramFiles"),
        env("ProgramFiles(x86)"),
        env("ProgramData"),
        env("USERPROFILE"),
        format!("{drive}\\users"),
        format!("{drive}\\program files"),
        format!("{drive}\\program files (x86)"),
        format!("{drive}\\programdata"),
        format!("{drive}\\windows"),
        format!("{drive}\\system volume information"),
        format!("{drive}\\recovery"),
        format!("{drive}\\boot"),
    ];
    list.retain(|s| !s.is_empty());
    list.contains(&p) || (!sys.is_empty() && p.starts_with(&(sys + "\\system32")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fomeg-du-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn wait(s: &Scan) {
        let t = Instant::now();
        while !s.is_done() {
            assert!(t.elapsed().as_secs() < 600, "quét quá lâu");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn sums_sizes_and_forgets() {
        let root = tmp("sum");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join("c")).unwrap();
        std::fs::write(root.join("top.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(root.join("a/x.bin"), vec![0u8; 3000]).unwrap();
        std::fs::write(root.join("a/b/y.bin"), vec![0u8; 5000]).unwrap();
        std::fs::write(root.join("c/z.bin"), vec![0u8; 200]).unwrap();
        let s = Scan::start(1, root.to_str().unwrap()).unwrap();
        wait(&s);
        let r = s.children(0, "size", true, 100).unwrap();
        assert!(r.node.done);
        assert_eq!((r.node.size, r.node.files, r.node.dirs, r.node.own_files), (9200, 4, 3, 1));
        assert_eq!(r.children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "c"]);
        assert_eq!(r.children[0].size, 8000);
        let a = r.children[0].id;
        let p = s.progress();
        assert_eq!((p.files, p.dirs, p.bytes, p.done), (4, 4, 9200, true));
        // top-k
        let k = s.children(0, "size", true, 1).unwrap();
        assert_eq!((k.children.len(), k.more), (1, 1));
        // xoá nhánh "a" khỏi cây
        s.forget(a);
        let r = s.children(0, "size", true, 100).unwrap();
        assert_eq!((r.node.size, r.node.files, r.node.dirs), (1200, 2, 1));
        s.forget_file(0, 1000, 0);
        assert_eq!(s.children(0, "size", true, 100).unwrap().node.size, 200);
        // tệp trực tiếp
        let f = list_files(root.join("a").to_str().unwrap(), 10).unwrap();
        assert_eq!((f.total, f.items[0].name.as_str(), f.items[0].size), (1, "x.bin", 3000));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Mọi thư mục của cây: (đường dẫn, size, alloc, tệp, thư mục con), sắp theo đường dẫn.
    fn snapshot(s: &Scan) -> Vec<(String, u64, u64, u64, u32)> {
        fn walk(s: &Scan, id: u32, out: &mut Vec<(String, u64, u64, u64, u32)>) {
            let r = s.children(id, "name", false, 10_000).unwrap();
            out.push((r.path, r.node.size, r.node.alloc, r.node.files, r.node.dirs));
            for c in r.children {
                walk(s, c.id, out);
            }
        }
        let mut v = Vec::new();
        walk(s, 0, &mut v);
        v.sort();
        v
    }

    fn fresh(root: &std::path::Path) -> Arc<Scan> {
        let s = Scan::start(2, root.to_str().unwrap()).unwrap();
        wait(&s);
        s
    }

    #[test]
    fn delete_updates_only_the_affected_nodes_like_a_rescan() {
        let root = tmp("after-delete");
        let rp = root.to_str().unwrap().to_owned();
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join("c/d")).unwrap();
        std::fs::write(root.join("top.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(root.join("a/x.bin"), vec![0u8; 3000]).unwrap();
        std::fs::write(root.join("a/x2.bin"), vec![0u8; 50]).unwrap();
        std::fs::write(root.join("a/b/y.bin"), vec![0u8; 5000]).unwrap();
        std::fs::write(root.join("a/b/keep.bin"), vec![0u8; 700]).unwrap();
        std::fs::write(root.join("c/z.bin"), vec![0u8; 200]).unwrap();
        std::fs::write(root.join("c/d/w.bin"), vec![0u8; 4100]).unwrap();
        let s = Scan::start(1, &rp).unwrap();
        wait(&s);
        assert_eq!(snapshot(&s), snapshot(&fresh(&root)));
        assert_eq!(parent_path(r"C:\a.txt").as_deref(), Some(r"C:\"));
        assert_eq!(parent_path(r"C:\a\b").as_deref(), Some(r"C:\a"));

        // 1) tệp: kích thước đã đổi từ lúc quét (x.bin 3000 -> 9000, x2.bin 50 -> 80) rồi xoá x.bin: số liệu phải khớp
        // đĩa (đọc lại tệp của thư mục cha), không phải "trừ 9000 khỏi số cũ 3000"
        let f = format!(r"{rp}\a\x.bin");
        let info = entry_info(&f).unwrap();
        assert!(!info.is_dir && info.size == 3000 && info.alloc >= 3000, "{info:?}");
        std::fs::write(&f, vec![0u8; 9000]).unwrap();
        std::fs::write(root.join("a/x2.bin"), vec![0u8; 80]).unwrap();
        std::fs::remove_file(&f).unwrap();
        assert_eq!(s.after_delete(&f, &info), Some(SyncResult::Updated));
        assert_eq!(snapshot(&s), snapshot(&fresh(&root)));
        // tệp chưa xoá được thì không đổi
        let top = format!(r"{rp}\top.bin");
        assert_eq!(s.after_delete(&top, &entry_info(&top).unwrap()), None);

        // 2) thư mục xoá một phần (y.bin đi, keep.bin còn; thêm nhánh mới trong lúc đó): chỉ đo lại "a"
        let a = format!(r"{rp}\a");
        let a_info = entry_info(&a).unwrap();
        assert!(a_info.is_dir);
        let old_b = s.find(&format!(r"{rp}\a\b")).unwrap();
        std::fs::remove_file(root.join("a/b/y.bin")).unwrap();
        std::fs::create_dir_all(root.join("a/n/m")).unwrap();
        std::fs::write(root.join("a/n/m/q.bin"), vec![0u8; 123]).unwrap();
        assert_eq!(s.after_delete(&a, &a_info), Some(SyncResult::Updated));
        assert_eq!(snapshot(&s), snapshot(&fresh(&root)));
        // "b" vẫn còn trên đĩa nên GIỮ nguyên id (đồng bộ tại chỗ); chỉ "n" và "n\m" là nút mới
        assert!(s.children(old_b, "size", true, 10).is_some(), "nút còn lại giữ id");
        let a_id = s.find(&a).unwrap();
        let r = s.children(a_id, "size", true, 10).unwrap();
        assert!(r.node.done && r.children.len() == 2);
        // xoá một phần lặp lại nhiều lần (không có thư mục mới): số nút của cây không tăng
        let len = lock(&s.tree).len;
        for i in 0..5 {
            std::fs::write(root.join(format!("a/b/tmp{i}.bin")), vec![0u8; 10]).unwrap();
            std::fs::remove_file(root.join(format!("a/b/tmp{i}.bin"))).unwrap();
            assert_eq!(s.after_delete(&a, &a_info), Some(SyncResult::Updated));
            assert_eq!(lock(&s.tree).len, len, "lần {i}: cây không được cấp thêm nút");
        }
        assert_eq!(snapshot(&s), snapshot(&fresh(&root)));
        // thư mục con biến mất khỏi đĩa: gỡ khỏi danh sách con, không cấp nút mới
        std::fs::remove_dir_all(root.join("a/n")).unwrap();
        assert_eq!(s.after_delete(&a, &a_info), Some(SyncResult::Updated));
        assert_eq!(lock(&s.tree).len, len);
        assert_eq!(s.children(a_id, "size", true, 10).unwrap().children.len(), 1);
        assert_eq!(snapshot(&s), snapshot(&fresh(&root)));
        // tạo lại "n": đúng hai nút mới (n, m)
        std::fs::create_dir_all(root.join("a/n/m")).unwrap();
        std::fs::write(root.join("a/n/m/q.bin"), vec![0u8; 123]).unwrap();
        assert_eq!(s.after_delete(&a, &a_info), Some(SyncResult::Updated));
        assert_eq!(lock(&s.tree).len, len + 2);
        assert_eq!(snapshot(&s), snapshot(&fresh(&root)));

        // 3) thư mục xoá hẳn
        let c = format!(r"{rp}\c");
        let c_info = entry_info(&c).unwrap();
        std::fs::remove_dir_all(root.join("c")).unwrap();
        assert_eq!(s.after_delete(&c, &c_info), Some(SyncResult::Gone));
        assert_eq!(snapshot(&s), snapshot(&fresh(&root)));
        // bộ đếm của dòng trạng thái khớp một lần quét mới
        let (p, q) = (s.progress(), fresh(&root).progress());
        assert_eq!((p.files, p.dirs, p.bytes), (q.files, q.dirs, q.bytes));
        // đo lại rồi xoá "n" (đã ghép từ cây phụ) vẫn đúng
        let n = format!(r"{rp}\a\n");
        std::fs::remove_dir_all(root.join("a/n")).unwrap();
        // loại mục không biết trước (menu Windows / dọn dẹp): đã mất mà cây có nút thư mục -> gỡ nhánh đó
        assert_eq!(s.sync_path(&n, SYNC_BUDGET), Some(SyncResult::Gone));
        assert_eq!(snapshot(&s), snapshot(&fresh(&root)));
        // thư mục cha chưa được lần quét liệt kê tệp: xoá tệp không đụng gì vào cây (lần quét sẽ đếm đúng)
        let idle = {
            let mut t = Tree::default();
            t.add(NONE, &rp, 0, false);
            Scan {
                id: 3,
                root: rp.clone(),
                tree: Mutex::new(t),
                queue: Mutex::new(Queue { jobs: Vec::new(), busy: 0 }),
                wake: Condvar::new(),
                cancel: AtomicBool::new(false),
                done: AtomicBool::new(false),
                started: Instant::now(),
                elapsed_ms: AtomicU64::new(0),
                files: AtomicU64::new(0),
                dirs: AtomicU64::new(0),
                bytes: AtomicU64::new(0),
                errors: AtomicU64::new(0),
            }
        };
        assert_eq!(idle.resync_files(0), SyncResult::Busy);
        let top = format!(r"{rp}\top.bin");
        let top_info = entry_info(&top).unwrap();
        std::fs::remove_file(&top).unwrap();
        assert_eq!(idle.after_delete(&top, &top_info), Some(SyncResult::Busy));
        assert_eq!(lock(&idle.tree).get(0).size, 0);
        // đường dẫn ngoài lần quét: không làm gì
        assert_eq!(s.after_delete(r"Z:\khong\co", &EntryInfo { is_dir: true, size: 0, alloc: 0 }), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ai_summary_finds_paths_and_stays_short() {
        let root = tmp("ai");
        std::fs::create_dir_all(root.join("Proj/node_modules/x")).unwrap();
        std::fs::create_dir_all(root.join("Proj/src")).unwrap();
        std::fs::write(root.join("Proj/big.iso"), vec![0u8; 7000]).unwrap();
        std::fs::write(root.join("Proj/src/a.rs"), vec![0u8; 100]).unwrap();
        let s = Scan::start(9, root.to_str().unwrap()).unwrap();
        wait(&s);
        let proj = format!("{}\\Proj", root.to_str().unwrap());
        // không phân biệt hoa thường, / hay \ cuối
        let id = s.find(&proj.to_uppercase().replace('\\', "/")).unwrap();
        assert_eq!(s.path_of(id).unwrap(), proj);
        assert!(s.find(&format!("{proj}2")).is_none());
        assert_eq!(s.dir_stats(&proj).unwrap().0, 7100);
        let text = s.ai_summary(Some(&proj));
        assert!(text.contains("đã quét xong") && text.contains("Thư mục đang hỏi") && text.contains("big.iso"), "{text}");
        assert!(text.len() <= SUMMARY_MAX + 4);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn helpers_for_ai() {
        assert_eq!(ym(1_700_000_000), "2023-11");
        assert_eq!(sz((9u64 << 30) + (200 << 20)), "9.2G");
        assert_eq!(num(2_770_000), "2.77M");
        assert_eq!(normalize_abs(r#" "C:/Users/a/Temp/" "#).as_deref(), Some(r"C:\Users\a\Temp"));
        assert_eq!(normalize_abs("C:\\").as_deref(), Some("C:\\"));
        assert!(normalize_abs("Temp").is_none() && normalize_abs("..\\x").is_none() && normalize_abs("C:x").is_none());
        // quy tắc rác khớp từ cuối đường dẫn lên
        let mut t = Tree::default();
        t.add(NONE, "C:\\", 0, false);
        let u = t.add(0, "Users", 0, false);
        let a = t.add(u, "A", 0, false);
        let ad = t.add(a, "AppData", 0, false);
        let l = t.add(ad, "Local", 0, false);
        let tp = t.add(l, "Temp", 0, false);
        assert!(t.ends_with(tp, &["temp", "local", "appdata"]));
        assert!(!t.ends_with(l, &["temp", "local", "appdata"]));
        assert_eq!(t.find("C:\\", r"c:\users\a\appdata\local\temp"), Some(tp));
        // gói npm toàn cục không phải rác (xoá là mất các CLI đã cài)
        let ro = t.add(ad, "Roaming", 0, false);
        let npm = t.add(ro, "npm", 0, false);
        let nm = t.add(npm, "node_modules", 0, false);
        let proj = t.add(a, "proj", 0, false);
        let pnm = t.add(proj, "node_modules", 0, false);
        let label = |id: u32| JUNK.iter().find(|(c, _)| t.ends_with(id, c)).map(|x| x.1);
        assert_eq!(label(tp), Some("tệp tạm người dùng"));
        assert_eq!(label(nm), Some(""));
        assert_eq!(label(pnm), Some("thư viện Node, cài lại được"));
    }

    #[test]
    fn node_stays_small() {
        // mỗi byte ở đây nhân với ~600k thư mục trên một ổ C bình thường
        assert_eq!(std::mem::size_of::<Dir>(), 56);
    }

    #[test]
    fn missing_root_is_an_error_and_protected_paths() {
        // thư mục tạm không tồn tại (không dùng ổ mạng: có thể treo tới hết thời gian chờ của mạng)
        let missing = tmp("missing").join("khong-ton-tai");
        assert!(Scan::start(2, missing.to_str().unwrap()).is_err());
        let sys = std::env::var("SystemRoot").unwrap();
        assert!(protected(&sys));
        assert!(protected(r"C:\"));
        assert!(protected(&format!("{sys}\\System32\\drivers")));
        assert!(!protected(r"C:\Users\someone\Downloads\x"));
        assert!(!drives().is_empty());
    }

    /// Đo thật trên ổ hệ thống: `cargo test --release --lib du::tests::scan_system_drive -- --ignored --nocapture`
    #[test]
    #[ignore = "đo thời gian trên ổ thật"]
    fn scan_system_drive() {
        let root = std::env::var("FOMEG_DU_ROOT").or_else(|_| std::env::var("SystemDrive")).unwrap_or("C:".into());
        let s = Scan::start(3, &root).unwrap();
        let mut last = 0;
        while !s.is_done() {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let p = s.progress();
            eprintln!("  {} s: {} thư mục, {} tệp (+{}/s)", p.elapsed_ms / 1000, p.dirs, p.files, p.files - last);
            last = p.files;
        }
        let p = s.progress();
        eprintln!(
            "{root}: {} tệp, {} thư mục, {:.1} GB, {} lỗi, {} ms, cây ≈ {:.1} MB",
            p.files,
            p.dirs,
            p.bytes as f64 / 1e9,
            p.errors,
            p.elapsed_ms,
            s.mem_estimate() as f64 / 1048576.0
        );
        fn dump(s: &Scan, id: u32, depth: usize, max: usize) {
            let r = s.children(id, "size", true, 6).unwrap();
            for c in &r.children {
                if c.size < (1 << 30) { continue; }
                eprintln!("{}{:>8.1} GB {:>8} tệp  {}{}", "    ".repeat(depth), c.size as f64 / 1073741824.0, c.files, c.name, if c.link { " [link]" } else { "" });
                if depth < max { dump(s, c.id, depth + 1, max); }
            }
        }
        dump(&s, 0, 0, std::env::var("FOMEG_DU_DEPTH").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
    }
}

#[cfg(test)]
mod timing {
    #[test]
    #[ignore = "đo"]
    fn time_drives() {
        let t = std::time::Instant::now();
        let d = super::drives();
        eprintln!("drives(): {:?} -> {:?}", t.elapsed(), d.iter().map(|x| (&x.root, x.kind, &x.label, x.ready)).collect::<Vec<_>>());
    }
}


#[cfg(test)]
mod ai_measure {
    /// `cargo test --release --lib du::ai_measure -- --ignored --nocapture`: tóm tắt thật của ổ hệ thống.
    #[test]
    #[ignore = "đo trên ổ thật"]
    fn summary_of_system_drive() {
        let root = std::env::var("SystemDrive").unwrap_or("C:".into());
        let s = super::Scan::start(1, &root).unwrap();
        while !s.is_done() {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let focus = std::env::var("USERPROFILE").unwrap();
        let t = std::time::Instant::now();
        let text = s.ai_summary(Some(&format!(r"{focus}\AppData\Local")));
        eprintln!("{text}\n--- {} ký tự, {:?}", text.chars().count(), t.elapsed());
    }
}
