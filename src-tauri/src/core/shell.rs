// Thao tác Windows Shell cho trang Ổ cứng:
// - menu chuột phải GỐC của Windows (IContextMenu – giống Explorer: Mở, Open with, Send to, Defender, Copy as path…)
//   kèm vài mục riêng của app ở đầu menu;
// - xoá vào Thùng rác / xoá vĩnh viễn (SHFileOperationW, có hộp tiến trình của Windows);
// - hộp chọn thư mục (IFileOpenDialog).
// Menu và hộp chọn phải chạy trên luồng giao diện (STA, sở hữu cửa sổ) – commands.rs gọi qua run_on_main_thread.

use std::cell::RefCell;
use crate::tr;
use std::ffi::c_void;

use windows::core::{Interface, PCSTR, PCWSTR, PSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    DefSubclassProc, FileOpenDialog, IContextMenu, IContextMenu2, IContextMenu3, IFileOpenDialog, IShellFolder,
    RemoveWindowSubclass, SHBindToParent, SHFileOperationW, SHParseDisplayName, SetWindowSubclass, CMF_EXPLORE,
    CMF_EXTENDEDVERBS, CMF_NORMAL, CMINVOKECOMMANDINFOEX, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, GCS_VERBW,
    SHFILEOPSTRUCTW, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DeleteMenu, DestroyMenu, GetCursorPos, GetMenuItemCount, GetMenuItemID, InsertMenuW,
    SetForegroundWindow, TrackPopupMenuEx, HMENU, MF_BYCOMMAND, MF_BYPOSITION, MF_SEPARATOR, MF_STRING, SW_SHOWNORMAL,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_DRAWITEM, WM_INITMENUPOPUP, WM_MEASUREITEM, WM_MENUCHAR,
};

use super::util::win_err;

const SHELL_FIRST: u32 = 1;
const SHELL_LAST: u32 = 0x7FFF;
/// id các mục riêng của app (ngoài khoảng của shell)
const APP_BASE: u32 = 0x8000;
const CMIC_MASK_UNICODE: u32 = 0x4000;
const CMIC_MASK_PTINVOKE: u32 = 0x2000_0000;
const CMIC_MASK_SHIFT_DOWN: u32 = 0x1000_0000;
const FO_DELETE: u32 = 3;
const FOF_NOCONFIRMATION: u16 = 0x10;
const FOF_ALLOWUNDO: u16 = 0x40;
const FOF_WANTNUKEWARNING: u16 = 0x4000;

/// Kết quả menu: không chọn gì / mục riêng của app / lệnh shell đã chạy (kèm verb nếu có).
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MenuPick {
    None,
    App { id: String },
    Shell { verb: String },
}

thread_local! {
    // IContextMenu2/3 của menu đang mở: cửa sổ chủ chuyển tiếp thông điệp vẽ menu con (Send to, Open with…).
    static ACTIVE: RefCell<Option<(Option<IContextMenu3>, Option<IContextMenu2>)>> = const { RefCell::new(None) };
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if matches!(msg, WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR) {
        let handled = ACTIVE.with(|a| {
            let a = a.borrow();
            let (cm3, cm2) = a.as_ref()?;
            unsafe {
                if let Some(cm3) = cm3 {
                    let mut r = LRESULT(0);
                    if cm3.HandleMenuMsg2(msg, wp, lp, Some(&mut r)).is_ok() {
                        return Some(r);
                    }
                } else if let Some(cm2) = cm2 {
                    if cm2.HandleMenuMsg(msg, wp, lp).is_ok() {
                        return Some(LRESULT(0));
                    }
                }
            }
            None
        });
        if let Some(r) = handled {
            return r;
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Pidl(*mut ITEMIDLIST);
impl Drop for Pidl {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0 as *const c_void)) };
    }
}
struct Menu(HMENU);
impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}

fn verb_of(cm: &IContextMenu, offset: u32) -> String {
    let mut buf = [0u16; 128];
    unsafe {
        if cm.GetCommandString(offset as usize, GCS_VERBW, None, PSTR(buf.as_mut_ptr() as *mut u8), buf.len() as u32).is_ok() {
            return super::util::from_wide_buf(&buf).to_ascii_lowercase();
        }
    }
    String::new()
}

