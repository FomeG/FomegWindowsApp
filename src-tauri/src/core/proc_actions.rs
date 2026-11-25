//! Thao tác tiến trình và mở giao diện Explorer bằng Windows API.

use std::collections::HashMap;
use crate::tr;
use std::ffi::{c_void, OsString};
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows::core::{w, Error, BOOL, HRESULT, PCWSTR, PWSTR};
use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessInformation};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER,
    ERROR_INVALID_PARAMETER, ERROR_NOT_FOUND, ERROR_SUCCESS, FILETIME, HANDLE, LUID,
    RPC_E_CHANGED_MODE, UNICODE_STRING, WAIT_OBJECT_0, WAIT_TIMEOUT, WIN32_ERROR,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, DuplicateTokenEx, EqualSid, GetTokenInformation, IsWellKnownSid,
    LookupPrivilegeValueW, SecurityImpersonation, TokenElevation, TokenPrimary, TokenSessionId,
    TokenUser, WinLocalSystemSid, LUID_AND_ATTRIBUTES, PSID, SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED,
    TOKEN_ACCESS_MASK, TOKEN_ADJUST_PRIVILEGES, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
    TOKEN_INFORMATION_CLASS, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
use windows::Win32::System::Threading::{
    CreateProcessW, CreateProcessWithTokenW, GetCurrentProcess, GetProcessInformation,
    GetProcessTimes, IsProcessCritical, OpenProcess, OpenProcessToken, ProcessProtectionLevelInfo,
    QueryFullProcessImageNameW, SetPriorityClass, TerminateProcess, WaitForSingleObject,
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW,
    CREATE_PROCESS_LOGON_FLAGS, DETACHED_PROCESS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS, PROCESS_ACCESS_RIGHTS, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION,
    PROCESS_NAME_WIN32, PROCESS_PROTECTION_LEVEL_INFORMATION, PROCESS_QUERY_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, PROTECTION_LEVEL_NONE, REALTIME_PRIORITY_CLASS, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_INVOKEIDLIST, SEE_MASK_NOASYNC,
    SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GetWindowThreadProcessId, SW_SHOWNORMAL,
};

const EXIT_WAIT: Duration = Duration::from_secs(3);
const GONE: &str = "procs.err.gone";
const CRITICAL: &str = "procs.err.critical";
const PPL: &str = "procs.err.ppl";
const SERVICE: &str = "procs.err.service";
const STILL_RUNNING: &str = "procs.err.still_running";

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // Đóng handle do Windows cấp cho mô-đun này.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn open_process(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> windows::core::Result<Handle> {
    // PID và quyền truy cập được truyền theo giá trị.
    unsafe { OpenProcess(access, false, pid) }.map(Handle)
}

fn open_token(process: HANDLE, access: TOKEN_ACCESS_MASK) -> windows::core::Result<Handle> {
    let mut token = HANDLE::default();
    // Windows ghi handle token vào vùng nhớ cục bộ.
    unsafe { OpenProcessToken(process, access, &mut token) }?;
    Ok(Handle(token))
}

fn is_error(error: &Error, code: WIN32_ERROR) -> bool {
    error.code() == HRESULT::from_win32(code.0)
}

fn is_gone(error: &Error) -> bool {
    is_error(error, ERROR_INVALID_PARAMETER) || is_error(error, ERROR_NOT_FOUND)
}

/// `action` = khoá yml của việc đang làm.
fn action_error(action: &str, error: &Error) -> String {
    let action = tr!(action);
    if is_error(error, ERROR_ACCESS_DENIED) {
        tr!("procs.err.access_denied", action = action)
    } else {
        tr!("procs.err.win_code", action = action, code = format!("{:08X}", error.code().0 as u32))
    }
}

fn open_error(action: &str, error: &Error) -> String {
    if is_gone(error) {
        tr!(GONE)
    } else {
        action_error(action, error)
    }
}

/// Bật SeDebugPrivilege cho token ứng dụng nếu Windows cấp quyền này.
pub fn enable_debug_privilege() -> bool {
    enable_privilege(SE_DEBUG_NAME)
}

