// Lấy một ảnh chụp tiến trình cho mỗi mẫu. Thông tin tĩnh gắn với PID và thời điểm tạo.
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::ffi::c_void;
use std::mem::{align_of, size_of, size_of_val};
use std::path::Path;
use std::ptr::null_mut;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, SetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, FILETIME, HANDLE, HWND,
    LPARAM, STATUS_BUFFER_OVERFLOW, STATUS_BUFFER_TOO_SMALL, STATUS_INFO_LENGTH_MISMATCH,
};
use windows::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, TokenElevation, TokenUser, PSID, SID_NAME_USE,
    TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Threading::{
    GetActiveProcessorCount, GetProcessTimes, OpenProcess, OpenProcessToken,
    QueryFullProcessImageNameW, ALL_PROCESSOR_GROUPS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId, InternalGetWindowText,
    IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
};

use crate::core::util::{from_wide_buf, wide, windir};

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret_len: *mut u32) -> i32;
    fn NtQueryInformationProcess(
        handle: HANDLE,
        class: u32,
        info: *mut c_void,
        len: u32,
        ret_len: *mut u32,
    ) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(hwnd: HWND, attr: u32, value: *mut c_void, len: u32) -> i32;
}

const SYSTEM_PROCESS_INFORMATION: u32 = 5;
const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
const DWMWA_CLOAKED: u32 = 14;
const FILETIME_UNIX_EPOCH: i64 = 116_444_736_000_000_000;
const MAX_PATH_CHARS: usize = 32_768;
const WINDOW_INTERVAL: u8 = 3;
const MAX_CACHED_USERS: usize = 1_024;

#[derive(Serialize, Clone)]
pub struct ProcStatic {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub exe: String,
    pub desc: String,
    pub publisher: String,
    pub cmd: String,
    pub start_ms: i64,
    pub session: u32,
    pub elevated: bool,
    pub user: String,
    pub kind: String,
}

#[derive(Clone, Copy)]
pub struct ProcRow {
    pub pid: u32,
    pub cpu: f32,
    pub mem: u64,
    pub disk: u64,
    pub threads: u32,
    pub handles: u32,
}

pub struct Totals {
    pub processes: u32,
    pub threads: u32,
    pub handles: u32,
}

pub struct ProcSample {
    pub t_ms: i64,
    pub rows: Vec<ProcRow>,
    pub added: Vec<ProcStatic>,
    pub removed: Vec<u32>,
    pub totals: Totals,
    /// Chỉ có bản đồ khi vừa quét lại cửa sổ.
    pub windows: Option<HashMap<u32, String>>,
}

pub struct ProcSampler {
    buf: Vec<u64>,
    offsets: Vec<usize>,
    live: HashMap<u32, LiveProc>,
    last: Option<Instant>,
    logical_cpus: u32,
    windows_root: String,
    details: Details,
    windows: WindowScan,
    window_exes: HashSet<String>,
    windows_in: u8,
}

struct LiveProc {
    data: ProcStatic,
    exe_lower: String,
    created: i64,
    cpu_ticks: u64,
    io_bytes: u64,
    row: ProcRow,
    seen: bool,
    is_windows: bool,
}

// Bố cục x64 của phần đầu SYSTEM_PROCESS_INFORMATION, trước mảng luồng.
#[repr(C)]
struct RawProc {
    _next: u32,
    threads: u32,
    private_ws: i64,
    _reserved1: [u8; 16],
    created: i64,
    user_time: i64,
    kernel_time: i64,
    name: RawUnicode,
    _base_priority: i32,
    pid: usize,
    ppid: usize,
    handles: u32,
    session: u32,
    _reserved2: [u8; 128],
    read_bytes: i64,
    write_bytes: i64,
    _other_bytes: i64,
}

#[repr(C)]
struct RawUnicode {
    len: u16,
    _max_len: u16,
    address: usize,
}

