// Không mở cửa sổ console ở bản release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Bản release nhúng sẵn fomeg-windows-app-mcp.exe (MCP server chỉ đọc, build trước bằng `npm run build:mcp`), app tự giải ra
// %LOCALAPPDATA%\fomeg-windows-app khi mở: chỉ cần phát hành một exe. Xem build.rs.
#[cfg(embed_mcp)]
static MCP_EXE: &[u8] = include_bytes!(env!("FOMEG_MCP_EXE"));
#[cfg(not(embed_mcp))]
static MCP_EXE: &[u8] = &[];

fn main() {
    fomeg_windows_app_lib::run(MCP_EXE)
}