/// Bật một quyền (SeDebug, SeBackup, SeRestore…) cho token ứng dụng nếu token có quyền đó.
pub fn enable_privilege(name: PCWSTR) -> bool {
    // Handle giả của tiến trình hiện tại không cần đóng.
    let Ok(token) = open_token(
        unsafe { GetCurrentProcess() },
        TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
    ) else {
        return false;
    };
    let mut luid = LUID::default();
    // Tên quyền là hằng UTF-16 và Windows ghi LUID vào biến cục bộ.
    if unsafe { LookupPrivilegeValueW(PCWSTR::null(), name, &mut luid) }.is_err() {
        return false;
    }
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // Windows có thể báo thành công dù token không có quyền này; đọc mã lỗi ngay.
    unsafe {
        AdjustTokenPrivileges(token.0, false, Some(&privileges), 0, None, None).is_ok()
            && GetLastError() == ERROR_SUCCESS
    }
}

fn check_pid(pid: u32) -> Result<(), String> {
    match pid {
        0 | 4 => Err(tr!(CRITICAL)),
        pid if pid == std::process::id() => {
            Err(tr!("procs.err.self"))
        }
        _ => Ok(()),
    }
}

fn image_path(process: HANDLE) -> windows::core::Result<Vec<u16>> {
    let mut path = vec![0; 512];
    loop {
        let mut len = path.len() as u32;
        // Windows ghi tối đa len ký tự UTF-16 vào bộ đệm.
        match unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut len,
            )
        } {
            Ok(()) => {
                path.truncate(len as usize);
                return Ok(path);
            }
            Err(error) if is_error(&error, ERROR_INSUFFICIENT_BUFFER) && path.len() < 32768 => {
                path.resize(path.len() * 2, 0);
            }
            Err(error) => return Err(error),
        }
    }
}

fn check_critical(process: HANDLE, path: &[u16]) -> Result<(), String> {
    let name = path
        .rsplit(|&c| c == u16::from(b'\\'))
        .next()
        .unwrap_or(path);
    let name = String::from_utf16_lossy(name);
    if [
        "csrss.exe",
        "wininit.exe",
        "winlogon.exe",
        "smss.exe",
        "services.exe",
        "lsass.exe",
        "System",
        "Registry",
        "Memory Compression",
        "MemCompression",
    ]
    .iter()
    .any(|n| n.eq_ignore_ascii_case(&name))
    {
        return Err(tr!(CRITICAL));
    }
    check_critical_state(process)
}

fn check_critical_state(process: HANDLE) -> Result<(), String> {
    let mut critical = BOOL(0);
    // Cần xác nhận cờ hệ thống trước khi cho phép kết thúc.
    unsafe { IsProcessCritical(process, &mut critical) }
        .map_err(|e| action_error("procs.act.verify_critical", &e))?;
    if critical.as_bool() {
        return Err(tr!(CRITICAL));
    }
    Ok(())
}

fn is_protected(process: HANDLE) -> bool {
    let mut info = PROCESS_PROTECTION_LEVEL_INFORMATION::default();
    // Bộ đệm khớp với lớp ProcessProtectionLevelInfo.
    unsafe {
        GetProcessInformation(
            process,
            ProcessProtectionLevelInfo,
            (&mut info as *mut PROCESS_PROTECTION_LEVEL_INFORMATION).cast(),
            size_of_val(&info) as u32,
        )
        .is_ok()
            && info.ProtectionLevel != PROTECTION_LEVEL_NONE
    }
}

fn creation_time(process: HANDLE) -> windows::core::Result<u64> {
    let [mut created, mut exited, mut kernel, mut user] = [FILETIME::default(); 4];
    // Windows ghi bốn mốc thời gian vào các biến cục bộ.
    unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) }?;
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

fn wait_ms(deadline: Instant) -> u32 {
    deadline
        .saturating_duration_since(Instant::now())
        .as_nanos()
        .div_ceil(1_000_000) as u32
}

fn wait_exit(process: HANDLE, millis: u32) -> Result<bool, String> {
    // Handle có quyền đồng bộ và còn hiệu lực trong toàn bộ lần chờ.
    match unsafe { WaitForSingleObject(process, millis) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(action_error(
            "procs.act.wait_exit",
            &Error::from_thread(),
        )),
    }
}