const _: () = {
    use std::mem::offset_of;

    assert!(size_of::<RawProc>() == 256 && size_of::<RawUnicode>() == 16);
    assert!(offset_of!(RawProc, _next) == 0 && offset_of!(RawProc, threads) == 4);
    assert!(offset_of!(RawProc, private_ws) == 8 && offset_of!(RawProc, created) == 32);
    assert!(offset_of!(RawProc, user_time) == 40 && offset_of!(RawProc, kernel_time) == 48);
    assert!(offset_of!(RawProc, name) == 56 && offset_of!(RawUnicode, address) == 8);
    assert!(offset_of!(RawProc, pid) == 80 && offset_of!(RawProc, ppid) == 88);
    assert!(offset_of!(RawProc, handles) == 96 && offset_of!(RawProc, session) == 100);
    assert!(offset_of!(RawProc, read_bytes) == 232 && offset_of!(RawProc, write_bytes) == 240);
    assert!(offset_of!(RawProc, _other_bytes) == 248);
};

impl RawUnicode {
    fn decode(&self, buf: &[u8]) -> String {
        if self.len == 0 || !self.len.is_multiple_of(2) {
            return String::new();
        }
        utf16_at(buf, self.address, self.len as usize / 2)
            .map(String::from_utf16_lossy)
            .unwrap_or_default()
    }
}

fn bytes(buf: &[u64], len: usize) -> &[u8] {
    // Bộ đệm đã khởi tạo; chỉ tạo lát cắt trong phạm vi bộ đệm.
    unsafe { std::slice::from_raw_parts(buf.as_ptr().cast(), len.min(size_of_val(buf))) }
}

fn bytes_at(buf: &[u8], address: usize, len: usize) -> Option<&[u8]> {
    let start = address.checked_sub(buf.as_ptr() as usize)?;
    buf.get(start..start.checked_add(len)?)
}

fn utf16_at(buf: &[u8], address: usize, len: usize) -> Option<&[u16]> {
    if !address.is_multiple_of(align_of::<u16>()) {
        return None;
    }
    let text = bytes_at(buf, address, len.checked_mul(2)?)?;
    // Địa chỉ đã kiểm tra căn lề và toàn bộ chuỗi nằm trong bộ đệm.
    Some(unsafe { std::slice::from_raw_parts(text.as_ptr().cast(), len) })
}

fn query_snapshot(buf: &mut Vec<u64>) -> Option<usize> {
    let capacity = u32::try_from(size_of_val(buf.as_slice())).ok()?;
    let mut used = 0;
    // Kernel ghi vào bộ đệm đã căn lề; chỉ gọi một lần trong mỗi mẫu.
    let status = unsafe {
        NtQuerySystemInformation(
            SYSTEM_PROCESS_INFORMATION,
            buf.as_mut_ptr().cast(),
            capacity,
            &mut used,
        )
    };
    if status == STATUS_INFO_LENGTH_MISMATCH.0 {
        // Giữ ảnh chụp cũ, thử lại bằng bộ đệm lớn hơn ở mẫu kế tiếp.
        let next = used.max(capacity).saturating_add(1024 * 1024);
        buf.resize(next as usize / size_of::<u64>(), 0);
    }
    (status >= 0 && used <= capacity).then_some(used as usize)
}

