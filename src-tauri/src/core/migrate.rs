// Chuyển dữ liệu từ tên cũ (quanlykhoidong / QuanLyKhoiDong, trước khi đổi tên thành fomeg-windows-app) sang tên mới.
// Chạy một lần lúc mở app, TRƯỚC khi tạo webview (thư mục dữ liệu WebView2 theo identifier). Lỗi -> bỏ qua, ghi log;
// không bao giờ xoá dữ liệu cũ khi chưa chép xong.
//  - %LOCALAPPDATA%\QuanLyKhoiDong        -> %LOCALAPPDATA%\fomeg-windows-app   (ai.json có khoá DPAPI, cache, lịch sử app, log)
//  - %LOCALAPPDATA%\com.quanlykhoidong.app -> %LOCALAPPDATA%\com.fomeg.windows-app (WebView2: chủ đề, ngôn ngữ, tuỳ chọn giao diện)
//  - HKCU\Software\QuanLyKhoiDong\Disabled -> HKCU\Software\fomeg-windows-app\Disabled (mục khởi động đã tắt – để bật lại được)
use std::fs;
use std::path::Path;

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

use super::util;

const OLD_DIR: &str = "QuanLyKhoiDong";
const OLD_WEBVIEW: &str = "com.quanlykhoidong.app";
const NEW_WEBVIEW: &str = "com.fomeg.windows-app";
const OLD_REG: &str = r"Software\QuanLyKhoiDong";
const OLD_MCP: [&str; 2] = ["qlkd-mcp.exe", "qlkd-mcp.exe.old"];

pub fn run() {
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        let base = Path::new(&base);
        if let Some(new) = util::data_dir() {
            move_dir(&base.join(OLD_DIR), &new, true);
        }
        move_dir(&base.join(OLD_WEBVIEW), &base.join(NEW_WEBVIEW), false);
    }
    registry();
}

/// Đổi tên thư mục; không được (tệp đang mở, vd. MCP cũ đang chạy) thì chép từng tệp cấp 1. `data` = thư mục dữ liệu
/// của app: bỏ qua exe MCP cũ (app tự giải bản mới). Thư mục mới đã có -> không làm gì (đã chuyển rồi).
fn move_dir(old: &Path, new: &Path, data: bool) {
    if !old.is_dir() || new.exists() {
        return;
    }
    if fs::rename(old, new).is_ok() {
        if data {
            for f in OLD_MCP {
                let _ = fs::remove_file(new.join(f));
            }
        }
        util::log_line(&format!("Đã chuyển {} -> {}", old.display(), new.display()));
        return;
    }
    if !data {
        // WebView2 khoá cả cây khi đang chạy; chép từng phần dễ hỏng hồ sơ -> để nguyên (chỉ mất tuỳ chọn giao diện)
        util::log_line(&format!("Không chuyển được {} (đang dùng)", old.display()));
        return;
    }
    let Ok(rd) = fs::read_dir(old) else { return };
    if fs::create_dir_all(new).is_err() {
        return;
    }
    for e in rd.flatten() {
        let name = e.file_name();
        if OLD_MCP.iter().any(|f| name.eq_ignore_ascii_case(f)) || !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let _ = fs::copy(e.path(), new.join(&name));
    }
    util::log_line(&format!("Đã chép dữ liệu từ {} (thư mục cũ còn lại, xoá được)", old.display()));
}

/// Chép các giá trị của HKCU\Software\QuanLyKhoiDong\Disabled sang khoá mới rồi mới xoá khoá cũ.
fn registry() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(old) = hkcu.open_subkey_with_flags(format!(r"{OLD_REG}\Disabled"), KEY_READ) else { return };
    let Some(new_path) = super::registry::BACKUP_KEY.strip_prefix(r"HKCU\") else { return };
    let Ok((new, _)) = hkcu.create_subkey(new_path) else { return };
    let mut ok = true;
    for v in old.enum_values() {
        match v {
            Ok((name, value)) => {
                // giá trị trùng tên ở khoá mới (hiếm) -> giữ cái mới
                if new.get_raw_value(&name).is_err() && new.set_raw_value(&name, &value).is_err() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
    }
    drop(old);
    if ok {
        let _ = hkcu.delete_subkey_all(OLD_REG);
        util::log_line("Đã chuyển mục khởi động đã tắt sang khoá registry mới.");
    } else {
        util::log_line("Chưa chuyển hết mục khởi động đã tắt; giữ khoá registry cũ.");
    }
}
