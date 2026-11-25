// MCP server (Model Context Protocol) qua stdin/stdout, CHỈ ĐỌC: Claude Code / Claude Desktop / coding agent khác khởi chạy
// `fomeg-windows-app-mcp.exe` và hỏi máy này đang làm gì bằng cùng bộ tool với trợ lý trong app (core/agent_tools.rs).
// Không nghe mạng, không có tool thao tác. Theo RustWTM crates/wtm-cli/src/mcp.rs.
// App đang mở thì mỗi lời gọi tool đi qua named pipe (core/agent_pipe.rs) để dùng dữ liệu SỐNG của app (lịch sử 1 giờ,
// kết quả quét ổ đĩa / dọn dẹp); app đóng thì tự đo tại chỗ. Bản release của app nhúng sẵn exe này và tự giải ra
// %LOCALAPPDATA%\fomeg-windows-app\fomeg-windows-app-mcp.exe (`install`).
//
// Cấu hình Claude Code:  claude mcp add fomeg-windows-app -- "C:\đường\dẫn\fomeg-windows-app-mcp.exe"

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::agent_pipe::{self, Remote};
use super::agent_tools::Toolbox;

/// Sau khi không nối được app, đợi bấy lâu mới thử lại (app có thể vừa mở).
const RETRY: Duration = Duration::from_secs(5);

/// Các bản giao thức hỗ trợ, mới nhất trước.
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub struct Server {
    tools: Toolbox<'static>,
    /// Pipe tới app đang mở (None = chưa nối / app đóng). `pipe` = None thì không bao giờ nối (test).
    remote: Option<Remote>,
    pipe: Option<String>,
    tried: Option<Instant>,
}

impl Default for Server {
    fn default() -> Self {
        Self { tools: Toolbox::new(None), remote: None, pipe: Some(agent_pipe::pipe_name()), tried: None }
    }
}

impl Server {
    /// Trả lời một tin JSON-RPC; None với thông báo (notification) hoặc tin trả lời của client.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        if !msg.is_object() {
            return Some(error(Value::Null, -32600, "invalid request: expected one JSON-RPC object"));
        }
        let method = msg.get("method")?;
        let id = msg.get("id").cloned()?;
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(error(id, -32600, "invalid request: `jsonrpc` must be \"2.0\""));
        }
        let result = match method.as_str() {
            Some("initialize") => Ok(initialize(&params)),
            Some("ping") => Ok(json!({})),
            Some("tools/list") => Ok(json!({ "tools": self.tools.tools() })),
            Some("tools/call") => self.call(&params),
            Some("prompts/list") => Ok(json!({ "prompts": prompts() })),
            Some("prompts/get") => prompt(&params),
            Some(m) => Err((-32601, format!("method not found: {m}"))),
            None => Err((-32600, "invalid request: `method` must be a string".into())),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, m)) => error(id, code, &m),
        })
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "`name` is required".to_owned()))?;
        let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        if !self.tools.tools().iter().any(|t| t["name"] == name) {
            return Err((-32602, format!("unknown tool: {name}")));
        }
        // lỗi của tool (tham số sai, không tìm thấy…) là KẾT QUẢ có isError, không phải lỗi giao thức
        let result = self.via_app(name, &args).unwrap_or_else(|| self.tools.run(name, &args));
        Ok(match result {
            Ok(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
            Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
        })
    }
}

impl Server {
    /// Chạy tool trong app đang mở. None = không có app (hoặc vừa mất kết nối) -> chạy tại chỗ.
    fn via_app(&mut self, name: &str, args: &Value) -> Option<Result<String, String>> {
        if self.remote.is_none() {
            let pipe = self.pipe.as_deref()?;
            if self.tried.is_some_and(|t| t.elapsed() < RETRY) {
                return None;
            }
            self.tried = Some(Instant::now());
            self.remote = Remote::connect(pipe, true);
        }
        match self.remote.as_mut()?.call(name, args) {
            Ok(r) => Some(r),
            Err(_) => {
                self.remote = None; // app vừa đóng
                None
            }
        }
    }
}

/// Nơi app giải fomeg-windows-app-mcp.exe nhúng sẵn.
pub fn installed_path() -> Option<PathBuf> {
    super::util::data_dir().map(|d| d.join("fomeg-windows-app-mcp.exe"))
}