fn snapshot_offsets(snapshot: &[u8], offsets: &mut Vec<usize>) -> bool {
    offsets.clear();
    let mut offset = 0;
    while let Some(header) = snapshot
        .get(offset..)
        .and_then(|tail| tail.get(..size_of::<RawProc>()))
    {
        let Some((next, _)) = header.split_first_chunk::<4>() else {
            return false;
        };
        let next = u32::from_ne_bytes(*next) as usize;
        offsets.push(offset);
        if next == 0 {
            return true;
        }
        if next < size_of::<RawProc>() || next > snapshot.len() - offset {
            break;
        }
        offset += next;
    }
    false
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // Handle do OpenProcess hoặc OpenProcessToken trả về khi thành công.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

impl Handle {
    fn process(pid: u32, expected_time: i64) -> Option<Self> {
        // Chỉ xin quyền đọc thông tin giới hạn.
        let process =
            Self(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // Kiểm tra PID chưa bị tái sử dụng giữa lúc chụp và mở handle.
        unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .ok()?;
        let actual_time =
            (i64::from(created.dwHighDateTime) << 32) | i64::from(created.dwLowDateTime);
        (actual_time == expected_time).then_some(process)
    }

    fn token(&self) -> Option<Self> {
        let mut token = HANDLE::default();
        // Token chỉ cần quyền truy vấn.
        unsafe { OpenProcessToken(self.0, TOKEN_QUERY, &mut token) }.ok()?;
        Some(Self(token))
    }
}

struct Details {
    exe_buf: Vec<u16>,
    cmd_buf: Vec<u64>,
    token_buf: [u64; 16],
    paths: HashMap<String, FileDetails>,
    users: HashMap<Vec<u8>, String>,
}

struct FileDetails {
    desc: String,
    publisher: String,
}

impl FileDetails {
    fn read(path: &str) -> Self {
        let (mut desc, publisher) = version_details(path);
        if desc.is_empty() {
            desc = Path::new(path)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        Self { desc, publisher }
    }
}

impl Details {
    fn new() -> Self {
        Self {
            exe_buf: vec![0; 1024],
            cmd_buf: vec![0; 4096 / size_of::<u64>()],
            token_buf: [0; 16],
            paths: HashMap::new(),
            users: HashMap::new(),
        }
    }

    fn exe(&mut self, process: &Handle) -> String {
        loop {
            let mut len = self.exe_buf.len() as u32;
            // Win32 ghi tối đa len ký tự vào bộ đệm.
            let result = unsafe {
                QueryFullProcessImageNameW(
                    process.0,
                    PROCESS_NAME_WIN32,
                    PWSTR(self.exe_buf.as_mut_ptr()),
                    &mut len,
                )
            };
            match result {
                Ok(()) => {
                    return self
                        .exe_buf
                        .get(..len as usize)
                        .map(String::from_utf16_lossy)
                        .unwrap_or_default()
                }
                Err(e)
                    if e.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult()
                        && self.exe_buf.len() < MAX_PATH_CHARS =>
                {
                    self.exe_buf.resize(self.exe_buf.len() * 2, 0);
                }
                Err(_) => return String::new(),
            }
        }
    }

    fn command_line(&mut self, process: &Handle) -> String {
        loop {
            let capacity = size_of_val(self.cmd_buf.as_slice());
            let mut used = 0;
            // Lớp 60 trả về UNICODE_STRING và nội dung chuỗi trong cùng bộ đệm.
            let status = unsafe {
                NtQueryInformationProcess(
                    process.0,
                    PROCESS_COMMAND_LINE_INFORMATION,
                    self.cmd_buf.as_mut_ptr().cast(),
                    capacity as u32,
                    &mut used,
                )
            };
            let used = used as usize;
            if status >= 0 && used >= size_of::<RawUnicode>() && used <= capacity {
                // Đầu bộ đệm đã căn lề và đủ chỗ cho UNICODE_STRING.
                let text = unsafe { &*self.cmd_buf.as_ptr().cast::<RawUnicode>() };
                return text.decode(bytes(&self.cmd_buf, used));
            }
            if ![
                STATUS_INFO_LENGTH_MISMATCH.0,
                STATUS_BUFFER_TOO_SMALL.0,
                STATUS_BUFFER_OVERFLOW.0,
            ]
            .contains(&status)
                || used <= capacity
                || used > size_of::<RawUnicode>() + 2 * MAX_PATH_CHARS
            {
                return String::new();
            }
            self.cmd_buf.resize(used.div_ceil(size_of::<u64>()), 0);
        }
    }

    fn user(&mut self, token: &Handle) -> String {
        let mut used = 0;
        let capacity = size_of_val(&self.token_buf);
        // Bộ đệm đủ chỗ cho TOKEN_USER và SID có tối đa 15 subauthority.
        let result = unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(self.token_buf.as_mut_ptr().cast()),
                capacity as u32,
                &mut used,
            )
        };
        if result.is_err() || (used as usize) < size_of::<TOKEN_USER>() || used as usize > capacity
        {
            return String::new();
        }
        // TOKEN_USER nằm ở đầu bộ đệm và đã được Win32 khởi tạo.
        let sid = unsafe { (*self.token_buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let buf = bytes(&self.token_buf, used as usize);
        let Some(header) = bytes_at(buf, sid.0 as usize, 8) else {
            return String::new();
        };
        let Some(key) = bytes_at(buf, sid.0 as usize, 8 + usize::from(header[1]) * 4) else {
            return String::new();
        };
        if let Some(user) = self.users.get(key) {
            return user.clone();
        }
        let user = account_name(sid);
        if self.users.len() >= MAX_CACHED_USERS {
            self.users.clear();
        }
        self.users.insert(key.to_vec(), user.clone());
        user
    }

    fn read(&mut self, raw: &RawProc, snapshot: &[u8]) -> ProcStatic {
        let mut data = ProcStatic {
            pid: raw.pid as u32,
            ppid: raw.ppid as u32,
            name: raw.name.decode(snapshot),
            exe: String::new(),
            desc: String::new(),
            publisher: String::new(),
            cmd: String::new(),
            start_ms: raw.created.saturating_sub(FILETIME_UNIX_EPOCH).max(0) / 10_000,
            session: raw.session,
            elevated: false,
            user: String::new(),
            kind: String::new(),
        };
        if let Some(process) = Handle::process(data.pid, raw.created) {
            data.exe = self.exe(&process);
            data.cmd = self.command_line(&process);
            if let Some(token) = process.token() {
                data.elevated = elevated(&token);
                data.user = self.user(&token);
            }
        }
        data
    }
}

fn account_name(sid: PSID) -> String {
    let mut name = [0u16; 256];
    let mut domain = [0u16; 256];
    let mut name_len = name.len() as u32;
    let mut domain_len = domain.len() as u32;
    let mut use_type = SID_NAME_USE::default();
    // SID nằm trong bộ đệm token đã kiểm tra; Win32 nhận kích thước cả hai mảng.
    let result = unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            sid,
            Some(PWSTR(name.as_mut_ptr())),
            &mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_len,
            &mut use_type,
        )
    };
    if result.is_ok() {
        from_wide_buf(&name)
    } else {
        String::new()
    }
}

fn elevated(token: &Handle) -> bool {
    let mut value = TOKEN_ELEVATION::default();
    let mut used = 0;
    // Win32 ghi trạng thái nâng quyền vào cấu trúc có kích thước cố định.
    let result = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&mut value as *mut TOKEN_ELEVATION).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut used,
        )
    };
    result.is_ok() && value.TokenIsElevated != 0
}