struct Target {
    handle: Handle,
    image: Vec<u16>,
    native_terminate: bool,
}

impl Target {
    fn open(pid: u32, expected_time: Option<u64>) -> Result<Self, String> {
        check_pid(pid)?;
        let query = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
        let access = query | PROCESS_TERMINATE;
        let first = open_process(pid, access);
        let opened = match first {
            Err(ref err) if is_error(err, ERROR_ACCESS_DENIED) => {
                enable_debug_privilege();
                open_process(pid, access)
            }
            _ => first,
        };
        let (handle, native_terminate) = match opened {
            Ok(handle) => (handle, true),
            Err(error) if is_error(&error, ERROR_ACCESS_DENIED) => {
                let handle = open_process(pid, query).map_err(|err| {
                    if open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)
                        .is_ok_and(|h| is_protected(h.0))
                    {
                        tr!(PPL)
                    } else {
                        open_error("procs.act.open_to_kill", &err)
                    }
                })?;
                (handle, false)
            }
            Err(error) => return Err(open_error("procs.act.open_to_kill", &error)),
        };

        if let Some(expected) = expected_time {
            let actual = creation_time(handle.0)
                .map_err(|err| action_error("procs.act.read_created", &err))?;
            if actual != expected {
                return Err(tr!(GONE));
            }
        }
        if wait_exit(handle.0, 0)? {
            return Err(tr!(GONE));
        }
        if !native_terminate && is_protected(handle.0) {
            return Err(tr!(PPL));
        }
        let image = image_path(handle.0)
            .map_err(|err| action_error("procs.act.read_name", &err))?;
        check_critical(handle.0, &image)?;
        Ok(Self {
            handle,
            image,
            native_terminate,
        })
    }

    fn exit(&self, pid: u32, deadline: Instant) -> Result<(), String> {
        check_critical_state(self.handle.0)?;
        if self.native_terminate {
            // Kết thúc trực tiếp bằng handle đã được kiểm tra.
            if let Err(err) = unsafe { TerminateProcess(self.handle.0, 1) } {
                if wait_exit(self.handle.0, 0)? {
                    return Err(tr!(GONE));
                }
                if !is_error(&err, ERROR_ACCESS_DENIED) {
                    return Err(action_error("procs.act.kill", &err));
                }
                if is_protected(self.handle.0) {
                    return Err(tr!(PPL));
                }
                self.taskkill(pid, deadline)?;
            }
        } else {
            self.taskkill(pid, deadline)?;
        }
        if wait_exit(self.handle.0, wait_ms(deadline))? {
            Ok(())
        } else {
            Err(tr!(STILL_RUNNING))
        }
    }

    fn taskkill(&self, pid: u32, deadline: Instant) -> Result<(), String> {
        // Giữ handle gốc để Windows không tái sử dụng PID trong lúc chạy taskkill.
        if wait_exit(self.handle.0, 0)? {
            return Err(tr!(GONE));
        }
        if wait_ms(deadline) == 0 {
            return Err(tr!(STILL_RUNNING));
        }
        let exe = windows_directory(GetSystemDirectoryW)?.join("taskkill.exe");
        let mut child = Command::new(exe)
            .args(["/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW.0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| io_error("procs.act.run_taskkill", &err))?;
        let waited = wait_exit(HANDLE(child.as_raw_handle()), wait_ms(deadline));
        if !matches!(waited, Ok(true)) {
            let _ = child.kill();
            if wait_exit(self.handle.0, 0)? {
                return Ok(());
            }
            waited?;
            return Err(tr!(STILL_RUNNING));
        }
        let status = child
            .wait()
            .map_err(|err| io_error("procs.act.read_taskkill", &err))?;
        if status.success() || wait_exit(self.handle.0, 0)? {
            Ok(())
        } else {
            Err(tr!("procs.err.taskkill_denied", code = status.code().unwrap_or(-1)))
        }
    }
}

const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005_u32 as i32;
const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023_u32 as i32;

fn nt_buffer(
    initial_bytes: usize,
    query: impl Fn(*mut c_void, u32, &mut u32) -> i32,
) -> Result<(Vec<usize>, usize), i32> {
    let mut bytes = initial_bytes;
    for _ in 0..6 {
        let mut buffer = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let len = size_of_val(buffer.as_slice());
        let mut used = 0;
        let status = query(buffer.as_mut_ptr().cast(), len as u32, &mut used);
        if status >= 0 && used as usize <= len {
            return Ok((buffer, used as usize));
        }
        if !matches!(
            status,
            STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_OVERFLOW | STATUS_BUFFER_TOO_SMALL
        ) {
            return Err(status);
        }
        bytes = (len * 2).max(used as usize);
        if bytes > 64 * 1024 * 1024 {
            break;
        }
    }
    Err(STATUS_BUFFER_TOO_SMALL)
}

#[repr(C)]
struct ProcessEntryPrefix {
    next: u32,
    thread_count: u32,
    private_working_set: i64,
    hard_fault_count: u32,
    thread_count_high_watermark: u32,
    cycle_time: u64,
    created: u64,
    user_time: i64,
    kernel_time: i64,
    image_name: UNICODE_STRING,
    base_priority: i32,
    pid: usize,
    ppid: usize,
}

#[derive(Clone, Copy)]
struct ProcessEntry {
    pid: u32,
    ppid: u32,
    created: u64,
}

fn process_snapshot() -> Result<Vec<ProcessEntry>, String> {
    let (buffer, used) = nt_buffer(512 * 1024, |ptr, len, used| {
        // Windows nhận bộ đệm được căn chỉnh và giới hạn theo len.
        unsafe { NtQuerySystemInformation(SystemProcessInformation, ptr, len, used).0 }
    })
    .map_err(|status| {
        tr!("procs.err.read_tree", status = format!("{:08X}", status as u32))
    })?;
    let invalid = || tr!("procs.err.bad_tree");
    let mut entries = Vec::new();
    let mut offset = 0;
    while offset + size_of::<ProcessEntryPrefix>() <= used {
        // Kiểm tra kích thước mục trước khi đọc; offset do Windows trả về tính theo byte.
        let entry = unsafe {
            buffer
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<ProcessEntryPrefix>()
                .read_unaligned()
        };
        entries.push(ProcessEntry {
            pid: entry.pid as u32,
            ppid: entry.ppid as u32,
            created: entry.created,
        });
        if entry.next == 0 {
            return Ok(entries);
        }
        if (entry.next as usize) < size_of::<ProcessEntryPrefix>() {
            break;
        }
        offset += entry.next as usize;
    }
    Err(invalid())
}

fn descendants(root: ProcessEntry, entries: Vec<ProcessEntry>) -> Vec<ProcessEntry> {
    let mut by_parent: HashMap<u32, Vec<ProcessEntry>> = HashMap::new();
    for entry in entries {
        by_parent.entry(entry.ppid).or_default().push(entry);
    }
    let mut ordered = vec![root];
    let mut index = 0;
    while let Some(parent) = ordered.get(index).copied() {
        if let Some(children) = by_parent.remove(&parent.pid) {
            ordered.extend(
                children
                    .into_iter()
                    .filter(|child| child.created > parent.created),
            );
        }
        index += 1;
    }
    ordered
}

/// Kết thúc tiến trình ngay. Nếu chọn cả cây, xử lý các tiến trình con sâu nhất trước.
pub fn kill(pid: u32, tree: bool) -> Result<(), String> {
    let root = Target::open(pid, None)?;
    if !tree {
        return root.exit(pid, Instant::now() + EXIT_WAIT);
    }

    let created = creation_time(root.handle.0)
        .map_err(|err| action_error("procs.act.read_root_created", &err))?;
    let entries = process_snapshot()?;
    let ordered = descendants(
        ProcessEntry {
            pid,
            ppid: 0,
            created,
        },
        entries,
    );
    let deadline = Instant::now() + EXIT_WAIT;
    let mut child_error = None;
    for child in ordered[1..].iter().rev() {
        if let Err(err) = Target::open(child.pid, Some(child.created))
            .and_then(|target| target.exit(child.pid, deadline))
        {
            if err != tr!(GONE) {
                child_error.get_or_insert_with(|| format!("PID {}: {err}", child.pid));
            }
        }
    }
    if let Err(err) = root.exit(pid, deadline) {
        // Tiến trình gốc có thể tự thoát khi tiến trình con kết thúc.
        if !wait_exit(root.handle.0, 0)? {
            return Err(err);
        }
    }
    child_error.map_or(Ok(()), Err)
}

fn command_line(process: HANDLE) -> Result<Vec<u16>, String> {
    let (buffer, used) = nt_buffer(1024, |ptr, len, used| {
        // Windows ghi UNICODE_STRING cùng nội dung vào cùng bộ đệm.
        unsafe {
            NtQueryInformationProcess(process, ProcessCommandLineInformation, ptr, len, used).0
        }
    })
    .map_err(|status| {
        tr!("procs.err.read_cmdline", status = format!("{:08X}", status as u32))
    })?;
    let invalid = || tr!("procs.err.bad_cmdline");
    if used < size_of::<UNICODE_STRING>() {
        return Err(invalid());
    }
    // Bộ đệm căn chỉnh theo con trỏ và có đủ chỗ cho phần đầu.
    let text = unsafe { &*buffer.as_ptr().cast::<UNICODE_STRING>() };
    let start = (text.Buffer.0 as usize)
        .checked_sub(buffer.as_ptr() as usize)
        .ok_or_else(invalid)?;
    let len = usize::from(text.Length);
    if len == 0 || len % 2 != 0 || start % 2 != 0 || start > used || len > used - start {
        return Err(invalid());
    }
    // Đọc qua bộ đệm gốc sau khi kiểm tra vị trí, độ dài và căn chỉnh.
    let command = unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<u16>().add(start / 2), len / 2)
    };
    if command.contains(&0) {
        return Err(invalid());
    }
    let mut copy = command.to_vec();
    copy.push(0);
    Ok(copy)
}

