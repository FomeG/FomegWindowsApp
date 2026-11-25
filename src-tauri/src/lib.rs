// Vietnamese's Ultimate W App (quản lý khởi động, dịch vụ, tiến trình, hiệu năng Windows) – backend Tauri.
// Mọi việc nặng chạy trong spawn_blocking / luồng riêng để giao diện không bao giờ bị treo.
pub mod core;

// Chuỗi đa ngôn ngữ của backend (locales/*.yml, vi mặc định). Dùng qua macro `tr!` (core/i18n.rs).
rust_i18n::i18n!("locales", fallback = "vi");
mod commands;
mod monitor;
mod state;
mod update;

use std::sync::Arc;

use tauri::Manager;

pub(crate) fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut tok = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok).is_err() {
            return false;
        }
        let mut el = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            tok,
            TokenElevation,
            Some(&mut el as *mut _ as *mut std::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(tok);
        ok && el.TokenIsElevated != 0
    }
}

/// fomeg-windows-app-mcp.exe nhúng trong exe của app (rỗng ở bản dev / khi chưa build MCP). Xem main.rs.
pub(crate) static MCP_EXE: std::sync::OnceLock<&'static [u8]> = std::sync::OnceLock::new();

pub fn run(mcp_exe: &'static [u8]) {
    let _ = MCP_EXE.set(mcp_exe);
    // dữ liệu của tên cũ (QuanLyKhoiDong) -> tên mới; phải trước khi tạo webview
    crate::core::migrate::run();
    // Quyền SeDebugPrivilege giúp đọc thông tin và kết thúc tiến trình của người dùng khác (khi chạy Admin).
    crate::core::proc_actions::enable_debug_privilege();
    let st = Arc::new(state::AppState::new());
    tauri::Builder::default()
        .plugin(update::plugin())
        .manage(st.clone())
        .manage(update::Pending::default())
        .setup(move |app| {
            monitor::spawn(app.handle().clone(), st.clone());
            // Agent bên ngoài (fomeg-windows-app-mcp) hỏi dữ liệu sống của app qua named pipe – chỉ đọc. Kèm giải MCP nhúng ra đĩa.
            crate::core::agent_pipe::spawn(crate::core::agent_pipe::pipe_name(), Arc::new(commands::AppView(st.clone())));
            if !mcp_exe.is_empty() {
                std::thread::spawn(move || {
                    if let Err(e) = crate::core::mcp::install(mcp_exe) {
                        crate::core::util::log_line(&format!("[MCP] Không giải được fomeg-windows-app-mcp.exe: {e}"));
                    }
                });
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("Vietnamese's Ultimate W App");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            update::update_info,
            update::update_check,
            update::update_install,
            commands::scan,
            commands::toggle_items,
            commands::service_set_mode,
            commands::service_control,
            commands::open_location,
            commands::get_settings,
            commands::set_log_dir,
            commands::set_restart_apps,
            commands::open_log,
            commands::mcp_info,
            commands::ram_history,
            commands::system_info,
            commands::metrics_history,
            commands::set_interval,
            commands::set_idle_interval,
            commands::set_locale,
            commands::relaunch_admin,
            commands::set_visible,
            commands::set_watch,
            commands::proc_kill,
            commands::proc_lookup,
            commands::proc_restart,
            commands::proc_open_location,
            commands::proc_properties,
            commands::proc_set_priority,
            commands::get_icons,
            commands::ports_list,
            commands::open_port,
            commands::open_tool,
            commands::ai_get_config,
            commands::ai_set_config,
            commands::ai_list_models,
            commands::ai_send,
            commands::ai_cancel,
            commands::ai_reset,
            commands::apps_list,
            commands::apps_measure,
            commands::apps_measure_stop,
            commands::apps_uninstall,
            commands::apps_modify,
            commands::apps_uninstall_stop,
            commands::apps_open,
            commands::apps_related,
            commands::leftovers_scan,
            commands::leftovers_remove_packages,
            commands::leftovers_forget,
            commands::cleanup_scan,
            commands::cleanup_apply,
            commands::du_drives,
            commands::du_start,
            commands::du_progress,
            commands::du_children,
            commands::du_files,
            commands::du_cancel,
            commands::du_close,
            commands::du_forget,
            commands::du_forget_file,
            commands::du_delete,
            commands::du_delete_paths,
            commands::du_lookup,
            commands::du_exists,
            commands::du_open,
            commands::du_menu,
            commands::du_pick_folder
        ])
        .run(tauri::generate_context!())
        .expect("lỗi khi chạy ứng dụng");
}