fn version_details(path: &str) -> (String, String) {
    let path = wide(path);
    // Tên đường dẫn có ký tự kết thúc, chỉ hỏi kích thước tài nguyên.
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    if size == 0 {
        return Default::default();
    }
    let mut buf = vec![0u64; (size as usize).div_ceil(size_of::<u64>())];
    // Win32 ghi tài nguyên vào bộ đệm đã căn lề.
    if unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, buf.as_mut_ptr().cast()) }
        .is_err()
    {
        return Default::default();
    }
    let block = bytes(&buf, size as usize);
    (
        version_text(block, "FileDescription"),
        version_text(block, "CompanyName"),
    )
}

fn version_text(block: &[u8], field: &str) -> String {
    let translations = version_value(block, r"\VarFileInfo\Translation")
        .and_then(|(address, len)| utf16_at(block, address, len / 2))
        .unwrap_or_default();
    let pairs = translations
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[lang, cp]| (lang, cp));
    for (lang, cp) in pairs.chain([(0x0409, 0x04b0), (0x0409, 0x04e4), (0x0000, 0x04b0)]) {
        let query = format!(r"\StringFileInfo\{lang:04x}{cp:04x}\{field}");
        if let Some(text) =
            version_value(block, &query).and_then(|(address, len)| utf16_at(block, address, len))
        {
            let text = from_wide_buf(text);
            if !text.trim().is_empty() {
                return text.trim().to_owned();
            }
        }
    }
    String::new()
}

fn version_value(block: &[u8], query: &str) -> Option<(usize, usize)> {
    let query = wide(query);
    let mut value = null_mut();
    let mut len = 0;
    // Win32 trả con trỏ vào tài nguyên; bên gọi kiểm tra phạm vi trước khi đọc.
    let ok = unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(query.as_ptr()),
            &mut value,
            &mut len,
        )
    };
    (ok.as_bool() && len != 0).then_some((value as usize, len as usize))
}