fn token_number(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> windows::core::Result<u32> {
    let mut value = 0u32;
    let mut used = 0;
    // TokenElevation và TokenSessionId đều dùng một DWORD.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some((&mut value as *mut u32).cast()),
            4,
            &mut used,
        )
    }?;
    Ok(value)
}

struct TokenSid(Box<[usize; 16]>);

impl TokenSid {
    fn read(token: HANDLE) -> windows::core::Result<Self> {
        let mut buffer = Box::new([0usize; 16]);
        let mut used = 0;
        // Bộ đệm căn chỉnh chứa TOKEN_USER và SID; địa chỉ giữ nguyên khi chuyển Box.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                size_of_val(buffer.as_ref()) as u32,
                &mut used,
            )
        }?;
        Ok(Self(buffer))
    }

    fn get(&self) -> PSID {
        // TokenUser đã điền cấu trúc và SID còn sống trong Box.
        unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    fn is_system(&self) -> bool {
        // SID đã được Windows điền và còn hiệu lực.
        unsafe { IsWellKnownSid(self.get(), WinLocalSystemSid) }.as_bool()
    }

    fn matches(&self, other: &Self) -> bool {
        // Cả hai SID đều còn hiệu lực trong bộ đệm riêng.
        unsafe { EqualSid(self.get(), other.get()) }.is_ok()
    }
}