/// Hiện menu chuột phải gốc của Windows cho `path` tại vị trí con trỏ chuột.
/// `app_items`: (id, nhãn) thêm ở đầu menu. `shift`: hiện cả mục mở rộng (như Shift+chuột phải trong Explorer).
/// PHẢI gọi trên luồng giao diện sở hữu `hwnd`.
pub fn context_menu(hwnd: HWND, path: &str, app_items: &[(&str, &str)], shift: bool) -> Result<MenuPick, String> {
    let (cm, menu) = build_menu(hwnd, path, app_items, shift)?;
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        ACTIVE.with(|a| *a.borrow_mut() = Some((cm.cast::<IContextMenu3>().ok(), cm.cast::<IContextMenu2>().ok())));
        let sub = SetWindowSubclass(hwnd, Some(subclass), 0x514C_4B44, 0).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenuEx(menu.0, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, pt.x, pt.y, hwnd, None).0 as u32;
        if sub {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass), 0x514C_4B44);
        }
        ACTIVE.with(|a| *a.borrow_mut() = None);

        if cmd == 0 {
            return Ok(MenuPick::None);
        }
        if cmd >= APP_BASE {
            let i = (cmd - APP_BASE) as usize;
            return Ok(app_items.get(i).map_or(MenuPick::None, |(id, _)| MenuPick::App { id: (*id).to_owned() }));
        }
        let offset = cmd - SHELL_FIRST;
        let verb = verb_of(&cm, offset);
        let info = CMINVOKECOMMANDINFOEX {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE | if shift { CMIC_MASK_SHIFT_DOWN } else { 0 },
            hwnd,
            lpVerb: PCSTR(offset as usize as *const u8),
            lpVerbW: PCWSTR(offset as usize as *const u16),
            nShow: SW_SHOWNORMAL.0,
            ptInvoke: pt,
            ..Default::default()
        };
        cm.InvokeCommand(&info as *const _ as *const _).map_err(|e| win_err(&e))?;
        Ok(MenuPick::Shell { verb })
    }
}

/// Dựng menu (IContextMenu của shell + mục riêng của app), chưa hiện.
fn build_menu(hwnd: HWND, path: &str, app_items: &[(&str, &str)], shift: bool) -> Result<(IContextMenu, Menu), String> {
    unsafe {
        let wpath = wide(path);
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(PCWSTR(wpath.as_ptr()), None, &mut pidl, 0, None).map_err(|e| win_err(&e))?;
        let pidl = Pidl(pidl);
        let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
        let folder: IShellFolder = SHBindToParent(pidl.0, Some(&mut child)).map_err(|e| win_err(&e))?;
        let items = [child as *const ITEMIDLIST];
        let cm: IContextMenu = folder.GetUIObjectOf(hwnd, &items, None).map_err(|e| win_err(&e))?;

        let menu = Menu(CreatePopupMenu().map_err(|e| win_err(&e))?);
        let mut flags = CMF_NORMAL | CMF_EXPLORE;
        if shift {
            flags |= CMF_EXTENDEDVERBS;
        }
        cm.QueryContextMenu(menu.0, 0, SHELL_FIRST, SHELL_LAST, flags).ok().map_err(|e| win_err(&e))?;

        // bỏ "Đổi tên": không có ô sửa tên tại chỗ như Explorer
        let n = GetMenuItemCount(Some(menu.0));
        for pos in (0..n).rev() {
            let id = GetMenuItemID(menu.0, pos);
            if (SHELL_FIRST..=SHELL_LAST).contains(&id) && verb_of(&cm, id - SHELL_FIRST) == "rename" {
                let _ = DeleteMenu(menu.0, id, MF_BYCOMMAND);
            }
        }
        // mục riêng của app ở đầu menu
        if !app_items.is_empty() {
            let _ = InsertMenuW(menu.0, 0, MF_BYPOSITION | MF_SEPARATOR, 0, PCWSTR::null());
            for (i, (_, label)) in app_items.iter().enumerate().rev() {
                let w = wide(label);
                let _ = InsertMenuW(menu.0, 0, MF_BYPOSITION | MF_STRING, (APP_BASE + i as u32) as usize, PCWSTR(w.as_ptr()));
            }
        }

        Ok((cm, menu))
    }
}

/// Xoá các đường dẫn: vào Thùng rác (`permanent = false`) hoặc vĩnh viễn. Có hộp tiến trình của Windows;
/// app đã hỏi xác nhận trước nên không hỏi lại. Gọi trên luồng nền.
pub fn delete(paths: &[String], permanent: bool) -> Result<bool, String> {
    if paths.is_empty() {
        return Ok(true);
    }
    // danh sách kết thúc bằng hai NUL
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(p.encode_utf16());
        from.push(0);
    }
    from.push(0);
    // không FOF_SILENT: thư mục lớn vẫn có hộp tiến trình của Windows
    let mut flags = FOF_NOCONFIRMATION;
    if !permanent {
        flags |= FOF_ALLOWUNDO | FOF_WANTNUKEWARNING;
    }
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: flags,
        ..Default::default()
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        return Err(match r {
            0x78 | 5 => tr!("shell.err.delete_denied"),
            0x20 => tr!("shell.err.in_use"),
            2 | 3 | 0x7C => tr!("shell.err.not_found"),
            _ => tr!("shell.err.delete_code", code = format!("{r:#x}")),
        });
    }
    Ok(!op.fAnyOperationsAborted.as_bool())
}

