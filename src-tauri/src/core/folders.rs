use windows::core::{Interface, GUID, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE, STGM_READ,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartup, FOLDERID_Startup, IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT,
};

use super::registry::{approved_on, AB};
use super::util::{from_wide_buf, wide};
use super::Item;
use crate::tr;

fn known_folder(id: &GUID) -> Option<String> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// Đọc đích + tham số của shortcut .lnk.
pub fn resolve_lnk(path: &str) -> Option<(String, String)> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = link.cast().ok()?;
        let w = wide(path);
        pf.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        let _ = link.GetPath(&mut buf, std::ptr::null_mut(), 0);
        let mut args = [0u16; 2048];
        let _ = link.GetArguments(&mut args);
        Some((from_wide_buf(&buf), from_wide_buf(&args)))
    }
}

pub fn scan(out: &mut Vec<Item>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    }
    let defs: [(&GUID, &str, &str); 2] = [
        (&FOLDERID_Startup, "HKCU", "startup.src.folder_user"),
        (&FOLDERID_CommonStartup, "HKLM", "startup.src.folder_all"),
    ];
    let ab_sub = format!(r"{AB}\StartupFolder");
    for (fid, h, label) in defs {
        let Some(dir) = known_folder(fid) else { continue };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if !ft.is_file() {
                continue;
            }
            let fname = e.file_name().to_string_lossy().into_owned();
            if fname.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let full = e.path().to_string_lossy().into_owned();
            let (mut target, mut args) = (full.clone(), String::new());
            if fname.to_ascii_lowercase().ends_with(".lnk") {
                if let Some((t, a)) = resolve_lnk(&full) {
                    if !t.is_empty() {
                        target = t;
                    }
                    args = a;
                }
            }
            let base = std::path::Path::new(&fname)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| fname.clone());
            out.push(Item {
                kind: "Folder".into(),
                name: base,
                source: tr!(label),
                command: format!("{target} {args}").trim().to_string(),
                file_path: target,
                key1: format!(r"{h}\{ab_sub}"),
                on: approved_on(h, &ab_sub, &fname),
                key2: fname,
                key3: dir.clone(),
                ..Default::default()
            });
        }
    }
}