/// Giải exe MCP nhúng trong app ra `installed_path()` nếu chưa có hoặc khác bản (app vừa cập nhật). Exe cũ đang chạy
/// (agent đang mở) vẫn đổi tên được, nên đổi sang `.old` rồi ghi bản mới.
pub fn install(bytes: &[u8]) -> Result<PathBuf, String> {
    let dest = installed_path().ok_or("Không có %LOCALAPPDATA%")?;
    if std::fs::read(&dest).is_ok_and(|old| old == bytes) {
        return Ok(dest);
    }
    let dir = dest.parent().ok_or("đường dẫn lạ")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join("fomeg-windows-app-mcp.new");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    let old = dir.join("fomeg-windows-app-mcp.old");
    let _ = std::fs::remove_file(&old);
    if dest.exists() {
        std::fs::rename(&dest, &old).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&old); // còn chạy thì để lần sau xoá
    super::util::log_line("[MCP] Đã giải fomeg-windows-app-mcp.exe");
    Ok(dest)
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    let version = PROTOCOLS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOLS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {}, "prompts": {} },
        "serverInfo": { "name": "fomeg-windows-app", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "Read-only view of this Windows PC from the Vietnamese's Ultimate W App: processes and process trees, \
startup items, services, listening ports, drives, cleanup candidates, hardware. Nothing here changes the PC. \
Explain to the user what to do in the app instead (it asks them to confirm every change)."
    })
}

fn prompts() -> Value {
    json!([
        { "name": "why_slow", "description": "Vì sao máy chậm? Xem tải hiện tại, tiến trình nặng và mục khởi động." },
        { "name": "app_ram", "description": "Một ứng dụng tốn bao nhiêu RAM tính cả tiến trình con.", "arguments": [{ "name": "name", "description": "Tên ứng dụng, vd. chrome", "required": true }] },
        { "name": "free_space", "description": "Gợi ý dọn dẹp an toàn để giải phóng ổ đĩa." }
    ])
}

fn prompt(params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arg = |k: &str| params["arguments"][k].as_str().unwrap_or("").to_owned();
    let text = match name {
        "why_slow" => "Máy tôi đang chậm. Dùng system_overview, list_processes và startup_items để tìm nguyên nhân rồi gợi ý cách xử lý trong ứng dụng Vietnamese's Ultimate W App.".to_owned(),
        "app_ram" => format!("Ứng dụng \"{}\" đang tốn bao nhiêu RAM tính cả các tiến trình con? Dùng list_processes (group) rồi process_tree.", arg("name")),
        "free_space" => "Ổ đĩa sắp đầy. Dùng drives và cleanup_report để gợi ý những gì dọn an toàn.".to_owned(),
        _ => return Err((-32602, format!("unknown prompt: {name}"))),
    };
    Ok(json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Vòng lặp stdio: mỗi dòng một tin JSON-RPC, mỗi câu trả lời một dòng. Kết thúc khi stdin đóng.
pub fn serve(input: impl BufRead, output: impl Write) -> std::io::Result<()> {
    serve_with(Server::default(), input, output)
}

fn serve_with(mut server: Server, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(_) => Some(error(Value::Null, -32700, "parse error")),
        };
        if let Some(r) = reply {
            serde_json::to_writer(&mut output, &r)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

pub fn serve_stdio() -> std::io::Result<()> {
    serve(std::io::stdin().lock(), std::io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_json_rpc_and_only_lists_read_only_tools() {
        let input = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"drives","arguments":{}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"process_report","arguments":{}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"end_process","arguments":{"pid":4}}}),
            json!({"jsonrpc":"2.0","id":6,"method":"nope"}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let mut out = Vec::new();
        // không nối app thật trong test
        serve_with(Server { pipe: None, ..Server::default() }, input.as_bytes(), &mut out).unwrap();
        let replies: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(replies.len(), 6, "thông báo không được trả lời");
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
        let names: Vec<&str> = replies[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"process_tree") && names.contains(&"cleanup_report"));
        for bad in ["end", "kill", "delete", "stop", "set_", "disable", "clean_now"] {
            assert!(!names.iter().any(|n| n.contains(bad)), "tool thao tác {bad}");
        }
        assert_eq!(replies[2]["result"]["isError"], false);
        assert!(replies[2]["result"]["content"][0]["text"].as_str().unwrap().contains(':'));
        assert_eq!(replies[3]["result"]["isError"], true);
        assert_eq!(replies[4]["error"]["code"], -32602);
        assert_eq!(replies[5]["error"]["code"], -32601);
    }
}