/// Hộp chọn thư mục của Windows. None = người dùng huỷ. Gọi trên luồng giao diện.
pub fn pick_folder(hwnd: HWND) -> Result<Option<String>, String> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(|e| win_err(&e))?;
        let opts = dlg.GetOptions().map_err(|e| win_err(&e))?;
        dlg.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).map_err(|e| win_err(&e))?;
        let title = wide(&tr!("shell.pick_folder_title"));
        let _ = dlg.SetTitle(PCWSTR(title.as_ptr()));
        if dlg.Show(Some(hwnd)).is_err() {
            return Ok(None); // huỷ
        }
        let item = dlg.GetResult().map_err(|e| win_err(&e))?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(|e| win_err(&e))?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const c_void));
        Ok(Some(s))
    }
}

/// Đường dẫn còn tồn tại không (sau một lệnh shell như Xoá / Cắt).
pub fn exists(path: &str) -> bool {
    std::path::Path::new(path).symlink_metadata().is_ok()
}

/// Mở URL hoặc công cụ hệ thống (services.msc…).
/// URL đi qua explorer.exe để trình duyệt chạy với quyền thường (app chạy quyền Admin thì
/// ShellExecute trực tiếp sẽ mở trình duyệt với quyền Admin).
pub fn open_target(target: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    if target.starts_with("http://") || target.starts_with("https://") {
        let exe = std::path::Path::new(&super::util::windir()).join("explorer.exe");
        std::process::Command::new(exe)
            .arg(target)
            .creation_flags(0x0000_0008) // DETACHED_PROCESS
            .spawn()
            .map(|_| ())
            .map_err(|e| tr!("shell.err.open", target = target, e = e))
    } else {
        use windows::Win32::UI::Shell::ShellExecuteW;
        let file = super::util::wide(target);
        let r = unsafe { ShellExecuteW(None, windows::core::w!("open"), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        // ShellExecuteW trả về > 32 khi thành công
        if r.0 as isize > 32 {
            Ok(())
        } else {
            Err(tr!("shell.err.open_code", target = target, code = r.0 as isize))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::WindowsAndMessaging::GetMenuStringW;

    #[test]
    fn native_menu_has_explorer_items_and_app_items() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let dir = std::env::temp_dir().join(format!("fomeg-shell-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cm, menu) = build_menu(HWND::default(), dir.to_str().unwrap(), &[("scan", "Quét thư mục này")], false).unwrap();
        let mut labels = Vec::new();
        let mut verbs = Vec::new();
        unsafe {
            for pos in 0..GetMenuItemCount(Some(menu.0)) {
                let mut buf = [0u16; 256];
                let n = GetMenuStringW(menu.0, pos as u32, Some(&mut buf), MF_BYPOSITION);
                labels.push(String::from_utf16_lossy(&buf[..n.max(0) as usize]));
                let id = GetMenuItemID(menu.0, pos);
                if (SHELL_FIRST..=SHELL_LAST).contains(&id) {
                    verbs.push(verb_of(&cm, id - SHELL_FIRST));
                }
            }
        }
        eprintln!("menu: {labels:?}
verbs: {verbs:?}");
        assert_eq!(labels[0], "Quét thư mục này");
        assert!(verbs.iter().any(|v| v == "delete"), "thiếu Xoá: {verbs:?}");
        assert!(verbs.iter().any(|v| v == "properties"), "thiếu Thuộc tính: {verbs:?}");
        assert!(!verbs.iter().any(|v| v == "rename"), "Đổi tên phải bị bỏ");
        drop(menu);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn permanent_delete_removes_folder() {
        let dir = std::env::temp_dir().join(format!("fomeg-shell-del-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/x.txt"), b"x").unwrap();
        let p = dir.to_str().unwrap().to_owned();
        assert!(exists(&p));
        assert!(delete(std::slice::from_ref(&p), true).unwrap());
        assert!(!exists(&p));
        assert!(delete(std::slice::from_ref(&p), true).is_err(), "xoá đường dẫn không tồn tại phải báo lỗi");
    }
}
