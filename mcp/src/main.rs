// fomeg-windows-app-mcp: MCP server chỉ đọc qua stdin/stdout. Mọi logic nằm ở fomeg_windows_app_lib::core::mcp (dùng chung bộ tool với trợ lý
// trong app). stdout chỉ dành cho JSON-RPC; lỗi ghi ra stderr.
fn main() {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("fomeg-windows-app-mcp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if let Err(e) = fomeg_windows_app_lib::core::mcp::serve_stdio() {
        eprintln!("fomeg-windows-app-mcp: {e}");
        std::process::exit(1);
    }
}
