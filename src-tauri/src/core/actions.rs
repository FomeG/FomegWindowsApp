use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use super::registry::{self, BACKUP_KEY};
use super::scan::refresh_service;
use super::util::log_line;
use crate::tr;
use super::{services, tasks, Item};

/// Bật/tắt một mục khởi động. Trả về item đã cập nhật.
pub fn toggle(mut it: Item, on: bool) -> Result<Item, String> {
    if it.on == on {
        return Ok(it);
    }
    match it.kind.as_str() {
        "Run" | "Folder" => registry::set_approved(&it.key1, &it.key2, on)?,
        "RegMove" => {
            if on {
                let name = if it.key3.is_empty() { format!("{}|{}", it.key1, it.key2) } else { it.key3.clone() };
                registry::move_value(BACKUP_KEY, &name, &it.key1, &it.key2)?;
                it.key3.clear();
            } else {
                let name = format!("{}|{}", it.key1, it.key2);
                registry::move_value(&it.key1, &it.key2, BACKUP_KEY, &name)?;
                it.key3 = name;
            }
        }
        "Task" => tasks::set_enabled(&it.key1, &it.key2, on)?,
        "Service" | "Svc" => {
            let mode = if on { "auto" } else { "demand" };
            services::set_mode(&it.key1, mode)?;
            log_line(&format!("[{}] {} ({}) -> {}", it.kind, it.name, it.source, if on { "BẬT" } else { "TẮT" }));
            let fresh = refresh_service(&it.key1)?;
            let (kind, id, source) = (it.kind.clone(), it.id.clone(), it.source.clone());
            let shared = it.shared;
            it = fresh;
            it.kind = kind;
            it.id = id;
            it.source = source;
            it.shared = shared;
            return Ok(it);
        }
        "Packaged" => registry::set_packaged(&it.key1, on)?,
        _ => return Err(tr!("startup.err.kind_unsupported")),
    }
    it.on = on;
    log_line(&format!("[{}] {} ({}) -> {}", it.kind, it.name, it.source, if on { "BẬT" } else { "TẮT" }));
    Ok(it)
}

pub fn service_mode(name: &str, mode: &str) -> Result<Item, String> {
    let before = services::query_one(name).map(|s| s.start_type).unwrap_or_default();
    services::set_mode(name, mode)?;
    log_line(&format!("[Dịch vụ] {name}: {before} -> {mode}"));
    refresh_service(name)
}

pub fn service_control(name: &str, start: bool) -> Result<Item, String> {
    services::control(name, start)?;
    log_line(&format!("[Dịch vụ] {} -> {}", name, if start { "CHẠY" } else { "DỪNG" }));
    refresh_service(name)
}

const DETACHED: u32 = 0x0000_0008;

fn spawn(exe: &str, args: &[&str]) {
    let _ = Command::new(exe).args(args).creation_flags(DETACHED).spawn();
}

pub fn open_location(it: &Item) {
    let fp = it.file_path.replace('/', "\\");
    if !fp.is_empty() && Path::new(&fp).exists() {
        let _ = Command::new("explorer.exe").raw_arg(format!("/select,\"{fp}\"")).spawn();
        return;
    }
    match it.kind.as_str() {
        "Task" => spawn("mmc.exe", &["taskschd.msc"]),
        "Service" | "Svc" => spawn("mmc.exe", &["services.msc"]),
        "Folder" => spawn("explorer.exe", &[&it.key3]),
        "Run" => {
            registry::set_regedit_last_key(&it.key3);
            spawn("regedit.exe", &["/m"]);
        }
        _ => {
            registry::set_regedit_last_key(&it.key1);
            spawn("regedit.exe", &["/m"]);
        }
    }
}
