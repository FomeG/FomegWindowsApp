// Đo RAM tiến trình native bằng một lời gọi NtQuerySystemInformation(SystemProcessInformation):
// lấy PID + tên + Private Working Set (giống cột "Bộ nhớ" của Task Manager) của MỌI tiến trình,
// kể cả tiến trình của SYSTEM / được bảo vệ, mà không cần mở handle từng tiến trình.
// Đường dẫn exe (để khớp mục khởi động) chỉ đọc một lần cho mỗi tiến trình mới rồi cache theo (PID, thời điểm tạo).
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret_len: *mut u32) -> i32;
}

const SYSTEM_PROCESS_INFORMATION: u32 = 5;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

// Vị trí các trường trong SYSTEM_PROCESS_INFORMATION (x64).
const OFF_NEXT: usize = 0;
const OFF_PRIVATE_WS: usize = 8;
const OFF_CREATE_TIME: usize = 32;
const OFF_NAME_LEN: usize = 56;
const OFF_NAME_PTR: usize = 64;
const OFF_PID: usize = 80;

pub struct Proc {
    pub pid: u32,
    /// Đường dẫn exe viết thường (rỗng nếu không đọc được)
    pub exe: String,
    pub name: String,
    pub bytes: u64,
}

#[derive(Default)]
pub struct Sampler {
    buf: Vec<u64>,
    /// pid -> (thời điểm tạo, đường dẫn exe)
    paths: HashMap<u32, (i64, String)>,
}

fn exe_of(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let s = if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
            String::from_utf16_lossy(&buf[..len as usize]).to_lowercase()
        } else {
            String::new()
        };
        let _ = CloseHandle(h);
        s
    }
}

unsafe fn rd<T: Copy>(base: *const u8, off: usize) -> T {
    std::ptr::read_unaligned(base.add(off) as *const T)
}

impl Sampler {
    fn query(&mut self) -> bool {
        if self.buf.is_empty() {
            self.buf = vec![0u64; 512 * 1024 / 8];
        }
        for _ in 0..4 {
            let mut need = 0u32;
            let st = unsafe {
                NtQuerySystemInformation(
                    SYSTEM_PROCESS_INFORMATION,
                    self.buf.as_mut_ptr() as *mut c_void,
                    (self.buf.len() * 8) as u32,
                    &mut need,
                )
            };
            if st >= 0 {
                return true;
            }
            if st != STATUS_INFO_LENGTH_MISMATCH {
                return false;
            }
            let want = (need as usize).max(self.buf.len() * 8) + 64 * 1024;
            self.buf = vec![0u64; want.div_ceil(8)];
        }
        false
    }

    pub fn sample(&mut self) -> Vec<Proc> {
        let mut out = Vec::with_capacity(400);
        if !self.query() {
            return out;
        }
        let mut seen: HashSet<u32> = HashSet::with_capacity(400);
        let base = self.buf.as_ptr() as *const u8;
        let end = self.buf.len() * 8;
        let mut off = 0usize;
        loop {
            if off + OFF_PID + 8 > end {
                break;
            }
            unsafe {
                let e = base.add(off);
                let next: u32 = rd(e, OFF_NEXT);
                let pid = rd::<usize>(e, OFF_PID) as u32;
                if pid != 0 {
                    let ws: i64 = rd(e, OFF_PRIVATE_WS);
                    let created: i64 = rd(e, OFF_CREATE_TIME);
                    let nlen: u16 = rd(e, OFF_NAME_LEN);
                    let nptr: *const u16 = rd(e, OFF_NAME_PTR);
                    let name = if nptr.is_null() || nlen == 0 {
                        String::new()
                    } else {
                        String::from_utf16_lossy(std::slice::from_raw_parts(nptr, nlen as usize / 2))
                    };
                    seen.insert(pid);
                    let exe = match self.paths.get(&pid) {
                        Some((t, p)) if *t == created => p.clone(),
                        _ => {
                            let p = exe_of(pid);
                            self.paths.insert(pid, (created, p.clone()));
                            p
                        }
                    };
                    out.push(Proc { pid, exe, name, bytes: ws.max(0) as u64 });
                }
                if next == 0 {
                    break;
                }
                off += next as usize;
            }
        }
        self.paths.retain(|pid, _| seen.contains(pid));
        out
    }
}

/// (đã dùng, tổng) RAM vật lý, byte.
pub fn system_memory() -> (u64, u64) {
    unsafe {
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if GlobalMemoryStatusEx(&mut m).is_ok() {
            (m.ullTotalPhys - m.ullAvailPhys, m.ullTotalPhys)
        } else {
            (0, 0)
        }
    }
}
