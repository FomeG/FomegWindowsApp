// Dịch vụ qua Service Control Manager API (thay cho Get-CimInstance + sc.exe).
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::core::BOOL;
use windows::Win32::System::Services::*;

use super::util::{pwstr, wide, win_err};
use super::is_critical;
use crate::tr;

pub struct SvcInfo {
    pub name: String,
    pub display: String,
    pub path: String,
    pub start_type: String,
    pub state: String,
    pub pid: u32,
    pub description: String,
    /// Tài khoản chạy dịch vụ (LocalSystem, NT AUTHORITY\LocalService…)
    pub account: String,
}

struct H(SC_HANDLE);
impl Drop for H {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

/// Bộ đệm căn lề 8 byte (các struct trả về chứa con trỏ).
struct Buf(Vec<u64>);
impl Buf {
    fn new(bytes: usize) -> Self {
        Buf(vec![0u64; bytes.div_ceil(8)])
    }
    fn bytes(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut u8, self.0.len() * 8) }
    }
    fn ptr<T>(&self) -> *const T {
        self.0.as_ptr() as *const T
    }
    fn len(&self) -> usize {
        self.0.len() * 8
    }
}

fn scm(access: u32) -> Result<H, String> {
    unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), access).map(H).map_err(|e| win_err(&e)) }
}

fn open(m: &H, name: &str, access: u32) -> Result<H, String> {
    let w = wide(name);
    unsafe { OpenServiceW(m.0, PCWSTR(w.as_ptr()), access).map(H).map_err(|e| win_err(&e)) }
}

fn state_str(s: SERVICE_STATUS_CURRENT_STATE) -> &'static str {
    match s {
        SERVICE_RUNNING => "running",
        SERVICE_STOPPED => "stopped",
        SERVICE_START_PENDING => "starting",
        SERVICE_STOP_PENDING => "stopping",
        SERVICE_PAUSED => "paused",
        _ => "pending",
    }
}

fn start_str(t: SERVICE_START_TYPE, delayed: bool) -> &'static str {
    match t {
        SERVICE_AUTO_START => {
            if delayed {
                "delayed"
            } else {
                "auto"
            }
        }
        SERVICE_DEMAND_START => "demand",
        SERVICE_DISABLED => "disabled",
        SERVICE_BOOT_START => "boot",
        SERVICE_SYSTEM_START => "system",
        _ => "demand",
    }
}

/// Cấu hình (kiểu khởi động, đường dẫn, mô tả) của một dịch vụ đã mở.
/// (đường dẫn, kiểu khởi động, mô tả, tài khoản)
fn query_config(h: &H, buf: &mut Buf) -> (String, String, String, String) {
    let mut path = String::new();
    let mut account = String::new();
    let mut st = SERVICE_DEMAND_START;
    let mut delayed = false;
    let mut desc = String::new();
    unsafe {
        let mut need = 0u32;
        let mut r = QueryServiceConfigW(h.0, Some(buf.ptr::<QUERY_SERVICE_CONFIGW>() as *mut _), buf.len() as u32, &mut need);
        if r.is_err() && need as usize > buf.len() {
            *buf = Buf::new(need as usize);
            r = QueryServiceConfigW(h.0, Some(buf.ptr::<QUERY_SERVICE_CONFIGW>() as *mut _), buf.len() as u32, &mut need);
        }
        if r.is_ok() {
            let c = &*buf.ptr::<QUERY_SERVICE_CONFIGW>();
            path = pwstr(c.lpBinaryPathName);
            account = pwstr(c.lpServiceStartName);
            st = c.dwStartType;
        }
        if st == SERVICE_AUTO_START {
            let mut need = 0u32;
            if QueryServiceConfig2W(h.0, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, Some(buf.bytes()), &mut need).is_ok() {
                delayed = (*buf.ptr::<SERVICE_DELAYED_AUTO_START_INFO>()).fDelayedAutostart.as_bool();
            }
        }
        let mut need = 0u32;
        let mut r = QueryServiceConfig2W(h.0, SERVICE_CONFIG_DESCRIPTION, Some(buf.bytes()), &mut need);
        if r.is_err() && need as usize > buf.len() {
            *buf = Buf::new(need as usize);
            r = QueryServiceConfig2W(h.0, SERVICE_CONFIG_DESCRIPTION, Some(buf.bytes()), &mut need);
        }
        if r.is_ok() {
            desc = pwstr((*buf.ptr::<SERVICE_DESCRIPTIONW>()).lpDescription);
        }
    }
    (path, start_str(st, delayed).to_string(), desc, account)
}