fn is_windows(data: &ProcStatic, exe_lower: &str, windows_root: &str) -> bool {
    const SYSTEM_NAMES: &[&str] = &[
        "system",
        "secure system",
        "registry",
        "memory compression",
        "memcompression",
        "smss.exe",
        "csrss.exe",
        "wininit.exe",
        "winlogon.exe",
        "services.exe",
        "lsass.exe",
        "lsaiso.exe",
        "svchost.exe",
        "fontdrvhost.exe",
        "dwm.exe",
    ];
    data.pid == 4
        || (exe_lower.is_empty() && data.session == 0)
        || exe_lower.starts_with(windows_root)
        || SYSTEM_NAMES
            .iter()
            .any(|name| data.name.eq_ignore_ascii_case(name))
}

struct WindowScan {
    titles: HashMap<u32, String>,
    pending: HashMap<u32, String>,
    text: Vec<u16>,
}

impl WindowScan {
    fn refresh(
        &mut self,
        live: &HashMap<u32, LiveProc>,
    ) -> windows::core::Result<HashMap<u32, String>> {
        self.pending.clear();

        // Xóa mã lỗi cũ: desktop không có cửa sổ có thể trả FALSE mà không đặt lỗi.
        unsafe { SetLastError(ERROR_SUCCESS) };
        // EnumWindows gọi đồng bộ, nên con trỏ tới trạng thái còn hiệu lực trong callback.
        let result =
            unsafe { EnumWindows(Some(window_callback), LPARAM(self as *mut Self as isize)) };
        if let Err(err) = result {
            if err.code().0 != 0 {
                return Err(err);
            }
        }
        self.pending.retain(|pid, _| live.contains_key(pid));
        std::mem::swap(&mut self.titles, &mut self.pending);
        Ok(self.titles.clone())
    }

    fn title(&mut self, hwnd: HWND) -> String {
        loop {
            // Đọc chú thích trực tiếp, tránh gửi WM_GETTEXT tới luồng giao diện.
            let len = unsafe { InternalGetWindowText(hwnd, &mut self.text) };
            if len <= 0 {
                return String::new();
            }
            if len as usize + 1 < self.text.len() || self.text.len() == MAX_PATH_CHARS {
                return self
                    .text
                    .get(..len as usize)
                    .map(String::from_utf16_lossy)
                    .unwrap_or_default();
            }
            self.text.resize(self.text.len() * 2, 0);
        }
    }
}

unsafe extern "system" fn window_callback(hwnd: HWND, state: LPARAM) -> BOOL {
    // HWND do EnumWindows cung cấp, có thể biến mất giữa các lần đọc.
    let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
    // GetWindow trả Err khi HWND chủ sở hữu rỗng.
    if !visible || unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok() {
        return BOOL(1);
    }
    // Đọc cờ kiểu cửa sổ, loại cửa sổ công cụ.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    if style & WS_EX_TOOLWINDOW.0 as isize != 0 {
        return BOOL(1);
    }
    let mut pid = 0;
    // Windows ghi PID vào biến cục bộ.
    if unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) } == 0 || pid == 0 {
        return BOOL(1);
    }
    // Tham số trỏ tới WindowScan do lời gọi EnumWindows đang mượn độc quyền.
    let scan = unsafe { &mut *(state.0 as *mut WindowScan) };
    if scan.pending.contains_key(&pid) {
        return BOOL(1);
    }
    let title = scan.title(hwnd);
    if title.is_empty() {
        return BOOL(1);
    }
    let mut cloaked = 0u32;
    // DWM ghi cờ ẩn vào một DWORD; cửa sổ đã biến mất cũng bị bỏ qua.
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        )
    };
    if result >= 0 && cloaked == 0 {
        scan.pending.insert(pid, title);
    }
    BOOL(1)
}