fn shell_token(session: u32, user: &TokenSid) -> Result<Handle, String> {
    let mut pid = 0;
    // Lấy PID của Explorer đang quản lý desktop tương tác.
    let thread = unsafe {
        let shell = GetShellWindow();
        GetWindowThreadProcessId(shell, Some(&mut pid))
    };
    let unavailable =
        || tr!("procs.err.no_shell_token");
    if thread == 0 || pid == 0 {
        return Err(unavailable());
    }
    let shell = open_process(pid, PROCESS_QUERY_INFORMATION)
        .map_err(|err| action_error("procs.act.open_explorer", &err))?;
    let token = open_token(shell.0, TOKEN_DUPLICATE)
        .map_err(|err| action_error("procs.act.open_explorer_token", &err))?;
    let mut primary = HANDLE::default();
    // Windows ghi token sơ cấp mới vào handle; không cho tiến trình con thừa kế handle.
    unsafe {
        DuplicateTokenEx(
            token.0,
            TOKEN_ASSIGN_PRIMARY | TOKEN_DUPLICATE | TOKEN_QUERY,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &mut primary,
        )
    }
    .map_err(|err| action_error("procs.act.dup_explorer_token", &err))?;
    let primary = Handle(primary);
    if token_number(primary.0, TokenElevation) != Ok(0)
        || token_number(primary.0, TokenSessionId) != Ok(session)
        || !TokenSid::read(primary.0).is_ok_and(|sid| sid.matches(user))
    {
        return Err(unavailable());
    }
    Ok(primary)
}

