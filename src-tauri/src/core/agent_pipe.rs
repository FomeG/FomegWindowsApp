// Named pipe nối MCP server (fomeg-windows-app-mcp.exe) với app đang mở, để agent bên ngoài thấy dữ liệu SỐNG của app: lịch sử
// hiệu năng 1 giờ, kết quả quét ổ đĩa và dọn dẹp đang có. App không mở thì MCP tự đo tại chỗ như trước.
//
// An toàn:
// - Pipe chỉ nhận lời gọi tool của Toolbox (core/agent_tools.rs), vốn CHỈ ĐỌC. Không có lệnh thao tác nào qua pipe.
// - DACL chỉ cho tài khoản đang chạy app (+ SYSTEM), nhãn toàn vẹn Medium để MCP chạy quyền thường kết nối được tới app
//   chạy Admin; từ chối client từ máy khác (PIPE_REJECT_REMOTE_CLIENTS); tên pipe theo phiên đăng nhập.
// - Client chỉ nói chuyện với pipe mà tiến trình chủ là fomeg-windows-app.exe (chống giả mạo pipe).
//
// Giao thức: mỗi dòng một JSON.
//   -> {"op":"call","name":"process_tree","args":{"pid":1}}   <- {"ok":true,"text":"…"} | {"ok":false,"text":"lỗi"}
//   -> {"op":"ping"}                                            <- {"ok":true,"text":"<app_info>"}

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

use super::agent_tools::{AppData, Toolbox};
use super::util::wide;

/// Số client cùng lúc (mỗi client một luồng + một Toolbox riêng).
const MAX_CLIENTS: u32 = 4;
/// Trần một dòng yêu cầu.
const MAX_LINE: u64 = 64 * 1024;
/// Exe được tin là app (kiểm tra tiến trình chủ của pipe).
const APP_EXE: &str = "fomeg-windows-app.exe";

/// Tên pipe của phiên đăng nhập hiện tại.
pub fn pipe_name() -> String {
    let mut session = 0u32;
    // chỉ đọc số phiên của chính tiến trình này
    let _ = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) };
    format!(r"\\.\pipe\fomeg-agent-{session}")
}

// ---------------------------------------------------------------------------
// Server (trong app)

/// Mở pipe trên một luồng nền. `app` cung cấp dữ liệu chỉ app có cho Toolbox của mỗi client.
pub fn spawn(name: String, app: Arc<dyn AppData + Send + Sync>) {
    let _ = std::thread::Builder::new().name("agent-pipe".into()).spawn(move || {
        if let Err(e) = serve(&name, app) {
            super::util::log_line(&format!("[MCP] Không mở được pipe cho agent: {e}"));
        }
    });
}

fn serve(name: &str, app: Arc<dyn AppData + Send + Sync>) -> Result<(), String> {
    let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;{})S:(ML;;NW;;;ME)", user_sid()?);
    let sd = SecurityDescriptor::new(&sddl)?;
    let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0 .0, bInheritHandle: false.into() };
    let wname = wide(name);
    let active = Arc::new(AtomicUsize::new(0));
    let mut first = true;
    loop {
        let mut mode = PIPE_ACCESS_DUPLEX;
        if first {
            mode |= FILE_FLAG_FIRST_PIPE_INSTANCE; // có pipe cùng tên rồi (app khác / giả mạo) thì không mở
        }
        // tên, SA còn sống suốt vòng lặp
        let h = unsafe { CreateNamedPipeW(PCWSTR(wname.as_ptr()), mode, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, MAX_CLIENTS, 64 * 1024, 64 * 1024, 0, Some(&sa)) };
        if h.is_invalid() {
            let e = unsafe { GetLastError() };
            if first {
                return Err(format!("CreateNamedPipe: {e:?}"));
            }
            std::thread::sleep(Duration::from_millis(500)); // đủ MAX_CLIENTS: đợi một client đóng
            continue;
        }
        first = false;
        // chờ client; ERROR_PIPE_CONNECTED = client đã nối trước khi gọi
        let ok = unsafe { ConnectNamedPipe(h, None) }.is_ok() || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !ok || active.load(Ordering::SeqCst) >= MAX_CLIENTS as usize {
            let _ = unsafe { CloseHandle(h) };
            continue;
        }
        // pipe vừa tạo, chuyển quyền sở hữu handle cho File (tự đóng khi client xong)
        let file = unsafe { File::from_raw_handle(h.0) };
        let (app, active) = (app.clone(), active.clone());
        active.fetch_add(1, Ordering::SeqCst);
        let _ = std::thread::Builder::new().name("agent-pipe-client".into()).spawn(move || {
            let _ = client(file, &*app);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// Phục vụ một client: mỗi dòng một yêu cầu, chỉ chạy tool của Toolbox (chỉ đọc).
fn client(file: File, app: &dyn AppData) -> std::io::Result<()> {
    let mut out = file.try_clone()?;
    let mut input = BufReader::new(file);
    let mut tools = Toolbox::new(Some(app));
    let names: Vec<String> = tools.tools().iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect();
    let mut line = String::new();
    loop {
        line.clear();
        if (&mut input).take(MAX_LINE).read_line(&mut line)? == 0 {
            return Ok(());
        }
        let req: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
        let reply = match req["op"].as_str() {
            Some("ping") => json!({ "ok": true, "text": app.app_info() }),
            Some("call") => {
                let name = req["name"].as_str().unwrap_or("");
                if names.iter().any(|n| n == name) {
                    match tools.run(name, &req["args"]) {
                        Ok(text) => json!({ "ok": true, "text": text }),
                        Err(text) => json!({ "ok": false, "text": text }),
                    }
                } else {
                    json!({ "ok": false, "text": format!("Không có tool `{name}`.") })
                }
            }
            _ => json!({ "ok": false, "text": "Yêu cầu không hợp lệ." }),
        };
        serde_json::to_writer(&mut out, &reply)?;
        out.write_all(b"\n")?;
        out.flush()?;
    }
}

/// SID dạng chuỗi của tài khoản đang chạy tiến trình này.
fn user_sid() -> Result<String, String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|e| e.to_string())?;
    let mut buf = [0usize; 64];
    let mut used = 0;
    // bộ đệm căn chỉnh đủ cho TOKEN_USER + SID
    let r = unsafe { GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), size_of_val(&buf) as u32, &mut used) };
    let _ = unsafe { CloseHandle(token) };
    r.map_err(|e| e.to_string())?;
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(|e| e.to_string())?;
    // Windows cấp chuỗi bằng LocalAlloc
    let s = unsafe { text.to_string() }.map_err(|e| e.to_string());
    let _ = unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    s
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn new(sddl: &str) -> Result<Self, String> {
        let w = wide(sddl);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(w.as_ptr()), SDDL_REVISION_1, &mut sd, None) }.map_err(|e| e.to_string())?;
        Ok(Self(sd))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // cấp bằng LocalAlloc trong ConvertStringSecurityDescriptor…
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0 .0))) };
    }
}