pub fn enumerate() -> Result<Vec<SvcInfo>, String> {
    let m = scm(SC_MANAGER_ENUMERATE_SERVICE | SC_MANAGER_CONNECT)?;
    let mut raw: Vec<(String, String, String, u32)> = Vec::new();
    let mut buf = Buf::new(256 * 1024);
    let mut resume = 0u32;
    loop {
        let mut need = 0u32;
        let mut count = 0u32;
        let r = unsafe {
            EnumServicesStatusExW(
                m.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                Some(buf.bytes()),
                &mut need,
                &mut count,
                Some(&mut resume),
                PCWSTR::null(),
            )
        };
        let arr = unsafe { std::slice::from_raw_parts(buf.ptr::<ENUM_SERVICE_STATUS_PROCESSW>(), count as usize) };
        for s in arr {
            let p = s.ServiceStatusProcess;
            raw.push((
                pwstr(s.lpServiceName),
                pwstr(s.lpDisplayName),
                state_str(p.dwCurrentState).to_string(),
                p.dwProcessId,
            ));
        }
        match r {
            Ok(()) => break,
            Err(e) if (e.code().0 as u32) & 0xFFFF == 234 => {
                if need as usize > buf.len() {
                    buf = Buf::new(need as usize);
                }
            }
            Err(e) => return Err(win_err(&e)),
        }
    }

    // Đọc cấu hình song song: mỗi dịch vụ cần 2-3 lời gọi RPC tới SCM.
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    let chunk = raw.len().div_ceil(threads).max(1);
    let mut out: Vec<SvcInfo> = Vec::with_capacity(raw.len());
    std::thread::scope(|sc| {
        let hs: Vec<_> = raw
            .chunks(chunk)
            .map(|part| {
                sc.spawn(move || {
                    let mut res = Vec::with_capacity(part.len());
                    let Ok(m) = scm(SC_MANAGER_CONNECT) else { return res };
                    let mut buf = Buf::new(8 * 1024);
                    for (name, display, state, pid) in part {
                        let (path, start_type, description, account) = match open(&m, name, SERVICE_QUERY_CONFIG) {
                            Ok(h) => query_config(&h, &mut buf),
                            Err(_) => (String::new(), "demand".into(), String::new(), String::new()),
                        };
                        res.push(SvcInfo {
                            name: name.clone(),
                            display: display.clone(),
                            path,
                            start_type,
                            state: state.clone(),
                            pid: *pid,
                            description,
                            account,
                        });
                    }
                    res
                })
            })
            .collect();
        for h in hs {
            if let Ok(v) = h.join() {
                out.extend(v);
            }
        }
    });
    Ok(out)
}

fn status(h: &H) -> Option<SERVICE_STATUS_PROCESS> {
    let mut buf = Buf::new(std::mem::size_of::<SERVICE_STATUS_PROCESS>() + 8);
    let mut need = 0u32;
    unsafe {
        QueryServiceStatusEx(h.0, SC_STATUS_PROCESS_INFO, Some(buf.bytes()), &mut need).ok()?;
        Some(*buf.ptr::<SERVICE_STATUS_PROCESS>())
    }
}

/// Trạng thái mới nhất của một dịch vụ.
pub fn query_one(name: &str) -> Result<SvcInfo, String> {
    let m = scm(SC_MANAGER_CONNECT)?;
    let h = open(&m, name, SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS)?;
    let mut buf = Buf::new(8 * 1024);
    let (path, start_type, description, account) = query_config(&h, &mut buf);
    let (state, pid) = status(&h).map(|s| (state_str(s.dwCurrentState).to_string(), s.dwProcessId)).unwrap_or_default();
    // Tên hiển thị
    let mut display = name.to_string();
    unsafe {
        let mut need = 0u32;
        let _ = QueryServiceConfigW(h.0, Some(buf.ptr::<QUERY_SERVICE_CONFIGW>() as *mut _), buf.len() as u32, &mut need);
        let c = &*buf.ptr::<QUERY_SERVICE_CONFIGW>();
        let d = pwstr(c.lpDisplayName);
        if !d.is_empty() {
            display = d;
        }
    }
    Ok(SvcInfo { name: name.to_string(), display, path, start_type, state, pid, description, account })
}

/// mode: auto | delayed | demand | disabled
pub fn set_mode(name: &str, mode: &str) -> Result<(), String> {
    if mode != "auto" && mode != "delayed" && is_critical(name) {
        return Err(tr!("services.err.critical_change"));
    }
    let st = match mode {
        "auto" | "delayed" => SERVICE_AUTO_START,
        "demand" => SERVICE_DEMAND_START,
        "disabled" => SERVICE_DISABLED,
        _ => return Err(tr!("services.err.bad_mode", mode = mode)),
    };
    let m = scm(SC_MANAGER_CONNECT)?;
    let h = open(&m, name, SERVICE_CHANGE_CONFIG | SERVICE_QUERY_STATUS | SERVICE_STOP)?;
    unsafe {
        ChangeServiceConfigW(
            h.0,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            st,
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
        .map_err(|e| win_err(&e))?;
        if st == SERVICE_AUTO_START {
            let info = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: BOOL::from(mode == "delayed") };
            let _ = ChangeServiceConfig2W(
                h.0,
                SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                Some(&info as *const _ as *const core::ffi::c_void),
            );
        }
        if st == SERVICE_DISABLED {
            let mut s = SERVICE_STATUS::default();
            let _ = ControlService(h.0, SERVICE_CONTROL_STOP, &mut s);
        }
    }
    Ok(())
}

/// Chạy / dừng rồi chờ trạng thái ổn định (tối đa ~8 giây) – gọi trong luồng nền.
pub fn control(name: &str, start: bool) -> Result<(), String> {
    if !start && is_critical(name) {
        return Err(tr!("services.err.critical_stop"));
    }
    let m = scm(SC_MANAGER_CONNECT)?;
    let h = open(&m, name, SERVICE_START | SERVICE_STOP | SERVICE_QUERY_STATUS)?;
    unsafe {
        if start {
            StartServiceW(h.0, None).map_err(|e| win_err(&e))?;
        } else {
            let mut s = SERVICE_STATUS::default();
            ControlService(h.0, SERVICE_CONTROL_STOP, &mut s).map_err(|e| win_err(&e))?;
        }
    }
    let target = if start { SERVICE_RUNNING } else { SERVICE_STOPPED };
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(8) {
        match status(&h) {
            Some(s) if s.dwCurrentState == target => return Ok(()),
            Some(s) if start && s.dwCurrentState == SERVICE_STOPPED && t0.elapsed() > Duration::from_millis(500) => {
                return Err(tr!("services.err.stopped_after_start"))
            }
            None => break,
            _ => std::thread::sleep(Duration::from_millis(120)),
        }
    }
    Ok(())
}