/// Khởi động lại với dòng lệnh, người dùng, phiên và mức nâng quyền của tiến trình cũ.
pub fn restart(pid: u32) -> Result<u32, String> {
    check_pid(pid)?;
    let mut session = 0;
    // Phiên 0 bị từ chối ngay cả khi ứng dụng không đọc được token đích.
    if unsafe { ProcessIdToSessionId(pid, &mut session) }.is_ok() && session == 0 {
        return Err(tr!(SERVICE));
    }
    let target = Target::open(pid, None)?;
    let token_error = |err: Error| action_error("procs.act.read_token", &err);
    let target_token = open_token(target.handle.0, TOKEN_QUERY).map_err(token_error)?;
    let session = token_number(target_token.0, TokenSessionId).map_err(token_error)?;
    let user = TokenSid::read(target_token.0).map_err(token_error)?;
    if session == 0 || user.is_system() {
        return Err(tr!(SERVICE));
    }
    let elevated = token_number(target_token.0, TokenElevation).map_err(token_error)? != 0;
    // Handle giả của tiến trình hiện tại không cần đóng.
    let current = open_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).map_err(token_error)?;
    if token_number(current.0, TokenSessionId).map_err(token_error)? != session {
        return Err(tr!("procs.err.other_session"));
    }
    let current_elevated = token_number(current.0, TokenElevation).map_err(token_error)? != 0;
    let plain_allowed = current_elevated == elevated
        && TokenSid::read(current.0)
            .map_err(token_error)?
            .matches(&user);
    let shell = if elevated {
        if !plain_allowed {
            return Err(tr!("procs.err.restart_denied"));
        }
        None
    } else {
        match shell_token(session, &user) {
            Ok(token) => Some(token),
            Err(_) if plain_allowed => None,
            Err(err) => return Err(err),
        }
    };

    let original_cmd = command_line(target.handle.0)?;
    // CreateProcessWithTokenW nhận tối đa 1024 đơn vị UTF-16, chưa tính NUL.
    let shell = if original_cmd.len() > 1025 {
        if !plain_allowed {
            return Err(tr!("procs.err.cmdline_too_long"));
        }
        None
    } else {
        shell
    };
    let exe = PathBuf::from(OsString::from_wide(&target.image));
    let parent = exe
        .parent()
        .ok_or_else(|| tr!("procs.err.no_parent"))?;
    let dir: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(parent.as_os_str())
        .chain(std::iter::once(0))
        .collect();
    let mut image = target.image.clone();
    image.push(0);
    let image = PCWSTR(image.as_ptr());
    let dir = PCWSTR(dir.as_ptr());
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    target.exit(pid, Instant::now() + EXIT_WAIT)?;

    if let Some(shell) = shell {
        let mut cmd = original_cmd.clone();
        // Các chuỗi kết thúc bằng NUL và dòng lệnh nằm trong bộ đệm có thể ghi.
        let launch = unsafe {
            CreateProcessWithTokenW(
                shell.0,
                CREATE_PROCESS_LOGON_FLAGS(0),
                image,
                Some(PWSTR(cmd.as_mut_ptr())),
                PROCESS_CREATION_FLAGS(0),
                None,
                dir,
                &startup,
                &mut info,
            )
        };
        match launch {
            Ok(()) => return Ok(new_pid(info)),
            Err(err) if !plain_allowed => {
                return Err(action_error(
                    "procs.act.relaunch_shell",
                    &err,
                ));
            }
            Err(_) => {}
        }
    }

    let mut cmd = original_cmd;
    // Kế thừa token đã được kiểm tra; không truyền handle mở sang tiến trình con.
    unsafe {
        CreateProcessW(
            image,
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(0),
            None,
            dir,
            &startup,
            &mut info,
        )
    }
    .map_err(|err| {
        action_error(
            "procs.act.relaunch",
            &err,
        )
    })?;
    Ok(new_pid(info))
}