// ---------------------------------------------------------------------------
// Client (trong fomeg-windows-app-mcp)

pub struct Remote {
    input: BufReader<File>,
    out: File,
}

impl Remote {
    /// Nối tới app nếu đang mở. `verify` = chỉ nhận pipe do fomeg-windows-app.exe mở.
    pub fn connect(name: &str, verify: bool) -> Option<Self> {
        let file = std::fs::OpenOptions::new().read(true).write(true).open(name).ok()?;
        if verify && !server_is_app(&file) {
            return None;
        }
        Some(Self { input: BufReader::new(file.try_clone().ok()?), out: file })
    }

    fn ask(&mut self, req: &Value) -> std::io::Result<(bool, String)> {
        serde_json::to_writer(&mut self.out, req)?;
        self.out.write_all(b"\n")?;
        self.out.flush()?;
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        let v: Value = serde_json::from_str(&line).map_err(std::io::Error::other)?;
        Ok((v["ok"].as_bool().unwrap_or(false), v["text"].as_str().unwrap_or("").to_owned()))
    }

    /// Gọi một tool trong app. Err = mất kết nối (người gọi tự chạy tại chỗ).
    pub fn call(&mut self, name: &str, args: &Value) -> std::io::Result<Result<String, String>> {
        let (ok, text) = self.ask(&json!({ "op": "call", "name": name, "args": args }))?;
        Ok(if ok { Ok(text) } else { Err(text) })
    }

    pub fn ping(&mut self) -> std::io::Result<String> {
        self.ask(&json!({ "op": "ping" })).map(|(_, t)| t)
    }
}

/// Tiến trình mở pipe có phải app không (tên exe).
fn server_is_app(file: &File) -> bool {
    let mut pid = 0u32;
    // handle pipe còn sống trong `file`
    if unsafe { GetNamedPipeServerProcessId(HANDLE(file.as_raw_handle()), &mut pid) }.is_err() {
        return false;
    }
    let Ok(p) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return false };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let r = unsafe { QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
    let _ = unsafe { CloseHandle(p) };
    r.is_ok() && String::from_utf16_lossy(&buf[..len as usize]).rsplit('\\').next().is_some_and(|n| n.eq_ignore_ascii_case(APP_EXE))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl AppData for Fake {
        fn history(&self, minutes: u32) -> Option<String> {
            Some(format!("lịch sử giả {minutes} phút"))
        }
        fn disk_summary(&self, _: Option<&str>) -> Option<String> {
            None
        }
        fn cleanup_summary(&self) -> Option<String> {
            None
        }
        fn app_info(&self) -> String {
            "app giả".into()
        }
    }

    #[test]
    fn mcp_reaches_the_open_app_through_the_pipe_read_only() {
        let name = format!(r"\\.\pipe\fomeg-agent-test-{}", std::process::id());
        spawn(name.clone(), Arc::new(Fake));
        let mut remote = None;
        for _ in 0..50 {
            remote = Remote::connect(&name, false);
            if remote.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut r = remote.expect("nối được pipe");
        assert_eq!(r.ping().unwrap(), "app giả");
        // dữ liệu chỉ app có đi qua pipe
        assert_eq!(r.call("metrics_history", &json!({ "minutes": 5 })).unwrap(), Ok("lịch sử giả 5 phút".into()));
        // không có tool thao tác; tên lạ bị từ chối
        assert!(r.call("end_process", &json!({ "pid": 4 })).unwrap().is_err());
        assert!(r.call("drives", &json!({})).unwrap().unwrap().contains(':'));
        // pipe của test không phải do fomeg-windows-app.exe mở -> client thật (verify) không nhận
        assert!(Remote::connect(&name, true).is_none());
    }
}