impl ProcSampler {
    #[allow(
        clippy::new_without_default,
        reason = "Giao diện công khai dùng hàm tạo new."
    )]
    pub fn new() -> Self {
        // Đếm CPU trên mọi nhóm xử lý để tính tỷ lệ trên toàn máy.
        let logical_cpus = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }.max(1);
        let mut windows_root = windir().to_lowercase();
        if !windows_root.ends_with('\\') {
            windows_root.push('\\');
        }
        Self {
            buf: vec![0; 4 * 1024 * 1024 / size_of::<u64>()],
            offsets: Vec::with_capacity(512),
            live: HashMap::with_capacity(512),
            last: None,
            logical_cpus,
            windows_root,
            details: Details::new(),
            windows: WindowScan {
                titles: HashMap::new(),
                pending: HashMap::new(),
                text: vec![0; 512],
            },
            window_exes: HashSet::new(),
            windows_in: 0,
        }
    }

    pub fn sample(&mut self) -> ProcSample {
        let used = query_snapshot(&mut self.buf);
        let now = Instant::now();
        let t_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as i64);
        let snapshot = bytes(&self.buf, used.unwrap_or_default());
        if !snapshot_offsets(snapshot, &mut self.offsets) {
            // Giữ dữ liệu trước đó khi kernel chưa trả được một ảnh chụp đầy đủ.
            let rows = self
                .live
                .values()
                .map(|p| ProcRow {
                    cpu: 0.0,
                    disk: 0,
                    ..p.row
                })
                .collect();
            return make_sample(t_ms, rows, Vec::new(), Vec::new(), None);
        }

        let elapsed = self
            .last
            .replace(now)
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64());
        let cpu_factor = 100.0 / (elapsed * 10_000_000.0 * f64::from(self.logical_cpus));
        let mut rows = Vec::with_capacity(self.offsets.len().saturating_sub(1));
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut window_membership_changed = false;
        for &offset in &self.offsets {
            // Các vị trí đã được kiểm tra đủ 256 byte và cấu trúc chỉ chứa số nguyên.
            let raw = unsafe {
                snapshot
                    .as_ptr()
                    .add(offset)
                    .cast::<RawProc>()
                    .read_unaligned()
            };
            if raw.pid == 0 {
                continue;
            }
            let pid = raw.pid as u32;
            let cpu_ticks = raw.user_time.max(0) as u64 + raw.kernel_time.max(0) as u64;
            let io_bytes = raw.read_bytes.max(0) as u64 + raw.write_bytes.max(0) as u64;
            let mut row = ProcRow {
                pid,
                cpu: 0.0,
                mem: raw.private_ws.max(0) as u64,
                disk: 0,
                threads: raw.threads,
                handles: raw.handles,
            };
            let entry = match self.live.entry(pid) {
                Entry::Occupied(entry) if entry.get().created == raw.created => {
                    let old = entry.into_mut();
                    if elapsed > 0.0 {
                        row.cpu = (cpu_ticks.saturating_sub(old.cpu_ticks) as f64 * cpu_factor)
                            .clamp(0.0, 100.0) as f32;
                        row.disk = (io_bytes.saturating_sub(old.io_bytes) as f64 / elapsed) as u64;
                    }
                    old
                }
                entry => {
                    if matches!(&entry, Entry::Occupied(_)) {
                        removed.push(pid);
                        window_membership_changed |= self.windows.titles.remove(&pid).is_some();
                    }
                    let mut data = self.details.read(&raw, snapshot);
                    let exe_lower = data.exe.to_lowercase();
                    if !data.exe.is_empty() {
                        let file = self
                            .details
                            .paths
                            .entry(exe_lower.clone())
                            .or_insert_with(|| FileDetails::read(&data.exe));
                        data.desc.clone_from(&file.desc);
                        data.publisher.clone_from(&file.publisher);
                    }
                    let is_windows = is_windows(&data, &exe_lower, &self.windows_root);
                    added.push(pid);
                    entry
                        .insert_entry(LiveProc {
                            data,
                            exe_lower,
                            created: raw.created,
                            cpu_ticks,
                            io_bytes,
                            row,
                            seen: true,
                            is_windows,
                        })
                        .into_mut()
                }
            };
            entry.cpu_ticks = cpu_ticks;
            entry.io_bytes = io_bytes;
            entry.row = row;
            entry.seen = true;
            rows.push(row);
        }
        self.live.retain(|&pid, proc| {
            if std::mem::take(&mut proc.seen) {
                return true;
            }
            removed.push(pid);
            window_membership_changed |= self.windows.titles.remove(&pid).is_some();
            false
        });
        // Chỉ lọc đường dẫn khi có tiến trình kết thúc hoặc PID bị tái sử dụng.
        if !removed.is_empty() {
            let live_paths: HashSet<&str> =
                self.live.values().map(|p| p.exe_lower.as_str()).collect();
            self.details
                .paths
                .retain(|path, _| live_paths.contains(path.as_str()));
        }

        let windows = if self.windows_in == 0 {
            self.windows.refresh(&self.live).ok()
        } else {
            None
        };
        self.windows_in = (self.windows_in + 1) % WINDOW_INTERVAL;
        window_membership_changed |= windows.is_some();
        if window_membership_changed {
            self.window_exes.clear();
            for pid in self.windows.titles.keys() {
                let Some(proc) = self.live.get(pid) else {
                    continue;
                };
                if !proc.exe_lower.is_empty() && !self.window_exes.contains(&proc.exe_lower) {
                    self.window_exes.insert(proc.exe_lower.clone());
                }
            }
            for proc in self.live.values_mut() {
                proc.update_kind(&self.windows.titles, &self.window_exes);
            }
        }
        let added = added
            .into_iter()
            .filter_map(|pid| {
                let proc = self.live.get_mut(&pid)?;
                if !window_membership_changed {
                    proc.update_kind(&self.windows.titles, &self.window_exes);
                }
                Some(proc.data.clone())
            })
            .collect();
        make_sample(t_ms, rows, added, removed, windows)
    }

    /// Tiêu đề cửa sổ hiện tại theo PID.
    pub fn window_titles(&self) -> &HashMap<u32, String> {
        &self.windows.titles
    }

    pub fn all_static(&self) -> Vec<ProcStatic> {
        self.live.values().map(|p| p.data.clone()).collect()
    }

    pub fn get_static(&self, pid: u32) -> Option<&ProcStatic> {
        self.live.get(&pid).map(|p| &p.data)
    }

    /// FILETIME gốc để đối chiếu với GetProcessTimes khi thao tác theo PID.
    pub(crate) fn creation_time(&self, pid: u32) -> Option<u64> {
        self.live.get(&pid).map(|p| p.created as u64)
    }

    pub fn exe_of(&self, pid: u32) -> Option<&str> {
        self.live
            .get(&pid)
            .map(|p| p.exe_lower.as_str())
            .filter(|exe| !exe.is_empty())
    }

    pub fn children_of(&self, pid: u32) -> Vec<u32> {
        let Some(parent) = self.live.get(&pid) else {
            return Vec::new();
        };
        self.live
            .values()
            .filter_map(|child| {
                (child.data.ppid == pid && child.data.pid != pid && child.created >= parent.created)
                    .then_some(child.data.pid)
            })
            .collect()
    }
}

impl LiveProc {
    fn update_kind(&mut self, titles: &HashMap<u32, String>, window_exes: &HashSet<String>) {
        let kind = if self.is_windows {
            "windows"
        } else if titles.contains_key(&self.data.pid) || window_exes.contains(&self.exe_lower) {
            "app"
        } else {
            "background"
        };
        if self.data.kind != kind {
            self.data.kind.clear();
            self.data.kind.push_str(kind);
        }
    }
}

fn make_sample(
    t_ms: i64,
    rows: Vec<ProcRow>,
    added: Vec<ProcStatic>,
    removed: Vec<u32>,
    windows: Option<HashMap<u32, String>>,
) -> ProcSample {
    let (threads, handles) = rows.iter().fold((0u32, 0u32), |(threads, handles), row| {
        (
            threads.saturating_add(row.threads),
            handles.saturating_add(row.handles),
        )
    });
    let totals = Totals {
        processes: rows.len() as u32,
        threads,
        handles,
    };
    ProcSample {
        t_ms,
        rows,
        added,
        removed,
        totals,
        windows,
    }
}