fn new_pid(info: PROCESS_INFORMATION) -> u32 {
    let _process = Handle(info.hProcess);
    let _thread = Handle(info.hThread);
    info.dwProcessId
}

pub fn set_priority(pid: u32, level: &str) -> Result<(), String> {
    let priority = match level {
        "idle" => IDLE_PRIORITY_CLASS,
        "below" => BELOW_NORMAL_PRIORITY_CLASS,
        "normal" => NORMAL_PRIORITY_CLASS,
        "above" => ABOVE_NORMAL_PRIORITY_CLASS,
        "high" => HIGH_PRIORITY_CLASS,
        "realtime" => REALTIME_PRIORITY_CLASS,
        _ => return Err(tr!("procs.err.bad_priority")),
    };
    let process = open_process(pid, PROCESS_SET_INFORMATION)
        .map_err(|err| open_error("procs.act.open_priority", &err))?;
    // Handle có quyền đổi thông tin tiến trình và mức ưu tiên là hằng Win32.
    unsafe { SetPriorityClass(process.0, priority) }
        .map_err(|err| action_error("procs.act.set_priority", &err))
}

fn windows_directory(get: unsafe fn(Option<&mut [u16]>) -> u32) -> Result<PathBuf, String> {
    let mut path = vec![0u16; 260];
    loop {
        // Hàm Win32 nhận độ dài của vùng nhớ và trả kích thước cần nếu thiếu chỗ.
        let len = unsafe { get(Some(&mut path)) } as usize;
        if len == 0 {
            return Err(action_error(
                "procs.act.windows_dir",
                &Error::from_thread(),
            ));
        }
        if len < path.len() {
            return Ok(PathBuf::from(OsString::from_wide(&path[..len])));
        }
        path.resize(len + 1, 0);
    }
}

/// `action` = khoá yml của việc đang làm.
fn io_error(action: &str, err: &std::io::Error) -> String {
    let action = tr!(action);
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        tr!("procs.err.access_denied", action = action)
    } else {
        tr!("procs.err.sys_code", action = action, code = err.raw_os_error().unwrap_or(-1))
    }
}

fn checked_path(path: &str) -> Result<&Path, String> {
    if path.is_empty() || path.contains(['\0', '"']) {
        return Err(tr!("procs.err.bad_path"));
    }
    let path = Path::new(path);
    path.metadata()
        .map_err(|err| io_error("procs.act.access_path", &err))?;
    Ok(path)
}

pub fn open_location(path: &str) -> Result<(), String> {
    checked_path(path)?;
    let explorer = windows_directory(GetWindowsDirectoryW)?.join("explorer.exe");
    Command::new(explorer)
        .raw_arg(format!("/select,\"{path}\""))
        .creation_flags(DETACHED_PROCESS.0)
        .spawn()
        .map_err(|err| io_error("procs.act.open_location", &err))?;
    Ok(())
}

pub fn properties(path: &str) -> Result<(), String> {
    checked_path(path)?;
    let file: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    // Shell có thể dùng COM; giữ nguyên apartment nếu luồng đã khởi tạo kiểu khác.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if com.is_err() && com != RPC_E_CHANGED_MODE {
        return Err(action_error(
            "procs.act.init_com",
            &Error::from_hresult(com),
        ));
    }

    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("properties"),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // Chuỗi UTF-16 và cấu trúc còn hiệu lực trong lúc gửi yêu cầu tới Shell.
    let result = unsafe { ShellExecuteExW(&mut info) };
    if com.is_ok() {
        // Cân bằng cả S_OK và S_FALSE trên đúng luồng khởi tạo.
        unsafe { CoUninitialize() };
    }
    result.map_err(|err| action_error("procs.act.properties", &err))
}
